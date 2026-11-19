//! Backup settings, "Back Up Now" and the backup list (design-1111s wiring step 3).
//!
//! HARD RULE 7: this module never deletes a backup. `keep`, `nightly` and `beforeUpdate` are
//! stored so the UI can show them, but there is NO scheduler, NO timer and NO prune here.
//! Pruning old backups needs Taisun's explicit OK first. A test asserts the non-test source
//! contains no file or directory removal call.
//!
//! Restore is not here either (`restore` / `restorePreview` stay unwired).

use super::packager::{pack_to, PackOpts, PackageInfo, Progress};
use chrono::{Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::Emitter;

const SETTINGS_FILE: &str = "backup_settings.json";
pub(crate) const PROGRESS_EVENT: &str = "backup://progress";
/// A backup needs this much free space on top of the size of the data it copies.
const FREE_SPACE_MARGIN: u64 = 5 * 1024 * 1024 * 1024;
const MAX_SETTINGS_BYTES: u64 = 16 * 1024;
const NEW_PREFIX: &str = "amara-backup-";
const LEGACY_PREFIX: &str = "nexus-backup-";

/// Files that are never copied into a backup, wherever they sit in the data dir.
/// Found by reading the writers: `crew.rs` (`crew_service_token`), `n8n_status.rs`
/// (`n8n_api_key`), `key_file.rs` (`amara-n8n.key` and its `.amara-n8n.key.*.tmp` files),
/// `ha.rs` (the app `.env` holding `HA_TOKEN`). Keychain items (`keychain.rs`) and the router
/// secrets file live outside the data dir and are never read.
pub(crate) const EXCLUDED_SECRET_FILES: &[&str] = &[
    super::key_file::KEY_FILE_NAME,
    super::crew::CREW_TOKEN_FILE,
    super::n8n_status::N8N_KEY_FILE,
    ".env",
    // Outbound opt-in: device-bound, never travels in a backup (`outbound.rs`).
    super::outbound::FILE_NAME,
    // Supervisor act opt-in: device-bound like the outbound one (`supervisor/act.rs`).
    super::supervisor::act::FILE_NAME,
    // Home windows: per-site browser profile records (`home_windows.rs`); never backed up.
    "webviews",
];
/// Any file or folder whose lower-cased name contains one of these is left out too.
pub(crate) const EXCLUDED_NAME_FRAGMENTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passphrase",
    "credential",
    "api_key",
    "apikey",
    "private_key",
    ".key.",
    ".env",
];
/// ...and so is anything with one of these extensions.
pub(crate) const EXCLUDED_SUFFIXES: &[&str] = &[".key", ".pem", ".p12", ".pfx"];

/// `config.json` is kept but the copy has these fields removed: a top-level (or nested) key
/// ending in one of these is a secret (`telegram_bot_token`, `anthropic_api_key`,
/// `n8n_webhook_secret`, `gateway_auth_token`, ...). Counts like `llm_token_threshold` stay.
const SECRET_FIELD_SUFFIXES: &[&str] = &[
    "_token",
    "_secret",
    "_api_key",
    "_apikey",
    "_password",
    "_passphrase",
    "_key",
];

