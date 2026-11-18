//! Bounded, read-only bridge to the NOOP `noop-local-access` MCP CLI (stdio JSON-RPC).
//! Fixed tool allowlist, validated args, no shell, hard timeout, parsed JSON.
//! Never logs payloads: results may contain biometric values.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const ALLOWED_TOOLS: [&str; 5] = [
    "health_snapshot",
    "metric_series",
    "data_freshness",
    "sleep_summary",
    "workout_summary",
];
const METRIC_KEYS: [&str; 9] = [
    "recovery",
    "hrv",
    "rhr",
    "resp_rate",
    "spo2",
    "strain",
    "sleep_total_min",
    "steps",
    "active_kcal",
];
const SETTINGS_KEY: &str = "noop_settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
pub enum WhoopSource {
    /// "NOOP (recommended)": NOOP owns strap sync; the direct BLE path must not start.
    #[default]
    #[serde(rename = "noop")]
    Noop,
    /// "Direct BLE".
    #[serde(rename = "ble")]
    Ble,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct NoopSettings {
    #[serde(default)]
    pub whoop_source: WhoopSource,
    #[serde(default)]
    pub binary_path: Option<String>,
    #[serde(default)]
    pub db_path: Option<String>,
}

impl NoopSettings {
    /// Gate for any direct WHOOP BLE startup. False whenever NOOP is selected.
    /// No direct WHOOP BLE code is wired in this repo today; any future BLE start must check this.
    pub fn whoop_ble_allowed(&self) -> bool {
        self.whoop_source == WhoopSource::Ble
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum NoopQueryError {
    ToolNotAllowed(String),
    InvalidArgs(String),
    BinaryMissing(String),
    SpawnFailed(String),
    Timeout(u64),
    ProcessFailed(String),
    BadOutput(String),
    ToolError(String),
    InvalidPath(String),
    SandboxMissing(String),
    PinMismatch(String),
    PinMissing(String),
}

impl NoopQueryError {
    /// Variant name only; safe to log (no payload).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ToolNotAllowed(_) => "tool_not_allowed",
            Self::InvalidArgs(_) => "invalid_args",
            Self::BinaryMissing(_) => "binary_missing",
            Self::SpawnFailed(_) => "spawn_failed",
            Self::Timeout(_) => "timeout",
            Self::ProcessFailed(_) => "process_failed",
            Self::BadOutput(_) => "bad_output",
            Self::ToolError(_) => "tool_error",
            Self::InvalidPath(_) => "invalid_path",
            Self::SandboxMissing(_) => "sandbox_missing",
            Self::PinMismatch(_) => "pin_mismatch",
            Self::PinMissing(_) => "pin_missing",
        }
    }
}

pub fn load_settings() -> NoopSettings {
    super::kv::load_kv_internal(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Default NOOP CLI, relative to the user's home (resolved at runtime, never hardcoded).
/// Chosen copy (2026-11-04, Taisun via Nexus Lead): ~/projects/noop-lucid-current.
pub const DEFAULT_BINARY_REL: &str =
    "projects/noop-lucid-current/Packages/NoopLocalAccess/.build/arm64-apple-macosx/release/noop-local-access";

/// `$HOME/<DEFAULT_BINARY_REL>`, or None if there is no home directory.
#[cfg(test)]
pub fn default_binary_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| default_binary_under(&h))
}

pub(crate) fn default_binary_under(home: &Path) -> PathBuf {
    home.join(DEFAULT_BINARY_REL)
}

/// File name of the bundled sidecar, next to the main executable (`Contents/MacOS/`), the same
/// place `amara-router` lives. Staged by the packaging step once the binary exists (Phase 10).
pub const BUNDLED_BINARY_NAME: &str = "noop-local-access";

/// The bundled sidecar inside `exe_dir`, if it is there.
pub(crate) fn bundled_binary_in(exe_dir: &Path) -> Option<PathBuf> {
    let p = exe_dir.join(BUNDLED_BINARY_NAME);
    p.is_file().then_some(p)
}

/// Where the CLI comes from. Order: bundled sidecar, then the `~/projects` build. (`~/.hermes/bin`
/// holds a copy but nothing in Nexus uses it, so it is not a candidate.)
pub(crate) fn resolve_binary(exe_dir: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    exe_dir
        .and_then(bundled_binary_in)
        .or_else(|| home.map(default_binary_under))
}

/// The folder of the main executable, only when the executable sits inside a `.app` bundle.
/// Outside a bundle (dev runs, tests) there is no bundled-sidecar root.
pub(crate) fn bundle_dir_for_exe(exe: &Path) -> Option<PathBuf> {
    let inside_app = exe
        .ancestors()
        .skip(1)
        .any(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")));
    inside_app.then(|| exe.parent().map(Path::to_path_buf))?
}

fn current_exe_dir() -> Option<PathBuf> {
    bundle_dir_for_exe(&std::env::current_exe().ok()?.canonicalize().ok()?)
}

// ── SHA-256 pin ──
//
// The pinned digests live in ONE file, `src-tauri/resources/noop-local-access.sha256`, compiled in.
// Taisun adds a digest there (see the file header). Empty = unpinned (runs, reported as such).

/// Fixed refusal text. Carries no path and no digest.
pub const PIN_MISMATCH: &str = "The NOOP CLI does not match its pinned SHA-256; it was not run.";
const PINS_RAW: &str = include_str!("../../resources/noop-local-access.sha256");

/// Digests from a pin file: `#` comments and blanks skipped; a line that is not 64 hex characters
/// is ignored (so a typo can never become "pinned to garbage" and silently match nothing).
pub(crate) fn parse_pins(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|l| {
            l.split('#')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|l| l.len() == 64 && l.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect()
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_PINS: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

fn active_pins() -> Vec<String> {
    // Tests never read the shipped pin file (their fake binaries would not match it); a test that
    // wants pins sets TEST_PINS. The shipped file is checked by its own test.
    #[cfg(test)]
    return TEST_PINS.with(|t| t.borrow().clone()).unwrap_or_default();
    #[cfg(not(test))]
    parse_pins(PINS_RAW)
}

/// Lowercase hex SHA-256 of a file.
pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Fixed refusal when no pin is listed but one is required.
pub const PIN_MISSING: &str = "No SHA-256 pin is set for the NOOP CLI; it was not run.";

/// Decide from the digest of the exact bytes about to run. `require_pin` is true in release
/// builds and for any bundled binary: an empty list then refuses with `pin_missing`. Otherwise an
/// empty list means "unpinned" and runs. A non-empty list must contain the digest.
pub(crate) fn pin_decision(
    digest: &str,
    pins: &[String],
    require_pin: bool,
) -> Result<(), NoopQueryError> {
    if pins.is_empty() {
        return if require_pin {
            Err(NoopQueryError::PinMissing(PIN_MISSING.into()))
        } else {
            Ok(())
        };
    }
    if pins.iter().any(|p| p == digest) {
        Ok(())
    } else {
        Err(NoopQueryError::PinMismatch(PIN_MISMATCH.into()))
    }
}

/// A private, hashed copy of the binary. The copy is what runs, so a swap of the original after
/// validation cannot be executed. Removed (file and folder) when dropped.
pub(crate) struct PrivateCopy {
    dir: PathBuf,
    path: PathBuf,
}

impl PrivateCopy {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Open `binary` with O_NOFOLLOW, fstat it (regular file, ours), copy the bytes into a fresh
    /// 0700 folder while hashing them, chmod 0500, then check the pin against that hash.
    pub(crate) fn prepare(
        binary: &Path,
        pins: &[String],
        require_pin: bool,
    ) -> Result<Self, NoopQueryError> {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
        let bad = |m: &str| NoopQueryError::InvalidPath(m.to_string());
        let mut src = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(binary)
            .map_err(|_| bad("cannot open the binary"))?;
        let meta = src.metadata().map_err(|_| bad("cannot stat the binary"))?;
        // SAFETY: getuid has no preconditions and cannot fail.
        if !meta.is_file() || meta.uid() != unsafe { libc::getuid() } {
            return Err(bad("binary is not a regular file owned by you"));
        }
        let dir = std::env::temp_dir().join(format!("nexus-noop-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| bad("cannot create a private folder"))?;
        let path = dir.join(BUNDLED_BINARY_NAME);
        let guard = PrivateCopy {
            dir,
            path: path.clone(),
        };
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)
            .map_err(|_| bad("cannot create the private copy"))?;
        let mut h = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = src
                .read(&mut buf)
                .map_err(|_| bad("cannot read the binary"))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n])
                .map_err(|_| bad("cannot write the private copy"))?;
        }
        out.sync_all().ok();
        drop(out);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))
            .map_err(|_| bad("cannot lock the private copy"))?;
        pin_decision(&hex::encode(h.finalize()), pins, require_pin)?;
        Ok(guard)
    }
}

