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
const PROGRESS_EVENT: &str = "backup://progress";
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

fn is_secret_field(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SECRET_FIELD_NAMES.contains(&k.as_str())
        || SECRET_FIELD_SUFFIXES.iter().any(|s| k.ends_with(s))
        || EXCLUDED_NAME_FRAGMENTS
            .iter()
            .any(|f| !f.starts_with('.') && k.contains(f) && !k.contains("threshold"))
}

fn redact(v: &mut Value, removed: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                if is_secret_field(&k) {
                    map.remove(&k);
                    removed.push(k);
                } else if let Some(child) = map.get_mut(&k) {
                    redact(child, removed);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| redact(i, removed)),
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSettings {
    pub nightly: bool,
    pub at: String,
    pub dir: String,
    pub keep: u32,
    pub before_update: bool,
    pub skip_on_battery_below: u32,
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
        }
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSettingsPatch {
    pub nightly: Option<bool>,
    pub at: Option<String>,
    pub dir: Option<String>,
    pub keep: Option<i64>,
    pub before_update: Option<bool>,
    pub skip_on_battery_below: Option<i64>,
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
    if let Some(v) = patch.before_update {
        s.before_update = v;
    }
    save_settings(data_dir, &s)?;
    Ok(s)
}

// ---- listing ----

#[derive(Serialize, Debug, Clone, PartialEq)]
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
fn parse_backup_name(name: &str) -> Option<NaiveDateTime> {
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

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackupRunResult {
    pub entry: BackupEntry,
    /// Left-out files and removed secret fields (names only).
    pub skipped: Vec<String>,
}

static BUSY: AtomicBool = AtomicBool::new(false);

/// Holds the single-backup flag; released on drop (also on early return or panic).
struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
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
    };
    let PackageInfo { skipped, .. } = pack_to(data_dir, &dir, &name, &opts).await?;
    progress("Done", 100);
    let entry = list_in(home, data_dir, &settings.dir, Local::now())?
        .into_iter()
        .find(|e| e.id == name)
        .ok_or_else(|| "io_error".to_string())?;
    Ok(BackupRunResult { entry, skipped })
}

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "dir_invalid".to_string())
}

#[tauri::command]
pub async fn backup_settings_get() -> Result<BackupSettings, String> {
    let data = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || load_settings(&data))
        .await
        .map_err(|_| "io_error".to_string())?
}

#[tauri::command]
pub async fn backup_settings_set(patch: BackupSettingsPatch) -> Result<BackupSettings, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || apply_patch(&home, &data, patch))
        .await
        .map_err(|_| "io_error".to_string())?
}

#[tauri::command]
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
const VERIFY_MAX_ENTRIES: usize = 200_000;
const VERIFY_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// A per-file hash map is ~100 bytes per file, so 32 MiB covers the entry cap.
const VERIFY_MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const VERIFY_RATIO_FLOOR: u64 = 1024 * 1024 * 1024;
const VERIFY_MAX_RATIO: u64 = 200;
const VERIFY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(600);
static VERIFY_BUSY: AtomicBool = AtomicBool::new(false);
const VERIFY_MAX_MISMATCHES: usize = 50;

