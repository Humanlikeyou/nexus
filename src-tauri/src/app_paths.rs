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
pub fn data_dir() -> PathBuf {
    data_dir_with_override(std::env::var_os(DATA_DIR_ENV))
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
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("config.json.bak-"))
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
            vec!["gateway_auth_token", "gateway_url", "llm_token_threshold", "telegram_chat_id"]
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
        assert_eq!(merge_legacy_config_in(&base).unwrap(), vec!["a".to_string()]);
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
}
