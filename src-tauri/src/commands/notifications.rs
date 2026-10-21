use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap()
                .join("Library/Application Support")
        })
        .join(crate::app_paths::DATA_DIR_NAME)
}

fn notifications_path() -> PathBuf {
    nexus_data_dir().join("notifications.json")
}

/// Mirror of the TypeScript AppNotification interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNotification {
    pub id: String,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub title: String,
    pub message: String,
    pub source: String,
    pub timestamp: i64,
    pub read: bool,
}

#[tauri::command]
pub fn read_notifications() -> Result<Vec<StoredNotification>, String> {
    let path = notifications_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read notifications: {}", e))?;
    let notifications: Vec<StoredNotification> =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse notifications: {}", e))?;
    Ok(notifications)
}

#[tauri::command]
pub fn save_notifications(notifications: Vec<StoredNotification>) -> Result<(), String> {
    let path = notifications_path();
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    let data = serde_json::to_string_pretty(&notifications)
        .map_err(|e| format!("Failed to serialize notifications: {}", e))?;
    fs::write(&path, data).map_err(|e| format!("Failed to write notifications: {}", e))?;
    Ok(())
}