impl Drop for PrivateCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BinaryStatus {
    /// `bundled` | `projects` | `settings` | `none`
    pub source: String,
    pub found: bool,
    /// SHA-256 of the resolved file (not a secret); what Taisun logs and pins.
    pub sha256: Option<String>,
    pub pins_listed: u32,
    /// `unpinned` | `match` | `mismatch` | `unknown`
    pub pin_state: String,
}

pub(crate) fn binary_status_for(
    settings: &NoopSettings,
    exe_dir: Option<&Path>,
    home: Option<&Path>,
    pins: &[String],
) -> BinaryStatus {
    let explicit = settings.binary_path.as_deref().filter(|p| !p.is_empty());
    let (source, path) = match explicit {
        Some(p) => ("settings", Some(PathBuf::from(p))),
        None => match exe_dir.and_then(bundled_binary_in) {
            Some(p) => ("bundled", Some(p)),
            None => ("projects", home.map(default_binary_under)),
        },
    };
    let digest = path
        .as_deref()
        .filter(|p| p.is_file())
        .and_then(|p| sha256_file(p).ok());
    let source = if digest.is_none() && explicit.is_none() {
        "none"
    } else {
        source
    };
    let pin_state = match (&digest, pins.is_empty()) {
        (_, true) => "unpinned",
        (Some(d), false) if pins.iter().any(|p| p == d) => "match",
        (Some(_), false) => "mismatch",
        (None, false) => "unknown",
    };
    BinaryStatus {
        source: source.into(),
        found: digest.is_some(),
        sha256: digest,
        pins_listed: pins.len() as u32,
        pin_state: pin_state.into(),
    }
}

/// Read-only: where the CLI resolves from, its SHA-256 and whether it matches the pin file.
/// Hashes one local file; runs nothing.
#[tauri::command]
#[specta::specta]
pub async fn noop_binary_status() -> BinaryStatus {
    tauri::async_runtime::spawn_blocking(|| {
        binary_status_for(
            &load_settings(),
            current_exe_dir().as_deref(),
            dirs::home_dir().as_deref(),
            &active_pins(),
        )
    })
    .await
    .unwrap_or(BinaryStatus {
        source: "none".into(),
        found: false,
        sha256: None,
        pins_listed: 0,
        pin_state: "unknown".into(),
    })
}

/// Fill unset paths with defaults: `db_path` from the same resolver Home uses
/// (`noop::resolve_store_path`) so Mind-Body and Home read the same store, and `binary_path`
/// from `default_binary_path()`. An explicit Settings value always wins, and the result is
/// still validated by `run_query` (allowlist, canonicalization, ownership, sandbox).
pub fn with_resolved_db(mut s: NoopSettings) -> NoopSettings {
    if s.binary_path.as_deref().is_none_or(str::is_empty) {
        let home = dirs::home_dir();
        if let Some(p) = resolve_binary(current_exe_dir().as_deref(), home.as_deref()) {
            s.binary_path = Some(p.display().to_string());
        }
    }
    if s.db_path.as_deref().is_none_or(str::is_empty) {
        if let Ok(p) = super::noop::resolve_store_path() {
            s.db_path = Some(p.display().to_string());
        }
    }
    s
}

#[tauri::command]
#[specta::specta]
pub fn get_noop_settings() -> NoopSettings {
    load_settings()
}

/// Exact rejection string when the user cancels the native path-change dialog.
pub const PATH_CHANGE_CANCELLED: &str = "cancelled: user declined path change";

/// Asks the user to approve new canonical NOOP paths. Injected so tests can approve/deny.
pub trait ConfirmPrompter {
    fn confirm_paths(&self, binary_path: Option<&str>, db_path: Option<&str>) -> bool;
}

/// Native Confirm/Cancel dialog (tauri-plugin-dialog). Must be called off the main thread.
struct DialogPrompter(tauri::AppHandle);

