use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::noop_query::{self, NoopSettings};

/// Trailing days for the session-start NOOP health_snapshot.
const BODY_CONTEXT_DAYS: i64 = 7;
const BODY_CONTEXT_TIMEOUT: Duration = Duration::from_secs(10);
/// Snapshot captured at the latest session start, consumed by the next save_session.
static PENDING_BODY_CONTEXT: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Best-effort health_snapshot for a session. None unless WhoopSource=noop and a binary is
/// configured; None on any error. Logs status and error kind only, never values.
pub(crate) fn capture_body_context_with(
    settings: &NoopSettings,
    timeout: Duration,
) -> Option<serde_json::Value> {
    if settings.whoop_ble_allowed() {
        return None;
    }
    let bin = settings.binary_path.as_deref().filter(|p| !p.is_empty())?;
    let args = serde_json::json!({ "days": BODY_CONTEXT_DAYS });
    match noop_query::run_query(
        Path::new(bin),
        settings.db_path.as_deref(),
        "health_snapshot",
        &args,
        timeout,
    ) {
        Ok(v) => {
            println!("[sessions] body_context: status=ok");
            Some(v)
        }
        Err(e) => {
            println!("[sessions] body_context: status=error kind={}", e.kind());
            None
        }
    }
}

/// Called at Muse session start. Spawns a background capture; never blocks or fails the caller.
pub fn begin_body_context_capture() {
    if let Ok(mut slot) = PENDING_BODY_CONTEXT.lock() {
        *slot = None;
    }
    std::thread::spawn(|| {
        let v = capture_body_context_with(&noop_query::load_settings(), BODY_CONTEXT_TIMEOUT);
        if let (Some(v), Ok(mut slot)) = (v, PENDING_BODY_CONTEXT.lock()) {
            *slot = Some(v);
        }
    });
}

fn take_pending_body_context() -> Option<serde_json::Value> {
    PENDING_BODY_CONTEXT.lock().ok().and_then(|mut s| s.take())
}

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
    /// NOOP health_snapshot taken at Muse session start. Absent in older session files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_context: Option<serde_json::Value>,
}

#[tauri::command]
pub fn save_session(
    mut session: RecordedSession,
    data: serde_json::Value,
) -> Result<String, String> {
    let dir = sessions_dir();
    crate::app_paths::create_private_dir_all(&dir)
        .map_err(|e| format!("Failed to create sessions dir: {}", e))?;

    super::path_safety::validate_name(&session.device_type)?;
    super::path_safety::validate_name(&session.id)?;
    let filename = format!("{}_{}.json", session.device_type, session.id);
    let path = super::path_safety::safe_join(&dir, &filename)?;

    if session.body_context.is_none() {
        session.body_context = take_pending_body_context();
    }
    let payload = serde_json::json!({
        "session": session,
        "data": data,
    });

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    crate::app_paths::write_private(&path, json)
        .map_err(|e| format!("Failed to write session: {}", e))?;

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

    #[test]
    fn body_context_is_optional_and_round_trips() {
        let old = serde_json::json!({ "id": "a", "device_type": "muse", "start": "s", "end": "e", "duration_secs": 1, "data_points": 0, "file_path": "", "summary": { "avg_focus": null, "avg_relaxation": null, "avg_hr": null, "max_hr": null, "avg_hrv": null } });
        let s: RecordedSession = serde_json::from_value(old).unwrap();
        assert!(s.body_context.is_none());
        assert!(serde_json::to_value(&s)
            .unwrap()
            .get("body_context")
            .is_none());
        let mut v = serde_json::to_value(&s).unwrap();
        v["body_context"] = serde_json::json!({ "status": "ok" });
        let s2: RecordedSession = serde_json::from_value(v).unwrap();
        assert_eq!(s2.body_context.unwrap()["status"], "ok");
    }

    #[cfg(unix)]
    fn fake_noop(body: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "bodyctx-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("fake-noop");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        (d, p)
    }

    #[cfg(unix)]
    #[test]
    fn body_context_capture_is_best_effort() {
        use super::noop_query::WhoopSource;
        let ok = r#"cat >/dev/null; echo '{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"status":"ok"}}}'"#;
        let (d, p) = fake_noop(ok);
        let mut s = NoopSettings {
            binary_path: Some(p.display().to_string()),
            ..Default::default()
        };
        assert_eq!(
            capture_body_context_with(&s, Duration::from_secs(5)).unwrap()["status"],
            "ok"
        );
        s.whoop_source = WhoopSource::Ble;
        assert!(capture_body_context_with(&s, Duration::from_secs(5)).is_none());
        assert!(
            capture_body_context_with(&NoopSettings::default(), Duration::from_secs(5)).is_none()
        );
        let (d2, slow) = fake_noop("sleep 30");
        let s2 = NoopSettings {
            binary_path: Some(slow.display().to_string()),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        assert!(capture_body_context_with(&s2, Duration::from_millis(300)).is_none());
        assert!(t.elapsed() < Duration::from_secs(5));
        let (d3, bad) = fake_noop("cat >/dev/null; echo nope");
        let s3 = NoopSettings {
            binary_path: Some(bad.display().to_string()),
            ..Default::default()
        };
        assert!(capture_body_context_with(&s3, Duration::from_secs(5)).is_none());
        for d in [d, d2, d3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
