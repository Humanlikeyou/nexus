/**
 * Terrance Continuity Commands
 *
 * Provides Tauri commands for:
 *  - read_identity_file     — loads config/terrance.identity.md from the app bundle
 *  - load_terrance_context  — loads active goals + last session summary from data dir
 *  - save_terrance_context  — persists context to data dir
 *  - store_session_log      — writes session summaries to memory/session_logs/
 */

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ─── Paths ─────────────────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.taisun.electron")
}

fn context_path() -> PathBuf {
    data_dir().join("terrance_context.json")
}

fn session_logs_dir() -> PathBuf {
    // Session logs stored alongside the app source so they can be committed to the repo
    // Falls back to data dir if the repo path isn't found
    let repo_path = std::env::current_exe()
        .ok()
        .and_then(|p| {
            // Walk up from the binary to find the project root (contains CLAUDE.md)
            let mut dir = p.parent()?.to_path_buf();
            for _ in 0..6 {
                if dir.join("CLAUDE.md").exists() {
                    return Some(dir.join("memory").join("session_logs"));
                }
                dir = dir.parent()?.to_path_buf();
            }
            None
        });
    repo_path.unwrap_or_else(|| data_dir().join("session_logs"))
}

// ─── Identity ─────────────────────────────────────────────────────────────────

/// Read the canonical Terrance identity file from config/terrance.identity.md.
/// Walks up from the binary to find the project root.
#[tauri::command]
pub fn read_identity_file() -> Result<String, String> {
    // Try to find the identity file relative to project root
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut dir = exe.parent().unwrap_or(&exe).to_path_buf();
    for _ in 0..8 {
        let candidate = dir.join("config").join("terrance.identity.md");
        if candidate.exists() {
            return fs::read_to_string(&candidate).map_err(|e| e.to_string());
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    // Fallback: check well-known project paths relative to home dir
    if let Some(home) = dirs::home_dir() {
        for rel in &["projects/nexus/config/terrance.identity.md"] {
            let candidate = home.join(rel);
            if candidate.exists() {
                return fs::read_to_string(&candidate).map_err(|e| e.to_string());
            }
        }
    }
    Err("Identity file not found. Ensure config/terrance.identity.md exists in the project root.".to_string())
}

// ─── Context Store ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerranceContext {
    pub active_goals: Vec<String>,
    pub last_session_summary: Option<String>,
    pub last_session_timestamp: Option<u64>,
    pub relevant_memory: Vec<String>,
}

#[tauri::command]
pub fn load_terrance_context() -> Result<TerranceContext, String> {
    let path = context_path();
    if !path.exists() {
        return Ok(TerranceContext::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_terrance_context(context: TerranceContext) -> Result<(), String> {
    let dir = data_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&context).map_err(|e| e.to_string())?;
    fs::write(context_path(), json).map_err(|e| e.to_string())
}

// ─── Session Log ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogEntry {
    pub id: String,
    pub timestamp: u64,
    pub message_count: u32,
    pub summary: String,
    pub provider: String,
    pub model: String,
}

#[tauri::command]
pub fn store_session_log(entry: SessionLogEntry) -> Result<(), String> {
    let dir = session_logs_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let filename = format!("{}.json", entry.id);
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_session_logs() -> Result<Vec<SessionLogEntry>, String> {
    let dir = session_logs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<SessionLogEntry> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| {
            fs::read_to_string(e.path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
        .collect();
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(entries)
}