impl ConfirmPrompter for DialogPrompter {
    fn confirm_paths(&self, binary_path: Option<&str>, db_path: Option<&str>) -> bool {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let msg = format!(
            "Allow Nexus to run the NOOP CLI with these paths?\n\nBinary: {}\nDatabase: {}",
            binary_path.unwrap_or("(not set)"),
            db_path.unwrap_or("(NOOP app default)")
        );
        self.0
            .dialog()
            .message(msg)
            .title("Confirm NOOP path change")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Confirm".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    }
}

/// Persist settings. If `binary_path` or `db_path` changes, the new paths are validated and a
/// native Confirm/Cancel dialog shows both canonical paths; nothing is saved unless confirmed
/// (cancel rejects with exactly [`PATH_CHANGE_CANCELLED`]). Async + spawn_blocking so the
/// blocking dialog never runs on the main thread.
#[tauri::command]
#[specta::specta]
pub async fn set_noop_settings(
    app: tauri::AppHandle,
    settings: NoopSettings,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let roots = Roots::current()?;
        let next = apply_settings(&load_settings(), settings, &DialogPrompter(app), &roots)?;
        let s = serde_json::to_string(&next).map_err(|e| e.to_string())?;
        super::kv::save_kv_internal(SETTINGS_KEY, s)
    })
    .await
    .map_err(|_| "settings task failed".to_string())?
}

/// Validate, then (only if a path changed) ask the prompter. Returns the settings to save.
pub(crate) fn apply_settings(
    current: &NoopSettings,
    mut next: NoopSettings,
    prompter: &dyn ConfirmPrompter,
    roots: &Roots,
) -> Result<NoopSettings, String> {
    let paths_changed = next.binary_path != current.binary_path || next.db_path != current.db_path;
    if let Some(b) = next.binary_path.as_deref() {
        let c = validate_path(b, PathKind::Binary, roots).map_err(|e| e.detail())?;
        next.binary_path = Some(c.display().to_string());
    }
    if let Some(d) = next.db_path.as_deref() {
        let c = validate_path(d, PathKind::Db, roots).map_err(|e| e.detail())?;
        next.db_path = Some(c.display().to_string());
    }
    if paths_changed
        && !prompter.confirm_paths(next.binary_path.as_deref(), next.db_path.as_deref())
    {
        return Err(PATH_CHANGE_CANCELLED.to_string());
    }
    Ok(next)
}

impl NoopQueryError {
    fn detail(&self) -> String {
        match serde_json::to_value(self) {
            Ok(v) => format!("{}: {}", self.kind(), v["detail"]),
            Err(_) => self.kind().to_string(),
        }
    }
}

// ── Path policy ──
//
// Both paths: absolute, canonicalized (symlinks resolved), a regular file, owned by the current
// uid, not group- or world-writable. Location allowlist, checked on the CANONICAL path:
//   binary_path: ~/projects/<repo>/Packages/NoopLocalAccess/.build/**  or  /usr/local/bin/**
//                (must also have an execute bit)
//   db_path:     ~/Library/Application Support/{OpenWhoop,NOOP}/**
//                ~/Library/Containers/<bundle>/Data/Library/Application Support/{OpenWhoop,NOOP}/**
//                  (the official sandboxed NOOP app store, see noop.rs)
//                ~/projects/<repo>/**  (dev copies)
// If db_path is unset the CLI resolves the official app container itself (read-only).
// Re-validated on every run (TOCTOU) and the canonical path is what gets spawned / passed.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Binary,
    Db,
}

pub struct Roots {
    home: PathBuf,
    usr_local_bin: PathBuf,
    /// The folder holding the main executable (`Contents/MacOS/`); the bundled CLI lives there.
    bundle_dir: Option<PathBuf>,
}

impl Roots {
    pub fn current() -> Result<Self, String> {
        let home = dirs::home_dir().ok_or("no home directory")?;
        let mut r = Self::with_home(&home);
        r.bundle_dir = current_exe_dir();
        Ok(r)
    }
    pub(crate) fn with_home(home: &Path) -> Self {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        Roots {
            home: canon(home),
            usr_local_bin: canon(Path::new("/usr/local/bin")),
            bundle_dir: None,
        }
    }
}

/// If `path` is under base/<one component>/tail.../<more>, return base/<component>/tail.
fn glob_root(path: &Path, base: &Path, tail: &[&str]) -> Option<PathBuf> {
    let rest = path.strip_prefix(base).ok()?;
    let comps: Vec<_> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let ok =
        comps.len() > tail.len() + 1 && comps[1..=tail.len()].iter().zip(tail).all(|(a, b)| a == b);
    ok.then(|| {
        comps[..=tail.len()]
            .iter()
            .fold(base.to_path_buf(), |p, c| p.join(c))
    })
}

fn prefix_root(path: &Path, base: &Path) -> Option<PathBuf> {
    (path.starts_with(base) && path != base).then(|| base.to_path_buf())
}

/// The allowlist root that `p` falls under, if any.
fn allowlist_root(p: &Path, kind: PathKind, r: &Roots) -> Option<PathBuf> {
    let projects = r.home.join("projects");
    match kind {
        PathKind::Binary => glob_root(p, &projects, &["Packages", "NoopLocalAccess", ".build"])
            .or_else(|| prefix_root(p, &r.usr_local_bin))
            .or_else(|| {
                r.bundle_dir
                    .as_deref()
                    .filter(|b| p == b.join(BUNDLED_BINARY_NAME))
                    .map(Path::to_path_buf)
            }),
        PathKind::Db => {
            let app_support = r.home.join("Library/Application Support");
            let containers = r.home.join("Library/Containers");
            ["OpenWhoop", "NOOP"]
                .iter()
                .find_map(|d| {
                    prefix_root(p, &app_support.join(d)).or_else(|| {
                        glob_root(
                            p,
                            &containers,
                            &["Data", "Library", "Application Support", d],
                        )
                    })
                })
                .or_else(|| glob_root(p, &projects, &[]))
        }
    }
}

/// Every folder from the file's parent up to and including `root` must be owned by the
/// current uid or root and not group- or world-writable.
fn parents_safe(file: &Path, root: &Path, uid: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut dir = file.parent();
    while let Some(d) = dir {
        let Ok(m) = std::fs::metadata(d) else {
            return false;
        };
        if !m.is_dir() || (m.uid() != uid && m.uid() != 0) || m.permissions().mode() & 0o022 != 0 {
            return false;
        }
        if d == root {
            return true;
        }
        dir = d.parent();
    }
    false
}

pub fn validate_path(raw: &str, kind: PathKind, r: &Roots) -> Result<PathBuf, NoopQueryError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let bad = |m: &str| Err(NoopQueryError::InvalidPath(m.to_string()));
    if raw.is_empty() || !Path::new(raw).is_absolute() {
        return bad("path must be absolute");
    }
    let canon = match Path::new(raw).canonicalize() {
        Ok(c) => c,
        Err(_) if kind == PathKind::Binary => return Err(binary_missing()),
        Err(_) => return bad("path does not exist"),
    };
    let meta = std::fs::metadata(&canon)
        .map_err(|_| NoopQueryError::InvalidPath("cannot stat path".into()))?;
    if !meta.is_file() {
        return bad("path is not a regular file");
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if meta.uid() != uid {
        return bad("file is not owned by the current user");
    }
    let mode = meta.permissions().mode();
    if mode & 0o022 != 0 {
        return bad("file is group- or world-writable");
    }
    if kind == PathKind::Binary && mode & 0o111 == 0 {
        return bad("binary is not executable");
    }
    let Some(root) = allowlist_root(&canon, kind, r) else {
        return bad(match kind {
            PathKind::Binary => "binary must be the bundled sidecar, under ~/projects/*/Packages/NoopLocalAccess/.build/, or under /usr/local/bin/",
            PathKind::Db => "database must be under ~/Library/Application Support/{OpenWhoop,NOOP}/, the NOOP app container, or ~/projects/*/",
        });
    };
    if !parents_safe(&canon, &root, uid) {
        return bad("a parent folder is group- or world-writable or not owned by you or root");
    }
    Ok(canon)
}

