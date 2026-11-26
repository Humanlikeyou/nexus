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

/// v1: the four tables (0053). v2 (foundations phase): recommendation observation, rationale,
/// suggested action, uncertainty, impact and effort; activity certainty. v3 (0063, Enterprise):
/// `business_id` on activity and recommendation, derived from a `biz:` reference and allowed only
/// on business rows. Additive only.
pub(crate) const SCHEMA_VERSION: i64 = 3;
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
    ("history", Scope::Personal),
    ("area", Scope::Personal),
    // 0062: Workout engine sessions (`workout:<session id>`).
    ("workout", Scope::Personal),
    ("replay", Scope::Neural),
    ("session", Scope::Neural),
    ("hemisync", Scope::Neural),
    ("ledger", Scope::Business),
    ("treasury", Scope::Business),
    ("arm", Scope::Business),
    // 0063: business workspace records, `biz:<business id>/<kind>/<id>` (business_workspace.rs).
    ("biz", Scope::Business),
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

/// The business a reference belongs to: `biz:<business id>/...` → the id. Any other store → None.
pub(crate) fn business_of(r: &RecordRef) -> Option<&str> {
    if r.store != "biz" {
        return None;
    }
    let id = r.id.split('/').next().unwrap_or("");
    (!id.is_empty()).then_some(id)
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
    "goal.status_changed",
    "link.added",
    "link.removed",
    "area.updated",
    "week.planned",
    // 0063: business workspaces. Every one has a `biz:` subject, so it carries its business id.
    "business.created",
    "business.updated",
    "business.context_changed",
    "business.context_approved",
    "business.memory_changed",
    "business.document_added",
    "business.document_changed",
    "business.connector_changed",
    "business.sync_finished",
    "business.permission_changed",
    "business.proposal_decided",
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
        // Rated without acting on it (a take), and a later re-rating.
        ("proposed" | "shown" | "evaluated", "evaluated") => "evaluated",
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
    /// `confirmed` (it happened as recorded) or `estimated` (e.g. a day-precision backfill).
    pub certainty: &'a str,
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
    /// A fixed id (takes use `take:<take id>`); `None` = a new UUID.
    pub id: Option<&'a str>,
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
    /// What was observed (plain statement of the facts the evidence shows).
    pub observation: Option<&'a str>,
    /// Why it matters.
    pub rationale: Option<&'a str>,
    /// What Taisun could do.
    pub suggested_action: Option<&'a str>,
    /// What is not known or could be wrong.
    pub uncertainty: Option<&'a str>,
    /// `low` | `medium` | `high`.
    pub impact: Option<&'a str>,
    /// `low` | `medium` | `high`.
    pub effort: Option<&'a str>,
}

