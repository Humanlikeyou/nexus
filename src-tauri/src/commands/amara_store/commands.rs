//! Repository reads and the Tauri commands over the shared data foundations (0053).
//!
//! Every command names a scope (`personal`, `neural` or `business`), which picks the file: business
//! commands only ever open `business/amara_business.db`. Recommendations created over IPC count as
//! model-written (the renderer can't prove otherwise), so every evidence reference must resolve to a
//! real record (see `resolve.rs`). Approvals and the execution history are read from the 0052
//! ledger; nothing here can grant an approval.

use super::{store_for, NewEvidence, NewRecommendation, Scope, Store};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn scope_of(s: &str) -> Result<Scope, String> {
    match s {
        "personal" => Ok(Scope::Personal),
        "neural" => Ok(Scope::Neural),
        "business" => Ok(Scope::Business),
        _ => Err("scope must be personal, neural or business".into()),
    }
}

fn err(e: impl std::fmt::Display) -> String {
    format!("amara store: {e}")
}

/// Provenance, derived from `source` (no extra column): manual, automated, inferred, imported.
pub(crate) fn provenance(source: &str) -> &'static str {
    match source {
        "user" => "manual",
        "scheduler" | "noop" => "automated",
        "model" => "inferred",
        _ => "imported",
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    pub activity_id: String,
    pub scope: String,
    pub kind: String,
    pub subject_ref: String,
    pub occurred_at: String,
    pub recorded_at: String,
    pub source: String,
    /// manual | automated | inferred | imported.
    pub provenance: String,
    /// confirmed | estimated.
    pub certainty: String,
    pub source_ref: Option<String>,
    pub actor: String,
    pub summary: Option<String>,
    pub data_json: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationRow {
    pub recommendation_id: String,
    pub scope: String,
    pub domain: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub observation: Option<String>,
    pub rationale: Option<String>,
    pub suggested_action: Option<String>,
    pub confidence: Option<f64>,
    pub confidence_basis: Option<String>,
    pub uncertainty: Option<String>,
    pub impact: Option<String>,
    pub effort: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: Option<String>,
    pub produced_by: String,
    pub model_tier: Option<String>,
    pub target_ref: Option<String>,
    pub supersedes_id: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRow {
    pub evidence_id: String,
    pub record_ref: String,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub role: String,
    pub quality: String,
    pub note: Option<String>,
    pub captured_at: String,
    pub snapshot_sha256: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeRow {
    pub outcome_id: String,
    pub outcome: String,
    pub at: String,
    pub actor: String,
    pub modification_json: Option<String>,
    pub result_ref: Option<String>,
    pub evaluation: Option<String>,
    pub note: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationDetail {
    pub recommendation: RecommendationRow,
    pub evidence: Vec<EvidenceRow>,
    pub outcomes: Vec<OutcomeRow>,
}

const REC_COLS: &str =
    "recommendation_id, scope, domain, kind, title, body, observation, rationale,
    suggested_action, confidence, confidence_basis, uncertainty, impact, effort, status, created_at,
    updated_at, expires_at, produced_by, model_tier, target_ref, supersedes_id";

fn rec_row(r: &rusqlite::Row) -> rusqlite::Result<RecommendationRow> {
    Ok(RecommendationRow {
        recommendation_id: r.get(0)?,
        scope: r.get(1)?,
        domain: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        body: r.get(5)?,
        observation: r.get(6)?,
        rationale: r.get(7)?,
        suggested_action: r.get(8)?,
        confidence: r.get(9)?,
        confidence_basis: r.get(10)?,
        uncertainty: r.get(11)?,
        impact: r.get(12)?,
        effort: r.get(13)?,
        status: r.get(14)?,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
        expires_at: r.get(17)?,
        produced_by: r.get(18)?,
        model_tier: r.get(19)?,
        target_ref: r.get(20)?,
        supersedes_id: r.get(21)?,
    })
}

impl Store {
    pub(crate) fn list_activity(
        &self,
        scope: Scope,
        subject_ref: Option<&str>,
        since: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ActivityRow>, String> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(
                "SELECT activity_id, scope, kind, subject_ref, occurred_at, recorded_at, source,
                    certainty, source_ref, actor, summary, data_json FROM activity
                 WHERE scope = ?1 AND (?2 IS NULL OR subject_ref = ?2) AND (?3 IS NULL OR occurred_at >= ?3)
                 ORDER BY occurred_at DESC LIMIT ?4",
            )
            .map_err(err)?;
        let rows = st
            .query_map(
                params![scope.as_str(), subject_ref, since, limit.min(1000)],
                |r| {
                    let source: String = r.get(6)?;
                    Ok(ActivityRow {
                        activity_id: r.get(0)?,
                        scope: r.get(1)?,
                        kind: r.get(2)?,
                        subject_ref: r.get(3)?,
                        occurred_at: r.get(4)?,
                        recorded_at: r.get(5)?,
                        provenance: provenance(&source).into(),
                        source,
                        certainty: r.get(7)?,
                        source_ref: r.get(8)?,
                        actor: r.get(9)?,
                        summary: r.get(10)?,
                        data_json: r.get(11)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn list_recommendations(
        &self,
        scope: Scope,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<RecommendationRow>, String> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(&format!(
                "SELECT {REC_COLS} FROM recommendation WHERE scope = ?1 AND (?2 IS NULL OR status = ?2)
                 ORDER BY created_at DESC LIMIT ?3"
            ))
            .map_err(err)?;
        let rows = st
            .query_map(params![scope.as_str(), status, limit.min(500)], rec_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn get_recommendation(
        &self,
        id: &str,
    ) -> Result<Option<RecommendationDetail>, String> {
        let conn = self.open()?;
        let Some(rec) = conn
            .query_row(
                &format!("SELECT {REC_COLS} FROM recommendation WHERE recommendation_id = ?1"),
                params![id],
                rec_row,
            )
            .optional()
            .map_err(err)?
        else {
            return Ok(None);
        };
        let evidence = conn
            .prepare(
                "SELECT evidence_id, record_ref, window_start, window_end, role, quality, note,
                    captured_at, snapshot_sha256 FROM recommendation_evidence
                 WHERE recommendation_id = ?1 ORDER BY captured_at, evidence_id",
            )
            .map_err(err)?
            .query_map(params![id], |r| {
                Ok(EvidenceRow {
                    evidence_id: r.get(0)?,
                    record_ref: r.get(1)?,
                    window_start: r.get(2)?,
                    window_end: r.get(3)?,
                    role: r.get(4)?,
                    quality: r.get(5)?,
                    note: r.get(6)?,
                    captured_at: r.get(7)?,
                    snapshot_sha256: r.get(8)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let outcomes = conn
            .prepare(
                "SELECT outcome_id, outcome, at, actor, modification_json, result_ref, evaluation, note
                 FROM recommendation_outcome WHERE recommendation_id = ?1 ORDER BY at, outcome_id",
            )
            .map_err(err)?
            .query_map(params![id], |r| {
                Ok(OutcomeRow {
                    outcome_id: r.get(0)?,
                    outcome: r.get(1)?,
                    at: r.get(2)?,
                    actor: r.get(3)?,
                    modification_json: r.get(4)?,
                    result_ref: r.get(5)?,
                    evaluation: r.get(6)?,
                    note: r.get(7)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(Some(RecommendationDetail {
            recommendation: rec,
            evidence,
            outcomes,
        }))
    }
}

// ─── Inputs ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, Clone, Debug, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceInput {
    pub record_ref: String,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub role: String,
    pub quality: String,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone, Debug, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendationInput {
    pub scope: String,
    pub domain: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub observation: Option<String>,
    pub rationale: Option<String>,
    pub suggested_action: Option<String>,
    pub confidence: Option<f64>,
    pub confidence_basis: Option<String>,
    pub uncertainty: Option<String>,
    pub impact: Option<String>,
    pub effort: Option<String>,
    pub expires_at: Option<String>,
    /// The inference purpose label that produced it (e.g. `weekly_review.suggest`).
    pub purpose: String,
    pub model_tier: Option<String>,
    pub target_ref: Option<String>,
    pub supersedes_id: Option<String>,
    pub evidence: Vec<EvidenceInput>,
}

/// Creates a model-written recommendation: `produced_by` is always `model:<purpose>`, so every
/// evidence reference must resolve to a real record.
pub(crate) fn create_from_input(
    data: &Path,
    input: &RecommendationInput,
) -> Result<String, String> {
    let scope = scope_of(&input.scope)?;
    let purpose: String = input
        .purpose
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(60)
        .collect();
    if purpose.is_empty() {
        return Err("purpose required".into());
    }
    if input.kind == "take" {
        return Err("takes are recorded by the take log, not created here".into());
    }
    let produced_by = format!("model:{purpose}");
    let tier = input
        .model_tier
        .as_deref()
        .filter(|t| matches!(*t, "light" | "deep" | "claude"));
    let evidence: Vec<NewEvidence> = input
        .evidence
        .iter()
        .map(|e| NewEvidence {
            record_ref: &e.record_ref,
            window_start: e.window_start.as_deref(),
            window_end: e.window_end.as_deref(),
            role: &e.role,
            quality: &e.quality,
            note: e.note.as_deref(),
        })
        .collect();
    store_for(data, scope).create_recommendation(
        &NewRecommendation {
            id: None,
            scope,
            domain: &input.domain,
            kind: &input.kind,
            title: &input.title,
            body: &input.body,
            expires_at: input.expires_at.as_deref(),
            produced_by: &produced_by,
            model_tier: tier,
            confidence: input.confidence,
            confidence_basis: input.confidence_basis.as_deref(),
            target_ref: input.target_ref.as_deref(),
            supersedes_id: input.supersedes_id.as_deref(),
            observation: input.observation.as_deref(),
            rationale: input.rationale.as_deref(),
            suggested_action: input.suggested_action.as_deref(),
            uncertainty: input.uncertainty.as_deref(),
            impact: input.impact.as_deref(),
            effort: input.effort.as_deref(),
        },
        &evidence,
    )
}

/// Taisun's response: shown (by the app), accepted, modified (with what changed), dismissed.
pub(crate) fn respond_in(
    data: &Path,
    scope: &str,
    id: &str,
    response: &str,
    modification_json: Option<&str>,
) -> Result<String, String> {
    let actor = match response {
        "shown" => "system",
        "accepted" | "modified" | "dismissed" => "taisun",
        _ => return Err("response must be shown, accepted, modified or dismissed".into()),
    };
    if response == "modified" && modification_json.is_none() {
        return Err("a modified response needs what was changed".into());
    }
    store_for(data, scope_of(scope)?).add_outcome(
        id,
        response,
        actor,
        modification_json,
        None,
        None,
        None,
    )
}

/// What happened afterwards: completed (with what it produced), evaluated, or expired.
pub(crate) fn outcome_in(
    data: &Path,
    scope: &str,
    id: &str,
    outcome: &str,
    evaluation: Option<&str>,
    result_ref: Option<&str>,
    note: Option<&str>,
) -> Result<String, String> {
    if !matches!(outcome, "completed" | "evaluated" | "expired") {
        return Err("outcome must be completed, evaluated or expired".into());
    }
    let actor = if outcome == "expired" {
        "system"
    } else {
        "taisun"
    };
    store_for(data, scope_of(scope)?)
        .add_outcome(id, outcome, actor, None, result_ref, evaluation, note)
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AuditTrail {
    pub record_ref: String,
    pub activity: Vec<ActivityRow>,
    /// Recommendations that target or cite the record.
    pub recommendations: Vec<RecommendationDetail>,
    /// Ledger executions that name the record (`execution:<id>` refs in outcomes, or the target).
    pub executions: Vec<crate::commands::execution::ExecutionRecord>,
}

/// Everything recorded about one record, read-only, from the scope its reference belongs to.
pub(crate) fn audit_in(data: &Path, record_ref: &str) -> Result<AuditTrail, String> {
    let r = super::RecordRef::parse(record_ref)?;
    let scope = r.scope;
    let store = store_for(data, scope);
    let activity = if scope == Scope::Ops {
        vec![]
    } else {
        store.list_activity(scope, Some(record_ref), None, 1000)?
    };
    let ids: Vec<String> = {
        let conn = store.open()?;
        let mut st = conn
            .prepare(
                "SELECT recommendation_id FROM recommendation WHERE target_ref = ?1
                 UNION SELECT recommendation_id FROM recommendation_evidence WHERE record_ref = ?1",
            )
            .map_err(err)?;
        let v = st
            .query_map(params![record_ref], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        v
    };
    let mut recommendations = vec![];
    let mut exec_ids: Vec<String> = vec![];
    for id in ids {
        if let Some(d) = store.get_recommendation(&id)? {
            for o in &d.outcomes {
                if let Some(e) = o
                    .result_ref
                    .as_deref()
                    .and_then(|x| x.strip_prefix("execution:"))
                {
                    exec_ids.push(e.to_string());
                }
            }
            recommendations.push(d);
        }
    }
    if r.store == "execution" {
        exec_ids.push(r.id.clone());
    }
    let ledger = crate::commands::execution::Ledger::at(data.join("execution.db"));
    let mut executions = vec![];
    if ledger.path().exists() {
        for e in exec_ids {
            if let Ok(Some(rec)) = ledger.get(&e) {
                executions.push(rec);
            }
        }
    }
    Ok(AuditTrail {
        record_ref: record_ref.to_string(),
        activity,
        recommendations,
        executions,
    })
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

fn data() -> std::path::PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn activity_list(
    scope: String,
    subject_ref: Option<String>,
    since: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ActivityRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = scope_of(&scope)?;
        store_for(&data(), s).list_activity(
            s,
            subject_ref.as_deref(),
            since.as_deref(),
            limit.unwrap_or(100),
        )
    })
    .await
    .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub async fn recommendation_list(
    scope: String,
    status: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<RecommendationRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = scope_of(&scope)?;
        store_for(&data(), s).list_recommendations(s, status.as_deref(), limit.unwrap_or(50))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub async fn recommendation_get(
    scope: String,
    id: String,
) -> Result<Option<RecommendationDetail>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        store_for(&data(), scope_of(&scope)?).get_recommendation(&id)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub async fn recommendation_create(input: RecommendationInput) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || create_from_input(&data(), &input))
        .await
        .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub async fn recommendation_respond(
    scope: String,
    id: String,
    response: String,
    modification_json: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        respond_in(
            &data(),
            &scope,
            &id,
            &response,
            modification_json.as_deref(),
        )
    })
    .await
    .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub async fn recommendation_record_outcome(
    scope: String,
    id: String,
    outcome: String,
    evaluation: Option<String>,
    result_ref: Option<String>,
    note: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        outcome_in(
            &data(),
            &scope,
            &id,
            &outcome,
            evaluation.as_deref(),
            result_ref.as_deref(),
            note.as_deref(),
        )
    })
    .await
    .map_err(err)?
}

/// Consequential-action approvals from the 0052 ledger (read-only: none can be granted here).
#[tauri::command]
#[specta::specta]
pub fn approvals_list(
    limit: Option<u32>,
) -> Result<Vec<crate::commands::execution::ledger::ApprovalRow>, String> {
    let ledger = crate::commands::execution::Ledger::at(data().join("execution.db"));
    ledger
        .approvals(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn audit_trail(record_ref: String) -> Result<AuditTrail, String> {
    tauri::async_runtime::spawn_blocking(move || audit_in(&data(), &record_ref))
        .await
        .map_err(err)?
}
