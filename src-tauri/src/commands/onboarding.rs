use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn onboarding_path() -> PathBuf {
    nexus_data_dir().join("onboarding.json")
}

#[tauri::command]
#[specta::specta]
pub fn check_onboarding() -> Result<bool, String> {
    Ok(onboarding_path().exists())
}

#[tauri::command]
#[specta::specta]
pub fn complete_onboarding() -> Result<(), String> {
    let path = onboarding_path();
    let dir = path.parent().unwrap();
    crate::app_paths::create_private_dir_all(dir)
        .map_err(|e| format!("Failed to create dir: {}", e))?;

    let data = serde_json::json!({
        "completed_at": chrono::Local::now().to_rfc3339(),
        "version": "0.1.0"
    });
    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&path, json)
        .map_err(|e| format!("Failed to write onboarding: {}", e))?;
    Ok(())
}
