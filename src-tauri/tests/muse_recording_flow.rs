//! End to end through the real commands, in a temp NEXUS_DATA_DIR (never the real app-support
//! folder): record every stream, mark aux, stop, read the file back, list it, and save a session.
//! One test function: the env var and the recorder are process-wide.
use nexus_lib::test_support::{
    list_sessions, mark_recording_aux, maybe_record_motion, maybe_record_ppg, maybe_record_sample,
    muse, recordings, save_session, start_eeg_recording, stop_eeg_recording, AuxInfo,
    RecordedSession, SessionSummary, StreamSet, DATA_DIR_ENV,
};
use std::time::Duration;

mod common;

fn lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn aux() -> AuxInfo {
    AuxInfo {
        detected: true,
        placement: "occipital".into(),
        detected_by: "signal-quality".into(),
        detected_at: Some("2026-11-11T00:00:00Z".into()),
    }
}

#[test]
fn recording_writes_lazy_header_all_streams_and_trailer() {
    let dir = common::tmp("muse-flow");
    std::env::set_var(DATA_DIR_ENV, &dir);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Nothing open yet.
        assert!(mark_recording_aux(aux()).await.is_err());

        muse::set_subscribed_streams(StreamSet {
            aux: true,
            ppg: vec!["infrared".into()],
            motion: vec!["gyroscope".into()],
        });
        let name = start_eeg_recording().await.unwrap();
        let path = dir.join("replays").join(&name);
        // Lazy: no header until the first line.
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);

        let eeg = |channel: &str| muse::EegSample {
            timestamp: 1000.0,
            channel: channel.into(),
            samples: vec![1.0; 12],
        };
        maybe_record_sample(&eeg("TP9")).await;
        maybe_record_sample(&eeg("AUX_L")).await; // never recorded
        maybe_record_sample(&eeg("AUX")).await;
        maybe_record_ppg(&muse::PpgSample {
            timestamp: 1001.0,
            channel: "infrared".into(),
            sequence: 7,
            samples: vec![1, 2, 3, 4, 5, 6],
        })
        .await;
        maybe_record_motion(&muse::MotionSample {
            timestamp: 1002.0,
            sensor: "gyroscope".into(),
            unit: "deg/s".into(),
            sequence: 9,
            samples: vec![[0.0, 1.0, 2.0]; 3],
        })
        .await;
        // The open file is listed as being recorded.
        let open = recordings::list_recordings().await.unwrap();
        assert_eq!(open[0].raw, "recording");

        assert!(mark_recording_aux(AuxInfo {
            placement: "frontal".into(),
            ..aux()
        })
        .await
        .is_err());
        mark_recording_aux(aux()).await.unwrap();
        stop_eeg_recording().await.unwrap();

        let l = lines(&path);
        let kinds: Vec<&str> = l.iter().map(|v| v["kind"].as_str().unwrap()).collect();
        assert_eq!(
            kinds,
            ["header", "sample", "sample", "ppg", "motion", "trailer"]
        );
        assert_eq!(l[0]["format"], 2);
        assert_eq!(
            l[0]["channels"],
            serde_json::json!(["TP9", "AF7", "AF8", "TP10", "AUX"])
        );
        assert_eq!(l[0]["ppg"], serde_json::json!(["infrared"]));
        assert_eq!(l[0]["motion"], serde_json::json!(["gyroscope"]));
        assert_eq!(l[1]["channel"], "TP9");
        assert_eq!(l[2]["channel"], "AUX");
        assert_eq!(l[3]["source"], "infrared");
        assert!(l[3].get("sequence").is_none() && l[4].get("sequence").is_none());
        assert_eq!(l[5]["aux"]["detected"], true);
        assert_eq!(l[5]["aux"]["placement"], "occipital");
        assert!(l[5]["end_ts"].as_f64().unwrap() > 1.0e12);

        // No open recording any more.
        assert!(mark_recording_aux(aux()).await.is_err());

        let items = recordings::list_recordings().await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw, "available");
        assert!(items[0].has_heart && items[0].has_motion);
        assert!(items[0].aux.as_ref().unwrap().detected);
        assert_eq!(items[0].retention, "kept");

        // A recording with no data still gets a header and a trailer.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        muse::set_subscribed_streams(StreamSet::default());
        let empty = start_eeg_recording().await.unwrap();
        stop_eeg_recording().await.unwrap();
        let l = lines(&dir.join("replays").join(&empty));
        assert_eq!(l.len(), 2);
        assert_eq!(
            (l[0]["kind"].as_str(), l[1]["kind"].as_str()),
            (Some("header"), Some("trailer"))
        );
        assert_eq!(l[0]["channels"].as_array().unwrap().len(), 4);
        assert_eq!(l[1]["aux"], serde_json::Value::Null);
    });

    // save_session writes score_scale 100, drops attached samples, and reads back unscaled.
    let session = |id: &str, start: &str, focus: f64| RecordedSession {
        id: id.into(),
        device_type: "muse".into(),
        start: start.into(),
        end: start.into(),
        duration_secs: 60,
        data_points: 1,
        file_path: String::new(),
        summary: SessionSummary {
            avg_focus: Some(focus),
            avg_relaxation: Some(0.5),
            avg_hr: None,
            max_hr: None,
            avg_hrv: None,
            calm_pct: None,
            time_in_target_pct: None,
            score_scale: None,
        },
        body_context: None,
        samples: None,
    };
    save_session(
        session("new1", "2026-11-11T00:00:00Z", 0.4),
        serde_json::json!({}),
    )
    .unwrap();
    let on_disk: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("sessions").join("muse_new1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(on_disk["session"]["summary"]["score_scale"], 100);
    assert!(on_disk["session"].get("samples").is_none());
    // A legacy file (no score_scale, 0-1 scores) alongside it.
    std::fs::write(
        dir.join("sessions").join("muse_old1.json"),
        serde_json::json!({"session": session("old1", "2026-02-18T00:00:00Z", 0.4), "data": []})
            .to_string()
            .replace(",\"score_scale\":100", ""),
    )
    .unwrap();
    let listed = list_sessions().unwrap();
    let get = |id: &str| {
        listed
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .summary
            .avg_focus
    };
    assert_eq!(get("new1"), Some(0.4), "score_scale 100: read as is");
    assert!(
        (get("old1").unwrap() - 40.0).abs() < 1e-9,
        "legacy 0-1 scaled on read"
    );
}