/// True when `name` (at `depth` below the data dir, 0 = top level) must stay out of a backup.
pub(crate) fn is_excluded_name(name: &str, depth: usize) -> bool {
    let lower = name.to_ascii_lowercase();
    if depth == 0 && lower == "config.json" {
        return true; // written separately, with secret fields removed
    }
    EXCLUDED_SECRET_FILES.iter().any(|f| lower == *f)
        || EXCLUDED_NAME_FRAGMENTS.iter().any(|f| lower.contains(f))
        || EXCLUDED_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Field names that are a secret on their own (`arm_config.json` `worker_api_keys[].key`).
const SECRET_FIELD_NAMES: &[&str] = &[
    "key",
    "token",
    "secret",
    "password",
    "passphrase",
    "apikey",
    "api_key",
    "auth",
    "authorization",
    "bearer",
    "cookie",
    "private_key",
    "seed",
    "mnemonic",
];

/// camelCase `...Key` after a lowercase letter or digit: `privateKey`, `accessKey`, `secretKey`,
/// `signingKey`, `sessionKey`, `encryptionKey`, `clientKey`, `apiKey`.
fn camel_key(key: &str) -> bool {
    let b = key.as_bytes();
    b.len() > 3
        && key.ends_with("Key")
        && (b[b.len() - 4].is_ascii_lowercase() || b[b.len() - 4].is_ascii_digit())
}

fn is_secret_field(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    camel_key(key)
        || SECRET_FIELD_NAMES.contains(&k.as_str())
        || SECRET_FIELD_SUFFIXES.iter().any(|s| k.ends_with(s))
        || EXCLUDED_NAME_FRAGMENTS
            .iter()
            .any(|f| !f.starts_with('.') && k.contains(f) && !k.contains("threshold"))
}

/// Whether a field is removed: `is_secret_field` by name, minus two exceptions.
/// - A NUMBER under a plural `*_tokens` name is a count (`prompt_tokens`), not a secret.
/// - A bare `key` is removed by default, whatever its value looks like. It is kept only where
///   `allow_key` is set: the one known identifier context, a `cited[]` entry of an Amara take
///   (`take.cited[]` in `amara_take_*.json`, shape `{key,label,display,baseline}`, see
///   `src-console/amaraTake/types.ts` `CitedPoint`).
fn is_secret_entry(k: &str, v: &Value, allow_key: bool) -> bool {
    if !is_secret_field(k) {
        return false;
    }
    let lower = k.to_ascii_lowercase();
    if v.is_number() && (lower == "tokens" || lower.ends_with("_tokens")) {
        return false;
    }
    !(lower == "key" && allow_key && v.is_string())
}

/// A `cited[]` item of an Amara take: an object in an array named `cited` that carries the other
/// three `CitedPoint` fields (`label`, `display`, `baseline`) as well.
fn is_cited_point(map: &serde_json::Map<String, Value>, parent: Option<&str>) -> bool {
    parent == Some("cited")
        && map.get("key").is_some_and(Value::is_string)
        && map.get("label").is_some_and(Value::is_string)
        && map.get("display").is_some_and(Value::is_string)
        && map
            .get("baseline")
            .is_some_and(|b| b.is_string() || b.is_null())
}

fn redact(v: &mut Value, removed: &mut Vec<String>) {
    redact_in(v, removed, None)
}

fn redact_in(v: &mut Value, removed: &mut Vec<String>, parent: Option<&str>) {
    match v {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            let allow_key = is_cited_point(map, parent);
            for k in keys {
                let secret = map
                    .get(&k)
                    .is_some_and(|val| is_secret_entry(&k, val, allow_key));
                if secret {
                    map.remove(&k);
                    removed.push(k);
                } else if let Some(child) = map.get_mut(&k) {
                    redact_in(child, removed, Some(&k));
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| redact_in(i, removed, parent)),
        _ => {}
    }
}

/// Parse JSON and drop every secret field at any depth. `None` if it isn't JSON (the caller
/// leaves the file out). Returns the cleaned bytes and the removed field names (never values).
pub(crate) fn redact_json_bytes(bytes: &[u8]) -> Option<(Vec<u8>, Vec<String>)> {
    let mut value: Value = serde_json::from_slice(bytes).ok()?;
    let mut removed = Vec::new();
    redact(&mut value, &mut removed);
    removed.sort();
    removed.dedup();
    serde_json::to_vec(&value).ok().map(|b| (b, removed))
}

/// Restore side of the redaction: `restored` (from a backup, secrets stripped) wins for every
/// field, but a secret field it lacks is kept from `current` (the live file), at any depth, so a
/// restore never wipes this Mac's tokens. `None` if either side isn't JSON (caller copies as before).
///
/// A secret is kept only while every endpoint-like field (url, host, endpoint, ...) of its object
/// and of every enclosing object is unchanged by the restore: a crafted archive that points an
/// endpoint elsewhere gets no live token for it (the secret is dropped and must be re-entered).
pub(crate) fn merge_kept_secrets(restored: &[u8], current: &[u8]) -> Option<Vec<u8>> {
    fn is_endpoint(k: &str) -> bool {
        let k = k.to_ascii_lowercase();
        [
            "url", "uri", "host", "endpoint", "server", "domain", "port", "address", "base",
            "chat_id",
        ]
        .iter()
        .any(|m| k.contains(m))
    }
    fn endpoints_same(
        r: &serde_json::Map<String, Value>,
        c: &serde_json::Map<String, Value>,
    ) -> bool {
        r.keys()
            .chain(c.keys())
            .filter(|k| is_endpoint(k) && !is_secret_field(k))
            .all(|k| r.get(k) == c.get(k))
    }
    fn merge(r: &mut Value, c: &Value, outer_same: bool) {
        if let (Value::Object(rm), Value::Object(cm)) = (r, c) {
            let same = outer_same && endpoints_same(rm, cm);
            for (k, cv) in cm {
                match rm.get_mut(k) {
                    None if same && is_secret_field(k) => {
                        rm.insert(k.clone(), cv.clone());
                    }
                    Some(rv) => merge(rv, cv, same),
                    None => {}
                }
            }
        }
    }
    let mut r: Value = serde_json::from_slice(restored).ok()?;
    let c: Value = serde_json::from_slice(current).ok()?;
    merge(&mut r, &c, true);
    serde_json::to_vec_pretty(&r).ok()
}

/// Copy `<data>/config.json` into `<dest>/config.json` without its secret fields (names are
/// noted in `skipped`, values never). An unreadable file is left out and noted.
pub(crate) fn write_redacted_config(
    data_dir: &Path,
    dest: &Path,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    let src = data_dir.join("config.json");
    let meta = match fs::symlink_metadata(&src) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if !meta.is_file() || meta.len() > 50_000_000 {
        skipped.push("config.json (not a plain file, left out)".into());
        return Ok(());
    }
    let parsed = fs::read(&src)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let Some(mut value) = parsed else {
        skipped.push("config.json (unreadable, left out)".into());
        return Ok(());
    };
    let mut removed = Vec::new();
    redact(&mut value, &mut removed);
    removed.sort();
    removed.dedup();
    for k in removed {
        skipped.push(format!("config.json field {k} (secret, removed)"));
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    fs::create_dir_all(dest).map_err(|e| format!("mkdir: {e}"))?;
    crate::app_paths::write_private(&dest.join("config.json"), bytes)
        .map_err(|e| format!("config copy: {e}"))
}

// ---- settings ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSettings {
    pub nightly: bool,
    pub at: String,
    pub dir: String,
    pub keep: u32,
    pub before_update: bool,
    pub skip_on_battery_below: u32,
    /// Reserved. Nothing deletes a backup (hard rule 7): no code path reads this to delete, and
    /// `BackupSettingsPatch` cannot set it. `backup_prune_plan` only lists what WOULD go.
    pub auto_prune: bool,
    /// When the last backup attempt (nightly or Back Up Now) finished, RFC 3339, and how it ended:
    /// `ok`, `skipped_busy`, `skipped_battery`, or the error code.
    pub last_run_at: Option<String>,
    pub last_run_result: Option<String>,
    /// When the last backup that succeeded finished (drives the missed-nightly catch-up).
    pub last_ok_at: Option<String>,
    /// Optional nightly `git bundle` of the repo into the backup dir (decision 0016). Off by default,
    /// a setting only (no UI, no upload). `last_bundle_result` is `ok` or a short code.
    pub git_bundle: bool,
    pub last_bundle_result: Option<String>,
    /// Scheduler retry state: how many runs of the slot `failed_slot` (RFC 3339) ended in an error.
    pub failed_attempts: u32,
    pub failed_slot: Option<String>,
}

impl Default for BackupSettings {
    /// Same as `BACKUP_DEFAULTS` in `src-console/standalone/adapters.ts`.
    fn default() -> Self {
        Self {
            nightly: true,
            at: "03:00".into(),
            dir: "~/Amara Backups".into(),
            keep: 14,
            before_update: true,
            skip_on_battery_below: 20,
            auto_prune: false,
            last_run_at: None,
            last_run_result: None,
            last_ok_at: None,
            git_bundle: false,
            last_bundle_result: None,
            failed_attempts: 0,
            failed_slot: None,
        }
    }
}

#[derive(Deserialize, Debug, Default, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSettingsPatch {
    pub nightly: Option<bool>,
    pub at: Option<String>,
    pub dir: Option<String>,
    pub keep: Option<i64>,
    pub before_update: Option<bool>,
    pub skip_on_battery_below: Option<i64>,
    pub git_bundle: Option<bool>,
}

fn valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit())
        && s[0..2].parse::<u32>().is_ok_and(|h| h < 24)
        && s[3..5].parse::<u32>().is_ok_and(|m| m < 60)
}

