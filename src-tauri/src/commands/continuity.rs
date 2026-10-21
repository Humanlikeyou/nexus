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
    let repo_path = std::env::current_exe().ok().and_then(|p| {
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
    Err(
        "Identity file not found. Ensure config/terrance.identity.md exists in the project root."
            .to_string(),
    )
}

/// Read the operational profile from config/terrance.profile.md.
/// Same lookup strategy as read_identity_file.
#[tauri::command]
pub fn read_profile_file() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut dir = exe.parent().unwrap_or(&exe).to_path_buf();
    for _ in 0..8 {
        let candidate = dir.join("config").join("terrance.profile.md");
        if candidate.exists() {
            return fs::read_to_string(&candidate).map_err(|e| e.to_string());
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    if let Some(home) = dirs::home_dir() {
        for rel in &["projects/nexus/config/terrance.profile.md"] {
            let candidate = home.join(rel);
            if candidate.exists() {
                return fs::read_to_string(&candidate).map_err(|e| e.to_string());
            }
        }
    }
    Err("Profile file not found.".to_string())
}

// ─── Context Store ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerranceContext {
    pub active_goals: Vec<String>,
    pub last_session_summary: Option<String>,
    pub last_session_timestamp: Option<u64>,
    pub relevant_memory: Vec<String>,
    #[serde(default)]
    pub last_consolidation_timestamp: Option<u64>,
    #[serde(default)]
    pub last_reasoning_timestamp: Option<u64>,
    #[serde(default)]
    pub last_daily_summary_timestamp: Option<u64>,
    #[serde(default)]
    pub daily_summary_hour: Option<u8>,
    #[serde(default)]
    pub last_health_check_timestamp: Option<u64>,
    #[serde(default)]
    pub last_prune_timestamp: Option<u64>,
    #[serde(default)]
    pub last_telegram_update_id: Option<i64>,
    #[serde(default)]
    pub last_midday_timestamp: Option<u64>,
    #[serde(default)]
    pub last_evening_timestamp: Option<u64>,
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

// ─── Archive / Prune ──────────────────────────────────────────────────────────

fn archive_dir() -> PathBuf {
    session_logs_dir()
        .parent()
        .unwrap_or(&session_logs_dir())
        .join("archive")
}

/// Move session logs older than `days_threshold` days to memory/archive/.
/// Returns the number of files moved.
#[tauri::command]
pub fn archive_old_session_logs(days_threshold: u32) -> Result<u32, String> {
    let dir = session_logs_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let archive = archive_dir();
    fs::create_dir_all(&archive).map_err(|e| e.to_string())?;

    let cutoff_ms = (days_threshold as u64) * 24 * 60 * 60 * 1000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let mut moved = 0u32;

    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.extension().map(|x| x == "json").unwrap_or(false) {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(log) = serde_json::from_str::<SessionLogEntry>(&content) {
                    if now - log.timestamp > cutoff_ms {
                        let dest = archive.join(entry.file_name());
                        if fs::rename(&path, &dest).is_ok() {
                            moved += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(moved)
}

/// Trim the relevantMemory array in terrance_context.json to at most `max_items` entries.
#[tauri::command]
pub fn prune_context_memory(max_items: u32) -> Result<u32, String> {
    let path = context_path();
    if !path.exists() {
        return Ok(0);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut ctx: TerranceContext = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let before = ctx.relevant_memory.len() as u32;
    let max = max_items as usize;
    if ctx.relevant_memory.len() > max {
        ctx.relevant_memory.truncate(max);
    }
    let after = ctx.relevant_memory.len() as u32;
    let json = serde_json::to_string_pretty(&ctx).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(before - after)
}

// ─── Custom Cron Jobs ───────────────────────────────────────────────────────

fn custom_jobs_path() -> PathBuf {
    data_dir().join("custom_cron_jobs.json")
}

/// Load custom cron jobs from disk. Returns JSON string.
#[tauri::command]
pub fn load_custom_jobs() -> Result<String, String> {
    let path = custom_jobs_path();
    if !path.exists() {
        return Ok("[]".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Save custom cron jobs to disk. Accepts raw JSON string.
#[tauri::command]
pub fn save_custom_jobs(data: String) -> Result<(), String> {
    let dir = data_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(custom_jobs_path(), data).map_err(|e| e.to_string())
}

// ─── Session Insights ───────────────────────────────────────────────────────

/// Get aggregated session insights: total sessions, tokens, providers, daily breakdown.
#[tauri::command]
pub fn get_session_insights() -> Result<String, String> {
    let dir = session_logs_dir();
    if !dir.exists() {
        return Ok("{}".to_string());
    }

    let entries: Vec<SessionLogEntry> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| {
            fs::read_to_string(e.path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
        .collect();

    let total_sessions = entries.len();
    let total_messages: u32 = entries.iter().map(|e| e.message_count).sum();

    // Provider breakdown
    let mut by_provider: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut by_model: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for entry in &entries {
        *by_provider.entry(entry.provider.clone()).or_insert(0) += 1;
        *by_model.entry(entry.model.clone()).or_insert(0) += 1;
    }

    // Daily breakdown (last 30 days)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let thirty_days_ms = 30u64 * 24 * 60 * 60 * 1000;
    let recent: Vec<&SessionLogEntry> = entries
        .iter()
        .filter(|e| now - e.timestamp < thirty_days_ms)
        .collect();

    let mut daily: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for entry in &recent {
        let date = chrono_date_from_ms(entry.timestamp);
        *daily.entry(date).or_insert(0) += 1;
    }

    let insights = serde_json::json!({
        "totalSessions": total_sessions,
        "totalMessages": total_messages,
        "byProvider": by_provider,
        "byModel": by_model,
        "dailyBreakdown": daily,
        "recentSessionCount": recent.len(),
    });

    serde_json::to_string_pretty(&insights).map_err(|e| e.to_string())
}

fn chrono_date_from_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let naive = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    naive.format("%Y-%m-%d").to_string()
}

// ─── Activity Log ────────────────────────────────────────────────────────────

fn activity_log_dir() -> PathBuf {
    // Activity logs live alongside session_logs in the data dir
    data_dir().join("activity_log")
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub name: String,
    pub status: String,
    pub started_at: u64,
    pub completed_at: u64,
    pub duration_ms: u64,
    pub detail: Option<String>,
    pub outcome: Option<String>,
}

fn redact_marker_value(text: &str, marker: &str, terminators: &[char]) -> String {
    let lower = text.to_ascii_lowercase();
    let marker_lower = marker.to_ascii_lowercase();
    let mut cursor = 0;
    let mut redacted = String::with_capacity(text.len());

    while let Some(relative_start) = lower[cursor..].find(&marker_lower) {
        let marker_start = cursor + relative_start;
        let value_start = marker_start + marker.len();
        let value_end = text[value_start..]
            .find(|character: char| terminators.contains(&character))
            .map(|relative_end| value_start + relative_end)
            .unwrap_or(text.len());
        redacted.push_str(&text[cursor..value_start]);
        redacted.push_str("[REDACTED]");
        cursor = value_end;
    }
    redacted.push_str(&text[cursor..]);
    redacted
}

pub(crate) fn redact_sensitive_text(text: &str) -> String {
    let mut redacted = redact_marker_value(
        text,
        "https://api.telegram.org/bot",
        &['/', '?', '#', ' ', '\n', '\r', '\"', '\''],
    );
    for marker in [
        "authorization: bearer ",
        "access_token=",
        "api_key=",
        "api_key:",
        "bot token: ",
        "token=",
        "token:",
    ] {
        redacted = redact_marker_value(&redacted, marker, &[' ', '\n', '\r', '\"', '\'', '&']);
    }
    redacted
}

fn redact_activity_entry(mut entry: ActivityEntry) -> ActivityEntry {
    entry.detail = entry.detail.map(|detail| redact_sensitive_text(&detail));
    entry.outcome = entry.outcome.map(|outcome| redact_sensitive_text(&outcome));
    entry
}

/// Store an activity entry to disk as a JSON file.
#[tauri::command]
pub fn store_activity_entry(entry: ActivityEntry) -> Result<(), String> {
    let dir = activity_log_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let entry = redact_activity_entry(entry);
    let filename = format!("{}.json", entry.id);
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// List recent activity entries. Returns entries from the last 24 hours
/// or up to `limit` entries, whichever yields more results.
#[tauri::command]
pub fn list_activity_entries(limit: Option<u32>) -> Result<Vec<ActivityEntry>, String> {
    let dir = activity_log_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let max = limit.unwrap_or(100) as usize;

    let mut entries: Vec<ActivityEntry> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| {
            let path = e.path();
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .map(|entry: ActivityEntry| {
                    let redacted = redact_activity_entry(entry.clone());
                    if redacted != entry {
                        if let Ok(json) = serde_json::to_string_pretty(&redacted) {
                            let _ = fs::write(&path, json);
                        }
                    }
                    redacted
                })
        })
        .collect();

    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    entries.truncate(max);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive_text;

    #[test]
    fn redacts_telegram_tokens_without_hiding_the_endpoint() {
        let text = "Network error at https://api.telegram.org/bot123456:secret/sendMessage";
        assert_eq!(
            redact_sensitive_text(text),
            "Network error at https://api.telegram.org/bot[REDACTED]/sendMessage"
        );
    }

    #[test]
    fn redacts_common_credential_values() {
        assert_eq!(
            redact_sensitive_text("Authorization: Bearer private-value failed"),
            "Authorization: Bearer [REDACTED] failed"
        );
        assert_eq!(
            redact_sensitive_text("https://example.com?access_token=private-value&x=1"),
            "https://example.com?access_token=[REDACTED]&x=1"
        );
    }
}

/// Prune activity log entries older than `days` days. Returns count removed.
#[tauri::command]
pub fn prune_activity_log(days: u32) -> Result<u32, String> {
    let dir = activity_log_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let cutoff_ms = (days as u64) * 24 * 60 * 60 * 1000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let mut removed = 0u32;

    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.extension().map(|x| x == "json").unwrap_or(false) {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(log) = serde_json::from_str::<ActivityEntry>(&content) {
                    if now - log.started_at > cutoff_ms {
                        if fs::remove_file(&path).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(removed)
}

// ─── Subagents ──────────────────────────────────────────────────────────────

fn subagents_path() -> PathBuf {
    data_dir().join("subagents.json")
}

/// Load subagents from disk. Returns JSON string.
#[tauri::command]
pub fn load_subagents() -> Result<String, String> {
    let path = subagents_path();
    if !path.exists() {
        return Ok("[]".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Save subagents to disk. Accepts raw JSON string.
#[tauri::command]
pub fn save_subagents(data: String) -> Result<(), String> {
    let dir = data_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(subagents_path(), data).map_err(|e| e.to_string())
}
