//! NOOP freeze tool (Stage G, Phase 10 P0 + P3 behind a switch).
//!
//! * `noop_freeze_report` (read-only): for the NOOP store, the files' sizes and SHA-256, the schema
//!   version (GRDB migration count and last migration id, plus `user_version`), per-table row counts
//!   and newest `ts`, whether NOOP.app is running, and the NAMES of NOOP's Keychain items (whether
//!   each exists; never a value, never a read of the secret). No biometric value is read or returned.
//! * `noop_freeze_copy` (gated): the P3 copy with the SQLite backup API to `<data>/health/whoop.sqlite`,
//!   then `integrity_check`, row counts, max timestamps and schema checks against the source, and an
//!   atomic no-overwrite publish. It refuses unless the switch is on AND NOOP.app is not running
//!   (an unreadable process list counts as running), and it needs the native approval dialog.
//!   `FREEZE_COPY_ENABLED` is `false`: the takeover is Taisun's decision.
//!
//! Neither command writes, locks, checkpoints or moves the NOOP store. The source is opened
//! READ_ONLY. Tests use temp databases and fakes only.

use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The gate for the copy. Flip only with Taisun's words logged in docs/decisions/.
pub const FREEZE_COPY_ENABLED: bool = false;
pub const COPY_DISABLED: &str = "disabled: the NOOP store copy is not enabled yet.";
const RUNNING: &str = "refused: NOOP.app is running (or could not be checked). Quit it first.";
const STORE_OPEN: &str =
    "refused: a process has the NOOP store open (or it could not be checked). Quit it first.";
const NO_SOURCE: &str = "The NOOP store was not found; nothing was copied.";
const DEST_EXISTS: &str = "refused: the destination already exists; nothing was changed.";
const DEST_UNSAFE: &str = "refused: the destination folder is not a plain folder you own.";
const COPY_FAILED: &str = "The copy failed; nothing was published.";
const VERIFY_FAILED: &str = "The copy did not verify; nothing was published.";