/// Resolve a settings `dir` to an absolute path and check it. Must be strictly inside `home`,
/// outside (and not containing) the app-data dir, with no `..` and no symlink on the way.
/// Errors are fixed codes; the path is never echoed.
pub(crate) fn resolve_dir(raw: &str, home: &Path, data_dir: &Path) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 1024 || raw.contains('\0') {
        return Err("dir_invalid".into());
    }
    let expanded = if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    };
    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("dir_invalid".into());
    }
    let home_canon = home.canonicalize().map_err(|_| "dir_invalid".to_string())?;
    let rel = expanded
        .strip_prefix(home)
        .or_else(|_| expanded.strip_prefix(&home_canon))
        .map_err(|_| "dir_not_in_home".to_string())?;
    if rel.as_os_str().is_empty() {
        return Err("dir_not_in_home".into()); // the home folder itself
    }
    // Walk down from home: a symlink anywhere on the way is refused.
    let mut cur = home_canon.clone();
    let mut exists = true;
    for comp in rel.components() {
        cur.push(comp);
        if !exists {
            continue;
        }
        match fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err("dir_is_symlink".into()),
            Ok(m) if !m.is_dir() => return Err("dir_not_a_directory".into()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => exists = false,
            Err(_) => return Err("dir_invalid".into()),
        }
    }
    let data_canon = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    if cur.starts_with(&data_canon) || data_canon.starts_with(&cur) || cur.starts_with(data_dir) {
        return Err("dir_in_app_data".into());
    }
    Ok(cur)
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SETTINGS_FILE)
}

