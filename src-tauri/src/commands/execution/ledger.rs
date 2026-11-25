//! The durable execution ledger: `<data>/execution.db` (decision 0052).
//!
//! - `executions` holds the current state of each execution (queryable).
//! - `execution_transitions` is append-only history: SQLite triggers refuse UPDATE and DELETE, so
//!   a transition can never be silently rewritten.
//! - `execution_approvals` holds approvals bound to one action and one parameter fingerprint, with
//!   an expiry, consumed at most once.
//!
//! Every status change is a compare-and-swap (`WHERE status = <expected>`) plus its transition row,
//! in one transaction: two writers racing on the same execution can't both win.
//!
//! This is not the telemetry `events` table (`events.db`) and not the ActivityBus.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: i64 = 1;

/// The ledger file in the app data folder.
pub(crate) fn default_path() -> PathBuf {
    crate::app_paths::data_dir().join("execution.db")
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecStatus {
    Requested,
    Authorized,
    Attempted,
    Executed,
    Verified,
    Failed,
    Partial,
    Cancelled,
    Unverified,
    NotAuthorized,
}

impl ExecStatus {
    pub const ALL: [ExecStatus; 10] = [
        Self::Requested,
        Self::Authorized,
        Self::Attempted,
        Self::Executed,
        Self::Verified,
        Self::Failed,
        Self::Partial,
        Self::Cancelled,
        Self::Unverified,
        Self::NotAuthorized,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "REQUESTED",
            Self::Authorized => "AUTHORIZED",
            Self::Attempted => "ATTEMPTED",
            Self::Executed => "EXECUTED",
            Self::Verified => "VERIFIED",
            Self::Failed => "FAILED",
            Self::Partial => "PARTIAL",
            Self::Cancelled => "CANCELLED",
            Self::Unverified => "UNVERIFIED",
            Self::NotAuthorized => "NOT_AUTHORIZED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// The allowed next states. Anything else is refused.
    pub fn may_become(self, next: Self) -> bool {
        use ExecStatus::*;
        matches!(
            (self, next),
            (Requested, Authorized | NotAuthorized | Cancelled)
                | (Authorized, Attempted | Cancelled)
                // A safe retry is a second ATTEMPTED row: each attempt stays in the history.
                | (Attempted, Attempted | Executed | Failed | Partial | Unverified)
                | (Executed, Verified | Failed | Unverified | Partial)
                // Later reconciliation of an uncertain outcome.
                | (Unverified, Verified | Failed)
                | (Partial, Verified | Failed)
        )
    }
}

/// One execution's current state.
#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub workflow_id: String,
    pub idempotency_key: String,
    pub created_at: String,
    pub updated_at: String,
    pub origin: String,
    pub action_id: String,
    pub tool_id: String,
    pub target_system: String,
    pub target_ref: Option<String>,
    pub action_class: String,
    pub risk: String,
    pub data_scope: String,
    /// Redacted parameters (JSON text).
    pub params_redacted: String,
    pub params_fingerprint: String,
    pub status: ExecStatus,
    pub attempts: u32,
    pub approval_id: Option<String>,
    pub expected_state: Option<String>,
    pub response: Option<String>,
    pub response_ref: Option<String>,
    pub verification_method: Option<String>,
    pub verification_result: Option<String>,
    pub error: Option<String>,
    pub recovery: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub seq: i64,
    pub execution_id: String,
    pub at: String,
    pub from_status: Option<ExecStatus>,
    pub to_status: ExecStatus,
    /// Redacted JSON text.
    pub detail: String,
}

/// One approval: which action, bound to which parameter fingerprint, when it expires, whether used.
#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRow {
    pub approval_id: String,
    pub action_id: String,
    pub params_fingerprint: String,
    pub granted_by: String,
    pub granted_at: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
    /// The execution that used it.
    pub consumed_by: Option<String>,
}

/// The fields a new execution starts with (already redacted by the controller).
pub(crate) struct NewExecution<'a> {
    pub workflow_id: &'a str,
    pub idempotency_key: &'a str,
    pub origin: &'a str,
    pub action_id: &'a str,
    pub tool_id: &'a str,
    pub target_system: &'a str,
    pub target_ref: Option<&'a str>,
    pub action_class: &'a str,
    pub risk: &'a str,
    pub data_scope: &'a str,
    pub params_redacted: &'a str,
    pub params_fingerprint: &'a str,
    pub expected_state: Option<&'a str>,
}

