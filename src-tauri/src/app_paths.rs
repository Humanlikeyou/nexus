//! App-data location and one-time migration from the legacy
//! `com.taisun.electron` directory to `com.taisun.nexus`.

use std::fs;
use std::path::{Path, PathBuf};

pub const DATA_DIR_NAME: &str = "com.taisun.nexus";
const LEGACY_DIR_NAME: &str = "com.taisun.electron";
const MARKER: &str = ".migrated_from_electron";
const CONFIG_FILE: &str = "config.json";
const CONFIG_MERGE_MARKER: &str = ".config_merged_v2";

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        fs::copy(src, dst)?;
    }
    Ok(())
}

/// Copy legacy data into the new directory. Never deletes or overwrites:
/// entries already present in the new dir are kept as-is. Runs once (marker).
pub fn migrate_legacy_data_dir_in(base: &Path) -> std::io::Result<Vec<String>> {
    let old = base.join(LEGACY_DIR_NAME);
    let new = base.join(DATA_DIR_NAME);
    let mut copied = Vec::new();
    if !old.is_dir() || new.join(MARKER).exists() {
        return Ok(copied);
    }
    fs::create_dir_all(&new)?;
    for entry in fs::read_dir(&old)? {
        let entry = entry?;
        let target = new.join(entry.file_name());
        if target.exists() {
            continue;
        }
        copy_recursive(&entry.path(), &target)?;
        copied.push(entry.file_name().to_string_lossy().to_string());
    }
    fs::write(
        new.join(MARKER),
        "legacy data copied; old directory left in place\n",
    )?;
    Ok(copied)
}

fn is_empty_value(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

/// Fill keys that are missing, null or "" in the new config.json from the
/// legacy one. Never overwrites a non-empty value. The first migration skipped
/// config.json whenever a (stale) one already existed, so this runs once more
/// on installs that already carry the first marker. Before rewriting, the
/// current file is backed up to `config.json.bak-<timestamp>` (0600); the
/// result is written atomically with mode 0600. Returns the merged key names.
pub fn merge_legacy_config_in(base: &Path) -> std::io::Result<Vec<String>> {
    use std::io::{Error, ErrorKind};
    let new = base.join(DATA_DIR_NAME);
    let new_cfg = new.join(CONFIG_FILE);
    let old_cfg = base.join(LEGACY_DIR_NAME).join(CONFIG_FILE);
    let mut merged = Vec::new();
    if new.join(CONFIG_MERGE_MARKER).exists() || !new_cfg.is_file() || !old_cfg.is_file() {
        return Ok(merged);
    }
    let parse = |p: &Path| -> std::io::Result<serde_json::Map<String, serde_json::Value>> {
        match serde_json::from_slice(&fs::read(p)?) {
            Ok(serde_json::Value::Object(m)) => Ok(m),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                format!("{} is not a JSON object", p.display()),
            )),
        }
    };
    // Parse failures leave everything untouched and the marker unwritten.
    let current_bytes = fs::read(&new_cfg)?;
    let mut current = parse(&new_cfg)?;
    let legacy = parse(&old_cfg)?;
    for (key, value) in legacy {
        if is_empty_value(Some(&value)) || !is_empty_value(current.get(&key)) {
            continue;
        }
        current.insert(key.clone(), value);
        merged.push(key);
    }
    if merged.is_empty() {
        // Nothing to merge, but the file may hold a bot token: tighten it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&new_cfg, fs::Permissions::from_mode(0o600))?;
        }
    } else {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let backup = new.join(format!("{CONFIG_FILE}.bak-{stamp}"));
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        std::io::Write::write_all(&mut opts.open(&backup)?, &current_bytes)?;
        let json = serde_json::to_string_pretty(&serde_json::Value::Object(current))
            .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
        crate::commands::continuity::atomic_write_with_mode(
            &new_cfg,
            json.as_bytes(),
            Some(0o600),
        )?;
    }
    fs::write(
        new.join(CONFIG_MERGE_MARKER),
        "legacy config.json keys merged; old directory left in place\n",
    )?;
    Ok(merged)
}

