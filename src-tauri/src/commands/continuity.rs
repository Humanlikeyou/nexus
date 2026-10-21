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

/// Environment variable that overrides the continuity data directory
/// (useful for tests, dev profiles and portable installs).
pub(crate) const DATA_DIR_ENV: &str = "NEXUS_DATA_DIR";

fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(crate::app_paths::DATA_DIR_NAME)
}

const CONTEXT_FILE: &str = "terrance_context.json";

fn context_path() -> PathBuf {
    data_dir().join(CONTEXT_FILE)
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
    #[serde(default, deserialize_with = "lenient_hour")]
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

/// Accept any JSON value for `dailySummaryHour`. Valid hours (0-23, integer or
/// integral float/string) are kept; anything else (negative, >23, wrong type)
/// becomes `None` so the frontend default applies instead of failing the
/// whole context load.
fn lenient_hour<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let as_f64 = match &value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    Ok(as_f64
        .filter(|h| h.fract() == 0.0 && (0.0..=23.0).contains(h))
        .map(|h| h as u8))
}

#[tauri::command]
pub fn load_terrance_context() -> Result<TerranceContext, String> {
    load_context_from(&data_dir())
}

#[tauri::command]
pub fn save_terrance_context(context: TerranceContext) -> Result<(), String> {
    save_context_to(&data_dir(), &context)
}

/// Load the context from `dir/terrance_context.json` (default if missing).
pub(crate) fn load_context_from(dir: &std::path::Path) -> Result<TerranceContext, String> {
    let path = dir.join(CONTEXT_FILE);
    if !path.exists() {
        return Ok(TerranceContext::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// Atomically save the context to `dir/terrance_context.json`.
pub(crate) fn save_context_to(dir: &std::path::Path, context: &TerranceContext) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(context).map_err(|e| e.to_string())?;
    atomic_write(&dir.join(CONTEXT_FILE), json.as_bytes()).map_err(|e| e.to_string())
}

/// Write `bytes` to `path` atomically: write a uniquely named temp file in the
/// same directory, fsync it, then rename over the target (and fsync the dir on
/// Unix). A crash mid-write leaves either the old file or the new one, never a
/// truncated mix.
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .to_string();
    let tmp = dir.join(format!(".{}.{}.tmp", file_name, uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            if let Ok(d) = fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
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
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-continuity-{}-{}", tag, uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn hour_of(json: &str) -> Option<u8> {
        let raw = format!(r#"{{"activeGoals":["g"],"relevantMemory":[],"dailySummaryHour":{}}}"#, json);
        let ctx: TerranceContext = serde_json::from_str(&raw).expect("context must still load");
        assert_eq!(ctx.active_goals, vec!["g".to_string()]);
        ctx.daily_summary_hour
    }

    #[test]
    fn daily_summary_hour_out_of_range_does_not_fail_load() {
        assert_eq!(hour_of("7"), Some(7));
        assert_eq!(hour_of("0"), Some(0));
        assert_eq!(hour_of("23"), Some(23));
        assert_eq!(hour_of("\"8\""), Some(8));
        assert_eq!(hour_of("24"), None);
        assert_eq!(hour_of("300"), None);
        assert_eq!(hour_of("-1"), None);
        assert_eq!(hour_of("7.5"), None);
        assert_eq!(hour_of("null"), None);
        assert_eq!(hour_of("\"abc\""), None);
    }

    #[test]
    fn legacy_context_without_new_fields_loads() {
        let ctx: TerranceContext = serde_json::from_str(
            r#"{"activeGoals":[],"lastSessionSummary":null,"lastSessionTimestamp":null,"relevantMemory":[]}"#,
        )
        .unwrap();
        assert!(ctx.daily_summary_hour.is_none());
        assert!(ctx.last_midday_timestamp.is_none());
    }

    #[test]
    fn context_roundtrip_in_custom_dir() {
        let dir = temp_dir("roundtrip");
        assert!(load_context_from(&dir).unwrap().active_goals.is_empty());
        assert!(!dir.join(CONTEXT_FILE).exists(), "load must not create the file");
        let mut ctx = TerranceContext::default();
        ctx.active_goals = vec!["ship nexus-v2".into()];
        ctx.daily_summary_hour = Some(6);
        ctx.last_evening_timestamp = Some(42);
        save_context_to(&dir, &ctx).unwrap();
        let raw = fs::read_to_string(dir.join(CONTEXT_FILE)).unwrap();
        assert!(raw.contains("\"dailySummaryHour\": 6"));
        let back = load_context_from(&dir).unwrap();
        assert_eq!(back.active_goals, ctx.active_goals);
        assert_eq!(back.daily_summary_hour, Some(6));
        assert_eq!(back.last_evening_timestamp, Some(42));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_context_errors_and_is_not_overwritten_by_load() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(CONTEXT_FILE), b"{\"activeGoals\": [").unwrap();
        assert!(load_context_from(&dir).is_err());
        assert_eq!(fs::read(dir.join(CONTEXT_FILE)).unwrap(), b"{\"activeGoals\": [");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp_files() {
        let dir = temp_dir("atomic");
        let path = dir.join("terrance_context.json");
        fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"{\"new\":true}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"new\":true}");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_failure_keeps_original_file() {
        let dir = temp_dir("atomic-fail");
        // Target is a directory, so the rename fails; nothing should be clobbered.
        let target = dir.join("target");
        fs::create_dir_all(target.join("child")).unwrap();
        assert!(atomic_write(&target, b"x").is_err());
        assert!(target.join("child").is_dir());
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

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

/// Failing repros for TEST_PLAN bugs #1 and #9 (see /workspace/nexus-qa/TEST_PLAN.md).
#[cfg(test)]
mod repro_tests {
    use super::*;

    /// Bug #9: an out-of-range `dailySummaryHour` (u8) makes the whole context
    /// unreadable, which the frontend turns into an empty context and then a wipe.
    /// Intended: tolerate or clamp the bad field and keep the rest of the context.
    #[test]
    fn repro_out_of_range_summary_hour_does_not_fail_whole_context() {
        let raw = r#"{"activeGoals":["Ship v2"],"lastSessionSummary":null,
            "lastSessionTimestamp":null,"relevantMemory":["m"],"dailySummaryHour":300}"#;
        let parsed = serde_json::from_str::<TerranceContext>(raw);
        assert!(parsed.is_ok(), "context load failed on one bad field: {:?}", parsed.err());
    }

    /// Bug #1 (Rust side): `save_terrance_context` uses a plain `fs::write`, so a crash
    /// mid-write leaves a truncated file. There's no path seam to fault-inject today.
    #[test]
    #[ignore = "needs approved refactor: save_terrance_context_at(path) + atomic tmp+rename; assert no partial file / no leftover .tmp"]
    fn repro_context_save_is_atomic() {
        todo!("write via save_terrance_context_at(tmpdir) with an injected failure between write and rename");
    }
}