pub(crate) fn load_settings(data_dir: &Path) -> Result<BackupSettings, String> {
    let path = settings_path(data_dir);
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BackupSettings::default()),
        Err(_) => return Err("storage_error".into()),
    };
    if !meta.is_file() || meta.len() > MAX_SETTINGS_BYTES {
        return Err("storage_error".into());
    }
    let bytes = fs::read(&path).map_err(|_| "storage_error".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "storage_error".to_string())
}

/// Atomic, 0600: new temp file (create_new), fsync, rename over the settings file.
fn save_settings(data_dir: &Path, s: &BackupSettings) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(s).map_err(|_| "storage_error".to_string())?;
    let tmp = data_dir.join(format!(
        ".{SETTINGS_FILE}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let mut f = crate::app_paths::private_open_options()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|_| "storage_error".to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "storage_error".to_string())?;
    fs::rename(&tmp, settings_path(data_dir)).map_err(|_| "storage_error".to_string())
}

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Validate `patch` against the stored settings and save the result.
pub(crate) fn apply_patch(
    home: &Path,
    data_dir: &Path,
    patch: BackupSettingsPatch,
) -> Result<BackupSettings, String> {
    let _g = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load_settings(data_dir)?;
    if let Some(at) = patch.at {
        if !valid_time(&at) {
            return Err("invalid_time".into());
        }
        s.at = at;
    }
    if let Some(k) = patch.keep {
        if !(1..=60).contains(&k) {
            return Err("invalid_keep".into());
        }
        s.keep = k as u32;
    }
    if let Some(b) = patch.skip_on_battery_below {
        if !(0..=100).contains(&b) {
            return Err("invalid_battery".into());
        }
        s.skip_on_battery_below = b as u32;
    }
    if let Some(d) = patch.dir {
        resolve_dir(&d, home, data_dir)?;
        s.dir = d.trim().to_string();
    }
    if let Some(v) = patch.nightly {
        s.nightly = v;
    }
    if let Some(v) = patch.git_bundle {
        s.git_bundle = v;
    }
    if let Some(v) = patch.before_update {
        s.before_update = v;
    }
    save_settings(data_dir, &s)?;
    Ok(s)
}

/// Record how a backup attempt ended. Same lock and atomic write as `apply_patch`; changes only the
/// `last_*` fields. `ok` also moves `last_ok_at`. A settings file that can't be read is left alone.
pub(crate) fn record_run(data_dir: &Path, at: &str, result: &str, ok: bool) -> Result<(), String> {
    let _g = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load_settings(data_dir)?;
    s.last_run_at = Some(at.to_string());
    s.last_run_result = Some(result.chars().take(40).collect());
    if ok {
        s.last_ok_at = Some(at.to_string());
    }
    save_settings(data_dir, &s)
}

/// The scheduler's retry counter: a failed run of `slot` adds one (a new slot starts at 1); a
/// success clears it.
pub(crate) fn record_attempt(data_dir: &Path, slot: &str, failed: bool) -> Result<(), String> {
    let _g = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load_settings(data_dir)?;
    if failed {
        s.failed_attempts = if s.failed_slot.as_deref() == Some(slot) {
            s.failed_attempts.saturating_add(1)
        } else {
            1
        };
        s.failed_slot = Some(slot.to_string());
    } else {
        s.failed_attempts = 0;
        s.failed_slot = None;
    }
    save_settings(data_dir, &s)
}

/// Record how the optional git bundle step ended.
pub(crate) fn record_bundle(data_dir: &Path, result: &str) -> Result<(), String> {
    let _g = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load_settings(data_dir)?;
    s.last_bundle_result = Some(result.chars().take(40).collect());
    save_settings(data_dir, &s)
}

// ---- listing ----

#[derive(Serialize, Debug, Clone, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// The file name (also the id `restore` will take later).
    pub id: String,
    pub label: String,
    pub made_at: String,
    pub size_bytes: u64,
    pub days_old: i64,
}

