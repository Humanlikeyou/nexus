use super::{terrence_actions, terrence_context, terrence_projects};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_PRIORITY_ITEMS: usize = 8;
const MAX_DECISION_TEXT_CHARS: usize = 4_000;
const MAX_DECISION_ITEMS: usize = 12;
const MAX_CONTEXT_ITEM_IDS: usize = 32;
const DECISION_STATUSES: &[&str] = &["open", "decided", "observing", "closed", "cancelled"];

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerrencePriority {
    pub rank: u32,
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub source_status: String,
    pub score: u8,
    pub confidence: f64,
    pub reasons: Vec<String>,
    pub context_item_ids: Vec<String>,
    pub action_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerrencePrioritySnapshot {
    pub id: String,
    pub generated_at: String,
    pub items: Vec<TerrencePriority>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DecisionEvidence {
    pub detail: String,
    pub source: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DecisionAlternative {
    pub label: String,
    pub tradeoffs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceDecision {
    pub id: String,
    pub question: String,
    pub status: String,
    pub constraints: Vec<String>,
    pub evidence: Vec<DecisionEvidence>,
    pub alternatives: Vec<DecisionAlternative>,
    pub recommendation: String,
    pub uncertainty: Option<String>,
    pub decision: Option<String>,
    pub outcome: Option<String>,
    pub conversation_id: Option<String>,
    pub context_item_ids: Vec<String>,
    pub provenance_source: String,
    pub created_at: String,
    pub updated_at: String,
    pub decided_at: Option<String>,
    pub outcome_recorded_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerrenceDecisionInput {
    pub id: Option<String>,
    pub question: String,
    pub constraints: Option<Vec<String>>,
    pub evidence: Option<Vec<DecisionEvidence>>,
    pub alternatives: Option<Vec<DecisionAlternative>>,
    pub recommendation: String,
    pub uncertainty: Option<String>,
    pub conversation_id: Option<String>,
    pub context_item_ids: Option<Vec<String>>,
    pub provenance_source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTerrenceDecisionInput {
    pub id: String,
    pub status: String,
    pub decision: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HermesDecisionInboxItem {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    question: Option<String>,
    constraints: Option<Vec<String>>,
    evidence: Option<Vec<DecisionEvidence>>,
    alternatives: Option<Vec<DecisionAlternative>>,
    recommendation: Option<String>,
    uncertainty: Option<String>,
    status: Option<String>,
    decision: Option<String>,
    outcome: Option<String>,
    provenance_source: Option<String>,
}

#[derive(Debug, Clone)]
struct RankedPriority {
    priority: TerrencePriority,
    updated_at: String,
}

fn priority_db() -> Result<Connection, String> {
    let conn = terrence_context::open_terrence_core_db()?;
    init_priority_db(&conn)?;
    Ok(conn)
}

fn init_priority_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS terrence_priority_snapshots (
            id                TEXT PRIMARY KEY,
            input_fingerprint TEXT NOT NULL UNIQUE,
            generated_at      TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS terrence_priority_items (
            snapshot_id       TEXT NOT NULL,
            rank              INTEGER NOT NULL,
            source_kind       TEXT NOT NULL,
            source_id         TEXT NOT NULL,
            title             TEXT NOT NULL,
            source_status     TEXT NOT NULL,
            score             INTEGER NOT NULL,
            confidence        REAL NOT NULL,
            reasons           TEXT NOT NULL,
            context_item_ids  TEXT NOT NULL,
            action_id         TEXT,
            PRIMARY KEY (snapshot_id, rank),
            FOREIGN KEY(snapshot_id) REFERENCES terrence_priority_snapshots(id)
        );
        CREATE INDEX IF NOT EXISTS idx_terrence_priority_items_source
            ON terrence_priority_items(source_kind, source_id);
        CREATE TABLE IF NOT EXISTS terrence_decisions (
            id                  TEXT PRIMARY KEY,
            question            TEXT NOT NULL,
            status              TEXT NOT NULL,
            constraints         TEXT NOT NULL,
            evidence            TEXT NOT NULL,
            alternatives        TEXT NOT NULL,
            recommendation      TEXT NOT NULL,
            uncertainty         TEXT,
            decision            TEXT,
            outcome             TEXT,
            conversation_id     TEXT,
            context_item_ids    TEXT NOT NULL,
            provenance_source   TEXT NOT NULL,
            created_at          TEXT NOT NULL,
            updated_at          TEXT NOT NULL,
            decided_at          TEXT,
            outcome_recorded_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_terrence_decisions_updated
            ON terrence_decisions(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_terrence_decisions_status
            ON terrence_decisions(status, updated_at DESC);
        ",
    )
    .map_err(|error| format!("Failed to initialize Amara priority schema: {error}"))
}

fn clean_required(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required."));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} may not exceed {max_chars} characters."));
    }
    Ok(value.to_string())
}

fn clean_optional(
    value: Option<String>,
    label: &str,
    max_chars: usize,
) -> Result<Option<String>, String> {
    match value {
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => Ok(Some(clean_required(&value, label, max_chars)?)),
        None => Ok(None),
    }
}

fn clean_list(values: Option<Vec<String>>, label: &str) -> Result<Vec<String>, String> {
    let mut cleaned = Vec::new();
    for value in values
        .unwrap_or_default()
        .into_iter()
        .take(MAX_DECISION_ITEMS)
    {
        let value = clean_required(&value, label, MAX_DECISION_TEXT_CHARS)?;
        if !cleaned.contains(&value) {
            cleaned.push(value);
        }
    }
    Ok(cleaned)
}

fn clean_context_item_ids(values: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let mut cleaned = Vec::new();
    for value in values
        .unwrap_or_default()
        .into_iter()
        .take(MAX_CONTEXT_ITEM_IDS)
    {
        let value = clean_required(&value, "Context item ID", 120)?;
        if !cleaned.contains(&value) {
            cleaned.push(value);
        }
    }
    Ok(cleaned)
}

fn hermes_decision_inbox_dir() -> Option<PathBuf> {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hermes")))
        .map(|home| home.join("nexus").join("decision-inbox"))
}

fn reject_decision_inbox_item(path: &Path) {
    let rejected = path.with_extension("rejected");
    let _ = fs::rename(path, rejected);
}

fn ingest_hermes_decision_inbox_in(conn: &Connection, directory: &Path) -> usize {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut imported = 0;
    for path in paths.into_iter().take(64) {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(item) = serde_json::from_str::<HermesDecisionInboxItem>(&contents) else {
            reject_decision_inbox_item(&path);
            continue;
        };
        let result = match item.kind.as_str() {
            "record" => create_terrence_decision_in(
                conn,
                CreateTerrenceDecisionInput {
                    id: item.id,
                    question: item.question.unwrap_or_default(),
                    constraints: item.constraints,
                    evidence: item.evidence,
                    alternatives: item.alternatives,
                    recommendation: item.recommendation.unwrap_or_default(),
                    uncertainty: item.uncertainty,
                    conversation_id: None,
                    context_item_ids: None,
                    provenance_source: item
                        .provenance_source
                        .or_else(|| Some("hermes_cross_channel_decision".to_string())),
                },
            )
            .map(|_| ()),
            "resolve" => resolve_terrence_decision_in(
                conn,
                ResolveTerrenceDecisionInput {
                    id: item.id.unwrap_or_default(),
                    status: item.status.unwrap_or_default(),
                    decision: item.decision,
                    outcome: item.outcome,
                },
            )
            .map(|_| ()),
            _ => Err("Unknown Hermes decision inbox item.".to_string()),
        };

        if result.is_ok() {
            let _ = fs::remove_file(&path);
            imported += 1;
        } else {
            reject_decision_inbox_item(&path);
        }
    }
    imported
}

fn ingest_hermes_decision_inbox(conn: &Connection) {
    if let Some(directory) = hermes_decision_inbox_dir() {
        let _ = ingest_hermes_decision_inbox_in(conn, &directory);
    }
}

fn metadata_text(metadata: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        metadata
            .get(*key)
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
    })
}

fn context_deadline(item: &terrence_context::ContextItem) -> Option<DateTime<Utc>> {
    let candidate = metadata_text(&item.metadata, &["dueAt", "due_at", "deadline"])
        .or_else(|| item.occurred_at.clone())?;
    DateTime::parse_from_rfc3339(&candidate)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn context_priority(item: terrence_context::ContextItem, now: DateTime<Utc>) -> RankedPriority {
    let mut score = match item.kind.as_str() {
        "commitment" => 40.0,
        "project" => 29.0,
        "goal" => 26.0,
        "inference" => 12.0,
        _ => 8.0,
    };
    let mut reasons = vec![format!("{} {}", item.status, item.kind)];
    match item.status.as_str() {
        "blocked" => {
            score += 26.0;
            reasons.push("currently blocked".to_string());
        }
        "paused" => score -= 14.0,
        _ => {}
    }
    match metadata_text(&item.metadata, &["urgency", "priority"]).as_deref() {
        Some("critical") | Some("high") => {
            score += 20.0;
            reasons.push("marked high urgency".to_string());
        }
        Some("medium") => {
            score += 9.0;
            reasons.push("marked medium urgency".to_string());
        }
        Some("low") => score -= 8.0,
        _ => {}
    }
    match metadata_text(
        &item.metadata,
        &["strategicValue", "strategic_value", "impact"],
    )
    .as_deref()
    {
        Some("high") => {
            score += 13.0;
            reasons.push("marked high strategic value".to_string());
        }
        Some("medium") => score += 6.0,
        _ => {}
    }
    if let Some(deadline) = context_deadline(&item) {
        let hours = (deadline - now).num_hours();
        if hours < 0 {
            score += 28.0;
            reasons.push("deadline has passed".to_string());
        } else if hours <= 24 {
            score += 23.0;
            reasons.push("deadline is within a day".to_string());
        } else if hours <= 72 {
            score += 15.0;
            reasons.push("deadline is within three days".to_string());
        } else if hours <= 24 * 7 {
            score += 8.0;
            reasons.push("deadline is within a week".to_string());
        }
    }
    let score = (score * (0.65 + item.confidence.clamp(0.0, 1.0) * 0.35))
        .round()
        .clamp(1.0, 99.0) as u8;
    RankedPriority {
        priority: TerrencePriority {
            rank: 0,
            source_kind: "context".to_string(),
            source_id: item.id.clone(),
            title: item.title,
            source_status: item.status,
            score,
            confidence: item.confidence,
            reasons,
            context_item_ids: vec![item.id],
            action_id: None,
        },
        updated_at: item.updated_at,
    }
}

fn action_priority(action: terrence_actions::TerrenceAction) -> RankedPriority {
    let (score, reason) = match action.status.as_str() {
        "needs_attention" => (82, "an earlier action needs attention"),
        "failed" => (76, "an action failed and may need a decision"),
        "awaiting_approval" => (64, "waiting for your approval"),
        "approved" => (58, "approved action is waiting to run"),
        "planning" | "requested" => (46, "action is being prepared"),
        _ => (1, "action state"),
    };
    RankedPriority {
        priority: TerrencePriority {
            rank: 0,
            source_kind: "action".to_string(),
            source_id: action.id.clone(),
            title: action.title,
            source_status: action.status,
            score,
            confidence: 1.0,
            reasons: vec![reason.to_string()],
            context_item_ids: action.context_item_ids,
            action_id: Some(action.id),
        },
        updated_at: action.updated_at,
    }
}

fn project_priority(project: terrence_projects::TerrenceProject) -> RankedPriority {
    let mut score: f64 = 35.0;
    let mut reasons = vec![format!("{} project", project.status)];
    if project.status == "blocked" {
        score += 30.0;
        reasons.push("currently blocked".to_string());
    } else if project.status == "paused" {
        score -= 15.0;
    }
    if project.next_action.is_some() {
        score += 11.0;
        reasons.push("has a defined next action".to_string());
    }
    if !project.blockers.is_empty() {
        score += 14.0;
        reasons.push(format!("{} recorded blocker(s)", project.blockers.len()));
    }
    let blocked_milestones = project
        .milestones
        .iter()
        .filter(|milestone| milestone.status == "blocked")
        .count();
    if blocked_milestones > 0 {
        score += 10.0;
        reasons.push(format!("{blocked_milestones} blocked milestone(s)"));
    }
    RankedPriority {
        priority: TerrencePriority {
            rank: 0,
            source_kind: "project".to_string(),
            source_id: project.id,
            title: project.title,
            source_status: project.status,
            score: score.round().clamp(1.0, 99.0) as u8,
            confidence: 1.0,
            reasons,
            context_item_ids: project.context_item_ids,
            action_id: None,
        },
        updated_at: project.updated_at,
    }
}

fn rank_priorities(
    context_items: Vec<terrence_context::ContextItem>,
    actions: Vec<terrence_actions::TerrenceAction>,
    projects: Vec<terrence_projects::TerrenceProject>,
    now: DateTime<Utc>,
) -> Vec<TerrencePriority> {
    let mut entries: Vec<RankedPriority> = context_items
        .into_iter()
        .map(|item| context_priority(item, now))
        .chain(actions.into_iter().map(action_priority))
        .chain(projects.into_iter().map(project_priority))
        .collect();
    entries.sort_by(|left, right| {
        right
            .priority
            .score
            .cmp(&left.priority.score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.priority.title.cmp(&right.priority.title))
    });
    entries
        .into_iter()
        .take(MAX_PRIORITY_ITEMS)
        .enumerate()
        .map(|(index, mut entry)| {
            entry.priority.rank = index as u32 + 1;
            entry.priority
        })
        .collect()
}

fn priority_input_fingerprint(
    context_items: &[terrence_context::ContextItem],
    actions: &[terrence_actions::TerrenceAction],
    projects: &[terrence_projects::TerrenceProject],
    now: DateTime<Utc>,
) -> Result<String, String> {
    let context = context_items
        .iter()
        .map(|item| {
            json!({
                "id": item.id,
                "kind": item.kind,
                "title": item.title,
                "status": item.status,
                "confidence": item.confidence,
                "updatedAt": item.updated_at,
                "occurredAt": item.occurred_at,
                "metadata": item.metadata,
            })
        })
        .collect::<Vec<_>>();
    let actions = actions
        .iter()
        .map(|action| {
            json!({
                "id": action.id,
                "title": action.title,
                "status": action.status,
                "updatedAt": action.updated_at,
                "contextItemIds": action.context_item_ids,
            })
        })
        .collect::<Vec<_>>();
    let projects = projects
        .iter()
        .map(|project| {
            json!({
                "id": project.id,
                "title": project.title,
                "status": project.status,
                "nextAction": project.next_action,
                "blockers": project.blockers,
                "milestones": project.milestones,
                "updatedAt": project.updated_at,
                "contextItemIds": project.context_item_ids,
            })
        })
        .collect::<Vec<_>>();
    let serialized = serde_json::to_vec(&json!({
        "priorityScoringDay": now.date_naive(),
        "context": context,
        "actions": actions,
        "projects": projects,
    }))
    .map_err(|error| format!("Failed to prepare Amara priority inputs: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(serialized);
    Ok(hex::encode(hasher.finalize()))
}

fn priority_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerrencePriority> {
    let reasons: String = row.get(7)?;
    let context_item_ids: String = row.get(8)?;
    Ok(TerrencePriority {
        rank: row.get(0)?,
        source_kind: row.get(1)?,
        source_id: row.get(2)?,
        title: row.get(3)?,
        source_status: row.get(4)?,
        score: row.get(5)?,
        confidence: row.get(6)?,
        reasons: serde_json::from_str(&reasons).unwrap_or_default(),
        context_item_ids: serde_json::from_str(&context_item_ids).unwrap_or_default(),
        action_id: row.get(9)?,
    })
}

fn snapshot_by_fingerprint(
    conn: &Connection,
    fingerprint: &str,
) -> Result<Option<TerrencePrioritySnapshot>, String> {
    let snapshot = conn
        .query_row(
            "SELECT id, generated_at FROM terrence_priority_snapshots WHERE input_fingerprint = ?1",
            params![fingerprint],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("Failed to read Amara priority snapshot: {error}"))?;
    let Some((id, generated_at)) = snapshot else {
        return Ok(None);
    };
    let mut statement = conn
        .prepare(
            "SELECT rank, source_kind, source_id, title, source_status, score, confidence,
                    reasons, context_item_ids, action_id
             FROM terrence_priority_items WHERE snapshot_id = ?1 ORDER BY rank ASC",
        )
        .map_err(|error| format!("Failed to prepare Amara priority items: {error}"))?;
    let items = statement
        .query_map(params![id], priority_from_row)
        .map_err(|error| format!("Failed to read Amara priority items: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to collect Amara priority items: {error}"))?;
    Ok(Some(TerrencePrioritySnapshot {
        id,
        generated_at,
        items,
    }))
}

fn persist_snapshot(
    conn: &Connection,
    fingerprint: &str,
    generated_at: String,
    items: &[TerrencePriority],
) -> Result<TerrencePrioritySnapshot, String> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO terrence_priority_snapshots (id, input_fingerprint, generated_at) VALUES (?1, ?2, ?3)",
        params![id, fingerprint, generated_at],
    )
    .map_err(|error| format!("Failed to persist Amara priority snapshot: {error}"))?;
    for item in items {
        conn.execute(
            "INSERT INTO terrence_priority_items (
                snapshot_id, rank, source_kind, source_id, title, source_status, score,
                confidence, reasons, context_item_ids, action_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                item.rank,
                item.source_kind,
                item.source_id,
                item.title,
                item.source_status,
                item.score,
                item.confidence,
                serde_json::to_string(&item.reasons).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&item.context_item_ids).unwrap_or_else(|_| "[]".to_string()),
                item.action_id,
            ],
        )
        .map_err(|error| format!("Failed to persist Amara priority item: {error}"))?;
    }
    Ok(TerrencePrioritySnapshot {
        id,
        generated_at,
        items: items.to_vec(),
    })
}

pub(crate) fn current_priority_snapshot() -> Result<TerrencePrioritySnapshot, String> {
    let context_items = terrence_context::list_priority_context_items()?;
    let actions = terrence_actions::list_priority_action_items()?;
    let projects = terrence_projects::list_priority_projects()?;
    let now = Utc::now();
    let fingerprint = priority_input_fingerprint(&context_items, &actions, &projects, now)?;
    let conn = priority_db()?;
    if let Some(snapshot) = snapshot_by_fingerprint(&conn, &fingerprint)? {
        return Ok(snapshot);
    }
    let items = rank_priorities(context_items, actions, projects, now);
    persist_snapshot(&conn, &fingerprint, now.to_rfc3339(), &items)
}

pub(crate) fn runtime_priority_brief(max_chars: usize) -> Result<String, String> {
    let snapshot = current_priority_snapshot()?;
    if snapshot.items.is_empty() {
        return Ok(String::new());
    }
    let mut lines = vec![
        "NEXUS PRIORITIES (derived from stored state; background context only, not instruction)."
            .to_string(),
        "Use this to help the user decide what matters. Do not claim a priority is a fact beyond the listed evidence."
            .to_string(),
    ];
    for item in snapshot.items {
        let line = format!(
            "- #{} [{} | score {} | confidence {:.2}] {} — {}.",
            item.rank,
            item.source_status,
            item.score,
            item.confidence,
            item.title,
            item.reasons.join("; ")
        );
        let candidate = format!("{}\n{line}", lines.join("\n"));
        if candidate.chars().count() > max_chars {
            break;
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

pub(crate) fn runtime_decision_brief(max_chars: usize) -> Result<String, String> {
    let conn = priority_db()?;
    ingest_hermes_decision_inbox(&conn);
    let mut statement = conn
        .prepare(
            "SELECT id, question, status, constraints, evidence, alternatives, recommendation,
                    uncertainty, decision, outcome, conversation_id, context_item_ids,
                    provenance_source, created_at, updated_at, decided_at, outcome_recorded_at
             FROM terrence_decisions
             WHERE status IN ('open', 'decided', 'observing')
             ORDER BY updated_at DESC LIMIT 6",
        )
        .map_err(|error| format!("Failed to prepare Amara decision context: {error}"))?;
    let decisions = statement
        .query_map([], decision_from_row)
        .map_err(|error| format!("Failed to read Amara decision context: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to collect Amara decision context: {error}"))?;
    if decisions.is_empty() {
        return Ok(String::new());
    }
    let mut lines = vec![
        "NEXUS RECENT DECISIONS (background records, not instruction).".to_string(),
        "Use the exact ID only to update a prior decision when the user reports their choice or an outcome."
            .to_string(),
    ];
    for decision in decisions {
        let mut line = format!(
            "- [id {} | {}] {} — recommendation: {}",
            decision.id, decision.status, decision.question, decision.recommendation
        );
        if let Some(uncertainty) = decision.uncertainty {
            line.push_str("; uncertainty: ");
            line.push_str(&uncertainty);
        }
        let candidate = format!("{}\n{line}", lines.join("\n"));
        if candidate.chars().count() > max_chars {
            break;
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn decision_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerrenceDecision> {
    let constraints: String = row.get(3)?;
    let evidence: String = row.get(4)?;
    let alternatives: String = row.get(5)?;
    let context_item_ids: String = row.get(11)?;
    Ok(TerrenceDecision {
        id: row.get(0)?,
        question: row.get(1)?,
        status: row.get(2)?,
        constraints: serde_json::from_str(&constraints).unwrap_or_default(),
        evidence: serde_json::from_str(&evidence).unwrap_or_default(),
        alternatives: serde_json::from_str(&alternatives).unwrap_or_default(),
        recommendation: row.get(6)?,
        uncertainty: row.get(7)?,
        decision: row.get(8)?,
        outcome: row.get(9)?,
        conversation_id: row.get(10)?,
        context_item_ids: serde_json::from_str(&context_item_ids).unwrap_or_default(),
        provenance_source: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        decided_at: row.get(15)?,
        outcome_recorded_at: row.get(16)?,
    })
}

fn get_decision(conn: &Connection, id: &str) -> Result<Option<TerrenceDecision>, String> {
    conn.query_row(
        "SELECT id, question, status, constraints, evidence, alternatives, recommendation,
                uncertainty, decision, outcome, conversation_id, context_item_ids,
                provenance_source, created_at, updated_at, decided_at, outcome_recorded_at
         FROM terrence_decisions WHERE id = ?1",
        params![id],
        decision_from_row,
    )
    .optional()
    .map_err(|error| format!("Failed to read Amara decision: {error}"))
}

fn clean_evidence(values: Option<Vec<DecisionEvidence>>) -> Result<Vec<DecisionEvidence>, String> {
    let mut cleaned = Vec::new();
    for value in values
        .unwrap_or_default()
        .into_iter()
        .take(MAX_DECISION_ITEMS)
    {
        let confidence = match value.confidence {
            Some(confidence) if confidence.is_finite() && (0.0..=1.0).contains(&confidence) => {
                Some(confidence)
            }
            Some(_) => {
                return Err("Decision evidence confidence must be between 0 and 1.".to_string())
            }
            None => None,
        };
        cleaned.push(DecisionEvidence {
            detail: clean_required(&value.detail, "Decision evidence", MAX_DECISION_TEXT_CHARS)?,
            source: clean_optional(value.source, "Decision evidence source", 300)?,
            confidence,
        });
    }
    Ok(cleaned)
}

fn clean_alternatives(
    values: Option<Vec<DecisionAlternative>>,
) -> Result<Vec<DecisionAlternative>, String> {
    let mut cleaned = Vec::new();
    for value in values
        .unwrap_or_default()
        .into_iter()
        .take(MAX_DECISION_ITEMS)
    {
        cleaned.push(DecisionAlternative {
            label: clean_required(
                &value.label,
                "Decision alternative",
                MAX_DECISION_TEXT_CHARS,
            )?,
            tradeoffs: clean_optional(
                value.tradeoffs,
                "Decision alternative tradeoffs",
                MAX_DECISION_TEXT_CHARS,
            )?,
        });
    }
    Ok(cleaned)
}

pub(crate) fn create_terrence_decision(
    input: CreateTerrenceDecisionInput,
) -> Result<TerrenceDecision, String> {
    let conn = priority_db()?;
    create_terrence_decision_in(&conn, input)
}

fn create_terrence_decision_in(
    conn: &Connection,
    input: CreateTerrenceDecisionInput,
) -> Result<TerrenceDecision, String> {
    let question = clean_required(
        &input.question,
        "Decision question",
        MAX_DECISION_TEXT_CHARS,
    )?;
    let recommendation = clean_required(
        &input.recommendation,
        "Decision recommendation",
        MAX_DECISION_TEXT_CHARS,
    )?;
    let constraints = clean_list(input.constraints, "Decision constraint")?;
    let evidence = clean_evidence(input.evidence)?;
    let alternatives = clean_alternatives(input.alternatives)?;
    if alternatives.len() < 2 {
        return Err("A recorded decision needs at least two viable alternatives.".to_string());
    }
    if evidence.is_empty() {
        return Err("A recorded decision needs at least one piece of evidence.".to_string());
    }
    let uncertainty = clean_optional(
        input.uncertainty,
        "Decision uncertainty",
        MAX_DECISION_TEXT_CHARS,
    )?;
    let conversation_id = clean_optional(input.conversation_id, "Conversation ID", 200)?;
    let context_item_ids = clean_context_item_ids(input.context_item_ids)?;
    let provenance_source =
        clean_optional(input.provenance_source, "Decision provenance source", 120)?
            .unwrap_or_else(|| "user_or_terrence_conversation".to_string());
    let now = Utc::now().to_rfc3339();
    let id = match input.id {
        Some(id) => Uuid::parse_str(&clean_required(&id, "Decision ID", 120)?)
            .map_err(|_| "Decision ID must be a UUID.".to_string())?
            .to_string(),
        None => Uuid::new_v4().to_string(),
    };
    conn.execute(
        "INSERT INTO terrence_decisions (
            id, question, status, constraints, evidence, alternatives, recommendation,
            uncertainty, conversation_id, context_item_ids, provenance_source, created_at, updated_at
         ) VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            question,
            serde_json::to_string(&constraints).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&evidence).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&alternatives).unwrap_or_else(|_| "[]".to_string()),
            recommendation,
            uncertainty,
            conversation_id,
            serde_json::to_string(&context_item_ids).unwrap_or_else(|_| "[]".to_string()),
            provenance_source,
            now,
        ],
    )
    .map_err(|error| format!("Failed to record Amara decision: {error}"))?;
    get_decision(conn, &id)?.ok_or_else(|| "Recorded Amara decision could not be read.".to_string())
}

// Not registered in generate_handler! (no live caller); kept so it can be restored.
#[allow(dead_code)]
#[tauri::command]
pub fn record_terrence_decision(
    input: CreateTerrenceDecisionInput,
) -> Result<TerrenceDecision, String> {
    create_terrence_decision(input)
}

#[tauri::command]
#[specta::specta]
pub fn list_terrence_decisions(limit: Option<u32>) -> Result<Vec<TerrenceDecision>, String> {
    let conn = priority_db()?;
    ingest_hermes_decision_inbox(&conn);
    let mut statement = conn
        .prepare(
            "SELECT id, question, status, constraints, evidence, alternatives, recommendation,
                    uncertainty, decision, outcome, conversation_id, context_item_ids,
                    provenance_source, created_at, updated_at, decided_at, outcome_recorded_at
             FROM terrence_decisions ORDER BY updated_at DESC LIMIT ?1",
        )
        .map_err(|error| format!("Failed to prepare Amara decision list: {error}"))?;
    let decisions = statement
        .query_map(params![limit.unwrap_or(20).min(100)], decision_from_row)
        .map_err(|error| format!("Failed to read Amara decisions: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to collect Amara decisions: {error}"))?;
    Ok(decisions)
}

#[tauri::command]
pub fn resolve_terrence_decision(
    input: ResolveTerrenceDecisionInput,
) -> Result<TerrenceDecision, String> {
    let conn = priority_db()?;
    resolve_terrence_decision_in(&conn, input)
}

fn resolve_terrence_decision_in(
    conn: &Connection,
    input: ResolveTerrenceDecisionInput,
) -> Result<TerrenceDecision, String> {
    let id = clean_required(&input.id, "Decision ID", 120)?;
    let status = clean_required(&input.status, "Decision status", 40)?.to_ascii_lowercase();
    if !DECISION_STATUSES.contains(&status.as_str()) || status == "open" || status == "cancelled" {
        return Err("A decision can be resolved as decided, observing, or closed.".to_string());
    }
    let decision = clean_optional(input.decision, "Decision result", MAX_DECISION_TEXT_CHARS)?;
    let outcome = clean_optional(input.outcome, "Decision outcome", MAX_DECISION_TEXT_CHARS)?;
    if decision.is_none() && outcome.is_none() {
        return Err(
            "Recording a decision update needs a choice or an observed outcome.".to_string(),
        );
    }
    let now = Utc::now().to_rfc3339();
    let existing =
        get_decision(conn, &id)?.ok_or_else(|| "Amara decision was not found.".to_string())?;
    if matches!(existing.status.as_str(), "closed" | "cancelled") {
        return Err("This Amara decision is already closed.".to_string());
    }
    conn.execute(
        "UPDATE terrence_decisions
         SET status = ?1,
             decision = COALESCE(?2, decision),
             outcome = COALESCE(?3, outcome),
             updated_at = ?4,
             decided_at = CASE WHEN ?1 IN ('decided', 'observing', 'closed') THEN COALESCE(decided_at, ?4) ELSE decided_at END,
             outcome_recorded_at = CASE WHEN ?3 IS NOT NULL THEN ?4 ELSE outcome_recorded_at END
         WHERE id = ?5",
        params![status, decision, outcome, now, id],
    )
    .map_err(|error| format!("Failed to resolve Amara decision: {error}"))?;
    get_decision(conn, &id)?.ok_or_else(|| "Resolved Amara decision could not be read.".to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_current_terrence_priority_snapshot() -> Result<TerrencePrioritySnapshot, String> {
    current_priority_snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_item(
        id: &str,
        kind: &str,
        status: &str,
        title: &str,
    ) -> terrence_context::ContextItem {
        terrence_context::ContextItem {
            id: id.to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            content: None,
            status: status.to_string(),
            confidence: 1.0,
            provenance_source: "test".to_string(),
            provenance_ref: None,
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
            occurred_at: None,
            supersedes_id: None,
            deleted_at: None,
            metadata: json!({}),
        }
    }

    #[test]
    fn blocked_due_commitment_ranks_ahead_of_an_active_goal() {
        let mut commitment = context_item("commitment", "commitment", "blocked", "File paperwork");
        commitment.occurred_at = Some("2026-08-20T10:00:00Z".to_string());
        let goal = context_item("goal", "goal", "active", "Build long-term income");
        let now = DateTime::parse_from_rfc3339("2026-08-21T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let ranked = rank_priorities(vec![goal, commitment], Vec::new(), Vec::new(), now);
        assert_eq!(ranked[0].title, "File paperwork");
        assert!(ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("blocked")));
        assert!(ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("deadline")));
    }

    #[test]
    fn action_needing_attention_is_ranked_with_an_explanation() {
        let action = terrence_actions::TerrenceAction {
            id: "action-1".to_string(),
            tool_id: "system.open_url".to_string(),
            title: "Open application materials".to_string(),
            status: "needs_attention".to_string(),
            requires_approval: true,
            parameters: json!({}),
            conversation_id: None,
            context_item_ids: vec!["goal-1".to_string()],
            requested_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-21T00:00:00Z".to_string(),
            approved_at: None,
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            retry_of_action_id: None,
            retry_allowed: false,
            summary: "Open link".to_string(),
        };
        let ranked = rank_priorities(Vec::new(), vec![action], Vec::new(), Utc::now());
        assert_eq!(ranked[0].source_kind, "action");
        assert_eq!(ranked[0].action_id.as_deref(), Some("action-1"));
        assert!(ranked[0].reasons[0].contains("needs attention"));
    }

    #[test]
    fn blocked_project_with_a_next_action_becomes_a_priority() {
        let project = terrence_projects::TerrenceProject {
            id: "project-1".to_string(),
            title: "Leadership practice".to_string(),
            objective: "Build a reliable leadership habit.".to_string(),
            current_state: "The weekly review is not scheduled.".to_string(),
            status: "blocked".to_string(),
            milestones: vec![terrence_projects::TerrenceProjectMilestone {
                title: "Schedule review".to_string(),
                status: "blocked".to_string(),
                due_at: None,
            }],
            next_action: Some("Choose a weekly review time.".to_string()),
            blockers: vec!["No protected time.".to_string()],
            dependencies: vec![],
            context_item_ids: vec!["goal-1".to_string()],
            provenance_source: "test".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-21T00:00:00Z".to_string(),
            completed_at: None,
        };
        let ranked = rank_priorities(Vec::new(), Vec::new(), vec![project], Utc::now());
        assert_eq!(ranked[0].source_kind, "project");
        assert!(ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("blocker")));
    }

    #[test]
    fn priority_snapshot_is_reloaded_for_identical_inputs() {
        let conn = Connection::open_in_memory().unwrap();
        init_priority_db(&conn).unwrap();
        let items = vec![TerrencePriority {
            rank: 1,
            source_kind: "context".to_string(),
            source_id: "goal-1".to_string(),
            title: "Focus work".to_string(),
            source_status: "active".to_string(),
            score: 42,
            confidence: 0.8,
            reasons: vec!["active goal".to_string()],
            context_item_ids: vec!["goal-1".to_string()],
            action_id: None,
        }];
        let saved = persist_snapshot(
            &conn,
            "same-inputs",
            "2026-08-21T00:00:00Z".to_string(),
            &items,
        )
        .unwrap();
        let loaded = snapshot_by_fingerprint(&conn, "same-inputs")
            .unwrap()
            .unwrap();
        assert_eq!(saved.id, loaded.id);
        assert_eq!(loaded.items[0].title, "Focus work");
    }

    #[test]
    fn decision_requires_evidence_and_multiple_alternatives() {
        assert!(clean_evidence(None).unwrap().is_empty());
        assert_eq!(
            clean_alternatives(Some(vec![DecisionAlternative {
                label: "A".to_string(),
                tradeoffs: None
            }]))
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn decision_persists_choice_and_observed_outcome() {
        let conn = Connection::open_in_memory().unwrap();
        init_priority_db(&conn).unwrap();
        let recorded = create_terrence_decision_in(
            &conn,
            CreateTerrenceDecisionInput {
                id: None,
                question: "Which planning tool should I use?".to_string(),
                constraints: Some(vec!["Keep the weekly review under 30 minutes".to_string()]),
                evidence: Some(vec![DecisionEvidence {
                    detail: "The current process is skipped when it takes too long.".to_string(),
                    source: Some("user report".to_string()),
                    confidence: Some(0.9),
                }]),
                alternatives: Some(vec![
                    DecisionAlternative {
                        label: "Use a paper planner".to_string(),
                        tradeoffs: Some("Lower setup, less automation".to_string()),
                    },
                    DecisionAlternative {
                        label: "Use the Amara OS review".to_string(),
                        tradeoffs: Some("More setup, preserves context".to_string()),
                    },
                ]),
                recommendation: "Use the Amara OS review for a two-week trial.".to_string(),
                uncertainty: Some("Consistency is still unproven.".to_string()),
                conversation_id: Some("conversation-1".to_string()),
                context_item_ids: Some(vec!["goal-1".to_string()]),
                provenance_source: Some("test".to_string()),
            },
        )
        .unwrap();

        let resolved = resolve_terrence_decision_in(
            &conn,
            ResolveTerrenceDecisionInput {
                id: recorded.id,
                status: "observing".to_string(),
                decision: Some("Run the two-week Amara OS review trial.".to_string()),
                outcome: Some("First review was completed on time.".to_string()),
            },
        )
        .unwrap();

        assert_eq!(resolved.status, "observing");
        assert_eq!(
            resolved.decision.as_deref(),
            Some("Run the two-week Amara OS review trial.")
        );
        assert_eq!(
            resolved.outcome.as_deref(),
            Some("First review was completed on time.")
        );
        assert!(resolved.decided_at.is_some());
        assert!(resolved.outcome_recorded_at.is_some());
    }

    #[test]
    fn decision_resolution_requires_a_choice_or_outcome() {
        let conn = Connection::open_in_memory().unwrap();
        init_priority_db(&conn).unwrap();
        let error = resolve_terrence_decision_in(
            &conn,
            ResolveTerrenceDecisionInput {
                id: "missing".to_string(),
                status: "decided".to_string(),
                decision: None,
                outcome: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("choice or an observed outcome"));
    }

    #[test]
    fn imports_telegram_decision_into_the_canonical_store() {
        let conn = Connection::open_in_memory().unwrap();
        init_priority_db(&conn).unwrap();
        let directory =
            std::env::temp_dir().join(format!("nexus-decision-inbox-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let id = Uuid::new_v4().to_string();
        let path = directory.join(format!("decision-record-{id}.json"));
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "type": "record",
                "id": id,
                "question": "Which routine should I begin with?",
                "constraints": ["Keep it sustainable"],
                "evidence": [{"detail": "Mornings are the most consistent time.", "source": "telegram", "confidence": 0.8}],
                "alternatives": [
                    {"label": "Meditate first", "tradeoffs": "Calmer start"},
                    {"label": "Exercise first", "tradeoffs": "More activation"}
                ],
                "recommendation": "Begin with meditation for a two-week trial.",
                "uncertainty": "Energy levels may change the best sequence.",
                "provenanceSource": "hermes_telegram"
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(ingest_hermes_decision_inbox_in(&conn, &directory), 1);
        let imported = get_decision(&conn, &id).unwrap().unwrap();
        assert_eq!(imported.provenance_source, "hermes_telegram");
        assert_eq!(imported.alternatives.len(), 2);
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