fn binary_missing() -> NoopQueryError {
    NoopQueryError::BinaryMissing(
        "NOOP CLI not found; build Packages/NoopLocalAccess with `swift build -c release` and set its path in Settings".into(),
    )
}

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// Network denied for the CLI. Verified 2026-11-04: real binary answers initialize/tools/list
/// under this profile; curl/nc under it fail (DNS + TCP blocked). sandbox-exec is deprecated
/// but present on current macOS; if it is missing we refuse to run rather than run unsandboxed.
pub const SANDBOX_PROFILE: &str = "(version 1)(allow default)(deny network*)";

fn int_arg(a: &Map<String, Value>, k: &str, min: i64, max: i64) -> Result<(), NoopQueryError> {
    match a.get(k) {
        None => Ok(()),
        Some(v) => match v.as_i64() {
            Some(n) if (min..=max).contains(&n) => Ok(()),
            _ => Err(NoopQueryError::InvalidArgs(format!(
                "{k} must be an integer {min}..={max}"
            ))),
        },
    }
}

fn is_day(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() && s.len() == 10
}

fn is_source_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validate tool + args against the allowlist. Unknown keys are rejected.
pub fn validate(tool: &str, args: &Value) -> Result<Map<String, Value>, NoopQueryError> {
    if !ALLOWED_TOOLS.contains(&tool) {
        return Err(NoopQueryError::ToolNotAllowed(
            tool.chars().take(64).collect(),
        ));
    }
    let a = match args {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(NoopQueryError::InvalidArgs("args must be an object".into())),
    };
    let allowed: &[&str] = match tool {
        "health_snapshot" | "sleep_summary" | "workout_summary" => &["days"],
        "metric_series" => &["key", "source", "days", "from_day", "to_day", "limit"],
        _ => &[],
    };
    if let Some(k) = a.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(NoopQueryError::InvalidArgs(format!(
            "unexpected arg: {}",
            k.chars().take(32).collect::<String>()
        )));
    }
    let max_days = if tool == "health_snapshot" { 120 } else { 4000 };
    int_arg(&a, "days", 1, max_days)?;
    if tool == "metric_series" {
        match a.get("key").and_then(Value::as_str) {
            Some(k) if METRIC_KEYS.contains(&k) => {}
            _ => {
                return Err(NoopQueryError::InvalidArgs(
                    "key must be a known metric key".into(),
                ))
            }
        }
        if let Some(v) = a.get("source") {
            if !v.as_str().is_some_and(is_source_id) {
                return Err(NoopQueryError::InvalidArgs(
                    "source must match [A-Za-z0-9_-]{1,64}".into(),
                ));
            }
        }
        for k in ["from_day", "to_day"] {
            if let Some(v) = a.get(k) {
                if !v.as_str().is_some_and(is_day) {
                    return Err(NoopQueryError::InvalidArgs(format!(
                        "{k} must be YYYY-MM-DD"
                    )));
                }
            }
        }
        int_arg(&a, "limit", 1, 2000)?;
    }
    Ok(a)
}

/// Run one MCP tools/call with a hard timeout (child killed on expiry). Paths are re-validated
/// here on every run and the canonical binary is spawned inside sandbox-exec (no network).
pub fn run_query(
    settings: &NoopSettings,
    roots: &Roots,
    tool: &str,
    args: &Value,
    timeout: Duration,
) -> Result<Value, NoopQueryError> {
    run_query_in(
        Path::new(SANDBOX_EXEC),
        settings,
        roots,
        tool,
        args,
        timeout,
    )
}

/// [`run_query`] with the sandbox launcher given, so tests can run on a host without
/// `sandbox-exec`. The launcher gets the same `-p PROFILE binary mcp` arguments; production
/// always passes [`SANDBOX_EXEC`].
pub(crate) fn run_query_in(
    sandbox_exec: &Path,
    settings: &NoopSettings,
    roots: &Roots,
    tool: &str,
    args: &Value,
    timeout: Duration,
) -> Result<Value, NoopQueryError> {
    let args = validate(tool, args)?;
    let raw_bin = settings
        .binary_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .ok_or_else(binary_missing)?;
    let binary = validate_path(raw_bin, PathKind::Binary, roots)?;
    // Bundled binary, or any release build, must be pinned. Dev builds may run unpinned.
    let bundled = roots
        .bundle_dir
        .as_deref()
        .is_some_and(|b| binary == b.join(BUNDLED_BINARY_NAME));
    let private =
        PrivateCopy::prepare(&binary, &active_pins(), bundled || !cfg!(debug_assertions))?;
    let db = match settings.db_path.as_deref().filter(|p| !p.is_empty()) {
        Some(d) => Some(validate_path(d, PathKind::Db, roots)?),
        None => None,
    };
    if !sandbox_exec.is_file() {
        return Err(NoopQueryError::SandboxMissing(format!(
            "{} not found; refusing to run the NOOP CLI without the network sandbox",
            sandbox_exec.display()
        )));
    }
    let mut cmd = Command::new(sandbox_exec);
    cmd.arg("-p")
        .arg(SANDBOX_PROFILE)
        .arg(private.path())
        .arg("mcp")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    if let Some(db) = &db {
        cmd.env("NOOP_DB_PATH", db);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| NoopQueryError::SpawnFailed(e.kind().to_string()))?;
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"nexus","version":"1"}}});
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":Value::Object(args)}});
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let _ = writeln!(stdin, "{init}\n{call}");
    } // closing stdin ends the server's read loop
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take(16 * 1024 * 1024).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(NoopQueryError::Timeout(timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(NoopQueryError::ProcessFailed(e.kind().to_string())),
        }
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(NoopQueryError::ProcessFailed(format!(
            "exit {:?}",
            status.code()
        )));
    }
    parse_response(&out)
}

