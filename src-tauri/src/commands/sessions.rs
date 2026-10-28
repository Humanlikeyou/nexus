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

fn sessions_dir() -> PathBuf {
    nexus_data_dir().join("sessions")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub avg_focus: Option<f64>,
    pub avg_relaxation: Option<f64>,
    pub avg_hr: Option<f64>,
    pub max_hr: Option<f64>,
    pub avg_hrv: Option<f64>,
    // Muse training only; older session files don't have them.
    #[serde(default)]
    pub calm_pct: Option<f64>,
    #[serde(default)]
    pub time_in_target_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedSession {
    pub id: String,
    pub device_type: String,
    pub start: String,
    pub end: String,
    pub duration_secs: u64,
    pub data_points: usize,
    pub file_path: String,
    pub summary: SessionSummary,
}

#[tauri::command]
pub fn save_session(session: RecordedSession, data: serde_json::Value) -> Result<String, String> {
    let dir = sessions_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create sessions dir: {}", e))?;

    super::path_safety::validate_name(&session.device_type)?;
    super::path_safety::validate_name(&session.id)?;
    let filename = format!("{}_{}.json", session.device_type, session.id);
    let path = super::path_safety::safe_join(&dir, &filename)?;

    let payload = serde_json::json!({
        "session": session,
        "data": data,
    });

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write session: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn list_sessions() -> Result<Vec<RecordedSession>, String> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let parsed: serde_json::Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(session_val) = parsed.get("session") {
            if let Ok(session) = serde_json::from_value::<RecordedSession>(session_val.clone()) {
                sessions.push(session);
            }
        }
    }

    // Sort by start time descending
    sessions.sort_by(|a, b| b.start.cmp(&a.start));
    Ok(sessions)
}

#[tauri::command]
pub fn delete_session(session_id: String) -> Result<(), String> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains(&session_id) {
            fs::remove_file(entry.path())
                .map_err(|e| format!("Failed to delete session: {}", e))?;
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_without_calm_fields_still_loads() {
        let old = serde_json::json!({ "avg_focus": 0.4, "avg_relaxation": 0.6, "avg_hr": null, "max_hr": null, "avg_hrv": null });
        let summary: SessionSummary = serde_json::from_value(old).unwrap();
        assert_eq!(summary.calm_pct, None);
        assert_eq!(summary.time_in_target_pct, None);
    }

    #[test]
    fn calm_fields_round_trip() {
        let new = serde_json::json!({ "avg_focus": null, "avg_relaxation": null, "avg_hr": null, "max_hr": null, "avg_hrv": null, "calm_pct": 61.5, "time_in_target_pct": 40.0 });
        let summary: SessionSummary = serde_json::from_value(new).unwrap();
        assert_eq!(summary.calm_pct, Some(61.5));
        assert_eq!(
            serde_json::to_value(&summary).unwrap()["time_in_target_pct"],
            40.0
        );
    }
}
