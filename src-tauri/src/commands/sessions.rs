use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::noop_query::{self, NoopSettings};

/// Trailing days for the session-start NOOP health_snapshot.
const BODY_CONTEXT_DAYS: i64 = 7;
const BODY_CONTEXT_TIMEOUT: Duration = Duration::from_secs(10);
/// Snapshot captured at the latest session start, consumed by the next save_session.
static PENDING_BODY_CONTEXT: Mutex<Option<BodyContext>> = Mutex::new(None);

/// Minimised NOOP context stored on a session. Explicit whitelist: everything else in the
/// health_snapshot (sources, freshness, appleDaily, battery, recentDays, spo2, skin temp,
/// steps, ...) is dropped.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BodyContext {
    #[serde(default)]
    pub today: Option<BodyToday>,
    #[serde(default)]
    pub latest_night: Option<BodyNight>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BodyToday {
    #[serde(default)]
    pub recovery: Option<f64>,
    #[serde(default)]
    pub hrv: Option<f64>,
    #[serde(default)]
    pub rhr: Option<f64>,
    #[serde(default)]
    pub strain: Option<f64>,
    #[serde(default)]
    pub sleep_total_min: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BodyNight {
    #[serde(default)]
    pub day: Option<String>,
    #[serde(default)]
    pub total_sleep_min: Option<f64>,
    #[serde(default)]
    pub efficiency: Option<f64>,
    #[serde(default)]
    pub deep_min: Option<f64>,
    #[serde(default)]
    pub rem_min: Option<f64>,
    #[serde(default)]
    pub light_min: Option<f64>,
}

impl BodyContext {
    /// Build from a NOOP health_snapshot payload (camelCase keys, see LocalAccessCore.swift).
    pub fn from_snapshot(v: &serde_json::Value) -> Self {
        let num = |o: &serde_json::Value, k: &str| o.get(k).and_then(serde_json::Value::as_f64);
        let today = v.get("today").filter(|t| t.is_object()).map(|t| BodyToday {
            recovery: num(t, "recovery"),
            hrv: num(t, "avgHrv"),
            rhr: num(t, "restingHr"),
            strain: num(t, "strain"),
            sleep_total_min: num(t, "totalSleepMin"),
        });
        let latest_night = v
            .get("latestNight")
            .filter(|n| n.is_object())
            .map(|n| BodyNight {
                day: n
                    .get("day")
                    .and_then(|d| d.as_str())
                    .map(|d| d.chars().take(10).collect()),
                total_sleep_min: num(n, "totalSleepMin"),
                efficiency: num(n, "efficiency"),
                deep_min: num(n, "deepMin"),
                rem_min: num(n, "remMin"),
                light_min: num(n, "lightMin"),
            });
        BodyContext {
            today,
            latest_night,
        }
    }
}

/// Best-effort health_snapshot for a session. None unless WhoopSource=noop and a binary is
/// configured; None on any error. Logs status and error kind only, never values.
pub(crate) fn capture_body_context_with(
    settings: &NoopSettings,
    roots: &noop_query::Roots,
    timeout: Duration,
) -> Option<BodyContext> {
    if settings.whoop_ble_allowed() {
        return None;
    }
    settings.binary_path.as_deref().filter(|p| !p.is_empty())?;
    let args = serde_json::json!({ "days": BODY_CONTEXT_DAYS });
    match noop_query::run_query(settings, roots, "health_snapshot", &args, timeout) {
        Ok(v) => {
            println!("[sessions] body_context: status=ok");
            Some(BodyContext::from_snapshot(&v))
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
        let Ok(roots) = noop_query::Roots::current() else {
            return;
        };
        let v =
            capture_body_context_with(&noop_query::load_settings(), &roots, BODY_CONTEXT_TIMEOUT);
        if let (Some(v), Ok(mut slot)) = (v, PENDING_BODY_CONTEXT.lock()) {
            *slot = Some(v);
        }
    });
}

fn take_pending_body_context() -> Option<BodyContext> {
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
    pub body_context: Option<BodyContext>,
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
        v["body_context"] = serde_json::json!({ "today": { "hrv": 1.23 } });
        let s2: RecordedSession = serde_json::from_value(v).unwrap();
        assert_eq!(s2.body_context.unwrap().today.unwrap().hrv, Some(1.23));
    }

    #[test]
    fn body_context_whitelists_fields() {
        let snap = serde_json::json!({
            "generatedAt": "2026-01-01T00:00:00Z", "sources": {"a": "b"},
            "freshness": {"dailyRows": 1}, "recentDays": [{"day": "2026-01-01"}],
            "appleDaily": [{"steps": 0}], "battery": {"percentage": 1.23},
            "latestSleep": {"durationMin": 1.0},
            "today": {"day": "2026-01-01", "source": "my-whoop", "recovery": 1.0, "avgHrv": 1.23,
                      "restingHr": 0, "strain": 1.23, "totalSleepMin": 1.23, "spo2Pct": 1.23,
                      "skinTempDevC": 0.0, "steps": 0, "respRateBpm": 1.23, "activeKcalEst": 1.23},
            "latestNight": {"day": "2026-01-01", "startTs": 0, "endTs": 60, "sessionCount": 1,
                            "totalSleepMin": 1.23, "inBedMin": 2.46, "efficiency": 0.5,
                            "deepMin": 0.0, "remMin": 0.0, "lightMin": 1.23}
        });
        let bc = BodyContext::from_snapshot(&snap);
        let v = serde_json::to_value(&bc).unwrap();
        let keys = |o: &serde_json::Value| {
            let mut k: Vec<String> = o.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&v), ["latest_night", "today"]);
        assert_eq!(
            keys(&v["today"]),
            ["hrv", "recovery", "rhr", "sleep_total_min", "strain"]
        );
        assert_eq!(
            keys(&v["latest_night"]),
            [
                "day",
                "deep_min",
                "efficiency",
                "light_min",
                "rem_min",
                "total_sleep_min"
            ]
        );
        assert_eq!(bc.today.unwrap().hrv, Some(1.23));
        let empty =
            BodyContext::from_snapshot(&serde_json::json!({"today": null, "latestNight": null}));
        assert_eq!(empty, BodyContext::default());
    }

    #[test]
    fn body_context_capture_is_best_effort() {
        use super::noop_query::test_util::{s as sp, FakeHome};
        use super::noop_query::WhoopSource;
        let h = FakeHome::new();
        let r = h.roots();
        let ok = h.script(r#"cat >/dev/null; echo '{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"today":{"avgHrv":1.23,"spo2Pct":1.0},"battery":{}}}}'"#);
        let mut s = NoopSettings {
            binary_path: sp(&ok),
            ..Default::default()
        };
        let bc = capture_body_context_with(&s, &r, Duration::from_secs(5)).unwrap();
        assert_eq!(bc.today.unwrap().hrv, Some(1.23));
        s.whoop_source = WhoopSource::Ble;
        assert!(capture_body_context_with(&s, &r, Duration::from_secs(5)).is_none());
        assert!(
            capture_body_context_with(&NoopSettings::default(), &r, Duration::from_secs(5))
                .is_none()
        );
        let h2 = FakeHome::new();
        let slow = h2.script("sleep 30");
        let s2 = NoopSettings {
            binary_path: sp(&slow),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        assert!(capture_body_context_with(&s2, &h2.roots(), Duration::from_millis(300)).is_none());
        assert!(t.elapsed() < Duration::from_secs(5));
        let h3 = FakeHome::new();
        let bad = h3.script("cat >/dev/null; echo nope");
        let s3 = NoopSettings {
            binary_path: sp(&bad),
            ..Default::default()
        };
        assert!(capture_body_context_with(&s3, &h3.roots(), Duration::from_secs(5)).is_none());
    }
}