/// Find the id=2 response; return parsed structured content (or JSON text content).
pub fn parse_response(out: &[u8]) -> Result<Value, NoopQueryError> {
    let text = std::str::from_utf8(out)
        .map_err(|_| NoopQueryError::BadOutput("non-UTF-8 stdout".into()))?;
    let resp = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("id") == Some(&json!(2)))
        .ok_or_else(|| NoopQueryError::BadOutput("no tools/call response".into()))?;
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return Err(NoopQueryError::ToolError(msg.chars().take(200).collect()));
    }
    let result = resp
        .get("result")
        .ok_or_else(|| NoopQueryError::BadOutput("missing result".into()))?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(NoopQueryError::ToolError("tool reported an error".into()));
    }
    if let Some(sc) = result.get("structuredContent") {
        return Ok(sc.clone());
    }
    let t = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| NoopQueryError::BadOutput("no content".into()))?;
    serde_json::from_str(t).map_err(|_| NoopQueryError::BadOutput("content is not JSON".into()))
}

#[tauri::command]
#[specta::specta]
pub async fn noop_query(tool: String, args: Option<Value>) -> Result<Value, NoopQueryError> {
    let s = with_resolved_db(load_settings());
    let roots = Roots::current().map_err(NoopQueryError::InvalidPath)?;
    let args = args.unwrap_or(Value::Null);
    tauri::async_runtime::spawn_blocking(move || {
        run_query(&s, &roots, &tool, &args, DEFAULT_TIMEOUT)
    })
    .await
    .map_err(|_| NoopQueryError::ProcessFailed("join".into()))?
}