/// Keychain service names NOOP.app uses (found in NOOP's source). Existence only.
pub(crate) const NOOP_KEYCHAIN_SERVICES: &[&str] = &[
    "com.noop.oura",
    "com.noop.oura.installkey",
    "com.noop.aicoach",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FreezeFile {
    pub name: String,
    pub bytes: i64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FreezeTable {
    pub name: String,
    pub rows: Option<i64>,
    /// Newest `ts` when the table has one (a timestamp, never a measurement).
    pub max_ts: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KeychainName {
    pub name: String,
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FreezeReport {
    pub store_found: bool,
    pub files: Vec<FreezeFile>,
    pub user_version: Option<i64>,
    pub migration_count: Option<i64>,
    pub last_migration: Option<String>,
    pub tables: Vec<FreezeTable>,
    /// `yes` | `no` | `unknown`
    pub noop_running: String,
    pub keychain_items: Vec<KeychainName>,
    pub copy_enabled: bool,
    /// Why the report is partial (fixed text, no path), when it is.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FreezeCopyResult {
    pub ok: bool,
    pub tables: Vec<FreezeTable>,
    pub migration_count: Option<i64>,
}

// ── process and Keychain probes (injectable) ──

/// Executable paths of running processes (`ps -axo comm=`), or None when the list is unavailable.
pub(crate) fn live_process_paths() -> Option<Vec<String>> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// True when `path` is the executable of a NOOP app bundle ("NOOP.app", "NOOP Lucid Preview.app",
/// "Strand.app" ...). Amara's own binaries and the `noop-local-access` helper do not match.
pub(crate) fn is_noop_app_process(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let Some(idx) = lower.find(".app/contents/") else {
        return false;
    };
    let app = lower[..idx].rsplit('/').next().unwrap_or("");
    app.contains("noop") || app == "strand"
}

/// `yes` / `no` / `unknown` from a process list.
pub(crate) fn running_state(paths: Option<&[String]>) -> &'static str {
    match paths {
        None => "unknown",
        Some(p) if p.iter().any(|x| is_noop_app_process(x)) => "yes",
        Some(_) => "no",
    }
}

/// Does a generic-password item with this service exist? Attributes only: the secret is never
/// requested, so there is no access prompt and nothing to leak.
pub(crate) trait KeychainNames {
    fn exists(&self, service: &str) -> bool;
}

#[cfg(target_os = "macos")]
pub(crate) struct MacKeychainNames;

#[cfg(target_os = "macos")]
impl KeychainNames for MacKeychainNames {
    fn exists(&self, service: &str) -> bool {
        use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
        ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
            .limit(Limit::Max(1))
            .search()
            .is_ok_and(|r| !r.is_empty())
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct MacKeychainNames;

#[cfg(not(target_os = "macos"))]
impl KeychainNames for MacKeychainNames {
    fn exists(&self, _: &str) -> bool {
        false
    }
}

/// Pure reading of an `lsof -F p` result for the store and its `-wal`: exit 1 with no output
/// means nobody has them open; exit 0 with output means a process does. Anything else is unknown.
pub(crate) fn open_state_from(code: Option<i32>, stdout: &[u8]) -> Option<bool> {
    match code {
        Some(1) if stdout.is_empty() => Some(false),
        Some(0) if !stdout.is_empty() => Some(true),
        _ => None,
    }
}

/// Does any process have `db` or `db-wal` open? `lsof` by full path, cleared environment.
/// `None` (unreadable result) is treated as "open" by the caller.
pub(crate) fn store_open_by_any(db: &Path) -> Option<bool> {
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let mut cmd = std::process::Command::new("/usr/sbin/lsof");
    cmd.args(["-F", "p", "--"]).arg(db);
    if wal.exists() {
        cmd.arg(&wal);
    }
    let out = cmd
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    open_state_from(out.status.code(), &out.stdout)
}

// ── read-only inspection ──

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn file_entry(path: &Path, name: &str) -> Option<FreezeFile> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(FreezeFile {
        name: name.to_string(),
        bytes: i64::try_from(meta.len()).unwrap_or(i64::MAX),
        sha256: super::noop_query::sha256_file(path).ok()?,
    })
}

fn files_of(db: &Path) -> Vec<FreezeFile> {
    let base = db
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = Vec::new();
    for suffix in ["", "-wal"] {
        let p = PathBuf::from(format!("{}{suffix}", db.display()));
        if let Some(f) = file_entry(&p, &format!("{base}{suffix}")) {
            out.push(f);
        }
    }
    out
}

fn table_list(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn has_ts(conn: &Connection, table: &str) -> bool {
    conn.prepare(&format!("PRAGMA table_info({})", quote_ident(table)))
        .and_then(|mut st| {
            let names = st.query_map([], |r| r.get::<_, String>(1))?;
            Ok(names.flatten().any(|n| n == "ts"))
        })
        .unwrap_or(false)
}

fn value_text(v: rusqlite::types::Value) -> Option<String> {
    use rusqlite::types::Value;
    match v {
        Value::Null | Value::Blob(_) => None,
        Value::Integer(i) => Some(i.to_string()),
        Value::Real(f) => Some(f.to_string()),
        Value::Text(t) => Some(t),
    }
}

fn table_stats(conn: &Connection, name: &str) -> FreezeTable {
    let q = quote_ident(name);
    let rows = conn
        .query_row(&format!("SELECT COUNT(*) FROM {q}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .ok();
    let max_ts = if has_ts(conn, name) {
        conn.query_row(&format!("SELECT MAX(ts) FROM {q}"), [], |r| {
            r.get::<_, rusqlite::types::Value>(0)
        })
        .ok()
        .and_then(value_text)
    } else {
        None
    };
    FreezeTable {
        name: name.to_string(),
        rows,
        max_ts,
    }
}

fn all_stats(conn: &Connection) -> rusqlite::Result<Vec<FreezeTable>> {
    Ok(table_list(conn)?
        .iter()
        .map(|t| table_stats(conn, t))
        .collect())
}

/// (migration count, last migration id) from GRDB's bookkeeping table.
fn migrations(conn: &Connection) -> (Option<i64>, Option<String>) {
    let count = conn
        .query_row("SELECT COUNT(*) FROM grdb_migrations", [], |r| {
            r.get::<_, i64>(0)
        })
        .ok();
    let last = conn
        .query_row(
            "SELECT identifier FROM grdb_migrations ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok();
    (count, last)
}

fn user_version(conn: &Connection) -> Option<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .ok()
}

pub(crate) fn report_for(
    store: Option<&Path>,
    procs: Option<&[String]>,
    kc: &dyn KeychainNames,
    copy_enabled: bool,
) -> FreezeReport {
    let keychain_items = NOOP_KEYCHAIN_SERVICES
        .iter()
        .map(|s| KeychainName {
            name: (*s).to_string(),
            present: kc.exists(s),
        })
        .collect();
    let mut rep = FreezeReport {
        store_found: false,
        files: Vec::new(),
        user_version: None,
        migration_count: None,
        last_migration: None,
        tables: Vec::new(),
        noop_running: running_state(procs).to_string(),
        keychain_items,
        copy_enabled,
        note: None,
    };
    let Some(path) = store.filter(|p| p.is_file()) else {
        rep.note = Some("The NOOP store was not found.".into());
        return rep;
    };
    rep.store_found = true;
    rep.files = files_of(path);
    match super::noop::open_store(path) {
        Ok(conn) => {
            rep.user_version = user_version(&conn);
            let (c, l) = migrations(&conn);
            rep.migration_count = c;
            rep.last_migration = l;
            match all_stats(&conn) {
                Ok(t) => rep.tables = t,
                Err(_) => rep.note = Some("The table list could not be read.".into()),
            }
        }
        Err(_) => rep.note = Some("The store could not be opened read-only.".into()),
    }
    rep
}

#[tauri::command]
#[specta::specta]
pub async fn noop_freeze_report() -> Result<FreezeReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let store = super::noop::resolve_store_path().ok();
        report_for(
            store.as_deref(),
            live_process_paths().as_deref(),
            &MacKeychainNames,
            FREEZE_COPY_ENABLED,
        )
    })
    .await
    .map_err(|_| "The report could not run.".to_string())
}

// ── gated copy ──

/// Destination: `<data>/health/whoop.sqlite`.
pub(crate) fn dest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("health").join("whoop.sqlite")
}

fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|_| DEST_UNSAFE.to_string())?;
        }
        Err(_) => return Err(DEST_UNSAFE.to_string()),
        Ok(_) => {}
    }
    let m = std::fs::symlink_metadata(dir).map_err(|_| DEST_UNSAFE.to_string())?;
    // SAFETY: getuid has no preconditions and cannot fail.
    if !m.is_dir() || m.uid() != unsafe { libc::getuid() } {
        return Err(DEST_UNSAFE.to_string());
    }
    if m.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| DEST_UNSAFE.to_string())?;
    }
    Ok(())
}