#[derive(Serialize, Debug, Clone, PartialEq)]
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
fn clean_name(s: &str) -> String {
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
struct VerifyCaps {
    entries: usize,
    total: u64,
    ratio_floor: u64,
    deadline: std::time::Duration,
}
const VERIFY_CAPS: VerifyCaps = VerifyCaps {
    entries: VERIFY_MAX_ENTRIES,
    total: VERIFY_MAX_TOTAL_BYTES,
    ratio_floor: VERIFY_RATIO_FLOOR,
    deadline: VERIFY_DEADLINE,
};

fn verify_archive(
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
struct ProgressEvent<'a> {
    phase: &'a str,
    pct: u8,
}

#[tauri::command]
pub async fn backup_now(app: tauri::AppHandle) -> Result<BackupRunResult, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    let d2 = data.clone();
    let settings = tokio::task::spawn_blocking(move || load_settings(&d2))
        .await
        .map_err(|_| "io_error".to_string())??;
    let emit = |phase: &str, pct: u8| {
        let _ = app.emit(PROGRESS_EVENT, ProgressEvent { phase, pct });
    };
    run_backup(&home, &data, &settings, &free_bytes, &emit, &BUSY).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex as M;

    fn fake_home(tag: &str) -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "nexus-backups-test-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(home.join("Desktop")).unwrap();
        let data = home.join("Library-data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("goals.json"), "[1]").unwrap();
        (home.canonicalize().unwrap(), data.canonicalize().unwrap())
    }

    fn patch_dir(d: &str) -> BackupSettingsPatch {
        BackupSettingsPatch {
            dir: Some(d.into()),
            ..Default::default()
        }
    }

    fn entries(path: &Path) -> Vec<String> {
        let f = fs::File::open(path).unwrap();
        let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
        a.entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect()
    }

    fn all_contents(path: &Path) -> String {
        use std::io::Read;
        let f = fs::File::open(path).unwrap();
        let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
        let mut out = String::new();
        for e in a.entries().unwrap() {
            let mut e = e.unwrap();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            out.push_str(&String::from_utf8_lossy(&b));
        }
        out
    }

    const PLENTY: &(dyn Fn(&Path) -> Result<u64, String> + Send + Sync) = &|_| Ok(u64::MAX);

    // ---- settings ----

    #[test]
    fn defaults_match_the_ts_defaults_and_serialise_camel_case() {
        let (home, data) = fake_home("defaults");
        let s = load_settings(&data).unwrap();
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"nightly":true,"at":"03:00","dir":"~/Amara Backups","keep":14,"beforeUpdate":true,"skipOnBatteryBelow":20})
        );
        assert!(!settings_path(&data).exists(), "a read writes nothing");
        let _ = home;
    }

    #[test]
    fn a_valid_patch_persists_atomically_0600() {
        let (home, data) = fake_home("persist");
        let p = BackupSettingsPatch {
            at: Some("23:59".into()),
            keep: Some(60),
            skip_on_battery_below: Some(0),
            nightly: Some(false),
            dir: Some("~/Custom Backups".into()),
            ..Default::default()
        };
        let s = apply_patch(&home, &data, p).unwrap();
        assert_eq!(load_settings(&data).unwrap(), s);
        assert_eq!(s.dir, "~/Custom Backups");
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(settings_path(&data))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let junk = fs::read_dir(&data)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(junk, 0);
    }

    #[test]
    fn bad_values_are_refused_and_change_nothing() {
        let (home, data) = fake_home("bad");
        for at in ["24:00", "3:00", "03:60", "ab:cd", "03-00", "", "03:000"] {
            let p = BackupSettingsPatch {
                at: Some(at.into()),
                ..Default::default()
            };
            assert_eq!(
                apply_patch(&home, &data, p).unwrap_err(),
                "invalid_time",
                "{at}"
            );
        }
        for k in [0, 61, -1] {
            let p = BackupSettingsPatch {
                keep: Some(k),
                ..Default::default()
            };
            assert_eq!(apply_patch(&home, &data, p).unwrap_err(), "invalid_keep");
        }
        for b in [-1, 101] {
            let p = BackupSettingsPatch {
                skip_on_battery_below: Some(b),
                ..Default::default()
            };
            assert_eq!(apply_patch(&home, &data, p).unwrap_err(), "invalid_battery");
        }
        // A bad field next to a good one: all-or-nothing.
        let p = BackupSettingsPatch {
            nightly: Some(false),
            keep: Some(0),
            ..Default::default()
        };
        assert!(apply_patch(&home, &data, p).is_err());
        assert!(!settings_path(&data).exists());
        for ok in ["00:00", "23:59", "03:00"] {
            let p = BackupSettingsPatch {
                at: Some(ok.into()),
                ..Default::default()
            };
            assert!(apply_patch(&home, &data, p).is_ok(), "{ok}");
        }
    }

    #[test]
    fn dir_must_be_inside_home_and_outside_app_data() {
        let (home, data) = fake_home("dir");
        let outside =
            std::env::temp_dir().join(format!("outside-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&outside).unwrap();
        let outside_s = outside.to_string_lossy().to_string();
        for (d, code) in [
            (outside_s.as_str(), "dir_not_in_home"),
            ("/etc", "dir_not_in_home"),
            ("~", "dir_not_in_home"),
            ("relative/path", "dir_invalid"),
            ("~/../escape", "dir_invalid"),
            ("", "dir_invalid"),
        ] {
            assert_eq!(
                apply_patch(&home, &data, patch_dir(d)).unwrap_err(),
                code,
                "{d}"
            );
        }
        // inside app data, the app data dir itself, below it, or a parent of it
        let inside = data.join("backups").to_string_lossy().to_string();
        assert_eq!(
            apply_patch(&home, &data, patch_dir(&inside)).unwrap_err(),
            "dir_in_app_data"
        );
        assert_eq!(
            apply_patch(&home, &data, patch_dir(&data.to_string_lossy())).unwrap_err(),
            "dir_in_app_data"
        );
        let nested_data = home.join("a").join("b");
        fs::create_dir_all(&nested_data).unwrap();
        assert_eq!(
            apply_patch(&home, &nested_data, patch_dir("~/a")).unwrap_err(),
            "dir_in_app_data"
        );
        assert!(apply_patch(&home, &data, patch_dir("~/Amara Backups")).is_ok());
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_symlinked_dir_or_parent_is_refused() {
        let (home, data) = fake_home("symlink");
        let real = home.join("real");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, home.join("link")).unwrap();
        assert_eq!(
            apply_patch(&home, &data, patch_dir("~/link")).unwrap_err(),
            "dir_is_symlink"
        );
        assert_eq!(
            apply_patch(&home, &data, patch_dir("~/link/sub")).unwrap_err(),
            "dir_is_symlink"
        );
        let file = home.join("afile");
        fs::write(&file, "x").unwrap();
        assert_eq!(
            apply_patch(&home, &data, patch_dir("~/afile")).unwrap_err(),
            "dir_not_a_directory"
        );
    }

    // ---- backup_now ----

    fn seed_secrets(data: &Path) {
        fs::write(data.join("amara-n8n.key"), "SENTINEL-KEYFILE").unwrap();
        fs::write(data.join("crew_service_token"), "SENTINEL-CREW").unwrap();
        fs::write(data.join("n8n_api_key"), "SENTINEL-N8N").unwrap();
        fs::write(data.join(".env"), "HA_TOKEN=SENTINEL-HA").unwrap();
        fs::write(data.join(".amara-n8n.key.abc.tmp"), "SENTINEL-TMP").unwrap();
        fs::write(data.join("my-Token.json"), "SENTINEL-TOKENFILE").unwrap();
        fs::write(data.join("app_secret.txt"), "SENTINEL-SECRETFILE").unwrap();
        fs::create_dir_all(data.join("sessions/sub")).unwrap();
        fs::write(data.join("sessions/sub/amara-n8n.key"), "SENTINEL-NESTED").unwrap();
        fs::write(data.join("sessions/sub/x.pem"), "SENTINEL-PEM").unwrap();
        fs::write(data.join("sessions/rec.ndjson"), "keepme").unwrap();
        // JSON stores whose secret field is a plain name (arm_config.json worker_api_keys[].key)
        fs::write(
            data.join("arm_config.json"),
            r#"{"poll_interval_secs":5,"worker_api_keys":[{"name":"w1","key":"SENTINEL-ARM"}]}"#,
        )
        .unwrap();
        fs::write(
            data.join("sessions/s1.json"),
            r#"{"auth":"SENTINEL-AUTH","eeg":[1,2]}"#,
        )
        .unwrap();
        fs::write(data.join("broken.json"), "{SENTINEL-BROKEN").unwrap();
        fs::write(
            data.join("config.json"),
            r#"{"theme":"dark","telegram_bot_token":"SENTINEL-TG","anthropic_api_key":"SENTINEL-AK","n8n_webhook_secret":"SENTINEL-WH","gateway_auth_token":"SENTINEL-GW","llm_token_threshold":512,"nested":{"x_password":"SENTINEL-PW","ok":1}}"#,
        )
        .unwrap();
    }

    fn quiet() -> impl Fn(&str, u8) + Send + Sync {
        |_, _| {}
    }

    #[tokio::test]
    async fn backup_now_writes_0600_into_a_new_0700_dir_without_any_secret() {
        let (home, data) = fake_home("run");
        seed_secrets(&data);
        let s = BackupSettings::default();
        let busy = AtomicBool::new(false);
        let r = run_backup(&home, &data, &s, PLENTY, &quiet(), &busy)
            .await
            .unwrap();
        let dir = home.join("Amara Backups");
        let arch = dir.join(&r.entry.id);
        assert!(r.entry.id.starts_with("amara-backup-") && r.entry.id.ends_with(".tar.gz"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&arch).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!busy.load(Ordering::SeqCst), "the flag is released");

        let names = entries(&arch);
        for bad in [
            "amara-n8n.key",
            "crew_service_token",
            "n8n_api_key",
            ".env",
            ".amara-n8n.key.",
            "my-Token",
            "app_secret",
            ".pem",
        ] {
            assert!(!names.iter().any(|n| n.contains(bad)), "{bad} in {names:?}");
        }
        let text = all_contents(&arch);
        assert!(
            !text.contains("SENTINEL"),
            "a secret value reached the archive"
        );
        // ...but ordinary data and the non-secret config fields did.
        assert!(names.iter().any(|n| n.ends_with("/goals.json")));
        assert!(names.iter().any(|n| n.ends_with("sessions/rec.ndjson")));
        assert!(text.contains("keepme") && text.contains("\"theme\": \"dark\""));
        // non-secret fields of redacted JSON stores survive (worker_api_keys goes whole: its name holds api_key)
        assert!(text.contains("poll_interval_secs") && text.contains("\"eeg\":[1,2]"));
        assert!(text.contains("llm_token_threshold") && text.contains("\"ok\": 1"));
        assert!(!text.contains("\"telegram_bot_token\""));
        assert!(r.skipped.iter().any(|x| x.contains("telegram_bot_token")));
        assert!(!format!("{:?}", r.skipped).contains("SENTINEL"));
        // The source files are untouched.
        assert!(data.join("amara-n8n.key").is_file() && data.join(".env").is_file());
    }

    /// Fail-before: with the filter off (what `pack_it_up` still does) the same data dir puts
    /// the secrets in the archive, so the scan in the test above really proves the filter.
    #[tokio::test]
    async fn without_the_filter_the_same_scan_would_fail() {
        let (home, data) = fake_home("nofilter");
        seed_secrets(&data);
        let out = home.join("out");
        fs::create_dir_all(&out).unwrap();
        let info = pack_to(&data, &out, "x.tar.gz", &PackOpts::default())
            .await
            .unwrap();
        let text = all_contents(Path::new(&info.path));
        assert!(text.contains("SENTINEL-HA") && text.contains("SENTINEL-TG"));
    }

    #[tokio::test]
    async fn the_desktop_pack_it_up_archive_leaves_secrets_out_too() {
        let (home, data) = fake_home("packitup");
        fs::create_dir_all(home.join("Desktop")).unwrap();
        seed_secrets(&data);
        let info = super::super::packager::pack_at(&home, &data).await.unwrap();
        let text = all_contents(Path::new(&info.path));
        assert!(
            !text.contains("SENTINEL"),
            "a secret reached the Desktop archive"
        );
        assert!(text.contains("keepme") && text.contains("poll_interval_secs"));
    }

    #[test]
    fn a_restore_keeps_the_live_secrets_the_backup_left_out() {
        let restored = br#"{"theme":"light","nested":{"ok":2},"worker_api_keys":[]}"#;
        let current = br#"{"theme":"dark","telegram_bot_token":"LIVE-TG","nested":{"ok":1,"x_password":"LIVE-PW"},"worker_api_keys":[{"key":"LIVE-ARM"}],"plain":"gone"}"#;
        let v: Value =
            serde_json::from_slice(&merge_kept_secrets(restored, current).unwrap()).unwrap();
        assert_eq!(v["theme"], "light"); // the backup wins for normal fields
        assert_eq!(v["nested"]["ok"], 2);
        assert_eq!(v["telegram_bot_token"], "LIVE-TG"); // a secret it lacks is kept
        assert_eq!(v["nested"]["x_password"], "LIVE-PW");
        assert_eq!(v["worker_api_keys"], serde_json::json!([])); // present in the backup: backup wins
        assert!(v.get("plain").is_none()); // non-secret live-only fields are not resurrected
        assert!(merge_kept_secrets(b"not json", current).is_none());
    }

    #[test]
    fn a_restore_that_moves_an_endpoint_does_not_get_the_live_token() {
        let current = br#"{"worker_base_url":"http://127.0.0.1:9000","gateway_auth_token":"LIVE-GW","telegram_bot_token":"LIVE-TG","n8n":{"url":"http://127.0.0.1:5678","webhook_secret":"LIVE-WH"}}"#;
        // top-level endpoint moved: no top-level or nested secret is carried over
        let moved =
            br#"{"worker_base_url":"https://evil.example","n8n":{"url":"http://127.0.0.1:5678"}}"#;
        let v: Value =
            serde_json::from_slice(&merge_kept_secrets(moved, current).unwrap()).unwrap();
        assert!(v.get("gateway_auth_token").is_none() && v.get("telegram_bot_token").is_none());
        assert!(v["n8n"].get("webhook_secret").is_none());
        // only the nested endpoint moved: top-level secrets stay, the nested one is dropped
        let nested =
            br#"{"worker_base_url":"http://127.0.0.1:9000","n8n":{"url":"https://evil.example"}}"#;
        let v: Value =
            serde_json::from_slice(&merge_kept_secrets(nested, current).unwrap()).unwrap();
        assert_eq!(v["gateway_auth_token"], "LIVE-GW");
        assert!(v["n8n"].get("webhook_secret").is_none());
        // a removed endpoint counts as a change too
        let gone = br#"{"n8n":{"url":"http://127.0.0.1:5678"}}"#;
        let v: Value = serde_json::from_slice(&merge_kept_secrets(gone, current).unwrap()).unwrap();
        assert!(v.get("gateway_auth_token").is_none());
    }

    #[tokio::test]
    async fn a_backup_still_passes_the_restore_allow_list() {
        use super::super::packager::{
            find_pack_root, plan_restore, restore_target, unpack_archive_checked,
        };
        let (home, data) = fake_home("allow");
        seed_secrets(&data);
        let busy = AtomicBool::new(false);
        let r = run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &quiet(),
            &busy,
        )
        .await
        .unwrap();
        // every file the archive holds under its top folder is either restorable or manifest.json
        let arch = home.join("Amara Backups").join(&r.entry.id);
        for n in entries(&arch) {
            if n.ends_with('/') || n.contains("/._") || n.starts_with("._") {
                continue;
            }
            let rel: Vec<String> = n.split('/').skip(1).map(str::to_string).collect();
            if rel == ["manifest.json"] {
                continue;
            }
            assert!(restore_target(&rel).is_some(), "not on the allow-list: {n}");
        }
        let staging = home.join("staging");
        fs::create_dir(&staging).unwrap();
        unpack_archive_checked(&arch, &staging, 2 * 1024 * 1024 * 1024, 50_000).unwrap();
        let plan = plan_restore(&find_pack_root(&staging).unwrap()).unwrap();
        assert!(plan.items.iter().any(|i| i.rel.ends_with("goals.json")));
    }

    #[tokio::test]
    async fn a_second_concurrent_call_is_refused() {
        let (home, data) = fake_home("busy");
        let busy = AtomicBool::new(false);
        let _held = BusyGuard::acquire(&busy).unwrap();
        let e = run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &quiet(),
            &busy,
        )
        .await
        .unwrap_err();
        assert_eq!(e, "already_running");
        assert!(
            busy.load(Ordering::SeqCst),
            "the refused call does not clear the holder's flag"
        );
        assert!(!home.join("Amara Backups").exists());
    }

    #[tokio::test]
    async fn too_little_free_space_refuses_before_writing() {
        let (home, data) = fake_home("space");
        fs::write(data.join("big.bin"), vec![0u8; 4096]).unwrap();
        let need = dir_size(&data) + FREE_SPACE_MARGIN;
        let busy = AtomicBool::new(false);
        let seen: M<HashSet<u64>> = M::new(HashSet::new());
        let low = |_: &Path| {
            seen.lock().unwrap().insert(need - 1);
            Ok(need - 1)
        };
        let e = run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            &low,
            &quiet(),
            &busy,
        )
        .await
        .unwrap_err();
        assert_eq!(e, "not_enough_space");
        let made = fs::read_dir(home.join("Amara Backups")).unwrap().count();
        assert_eq!(made, 0, "no archive was started");
        let exact = |_: &Path| Ok(need);
        assert!(run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            &exact,
            &quiet(),
            &busy
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn progress_events_run_up_to_100() {
        let (home, data) = fake_home("progress");
        let log: M<Vec<(String, u8)>> = M::new(Vec::new());
        let rec = |p: &str, n: u8| log.lock().unwrap().push((p.to_string(), n));
        let busy = AtomicBool::new(false);
        run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &rec,
            &busy,
        )
        .await
        .unwrap();
        let log = log.lock().unwrap();
        assert!(log.windows(2).all(|w| w[0].1 <= w[1].1), "{log:?}");
        assert_eq!(log.last().unwrap(), &("Done".to_string(), 100));
        assert!(log.iter().any(|(p, _)| p == "Compressing"));
    }

    #[tokio::test]
    async fn a_symlinked_destination_is_refused() {
        let (home, data) = fake_home("symdest");
        let real = home.join("real");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, home.join("Amara Backups")).unwrap();
        let busy = AtomicBool::new(false);
        let e = run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &quiet(),
            &busy,
        )
        .await
        .unwrap_err();
        assert_eq!(e, "dir_is_symlink");
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    }

    #[test]
    fn free_bytes_reads_a_real_volume() {
        assert!(free_bytes(&std::env::temp_dir()).unwrap() > 0);
        assert!(free_bytes(Path::new("/definitely/not/here")).is_err());
    }

    // ---- list ----

    #[test]
    fn list_shows_only_backup_files_newest_first_and_never_follows_links() {
        let (home, data) = fake_home("list");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let put = |n: &str, c: &str| fs::write(dir.join(n), c).unwrap();
        put("amara-backup-20261110-030000.tar.gz", "aaaa");
        put("amara-backup-20261112-030000.tar.gz", "bb");
        put("nexus-backup-20261111_101500.tar.gz", "c");
        put("notes.txt", "x");
        put("amara-backup-2026-bad.tar.gz", "x");
        put("amara-backup-20261112-030000.tar.gz.part", "x");
        put("amara-n8n.key", "SENTINEL");
        fs::create_dir(dir.join("amara-backup-20261113-030000.tar.gz")).unwrap();
        std::os::unix::fs::symlink(
            dir.join("amara-backup-20261110-030000.tar.gz"),
            dir.join("amara-backup-20261114-030000.tar.gz"),
        )
        .unwrap();
        let now = Local.with_ymd_and_hms(2026, 11, 12, 12, 0, 0).unwrap();
        let l = list_in(&home, &data, "~/Amara Backups", now).unwrap();
        let ids: Vec<&str> = l.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "amara-backup-20261112-030000.tar.gz",
                "nexus-backup-20261111_101500.tar.gz",
                "amara-backup-20261110-030000.tar.gz"
            ]
        );
        assert_eq!(l[0].days_old, 0);
        assert_eq!(l[1].days_old, 1);
        assert_eq!(l[2].days_old, 2);
        assert_eq!(l[0].size_bytes, 2);
        assert_eq!(l[0].label, "Nov 12, 3:00 AM");
        let j = serde_json::to_value(&l[0]).unwrap();
        for k in ["id", "label", "madeAt", "sizeBytes", "daysOld"] {
            assert!(j.get(k).is_some(), "{k}");
        }
        // a missing folder is an empty list, not an error
        assert!(list_in(&home, &data, "~/Nope", now).unwrap().is_empty());
    }

    // ---- exclusions ----

    #[test]
    fn exclusion_rules() {
        for n in [
            "amara-n8n.key",
            "crew_service_token",
            "n8n_api_key",
            ".env",
            ".env.local",
            "A.KEY",
            "x_token",
            "client_secret.json",
            "cert.pem",
            ".amara-n8n.key.9f.tmp",
            "config.json",
        ] {
            assert!(is_excluded_name(n, 0), "{n}");
        }
        assert!(
            !is_excluded_name("config.json", 1),
            "only the top-level config is redacted"
        );
        for n in [
            "goals.json",
            "events.db",
            "kv",
            "eeg_20260217_100521.ndjson",
            "sessions",
        ] {
            assert!(!is_excluded_name(n, 0), "{n}");
        }
        assert!(is_secret_field("telegram_bot_token") && is_secret_field("anthropic_api_key"));
        assert!(!is_secret_field("llm_token_threshold") && !is_secret_field("theme"));
    }

    // ---- module source ----

    #[test]
    fn this_module_has_no_delete_or_prune_and_no_scheduler() {
        let src = include_str!("backups.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        for bad in [
            "remove_file",
            "remove_dir",
            "fs::remove",
            "tokio::time",
            "interval(",
            "thread::sleep",
            "spawn(",
        ] {
            assert!(!body.contains(bad), "backups.rs must not contain {bad}");
        }
    }

    #[test]
    fn nothing_in_the_data_dir_or_the_backup_dir_is_deleted_by_a_run() {
        // Fail-before shape: a run over a populated backup dir leaves every old backup alone.
        let (home, data) = fake_home("nodelete");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..20 {
            fs::write(
                dir.join(format!("amara-backup-202610{:02}-030000.tar.gz", i + 1)),
                "old",
            )
            .unwrap();
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let s = BackupSettings {
            keep: 1,
            ..Default::default()
        };
        let busy = AtomicBool::new(false);
        rt.block_on(run_backup(&home, &data, &s, PLENTY, &quiet(), &busy))
            .unwrap();
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            21,
            "keep=1 deleted nothing"
        );
    }

    // ---- backup_verify_last ----

    fn make_archive(path: &Path, files: &[(&str, &[u8])]) {
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (n, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            b.append_data(&mut h, format!("amara-pack-1/{n}"), *data)
                .unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }

    fn snapshot(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| {
                let m = fs::symlink_metadata(e.path()).unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    m.len(),
                    m.modified().unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }

    fn sha_hex(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(b))
    }

    const NEWEST: &str = "amara-backup-20261112-030000.tar.gz";

    #[test]
    fn verify_none_when_there_is_no_archive() {
        let (home, data) = fake_home("vnone");
        assert_eq!(
            verify_last_in(
                &home,
                &data,
                "~/Amara Backups",
                &AtomicBool::new(false),
                &AtomicBool::new(false)
            )
            .unwrap(),
            None
        );
        fs::create_dir_all(home.join("Amara Backups")).unwrap();
        fs::write(home.join("Amara Backups/notes.txt"), "x").unwrap();
        assert_eq!(
            verify_last_in(
                &home,
                &data,
                "~/Amara Backups",
                &AtomicBool::new(false),
                &AtomicBool::new(false)
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn verify_a_good_archive_reads_every_entry_and_changes_nothing() {
        let (home, data) = fake_home("vgood");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        make_archive(
            &dir.join("amara-backup-20261101-030000.tar.gz"),
            &[("manifest.json", b"{}")],
        );
        make_archive(
            &dir.join(NEWEST),
            &[
                ("manifest.json", b"{\"version\":\"0.1.0\"}"),
                ("goals.json", b"[1]"),
            ],
        );
        let before = snapshot(&dir);
        let r = verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(r.file, NEWEST);
        assert!(r.ok, "{r:?}");
        assert_eq!((r.checked, r.mode.as_str()), (2, "structure"));
        assert!(r.mismatched.is_empty() && r.size > 0 && !r.at.is_empty());
        assert_eq!(snapshot(&dir), before);
    }

    #[test]
    fn verify_with_hashes_flags_a_tampered_entry() {
        let (home, data) = fake_home("vtamper");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let man = |h: &str| format!("{{\"sha256\":{{\"goals.json\":\"{h}\"}}}}").into_bytes();
        make_archive(
            &dir.join(NEWEST),
            &[
                ("manifest.json", &man(&sha_hex(b"[1]"))),
                ("goals.json", b"[1]"),
            ],
        );
        let r = verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(r.ok && r.mode == "sha256", "{r:?}");
        make_archive(
            &dir.join(NEWEST),
            &[
                ("manifest.json", &man(&sha_hex(b"[1]"))),
                ("goals.json", b"[2]"),
            ],
        );
        let before = snapshot(&dir);
        let r = verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(!r.ok);
        assert_eq!(r.mismatched, vec!["goals.json".to_string()]);
        assert_eq!(snapshot(&dir), before);
    }

    #[test]
    fn verify_flags_secret_names_and_a_truncated_archive() {
        let (home, data) = fake_home("vsecret");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        make_archive(
            &dir.join(NEWEST),
            &[("manifest.json", b"{}"), ("telegram_token.txt", b"x")],
        );
        let r = verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(!r.ok);
        assert_eq!(
            r.mismatched,
            vec!["amara-pack-1/telegram_token.txt".to_string()]
        );
        let bytes = fs::read(dir.join(NEWEST)).unwrap();
        fs::write(dir.join(NEWEST), &bytes[..bytes.len() / 2]).unwrap();
        let r = verify_last_in(
            &home,
            &data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(!r.ok && !r.mismatched.is_empty());
    }

    #[test]
    fn verify_refuses_a_symlinked_archive() {
        let (home, data) = fake_home("vsym");
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let real = home.join("elsewhere.tar.gz");
        make_archive(&real, &[("manifest.json", b"{}")]);
        std::os::unix::fs::symlink(&real, dir.join(NEWEST)).unwrap();
        let before = snapshot(&dir);
        assert_eq!(
            verify_last_in(
                &home,
                &data,
                "~/Amara Backups",
                &AtomicBool::new(false),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            "symlink_refused"
        );
        assert_eq!(snapshot(&dir), before);
    }

    // ---- verify hardening ----

    fn vopen(home: &Path, data: &Path) -> Result<Option<VerifyReport>, String> {
        verify_last_in(
            home,
            data,
            "~/Amara Backups",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
    }

    fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let (home, data) = fake_home(tag);
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(NEWEST);
        (home, data, p)
    }

    #[test]
    fn verify_is_refused_while_a_backup_runs_or_another_verify_is_running() {
        let (home, data, p) = setup("vbusy");
        make_archive(&p, &[("manifest.json", b"{}")]);
        let on = AtomicBool::new(true);
        let off = AtomicBool::new(false);
        assert_eq!(
            verify_last_in(&home, &data, "~/Amara Backups", &on, &off).unwrap_err(),
            "backup_running"
        );
        assert_eq!(
            verify_last_in(&home, &data, "~/Amara Backups", &off, &on).unwrap_err(),
            "busy"
        );
        assert!(
            on.load(Ordering::SeqCst),
            "a refused call must not release the other flag"
        );
        assert!(vopen(&home, &data).unwrap().unwrap().ok);
    }

    #[test]
    fn verify_checks_the_gzip_trailer() {
        let (home, data, p) = setup("vtrailer");
        make_archive(&p, &[("manifest.json", b"{}"), ("goals.json", b"[1]")]);
        let mut b = fs::read(&p).unwrap();
        let n = b.len();
        b[n - 8] ^= 0xff; // CRC32 in the trailer
        fs::write(&p, &b).unwrap();
        let r = vopen(&home, &data).unwrap().unwrap();
        assert!(!r.ok, "{r:?}");
        assert!(r.mismatched.contains(&"archive_unreadable".to_string()));
    }

    #[test]
    fn verify_aborts_on_streamed_size_ratio_deadline_and_entry_caps() {
        let (home, data, p) = setup("vcaps");
        make_archive(
            &p,
            &[("manifest.json", b"{}"), ("big.bin", &vec![0u8; 1 << 20])],
        );
        let go = |caps: VerifyCaps| verify_archive(&p, NEWEST, String::new(), caps);
        let loose = VerifyCaps {
            entries: 100,
            total: u64::MAX,
            ratio_floor: u64::MAX,
            deadline: std::time::Duration::from_secs(60),
        };
        assert!(go(loose).unwrap().ok);
        assert_eq!(
            go(VerifyCaps {
                total: 1000,
                ..loose
            })
            .unwrap_err(),
            "archive_too_large"
        );
        assert_eq!(
            go(VerifyCaps {
                ratio_floor: 1000,
                ..loose
            })
            .unwrap_err(),
            "compression_ratio"
        );
        assert_eq!(
            go(VerifyCaps {
                deadline: std::time::Duration::ZERO,
                ..loose
            })
            .unwrap_err(),
            "verify_timeout"
        );
        assert_eq!(
            go(VerifyCaps {
                entries: 1,
                ..loose
            })
            .unwrap_err(),
            "too_many_entries"
        );
        let _ = (home, data);
    }

    #[test]
    fn verify_counts_directories_toward_the_entry_cap() {
        let (_h, _d, p) = setup("vdirs");
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(&p).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for i in 0..5 {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o700);
            h.set_cksum();
            b.append_data(&mut h, format!("amara-pack-1/d{i}/"), &b""[..])
                .unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        let caps = VerifyCaps {
            entries: 3,
            ..VERIFY_CAPS
        };
        assert_eq!(
            verify_archive(&p, NEWEST, String::new(), caps).unwrap_err(),
            "too_many_entries"
        );
    }

    #[test]
    fn verify_treats_a_bad_manifest_as_broken_not_structure_mode() {
        let hex = sha_hex(b"[1]");
        type Case = (&'static str, Vec<(&'static str, Vec<u8>)>);
        let cases: Vec<Case> = vec![
            ("unparseable", vec![("manifest.json", b"{nope".to_vec())]),
            ("not_object", vec![("manifest.json", b"[1]".to_vec())]),
            (
                "bad_hash",
                vec![("manifest.json", b"{\"sha256\":{\"a\":\"zz\"}}".to_vec())],
            ),
            (
                "bad_map",
                vec![("manifest.json", b"{\"sha256\":5}".to_vec())],
            ),
            (
                "duplicate",
                vec![
                    ("manifest.json", b"{}".to_vec()),
                    ("manifest.json", b"{}".to_vec()),
                ],
            ),
            (
                "oversized",
                vec![("manifest.json", {
                    let mut v = b"{\"x\":\"".to_vec();
                    v.extend(vec![b'a'; (VERIFY_MAX_MANIFEST_BYTES as usize) + 10]);
                    v.extend(b"\"}");
                    v
                })],
            ),
            (
                "missing_listed",
                vec![(
                    "manifest.json",
                    format!("{{\"sha256\":{{\"gone.json\":\"{hex}\"}}}}").into_bytes(),
                )],
            ),
        ];
        for (tag, files) in cases {
            let (home, data, p) = setup(&format!("vman-{tag}"));
            let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
            make_archive(&p, &refs);
            let r = vopen(&home, &data).unwrap().unwrap();
            assert!(!r.ok, "{tag}: {r:?}");
            if tag == "missing_listed" {
                assert_eq!(r.mismatched, vec!["missing: gone.json".to_string()]);
            } else {
                assert!(
                    r.mismatched.contains(&"manifest_invalid".to_string()),
                    "{tag}: {r:?}"
                );
            }
        }
        let (home, data, p) = setup("vman-none");
        make_archive(&p, &[("goals.json", b"[1]")]);
        let r = vopen(&home, &data).unwrap().unwrap();
        assert!(!r.ok && r.mismatched == vec!["manifest_missing".to_string()]);
    }

    #[test]
    fn returned_names_are_short_and_free_of_control_and_bidi_characters() {
        let dirty = format!("a\u{202E}b\u{200B}c\nd\u{0}{}", "x".repeat(500));
        let c = clean_name(&dirty);
        assert!(c.starts_with("abcd") && c.chars().count() == 200);
        assert!(!c
            .chars()
            .any(|ch| ch.is_control() || ch == '\u{202E}' || ch == '\u{200B}'));
        let (home, data, p) = setup("vnames");
        make_archive(&p, &[("manifest.json", b"{}"), ("tok\u{202E}en.txt", b"x")]);
        let r = vopen(&home, &data).unwrap().unwrap();
        assert_eq!(r.mismatched, vec!["amara-pack-1/token.txt".to_string()]);
    }

    #[tokio::test]
    async fn a_fresh_pack_verifies_in_sha256_mode_and_a_tampered_one_does_not() {
        let (home, data) = fake_home("vpack");
        fs::write(data.join("notes.txt"), "hello").unwrap();
        let busy = AtomicBool::new(false);
        let r = run_backup(
            &home,
            &data,
            &BackupSettings::default(),
            PLENTY,
            &quiet(),
            &busy,
        )
        .await
        .unwrap();
        let v = vopen(&home, &data).unwrap().unwrap();
        assert_eq!(v.file, r.entry.id);
        assert!(v.ok && v.mode == "sha256" && v.checked >= 3, "{v:?}");
        // Rewrite the archive with one entry's bytes changed and the manifest left alone.
        let path = home.join("Amara Backups").join(&r.entry.id);
        let mut a = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&path).unwrap()));
        let mut items: Vec<(String, Vec<u8>)> = Vec::new();
        for e in a.entries().unwrap() {
            let mut e = e.unwrap();
            if e.header().entry_type().is_file() {
                let mut b = Vec::new();
                std::io::Read::read_to_end(&mut e, &mut b).unwrap();
                if e.path().unwrap().ends_with("goals.json") {
                    b = b"[666]".to_vec();
                }
                items.push((e.path().unwrap().to_string_lossy().into_owned(), b));
            }
        }
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (n, d) in &items {
            let mut h = tar::Header::new_gnu();
            h.set_size(d.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            b.append_data(&mut h, n, d.as_slice()).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        let v = vopen(&home, &data).unwrap().unwrap();
        assert!(
            !v.ok && v.mismatched.contains(&"goals.json".to_string()),
            "{v:?}"
        );
    }
}