/// Test helper: a fake `home` with an allowlisted build dir for fake CLI scripts.
#[cfg(test)]
pub(crate) mod test_util {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    pub struct FakeHome(pub PathBuf);
    impl FakeHome {
        pub fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "noopq-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&p).unwrap();
            FakeHome(p.canonicalize().unwrap())
        }
        pub fn build_dir(&self) -> PathBuf {
            let d = self
                .0
                .join("projects/noop-main/Packages/NoopLocalAccess/.build/release");
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        pub fn file(&self, rel: &str, body: &str, mode: u32) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            p
        }
        pub fn script(&self, body: &str) -> PathBuf {
            self.build_dir();
            self.file(
                "projects/noop-main/Packages/NoopLocalAccess/.build/release/fake-noop",
                &format!("#!/bin/sh\n{body}\n"),
                0o755,
            )
        }
        /// Stands in for `sandbox-exec` where it does not exist (Linux). It insists on the real
        /// `-p <SANDBOX_PROFILE> <binary> mcp` arguments, then runs the binary.
        pub fn sandbox(&self) -> PathBuf {
            self.file(
                "bin/fake-sandbox-exec",
                &format!(
                    "#!/bin/sh\n[ \"$1\" = \"-p\" ] && [ \"$2\" = '{}' ] && [ \"$4\" = mcp ] || exit 97\nshift 2\nexec \"$@\"\n",
                    super::SANDBOX_PROFILE
                ),
                0o755,
            )
        }
        pub fn roots(&self) -> super::Roots {
            super::Roots::with_home(&self.0)
        }
    }
    impl Drop for FakeHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub fn s(p: &Path) -> Option<String> {
        Some(p.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_tool_and_args() {
        assert!(matches!(
            validate("rm", &Value::Null),
            Err(NoopQueryError::ToolNotAllowed(_))
        ));
        assert!(validate("data_freshness", &json!({"days": 1})).is_err());
        assert!(validate("health_snapshot", &json!({"days": 121})).is_err());
        assert!(validate("health_snapshot", &json!({"days": "5"})).is_err());
        assert!(validate("health_snapshot", &json!([1])).is_err());
        assert!(validate("metric_series", &json!({})).is_err());
        assert!(validate("metric_series", &json!({"key": "hrv; rm -rf"})).is_err());
        assert!(validate("metric_series", &json!({"key": "hrv", "source": "../x"})).is_err());
        assert!(validate(
            "metric_series",
            &json!({"key": "hrv", "from_day": "2026-13-01"})
        )
        .is_err());
        assert!(validate("metric_series", &json!({"key": "hrv", "limit": 0})).is_err());
    }

    #[test]
    fn accepts_valid_args() {
        for t in ALLOWED_TOOLS.iter().filter(|t| **t != "metric_series") {
            assert!(validate(t, &Value::Null).is_ok());
        }
        assert!(validate("health_snapshot", &json!({"days": 14})).is_ok());
        assert!(validate("metric_series", &json!({"key": "recovery", "source": "my-whoop", "from_day": "2026-10-01", "to_day": "2026-10-31", "limit": 100})).is_ok());
    }

    use super::test_util::{s as sp, FakeHome};

    fn with_bin(p: &Path) -> NoopSettings {
        NoopSettings {
            binary_path: sp(p),
            ..Default::default()
        }
    }

    #[test]
    fn missing_binary_is_structured() {
        let h = FakeHome::new();
        let r = run_query(
            &NoopSettings::default(),
            &h.roots(),
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        );
        assert!(matches!(r, Err(NoopQueryError::BinaryMissing(_))));
        let gone = h.build_dir().join("nope");
        let r = run_query(
            &with_bin(&gone),
            &h.roots(),
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        );
        assert!(matches!(r, Err(NoopQueryError::BinaryMissing(_))));
        let r = run_query(
            &with_bin(Path::new("relative")),
            &h.roots(),
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        );
        assert!(matches!(r, Err(NoopQueryError::InvalidPath(_))));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_fake_server_and_times_out_in_sandbox() {
        let h = FakeHome::new();
        let p = h.script(r#"cat >/dev/null; echo '{"jsonrpc":"2.0","id":1,"result":{}}'; echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\":true,\"n\":3}"}]}}'"#);
        let v = run_query(
            &with_bin(&p),
            &h.roots(),
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        )
        .unwrap();
        assert_eq!(v["n"], 3);
        let h2 = FakeHome::new();
        let p2 = h2.script("sleep 30");
        let t = Instant::now();
        let r = run_query(
            &with_bin(&p2),
            &h2.roots(),
            "data_freshness",
            &Value::Null,
            Duration::from_millis(300),
        );
        assert!(matches!(r, Err(NoopQueryError::Timeout(_))));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_denies_network() {
        // The fake CLI tries TCP; under the profile it must fail and report that.
        let h = FakeHome::new();
        let p = h.script(r#"cat >/dev/null; if /usr/bin/nc -z -w2 1.1.1.1 443 2>/dev/null; then r=open; else r=denied; fi; echo "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"net\":\"$r\"}}}""#);
        let v = run_query(
            &with_bin(&p),
            &h.roots(),
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        )
        .unwrap();
        assert_eq!(v["net"], "denied");
    }

    #[test]
    fn path_policy() {
        use std::os::unix::fs::symlink;
        let h = FakeHome::new();
        let r = h.roots();
        let ok = h.script("true");
        assert_eq!(
            validate_path(ok.to_str().unwrap(), PathKind::Binary, &r).unwrap(),
            ok
        );
        // symlink inside the allowlist pointing outside it
        let outside = h.file("elsewhere/evil", "#!/bin/sh\n", 0o755);
        let link = h.build_dir().join("link");
        symlink(&outside, &link).unwrap();
        assert!(matches!(
            validate_path(link.to_str().unwrap(), PathKind::Binary, &r),
            Err(NoopQueryError::InvalidPath(_))
        ));
        // outside allowlist
        assert!(validate_path(outside.to_str().unwrap(), PathKind::Binary, &r).is_err());
        // group- / world-writable
        let gw = h.file(
            "projects/noop-main/Packages/NoopLocalAccess/.build/gw",
            "x",
            0o775,
        );
        let ww = h.file(
            "projects/noop-main/Packages/NoopLocalAccess/.build/ww",
            "x",
            0o757,
        );
        assert!(validate_path(gw.to_str().unwrap(), PathKind::Binary, &r).is_err());
        assert!(validate_path(ww.to_str().unwrap(), PathKind::Binary, &r).is_err());
        // directory, not a file
        assert!(validate_path(h.build_dir().to_str().unwrap(), PathKind::Binary, &r).is_err());
        // not executable
        let nx = h.file(
            "projects/noop-main/Packages/NoopLocalAccess/.build/nx",
            "x",
            0o644,
        );
        assert!(validate_path(nx.to_str().unwrap(), PathKind::Binary, &r).is_err());
        // wrong owner: a root-owned file outside our control (skip if not root-owned)
        if std::fs::metadata("/bin/sh")
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
            .unwrap_or(false)
        {
            assert!(validate_path("/bin/sh", PathKind::Binary, &r).is_err());
        }
        // db allowlist
        let db1 = h.file(
            "Library/Application Support/OpenWhoop/whoop.sqlite",
            "",
            0o600,
        );
        let db2 = h.file(
            "Library/Containers/com.x/Data/Library/Application Support/NOOP/a.sqlite",
            "",
            0o600,
        );
        let db3 = h.file("projects/noop-main/test.sqlite", "", 0o600);
        let db_bad = h.file("Library/Application Support/Other/x.sqlite", "", 0o600);
        let db_ww = h.file("Library/Application Support/OpenWhoop/ww.sqlite", "", 0o606);
        for d in [&db1, &db2, &db3] {
            assert!(
                validate_path(d.to_str().unwrap(), PathKind::Db, &r).is_ok(),
                "{d:?}"
            );
        }
        assert!(validate_path(db_bad.to_str().unwrap(), PathKind::Db, &r).is_err());
        assert!(validate_path(db_ww.to_str().unwrap(), PathKind::Db, &r).is_err());
        assert!(validate_path(h.0.join("projects").to_str().unwrap(), PathKind::Db, &r).is_err());
    }

    struct Fixed(bool, std::cell::Cell<u32>);
    impl ConfirmPrompter for Fixed {
        fn confirm_paths(&self, _: Option<&str>, _: Option<&str>) -> bool {
            self.1.set(self.1.get() + 1);
            self.0
        }
    }

    #[test]
    fn settings_path_change_needs_dialog_approval() {
        let h = FakeHome::new();
        let r = h.roots();
        let ok = h.script("true");
        let cur = NoopSettings::default();
        let next = with_bin(&ok);
        let deny = Fixed(false, Default::default());
        assert_eq!(
            apply_settings(&cur, next.clone(), &deny, &r),
            Err(PATH_CHANGE_CANCELLED.to_string())
        );
        assert_eq!(deny.1.get(), 1);
        let approve = Fixed(true, Default::default());
        let saved = apply_settings(&cur, next.clone(), &approve, &r).unwrap();
        assert_eq!(saved.binary_path, sp(&ok));
        // source-only change skips the dialog
        let mut src = saved.clone();
        src.whoop_source = WhoopSource::Ble;
        let never = Fixed(false, Default::default());
        assert!(apply_settings(&saved, src, &never, &r).is_ok());
        assert_eq!(never.1.get(), 0);
        // invalid path rejected before any dialog
        let bad = with_bin(Path::new("/etc/hosts"));
        let a2 = Fixed(true, Default::default());
        assert!(apply_settings(&cur, bad, &a2, &r)
            .unwrap_err()
            .starts_with("invalid_path"));
        assert_eq!(a2.1.get(), 0);
    }

    #[test]
    fn writable_parent_folder_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [0o777, 0o775] {
            let h = FakeHome::new();
            let p = h.script("true");
            let r = h.roots();
            assert!(validate_path(p.to_str().unwrap(), PathKind::Binary, &r).is_ok());
            let dir = h.build_dir();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(
                validate_path(p.to_str().unwrap(), PathKind::Binary, &r).is_err(),
                "{mode:o}"
            );
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            // the allowlist root itself (.build) counts too
            let root = dir.parent().unwrap();
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(
                validate_path(p.to_str().unwrap(), PathKind::Binary, &r).is_err(),
                "root {mode:o}"
            );
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn parse_errors_are_structured() {
        assert!(matches!(
            parse_response(b"garbage"),
            Err(NoopQueryError::BadOutput(_))
        ));
        let e = br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"bad"}}"#;
        assert!(matches!(
            parse_response(e),
            Err(NoopQueryError::ToolError(_))
        ));
        let sc = br#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"a":1}}}"#;
        assert_eq!(parse_response(sc).unwrap()["a"], 1);
    }

    #[test]
    fn default_binary_is_under_home_and_explicit_wins() {
        let home = Path::new("/h");
        assert_eq!(
            super::default_binary_under(home),
            home.join("projects/noop-lucid-current/Packages/NoopLocalAccess/.build/arm64-apple-macosx/release/noop-local-access")
        );
        let filled = with_resolved_db(NoopSettings::default());
        assert_eq!(
            filled.binary_path.map(PathBuf::from),
            super::default_binary_path()
        );
        let explicit = with_resolved_db(NoopSettings {
            binary_path: Some("/usr/local/bin/x".into()),
            ..Default::default()
        });
        assert_eq!(explicit.binary_path.as_deref(), Some("/usr/local/bin/x"));
    }

    /// Live check against the real default binary + Home's store. Prints key names only.
    #[test]
    #[ignore]
    fn live_default_data_freshness() {
        let s = with_resolved_db(load_settings_for_live());
        let r = Roots::current().unwrap();
        for tool in ["data_freshness", "health_snapshot"] {
            let v = run_query(&s, &r, tool, &json!({}), Duration::from_secs(10)).unwrap();
            let keys: Vec<_> = v
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            println!("{tool}: object={} keys={keys:?}", v.is_object());
            // Timestamps only (…At, date, timestamp, iso, latest…Day keys); never metric values.
            fn ts(v: &Value, path: String, out: &mut Vec<String>) {
                if let Some(o) = v.as_object() {
                    for (k, c) in o {
                        let p = format!("{path}.{k}");
                        let lk = k.to_lowercase();
                        if c.is_string()
                            && (lk.ends_with("at")
                                || lk.contains("date")
                                || lk.contains("timestamp")
                                || lk == "iso"
                                || lk.starts_with("latest") && lk.ends_with("day"))
                        {
                            out.push(format!("{p}={}", c.as_str().unwrap_or("")));
                        } else {
                            ts(c, p, out);
                        }
                    }
                }
            }
            let mut out = Vec::new();
            ts(&v, String::new(), &mut out);
            out.truncate(12);
            println!("{tool} timestamps: {out:?}");
        }
    }

    fn load_settings_for_live() -> NoopSettings {
        NoopSettings::default()
    }

    fn sha_of(p: &Path) -> String {
        sha256_file(p).unwrap()
    }

    fn with_pins<R>(pins: &[&str], f: impl FnOnce() -> R) -> R {
        TEST_PINS.with(|t| *t.borrow_mut() = Some(pins.iter().map(|s| s.to_string()).collect()));
        let r = f();
        TEST_PINS.with(|t| *t.borrow_mut() = None);
        r
    }

    #[test]
    fn pin_file_parses_comments_blanks_and_rejects_typos() {
        let good = "a".repeat(64);
        let upper = "AB".repeat(32);
        let raw = format!(
            "# note\n\n{good}  # trailing\n{upper}\nnot-hex\n{}\n",
            "c".repeat(63)
        );
        let pins = parse_pins(&raw);
        assert_eq!(pins, vec![good, upper.to_ascii_lowercase()]);
        // The shipped file only holds comments or valid digests, so a typo cannot hide in it.
        for l in PINS_RAW.lines() {
            let t = l.trim();
            assert!(
                t.is_empty() || t.starts_with('#') || parse_pins(t).len() == 1,
                "malformed line in noop-local-access.sha256"
            );
        }
    }

    #[test]
    fn pin_decision_unset_match_mismatch_and_missing_in_release() {
        let d = "a".repeat(64);
        let other = "b".repeat(64);
        // Dev: an empty list runs. Release or bundled (require_pin): it refuses with pin_missing.
        assert!(pin_decision(&d, &[], false).is_ok());
        let miss = pin_decision(&d, &[], true).unwrap_err();
        assert_eq!(miss.kind(), "pin_missing");
        assert!(serde_json::to_string(&miss).unwrap().contains(PIN_MISSING));
        // A listed digest decides regardless of mode.
        for req in [false, true] {
            assert!(pin_decision(&d, &[other.clone(), d.clone()], req).is_ok());
            let e = pin_decision(&d, std::slice::from_ref(&other), req).unwrap_err();
            assert_eq!(e.kind(), "pin_mismatch");
            let t = serde_json::to_string(&e).unwrap();
            assert!(t.contains(PIN_MISMATCH) && !t.contains(&d) && !t.contains(&other));
        }
        // The shipped pin file lists the binary used today, so a release build is not "missing".
        assert!(!parse_pins(PINS_RAW).is_empty());
    }

    #[test]
    fn private_copy_is_hashed_locked_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let h = FakeHome::new();
        let p = h.script("echo hi");
        let d = sha_of(&p);
        let (dir, copy_path);
        {
            let c = PrivateCopy::prepare(&p, std::slice::from_ref(&d), true).unwrap();
            copy_path = c.path().to_path_buf();
            dir = copy_path.parent().unwrap().to_path_buf();
            assert_eq!(
                std::fs::read(&copy_path).unwrap(),
                std::fs::read(&p).unwrap()
            );
            assert_eq!(
                std::fs::metadata(&copy_path).unwrap().permissions().mode() & 0o777,
                0o500
            );
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            // Changing the original afterwards does not change what would run.
            std::fs::write(&p, "#!/bin/sh\necho swapped\n").unwrap();
            assert_eq!(sha_of(&copy_path), d);
        }
        assert!(
            !copy_path.exists() && !dir.exists(),
            "copy and folder are removed"
        );
        // A refused pin also leaves nothing behind.
        let before: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("nexus-noop-"))
            .map(|e| e.path())
            .collect();
        let _ = PrivateCopy::prepare(&p, &["0".repeat(64)], true)
            .err()
            .unwrap();
        let after = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("nexus-noop-"))
            .filter(|e| !before.contains(&e.path()))
            .count();
        assert_eq!(after, 0, "refusal leaves no private folder behind");
    }

    #[test]
    fn a_swap_after_validation_is_not_executed() {
        let h = FakeHome::new();
        let marker = h.0.join("ran");
        let p = h.script("exit 0");
        let roots = h.roots();
        let canon = validate_path(&p.display().to_string(), PathKind::Binary, &roots).unwrap();
        let pin = sha_of(&canon);
        // Swap the file for one that would touch the marker, after validation and the pin read.
        std::fs::write(&p, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        let r = PrivateCopy::prepare(&canon, &[pin], true);
        assert!(matches!(r, Err(NoopQueryError::PinMismatch(_))));
        assert!(!marker.exists());
        // A symlink swapped in for the file is refused by O_NOFOLLOW before any read.
        let target = h.file("elsewhere/real", "#!/bin/sh\ntrue\n", 0o755);
        std::fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&target, &p).unwrap();
        let r = PrivateCopy::prepare(&p, &[], false);
        assert_eq!(r.err().unwrap().kind(), "invalid_path");
    }

    #[test]
    fn bundled_binary_without_pins_is_refused_and_a_run_uses_the_copy() {
        let h = FakeHome::new();
        let b = h.file(
            "App.app/Contents/MacOS/noop-local-access",
            "#!/bin/sh\ncat >/dev/null; echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'; echo '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"n\\\":1}\"}]}}'\n",
            0o755,
        );
        let mut roots = h.roots();
        roots.bundle_dir = Some(b.parent().unwrap().canonicalize().unwrap());
        let sandbox = h.sandbox();
        let run = |pins: &[&str]| {
            with_pins(pins, || {
                run_query_in(
                    &sandbox,
                    &with_bin(&b),
                    &roots,
                    "data_freshness",
                    &Value::Null,
                    DEFAULT_TIMEOUT,
                )
            })
        };
        assert_eq!(run(&[]).unwrap_err().kind(), "pin_missing");
        let pin = sha_of(&b);
        assert_eq!(run(&[pin.as_str()]).unwrap()["n"], 1);
    }

    #[test]
    fn bundle_root_needs_a_dot_app_and_matches_only_the_exact_file() {
        let app = Path::new("/Applications/Amara OS.app/Contents/MacOS/nexus");
        assert_eq!(
            bundle_dir_for_exe(app),
            Some(PathBuf::from("/Applications/Amara OS.app/Contents/MacOS"))
        );
        assert_eq!(bundle_dir_for_exe(Path::new("/x/target/debug/nexus")), None);
        assert_eq!(bundle_dir_for_exe(Path::new("/x/App.app")), None);
    }

    #[test]
    fn a_pin_mismatch_refuses_before_anything_is_spawned() {
        let h = FakeHome::new();
        let marker = h.0.join("ran");
        let p = h.script(&format!("touch '{}'", marker.display()));
        let sandbox = h.sandbox();
        let r = with_pins(&["0".repeat(64).as_str()], || {
            run_query_in(
                &sandbox,
                &with_bin(&p),
                &h.roots(),
                "data_freshness",
                &Value::Null,
                DEFAULT_TIMEOUT,
            )
        });
        assert!(matches!(r, Err(NoopQueryError::PinMismatch(_))));
        assert!(!marker.exists(), "the binary must not run on a mismatch");
    }

    #[test]
    fn a_matching_pin_runs_and_a_changed_binary_stops() {
        let h = FakeHome::new();
        let body = r#"cat >/dev/null; echo '{"jsonrpc":"2.0","id":1,"result":{}}'; echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"n\":7}"}]}}'"#;
        let p = h.script(body);
        let pin = sha_of(&p);
        let sandbox = h.sandbox();
        let run = |pin: &str| {
            with_pins(&[pin], || {
                run_query_in(
                    &sandbox,
                    &with_bin(&p),
                    &h.roots(),
                    "data_freshness",
                    &Value::Null,
                    DEFAULT_TIMEOUT,
                )
            })
        };
        assert_eq!(run(&pin).unwrap()["n"], 7);
        // Same path, different bytes: the old pin no longer matches.
        h.script(&format!("{body}\n# tampered"));
        assert!(matches!(run(&pin), Err(NoopQueryError::PinMismatch(_))));
    }

    #[test]
    fn bundled_sidecar_wins_then_projects_fallback() {
        let h = FakeHome::new();
        let exe = h.0.join("App.app/Contents/MacOS");
        std::fs::create_dir_all(&exe).unwrap();
        let home = h.0.clone();
        assert_eq!(
            resolve_binary(Some(&exe), Some(&home)),
            Some(default_binary_under(&home)),
            "no bundled file: projects fallback"
        );
        let b = h.file(
            "App.app/Contents/MacOS/noop-local-access",
            "#!/bin/sh\n",
            0o755,
        );
        assert_eq!(resolve_binary(Some(&exe), Some(&home)), Some(b.clone()));
        assert_eq!(
            resolve_binary(None, Some(&home)),
            Some(default_binary_under(&home))
        );
        assert_eq!(resolve_binary(None, None), None);
        // An explicit Settings value is untouched by resolution.
        let filled = with_resolved_db(NoopSettings {
            binary_path: Some("/usr/local/bin/x".into()),
            ..Default::default()
        });
        assert_eq!(filled.binary_path.as_deref(), Some("/usr/local/bin/x"));
    }

    #[test]
    fn bundle_folder_is_an_allowlisted_binary_root_but_only_for_that_folder() {
        let h = FakeHome::new();
        let exe = h.0.join("App.app/Contents/MacOS");
        let b = h.file(
            "App.app/Contents/MacOS/noop-local-access",
            "#!/bin/sh\n",
            0o755,
        );
        let other = h.file("elsewhere/noop-local-access", "#!/bin/sh\n", 0o755);
        let mut roots = h.roots();
        let raw = b.display().to_string();
        assert!(
            validate_path(&raw, PathKind::Binary, &roots).is_err(),
            "not allowed without a bundle root"
        );
        roots.bundle_dir = Some(exe.canonicalize().unwrap());
        assert!(validate_path(&raw, PathKind::Binary, &roots).is_ok());
        assert!(validate_path(&other.display().to_string(), PathKind::Binary, &roots).is_err());
        // Another file in the same folder is not the sidecar.
        let sibling = h.file("App.app/Contents/MacOS/other-tool", "#!/bin/sh\n", 0o755);
        assert!(validate_path(&sibling.display().to_string(), PathKind::Binary, &roots).is_err());
    }

    #[test]
    fn binary_status_reports_source_digest_and_pin_state_only() {
        let h = FakeHome::new();
        let exe = h.0.join("App.app/Contents/MacOS");
        let home = h.0.clone();
        let none = binary_status_for(&NoopSettings::default(), Some(&exe), Some(&home), &[]);
        assert_eq!(
            (none.source.as_str(), none.found, none.pin_state.as_str()),
            ("none", false, "unpinned")
        );
        let p = h.script("echo hi");
        // Put the script where the ~/projects default expects it.
        let dst = default_binary_under(&home);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(&p, &dst).unwrap();
        let d = sha_of(&dst);
        let st = binary_status_for(&NoopSettings::default(), Some(&exe), Some(&home), &[]);
        assert_eq!(
            (
                st.source.as_str(),
                st.sha256.as_deref(),
                st.pin_state.as_str()
            ),
            ("projects", Some(d.as_str()), "unpinned")
        );
        let m = binary_status_for(
            &NoopSettings::default(),
            Some(&exe),
            Some(&home),
            std::slice::from_ref(&d),
        );
        assert_eq!(m.pin_state, "match");
        let x = binary_status_for(
            &NoopSettings::default(),
            Some(&exe),
            Some(&home),
            &["0".repeat(64)],
        );
        assert_eq!((x.pin_state.as_str(), x.pins_listed), ("mismatch", 1));
        let b = h.file(
            "App.app/Contents/MacOS/noop-local-access",
            "#!/bin/sh\n:\n",
            0o755,
        );
        let bs = binary_status_for(&NoopSettings::default(), Some(&exe), Some(&home), &[]);
        assert_eq!(
            (bs.source.as_str(), bs.sha256),
            ("bundled", Some(sha_of(&b)))
        );
    }

    #[test]
    fn whoop_source_defaults_to_noop_and_blocks_ble() {
        let s: NoopSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.whoop_source, WhoopSource::Noop);
        assert!(!s.whoop_ble_allowed());
        let b: NoopSettings = serde_json::from_str(r#"{"whoop_source":"ble"}"#).unwrap();
        assert!(b.whoop_ble_allowed());
    }
}
