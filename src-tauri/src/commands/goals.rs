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
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Optional link to a business-ledger venture (decision 0037). Old files without it load as `None`.
    /// Set only through `goal_venture_set`, which checks the ledger and logs History + Undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venture_id: Option<String>,
    /// Habits only (decision 0045 item 3): why this habit matters to Taisun, in his words. Old files load as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Habits only: the `HH:MM` JST time Needs you may start asking whether today is marked. `None` = 20:00.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_at: Option<String>,
    /// Habits only: a paused habit is not asked about and does not count as missed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
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
    let stamp = crate::commands::shared::time::now_jst().to_rfc3339();
    for g in goals.iter_mut() {
        // A row written by the console carries no timestamps; the habit tracker needs the day it was created.
        let old_row = old.iter().find(|o| o.id == g.id);
        if g.created_at.is_empty() {
            g.created_at = old_row.map_or_else(|| stamp.clone(), |o| o.created_at.clone());
        }
        if g.updated_at.is_empty() {
            g.updated_at = stamp.clone();
        }
        g.remind_at = g.remind_at.take().and_then(|t| valid_hhmm(&t));
        g.why = g
            .why
            .take()
            .map(|w| {
                w.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect::<String>()
            })
            .filter(|w| !w.is_empty());
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

/// `HH:MM` (24 h) or nothing: a reminder time that is not a real clock time is dropped, never stored.
pub(crate) fn valid_hhmm(t: &str) -> Option<String> {
    let t = t.trim();
    let (h, m) = t.split_once(':')?;
    let ok = h.len() == 2
        && m.len() == 2
        && h.parse::<u32>().is_ok_and(|h| h < 24)
        && m.parse::<u32>().is_ok_and(|m| m < 60);
    ok.then(|| t.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_goal(goal_id: String) -> Result<Vec<Goal>, String> {
    let mut goals = read_goals()?;
    goals.retain(|g| g.id != goal_id);
    save_goals(goals.clone())?;
    Ok(goals)
}