/// `amara-backup-YYYYMMDD-HHMMSS.tar.gz` or legacy `nexus-backup-YYYYMMDD_HHMMSS.tar.gz`.
pub(crate) fn parse_backup_name(name: &str) -> Option<NaiveDateTime> {
    let (rest, sep) = if let Some(r) = name.strip_prefix(NEW_PREFIX) {
        (r, '-')
    } else if let Some(r) = name.strip_prefix(LEGACY_PREFIX) {
        (r, '_')
    } else {
        return None;
    };
    let stamp = rest.strip_suffix(".tar.gz")?;
    let (d, t) = stamp.split_once(sep)?;
    if d.len() != 8 || t.len() != 6 || !(d.bytes().chain(t.bytes())).all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDateTime::parse_from_str(&format!("{d}{t}"), "%Y%m%d%H%M%S").ok()
}

pub(crate) fn list_in(
    home: &Path,
    data_dir: &Path,
    dir_setting: &str,
    now: chrono::DateTime<Local>,
) -> Result<Vec<BackupEntry>, String> {
    let dir = resolve_dir(dir_setting, home, data_dir)?;
    let rd = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("io_error".into()),
    };
    let mut out: Vec<(chrono::DateTime<Local>, BackupEntry)> = Vec::new();
    for entry in rd.flatten() {
        // DirEntry::file_type never follows a symlink.
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(naive) = parse_backup_name(&name) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let made = Local
            .from_local_datetime(&naive)
            .earliest()
            .or_else(|| meta.modified().ok().map(chrono::DateTime::<Local>::from));
        let Some(made) = made else { continue };
        out.push((
            made,
            BackupEntry {
                label: made.format("%b %-d, %-I:%M %p").to_string(),
                made_at: made.to_rfc3339(),
                size_bytes: meta.len(),
                days_old: (now - made).num_days().max(0),
                id: name,
            },
        ));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.id.cmp(&a.1.id)));
    Ok(out.into_iter().map(|(_, e)| e).collect())
}

// ---- run ----

#[derive(Serialize, Debug, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupRunResult {
    pub entry: BackupEntry,
    /// Left-out files and removed secret fields (names only).
    pub skipped: Vec<String>,
}

pub(crate) static BUSY: AtomicBool = AtomicBool::new(false);

/// Holds the single-backup flag; released on drop (also on early return or panic).
pub(crate) struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    pub(crate) fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn dir_size(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                total = total.saturating_add(e.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
    }
    total
}

/// Free bytes available to this user on the volume holding `path` (`statvfs`).
#[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
pub(crate) fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "io_error")?;
    // SAFETY: `c` is a valid NUL-terminated path and `s` is a zeroed out-parameter that
    // statvfs fills in; nothing else is touched.
    let s = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return Err("io_error".into());
        }
        s
    };
    Ok((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))
}

pub(crate) type FreeSpaceFn<'a> = &'a (dyn Fn(&Path) -> Result<u64, String> + Send + Sync);

