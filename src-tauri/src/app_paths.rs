//! App-data location and one-time migration from the legacy
//! `com.taisun.electron` directory to `com.taisun.nexus`.

use std::fs;
use std::path::{Path, PathBuf};

pub const DATA_DIR_NAME: &str = "com.taisun.nexus";
const LEGACY_DIR_NAME: &str = "com.taisun.electron";
const MARKER: &str = ".migrated_from_electron";

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

pub fn migrate_legacy_data_dir() {
    let Some(base) = dirs::data_dir() else { return };
    match migrate_legacy_data_dir_in(&base) {
        Ok(c) if !c.is_empty() => eprintln!("[nexus] migrated {} legacy app-data entries", c.len()),
        Ok(_) => {}
        Err(e) => eprintln!("[nexus] legacy app-data migration failed: {e}"),
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
}