/// Optional field updates that ride along with a transition.
#[derive(Default, Clone, Debug)]
pub(crate) struct Fields {
    pub response: Option<String>,
    pub response_ref: Option<String>,
    pub verification_method: Option<String>,
    pub verification_result: Option<String>,
    pub error: Option<String>,
    pub recovery: Option<String>,
    pub approval_id: Option<String>,
    pub bump_attempts: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum LedgerError {
    /// The execution wasn't in the expected state (someone else moved it, or it never was).
    Conflict {
        current: Option<ExecStatus>,
    },
    /// The transition isn't in the allowed table.
    IllegalTransition(ExecStatus, ExecStatus),
    Storage(String),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { current } => write!(
                f,
                "execution state changed underneath (now {})",
                current.map(ExecStatus::as_str).unwrap_or("missing")
            ),
            Self::IllegalTransition(a, b) => {
                write!(f, "illegal transition {} -> {}", a.as_str(), b.as_str())
            }
            Self::Storage(e) => write!(f, "execution ledger: {e}"),
        }
    }
}

fn storage(e: impl std::fmt::Display) -> LedgerError {
    LedgerError::Storage(e.to_string())
}

pub(crate) struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub(crate) fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Opens (creating if needed) and brings the schema up to date. Never drops or rewrites data.
    pub(crate) fn open(&self) -> Result<Connection, LedgerError> {
        let conn = crate::commands::shared::sqlite::open_private_wal(&self.path)
            .map_err(|step| LedgerError::Storage(format!("open failed: {step:?}")))?;
        init(&conn)?;
        Ok(conn)
    }

    pub(crate) fn insert(&self, n: &NewExecution) -> Result<ExecutionRecord, LedgerError> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now();
        tx.execute(
            "INSERT INTO executions (execution_id, workflow_id, idempotency_key, created_at, updated_at,
                origin, action_id, tool_id, target_system, target_ref, action_class, risk, data_scope,
                params_redacted, params_fingerprint, status, attempts, expected_state)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'REQUESTED', 0, ?15)",
            params![
                id, n.workflow_id, n.idempotency_key, now, n.origin, n.action_id, n.tool_id,
                n.target_system, n.target_ref, n.action_class, n.risk, n.data_scope,
                n.params_redacted, n.params_fingerprint, n.expected_state
            ],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO execution_transitions (execution_id, at, from_status, to_status, detail)
             VALUES (?1, ?2, NULL, 'REQUESTED', '{}')",
            params![id, now],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.get(&id)?
            .ok_or_else(|| LedgerError::Storage("inserted row not found".into()))
    }

    /// The existing execution for an idempotency key, if any.
    pub(crate) fn by_idempotency_key(
        &self,
        key: &str,
    ) -> Result<Option<ExecutionRecord>, LedgerError> {
        let conn = self.open()?;
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM executions WHERE idempotency_key = ?1"),
            params![key],
            row_to_record,
        )
        .optional()
        .map_err(storage)
    }

    pub(crate) fn get(&self, id: &str) -> Result<Option<ExecutionRecord>, LedgerError> {
        let conn = self.open()?;
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM executions WHERE execution_id = ?1"),
            params![id],
            row_to_record,
        )
        .optional()
        .map_err(storage)
    }

    /// Executions currently in one of `statuses`, oldest first.
    pub(crate) fn in_status(
        &self,
        statuses: &[ExecStatus],
    ) -> Result<Vec<ExecutionRecord>, LedgerError> {
        let conn = self.open()?;
        let list: Vec<String> = statuses
            .iter()
            .map(|s| format!("'{}'", s.as_str()))
            .collect();
        let mut st = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM executions WHERE status IN ({}) ORDER BY created_at",
                list.join(",")
            ))
            .map_err(storage)?;
        let rows = st
            .query_map([], row_to_record)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(rows)
    }

    pub(crate) fn recent(&self, limit: u32) -> Result<Vec<ExecutionRecord>, LedgerError> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM executions ORDER BY created_at DESC LIMIT ?1"
            ))
            .map_err(storage)?;
        let rows = st
            .query_map(params![limit.min(500)], row_to_record)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(rows)
    }

    pub(crate) fn history(&self, id: &str) -> Result<Vec<Transition>, LedgerError> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(
                "SELECT seq, execution_id, at, from_status, to_status, detail
                 FROM execution_transitions WHERE execution_id = ?1 ORDER BY seq",
            )
            .map_err(storage)?;
        let rows = st
            .query_map(params![id], |r| {
                let from: Option<String> = r.get(3)?;
                let to: String = r.get(4)?;
                Ok(Transition {
                    seq: r.get(0)?,
                    execution_id: r.get(1)?,
                    at: r.get(2)?,
                    from_status: from.as_deref().and_then(ExecStatus::parse),
                    to_status: ExecStatus::parse(&to).unwrap_or(ExecStatus::Failed),
                    detail: r.get(5)?,
                })
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(rows)
    }

    /// Compare-and-swap `from` → `to`, with the transition row, in one transaction.
    pub(crate) fn transition(
        &self,
        id: &str,
        from: ExecStatus,
        to: ExecStatus,
        fields: &Fields,
        detail: &Value,
    ) -> Result<ExecutionRecord, LedgerError> {
        if !from.may_become(to) {
            return Err(LedgerError::IllegalTransition(from, to));
        }
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let now = now();
        let changed = tx
            .execute(
                "UPDATE executions SET status = ?1, updated_at = ?2,
                    response = COALESCE(?3, response),
                    response_ref = COALESCE(?4, response_ref),
                    verification_method = COALESCE(?5, verification_method),
                    verification_result = COALESCE(?6, verification_result),
                    error = COALESCE(?7, error),
                    recovery = COALESCE(?8, recovery),
                    approval_id = COALESCE(?9, approval_id),
                    attempts = attempts + ?10
                 WHERE execution_id = ?11 AND status = ?12",
                params![
                    to.as_str(),
                    now,
                    fields.response,
                    fields.response_ref,
                    fields.verification_method,
                    fields.verification_result,
                    fields.error,
                    fields.recovery,
                    fields.approval_id,
                    i64::from(fields.bump_attempts),
                    id,
                    from.as_str()
                ],
            )
            .map_err(storage)?;
        if changed != 1 {
            let current: Option<String> = tx
                .query_row(
                    "SELECT status FROM executions WHERE execution_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            return Err(LedgerError::Conflict {
                current: current.as_deref().and_then(ExecStatus::parse),
            });
        }
        tx.execute(
            "INSERT INTO execution_transitions (execution_id, at, from_status, to_status, detail)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, now, from.as_str(), to.as_str(), detail.to_string()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.get(id)?
            .ok_or_else(|| LedgerError::Storage("row vanished".into()))
    }

    // ── Approvals ──────────────────────────────────────────────────────────

    /// The most recent approvals, newest first (read-only; for the audit view).
    pub(crate) fn approvals(&self, limit: u32) -> Result<Vec<ApprovalRow>, LedgerError> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(
                "SELECT approval_id, action_id, params_fingerprint, granted_by, granted_at, expires_at,
                    consumed_at, consumed_by FROM execution_approvals ORDER BY granted_at DESC LIMIT ?1",
            )
            .map_err(storage)?;
        let rows = st
            .query_map(params![limit.min(500)], |r| {
                Ok(ApprovalRow {
                    approval_id: r.get(0)?,
                    action_id: r.get(1)?,
                    params_fingerprint: r.get(2)?,
                    granted_by: r.get(3)?,
                    granted_at: r.get(4)?,
                    expires_at: r.get(5)?,
                    consumed_at: r.get(6)?,
                    consumed_by: r.get(7)?,
                })
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(rows)
    }

    pub(crate) fn grant_approval(
        &self,
        action_id: &str,
        params_fingerprint: &str,
        granted_by: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, LedgerError> {
        let conn = self.open()?;
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO execution_approvals (approval_id, action_id, params_fingerprint, granted_by,
                granted_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                action_id,
                params_fingerprint,
                granted_by,
                now(),
                expires_at.to_rfc3339()
            ],
        )
        .map_err(storage)?;
        Ok(id)
    }

    /// Consumes an approval for exactly this action and fingerprint, once, before it expires.
    /// Returns why not, as a fixed label.
    pub(crate) fn consume_approval(
        &self,
        approval_id: &str,
        action_id: &str,
        params_fingerprint: &str,
        execution_id: &str,
        at: DateTime<Utc>,
    ) -> Result<Result<(), &'static str>, LedgerError> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let row: Option<(String, String, String, Option<String>)> = tx
            .query_row(
                "SELECT action_id, params_fingerprint, expires_at, consumed_at
                 FROM execution_approvals WHERE approval_id = ?1",
                params![approval_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(storage)?;
        let Some((a, fp, exp, consumed)) = row else {
            return Ok(Err("approval_missing"));
        };
        if a != action_id {
            return Ok(Err("approval_other_action"));
        }
        if fp != params_fingerprint {
            return Ok(Err("parameters_changed_after_approval"));
        }
        if consumed.is_some() {
            return Ok(Err("approval_already_used"));
        }
        let expired = DateTime::parse_from_rfc3339(&exp)
            .map(|e| e.with_timezone(&Utc) <= at)
            .unwrap_or(true);
        if expired {
            return Ok(Err("approval_expired"));
        }
        let n = tx
            .execute(
                "UPDATE execution_approvals SET consumed_at = ?1, consumed_by = ?2
                 WHERE approval_id = ?3 AND consumed_at IS NULL",
                params![at.to_rfc3339(), execution_id, approval_id],
            )
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(if n == 1 {
            Ok(())
        } else {
            Err("approval_already_used")
        })
    }
}

const COLUMNS: &str = "execution_id, workflow_id, idempotency_key, created_at, updated_at, origin,
    action_id, tool_id, target_system, target_ref, action_class, risk, data_scope, params_redacted,
    params_fingerprint, status, attempts, approval_id, expected_state, response, response_ref,
    verification_method, verification_result, error, recovery";

fn row_to_record(r: &rusqlite::Row) -> rusqlite::Result<ExecutionRecord> {
    let status: String = r.get(15)?;
    Ok(ExecutionRecord {
        execution_id: r.get(0)?,
        workflow_id: r.get(1)?,
        idempotency_key: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        origin: r.get(5)?,
        action_id: r.get(6)?,
        tool_id: r.get(7)?,
        target_system: r.get(8)?,
        target_ref: r.get(9)?,
        action_class: r.get(10)?,
        risk: r.get(11)?,
        data_scope: r.get(12)?,
        params_redacted: r.get(13)?,
        params_fingerprint: r.get(14)?,
        status: ExecStatus::parse(&status).unwrap_or(ExecStatus::Failed),
        attempts: r.get::<_, i64>(16)?.max(0) as u32,
        approval_id: r.get(17)?,
        expected_state: r.get(18)?,
        response: r.get(19)?,
        response_ref: r.get(20)?,
        verification_method: r.get(21)?,
        verification_result: r.get(22)?,
        error: r.get(23)?,
        recovery: r.get(24)?,
    })
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn init(conn: &Connection) -> Result<(), LedgerError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS executions (
            execution_id        TEXT PRIMARY KEY,
            workflow_id         TEXT NOT NULL,
            idempotency_key     TEXT NOT NULL UNIQUE,
            created_at          TEXT NOT NULL,
            updated_at          TEXT NOT NULL,
            origin              TEXT NOT NULL,
            action_id           TEXT NOT NULL,
            tool_id             TEXT NOT NULL,
            target_system       TEXT NOT NULL,
            target_ref          TEXT,
            action_class        TEXT NOT NULL,
            risk                TEXT NOT NULL,
            data_scope          TEXT NOT NULL,
            params_redacted     TEXT NOT NULL,
            params_fingerprint  TEXT NOT NULL,
            status              TEXT NOT NULL,
            attempts            INTEGER NOT NULL DEFAULT 0,
            approval_id         TEXT,
            expected_state      TEXT,
            response            TEXT,
            response_ref        TEXT,
            verification_method TEXT,
            verification_result TEXT,
            error               TEXT,
            recovery            TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_executions_status ON executions(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_executions_workflow ON executions(workflow_id, created_at);
        CREATE TABLE IF NOT EXISTS execution_transitions (
            seq          INTEGER PRIMARY KEY AUTOINCREMENT,
            execution_id TEXT NOT NULL REFERENCES executions(execution_id),
            at           TEXT NOT NULL,
            from_status  TEXT,
            to_status    TEXT NOT NULL,
            detail       TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX IF NOT EXISTS idx_transitions_execution ON execution_transitions(execution_id, seq);
        CREATE TRIGGER IF NOT EXISTS execution_transitions_append_only_update
            BEFORE UPDATE ON execution_transitions
            BEGIN SELECT RAISE(ABORT, 'execution_transitions is append-only'); END;
        CREATE TRIGGER IF NOT EXISTS execution_transitions_append_only_delete
            BEFORE DELETE ON execution_transitions
            BEGIN SELECT RAISE(ABORT, 'execution_transitions is append-only'); END;
        CREATE TABLE IF NOT EXISTS execution_approvals (
            approval_id        TEXT PRIMARY KEY,
            action_id          TEXT NOT NULL,
            params_fingerprint TEXT NOT NULL,
            granted_by         TEXT NOT NULL,
            granted_at         TEXT NOT NULL,
            expires_at         TEXT NOT NULL,
            consumed_at        TEXT,
            consumed_by        TEXT
        );
        ",
    )
    .map_err(storage)?;
    let found: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    match found.and_then(|v| v.parse::<i64>().ok()) {
        None => {
            conn.execute(
                "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )
            .map_err(storage)?;
        }
        // A newer app wrote this file: refuse to touch it rather than guess.
        Some(v) if v > SCHEMA_VERSION => {
            return Err(LedgerError::Storage(format!(
                "execution.db schema {v} is newer than this app ({SCHEMA_VERSION})"
            )))
        }
        Some(_) => {}
    }
    Ok(())
}