/// One backup. Everything injectable so tests use temp dirs and a fake free-space reading.
pub(crate) async fn run_backup(
    home: &Path,
    data_dir: &Path,
    settings: &BackupSettings,
    free_space: FreeSpaceFn<'_>,
    progress: Progress<'_>,
    busy: &AtomicBool,
) -> Result<BackupRunResult, String> {
    let _guard = BusyGuard::acquire(busy).ok_or_else(|| "already_running".to_string())?;
    progress("Checking", 1);
    let dir = resolve_dir(&settings.dir, home, data_dir)?;
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        Ok(_) => return Err("dir_not_a_directory".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::app_paths::create_private_dir_all(&dir).map_err(|_| "io_error".to_string())?;
        }
        Err(_) => return Err("io_error".into()),
    }
    let need = dir_size(data_dir).saturating_add(FREE_SPACE_MARGIN);
    if free_space(&dir)? < need {
        return Err("not_enough_space".into());
    }
    let name = format!(
        "{NEW_PREFIX}{}.tar.gz",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let opts = PackOpts {
        exclude_secrets: true,
        progress: Some(progress),
        ..Default::default()
    };
    let PackageInfo { skipped, .. } = pack_to(data_dir, &dir, &name, &opts).await?;
    progress("Done", 100);
    let entry = list_in(home, data_dir, &settings.dir, Local::now())?
        .into_iter()
        .find(|e| e.id == name)
        .ok_or_else(|| "io_error".to_string())?;
    Ok(BackupRunResult { entry, skipped })
}

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "dir_invalid".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn backup_settings_get() -> Result<BackupSettings, String> {
    let data = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || load_settings(&data))
        .await
        .map_err(|_| "io_error".to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn backup_settings_set(patch: BackupSettingsPatch) -> Result<BackupSettings, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || apply_patch(&home, &data, patch))
        .await
        .map_err(|_| "io_error".to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn backup_list() -> Result<Vec<BackupEntry>, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || {
        let s = load_settings(&data)?;
        list_in(&home, &data, &s.dir, Local::now())
    })
    .await
    .map_err(|_| "io_error".to_string())?
}

// ---- verify (read-only) ----

/// Caps so a huge or hostile archive can't tie the app up.
/// Dirs count too. The app data is ~2.2 GB today (checked read-only), so 16 GiB leaves ~7x headroom.
pub(crate) const VERIFY_MAX_ENTRIES: usize = 200_000;
const VERIFY_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// A per-file hash map is ~100 bytes per file, so 32 MiB covers the entry cap.
pub(crate) const VERIFY_MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const VERIFY_RATIO_FLOOR: u64 = 1024 * 1024 * 1024;
const VERIFY_MAX_RATIO: u64 = 200;
const VERIFY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(600);
pub(crate) static VERIFY_BUSY: AtomicBool = AtomicBool::new(false);
const VERIFY_MAX_MISMATCHES: usize = 50;

#[derive(Serialize, Debug, Clone, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    /// Archive file name.
    pub file: String,
    /// When it was made (RFC 3339, from the file name).
    pub at: String,
    pub size: u64,
    pub ok: bool,
    /// Entries read through to the end (and, with hashes, compared).
    pub checked: usize,
    /// Names only: hash differs, listed but missing, secret-looking, link, or unreadable.
    pub mismatched: Vec<String>,
    /// `sha256` when the manifest carries per-file hashes, else `structure`: the packager's manifest
    /// holds metadata only, so every entry is read to its end (gzip CRC) and names are screened.
    pub mode: String,
}

/// Names go back to the UI: no control, zero-width or bidi characters, at most 200 chars.
pub(crate) fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c as u32,
                    0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF)
        })
        .take(200)
        .collect()
}

/// Counts the compressed bytes read, for the compression-ratio guard.
struct CountingReader<R> {
    inner: R,
    n: std::rc::Rc<std::cell::Cell<u64>>,
}
impl<R: std::io::Read> std::io::Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.n.set(self.n.get() + k as u64);
        Ok(k)
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The limits in force for one verification (tests pass small ones).
#[derive(Clone, Copy)]
pub(crate) struct VerifyCaps {
    entries: usize,
    total: u64,
    ratio_floor: u64,
    deadline: std::time::Duration,
}
pub(crate) const VERIFY_CAPS: VerifyCaps = VerifyCaps {
    entries: VERIFY_MAX_ENTRIES,
    total: VERIFY_MAX_TOTAL_BYTES,
    ratio_floor: VERIFY_RATIO_FLOOR,
    deadline: VERIFY_DEADLINE,
};

