//! Activity history, recommendations, evidence and outcomes (decision 0053, approved 2026-11-25).
//!
//! Two physical files, one per isolation boundary, same schema:
//! - `<data>/amara.db`: scopes `personal`, `neural`, `ops`;
//! - `<data>/business/amara_business.db`: scope `business`.
//!
//! Existing stores stay the sources of truth. These tables hold what didn't exist: activity
//! history (C), recommendations (E), their outcomes (F), and evidence that points at exact records
//! and time windows through [`RecordRef`] strings (no copies of telemetry or personal values).
//!
//! Rules enforced here (permission model §6, confirmed by Taisun):
//! - Business never mixes with personal or neural, in either direction (and lives in its own file).
//! - A recommendation (other than a `take`) needs at least one `observation` evidence row.
//! - `confidence` needs a `confidence_basis`.
//! - `activity` and `recommendation_outcome` are append-only (UPDATE refused by triggers).
//! - Forgetting a record removes the activity and evidence that point at it.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: i64 = 1;
const MAX_DATA_JSON: usize = 4 * 1024;
const MAX_TEXT: usize = 2_000;

// ─── Scopes and record references ────────────────────────────────────────────

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Personal,
    Business,
    Neural,
    Ops,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Business => "business",
            Self::Neural => "neural",
            Self::Ops => "ops",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Personal, Self::Business, Self::Neural, Self::Ops]
            .into_iter()
            .find(|x| x.as_str() == s)
    }
}

/// May evidence in scope `ev` support a recommendation in scope `rec`?
pub(crate) fn evidence_allowed(rec: Scope, ev: Scope) -> bool {
    use Scope::*;
    match rec {
        Business => ev == Business,
        Personal | Neural => matches!(ev, Personal | Neural),
        Ops => ev == Ops,
    }
}

/// The stores a reference may name, and the scope each belongs to.
const STORES: &[(&str, Scope)] = &[
    ("goals", Scope::Personal),
    ("habit_day", Scope::Personal),
    ("roadmap", Scope::Personal),
    ("checkin", Scope::Personal),
    ("money", Scope::Personal),
    ("date", Scope::Personal),
    ("project", Scope::Personal),
    ("action", Scope::Personal),
    ("decision", Scope::Personal),
    ("context", Scope::Personal),
    ("chat", Scope::Personal),
    ("noop", Scope::Personal),
    ("daily", Scope::Personal),
    ("pattern", Scope::Personal),
    ("review", Scope::Personal),
    ("take", Scope::Personal),
    ("replay", Scope::Neural),
    ("session", Scope::Neural),
    ("hemisync", Scope::Neural),
    ("ledger", Scope::Business),
    ("treasury", Scope::Business),
    ("arm", Scope::Business),
    ("execution", Scope::Ops),
    ("job", Scope::Ops),
    ("activity", Scope::Ops),
    ("recommendation", Scope::Ops),
];

/// `<store>:<record-id>[@<version>][#<window>]` (spec §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordRef {
    pub store: String,
    pub id: String,
    pub version: Option<String>,
    pub window: Option<String>,
    pub scope: Scope,
}

impl RecordRef {
    pub fn parse(s: &str) -> Result<Self, String> {
        if s.len() > 300 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err("record ref: bad characters or too long".into());
        }
        let (store, rest) = s.split_once(':').ok_or("record ref: no store")?;
        let scope = STORES
            .iter()
            .find(|(n, _)| *n == store)
            .map(|(_, sc)| *sc)
            .ok_or_else(|| format!("record ref: unknown store {store}"))?;
        let (rest, window) = match rest.split_once('#') {
            Some((r, w)) if !w.is_empty() => (r, Some(w.to_string())),
            Some(_) => return Err("record ref: empty window".into()),
            None => (rest, None),
        };
        let (id, version) = match rest.split_once('@') {
            Some((i, v)) if !v.is_empty() => (i, Some(v.to_string())),
            Some(_) => return Err("record ref: empty version".into()),
            None => (rest, None),
        };
        if id.is_empty() {
            return Err("record ref: empty id".into());
        }
        Ok(Self {
            store: store.into(),
            id: id.into(),
            version,
            window,
            scope,
        })
    }
}