pub fn migrate_legacy_data_dir() {
    // A dev NEXUS_CONFIG_DIR profile is self-contained: never copy into the real data dir.
    if dev_config_dir_set() {
        return;
    }
    let Some(base) = dirs::data_dir() else { return };
    match migrate_legacy_data_dir_in(&base) {
        Ok(c) if !c.is_empty() => eprintln!("[nexus] migrated {} legacy app-data entries", c.len()),
        Ok(_) => {}
        Err(e) => eprintln!("[nexus] legacy app-data migration failed: {e}"),
    }
    match merge_legacy_config_in(&base) {
        Ok(k) if !k.is_empty() => eprintln!("[nexus] merged {} legacy config keys", k.len()),
        Ok(_) => {}
        Err(e) => eprintln!("[nexus] legacy config merge failed: {e}"),
    }
}

/// Environment variable that overrides the app data directory
/// (tests, dev profiles, portable installs).
pub const DATA_DIR_ENV: &str = "NEXUS_DATA_DIR";

/// The app data directory: `$NEXUS_DATA_DIR` if set and non-empty, else
/// `<platform data dir>/com.taisun.nexus`.
/// Create `path` and any missing parents as 0700 (owner only). Existing directories are left
/// as they are: this never chmods. Security review 2026-10-29 #1: the data dir held EEG
/// replays, chat history and kv readable by other local users (default umask 0755/0644).
pub fn create_private_dir_all(path: &Path) -> std::io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path)
}

/// OpenOptions whose newly created files are 0600 (owner only). Existing files keep their mode.
pub fn private_open_options() -> fs::OpenOptions {
    let mut o = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// `fs::write`, but a newly created file is 0600.
pub fn write_private(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = private_open_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes.as_ref())
}

