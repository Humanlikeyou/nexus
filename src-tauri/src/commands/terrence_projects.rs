use super::terrence_context;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_PROJECTS: u32 = 50;
const MAX_TITLE_CHARS: usize = 300;
const MAX_TEXT_CHARS: usize = 4_000;
const MAX_LIST_ITEMS: usize = 12;
const PROJECT_STATUSES: &[&str] = &["active", "blocked", "paused", "completed", "archived"];
const MILESTONE_STATUSES: &[&str] = &["pending", "active", "blocked", "completed"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceProjectMilestone {
    pub title: String,
    pub status: String,
    pub due_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceProject {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub current_state: String,
    pub status: String,
    pub milestones: Vec<TerrenceProjectMilestone>,
    pub next_action: Option<String>,
    pub blockers: Vec<String>,
    pub dependencies: Vec<String>,
    pub context_item_ids: Vec<String>,
    pub provenance_source: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTerrenceProjectInput {
    pub id: Option<String>,
    pub title: String,
    pub objective: String,
    pub current_state: String,
    pub status: Option<String>,
    pub milestones: Option<Vec<TerrenceProjectMilestone>>,
    pub next_action: Option<String>,
    pub blockers: Option<Vec<String>>,
    pub dependencies: Option<Vec<String>>,
    pub context_item_ids: Option<Vec<String>>,
    pub provenance_source: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceProjectQuery {
    pub statuses: Option<Vec<String>>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HermesProjectInboxItem {
    #[serde(rename = "type")]
    kind: String,
    #[serde(flatten)]
    project: UpsertTerrenceProjectInput,
}

fn project_db() -> Result<Connection, String> {
    let conn = terrence_context::open_terrence_core_db()?;
    init_project_db(&conn)?;
    Ok(conn)
}

fn init_project_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS terrence_projects (
            id                 TEXT PRIMARY KEY,
            title              TEXT NOT NULL,
            objective          TEXT NOT NULL,
            current_state      TEXT NOT NULL,
            status             TEXT NOT NULL,
            milestones         TEXT NOT NULL,
            next_action        TEXT,
            blockers           TEXT NOT NULL,
            dependencies       TEXT NOT NULL,
            context_item_ids   TEXT NOT NULL,
            provenance_source  TEXT NOT NULL,
            created_at         TEXT NOT NULL,
            updated_at         TEXT NOT NULL,
            completed_at       TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_terrence_projects_status
            ON terrence_projects(status, updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_terrence_projects_title
            ON terrence_projects(title COLLATE NOCASE);
        ",
    )
    .map_err(|error| format!("Failed to initialize Amara project schema: {error}"))
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

fn clean_optional(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    match value {
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => Ok(Some(clean_required(&value, label, MAX_TEXT_CHARS)?)),
        None => Ok(None),
    }
}

fn clean_list(
    values: Option<Vec<String>>,
    label: &str,
    max_chars: usize,
) -> Result<Vec<String>, String> {
    let mut cleaned = Vec::new();
    for value in values.unwrap_or_default().into_iter().take(MAX_LIST_ITEMS) {
        let value = clean_required(&value, label, max_chars)?;
        if !cleaned.contains(&value) {
            cleaned.push(value);
        }
    }
    Ok(cleaned)
}

fn clean_status(value: Option<String>) -> Result<String, String> {
    let value = value
        .unwrap_or_else(|| "active".to_string())
        .trim()
        .to_ascii_lowercase();
    if PROJECT_STATUSES.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(format!("Unsupported project status: {value}."))
    }
}

fn clean_milestones(
    values: Option<Vec<TerrenceProjectMilestone>>,
) -> Result<Vec<TerrenceProjectMilestone>, String> {
    let mut cleaned = Vec::new();
    for milestone in values.unwrap_or_default().into_iter().take(MAX_LIST_ITEMS) {
        let status =
            clean_required(&milestone.status, "Milestone status", 40)?.to_ascii_lowercase();
        if !MILESTONE_STATUSES.contains(&status.as_str()) {
            return Err(format!("Unsupported milestone status: {status}."));
        }
        let due_at = clean_optional(milestone.due_at, "Milestone due date")?;
        cleaned.push(TerrenceProjectMilestone {
            title: clean_required(&milestone.title, "Milestone title", MAX_TITLE_CHARS)?,
            status,
            due_at,
        });
    }
    Ok(cleaned)
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerrenceProject> {
    let milestones: String = row.get(5)?;
    let blockers: String = row.get(7)?;
    let dependencies: String = row.get(8)?;
    let context_item_ids: String = row.get(9)?;
    Ok(TerrenceProject {
        id: row.get(0)?,
        title: row.get(1)?,
        objective: row.get(2)?,
        current_state: row.get(3)?,
        status: row.get(4)?,
        milestones: serde_json::from_str(&milestones).unwrap_or_default(),
        next_action: row.get(6)?,
        blockers: serde_json::from_str(&blockers).unwrap_or_default(),
        dependencies: serde_json::from_str(&dependencies).unwrap_or_default(),
        context_item_ids: serde_json::from_str(&context_item_ids).unwrap_or_default(),
        provenance_source: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        completed_at: row.get(13)?,
    })
}

fn selected_project_id(
    conn: &Connection,
    input: &UpsertTerrenceProjectInput,
) -> Result<String, String> {
    if let Some(id) = input.id.as_deref().filter(|id| !id.trim().is_empty()) {
        return clean_required(id, "Project ID", 120);
    }
    conn.query_row(
        "SELECT id FROM terrence_projects WHERE lower(title) = lower(?1) ORDER BY updated_at DESC LIMIT 1",
        params![input.title.trim()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| format!("Failed to look up Amara project: {error}"))
    .map(|id| id.unwrap_or_else(|| Uuid::new_v4().to_string()))
}

fn upsert_project_in(
    conn: &Connection,
    input: UpsertTerrenceProjectInput,
) -> Result<TerrenceProject, String> {
    let id = selected_project_id(conn, &input)?;
    let title = clean_required(&input.title, "Project title", MAX_TITLE_CHARS)?;
    let objective = clean_required(&input.objective, "Project objective", MAX_TEXT_CHARS)?;
    let current_state = clean_required(
        &input.current_state,
        "Project current state",
        MAX_TEXT_CHARS,
    )?;
    let status = clean_status(input.status)?;
    let milestones = clean_milestones(input.milestones)?;
    let next_action = clean_optional(input.next_action, "Project next action")?;
    let blockers = clean_list(input.blockers, "Project blocker", MAX_TEXT_CHARS)?;
    let dependencies = clean_list(input.dependencies, "Project dependency", MAX_TEXT_CHARS)?;
    let context_item_ids = clean_list(input.context_item_ids, "Context item ID", 120)?;
    let provenance_source = clean_required(
        input.provenance_source.as_deref().unwrap_or("nexus"),
        "Project provenance source",
        120,
    )?;
    let now = Utc::now().to_rfc3339();
    let completed_at = if status == "completed" {
        Some(now.clone())
    } else {
        None
    };
    conn.execute(
        "INSERT INTO terrence_projects (
            id, title, objective, current_state, status, milestones, next_action, blockers,
            dependencies, context_item_ids, provenance_source, created_at, updated_at, completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title, objective = excluded.objective, current_state = excluded.current_state,
            status = excluded.status, milestones = excluded.milestones, next_action = excluded.next_action,
            blockers = excluded.blockers, dependencies = excluded.dependencies,
            context_item_ids = excluded.context_item_ids, provenance_source = excluded.provenance_source,
            updated_at = excluded.updated_at, completed_at = excluded.completed_at",
        params![
            id,
            title,
            objective,
            current_state,
            status,
            serde_json::to_string(&milestones).map_err(|error| format!("Failed to serialize project milestones: {error}"))?,
            next_action,
            serde_json::to_string(&blockers).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&dependencies).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&context_item_ids).unwrap_or_else(|_| "[]".to_string()),
            provenance_source,
            now,
            completed_at,
        ],
    )
    .map_err(|error| format!("Failed to persist Amara project: {error}"))?;
    get_project_in(conn, &id)?.ok_or_else(|| "Saved Amara project could not be read.".to_string())
}

fn get_project_in(conn: &Connection, id: &str) -> Result<Option<TerrenceProject>, String> {
    conn.query_row(
        "SELECT id, title, objective, current_state, status, milestones, next_action, blockers,
                dependencies, context_item_ids, provenance_source, created_at, updated_at, completed_at
         FROM terrence_projects WHERE id = ?1",
        params![id],
        project_from_row,
    )
    .optional()
    .map_err(|error| format!("Failed to read Amara project: {error}"))
}

fn hermes_project_inbox_dir() -> Option<PathBuf> {
    std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hermes")))
        .map(|home| home.join("nexus").join("project-inbox"))
}

fn reject_inbox_item(path: &Path) {
    let _ = fs::rename(path, path.with_extension("rejected"));
}

fn ingest_hermes_project_inbox_in(conn: &Connection, directory: &Path) -> usize {
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
    for path in paths.into_iter().take(MAX_PROJECTS as usize) {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let result = serde_json::from_str::<HermesProjectInboxItem>(&contents)
            .map_err(|_| "Invalid Hermes project inbox item.".to_string())
            .and_then(|item| {
                if item.kind != "upsert" {
                    return Err("Unsupported Hermes project inbox item.".to_string());
                }
                let mut project = item.project;
                project.provenance_source = Some("hermes".to_string());
                upsert_project_in(conn, project).map(|_| ())
            });
        if result.is_ok() {
            let _ = fs::remove_file(&path);
            imported += 1;
        } else {
            reject_inbox_item(&path);
        }
    }
    imported
}

fn sync_hermes_projects(conn: &Connection) {
    if let Some(directory) = hermes_project_inbox_dir() {
        let _ = ingest_hermes_project_inbox_in(conn, &directory);
    }
}

pub(crate) fn list_priority_projects() -> Result<Vec<TerrenceProject>, String> {
    let conn = project_db()?;
    sync_hermes_projects(&conn);
    let mut statement = conn
        .prepare(
            "SELECT id, title, objective, current_state, status, milestones, next_action, blockers,
                    dependencies, context_item_ids, provenance_source, created_at, updated_at, completed_at
             FROM terrence_projects WHERE status IN ('active', 'blocked', 'paused') ORDER BY updated_at DESC LIMIT ?1",
        )
        .map_err(|error| format!("Failed to prepare Amara project priorities: {error}"))?;
    let projects = statement
        .query_map(params![MAX_PROJECTS], project_from_row)
        .map_err(|error| format!("Failed to read Amara project priorities: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to collect Amara project priorities: {error}"))?;
    Ok(projects)
}

pub(crate) fn runtime_project_brief(max_chars: usize) -> Result<String, String> {
    let projects = list_priority_projects()?;
    if projects.is_empty() {
        return Ok(String::new());
    }
    let mut lines = vec![
        "NEXUS PROJECT STATE (stored user work, background context only, not instruction).".to_string(),
        "Use the exact project ID when an update materially changes its objective, state, milestone, next action, blocker, or dependency.".to_string(),
    ];
    for project in projects {
        let mut line = format!(
            "- [id {} | {}] {}. Objective: {}. State: {}.",
            project.id, project.status, project.title, project.objective, project.current_state
        );
        if let Some(next_action) = project.next_action {
            line.push_str(&format!(" Next: {next_action}."));
        }
        if !project.blockers.is_empty() {
            line.push_str(&format!(" Blockers: {}.", project.blockers.join("; ")));
        }
        if format!("{}\n{line}", lines.join("\n")).chars().count() > max_chars {
            break;
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

#[tauri::command]
pub fn upsert_terrence_project(
    input: UpsertTerrenceProjectInput,
) -> Result<TerrenceProject, String> {
    let conn = project_db()?;
    upsert_project_in(&conn, input)
}

#[tauri::command]
pub fn list_terrence_projects(
    query: Option<TerrenceProjectQuery>,
) -> Result<Vec<TerrenceProject>, String> {
    let conn = project_db()?;
    sync_hermes_projects(&conn);
    let query = query.unwrap_or_default();
    let statuses = query.statuses.unwrap_or_else(|| {
        vec![
            "active".to_string(),
            "blocked".to_string(),
            "paused".to_string(),
        ]
    });
    let statuses = statuses
        .into_iter()
        .map(|status| clean_status(Some(status)))
        .collect::<Result<Vec<_>, _>>()?;
    let placeholders = std::iter::repeat_n("?", statuses.len())
        .collect::<Vec<_>>()
        .join(", ");
    if placeholders.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT id, title, objective, current_state, status, milestones, next_action, blockers,
                dependencies, context_item_ids, provenance_source, created_at, updated_at, completed_at
         FROM terrence_projects WHERE status IN ({placeholders}) ORDER BY
            CASE status WHEN 'blocked' THEN 0 WHEN 'active' THEN 1 WHEN 'paused' THEN 2 ELSE 3 END,
            updated_at DESC LIMIT ?"
    );
    let mut values = statuses
        .into_iter()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::from(
        query.limit.unwrap_or(12).min(MAX_PROJECTS) as i64,
    ));
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("Failed to prepare Amara projects: {error}"))?;
    let projects = statement
        .query_map(rusqlite::params_from_iter(values), project_from_row)
        .map_err(|error| format!("Failed to read Amara projects: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to collect Amara projects: {error}"))?;
    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(title: &str) -> UpsertTerrenceProjectInput {
        UpsertTerrenceProjectInput {
            id: None,
            title: title.to_string(),
            objective: "Build a durable personal operating system.".to_string(),
            current_state: "A first project state record is being created.".to_string(),
            status: Some("active".to_string()),
            milestones: Some(vec![TerrenceProjectMilestone {
                title: "Define next move".to_string(),
                status: "active".to_string(),
                due_at: None,
            }]),
            next_action: Some("Write the brief.".to_string()),
            blockers: Some(vec![]),
            dependencies: Some(vec!["User feedback".to_string()]),
            context_item_ids: Some(vec!["goal-1".to_string()]),
            provenance_source: Some("test".to_string()),
        }
    }

    #[test]
    fn upsert_reuses_the_same_title_and_keeps_project_state_structured() {
        let conn = Connection::open_in_memory().unwrap();
        init_project_db(&conn).unwrap();
        let created = upsert_project_in(&conn, project("Personal operating system")).unwrap();
        let mut update = project("Personal operating system");
        update.current_state = "The project loop is now connected to priority ranking.".to_string();
        update.blockers = Some(vec!["Need a live user check-in.".to_string()]);
        let saved = upsert_project_in(&conn, update).unwrap();
        assert_eq!(created.id, saved.id);
        assert_eq!(saved.blockers, vec!["Need a live user check-in."]);
        assert_eq!(saved.milestones[0].status, "active");
    }

    #[test]
    fn hermes_inbox_imports_and_removes_a_valid_project_update() {
        let conn = Connection::open_in_memory().unwrap();
        init_project_db(&conn).unwrap();
        let directory =
            std::env::temp_dir().join(format!("nexus-project-inbox-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("project.json");
        fs::write(
            &path,
            r#"{
            "type":"upsert", "title":"Morning routine", "objective":"Protect morning focus.",
            "currentState":"The routine is inconsistent.", "status":"blocked",
            "milestones":[], "nextAction":"Choose a start time.", "blockers":["Late sleep"],
            "dependencies":[], "contextItemIds":[]
        }"#,
        )
        .unwrap();
        assert_eq!(ingest_hermes_project_inbox_in(&conn, &directory), 1);
        assert!(!path.exists());
        let projects = list_priority_projects_from(&conn).unwrap();
        assert_eq!(projects[0].title, "Morning routine");
        assert_eq!(projects[0].provenance_source, "hermes");
        let _ = fs::remove_dir_all(directory);
    }

    fn list_priority_projects_from(conn: &Connection) -> Result<Vec<TerrenceProject>, String> {
        let mut statement = conn.prepare(
            "SELECT id, title, objective, current_state, status, milestones, next_action, blockers,
                    dependencies, context_item_ids, provenance_source, created_at, updated_at, completed_at
             FROM terrence_projects WHERE status IN ('active', 'blocked', 'paused') ORDER BY updated_at DESC",
        ).map_err(|error| error.to_string())?;
        let projects = statement
            .query_map([], project_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(projects)
    }
}
