use std::fs;

// Simple key-value store persisted to the app data directory.
// Each key maps to a file: {app_data}/kv/{key}.json

/// `{app data}/kv`; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn kv_root() -> std::path::PathBuf {
    crate::app_paths::data_dir().join("kv")
}

fn kv_dir() -> Result<std::path::PathBuf, String> {
    let dir = kv_root();
    if !dir.exists() {
        crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Exact rejection for protected keys via the generic commands.
pub const RESERVED_KEY: &str = "reserved key";

/// Keys the webview must not touch through the generic save_kv/load_kv commands. Each has a
/// dedicated, validated command instead.
/// - `noop_settings`: NOOP binary/db paths (the binary is executed) and WHOOP source. Written only
///   by `set_noop_settings` (validation + native confirm). Load is refused too: it holds
///   canonical filesystem paths; read it via `get_noop_settings`.
pub(crate) const PROTECTED_KEYS: &[&str] = &["noop_settings"];

/// Normalise for the protected check: last path segment, strip ".json", trim, drop dots,
/// lowercase, same sanitisation as the filename, trim '_'. Defeats case and path tricks.
fn normalize_key(key: &str) -> String {
    let last = key.rsplit(['/', '\\']).next().unwrap_or(key).trim();
    let lower = last.to_lowercase();
    let no_ext = lower.strip_suffix(".json").unwrap_or(&lower);
    let no_dots: String = no_ext.chars().filter(|c| *c != '.').collect();
    sanitize_key(no_dots.trim()).trim_matches('_').to_string()
}

fn is_protected(key: &str) -> bool {
    let n = normalize_key(key);
    // Also check the exact filename form the key would be stored under.
    let f = sanitize_key(key).to_lowercase();
    PROTECTED_KEYS
        .iter()
        .any(|p| n == *p || f == *p || f.trim_matches('_') == *p)
}

fn load_in(dir: &std::path::Path, key: &str) -> Result<Option<String>, String> {
    let path = dir.join(format!("{}.json", sanitize_key(key)));
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn save_in(dir: &std::path::Path, key: &str, value: String) -> Result<(), String> {
    let path = dir.join(format!("{}.json", sanitize_key(key)));
    crate::app_paths::write_private(&path, value).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn load_kv(key: String) -> Result<Option<String>, String> {
    if is_protected(&key) {
        return Err(RESERVED_KEY.into());
    }
    load_in(&kv_dir()?, &key)
}

#[tauri::command]
#[specta::specta]
pub fn save_kv(key: String, value: String) -> Result<(), String> {
    if is_protected(&key) {
        return Err(RESERVED_KEY.into());
    }
    save_in(&kv_dir()?, &key, value)
}

/// Rust-side reader; may read protected keys. Not a command.
pub(crate) fn load_kv_internal(key: &str) -> Result<Option<String>, String> {
    load_in(&kv_dir()?, key)
}

/// Rust-side writer; may write protected keys. Not a command.
pub(crate) fn save_kv_internal(key: &str, value: String) -> Result<(), String> {
    save_in(&kv_dir()?, key, value)
}

#[cfg(test)]
mod protected_tests {
    use super::*;

    #[test]
    fn protected_key_variants_are_reserved_and_file_untouched() {
        let dir = std::env::temp_dir().join(format!("kvprot-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        save_in(&dir, "noop_settings", "{\"orig\":1}".into()).unwrap();
        let file = dir.join("noop_settings.json");
        let before = fs::read(&file).unwrap();
        for k in [
            "noop_settings",
            "NOOP_Settings",
            " noop_settings ",
            "./noop_settings",
            "../x/noop_settings",
            "..\\noop_settings",
            "noop_settings.json",
            "noop.settings",
            "_noop_settings_",
        ] {
            assert!(is_protected(k), "{k}");
            assert_eq!(
                save_kv(k.into(), "{}".into()),
                Err(RESERVED_KEY.to_string()),
                "{k}"
            );
            assert_eq!(load_kv(k.into()), Err(RESERVED_KEY.to_string()), "{k}");
        }
        assert_eq!(fs::read(&file).unwrap(), before);
        assert!(!is_protected("ritual_timestamps"));
        assert!(!is_protected("autoheal.declinedUntil"));
        let _ = fs::remove_dir_all(&dir);
    }
}