// ─── Vocabularies ────────────────────────────────────────────────────────────

const ACTIVITY_KINDS: &[&str] = &[
    "habit.completed",
    "habit.missed",
    "goal.created",
    "goal.progressed",
    "goal.milestone_reached",
    "goal.completed",
    "project.updated",
    "checkin.logged",
    "workout.finished",
    "neuro.session_finished",
    "date.done",
    "money.logged",
    "business.action_done",
];
const ACTIVITY_SOURCES: &[&str] = &["user", "model", "scheduler", "import", "backfill", "noop"];
const ACTORS: &[&str] = &["taisun", "amara", "system"];
const EVIDENCE_ROLES: &[&str] = &["observation", "calculation", "hypothesis"];
const EVIDENCE_QUALITY: &[&str] = &["measured", "estimate", "self_report", "partial", "missing"];
const DOMAINS: &[&str] = &["life", "fitness", "neuro", "money", "enterprise", "cross"];

/// Recommendation status machine; an outcome moves it along.
fn next_status(current: &str, outcome: &str) -> Option<&'static str> {
    Some(match (current, outcome) {
        ("proposed", "shown") => "shown",
        ("proposed" | "shown", "accepted") => "accepted",
        ("proposed" | "shown", "modified") => "modified",
        ("proposed" | "shown", "dismissed") => "dismissed",
        ("proposed" | "shown" | "accepted" | "modified", "expired") => "expired",
        ("accepted" | "modified", "completed") => "completed",
        ("completed" | "dismissed" | "expired", "evaluated") => "evaluated",
        _ => return None,
    })
}

fn check(list: &[&str], v: &str, what: &str) -> Result<(), String> {
    list.contains(&v)
        .then_some(())
        .ok_or_else(|| format!("{what}: {v} is not allowed"))
}

fn capped(s: &str, what: &str) -> Result<(), String> {
    (s.chars().count() <= MAX_TEXT)
        .then_some(())
        .ok_or_else(|| format!("{what}: too long"))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

// ─── Store ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// `amara.db`: personal, neural, ops.
    Personal,
    /// `business/amara_business.db`: business.
    Business,
}

impl Boundary {
    fn allows(self, s: Scope) -> bool {
        match self {
            Self::Personal => matches!(s, Scope::Personal | Scope::Neural | Scope::Ops),
            Self::Business => s == Scope::Business,
        }
    }
    fn scope_check_sql(self) -> &'static str {
        match self {
            Self::Personal => "scope IN ('personal','neural','ops')",
            Self::Business => "scope = 'business'",
        }
    }
}

pub(crate) struct Store {
    path: PathBuf,
    boundary: Boundary,
}

/// The file for a scope's boundary.
pub(crate) fn store_for(data: &Path, scope: Scope) -> Store {
    if scope == Scope::Business {
        Store::at(
            data.join("business").join("amara_business.db"),
            Boundary::Business,
        )
    } else {
        Store::at(data.join("amara.db"), Boundary::Personal)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    Inserted(String),
    /// The same activity was recorded before (idempotent replay).
    Duplicate(String),
}

pub(crate) struct NewActivity<'a> {
    pub scope: Scope,
    pub kind: &'a str,
    pub subject_ref: &'a str,
    pub occurred_at: &'a str,
    pub source: &'a str,
    pub source_ref: Option<&'a str>,
    pub actor: &'a str,
    pub summary: Option<&'a str>,
    pub data_json: Option<&'a str>,
}

pub(crate) struct NewRecommendation<'a> {
    pub scope: Scope,
    pub domain: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub expires_at: Option<&'a str>,
    pub produced_by: &'a str,
    pub model_tier: Option<&'a str>,
    pub confidence: Option<f64>,
    pub confidence_basis: Option<&'a str>,
    pub target_ref: Option<&'a str>,
    pub supersedes_id: Option<&'a str>,
}

