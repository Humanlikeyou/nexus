use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn goals_path() -> PathBuf {
    nexus_data_dir().join("goals.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub category: String,
    pub target: f64,
    pub current: f64,
    pub unit: String,
    pub deadline: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Optional link to a business-ledger venture (decision 0037). Old files without it load as `None`.
    /// Set only through `goal_venture_set`, which checks the ledger and logs History + Undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venture_id: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn read_goals() -> Result<Vec<Goal>, String> {
    let path = goals_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path).map_err(|e| format!("Failed to read goals: {}", e))?;
    let goals: Vec<Goal> =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse goals: {}", e))?;
    Ok(goals)
}

#[tauri::command]
#[specta::specta]
pub fn save_goals(goals: Vec<Goal>) -> Result<(), String> {
    save_goals_at(&goals_path(), goals)
}

/// The venture link never comes from the caller: each goal takes `venture_id` from the goal with the same id
/// on disk (None when there is no match or the file can't be read). Only `goal_venture_set` changes a link.
pub(crate) fn save_goals_at(path: &std::path::Path, goals: Vec<Goal>) -> Result<(), String> {
    let old: Vec<Goal> = fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default();
    let mut goals = goals;
    for g in goals.iter_mut() {
        g.venture_id = old
            .iter()
            .find(|o| o.id == g.id)
            .and_then(|o| o.venture_id.clone());
    }
    let dir = path.parent().unwrap();
    crate::app_paths::create_private_dir_all(dir)
        .map_err(|e| format!("Failed to create dir: {}", e))?;
    let data =
        serde_json::to_string_pretty(&goals).map_err(|e| format!("Failed to serialize: {}", e))?;
    crate::app_paths::write_private(path, data)
        .map_err(|e| format!("Failed to write goals: {}", e))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn delete_goal(goal_id: String) -> Result<Vec<Goal>, String> {
    let mut goals = read_goals()?;
    goals.retain(|g| g.id != goal_id);
    save_goals(goals.clone())?;
    Ok(goals)
}