pub(crate) struct NewEvidence<'a> {
    pub record_ref: &'a str,
    pub window_start: Option<&'a str>,
    pub window_end: Option<&'a str>,
    pub role: &'a str,
    pub quality: &'a str,
    pub note: Option<&'a str>,
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
        init(&conn, self.boundary, &self.path)?;
        Ok(conn)
    }

    pub(crate) fn boundary(&self) -> Boundary {
        self.boundary
    }

    /// The app data folder this store sits in (the business file is one level down).
    pub(crate) fn data_dir(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        match self.boundary {
            Boundary::Personal => parent.to_path_buf(),
            Boundary::Business => parent.parent().unwrap_or(parent).to_path_buf(),
        }
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
        check(&["confirmed", "estimated"], a.certainty, "certainty")?;
        check(ACTORS, a.actor, "actor")?;
        let subject = RecordRef::parse(a.subject_ref)?;
        if !self.boundary.allows(subject.scope) {
            return Err("activity subject is in another boundary".into());
        }
        // 0063: a workspace record's activity is tied to its business; nothing else carries one.
        let business_id = business_of(&subject).map(str::to_string);
        if subject.store == "biz" && business_id.is_none() {
            return Err("activity subject names no business".into());
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
                    recorded_at, source, source_ref, actor, summary, data_json, schema_version, certainty,
                    business_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    id, a.scope.as_str(), a.kind, a.subject_ref, a.occurred_at, now(), a.source,
                    a.source_ref, a.actor, a.summary, a.data_json, SCHEMA_VERSION, a.certainty,
                    business_id
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
        let mut business_id: Option<String> = None;
        if let Some(t) = r.target_ref {
            let t = RecordRef::parse(t)?;
            if !evidence_allowed(r.scope, t.scope) {
                return Err("target is in a scope this recommendation can't touch".into());
            }
            business_id = business_of(&t).map(str::to_string);
        }
        for (v, what) in [(r.impact, "impact"), (r.effort, "effort")] {
            if let Some(v) = v {
                check(&["low", "medium", "high"], v, what)?;
            }
        }
        for t in [
            r.observation,
            r.rationale,
            r.suggested_action,
            r.uncertainty,
        ]
        .into_iter()
        .flatten()
        {
            capped(t, "recommendation text")?;
        }
        // Evidence must point at records that exist. A model-written recommendation may cite only
        // records the resolver found; rules and the user may also cite stores it can't check
        // (NOOP, derived rows), but never a record that is missing.
        let by_model = r.produced_by.starts_with("model:");
        let data = self.data_dir();
        let mut shas: Vec<Option<String>> = Vec::with_capacity(evidence.len());
        let mut observations = 0;
        for e in evidence {
            let rr = RecordRef::parse(e.record_ref)?;
            // 0063: a business recommendation's evidence stays inside its own business.
            if let (Some(b), Some(eb)) = (business_id.as_deref(), business_of(&rr)) {
                if b != eb {
                    return Err(format!(
                        "evidence {} belongs to another business",
                        e.record_ref
                    ));
                }
            }
            match resolve::resolve(&data, &rr) {
                resolve::Resolution::Found { sha256 } => shas.push(Some(sha256)),
                resolve::Resolution::Missing => {
                    return Err(format!("evidence {}: no such record", e.record_ref))
                }
                resolve::Resolution::Unverifiable if by_model => {
                    return Err(format!(
                        "evidence {}: can't be checked, so a model may not cite it",
                        e.record_ref
                    ))
                }
                resolve::Resolution::Unverifiable => shas.push(None),
            }
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
        let id =
            r.id.map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let t = now();
        tx.execute(
            "INSERT INTO recommendation (recommendation_id, scope, domain, kind, title, body, status,
                created_at, updated_at, expires_at, produced_by, model_tier, confidence,
                confidence_basis, target_ref, supersedes_id, schema_version, observation, rationale,
                suggested_action, uncertainty, impact, effort, business_id)
             VALUES (?1,?2,?3,?4,?5,?6,'proposed',?7,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
            params![
                id, r.scope.as_str(), r.domain, r.kind, r.title, r.body, t, r.expires_at,
                r.produced_by, r.model_tier, r.confidence, r.confidence_basis, r.target_ref,
                r.supersedes_id, SCHEMA_VERSION, r.observation, r.rationale, r.suggested_action,
                r.uncertainty, r.impact, r.effort, business_id
            ],
        )
        .map_err(|e| format!("amara store: {e}"))?;
        for (e, sha) in evidence.iter().zip(shas) {
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
                    // The resolver's hash of the record as read; never the caller's.
                    sha
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

    /// Migration backups and rollback copies of this store (`<file>.v<n>-*.bak`, `<file>.replaced-*`).
    pub(crate) fn backup_copies(&self) -> Vec<PathBuf> {
        let Some(name) = self.path.file_name().and_then(|n| n.to_str()) else {
            return vec![];
        };
        let Some(dir) = self.path.parent() else {
            return vec![];
        };
        std::fs::read_dir(dir)
            .map(|it| {
                it.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                            (n.starts_with(&format!("{name}.v")) && n.ends_with(".bak"))
                                || n.starts_with(&format!("{name}.replaced-"))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Removes backup copies older than `days` (they would otherwise keep data forever).
    pub(crate) fn prune_backups(&self, days: u64) -> usize {
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
        self.backup_copies()
            .into_iter()
            .filter(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < cutoff)
            })
            .filter(|p| std::fs::remove_file(p).is_ok())
            .count()
    }

    /// "Forget this": removes activity about the record and evidence pointing at it. A recommendation
    /// left with no observation is expired with note `evidence_deleted`. Returns rows removed.
    pub(crate) fn forget(&self, record_ref: &str) -> Result<usize, String> {
        let n = self.forget_in_file(record_ref, None)?;
        // Backups must not keep what was forgotten.
        for copy in self.backup_copies() {
            if let Err(e) = self.forget_in_file(record_ref, Some(&copy)) {
                eprintln!("[amara-store] forget in a backup copy failed: {e}");
            }
        }
        Ok(n)
    }

    fn forget_in_file(&self, record_ref: &str, copy: Option<&Path>) -> Result<usize, String> {
        RecordRef::parse(record_ref)?;
        let mut conn = match copy {
            None => self.open()?,
            // A backup is opened as it is (never migrated); an older schema has the same columns used here.
            Some(p) => Connection::open(p).map_err(|e| format!("amara store: {e}"))?,
        };
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

/// The domain owners' hook (step 4): records one activity after the owner's own write succeeded.
/// Best effort by design: a failure here is logged and never undoes or fails the user's change.
pub(crate) fn note(data: &Path, a: NewActivity) {
    if let Err(e) = store_for(data, a.scope).record_activity(&a) {
        eprintln!("[amara-store] activity {} not recorded: {e}", a.kind);
    }
}

/// A forgotten take: its recommendation goes, with its outcomes (cascade). Logged on failure.
pub(crate) fn forget_take(data: &Path, take_id: &str, page: &str) {
    let (scope, _) = take_scope(page);
    let r = store_for(data, scope).open().and_then(|c| {
        c.execute(
            "DELETE FROM recommendation WHERE recommendation_id = ?1",
            params![format!("take:{take_id}")],
        )
        .map_err(|e| e.to_string())
    });
    if let Err(e) = r {
        eprintln!("[amara-store] forgotten take not removed: {e}");
    }
}

/// A deleted domain record: the activity about it and the evidence pointing at it go too.
pub(crate) fn forget_record(data: &Path, record_ref: &str) {
    for scope in [Scope::Personal, Scope::Business] {
        if let Err(e) = store_for(data, scope).forget(record_ref) {
            eprintln!("[amara-store] forget {record_ref}: {e}");
        }
    }
}

/// Which file and domain a take on `page` belongs to.
pub(crate) fn take_scope(page: &str) -> (Scope, &'static str) {
    match page {
        "business" => (Scope::Business, "enterprise"),
        "neuro" => (Scope::Neural, "neuro"),
        "fitness" | "mindBody" => (Scope::Personal, "fitness"),
        _ => (Scope::Personal, "life"),
    }
}

/// A new take becomes a `take` recommendation (`take:<id>`), alongside `take_log.jsonl`.
pub(crate) fn note_take(
    data: &Path,
    take_id: &str,
    page: &str,
    kind: &str,
    text: &str,
    tier: &str,
) {
    let (scope, domain) = take_scope(page);
    let rid = format!("take:{take_id}");
    let title: String = text.chars().take(120).collect();
    let produced = format!("model:take.{kind}");
    let tier = matches!(tier, "light" | "deep" | "claude").then_some(tier);
    let r = store_for(data, scope).create_recommendation(
        &NewRecommendation {
            observation: None,
            rationale: None,
            suggested_action: None,
            uncertainty: None,
            impact: None,
            effort: None,
            id: Some(&rid),
            scope,
            domain,
            kind: "take",
            title: if title.trim().is_empty() {
                "Take"
            } else {
                &title
            },
            body: text,
            expires_at: None,
            produced_by: &produced,
            model_tier: tier,
            confidence: None,
            confidence_basis: None,
            target_ref: None,
            supersedes_id: None,
        },
        &[],
    );
    if let Err(e) = r {
        eprintln!("[amara-store] take recommendation not recorded: {e}");
    }
}

/// `take_shown` / `take_rating` become outcomes on the take's recommendation. A take written before
/// the store existed has no recommendation: nothing to record, and that is not an error.
pub(crate) fn note_take_event(
    data: &Path,
    take_id: &str,
    page: &str,
    kind: &str,
    helpful: Option<bool>,
) {
    let (scope, _) = take_scope(page);
    let store = store_for(data, scope);
    let rid = format!("take:{take_id}");
    let known = store.open().and_then(|c| {
        c.query_row(
            "SELECT status FROM recommendation WHERE recommendation_id = ?1",
            params![rid],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())
    });
    let Ok(Some(status)) = known else { return };
    let r = match (kind, helpful) {
        ("take_shown", _) if status == "proposed" => {
            store.add_outcome(&rid, "shown", "system", None, None, None, None)
        }
        ("take_rating", Some(h)) => store.add_outcome(
            &rid,
            "evaluated",
            "taisun",
            None,
            None,
            Some(if h { "helped" } else { "no_effect" }),
            Some(if h {
                "rated helpful"
            } else {
                "rated not helpful"
            }),
        ),
        _ => return,
    };
    if let Err(e) = r {
        eprintln!("[amara-store] take outcome not recorded: {e}");
    }
}

/// The data folder above a store folder, only when the folder has its production name (`takes`,
/// `checkins`, ...). Anything else (a test's scratch folder) records nothing.
pub(crate) fn data_above<'a>(dir: &'a Path, expected: &str) -> Option<&'a Path> {
    if dir.file_name()? == expected {
        dir.parent()
    } else {
        None
    }
}

/// `YYYY-MM-DD` as the start of that day in JST (most domain records carry a day, not a time).
pub(crate) fn day_start(day: &str) -> String {
    format!("{day}T00:00:00+09:00")
}

fn init(conn: &Connection, b: Boundary, path: &Path) -> Result<(), String> {
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
    let (version, fresh) = match found.and_then(|v| v.parse::<i64>().ok()) {
        None => {
            // A new file starts at v1 (the tables above) and is migrated forward like any other.
            conn.execute(
                "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', '1')",
                [],
            )
            .map_err(|e| format!("amara store: {e}"))?;
            (1, true)
        }
        Some(v) if v > SCHEMA_VERSION => {
            return Err(format!(
                "amara store schema {v} is newer than this app ({SCHEMA_VERSION}); not touched"
            ))
        }
        Some(v) => (v, false),
    };
    // One backup per open: when v2 already copied the original file, v3 needs no second copy of
    // a state that only existed inside this open.
    let mut backed_up = false;
    if version < 2 {
        backed_up = migrate_to_v2(conn, path, fresh)?;
    }
    if version < 3 {
        migrate_to_v3(conn, path, fresh || backed_up)?;
    }
    Ok(())
}

/// v2 → v3 (additive, 0063): `business_id` on activity and recommendation. The CHECK allows it on
/// business rows only, so the personal file can never hold one. Same backup and one-transaction
/// rule as v2: on any error nothing changes and the file stays v2. `skip_backup`: a new file, or
/// the v2 step in this same open already copied the original.
fn migrate_to_v3(conn: &Connection, path: &Path, skip_backup: bool) -> Result<(), String> {
    let has_rows: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM activity) + (SELECT COUNT(*) FROM recommendation)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("amara store: {e}"))?;
    if !skip_backup && has_rows > 0 {
        backup_before_migration(conn, path, 2)?;
    }
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| format!("amara store: {e}"))?;
    match migrate_v3_locked(conn) {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|e| format!("amara store: migration to v3 failed: {e}")),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn migrate_v3_locked(tx: &Connection) -> Result<(), String> {
    let now: String = tx
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("amara store: {e}"))?;
    if now.parse::<i64>().unwrap_or(0) >= 3 {
        return Ok(());
    }
    tx.execute_batch(
        "
        ALTER TABLE activity ADD COLUMN business_id TEXT
            CHECK (business_id IS NULL OR scope = 'business');
        ALTER TABLE recommendation ADD COLUMN business_id TEXT
            CHECK (business_id IS NULL OR scope = 'business');
        CREATE INDEX IF NOT EXISTS idx_activity_business ON activity(business_id, occurred_at);
        CREATE INDEX IF NOT EXISTS idx_recommendation_business ON recommendation(business_id, created_at);
        ",
    )
    .map_err(|e| format!("amara store: migration to v3 failed: {e}"))?;
    #[cfg(test)]
    if FAIL_MIGRATION.with(|f| f.get()) {
        return Err("amara store: migration to v3 failed: injected".into());
    }
    tx.execute(
        "UPDATE schema_meta SET value = '3' WHERE key = 'schema_version'",
        [],
    )
    .map_err(|e| format!("amara store: {e}"))?;
    Ok(())
}

/// v1 → v2 (additive): recommendation observation, rationale, suggested_action, uncertainty,
/// impact, effort; activity certainty. A file that already holds rows is copied first
/// (`VACUUM INTO <file>.v1-<timestamp>.bak`, 0600). The change and the version bump are one
/// transaction: on any error nothing changes and the file stays v1.
/// Returns whether a backup was taken.
fn migrate_to_v2(conn: &Connection, path: &Path, fresh: bool) -> Result<bool, String> {
    let has_rows: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM activity) + (SELECT COUNT(*) FROM recommendation)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("amara store: {e}"))?;
    let backup = !fresh && has_rows > 0;
    if backup {
        backup_before_migration(conn, path, 1)?;
    }
    // Take the write lock, then re-check: another connection may have migrated meanwhile.
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| format!("amara store: {e}"))?;
    match migrate_v2_locked(conn) {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map(|_| backup)
            .map_err(|e| format!("amara store: migration to v2 failed: {e}")),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn migrate_v2_locked(tx: &Connection) -> Result<(), String> {
    let now: String = tx
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("amara store: {e}"))?;
    if now.parse::<i64>().unwrap_or(0) >= 2 {
        return Ok(());
    }
    tx.execute_batch(
        "
        ALTER TABLE recommendation ADD COLUMN observation TEXT;
        ALTER TABLE recommendation ADD COLUMN rationale TEXT;
        ALTER TABLE recommendation ADD COLUMN suggested_action TEXT;
        ALTER TABLE recommendation ADD COLUMN uncertainty TEXT;
        ALTER TABLE recommendation ADD COLUMN impact TEXT
            CHECK (impact IS NULL OR impact IN ('low','medium','high'));
        ALTER TABLE recommendation ADD COLUMN effort TEXT
            CHECK (effort IS NULL OR effort IN ('low','medium','high'));
        ALTER TABLE activity ADD COLUMN certainty TEXT NOT NULL DEFAULT 'confirmed'
            CHECK (certainty IN ('confirmed','estimated'));
        ",
    )
    .map_err(|e| format!("amara store: migration to v2 failed: {e}"))?;
    #[cfg(test)]
    if FAIL_MIGRATION.with(|f| f.get()) {
        return Err("amara store: migration to v2 failed: injected".into());
    }
    tx.execute(
        "UPDATE schema_meta SET value = '2' WHERE key = 'schema_version'",
        [],
    )
    .map_err(|e| format!("amara store: {e}"))?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_MIGRATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `<file>.v<from>-<timestamp>.bak` beside the database, as one consistent snapshot.
fn backup_before_migration(conn: &Connection, path: &Path, from: i64) -> Result<PathBuf, String> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.6fZ");
    let name = format!(
        "{}.v{from}-{stamp}.bak",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("store.db")
    );
    let dest = path.with_file_name(name);
    // Created empty and 0600 first, so the copy is never readable by others, even for a moment.
    crate::app_paths::ensure_private_file(&dest)
        .map_err(|e| format!("amara store: backup file: {e}"))?;
    conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])
        .map_err(|e| {
            format!("amara store: backup before migration failed, nothing changed: {e}")
        })?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("amara store: backup permissions: {e}"))?;
    Ok(dest)
}

