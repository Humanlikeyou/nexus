/**
 * Amara Continuity Commands
 *
 * Provides Tauri commands for:
 *  - read_identity_file     — Amara's identity: <data>/config/terrance.identity.md, else the generic default
 *  - load_terrance_context  — loads active goals + last session summary from data dir
 *  - save_terrance_context  — persists context to data dir
 *  - store_session_log      — writes session summaries to <data>/session_logs/
 *
 * Private data never lives in the source repository (decision 0050, private_store.rs).
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

pub(crate) fn context_path() -> PathBuf {
    data_dir().join(CONTEXT_FILE)
}

/// Session summaries live in the app-data folder in every build (decision 0050). Older development builds
/// wrote them into the repository's memory/session_logs; private_store copies those over once at startup.
fn session_logs_dir() -> PathBuf {
    super::private_store::session_logs_dir_in(&data_dir())
}

// ─── Identity ─────────────────────────────────────────────────────────────────

/// Amara's identity: the private copy in `<data>/config/terrance.identity.md`, else the generic default
/// embedded at build time. Never read from the repository or from beside the binary.
#[tauri::command]
#[specta::specta]
pub fn read_identity_file() -> Result<String, String> {
    Ok(super::private_store::identity_in(&data_dir()))
}

/// The operational profile: the private copy in `<data>/config/terrance.profile.md`, else the generic
/// template embedded at build time.
#[tauri::command]
#[specta::specta]
pub fn read_profile_file() -> Result<String, String> {
    Ok(super::private_store::profile_in(&data_dir()))
}

// ─── Context Store ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default, specta::Type)]
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
    #[specta(type = Option<u8>)]
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
#[specta::specta]
pub fn load_terrance_context() -> Result<TerranceContext, String> {
    load_context_from(&data_dir())
}

#[tauri::command]
#[specta::specta]
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
    // Private data: always 0600, even when replacing an older 0644 file.
    atomic_write_with_mode(&dir.join(CONTEXT_FILE), json.as_bytes(), Some(0o600))
        .map_err(|e| e.to_string())
}

// The atomic write moved to `shared::fsx` (package 2). Its tests stay below.
#[cfg(test)]
use super::shared::fsx::atomic_write;
use super::shared::fsx::atomic_write_with_mode;

// ─── Session Log ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, specta::Type)]
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
#[specta::specta]
pub fn store_session_log(entry: SessionLogEntry) -> Result<(), String> {
    let dir = session_logs_dir();
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    super::path_safety::validate_name(&entry.id)?;
    let path = super::path_safety::safe_join(&dir, &format!("{}.json", entry.id))?;
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
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
    super::private_store::archive_dir_in(&data_dir())
}

/// Move session logs older than `days_threshold` days to <data>/archive/.
/// Returns the number of files moved.
#[tauri::command]
#[specta::specta]
pub async fn archive_old_session_logs(days_threshold: u32) -> Result<u32, String> {
    run_blocking(move || archive_old_session_logs_sync(days_threshold)).await
}

fn archive_old_session_logs_sync(days_threshold: u32) -> Result<u32, String> {
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
#[specta::specta]
pub async fn prune_context_memory(max_items: u32) -> Result<u32, String> {
    run_blocking(move || prune_context_memory_sync(max_items)).await
}

fn prune_context_memory_sync(max_items: u32) -> Result<u32, String> {
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
#[specta::specta]
pub fn load_custom_jobs() -> Result<String, String> {
    let path = custom_jobs_path();
    if !path.exists() {
        return Ok("[]".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Save custom cron jobs to disk. Accepts raw JSON string.
#[tauri::command]
#[specta::specta]
pub fn save_custom_jobs(data: String) -> Result<(), String> {
    let dir = data_dir();
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&custom_jobs_path(), data).map_err(|e| e.to_string())
}

// ─── Session Insights ───────────────────────────────────────────────────────

/// Get aggregated session insights: total sessions, tokens, providers, daily breakdown.
#[tauri::command]
#[specta::specta]
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

pub(crate) fn activity_log_dir() -> PathBuf {
    // Activity logs live alongside session_logs in the data dir
    data_dir().join("activity_log")
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, specta::Type)]
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
#[specta::specta]
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
#[specta::specta]
pub fn list_activity_entries(limit: Option<u32>) -> Result<Vec<ActivityEntry>, String> {
    list_activity_entries_in(&activity_log_dir(), limit)
}

fn list_activity_entries_in(
    dir: &std::path::Path,
    limit: Option<u32>,
) -> Result<Vec<ActivityEntry>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let max = limit.unwrap_or(100) as usize;

    // Only the newest files are read (by the timestamp in the file name), not the whole dir.
    let mut entries: Vec<ActivityEntry> = activity_files_newest_first(dir)
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

    #[tokio::test]
    async fn archive_on_blocking_pool_matches_sync_result() {
        let seed = |dir: &std::path::Path| {
            for i in 0..3 {
                activity_at(dir, NOW - 40 * DAY_MS - i, 1);
            }
            activity_at(dir, NOW - DAY_MS, 1);
        };
        let a = temp_dir("act-async");
        let b = temp_dir("act-sync");
        seed(&a);
        seed(&b);
        let dir = a.clone();
        let r_async = run_blocking(move || archive_activity_log_in(&dir, NOW, 30, 5000, false))
            .await
            .unwrap();
        let r_sync = archive_activity_log_in(&b, NOW, 30, 5000, false).unwrap();
        assert_eq!(
            (
                r_async.scanned,
                r_async.archived,
                r_async.kept,
                &r_async.months
            ),
            (r_sync.scanned, r_sync.archived, r_sync.kept, &r_sync.months)
        );
        assert_eq!(r_async.archived, 3);
        assert_eq!(activity_files_newest_first(&a).len(), 1);
        let panicked: Result<u32, String> = run_blocking(|| panic!("boom")).await;
        assert!(panicked.unwrap_err().starts_with("background task failed"));
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

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

    // ── Archive safety (QA top gap: a bug here loses data) ──────────────────

    fn json_files(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn gz_lines(path: &std::path::Path, lines: &[&str]) {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        for l in lines {
            enc.write_all(l.as_bytes()).unwrap();
            enc.write_all(b"\n").unwrap();
        }
        enc.finish().unwrap();
    }

    /// Two months selected, the later month's write fails: the earlier month was archived and
    /// verified (its sources go), every source of the failed month stays and its old archive is
    /// unchanged. Failure is per month, after any earlier month already completed.
    fn assert_failed_month_kept(dir: &std::path::Path, oct_ids: &[String], before: &[u8]) {
        let gz = dir.join("archive").join("2026-10.jsonl.gz");
        for id in oct_ids {
            assert!(dir.join(format!("{id}.json")).exists(), "{id} kept");
        }
        assert_eq!(fs::read(&gz).unwrap(), before, "old archive unchanged");
        assert!(!dir.join("archive").join(".2026-10.jsonl.gz.tmp").is_file());
    }

    const OCT_5: u64 = 1_791_158_400_000; // 2026-10-05 00:00 UTC
    const SEP_5: u64 = 1_788_566_400_000; // 2026-09-05 00:00 UTC
    const LATER: u64 = OCT_5 + 60 * DAY_MS; // both months are past the 30-day window

    fn seed_two_months(dir: &std::path::Path) -> (Vec<String>, Vec<String>, Vec<u8>) {
        let sep: Vec<String> = (0..3).map(|i| activity_at(dir, SEP_5 + i, 1)).collect();
        let oct: Vec<String> = (0..3).map(|i| activity_at(dir, OCT_5 + i, 1)).collect();
        let archive = dir.join("archive");
        fs::create_dir_all(&archive).unwrap();
        let existing = r#"{"id":"task-1791100000000-1","type":"t","name":"Old","status":"completed","startedAt":1791100000000,"completedAt":1791100000005,"durationMs":5}"#;
        gz_lines(&archive.join("2026-10.jsonl.gz"), &[existing]);
        let before = fs::read(archive.join("2026-10.jsonl.gz")).unwrap();
        (sep, oct, before)
    }

    #[test]
    fn archive_write_failure_keeps_sources_and_old_archive() {
        // A directory squatting on the temp path: the temp file can't be created.
        let dir = temp_dir("act-tmpdir");
        let (sep, oct, before) = seed_two_months(&dir);
        fs::create_dir_all(dir.join("archive").join(".2026-10.jsonl.gz.tmp")).unwrap();
        let err = archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap_err();
        assert!(
            err.starts_with(&format!(
                "Archived {} files from 1 months; stopped at 2026-10: ",
                sep.len()
            )) && err.ends_with("; remaining files kept."),
            "{err}"
        );
        assert_failed_month_kept(&dir, &oct, &before);
        for id in &sep {
            assert!(
                !dir.join(format!("{id}.json")).exists(),
                "earlier month done"
            );
        }
        assert_eq!(
            read_archive_lines(&dir.join("archive").join("2026-09.jsonl.gz"))
                .unwrap()
                .len(),
            3
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_read_only_archive_dir_keeps_every_source() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("act-ro");
        let (sep, oct, before) = seed_two_months(&dir);
        let archive = dir.join("archive");
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o500)).unwrap();
        let result = archive_activity_log_in(&dir, LATER, 30, 5000, false);
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        // The first month (September) already fails: nothing at all is removed.
        for id in sep.iter().chain(&oct) {
            assert!(dir.join(format!("{id}.json")).exists(), "{id} kept");
        }
        assert_failed_month_kept(&dir, &oct, &before);
        assert!(!archive.join("2026-09.jsonl.gz").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_corrupt_existing_gz_keeps_sources_and_archive() {
        let dir = temp_dir("act-corrupt");
        let (_, oct, _) = seed_two_months(&dir);
        let gz = dir.join("archive").join("2026-10.jsonl.gz");
        fs::write(&gz, b"\x1f\x8b\x08\x00 truncated gzip").unwrap();
        let before = fs::read(&gz).unwrap();
        assert!(archive_activity_log_in(&dir, LATER, 30, 5000, false).is_err());
        assert_failed_month_kept(&dir, &oct, &before);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Pinned: a valid gzip with one unparsable line fails verification (every line must parse),
    /// so that month is never archived again until the bad line is fixed by hand. Safe (nothing
    /// is lost), but it blocks the month; files keep piling up as JSON.
    #[test]
    fn archive_bad_line_in_existing_gz_fails_verification_and_keeps_sources() {
        let dir = temp_dir("act-badline");
        let (_, oct, _) = seed_two_months(&dir);
        let gz = dir.join("archive").join("2026-10.jsonl.gz");
        gz_lines(&gz, &["not json"]);
        let before = fs::read(&gz).unwrap();
        let err = archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap_err();
        assert!(
            err.contains("stopped at 2026-10: archive failed verification; remaining files kept."),
            "{err}"
        );
        assert_failed_month_kept(&dir, &oct, &before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_over_the_5000_file_cap_moves_the_oldest_10() {
        let dir = temp_dir("act-5010");
        // 5,010 files all inside the 30-day window, one second apart.
        for i in 0..5_010u64 {
            activity_at(&dir, NOW - i * 1000, 1);
        }
        let r = archive_activity_log_in(&dir, NOW, ACTIVITY_KEEP_DAYS, ACTIVITY_MAX_FILES, false)
            .unwrap();
        assert_eq!(
            (r.scanned, r.selected, r.archived, r.kept),
            (5_010, 10, 10, 5_000)
        );
        let left = activity_files_newest_first(&dir);
        assert_eq!(left.len(), 5_000);
        assert_eq!(left.first().unwrap().0, NOW);
        assert_eq!(left.last().unwrap().0, NOW - 4_999 * 1000);
        let archived: Vec<String> =
            read_archive_lines(&dir.join("archive").join("2026-10.jsonl.gz"))
                .unwrap()
                .iter()
                .map(|l| serde_json::from_str::<ActivityEntry>(l).unwrap().id)
                .collect();
        let mut expected: Vec<String> = (5_000..5_010u64)
            .map(|i| format!("task-{}-1", NOW - i * 1000))
            .collect();
        let mut got = archived.clone();
        got.sort();
        expected.sort();
        assert_eq!(got, expected);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Re-running in the same month appends (line count = sum, distinct ids stay distinct).
    /// Pinned: there is no dedupe. If a file with an already-archived id reappears, the second
    /// run appends it again and the month holds that id twice.
    #[test]
    fn archive_rerun_same_month_appends_without_dedupe() {
        let dir = temp_dir("act-rerun");
        let first: Vec<String> = (0..4).map(|i| activity_at(&dir, SEP_5 + i, 1)).collect();
        archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap();
        let second: Vec<String> = (0..3)
            .map(|i| activity_at(&dir, SEP_5 + 100 + i, 2))
            .collect();
        archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap();
        let gz = dir.join("archive").join("2026-09.jsonl.gz");
        let ids = |gz: &std::path::Path| -> Vec<String> {
            read_archive_lines(gz)
                .unwrap()
                .iter()
                .map(|l| serde_json::from_str::<ActivityEntry>(l).unwrap().id)
                .collect()
        };
        let got = ids(&gz);
        assert_eq!(got.len(), first.len() + second.len());
        let unique: std::collections::HashSet<&String> = got.iter().collect();
        assert_eq!(unique.len(), got.len(), "no duplicate ids");

        // Same id again: appended, not deduped.
        activity_at(&dir, SEP_5, 1);
        archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap();
        let got = ids(&gz);
        assert_eq!(got.len(), 8);
        assert_eq!(got.iter().filter(|id| **id == first[0]).count(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Months are UTC (month_of formats the UTC date): 2026-10-01 00:30 JST is 2026-09-30 15:30
    /// UTC, so it lands in 2026-09.jsonl.gz, not 2026-10.
    #[test]
    fn archive_months_are_utc_not_tokyo() {
        let dir = temp_dir("act-month");
        let ms = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:30:00+09:00")
            .unwrap()
            .timestamp_millis() as u64;
        let id = activity_at(&dir, ms, 1);
        let r = archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap();
        assert_eq!(r.months, vec![("2026-09".to_string(), 1)]);
        let lines = read_archive_lines(&dir.join("archive").join("2026-09.jsonl.gz")).unwrap();
        assert!(lines[0].contains(&id));
        assert!(!dir.join("archive").join("2026-10.jsonl.gz").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// After archiving, the list reads only the newest limit+20 files. Proof: list rewrites any
    /// file it reads that holds a credential (redaction); one just outside the window is untouched.
    #[test]
    fn list_after_archive_reads_only_the_newest_limit_plus_20() {
        let dir = temp_dir("act-list");
        for i in 0..40u64 {
            activity_at(&dir, SEP_5 + i, 1); // archived
        }
        let newest: Vec<String> = (0..60u64)
            .map(|i| activity_at(&dir, NOW - i * 1000, 1))
            .collect();
        archive_activity_log_in(&dir, NOW, 30, 5000, false).unwrap();
        assert_eq!(activity_files_newest_first(&dir).len(), 60);

        let with_secret = |id: &str, ms: u64| {
            let raw = format!(
                r#"{{"id":"{id}","type":"t","name":"Job","status":"failed","startedAt":{ms},"completedAt":{ms},"durationMs":0,"detail":"token=FAKE_TOKEN_NOT_REAL"}}"#
            );
            fs::write(dir.join(format!("{id}.json")), raw).unwrap();
        };
        // limit 30 → window = newest 50 files: index 49 is inside, index 50 is outside.
        with_secret(&newest[49], NOW - 49_000);
        with_secret(&newest[50], NOW - 50_000);
        let outside_before = fs::read(dir.join(format!("{}.json", newest[50]))).unwrap();

        let got = list_activity_entries_in(&dir, Some(30)).unwrap();
        let got_ids: Vec<&str> = got.iter().map(|e| e.id.as_str()).collect();
        let want: Vec<&str> = newest[..30].iter().map(String::as_str).collect();
        assert_eq!(got_ids, want, "newest 30, newest first");
        let inside = fs::read_to_string(dir.join(format!("{}.json", newest[49]))).unwrap();
        assert!(!inside.contains("FAKE_TOKEN_NOT_REAL"), "read and redacted");
        assert_eq!(
            fs::read(dir.join(format!("{}.json", newest[50]))).unwrap(),
            outside_before,
            "outside the window: never read"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_leaves_unparsable_and_foreign_files_and_finishes() {
        let dir = temp_dir("act-foreign");
        let old: Vec<String> = (0..3).map(|i| activity_at(&dir, SEP_5 + i, 1)).collect();
        // Old by name, but not activity entries.
        fs::write(dir.join("task-1788566400100-7.json"), "{ truncated").unwrap();
        fs::write(
            dir.join("task-1788566400200-8.json"),
            r#"{"hello":"world"}"#,
        )
        .unwrap();
        // Foreign: not .json, and a subdirectory with a .json name.
        fs::write(dir.join("notes.txt"), "keep me").unwrap();
        fs::write(dir.join(".DS_Store"), [0u8, 1, 2]).unwrap();
        fs::create_dir_all(dir.join("nested.json")).unwrap();
        let r = archive_activity_log_in(&dir, LATER, 30, 5000, false).unwrap();
        assert_eq!((r.archived, r.skipped), (3, 2));
        for id in &old {
            assert!(!dir.join(format!("{id}.json")).exists());
        }
        for f in [
            "task-1788566400100-7.json",
            "task-1788566400200-8.json",
            "notes.txt",
            ".DS_Store",
        ] {
            assert!(dir.join(f).is_file(), "{f} kept");
        }
        assert!(dir.join("nested.json").is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_dry_run_touches_nothing() {
        let dir = temp_dir("act-dry-mtime");
        for i in 0..3 {
            activity_at(&dir, SEP_5 + i, 1);
        }
        activity_at(&dir, NOW, 1);
        let snapshot = |dir: &std::path::Path| -> Vec<(String, std::time::SystemTime, u64)> {
            json_files(dir)
                .into_iter()
                .map(|n| {
                    let m = fs::metadata(dir.join(&n)).unwrap();
                    (n, m.modified().unwrap(), m.len())
                })
                .collect()
        };
        let before = snapshot(&dir);
        let dir_mtime = fs::metadata(&dir).unwrap().modified().unwrap();
        let r = archive_activity_log_in(&dir, LATER + 30 * DAY_MS, 30, 5000, true).unwrap();
        assert_eq!((r.selected, r.archived), (4, 0));
        assert_eq!(snapshot(&dir), before, "same files, mtimes and sizes");
        assert_eq!(fs::metadata(&dir).unwrap().modified().unwrap(), dir_mtime);
        assert!(
            !dir.join("archive").exists(),
            "no archive dir, no temp files"
        );
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

    #[cfg(unix)]
    #[test]
    fn save_context_to_rewrites_an_existing_0644_context_as_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("nexus-ctx-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONTEXT_FILE);
        fs::write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        save_context_to(&dir, &TerranceContext::default()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(&path).unwrap();
        save_context_to(&dir, &TerranceContext::default()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&dir).unwrap();
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
        let existing = dir.join("settings.json");
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
#[specta::specta]
pub async fn prune_activity_log(days: u32) -> Result<u32, String> {
    run_blocking(move || {
        archive_activity_log_sync(Some(days), None, Some(false)).map(|r| r.archived as u32)
    })
    .await
}

/// Run file-heavy archive/prune work on the blocking pool so the Tauri main
/// thread (and the UI) never waits on it. A panic in the job becomes an Err.
async fn run_blocking<T, F>(job: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
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

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, specta::Type)]
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
    for (done_months, (month, items)) in by_month.iter().enumerate() {
        // Per-month step; on failure earlier months are already archived, so say so.
        let step = (|| -> Result<(), String> {
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
            let ids: std::collections::HashSet<&str> =
                parsed.iter().map(|e| e.id.as_str()).collect();
            let ok = check.len() == before + items.len()
                && parsed.len() == check.len()
                && items.iter().all(|(_, e)| ids.contains(e.id.as_str()));
            if !ok {
                let _ = fs::remove_file(&tmp);
                return Err("archive failed verification".to_string());
            }
            fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
            for (path, _) in items {
                if fs::remove_file(path).is_ok() {
                    report.archived += 1;
                }
            }
            Ok(())
        })();
        if let Err(e) = step {
            // Raw io errors can carry paths; only the verification reason is passed through.
            let e = if e == "archive failed verification" {
                e
            } else {
                "archive write failed".to_string()
            };
            return Err(format!(
                "Archived {} files from {done_months} months; stopped at {month}: {e}; remaining files kept.",
                report.archived
            ));
        }
    }
    report.kept = report.scanned - report.archived;
    Ok(report)
}

/// Archive old activity entries (see `archive_activity_log_in`). Defaults: 30 days, 5000 files.
#[tauri::command]
#[specta::specta]
pub async fn archive_activity_log(
    keep_days: Option<u32>,
    max_files: Option<u32>,
    dry_run: Option<bool>,
) -> Result<ActivityArchiveReport, String> {
    run_blocking(move || archive_activity_log_sync(keep_days, max_files, dry_run)).await
}

fn archive_activity_log_sync(
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
#[specta::specta]
pub fn load_subagents() -> Result<String, String> {
    let path = subagents_path();
    if !path.exists() {
        return Ok("[]".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Save subagents to disk. Accepts raw JSON string.
#[tauri::command]
#[specta::specta]
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
