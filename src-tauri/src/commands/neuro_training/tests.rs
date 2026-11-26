use super::*;
use crate::commands::local_store::tests::temp_dir;
use serde_json::json;

pub(crate) fn progress(valid: f64, trained: f64, scales: &[f64]) -> Value {
    let mut log = vec![
        json!({ "at_s": 0, "event": "baseline", "window_reward_rate": null, "scale_before": 1, "scale_after": 1, "reason": "set from the eyes-open baseline", "reward_min": { "Alpha": 33.0 }, "inhibit_max": { "High Beta": 9.0 } }),
    ];
    let mut before = 1.0;
    for (i, s) in scales.iter().enumerate() {
        log.push(json!({ "at_s": 30 * (i + 1), "event": "adapt", "window_reward_rate": 0.7, "scale_before": before, "scale_after": s, "reason": "x", "reward_min": { "Alpha": 33.0 * s }, "inhibit_max": {} }));
        before = *s;
    }
    json!({ "version": "progress-v1-0059", "protocol_set": "protocols-v1-0059", "protocol_id": "calm", "method": "training-v2-0057",
        "valid_pct": valid, "trained_seconds": trained, "time_in_target_pct": 58.0, "target_reward_rate": 0.6,
        "held_seconds": { "blink": 4.5, "not_live": 0 }, "threshold_log": log })
}

pub(crate) fn write_session(dir: &Path, id: &str, start: &str, source: &str, nf: Value) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let body = json!({
        "session": { "id": id, "device_type": "muse", "start": start, "end": start, "duration_secs": 600, "data_points": 10, "file_path": "x.ndjson",
            "summary": { "avg_focus": 50.0, "avg_relaxation": 60.0, "avg_hr": null, "max_hr": null, "avg_hrv": null, "score_scale": 100 } },
        "data": { "data_source": source, "neurofeedback": nf }
    });
    let path = dir.join(format!("muse_{id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
    path
}

#[test]
fn reads_a_progress_record_and_derives_traceable_numbers() {
    let dir = temp_dir("neuro-training").join("sessions");
    write_session(
        &dir,
        "a",
        "2026-11-20T10:00:00Z",
        "live",
        json!({ "progress_record": progress(92.0, 540.0, &[1.04, 1.0816]) }),
    );
    let r = records_in(&dir);
    assert_eq!(r.len(), 1);
    let r = &r[0];
    assert_eq!(r.protocol_id, "calm");
    assert_eq!(r.method, "training-v2-0057");
    assert_eq!(r.threshold_checks, 2);
    assert_eq!(r.final_threshold_scale, Some(1.0816));
    assert_eq!(
        r.baseline_levels["Alpha"], 30.0,
        "baseline reward threshold / 1.1"
    );
    assert_eq!(r.held_seconds["blink"], 4.5);
    assert!(r.eligible, "{:?}", r.not_eligible_reason);
}

#[test]
fn eligibility_needs_live_data_enough_valid_time_and_length() {
    let dir = temp_dir("neuro-training").join("sessions");
    write_session(
        &dir,
        "replay",
        "2026-11-20T10:00:00Z",
        "replay",
        json!({ "progress_record": progress(95.0, 600.0, &[]) }),
    );
    write_session(
        &dir,
        "noisy",
        "2026-11-21T10:00:00Z",
        "live",
        json!({ "progress_record": progress(55.0, 600.0, &[]) }),
    );
    write_session(
        &dir,
        "short",
        "2026-11-22T10:00:00Z",
        "live",
        json!({ "progress_record": progress(95.0, 120.0, &[]) }),
    );
    let r = records_in(&dir);
    let why: BTreeMap<_, _> = r
        .iter()
        .map(|x| {
            (
                x.session_id.as_str(),
                x.not_eligible_reason.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert!(r.iter().all(|x| !x.eligible));
    assert!(why["replay"].contains("live headset"));
    assert!(why["noisy"].contains("70%"));
    assert!(why["short"].contains("3 minutes"));
}

#[test]
fn sessions_without_a_progress_record_are_left_out_and_files_are_untouched() {
    let dir = temp_dir("neuro-training").join("sessions");
    let old = write_session(
        &dir,
        "old",
        "2026-10-01T10:00:00Z",
        "live",
        json!({ "protocol_id": "calm" }),
    );
    let new = write_session(
        &dir,
        "new",
        "2026-11-23T10:00:00Z",
        "live",
        json!({ "progress_record": progress(90.0, 600.0, &[]) }),
    );
    let (b_old, b_new) = (std::fs::read(&old).unwrap(), std::fs::read(&new).unwrap());
    let r = records_in(&dir);
    assert_eq!(
        r.iter().map(|x| x.session_id.as_str()).collect::<Vec<_>>(),
        ["new"]
    );
    assert_eq!(std::fs::read(&old).unwrap(), b_old);
    assert_eq!(std::fs::read(&new).unwrap(), b_new);
}

#[test]
fn hostile_values_are_ignored_not_trusted() {
    let dir = temp_dir("neuro-training").join("sessions");
    let mut p = progress(90.0, 600.0, &[]);
    p["valid_pct"] = json!("lots");
    p["protocol_id"] = json!("x".repeat(5000));
    write_session(
        &dir,
        "h",
        "2026-11-23T10:00:00Z",
        "live",
        json!({ "progress_record": p }),
    );
    let r = &records_in(&dir)[0];
    assert_eq!(r.valid_pct, None);
    assert!(!r.eligible);
    assert_eq!(r.protocol_id, "");
}

#[test]
fn a_missing_sessions_folder_is_empty_not_an_error() {
    assert!(records_in(&temp_dir("neuro-training").join("nope")).is_empty());
}
