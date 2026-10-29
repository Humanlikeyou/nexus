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

#[tauri::command]
pub fn load_kv(key: String) -> Result<Option<String>, String> {
    let path = kv_dir()?.join(format!("{}.json", sanitize_key(&key)));
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(Some(content))
}

#[tauri::command]
pub fn save_kv(key: String, value: String) -> Result<(), String> {
    let path = kv_dir()?.join(format!("{}.json", sanitize_key(&key)));
    crate::app_paths::write_private(&path, value).map_err(|e| e.to_string())
}