pub(crate) fn verify_archive(
    path: &Path,
    file: &str,
    at: String,
    caps: VerifyCaps,
) -> Result<VerifyReport, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::Instant;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "symlink_refused_or_unreadable".to_string())?;
    let meta = f.metadata().map_err(|_| "io_error".to_string())?;
    if !meta.is_file() {
        return Err("not_a_file".into());
    }
    let mut report = VerifyReport {
        file: clean_name(file),
        at,
        size: meta.len(),
        ok: false,
        checked: 0,
        mismatched: Vec::new(),
        mode: "structure".into(),
    };
    let flag = |report: &mut VerifyReport, name: &str| {
        if report.mismatched.len() < VERIFY_MAX_MISMATCHES {
            report.mismatched.push(clean_name(name));
        }
    };
    let started = Instant::now();
    let compressed = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let reader = CountingReader {
        inner: f,
        n: compressed.clone(),
    };
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut digests: std::collections::HashMap<String, String> = Default::default();
    let mut manifest: Option<Vec<u8>> = None;
    let mut manifest_bad = false;
    let mut seen: std::collections::HashSet<String> = Default::default();
    let mut total: u64 = 0;
    let mut count = 0usize;
    let mut broken = false;
    // Every streamed byte (entries and the gzip tail) passes through here.
    let mut account = |n: usize| -> Result<(), String> {
        total = total.saturating_add(n as u64);
        if total > caps.total {
            return Err("archive_too_large".into());
        }
        if total > caps.ratio_floor
            && total > VERIFY_MAX_RATIO.saturating_mul(compressed.get().max(1))
        {
            return Err("compression_ratio".into());
        }
        if started.elapsed() > caps.deadline {
            return Err("verify_timeout".into());
        }
        Ok(())
    };
    match tar.entries() {
        Err(_) => {
            flag(&mut report, "archive_unreadable");
            broken = true;
        }
        Ok(entries) => {
            for entry in entries {
                let mut entry = match entry {
                    Ok(e) => e,
                    Err(_) => {
                        flag(&mut report, "archive_unreadable");
                        broken = true;
                        break;
                    }
                };
                count += 1;
                if count > caps.entries {
                    return Err("too_many_entries".into());
                }
                let name = entry
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "bad_name".into());
                let ty = entry.header().entry_type();
                if ty.is_dir() {
                    continue;
                }
                if !ty.is_file() {
                    flag(&mut report, &name);
                    continue;
                }
                if entry.size() > caps.total {
                    return Err("archive_too_large".into());
                }
                let rel: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).skip(1).collect();
                let rel_s = rel.join("/");
                let last = rel.last().copied().unwrap_or("");
                let is_manifest = rel_s == "manifest.json";
                let skip_hash = last.starts_with("._"); // AppleDouble metadata is never restored
                if !skip_hash {
                    if is_excluded_name(&clean_name(last), 1) {
                        flag(&mut report, &name);
                    }
                    if !seen.insert(rel_s.clone()) {
                        if is_manifest {
                            manifest_bad = true;
                        } else {
                            flag(&mut report, &name);
                        }
                    }
                }
                let mut hasher = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                let mut kept: Vec<u8> = Vec::new();
                let mut kept_over = false;
                loop {
                    match entry.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            account(n)?;
                            hasher.update(&buf[..n]);
                            if is_manifest {
                                if (kept.len() + n) as u64 <= VERIFY_MAX_MANIFEST_BYTES {
                                    kept.extend_from_slice(&buf[..n]);
                                } else {
                                    kept_over = true;
                                }
                            }
                        }
                        Err(_) => {
                            flag(&mut report, "archive_unreadable");
                            broken = true;
                            break;
                        }
                    }
                }
                if broken {
                    break;
                }
                report.checked += 1;
                if is_manifest {
                    if kept_over {
                        manifest_bad = true;
                    } else if manifest.is_none() {
                        manifest = Some(kept);
                    }
                }
                if !skip_hash {
                    digests.insert(rel_s, hex::encode(hasher.finalize()));
                }
            }
        }
    }
    // Drain the gzip stream to its end so the CRC / length trailer is checked.
    if !broken {
        let mut gz = tar.into_inner();
        let mut buf = [0u8; 64 * 1024];
        loop {
            match gz.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => account(n)?,
                Err(_) => {
                    flag(&mut report, "archive_unreadable");
                    broken = true;
                    break;
                }
            }
        }
    }
    // Per-file hashes, if the manifest carries them (`sha256`: { "relative/path": "hex" }).
    let mut listed: Option<serde_json::Map<String, Value>> = None;
    if !broken {
        match manifest.as_deref() {
            None => flag(&mut report, "manifest_missing"),
            Some(m) => match serde_json::from_slice::<Value>(m) {
                Ok(Value::Object(o)) => match o.get("sha256") {
                    None => {}
                    Some(Value::Object(map))
                        if map.values().all(|v| v.as_str().is_some_and(is_hex64)) =>
                    {
                        listed = Some(map.clone());
                    }
                    Some(_) => manifest_bad = true,
                },
                _ => manifest_bad = true,
            },
        }
        if manifest_bad {
            flag(&mut report, "manifest_invalid");
        }
    }
    if let Some(map) = listed.filter(|_| !manifest_bad) {
        report.mode = "sha256".into();
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        for k in keys {
            let want = map[k].as_str().unwrap_or("").to_ascii_lowercase();
            match digests.get(k.as_str()) {
                None => flag(&mut report, &format!("missing: {k}")),
                Some(got) if *got != want => flag(&mut report, k),
                Some(_) => {}
            }
        }
    }
    report.ok = !broken && report.mismatched.is_empty() && report.checked > 0;
    Ok(report)
}

