/**
 * Amara Continuity Commands
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

/// Shared app-data location (com.taisun.nexus, or `$NEXUS_DATA_DIR`). Legacy
/// com.taisun.electron data is copied in, non-destructively, by
/// app_paths::migrate_legacy_data_dir() at startup before any command runs.
fn data_dir() -> PathBuf {
    crate::app_paths::data_dir()
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

/// Read the canonical Amara identity file from config/terrance.identity.md.
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
        {
            let rel = &"projects/nexus/config/terrance.identity.md";
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
        {
            let rel = &"projects/nexus/config/terrance.profile.md";
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
    /// Daily Summary slot (ms) recorded as "Skipped (missed window)" so the skip isn't re-logged every tick.
    #[serde(default)]
    pub daily_summary_skipped_slot: Option<u64>,
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
pub(crate) fn save_context_to(
    dir: &std::path::Path,
    context: &TerranceContext,
) -> Result<(), String> {
    crate::app_paths::create_private_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(context).map_err(|e| e.to_string())?;
    atomic_write(&dir.join(CONTEXT_FILE), json.as_bytes()).map_err(|e| e.to_string())
}

/// Write `bytes` to `path` atomically: write a uniquely named temp file in the
/// same directory, fsync it, then rename over the target (and fsync the dir on
/// Unix). A crash mid-write leaves either the old file or the new one, never a
/// truncated mix.
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with_mode(path, bytes, None)
}

/// Like [`atomic_write`], but the result always has exactly `mode` (Unix)
/// instead of inheriting the original file's mode. Use for files holding
/// secrets, e.g. `atomic_write_with_mode(p, b, Some(0o600))`.
pub(crate) fn atomic_write_with_mode(
    path: &std::path::Path,
    bytes: &[u8],
    forced_mode: Option<u32>,
) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
        })?
        .to_string_lossy()
        .to_string();
    // Refuse to replace a symlink (it could point anywhere); callers must
    // resolve and validate real paths themselves.
    if fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to replace symlink {}", path.display()),
        ));
    }
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        file_name,
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // Never widen permissions: create the temp file 0600, then match the
        // original's mode (if one exists) before it replaces the target.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = match forced_mode {
                Some(m) => m & 0o777,
                None => fs::metadata(path)
                    // Only rwx bits: never carry setuid/setgid/sticky over.
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0o600),
            };
            f.set_permissions(fs::Permissions::from_mode(mode))?;
        }
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
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    super::path_safety::validate_name(&entry.id)?;
    let path = super::path_safety::safe_join(&dir, &format!("{}.json", entry.id))?;
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&path, json).map_err(|e| e.to_string())
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
    crate::app_paths::create_private_dir_all(&archive).map_err(|e| e.to_string())?;

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
    crate::app_paths::write_private(&path, json).map_err(|e| e.to_string())?;
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
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&custom_jobs_path(), data).map_err(|e| e.to_string())
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
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    let entry = redact_activity_entry(entry);
    super::path_safety::validate_name(&entry.id)?;
    let path = super::path_safety::safe_join(&dir, &format!("{}.json", entry.id))?;
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&path, json).map_err(|e| e.to_string())
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

    // Only the newest files are read (by the timestamp in the file name), not the whole dir.
    let mut entries: Vec<ActivityEntry> = activity_files_newest_first(&dir)
        .into_iter()
        .take(max.saturating_add(20))
        .filter_map(|(_, path)| {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .map(|entry: ActivityEntry| {
                    let redacted = redact_activity_entry(entry.clone());
                    if redacted != entry {
                        if let Ok(json) = serde_json::to_string_pretty(&redacted) {
                            let _ = crate::app_paths::write_private(&path, json);
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

    fn activity_at(dir: &std::path::Path, ms: u64, n: u32) -> String {
        let id = format!("task-{ms}-{n}");
        let e = ActivityEntry {
            id: id.clone(),
            entry_type: "t".into(),
            name: "Job".into(),
            status: "completed".into(),
            started_at: ms,
            completed_at: ms + 5,
            duration_ms: 5,
            detail: None,
            outcome: Some("ok".into()),
        };
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string(&e).unwrap(),
        )
        .unwrap();
        id
    }

    const NOW: u64 = 1_793_000_000_000; // 2026-10-27

    #[test]
    fn archive_dry_run_counts_without_touching_files() {
        let dir = temp_dir("act-dry");
        for i in 0..3 {
            activity_at(&dir, NOW - 40 * DAY_MS - i, 1);
        }
        for i in 0..2 {
            activity_at(&dir, NOW - DAY_MS - i, 1);
        }
        let r = archive_activity_log_in(&dir, NOW, 30, 5000, true).unwrap();
        assert_eq!((r.scanned, r.selected, r.archived, r.kept), (5, 3, 0, 2));
        assert_eq!(r.months, vec![("2026-09".to_string(), 3)]);
        assert_eq!(activity_files_newest_first(&dir).len(), 5);
        assert!(!dir.join("archive").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_moves_old_entries_into_verified_monthly_gz_and_appends() {
        let dir = temp_dir("act-arch");
        let old_ids: Vec<String> = (0..3)
            .map(|i| activity_at(&dir, NOW - 40 * DAY_MS - i, 1))
            .collect();
        let recent = activity_at(&dir, NOW - DAY_MS, 1);
        fs::write(dir.join("task-1700000000000-9.json"), "not json").unwrap();
        let r = archive_activity_log_in(&dir, NOW, 30, 5000, false).unwrap();
        assert_eq!((r.archived, r.skipped), (3, 1));
        let gz = dir.join("archive").join("2026-09.jsonl.gz");
        let lines = read_archive_lines(&gz).unwrap();
        assert_eq!(lines.len(), 3);
        for id in &old_ids {
            assert!(!dir.join(format!("{id}.json")).exists());
            assert!(lines.iter().any(|l| l.contains(id.as_str())));
        }
        assert!(dir.join(format!("{recent}.json")).exists());
        assert!(
            dir.join("task-1700000000000-9.json").exists(),
            "unparsable file kept"
        );
        // A second run appends to the same month without losing earlier lines.
        activity_at(&dir, NOW - 41 * DAY_MS, 2);
        archive_activity_log_in(&dir, NOW, 30, 5000, false).unwrap();
        assert_eq!(read_archive_lines(&gz).unwrap().len(), 4);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_enforces_file_cap_oldest_first() {
        let dir = temp_dir("act-cap");
        for i in 0..6u64 {
            activity_at(&dir, NOW - i * 1000, 1);
        }
        let r = archive_activity_log_in(&dir, NOW, 30, 4, false).unwrap();
        assert_eq!((r.archived, r.kept), (2, 4));
        let left: Vec<u64> = activity_files_newest_first(&dir)
            .iter()
            .map(|f| f.0)
            .collect();
        assert_eq!(left, vec![NOW, NOW - 1000, NOW - 2000, NOW - 3000]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn newest_first_orders_by_file_name_timestamp() {
        let dir = temp_dir("act-order");
        activity_at(&dir, NOW - 5000, 1);
        activity_at(&dir, NOW, 1);
        activity_at(&dir, NOW - 1000, 1);
        let ms: Vec<u64> = activity_files_newest_first(&dir)
            .iter()
            .map(|f| f.0)
            .collect();
        assert_eq!(ms, vec![NOW, NOW - 1000, NOW - 5000]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Dry run against a real dir: NEXUS_ACTIVITY_DRY_RUN_DIR=... cargo test -- --ignored dry_run_real
    #[test]
    #[ignore]
    fn dry_run_real_activity_dir() {
        let Ok(d) = std::env::var("NEXUS_ACTIVITY_DRY_RUN_DIR") else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let r = archive_activity_log_in(
            std::path::Path::new(&d),
            now,
            ACTIVITY_KEEP_DAYS,
            ACTIVITY_MAX_FILES,
            true,
        )
        .unwrap();
        println!("DRYRUN {}", serde_json::to_string(&r).unwrap());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-continuity-{}-{}",
            tag,
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn hour_of(json: &str) -> Option<u8> {
        let raw = format!(
            r#"{{"activeGoals":["g"],"relevantMemory":[],"dailySummaryHour":{}}}"#,
            json
        );
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
        assert!(
            !dir.join(CONTEXT_FILE).exists(),
            "load must not create the file"
        );
        let ctx = TerranceContext {
            active_goals: vec!["ship nexus-v2".into()],
            daily_summary_hour: Some(6),
            last_evening_timestamp: Some(42),
            daily_summary_skipped_slot: Some(7),
            ..Default::default()
        };
        save_context_to(&dir, &ctx).unwrap();
        let raw = fs::read_to_string(dir.join(CONTEXT_FILE)).unwrap();
        assert!(raw.contains("\"dailySummaryHour\": 6"));
        let back = load_context_from(&dir).unwrap();
        assert_eq!(back.active_goals, ctx.active_goals);
        assert_eq!(back.daily_summary_hour, Some(6));
        assert_eq!(back.last_evening_timestamp, Some(42));
        assert!(raw.contains("\"dailySummarySkippedSlot\": 7"));
        assert_eq!(back.daily_summary_skipped_slot, Some(7));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn daily_summary_skipped_slot_round_trips_and_defaults() {
        let dir = temp_dir("skipped-slot");
        let write_with = |slot: Option<serde_json::Value>| {
            let mut v = serde_json::to_value(TerranceContext {
                last_daily_summary_timestamp: Some(5),
                ..Default::default()
            })
            .unwrap();
            let obj = v.as_object_mut().unwrap();
            obj.remove("dailySummarySkippedSlot");
            if let Some(slot) = slot {
                obj.insert("dailySummarySkippedSlot".into(), slot);
            }
            fs::write(dir.join(CONTEXT_FILE), v.to_string()).unwrap();
        };
        // Older context files have no key: loads as None, other fields intact.
        write_with(None);
        let ctx = load_context_from(&dir).unwrap();
        assert_eq!(ctx.daily_summary_skipped_slot, None);
        assert_eq!(ctx.last_daily_summary_timestamp, Some(5));

        // Explicit null is also None.
        write_with(Some(serde_json::Value::Null));
        assert_eq!(
            load_context_from(&dir).unwrap().daily_summary_skipped_slot,
            None
        );

        // A real ms slot survives save -> load exactly, and saving None clears it.
        let slot: u64 = 1_793_232_000_000; // a realistic ms timestamp (2026)
        let mut ctx = TerranceContext {
            daily_summary_skipped_slot: Some(slot),
            ..Default::default()
        };
        save_context_to(&dir, &ctx).unwrap();
        assert_eq!(
            load_context_from(&dir).unwrap().daily_summary_skipped_slot,
            Some(slot)
        );
        let raw = fs::read_to_string(dir.join(CONTEXT_FILE)).unwrap();
        assert!(raw.contains(&format!("\"dailySummarySkippedSlot\": {slot}")));
        assert!(!raw.contains("daily_summary_skipped_slot"));
        ctx.daily_summary_skipped_slot = None;
        save_context_to(&dir, &ctx).unwrap();
        assert_eq!(
            load_context_from(&dir).unwrap().daily_summary_skipped_slot,
            None
        );

        // Wrong type is a load error, and the file is left as-is.
        write_with(Some(serde_json::json!("soon")));
        assert!(load_context_from(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_context_errors_and_is_not_overwritten_by_load() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(CONTEXT_FILE), b"{\"activeGoals\": [").unwrap();
        assert!(load_context_from(&dir).is_err());
        assert_eq!(
            fs::read(dir.join(CONTEXT_FILE)).unwrap(),
            b"{\"activeGoals\": ["
        );
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

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_restrictive_mode_and_defaults_new_files_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        let existing = dir.join("openclaw.json");
        fs::write(&existing, b"{}").unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o600)).unwrap();
        atomic_write(&existing, b"{\"a\":1}").unwrap();
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let fresh = dir.join("new.json");
        atomic_write(&fresh, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let shared = dir.join("shared.json");
        fs::write(&shared, b"{}").unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write(&shared, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&shared).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_drops_setuid_setgid_and_sticky_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("special-bits");
        let target = dir.join("tool.json");
        fs::write(&target, b"{}").unwrap();
        // Owner may set setuid on their own file; sticky on files may be refused
        // by some kernels, so fall back to setuid+setgid only.
        if fs::set_permissions(&target, fs::Permissions::from_mode(0o7755)).is_err() {
            fs::set_permissions(&target, fs::Permissions::from_mode(0o6755)).unwrap();
        }
        let before = fs::metadata(&target).unwrap().permissions().mode();
        assert_ne!(before & 0o7000, 0, "test setup should have special bits");
        atomic_write(&target, b"{\"a\":1}").unwrap();
        let after = fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(after & 0o7000, 0, "special bits must not be copied");
        assert_eq!(after & 0o777, 0o755);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_refuses_to_replace_a_symlink() {
        let dir = temp_dir("symlink");
        let real = dir.join("real.json");
        fs::write(&real, b"original").unwrap();
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(atomic_write(&link, b"new").is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"original");
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        // Dangling symlink is refused too.
        let dangling = dir.join("dangling.json");
        std::os::unix::fs::symlink(dir.join("missing"), &dangling).unwrap();
        assert!(atomic_write(&dangling, b"x").is_err());
        assert!(!dir.join("missing").exists());
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

/// Retire activity log entries older than `days` days (and beyond the 5000-file cap)
/// into monthly archives; nothing is deleted without a verified archive copy.
/// Returns the number of files archived.
#[tauri::command]
pub fn prune_activity_log(days: u32) -> Result<u32, String> {
    archive_activity_log(Some(days), None, Some(false)).map(|r| r.archived as u32)
}

// ─── Activity log retention ─────────────────────────────────────────────────
// Recent entries stay as one JSON file each; older ones move into monthly
// `activity_log/archive/YYYY-MM.jsonl.gz` (UTC month of startedAt). A source file
// is removed only after its month archive was rewritten, re-read and verified.

pub const ACTIVITY_KEEP_DAYS: u32 = 30;
pub const ACTIVITY_MAX_FILES: usize = 5_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Millisecond timestamp from an activity file name (`task-<ms>-<n>.json`), else the file mtime.
fn activity_file_ms(path: &std::path::Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    let from_name = stem.split('-').find_map(|part| {
        part.parse::<u64>()
            .ok()
            .filter(|ms| *ms > 1_000_000_000_000)
    });
    from_name.or_else(|| {
        fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as u64)
    })
}

/// Activity JSON files in `dir` with their timestamp, newest first.
fn activity_files_newest_first(dir: &std::path::Path) -> Vec<(u64, PathBuf)> {
    let mut files: Vec<(u64, PathBuf)> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map(|x| x == "json").unwrap_or(false))
                .filter_map(|p| activity_file_ms(&p).map(|ms| (ms, p)))
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityArchiveReport {
    pub dry_run: bool,
    pub scanned: usize,
    /// Files selected for the archive (older than keep_days, or beyond the file cap).
    pub selected: usize,
    /// Files actually moved into an archive and removed (0 on a dry run).
    pub archived: usize,
    /// Files left in place because they could not be parsed.
    pub skipped: usize,
    /// Files left as JSON after the run (or that would be, on a dry run).
    pub kept: usize,
    /// (YYYY-MM, entries) per month archive.
    pub months: Vec<(String, usize)>,
}

fn read_archive_lines(path: &std::path::Path) -> Result<Vec<String>, String> {
    use std::io::Read;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut text = String::new();
    flate2::read::MultiGzDecoder::new(fs::File::open(path).map_err(|e| e.to_string())?)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect())
}

fn month_of(ms: u64) -> String {
    chrono::DateTime::from_timestamp((ms / 1000) as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap())
        .format("%Y-%m")
        .to_string()
}

/// Archive activity files older than `keep_days`, and the oldest files beyond
/// `max_files`, into monthly gzip JSONL files under `dir/archive/`.
pub(crate) fn archive_activity_log_in(
    dir: &std::path::Path,
    now_ms: u64,
    keep_days: u32,
    max_files: usize,
    dry_run: bool,
) -> Result<ActivityArchiveReport, String> {
    use std::collections::BTreeMap;
    use std::io::Write;
    let mut report = ActivityArchiveReport {
        dry_run,
        ..Default::default()
    };
    if !dir.exists() {
        return Ok(report);
    }
    let files = activity_files_newest_first(dir);
    report.scanned = files.len();
    let cutoff = now_ms.saturating_sub(keep_days as u64 * DAY_MS);
    let selected: Vec<&(u64, PathBuf)> = files
        .iter()
        .enumerate()
        .filter(|(i, (ms, _))| *ms < cutoff || *i >= max_files)
        .map(|(_, f)| f)
        .collect();
    report.selected = selected.len();

    // Group parsed entries by month; unparsable files stay where they are.
    let mut by_month: BTreeMap<String, Vec<(PathBuf, ActivityEntry)>> = BTreeMap::new();
    for (ms, path) in selected {
        match fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<ActivityEntry>(&s).ok())
        {
            Some(entry) => {
                let at = if entry.started_at > 0 {
                    entry.started_at
                } else {
                    *ms
                };
                by_month
                    .entry(month_of(at))
                    .or_default()
                    .push((path.clone(), redact_activity_entry(entry)));
            }
            None => report.skipped += 1,
        }
    }
    report.months = by_month.iter().map(|(m, v)| (m.clone(), v.len())).collect();
    if dry_run {
        report.kept = report.scanned - (report.selected - report.skipped);
        return Ok(report);
    }

    let archive_dir = dir.join("archive");
    crate::app_paths::create_private_dir_all(&archive_dir).map_err(|e| e.to_string())?;
    for (month, items) in &by_month {
        let target = archive_dir.join(format!("{month}.jsonl.gz"));
        let mut lines = read_archive_lines(&target)?;
        let before = lines.len();
        for (_, entry) in items {
            lines.push(serde_json::to_string(entry).map_err(|e| e.to_string())?);
        }
        let tmp = archive_dir.join(format!(".{month}.jsonl.gz.tmp"));
        {
            let mut enc = flate2::write::GzEncoder::new(
                crate::app_paths::private_open_options()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&tmp)
                    .map_err(|e| e.to_string())?,
                flate2::Compression::default(),
            );
            for line in &lines {
                enc.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                enc.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            enc.finish()
                .map_err(|e| e.to_string())?
                .sync_all()
                .map_err(|e| e.to_string())?;
        }
        // Verify: every line parses, the count matches, and each new id is present.
        let check = read_archive_lines(&tmp)?;
        let parsed: Vec<ActivityEntry> = check
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let ids: std::collections::HashSet<&str> = parsed.iter().map(|e| e.id.as_str()).collect();
        let ok = check.len() == before + items.len()
            && parsed.len() == check.len()
            && items.iter().all(|(_, e)| ids.contains(e.id.as_str()));
        if !ok {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "activity archive {month} failed verification; sources kept"
            ));
        }
        fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        for (path, _) in items {
            if fs::remove_file(path).is_ok() {
                report.archived += 1;
            }
        }
    }
    report.kept = report.scanned - report.archived;
    Ok(report)
}

/// Archive old activity entries (see `archive_activity_log_in`). Defaults: 30 days, 5000 files.
#[tauri::command]
pub fn archive_activity_log(
    keep_days: Option<u32>,
    max_files: Option<u32>,
    dry_run: Option<bool>,
) -> Result<ActivityArchiveReport, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let started = std::time::Instant::now();
    let result = archive_activity_log_in(
        &activity_log_dir(),
        now,
        keep_days.unwrap_or(ACTIVITY_KEEP_DAYS),
        max_files.map(|m| m as usize).unwrap_or(ACTIVITY_MAX_FILES),
        dry_run.unwrap_or(false),
    );
    let ms = started.elapsed().as_millis();
    match &result {
        Ok(r) => eprintln!(
            "[activity-archive] {}scanned {}, archived {}, skipped {}, kept {}, months {:?} in {ms} ms",
            if r.dry_run { "dry run: " } else { "" },
            r.scanned,
            r.archived,
            r.skipped,
            r.kept,
            r.months
        ),
        Err(e) => eprintln!("[activity-archive] failed after {ms} ms: {e}"),
    }
    result
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
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&subagents_path(), data).map_err(|e| e.to_string())
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
        assert!(
            parsed.is_ok(),
            "context load failed on one bad field: {:?}",
            parsed.err()
        );
    }

    /// Bug #1 (Rust side): `save_terrance_context` uses a plain `fs::write`, so a crash
    /// mid-write leaves a truncated file. There's no path seam to fault-inject today.
    /// Bug #1: now covered via the `save_context_to(dir)` seam and `atomic_write`.
    #[test]
    fn repro_context_save_is_atomic() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-repro-atomic-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let mut ctx = TerranceContext {
            active_goals: vec!["first".into()],
            ..Default::default()
        };
        save_context_to(&dir, &ctx).unwrap();
        ctx.active_goals = vec!["second".into()];
        save_context_to(&dir, &ctx).unwrap();
        // Injected failure: the target is a non-empty directory, so the rename fails.
        let blocked = dir.join("blocked");
        fs::create_dir_all(blocked.join("x")).unwrap();
        assert!(atomic_write(&blocked, b"partial").is_err());
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            names.iter().all(|n| !n.ends_with(".tmp")),
            "leftover tmp: {names:?}"
        );
        assert_eq!(
            load_context_from(&dir).unwrap().active_goals,
            vec!["second".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Legacy com.taisun.electron context is migrated into com.taisun.nexus and
    /// loads intact; the legacy copy is left in place and existing new data wins.
    #[test]
    fn legacy_context_is_migrated_and_loadable() {
        let base =
            std::env::temp_dir().join(format!("nexus-ctx-mig-{}", uuid::Uuid::new_v4().simple()));
        let legacy = base.join("com.taisun.electron");
        fs::create_dir_all(&legacy).unwrap();
        let raw = r#"{"activeGoals":["Legacy goal"],"lastSessionSummary":"s","lastSessionTimestamp":1,"relevantMemory":["m"],"dailySummaryHour":6}"#;
        fs::write(legacy.join(CONTEXT_FILE), raw).unwrap();
        fs::write(legacy.join("subagents.json"), "[]").unwrap();

        let copied = crate::app_paths::migrate_legacy_data_dir_in(&base).unwrap();
        assert!(copied.contains(&CONTEXT_FILE.to_string()));
        let new_dir = base.join(crate::app_paths::DATA_DIR_NAME);
        let ctx = load_context_from(&new_dir).unwrap();
        assert_eq!(ctx.active_goals, vec!["Legacy goal".to_string()]);
        assert_eq!(ctx.daily_summary_hour, Some(6));
        assert!(
            legacy.join(CONTEXT_FILE).exists(),
            "legacy data must not be removed"
        );

        // A later save goes to the new dir and is not clobbered by a re-run.
        let mut updated = ctx.clone();
        updated.active_goals = vec!["New goal".into()];
        save_context_to(&new_dir, &updated).unwrap();
        assert!(crate::app_paths::migrate_legacy_data_dir_in(&base)
            .unwrap()
            .is_empty());
        assert_eq!(
            load_context_from(&new_dir).unwrap().active_goals,
            vec!["New goal".to_string()]
        );
        let _ = fs::remove_dir_all(&base);
    }
}