pub(crate) struct NewEvidence<'a> {
    pub record_ref: &'a str,
    pub window_start: Option<&'a str>,
    pub window_end: Option<&'a str>,
    pub role: &'a str,
    pub quality: &'a str,
    pub note: Option<&'a str>,
    pub snapshot_sha256: Option<&'a str>,
}

impl Store {
    pub(crate) fn at(path: impl Into<PathBuf>, boundary: Boundary) -> Self {
        Self {
            path: path.into(),
            boundary,
        }
    }

    /// Opens (creating if needed). Idempotent; a newer schema on disk is refused, never repaired.
    pub(crate) fn open(&self) -> Result<Connection, String> {
        let conn = super::shared::sqlite::open_private_wal(&self.path)
            .map_err(|s| format!("amara store: open failed: {s:?}"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| format!("amara store: {e}"))?;
        init(&conn, self.boundary)?;
        Ok(conn)
    }

    fn scope_ok(&self, s: Scope) -> Result<(), String> {
        self.boundary
            .allows(s)
            .then_some(())
            .ok_or_else(|| format!("scope {} does not belong in this file", s.as_str()))
    }

    pub(crate) fn record_activity(&self, a: &NewActivity) -> Result<Recorded, String> {
        self.scope_ok(a.scope)?;
        check(ACTIVITY_KINDS, a.kind, "activity kind")?;
        check(ACTIVITY_SOURCES, a.source, "activity source")?;
        check(ACTORS, a.actor, "actor")?;
        let subject = RecordRef::parse(a.subject_ref)?;
        if !self.boundary.allows(subject.scope) {
            return Err("activity subject is in another boundary".into());
        }
        if let Some(r) = a.source_ref {
            RecordRef::parse(r)?;
        }
        chrono::DateTime::parse_from_rfc3339(a.occurred_at)
            .map_err(|_| "occurred_at must be RFC 3339".to_string())?;
        if let Some(s) = a.summary {
            capped(s, "summary")?;
        }
        if let Some(d) = a.data_json {
            if d.len() > MAX_DATA_JSON {
                return Err("data_json is over 4 KB".into());
            }
            serde_json::from_str::<serde_json::Value>(d)
                .map_err(|_| "data_json is not JSON".to_string())?;
        }
        let conn = self.open()?;
        let id = uuid::Uuid::new_v4().to_string();
        let n = conn
            .execute(
                "INSERT OR IGNORE INTO activity (activity_id, scope, kind, subject_ref, occurred_at,
                    recorded_at, source, source_ref, actor, summary, data_json, schema_version)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    id, a.scope.as_str(), a.kind, a.subject_ref, a.occurred_at, now(), a.source,
                    a.source_ref, a.actor, a.summary, a.data_json, SCHEMA_VERSION
                ],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        if n == 1 {
            return Ok(Recorded::Inserted(id));
        }
        let existing: String = conn
            .query_row(
                "SELECT activity_id FROM activity WHERE kind=?1 AND subject_ref=?2 AND occurred_at=?3 AND source=?4",
                params![a.kind, a.subject_ref, a.occurred_at, a.source],
                |r| r.get(0),
            )
            .map_err(|e| format!("amara store: {e}"))?;
        Ok(Recorded::Duplicate(existing))
    }