/// Rollback: put a pre-migration backup back in place of the database (the older app build then
/// reads it). The current file is kept as `<file>.replaced-<timestamp>`; nothing is deleted.
/// No connection to the database may be open.
// The rollback tool: run with the app closed (tests drive it; the steps are in decision 0053).
#[allow(dead_code)]
pub(crate) fn restore_backup(db: &Path, backup: &Path) -> Result<PathBuf, String> {
    if !backup.is_file() {
        return Err("restore: the backup file doesn't exist".into());
    }
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.6fZ");
    let kept = db.with_file_name(format!(
        "{}.replaced-{stamp}",
        db.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("store.db")
    ));
    if db.exists() {
        // Fold the WAL into the file first so the kept copy is complete.
        if let Ok(c) = Connection::open(db) {
            let _ = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
        std::fs::rename(db, &kept).map_err(|e| format!("restore: {e}"))?;
    }
    for side in ["-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{side}", db.display()));
        if p.exists() {
            let _ = std::fs::rename(&p, PathBuf::from(format!("{}{side}", kept.display())));
        }
    }
    std::fs::copy(backup, db).map_err(|e| format!("restore: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(db, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("restore: {e}"))?;
    Ok(kept)
}

/// 0053 step 1: create both stores at startup (idempotent) and log the first creation in
/// `<data>/migrations/log.jsonl`. Touches no existing file. Errors are logged, never fatal.
pub(crate) fn init_on_startup(data: &Path) {
    for scope in [Scope::Personal, Scope::Business] {
        let store = store_for(data, scope);
        let existed = store.path.exists();
        let pruned = store.prune_backups(30);
        if pruned > 0 {
            eprintln!("[amara-store] removed {pruned} backup copy(ies) older than 30 days");
        }
        match store.open() {
            Ok(_) if !existed => log_migration(data, &store.path, "created schema v2"),
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

pub(crate) mod commands;
pub(crate) mod resolve;

#[cfg(test)]
mod tests;
