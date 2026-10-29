use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
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

impl SessionSummary {
    /// Sessions recorded before the 0–100 score scale stored Avg focus / relaxation as 0–1.
    /// On read, a summary whose present scores all lie in 0..=1 is treated as legacy and scaled
    /// to 0–100. A real 0–100 average of ≤1 on every score is not a plausible session, so the
    /// heuristic cannot misfire on new files in practice; nothing is rewritten on disk.
    pub fn normalize_legacy_scores(&mut self) {
        let scores = [self.avg_focus, self.avg_relaxation];
        let present: Vec<f64> = scores.iter().flatten().copied().collect();
        let legacy = !present.is_empty()
            && present
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && present.iter().any(|v| *v > 0.0);
        if legacy {
            self.avg_focus = self.avg_focus.map(|v| v * 100.0);
            self.avg_relaxation = self.avg_relaxation.map(|v| v * 100.0);
        }
    }
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
            if let Ok(mut session) = serde_json::from_value::<RecordedSession>(session_val.clone())
            {
                session.summary.normalize_legacy_scores();
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

    fn summary(focus: Option<f64>, relaxation: Option<f64>) -> SessionSummary {
        SessionSummary {
            avg_focus: focus,
            avg_relaxation: relaxation,
            avg_hr: None,
            max_hr: None,
            avg_hrv: None,
            calm_pct: None,
            time_in_target_pct: None,
        }
    }

    #[test]
    fn legacy_unit_scores_are_scaled_to_0_100_on_read() {
        let mut old = summary(Some(0.4), Some(0.62));
        old.normalize_legacy_scores();
        assert!((old.avg_focus.unwrap() - 40.0).abs() < 1e-9);
        assert!((old.avg_relaxation.unwrap() - 62.0).abs() < 1e-9);

        let mut one_sided = summary(None, Some(1.0));
        one_sided.normalize_legacy_scores();
        assert_eq!(one_sided.avg_focus, None);
        assert_eq!(one_sided.avg_relaxation, Some(100.0));
    }

    #[test]
    fn new_scale_and_empty_scores_are_left_alone() {
        for (f, r) in [
            (Some(62.0), Some(71.0)),
            (Some(0.5), Some(71.0)),
            (None, None),
            (Some(0.0), Some(0.0)),
        ] {
            let mut s = summary(f, r);
            s.normalize_legacy_scores();
            assert_eq!((s.avg_focus, s.avg_relaxation), (f, r));
        }
    }

    #[test]
    fn legacy_session_file_body_normalizes() {
        // Old-format file body; list_sessions itself is covered in tests/data_dir_env.rs.
        let file = serde_json::json!({ "session": { "id": "a", "device_type": "muse", "start": "2026-01-01T00:00:00Z", "end": "2026-01-01T00:10:00Z", "duration_secs": 600, "data_points": 10, "file_path": "", "summary": { "avg_focus": 0.55, "avg_relaxation": 0.7, "avg_hr": null, "max_hr": null, "avg_hrv": null } }, "data": [] });
        let mut session: RecordedSession = serde_json::from_value(file["session"].clone()).unwrap();
        session.summary.normalize_legacy_scores();
        assert!((session.summary.avg_focus.unwrap() - 55.0).abs() < 1e-9);
        assert!((session.summary.avg_relaxation.unwrap() - 70.0).abs() < 1e-9);
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