    /// One recommendation and its evidence, in one transaction, or nothing.
    pub(crate) fn create_recommendation(
        &self,
        r: &NewRecommendation,
        evidence: &[NewEvidence],
    ) -> Result<String, String> {
        self.scope_ok(r.scope)?;
        check(DOMAINS, r.domain, "domain")?;
        capped(r.title, "title")?;
        capped(r.body, "body")?;
        if r.title.trim().is_empty() || r.kind.trim().is_empty() || r.produced_by.trim().is_empty()
        {
            return Err("recommendation needs a kind, title and producer".into());
        }
        match (r.confidence, r.confidence_basis) {
            (Some(c), Some(b)) if (0.0..=1.0).contains(&c) && !b.trim().is_empty() => {}
            (Some(_), _) => return Err("confidence needs a basis and must be 0..1".into()),
            (None, _) => {}
        }
        if let Some(t) = r.target_ref {
            let t = RecordRef::parse(t)?;
            if !evidence_allowed(r.scope, t.scope) {
                return Err("target is in a scope this recommendation can't touch".into());
            }
        }
        let mut observations = 0;
        for e in evidence {
            let rr = RecordRef::parse(e.record_ref)?;
            if !evidence_allowed(r.scope, rr.scope) {
                return Err(format!(
                    "evidence {} ({}) can't support a {} recommendation",
                    rr.store,
                    rr.scope.as_str(),
                    r.scope.as_str()
                ));
            }
            check(EVIDENCE_ROLES, e.role, "evidence role")?;
            check(EVIDENCE_QUALITY, e.quality, "evidence quality")?;
            if matches!(rr.store.as_str(), "replay" | "noop" | "daily") && e.window_start.is_none()
            {
                return Err(format!("evidence from {} needs its time window", rr.store));
            }
            if e.role == "observation" {
                observations += 1;
            }
        }
        if r.kind != "take" && observations == 0 {
            return Err("a recommendation needs at least one observation as evidence".into());
        }
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("amara store: {e}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let t = now();
        tx.execute(
            "INSERT INTO recommendation (recommendation_id, scope, domain, kind, title, body, status,
                created_at, updated_at, expires_at, produced_by, model_tier, confidence,
                confidence_basis, target_ref, supersedes_id, schema_version)
             VALUES (?1,?2,?3,?4,?5,?6,'proposed',?7,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                id, r.scope.as_str(), r.domain, r.kind, r.title, r.body, t, r.expires_at,
                r.produced_by, r.model_tier, r.confidence, r.confidence_basis, r.target_ref,
                r.supersedes_id, SCHEMA_VERSION
            ],
        )
        .map_err(|e| format!("amara store: {e}"))?;
        for e in evidence {
            tx.execute(
                "INSERT INTO recommendation_evidence (evidence_id, recommendation_id, record_ref,
                    window_start, window_end, role, quality, note, captured_at, snapshot_sha256)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    id,
                    e.record_ref,
                    e.window_start,
                    e.window_end,
                    e.role,
                    e.quality,
                    e.note,
                    t,
                    e.snapshot_sha256
                ],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        }
        tx.commit().map_err(|e| format!("amara store: {e}"))?;
        Ok(id)
    }

    /// Appends an outcome and moves the status, compare-and-swap, in one transaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_outcome(
        &self,
        recommendation_id: &str,
        outcome: &str,
        actor: &str,
        modification_json: Option<&str>,
        result_ref: Option<&str>,
        evaluation: Option<&str>,
        note: Option<&str>,
    ) -> Result<String, String> {
        check(ACTORS, actor, "actor")?;
        if let Some(r) = result_ref {
            RecordRef::parse(r)?;
        }
        if outcome == "evaluated" {
            check(
                &["helped", "no_effect", "worse", "unknown"],
                evaluation.unwrap_or(""),
                "evaluation",
            )?;
        }
        if let Some(m) = modification_json {
            serde_json::from_str::<serde_json::Value>(m)
                .map_err(|_| "modification is not JSON".to_string())?;
        }
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("amara store: {e}"))?;
        let current: Option<String> = tx
            .query_row(
                "SELECT status FROM recommendation WHERE recommendation_id=?1",
                params![recommendation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| format!("amara store: {e}"))?;
        let current = current.ok_or("recommendation not found")?;
        let next = next_status(&current, outcome)
            .ok_or_else(|| format!("can't go from {current} to {outcome}"))?;
        let t = now();
        let changed = tx
            .execute(
                "UPDATE recommendation SET status=?1, updated_at=?2 WHERE recommendation_id=?3 AND status=?4",
                params![next, t, recommendation_id, current],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        if changed != 1 {
            return Err("changed: the recommendation moved meanwhile".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO recommendation_outcome (outcome_id, recommendation_id, outcome, at, actor,
                modification_json, result_ref, evaluation, note) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, recommendation_id, outcome, t, actor, modification_json, result_ref, evaluation, note],
        )
        .map_err(|e| format!("amara store: {e}"))?;
        tx.commit().map_err(|e| format!("amara store: {e}"))?;
        Ok(id)
    }

    /// "Forget this": removes activity about the record and evidence pointing at it. A recommendation
    /// left with no observation is expired with note `evidence_deleted`. Returns rows removed.
    pub(crate) fn forget(&self, record_ref: &str) -> Result<usize, String> {
        RecordRef::parse(record_ref)?;
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("amara store: {e}"))?;
        let mut touched: Vec<(String, String)> = {
            let mut st = tx
                .prepare(
                    "SELECT DISTINCT r.recommendation_id, r.status FROM recommendation r
                     JOIN recommendation_evidence e ON e.recommendation_id = r.recommendation_id
                     WHERE e.record_ref = ?1",
                )
                .map_err(|e| format!("amara store: {e}"))?;
            let rows = st
                .query_map(params![record_ref], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| format!("amara store: {e}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("amara store: {e}"))?;
            rows
        };
        let a = tx
            .execute(
                "DELETE FROM activity WHERE subject_ref = ?1",
                params![record_ref],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        let e = tx
            .execute(
                "DELETE FROM recommendation_evidence WHERE record_ref = ?1",
                params![record_ref],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        let t = now();
        for (rid, status) in touched.drain(..) {
            let left: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM recommendation_evidence WHERE recommendation_id=?1 AND role='observation'",
                    params![rid],
                    |r| r.get(0),
                )
                .map_err(|e| format!("amara store: {e}"))?;
            if left == 0 {
                if let Some(next) = next_status(&status, "expired") {
                    tx.execute(
                        "UPDATE recommendation SET status=?1, updated_at=?2 WHERE recommendation_id=?3",
                        params![next, t, rid],
                    )
                    .map_err(|e| format!("amara store: {e}"))?;
                    tx.execute(
                        "INSERT INTO recommendation_outcome (outcome_id, recommendation_id, outcome, at, actor, note)
                         VALUES (?1,?2,'expired',?3,'system','evidence_deleted')",
                        params![uuid::Uuid::new_v4().to_string(), rid, t],
                    )
                    .map_err(|e| format!("amara store: {e}"))?;
                }
            }
        }
        tx.commit().map_err(|e| format!("amara store: {e}"))?;
        Ok(a + e)
    }
}

fn init(conn: &Connection, b: Boundary) -> Result<(), String> {
    let scope = b.scope_check_sql();
    conn.execute_batch(&format!(
        "
        CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS activity (
            activity_id    TEXT PRIMARY KEY,
            scope          TEXT NOT NULL CHECK ({scope}),
            kind           TEXT NOT NULL,
            subject_ref    TEXT NOT NULL,
            occurred_at    TEXT NOT NULL,
            recorded_at    TEXT NOT NULL,
            source         TEXT NOT NULL,
            source_ref     TEXT,
            actor          TEXT NOT NULL,
            summary        TEXT,
            data_json      TEXT,
            schema_version INTEGER NOT NULL,
            UNIQUE (kind, subject_ref, occurred_at, source)
        );
        CREATE INDEX IF NOT EXISTS idx_activity_subject ON activity(subject_ref);
        CREATE INDEX IF NOT EXISTS idx_activity_time ON activity(scope, occurred_at);
        CREATE TRIGGER IF NOT EXISTS activity_append_only BEFORE UPDATE ON activity
            BEGIN SELECT RAISE(ABORT, 'activity is append-only'); END;
        CREATE TABLE IF NOT EXISTS recommendation (
            recommendation_id TEXT PRIMARY KEY,
            scope             TEXT NOT NULL CHECK ({scope}),
            domain            TEXT NOT NULL,
            kind              TEXT NOT NULL,
            title             TEXT NOT NULL,
            body              TEXT NOT NULL,
            status            TEXT NOT NULL,
            created_at        TEXT NOT NULL,
            updated_at        TEXT NOT NULL,
            expires_at        TEXT,
            produced_by       TEXT NOT NULL,
            model_tier        TEXT,
            confidence        REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
            confidence_basis  TEXT,
            target_ref        TEXT,
            supersedes_id     TEXT REFERENCES recommendation(recommendation_id),
            schema_version    INTEGER NOT NULL,
            CHECK (confidence IS NULL OR confidence_basis IS NOT NULL)
        );
        CREATE INDEX IF NOT EXISTS idx_recommendation_status ON recommendation(status, created_at);
        CREATE TABLE IF NOT EXISTS recommendation_evidence (
            evidence_id       TEXT PRIMARY KEY,
            recommendation_id TEXT NOT NULL REFERENCES recommendation(recommendation_id) ON DELETE CASCADE,
            record_ref        TEXT NOT NULL,
            window_start      TEXT,
            window_end        TEXT,
            role              TEXT NOT NULL CHECK (role IN ('observation','calculation','hypothesis')),
            quality           TEXT NOT NULL CHECK (quality IN ('measured','estimate','self_report','partial','missing')),
            note              TEXT,
            captured_at       TEXT NOT NULL,
            snapshot_sha256   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_evidence_rec ON recommendation_evidence(recommendation_id);
        CREATE INDEX IF NOT EXISTS idx_evidence_ref ON recommendation_evidence(record_ref);
        CREATE TABLE IF NOT EXISTS recommendation_outcome (
            outcome_id        TEXT PRIMARY KEY,
            recommendation_id TEXT NOT NULL REFERENCES recommendation(recommendation_id) ON DELETE CASCADE,
            outcome           TEXT NOT NULL,
            at                TEXT NOT NULL,
            actor             TEXT NOT NULL,
            modification_json TEXT,
            result_ref        TEXT,
            evaluation        TEXT,
            note              TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_outcome_rec ON recommendation_outcome(recommendation_id, at);
        CREATE TRIGGER IF NOT EXISTS outcome_append_only BEFORE UPDATE ON recommendation_outcome
            BEGIN SELECT RAISE(ABORT, 'recommendation_outcome is append-only'); END;
        "
    ))
    .map_err(|e| format!("amara store: {e}"))?;
    let found: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("amara store: {e}"))?;
    match found.and_then(|v| v.parse::<i64>().ok()) {
        None => {
            conn.execute(
                "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )
            .map_err(|e| format!("amara store: {e}"))?;
        }
        Some(v) if v > SCHEMA_VERSION => {
            return Err(format!(
                "amara store schema {v} is newer than this app ({SCHEMA_VERSION}); not touched"
            ))
        }
        Some(_) => {}
    }
    Ok(())
}

/// 0053 step 1: create both stores at startup (idempotent) and log the first creation in
/// `<data>/migrations/log.jsonl`. Touches no existing file. Errors are logged, never fatal.
pub(crate) fn init_on_startup(data: &Path) {
    for scope in [Scope::Personal, Scope::Business] {
        let store = store_for(data, scope);
        let existed = store.path.exists();
        match store.open() {
            Ok(_) if !existed => log_migration(data, &store.path, "created schema v1"),
            Ok(_) => {}
            Err(e) => eprintln!("[amara-store] {e}"),
        }
    }
}

fn log_migration(data: &Path, file: &Path, what: &str) {
    let line = serde_json::json!({
        "at": now(),
        "decision": "0053",
        "step": 1,
        "file": file.strip_prefix(data).unwrap_or(file).to_string_lossy(),
        "result": what,
        "schema_version": SCHEMA_VERSION,
    })
    .to_string();
    let path = data.join("migrations").join("log.jsonl");
    let ok = crate::app_paths::create_private_dir_all(path.parent().unwrap_or(data)).is_ok()
        && crate::app_paths::private_open_options()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            })
            .is_ok();
    if !ok {
        eprintln!("[amara-store] could not write the migration log");
    }
}

#[cfg(test)]
mod tests;