struct TmpGuard(PathBuf);
impl Drop for TmpGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for s in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{s}", self.0.display()));
        }
    }
}

/// Backup-API copy of `src` to `dest`, verified, then published atomically without overwriting.
/// Order of refusals: switch, NOOP running, source, store open, destination. Never touches `src` beyond a
/// read-only open.
pub(crate) fn copy_core(
    src: &Path,
    dest: &Path,
    enabled: bool,
    procs: Option<&[String]>,
    store_open: Option<bool>,
) -> Result<FreezeCopyResult, String> {
    use std::os::unix::fs::PermissionsExt;
    if !enabled {
        return Err(COPY_DISABLED.to_string());
    }
    if running_state(procs) != "no" {
        return Err(RUNNING.to_string());
    }
    if !src.is_file() {
        return Err(NO_SOURCE.to_string());
    }
    // Nobody may hold the store or its -wal open (an unreadable answer counts as open).
    if store_open != Some(false) {
        return Err(STORE_OPEN.to_string());
    }
    let dir = dest.parent().ok_or_else(|| DEST_UNSAFE.to_string())?;
    ensure_private_dir(dir)?;
    if std::fs::symlink_metadata(dest).is_ok() {
        return Err(DEST_EXISTS.to_string());
    }
    let tmp = dir.join(format!(".whoop.sqlite.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let guard = TmpGuard(tmp.clone());

    let source = super::noop::open_store(src).map_err(|_| COPY_FAILED.to_string())?;
    {
        // Created 0600 by us first, so SQLite never creates the file with a looser mode.
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|_| COPY_FAILED.to_string())?;
        }
        let mut out = Connection::open_with_flags(
            &tmp,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| COPY_FAILED.to_string())?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| COPY_FAILED.to_string())?;
        let bk = Backup::new(&source, &mut out).map_err(|_| COPY_FAILED.to_string())?;
        bk.run_to_completion(256, Duration::from_millis(5), None)
            .map_err(|_| COPY_FAILED.to_string())?;
        drop(bk);
        // Fold any journal into the single file before the checks and the rename.
        let _ = out.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }

    // Verify the copy against the source.
    let copy = Connection::open_with_flags(
        &tmp,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| VERIFY_FAILED.to_string())?;
    let ok: String = copy
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| VERIFY_FAILED.to_string())?;
    if ok != "ok" {
        return Err(VERIFY_FAILED.to_string());
    }
    let want = all_stats(&source).map_err(|_| VERIFY_FAILED.to_string())?;
    let got = all_stats(&copy).map_err(|_| VERIFY_FAILED.to_string())?;
    if want != got
        || want.iter().any(|t| t.rows.is_none())
        || user_version(&source) != user_version(&copy)
        || migrations(&source) != migrations(&copy)
    {
        return Err(VERIFY_FAILED.to_string());
    }
    let (migration_count, _) = migrations(&copy);
    drop(copy);

    std::fs::File::open(&tmp)
        .and_then(|f| f.sync_all())
        .map_err(|_| COPY_FAILED.to_string())?;
    // hard_link fails if `dest` exists: an atomic publish that can never overwrite.
    std::fs::hard_link(&tmp, dest).map_err(|_| DEST_EXISTS.to_string())?;
    drop(guard);
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(FreezeCopyResult {
        ok: true,
        tables: got,
        migration_count,
    })
}