/// Create an SQLite database file as 0600 before `Connection::open`, so the db (and its
/// -wal/-shm, which SQLite creates with the db file's mode) is owner-only.
pub fn ensure_private_file(path: &Path) -> std::io::Result<()> {
    match private_open_options()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// Set an SQLite database and its `-wal`/`-shm` sidecars (those that exist) to 0600. Call after
/// opening the db in WAL mode: SQLite gives the sidecars the db file's mode, so an older 0644 db
/// would otherwise keep producing 0644 sidecars. Never follows or changes a symlink.
pub fn restrict_sqlite_files(db: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let name = db.as_os_str().to_os_string();
        for suffix in ["", "-wal", "-shm"] {
            let mut p = name.clone();
            p.push(suffix);
            let p = PathBuf::from(p);
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_file() => {
                    if m.permissions().mode() & 0o777 != 0o600 {
                        fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    #[cfg(not(unix))]
    let _ = db;
    Ok(())
}

/// Dev-only override of the whole app data/config directory (config.json, the app `.env` with
/// the HA token, continuity data, kv, dbs, …). Honoured only when `debug_assertions` is on;
/// release builds ignore it completely. It wins over `NEXUS_DATA_DIR`.
///
/// Rules: absolute, no `..` component; created 0700 if missing; refused if it is a symlink,
/// not a directory, not owned by the current user, or group/world-writable. A refused value
/// never falls back to the real data dir: `check_dev_config_dir` exits at startup, and
/// `data_dir` panics (dev builds only).
pub const CONFIG_DIR_ENV: &str = "NEXUS_CONFIG_DIR";

const DEV_OVERRIDE_ENABLED: bool = cfg!(debug_assertions);

fn dev_config_dir_set() -> bool {
    DEV_OVERRIDE_ENABLED && std::env::var_os(CONFIG_DIR_ENV).is_some_and(|v| !v.is_empty())
}

/// Validates (and creates if missing) a `NEXUS_CONFIG_DIR` value. Errors are fixed text plus
/// the reason; the path itself is not secret.
fn validate_config_dir(raw: &std::ffi::OsStr, uid: u32) -> Result<PathBuf, String> {
    use std::path::Component;
    let p = PathBuf::from(raw);
    if !p.is_absolute() {
        return Err(format!("{CONFIG_DIR_ENV} must be an absolute path"));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{CONFIG_DIR_ENV} must not contain '..'"));
    }
    let meta = match fs::symlink_metadata(&p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            create_private_dir_all(&p)
                .map_err(|_| format!("{CONFIG_DIR_ENV} could not be created"))?;
            fs::symlink_metadata(&p).map_err(|_| format!("{CONFIG_DIR_ENV} is not readable"))?
        }
        Err(_) => return Err(format!("{CONFIG_DIR_ENV} is not readable")),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("{CONFIG_DIR_ENV} must not be a symlink"));
    }
    if !meta.is_dir() {
        return Err(format!("{CONFIG_DIR_ENV} must be a directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != uid {
            return Err(format!(
                "{CONFIG_DIR_ENV} must be owned by the current user"
            ));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(format!(
                "{CONFIG_DIR_ENV} must not be group- or world-writable"
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = uid;
    Ok(p)
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    0
}

/// Resolution with every input injected (tests). `dev` = debug build.
fn resolve_data_dir(
    dev: bool,
    config_dir: Option<std::ffi::OsString>,
    data_dir_env: Option<std::ffi::OsString>,
    uid: u32,
) -> Result<PathBuf, String> {
    if dev {
        if let Some(raw) = config_dir.filter(|v| !v.is_empty()) {
            return validate_config_dir(&raw, uid);
        }
    }
    Ok(data_dir_with_override(data_dir_env))
}

/// Startup check: in a dev build with a bad `NEXUS_CONFIG_DIR`, exit before anything reads or
/// writes app data (never fall back to the real directory). No-op in release builds.
pub fn check_dev_config_dir() {
    if !dev_config_dir_set() {
        return;
    }
    match resolve_data_dir(true, std::env::var_os(CONFIG_DIR_ENV), None, current_uid()) {
        Ok(p) => eprintln!(
            "[nexus] dev build: app data in {} ({CONFIG_DIR_ENV})",
            p.display()
        ),
        Err(e) => {
            eprintln!("[nexus] refusing to start: {e}");
            std::process::exit(78); // EX_CONFIG
        }
    }
}

pub fn data_dir() -> PathBuf {
    let config_dir = if DEV_OVERRIDE_ENABLED {
        std::env::var_os(CONFIG_DIR_ENV)
    } else {
        None
    };
    match resolve_data_dir(
        DEV_OVERRIDE_ENABLED,
        config_dir,
        std::env::var_os(DATA_DIR_ENV),
        current_uid(),
    ) {
        Ok(p) => p,
        // Dev builds only (release never validates); startup already exited on this.
        Err(e) => panic!("{e}"),
    }
}

fn data_dir_with_override(override_dir: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(dir) = override_dir.filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DATA_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn private_helpers_create_0700_dirs_and_0600_files_and_never_chmod_existing() {
        use std::os::unix::fs::PermissionsExt;
        let base =
            std::env::temp_dir().join(format!("nexus-priv-{}", uuid::Uuid::new_v4().simple()));
        let nested = base.join("a/b");
        create_private_dir_all(&nested).unwrap();
        for d in [&base, &base.join("a"), &nested] {
            assert_eq!(
                fs::metadata(d).unwrap().permissions().mode() & 0o777,
                0o700,
                "{}",
                d.display()
            );
        }
        let f = nested.join("x.json");
        write_private(&f, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let db = nested.join("x.db");
        ensure_private_file(&db).unwrap();
        ensure_private_file(&db).unwrap();
        assert_eq!(
            fs::metadata(&db).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Existing paths keep their mode (no chmod of existing data).
        let old = base.join("old");
        fs::create_dir(&old).unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir_all(&old).unwrap();
        assert_eq!(
            fs::metadata(&old).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let oldf = base.join("old.json");
        fs::write(&oldf, b"1").unwrap();
        fs::set_permissions(&oldf, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&oldf, b"2").unwrap();
        assert_eq!(
            fs::metadata(&oldf).unwrap().permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn restrict_sqlite_files_sets_db_wal_shm_to_0600_and_skips_missing_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let base =
            std::env::temp_dir().join(format!("nexus-sqlite-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&base).unwrap();
        let db = base.join("chat_history.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            conn.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);")
                .unwrap();
            for s in ["", "-wal", "-shm"] {
                let p = PathBuf::from(format!("{}{s}", db.display()));
                fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
            }
            restrict_sqlite_files(&db).unwrap();
            for s in ["", "-wal", "-shm"] {
                let p = PathBuf::from(format!("{}{s}", db.display()));
                assert_eq!(
                    fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                    0o600,
                    "{s}"
                );
            }
        }
        // Missing sidecars are fine; a symlinked sidecar is left alone (target unchanged).
        let other = base.join("other.db");
        fs::write(&other, b"").unwrap();
        let target = base.join("target");
        fs::write(&target, b"").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&target, base.join("other.db-wal")).unwrap();
        restrict_sqlite_files(&other).unwrap();
        assert_eq!(
            fs::metadata(&other).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn data_dir_honours_override() {
        assert_eq!(
            data_dir_with_override(Some("/tmp/nexus-custom".into())),
            PathBuf::from("/tmp/nexus-custom")
        );
        assert!(data_dir_with_override(Some("".into())).ends_with(DATA_DIR_NAME));
        assert!(data_dir_with_override(None).ends_with(DATA_DIR_NAME));
    }

    #[test]
    fn copies_missing_entries_without_overwriting() {
        let base = std::env::temp_dir().join(format!("nexus-mig-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(LEGACY_DIR_NAME).join("kv")).unwrap();
        fs::write(base.join(LEGACY_DIR_NAME).join("kv/a"), "old").unwrap();
        fs::write(base.join(LEGACY_DIR_NAME).join("config.json"), "old").unwrap();
        fs::create_dir_all(base.join(DATA_DIR_NAME)).unwrap();
        fs::write(base.join(DATA_DIR_NAME).join("config.json"), "new").unwrap();

        let copied = migrate_legacy_data_dir_in(&base).unwrap();
        assert_eq!(copied, vec!["kv".to_string()]);
        let n = base.join(DATA_DIR_NAME);
        assert_eq!(fs::read_to_string(n.join("kv/a")).unwrap(), "old");
        assert_eq!(fs::read_to_string(n.join("config.json")).unwrap(), "new");
        assert!(base.join(LEGACY_DIR_NAME).join("kv/a").exists());
        // second run is a no-op
        assert!(migrate_legacy_data_dir_in(&base).unwrap().is_empty());
        let _ = fs::remove_dir_all(&base);
    }

    use std::os::unix::fs::PermissionsExt;

    fn merge_fixture(tag: &str, legacy: &str, current: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("nexus-cfg-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(LEGACY_DIR_NAME)).unwrap();
        fs::create_dir_all(base.join(DATA_DIR_NAME)).unwrap();
        fs::write(base.join(LEGACY_DIR_NAME).join(CONFIG_FILE), legacy).unwrap();
        let cfg = base.join(DATA_DIR_NAME).join(CONFIG_FILE);
        fs::write(&cfg, current).unwrap();
        fs::set_permissions(&cfg, fs::Permissions::from_mode(0o644)).unwrap();
        // Existing installs already carry the first marker.
        fs::write(base.join(DATA_DIR_NAME).join(MARKER), "").unwrap();
        base
    }

    fn read_cfg(base: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(base.join(DATA_DIR_NAME).join(CONFIG_FILE)).unwrap())
            .unwrap()
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn backups(base: &Path) -> Vec<PathBuf> {
        fs::read_dir(base.join(DATA_DIR_NAME))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("config.json.bak-")
            })
            .collect()
    }

    #[test]
    fn config_merge_fills_missing_null_and_empty_keys_only() {
        let base = merge_fixture(
            "rules",
            r#"{"telegram_chat_id":"123","gateway_url":"http://legacy","gateway_auth_token":"tok",
               "compute_mode":"local","n8n_webhook_secret":"","llm_token_threshold":4000,
               "telegram_bot_token":"old-bot","only_legacy":null}"#,
            r#"{"telegram_chat_id":"","gateway_url":null,"telegram_bot_token":"new-bot",
               "compute_mode":"hybrid","extra":"keep"}"#,
        );
        let mut merged = merge_legacy_config_in(&base).unwrap();
        merged.sort();
        assert_eq!(
            merged,
            vec![
                "gateway_auth_token",
                "gateway_url",
                "llm_token_threshold",
                "telegram_chat_id"
            ]
        );
        let cfg = read_cfg(&base);
        assert_eq!(cfg["telegram_chat_id"], "123");
        assert_eq!(cfg["gateway_url"], "http://legacy");
        assert_eq!(cfg["llm_token_threshold"], 4000);
        // never overwrite a non-empty value
        assert_eq!(cfg["telegram_bot_token"], "new-bot");
        assert_eq!(cfg["compute_mode"], "hybrid");
        assert_eq!(cfg["extra"], "keep");
        // empty legacy values are not copied
        assert!(cfg.get("n8n_webhook_secret").is_none());
        assert!(cfg.get("only_legacy").is_none());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_merge_backs_up_and_writes_0600() {
        let current = r#"{"telegram_chat_id":""}"#;
        let base = merge_fixture("backup", r#"{"telegram_chat_id":"123"}"#, current);
        merge_legacy_config_in(&base).unwrap();
        let baks = backups(&base);
        assert_eq!(baks.len(), 1);
        assert_eq!(fs::read_to_string(&baks[0]).unwrap(), current);
        assert_eq!(mode(&baks[0]), 0o600);
        assert_eq!(mode(&base.join(DATA_DIR_NAME).join(CONFIG_FILE)), 0o600);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_merge_with_nothing_to_merge_only_tightens_perms() {
        let base = merge_fixture("noop", r#"{"a":"x"}"#, r#"{"a":"y"}"#);
        assert!(merge_legacy_config_in(&base).unwrap().is_empty());
        assert!(backups(&base).is_empty());
        assert_eq!(read_cfg(&base)["a"], "y");
        assert_eq!(mode(&base.join(DATA_DIR_NAME).join(CONFIG_FILE)), 0o600);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_merge_runs_once() {
        let base = merge_fixture("once", r#"{"a":"x"}"#, r#"{}"#);
        assert_eq!(
            merge_legacy_config_in(&base).unwrap(),
            vec!["a".to_string()]
        );
        assert!(base.join(DATA_DIR_NAME).join(CONFIG_MERGE_MARKER).exists());
        // a later legacy change is not picked up again
        fs::write(base.join(LEGACY_DIR_NAME).join(CONFIG_FILE), r#"{"b":"z"}"#).unwrap();
        assert!(merge_legacy_config_in(&base).unwrap().is_empty());
        assert!(read_cfg(&base).get("b").is_none());
        assert_eq!(backups(&base).len(), 1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_merge_leaves_invalid_json_untouched_and_retries() {
        let base = merge_fixture("bad", r#"{"a":"x"}"#, "not json");
        assert!(merge_legacy_config_in(&base).is_err());
        assert_eq!(
            fs::read_to_string(base.join(DATA_DIR_NAME).join(CONFIG_FILE)).unwrap(),
            "not json"
        );
        assert!(!base.join(DATA_DIR_NAME).join(CONFIG_MERGE_MARKER).exists());
        let _ = fs::remove_dir_all(&base);
    }

    // ── NEXUS_CONFIG_DIR (dev builds) ────────────────────────────────────────

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "nexus-cfgdir-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        d
    }

    #[cfg(unix)]
    #[test]
    fn config_dir_is_created_0700_and_wins_over_data_dir_in_dev() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("new").join("nested");
        let got = resolve_data_dir(
            true,
            Some(d.clone().into()),
            Some("/tmp/other".into()),
            current_uid(),
        )
        .unwrap();
        assert_eq!(got, d);
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // An existing private dir is accepted as is.
        assert_eq!(
            validate_config_dir(d.as_os_str(), current_uid()).unwrap(),
            d
        );
        let _ = fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn config_dir_ignored_in_release_and_when_empty() {
        let d = tmp("release");
        // Release: ignored completely, not even created or validated.
        assert_eq!(
            resolve_data_dir(false, Some(d.clone().into()), None, current_uid()).unwrap(),
            data_dir_with_override(None)
        );
        assert!(!d.exists());
        assert_eq!(
            resolve_data_dir(false, Some("relative/bad".into()), Some("/tmp/x".into()), 0).unwrap(),
            PathBuf::from("/tmp/x")
        );
        // Dev with an empty value: falls through to NEXUS_DATA_DIR / default.
        assert_eq!(
            resolve_data_dir(true, Some("".into()), Some("/tmp/x".into()), 0).unwrap(),
            PathBuf::from("/tmp/x")
        );
        assert_eq!(DEV_OVERRIDE_ENABLED, cfg!(debug_assertions));
    }

    #[cfg(unix)]
    #[test]
    fn config_dir_refusals() {
        use std::os::unix::fs::PermissionsExt;
        let uid = current_uid();
        assert!(validate_config_dir("relative/dir".as_ref(), uid).is_err());
        let base = tmp("bad");
        create_private_dir_all(&base).unwrap();
        let dotdot = format!("{}/a/../b", base.display());
        assert!(validate_config_dir(dotdot.as_ref(), uid)
            .unwrap_err()
            .contains(".."));
        assert!(!base.join("b").exists());
        // symlink to a private dir
        let real = base.join("real");
        create_private_dir_all(&real).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(validate_config_dir(link.as_os_str(), uid)
            .unwrap_err()
            .contains("symlink"));
        // group- and world-writable
        for mode in [0o770, 0o707, 0o777] {
            fs::set_permissions(&real, fs::Permissions::from_mode(mode)).unwrap();
            assert!(validate_config_dir(real.as_os_str(), uid)
                .unwrap_err()
                .contains("writable"));
        }
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        // wrong owner, and a regular file
        assert!(validate_config_dir(real.as_os_str(), uid + 1).is_err());
        let file = base.join("file");
        fs::write(&file, "x").unwrap();
        assert!(validate_config_dir(file.as_os_str(), uid).is_err());
        // dev resolution surfaces the refusal (never falls back to the default)
        assert!(resolve_data_dir(true, Some(link.into()), None, uid).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    /// Runs this test binary again with NEXUS_CONFIG_DIR set (no set_var in this process, so
    /// parallel tests are unaffected) and checks config.json, the app .env and continuity data
    /// all resolve, and are written, under it.
    #[cfg(debug_assertions)]
    #[test]
    fn config_json_app_env_and_continuity_follow_config_dir() {
        if std::env::var_os("NEXUS_CFGDIR_PROBE").is_some() {
            let d = PathBuf::from(std::env::var_os(CONFIG_DIR_ENV).unwrap());
            assert_eq!(data_dir(), d);
            assert_eq!(
                crate::commands::telegram::config_path(),
                d.join("config.json")
            );
            assert_eq!(crate::commands::ha::app_env_dir(), d);
            assert_eq!(
                crate::commands::continuity::context_path(),
                d.join("terrance_context.json")
            );
            assert!(crate::commands::continuity::activity_log_dir().starts_with(&d));
            crate::commands::telegram::update_config(|c| {
                c.ha_url = Some("http://127.0.0.1:8123".into())
            })
            .unwrap();
            assert!(d.join("config.json").is_file());
            return;
        }
        let d = tmp("probe");
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app_paths::tests::config_json_app_env_and_continuity_follow_config_dir",
                "--nocapture",
            ])
            .env("NEXUS_CFGDIR_PROBE", "1")
            .env(CONFIG_DIR_ENV, &d)
            .env_remove(DATA_DIR_ENV)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"));
        assert!(d.join("config.json").is_file());
        let _ = fs::remove_dir_all(&d);
    }
}