/// Newest Amara backup in the configured dir, checked without writing anything. `None` when there
/// is no backup. A symlink where a backup should be is refused, not followed.
pub(crate) fn verify_last_in(
    home: &Path,
    data_dir: &Path,
    dir_setting: &str,
    backup_busy: &AtomicBool,
    verify_busy: &AtomicBool,
) -> Result<Option<VerifyReport>, String> {
    // A backup being written is not a damaged one: the caller shows "not checked yet".
    if backup_busy.load(Ordering::Acquire) {
        return Err("backup_running".into());
    }
    let _guard = BusyGuard::acquire(verify_busy).ok_or_else(|| "busy".to_string())?;
    let dir = resolve_dir(dir_setting, home, data_dir)?;
    let rd = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("io_error".into()),
    };
    let mut newest: Option<(NaiveDateTime, String)> = None;
    for entry in rd.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(t) = parse_backup_name(&name) else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(bt, bn)| (t, &name) > (*bt, bn))
        {
            newest = Some((t, name));
        }
    }
    let Some((naive, name)) = newest else {
        return Ok(None);
    };
    let path = dir.join(&name);
    let meta = fs::symlink_metadata(&path).map_err(|_| "io_error".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("symlink_refused".into());
    }
    if !meta.is_file() {
        return Err("not_a_file".into());
    }
    let at = Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.to_rfc3339())
        .unwrap_or_default();
    verify_archive(&path, &name, at, VERIFY_CAPS).map(Some)
}

/// Read-only: checks the newest backup and reports. Writes and deletes nothing.
#[tauri::command]
#[specta::specta]
pub async fn backup_verify_last() -> Result<Option<VerifyReport>, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || {
        let s = load_settings(&data)?;
        verify_last_in(&home, &data, &s.dir, &BUSY, &VERIFY_BUSY)
    })
    .await
    .map_err(|_| "io_error".to_string())?
}

#[derive(Serialize, Clone)]
pub(crate) struct ProgressEvent<'a> {
    pub(crate) phase: &'a str,
    pub(crate) pct: u8,
}

/// One backup with progress events, then the outcome is written to the settings. Shared by
/// `backup_now` and the nightly scheduler; the single-flight flag is `BUSY`, so they never overlap.
/// A refused second run (`already_running`) records nothing: the run that holds the flag will.
pub(crate) async fn run_and_record(app: &tauri::AppHandle) -> Result<BackupRunResult, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    let d2 = data.clone();
    let settings = tokio::task::spawn_blocking(move || load_settings(&d2))
        .await
        .map_err(|_| "io_error".to_string())??;
    let emit = |phase: &str, pct: u8| {
        let _ = app.emit(PROGRESS_EVENT, ProgressEvent { phase, pct });
    };
    let out = run_backup(&home, &data, &settings, &free_bytes, &emit, &BUSY).await;
    let code = match &out {
        Ok(_) => "ok".to_string(),
        Err(e) => e.clone(),
    };
    if code != "already_running" {
        let ok = out.is_ok();
        let at = Local::now().to_rfc3339();
        let _ = tokio::task::spawn_blocking(move || record_run(&data, &at, &code, ok)).await;
    }
    out
}

#[tauri::command]
#[specta::specta]
pub async fn backup_now(app: tauri::AppHandle) -> Result<BackupRunResult, String> {
    run_and_record(&app).await
}

#[cfg(test)]
mod tests;