/// Gated and click-only. Refuses while the switch is off or NOOP.app runs; asks in the native
/// dialog (names the destination folder, never a value); returns counts only.
#[tauri::command]
#[specta::specta]
pub async fn noop_freeze_copy(app: tauri::AppHandle) -> Result<FreezeCopyResult, String> {
    if !FREEZE_COPY_ENABLED {
        return Err(COPY_DISABLED.to_string());
    }
    if running_state(live_process_paths().as_deref()) != "no" {
        return Err(RUNNING.to_string());
    }
    // Check everything that can be checked before bothering Taisun with a dialog.
    let src = super::noop::resolve_store_path().map_err(|_| NO_SOURCE.to_string())?;
    if !src.is_file() {
        return Err(NO_SOURCE.to_string());
    }
    if store_open_by_any(&src) != Some(false) {
        return Err(STORE_OPEN.to_string());
    }
    let text = super::approval_gate::approval_text(
        "Copy the NOOP health store into Amara",
        "A verified copy is made in Amara's data folder (health/whoop.sqlite). The NOOP store is only read, never changed. Choose Allow, not Always Allow. Allow only if you just asked for this and NOOP is quit.",
    )?;
    let key = super::approval_gate::service_key("noop-freeze-copy");
    let _guard = super::approval_gate::require_approval(&app, &[key], text).await?;
    tauri::async_runtime::spawn_blocking(move || {
        copy_core(
            &src,
            &dest_path(&crate::app_paths::data_dir()),
            FREEZE_COPY_ENABLED,
            live_process_paths().as_deref(),
            store_open_by_any(&src),
        )
    })
    .await
    .map_err(|_| COPY_FAILED.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Names(Vec<&'static str>);
    impl KeychainNames for Names {
        fn exists(&self, s: &str) -> bool {
            self.0.contains(&s)
        }
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-freeze-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    /// A fake NOOP store: GRDB bookkeeping, a table with `ts`, one without, rows with a sentinel
    /// "measurement" that must never appear in any output.
    fn make_store(dir: &Path) -> PathBuf {
        let p = dir.join("whoop.sqlite");
        let c = Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE grdb_migrations (identifier TEXT NOT NULL PRIMARY KEY);
             INSERT INTO grdb_migrations VALUES ('v1'),('v2'),('v47-last');
             CREATE TABLE hrSample (deviceId TEXT, ts INTEGER, bpm INTEGER);
             INSERT INTO hrSample VALUES ('d', 1000, 987654), ('d', 2500, 987655), ('d', 1700, 987656);
             CREATE TABLE dailyMetric (day TEXT, recovery REAL);
             INSERT INTO dailyMetric VALUES ('2026-11-01', 987.654);
             CREATE TABLE \"odd\"\"name\" (ts TEXT);
             INSERT INTO \"odd\"\"name\" VALUES ('2026-11-02T00:00:00Z');
             PRAGMA user_version = 47;",
        )
        .unwrap();
        p
    }

    fn quiet() -> Vec<String> {
        vec![
            "/Applications/Amara OS.app/Contents/MacOS/nexus".into(),
            "/usr/bin/sandbox-exec".into(),
            "/x/Packages/NoopLocalAccess/.build/release/noop-local-access".into(),
        ]
    }

    #[test]
    fn noop_app_process_detection() {
        assert!(is_noop_app_process(
            "/Applications/NOOP Lucid Preview.app/Contents/MacOS/NOOP Lucid Preview"
        ));
        assert!(is_noop_app_process(
            "/Applications/NOOP.app/Contents/MacOS/NOOP"
        ));
        assert!(is_noop_app_process(
            "/Users/x/Applications/Strand.app/Contents/MacOS/Strand"
        ));
        assert!(!is_noop_app_process(
            "/Applications/Amara OS.app/Contents/MacOS/nexus"
        ));
        assert!(!is_noop_app_process(
            "/Users/x/projects/noop-lucid-current/Packages/NoopLocalAccess/.build/release/noop-local-access"
        ));
        assert_eq!(running_state(None), "unknown");
        assert_eq!(running_state(Some(&quiet())), "no");
        let mut p = quiet();
        p.push("/Applications/NOOP.app/Contents/MacOS/NOOP".into());
        assert_eq!(running_state(Some(&p)), "yes");
    }

    #[test]
    fn report_has_counts_checksums_version_and_keychain_names_only() {
        let d = tmp();
        let store = make_store(&d);
        let before = std::fs::read(&store).unwrap();
        let rep = report_for(
            Some(&store),
            Some(&quiet()),
            &Names(vec!["com.noop.oura"]),
            false,
        );
        assert!(rep.store_found);
        assert_eq!(rep.user_version, Some(47));
        assert_eq!(rep.migration_count, Some(3));
        assert_eq!(rep.last_migration.as_deref(), Some("v47-last"));
        assert_eq!(rep.noop_running, "no");
        assert!(!rep.copy_enabled);
        let hr = rep.tables.iter().find(|t| t.name == "hrSample").unwrap();
        assert_eq!((hr.rows, hr.max_ts.as_deref()), (Some(3), Some("2500")));
        let dm = rep.tables.iter().find(|t| t.name == "dailyMetric").unwrap();
        assert_eq!((dm.rows, dm.max_ts.clone()), (Some(1), None));
        assert!(rep
            .tables
            .iter()
            .any(|t| t.name == "odd\"name" && t.rows == Some(1)));
        assert_eq!(rep.files.len(), 1);
        assert_eq!(rep.files[0].sha256.len(), 64);
        let names: Vec<_> = rep
            .keychain_items
            .iter()
            .map(|k| (k.name.as_str(), k.present))
            .collect();
        assert_eq!(
            names,
            vec![
                ("com.noop.oura", true),
                ("com.noop.oura.installkey", false),
                ("com.noop.aicoach", false)
            ]
        );
        // No measurement value appears anywhere in the serialized report, and the store is unchanged.
        let json = serde_json::to_string(&rep).unwrap();
        assert!(!json.contains("98765"));
        assert!(
            !json.contains(&d.display().to_string()),
            "no path in the report"
        );
        assert_eq!(std::fs::read(&store).unwrap(), before);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn report_for_missing_and_unreadable_stores_is_partial_not_an_error() {
        let d = tmp();
        let rep = report_for(None, None, &Names(vec![]), false);
        assert!(!rep.store_found);
        assert_eq!(rep.noop_running, "unknown");
        assert!(rep.note.is_some());
        let junk = d.join("whoop.sqlite");
        std::fs::write(&junk, b"this is not a database at all, just bytes").unwrap();
        let rep = report_for(Some(&junk), Some(&quiet()), &Names(vec![]), false);
        assert!(rep.store_found);
        assert!(rep.tables.is_empty());
        assert!(rep.note.is_some());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_is_off_by_default_and_the_command_is_gated() {
        const { assert!(!FREEZE_COPY_ENABLED) };
        let d = tmp();
        let store = make_store(&d);
        let dest = d.join("out/health/whoop.sqlite");
        let r = copy_core(&store, &dest, false, Some(&quiet()), Some(false));
        assert_eq!(r.unwrap_err(), COPY_DISABLED);
        assert!(
            !dest.exists() && !d.join("out").exists(),
            "nothing created when off"
        );
        let src = include_str!("noop_freeze.rs");
        let cmd = &src[src.find("pub async fn noop_freeze_copy").unwrap()..];
        let cmd = &cmd[..cmd.find("#[cfg(test)]").unwrap()];
        let i_gate = cmd.find("FREEZE_COPY_ENABLED").unwrap();
        let i_run = cmd.find("running_state").unwrap();
        let i_ask = cmd.find("require_approval").unwrap();
        assert!(
            i_gate < i_run && i_run < i_ask,
            "switch, then running check, then the dialog"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_refuses_while_noop_runs_or_the_process_list_is_unknown() {
        let d = tmp();
        let store = make_store(&d);
        let dest = d.join("out/health/whoop.sqlite");
        let mut running = quiet();
        running
            .push("/Applications/NOOP Lucid Preview.app/Contents/MacOS/NOOP Lucid Preview".into());
        assert_eq!(
            copy_core(&store, &dest, true, Some(&running), Some(false)).unwrap_err(),
            RUNNING
        );
        assert_eq!(
            copy_core(&store, &dest, true, None, Some(false)).unwrap_err(),
            RUNNING
        );
        assert!(!d.join("out").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_refuses_while_any_process_has_the_store_open_or_it_cannot_be_checked() {
        let d = tmp();
        let store = make_store(&d);
        let dest = dest_path(&d.join("data"));
        for st in [Some(true), None] {
            assert_eq!(
                copy_core(&store, &dest, true, Some(&quiet()), st).unwrap_err(),
                STORE_OPEN
            );
        }
        assert!(!d.join("data").exists(), "nothing created on refusal");
        // Reading of lsof results: exit 1 + no output = closed; output = open; anything else unknown.
        assert_eq!(open_state_from(Some(1), b""), Some(false));
        assert_eq!(open_state_from(Some(0), b"p123\n"), Some(true));
        assert_eq!(open_state_from(Some(0), b""), None);
        assert_eq!(open_state_from(Some(1), b"p1\n"), None);
        assert_eq!(open_state_from(Some(2), b""), None);
        assert_eq!(open_state_from(None, b""), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn live_lsof_sees_a_temp_file_held_open() {
        let d = tmp();
        let f = d.join("held.sqlite");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(store_open_by_any(&f), Some(false));
        let held = std::fs::File::open(&f).unwrap();
        assert_eq!(store_open_by_any(&f), Some(true));
        drop(held);
        assert_eq!(store_open_by_any(&f), Some(false));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn intermediate_folders_are_created_0700_and_the_temp_file_is_precreated_0600() {
        let d = tmp();
        let store = make_store(&d);
        let dest = d.join("a/b/health/whoop.sqlite");
        copy_core(&store, &dest, true, Some(&quiet()), Some(false)).unwrap();
        for sub in ["a", "a/b", "a/b/health"] {
            let m = std::fs::metadata(d.join(sub)).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, 0o700, "{sub}");
        }
        let src = include_str!("noop_freeze.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        let pre = prod.find(".create_new(true)").unwrap();
        let open = prod
            .find("SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX")
            .unwrap();
        assert!(pre < open, "0600 file exists before SQLite opens it");
        assert!(!prod.contains("SQLITE_OPEN_CREATE"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn verified_copy_matches_and_leaves_the_source_untouched() {
        let d = tmp();
        let store = make_store(&d);
        let before = std::fs::read(&store).unwrap();
        let dest = dest_path(&d.join("data"));
        let res = copy_core(&store, &dest, true, Some(&quiet()), Some(false)).unwrap();
        assert!(res.ok);
        assert_eq!(res.migration_count, Some(3));
        let hr = res.tables.iter().find(|t| t.name == "hrSample").unwrap();
        assert_eq!((hr.rows, hr.max_ts.as_deref()), (Some(3), Some("2500")));
        let c = Connection::open_with_flags(&dest, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let ic: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ic, "ok");
        assert_eq!(user_version(&c), Some(47));
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(dest.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        // No temp file left behind; the source bytes are identical.
        let names: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["whoop.sqlite".to_string()]);
        assert_eq!(std::fs::read(&store).unwrap(), before);
        let json = serde_json::to_string(&res).unwrap();
        assert!(!json.contains("98765"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_never_overwrites_and_refuses_unsafe_destinations() {
        let d = tmp();
        let store = make_store(&d);
        let dest = dest_path(&d.join("data"));
        copy_core(&store, &dest, true, Some(&quiet()), Some(false)).unwrap();
        let first = std::fs::read(&dest).unwrap();
        assert_eq!(
            copy_core(&store, &dest, true, Some(&quiet()), Some(false)).unwrap_err(),
            DEST_EXISTS
        );
        assert_eq!(std::fs::read(&dest).unwrap(), first);
        // A symlinked health folder is refused.
        let target = d.join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        let link_root = d.join("data2");
        std::fs::create_dir_all(&link_root).unwrap();
        std::os::unix::fs::symlink(&target, link_root.join("health")).unwrap();
        assert_eq!(
            copy_core(
                &store,
                &dest_path(&link_root),
                true,
                Some(&quiet()),
                Some(false)
            )
            .unwrap_err(),
            DEST_UNSAFE
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn missing_or_corrupt_source_publishes_nothing() {
        let d = tmp();
        let dest = dest_path(&d.join("data"));
        assert_eq!(
            copy_core(
                &d.join("nope.sqlite"),
                &dest,
                true,
                Some(&quiet()),
                Some(false)
            )
            .unwrap_err(),
            NO_SOURCE
        );
        let junk = d.join("junk.sqlite");
        std::fs::write(&junk, b"not a database, only some bytes here to be garbage").unwrap();
        assert!(copy_core(&junk, &dest, true, Some(&quiet()), Some(false)).is_err());
        assert!(!dest.exists(), "no destination on failure");
        let leftovers = std::fs::read_dir(d.join("data/health"))
            .map(|r| r.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "temp file cleaned up");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn source_is_only_opened_read_only_and_no_secret_is_read() {
        let src = include_str!("noop_freeze.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(prod.contains("noop::open_store"));
        // The only read-write open is the temp destination.
        assert_eq!(prod.matches("SQLITE_OPEN_READ_WRITE").count(), 1);
        assert!(
            !prod.contains("get_generic_password"),
            "names only, never a secret read"
        );
        assert!(!prod.contains(".load_data(") && !prod.contains("load_refs"));
        assert!(!prod.contains("Command::new(\"/usr/bin/security\")"));
    }
}
