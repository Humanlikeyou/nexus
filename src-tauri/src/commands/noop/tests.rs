use super::*;
use chrono::TimeZone;
use std::fs;
use uuid::Uuid;

fn heart_rate_database(rows: &[(&str, i64, i64)]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("terrence-hr-{}.sqlite", Uuid::new_v4()));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE hrSample (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, bpm INTEGER NOT NULL,
             synced INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (deviceId, ts));",
    )
    .unwrap();
    for (device, ts, bpm) in rows {
        conn.execute(
            "INSERT INTO hrSample (deviceId, ts, bpm) VALUES (?1, ?2, ?3)",
            params![device, ts, bpm],
        )
        .unwrap();
    }
    path
}

#[test]
fn live_heart_rate_takes_the_newest_sample_for_this_device() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let at = now.timestamp();
    let path = heart_rate_database(&[
        ("my-whoop", at - 30, 61),
        ("my-whoop", at - 10, 64),
        ("other", at - 1, 99),
    ]);
    let reading = latest_heart_rate(&path, "my-whoop", now).unwrap().unwrap();
    assert_eq!((reading.bpm, reading.at), (64, at - 10));
    let expected: Vec<_> = reading.trace.iter().map(|s| s.bpm).collect();
    assert_eq!(expected, vec![61, 64]);
    let _ = fs::remove_file(path);
}

#[test]
fn live_heart_rate_hides_stale_future_and_implausible_samples() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let at = now.timestamp();
    for (ts, bpm) in [
        (at - LIVE_HR_MAX_AGE_SECS - 1, 64),
        (at + LIVE_HR_MAX_FUTURE_SECS + 1, 64),
        (at - 5, 0),
        (at - 5, 300),
    ] {
        let path = heart_rate_database(&[("my-whoop", ts, bpm)]);
        assert_eq!(latest_heart_rate(&path, "my-whoop", now).unwrap(), None);
        let _ = fs::remove_file(path);
    }
}

#[test]
fn live_heart_rate_skips_a_future_dated_row() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let at = now.timestamp();
    let path = heart_rate_database(&[
        ("my-whoop", at - 5, 66),
        ("my-whoop", at + 1_000_000_000, 70),
    ]);
    let reading = latest_heart_rate(&path, "my-whoop", now).unwrap();
    assert_eq!(reading.map(|r| r.bpm), Some(66));
    let _ = fs::remove_file(path);
}

#[test]
fn live_heart_rate_trace_is_oldest_first_bounded_and_skips_off_wrist_samples() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let at = now.timestamp();
    let mut rows: Vec<(&str, i64, i64)> = (0..400).map(|i| ("my-whoop", at - i, 70)).collect();
    rows[3].2 = 0;
    rows.push(("my-whoop", at - 3_000, 90));
    let path = heart_rate_database(&rows);
    let trace = latest_heart_rate(&path, "my-whoop", now)
        .unwrap()
        .unwrap()
        .trace;
    assert_eq!(trace.len() as i64, LIVE_HR_TRACE_ROWS - 1);
    assert!(trace.windows(2).all(|pair| pair[0].at < pair[1].at));
    assert!(trace.iter().all(|sample| sample.bpm == 70));
    let _ = fs::remove_file(path);
}

#[test]
fn live_heart_rate_trace_window_is_seconds_not_rows() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let at = now.timestamp();
    // One sample every 2 s: the row cap alone would reach 288 s back.
    let rows: Vec<(&str, i64, i64)> = (0..200).map(|i| ("my-whoop", at - 2 * i, 70)).collect();
    let path = heart_rate_database(&rows);
    let trace = latest_heart_rate(&path, "my-whoop", now)
        .unwrap()
        .unwrap()
        .trace;
    let span = trace.last().unwrap().at - trace.first().unwrap().at;
    assert!(span <= LIVE_HR_TRACE_WINDOW_SECS, "span {span}s");
    assert_eq!(trace.len() as i64, LIVE_HR_TRACE_WINDOW_SECS / 2 + 1);
    let _ = fs::remove_file(path);
}

#[test]
fn live_heart_rate_is_none_when_there_are_no_samples() {
    let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
    let path = heart_rate_database(&[]);
    assert_eq!(latest_heart_rate(&path, "my-whoop", now).unwrap(), None);
    let _ = fs::remove_file(path);
}

fn seeded_database() -> PathBuf {
    let path = std::env::temp_dir().join(format!("terrence-noop-{}.sqlite", Uuid::new_v4()));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
            "
            CREATE TABLE grdb_migrations(identifier TEXT);
            CREATE TABLE dailyMetric (
                deviceId TEXT NOT NULL, day TEXT NOT NULL, totalSleepMin REAL, efficiency REAL,
                deepMin REAL, remMin REAL, lightMin REAL, restingHr INTEGER, avgHrv REAL,
                recovery REAL, strain REAL, spo2Pct REAL, skinTempDevC REAL, respRateBpm REAL
            );
            CREATE TABLE sleepSession (
                deviceId TEXT NOT NULL, startTs INTEGER NOT NULL, endTs INTEGER NOT NULL,
                efficiency REAL, restingHr INTEGER, avgHrv REAL, stagesJSON TEXT
            );
            CREATE TABLE battery (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, soc REAL, charging INTEGER);
            INSERT INTO dailyMetric VALUES
              ('my-whoop-noop', '2026-08-24', 410, .89, 80, 95, 235, 56, 54, 70, 9, 97, .1, 14.2),
              ('my-whoop',      '2026-08-24', 430, .91, 85, 100, 245, 54, 59, 75, 8, 98, .0, 14.1),
              ('my-whoop',      '2026-08-25', 395, .86, 70, 90, 235, 58, 48, 63, 11, 97, .2, 14.4),
              ('my-whoop',      '2026-08-26', 425, .90, 82, 104, 239, 55, 62, 78, 7, 98, -.1, 14.0);
            INSERT INTO sleepSession VALUES ('my-whoop', 1787670000, 1787695500, .90, 55, 62, NULL);
            INSERT INTO battery VALUES ('my-whoop', 1787700000, 14, 0);
            ",
        )
        .unwrap();
    path
}

#[test]
fn reads_noop_metrics_without_creating_a_parallel_store() {
    let path = seeded_database();
    let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert_eq!(snapshot.status, "ready");
    assert_eq!(snapshot.source, "noop");
    assert_eq!(snapshot.latest_day.as_deref(), Some("2026-08-26"));
    assert_eq!(snapshot.recent_days.len(), 3);
    assert_eq!(snapshot.recent_days[0].total_sleep_min, Some(430.0));
    assert_eq!(
        snapshot.today.as_ref().and_then(|day| day.recovery),
        Some(78.0)
    );
    assert_eq!(
        snapshot.battery.as_ref().map(|battery| battery.percentage),
        Some(14.0)
    );
    assert_eq!(
        snapshot
            .battery
            .as_ref()
            .and_then(|battery| battery.charging),
        Some(false)
    );
    assert!(snapshot
        .insights
        .iter()
        .any(|insight| insight.metric == "recovery"));
    assert!(path.exists());
    let _ = fs::remove_file(path);
}

#[test]
fn marks_old_noop_data_stale_without_calling_it_current() {
    let path = seeded_database();
    let now = Utc.with_ymd_and_hms(2026, 8, 31, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert_eq!(snapshot.status, "stale");
    assert!(snapshot.message.unwrap().contains("more than 2 days old"));
    let _ = fs::remove_file(path);
}

#[test]
fn waits_for_a_real_overnight_record_before_marking_noop_ready() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
            "UPDATE dailyMetric SET totalSleepMin = NULL, recovery = NULL, avgHrv = NULL, restingHr = NULL WHERE day = '2026-08-26'",
            [],
        )
        .unwrap();
    conn.execute("DELETE FROM sleepSession", []).unwrap();
    drop(conn);

    let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert_eq!(snapshot.status, "waiting_for_nightly_data");
    assert!(snapshot.message.unwrap().contains("overnight sleep record"));
    let _ = fs::remove_file(path);
}

#[test]
fn bridges_adjacent_noop_sleep_segments_for_the_displayed_night() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM sleepSession", []).unwrap();
    conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', 1788611871, 1788627827, .94, 77, 264, ?1)",
            params![r#"[{"start":1788611871,"end":1788626931,"stage":"light"},{"start":1788626931,"end":1788627827,"stage":"wake"}]"#],
        )
        .unwrap();
    conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', 1788628977, 1788651087, .93, 68, 344, ?1)",
            params![r#"[{"start":1788628977,"end":1788640000,"stage":"light"},{"start":1788640000,"end":1788643600,"stage":"deep"},{"start":1788643600,"end":1788649577,"stage":"rem"},{"start":1788649577,"end":1788651087,"stage":"wake"}]"#],
        )
        .unwrap();
    drop(conn);

    let now = Utc.with_ymd_and_hms(2026, 9, 6, 1, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
    assert!(brief.contains("9h 54m asleep across 2 sessions"));
    assert!(brief
        .contains("Latest recorded sleep-segment signals: HRV 344 ms, resting heart rate 68 bpm"));
    assert!(
        brief.contains("Merged sleep stages: light 7h 15m, deep 1h 00m, REM 1h 40m, awake 0h 40m")
    );
    let sleep = snapshot.latest_sleep.as_ref().unwrap();
    assert_eq!(sleep.started_at, timestamp_iso(1788611871));
    assert_eq!(sleep.ended_at, timestamp_iso(1788651087));
    assert!((sleep.duration_min - 594.33).abs() < 0.01);
    assert_eq!(sleep.session_count, 2);
    assert!((sleep.light_min.unwrap() - 434.72).abs() < 0.01);
    assert_eq!(sleep.deep_min, Some(60.0));
    assert!((sleep.rem_min.unwrap() - 99.62).abs() < 0.01);
    assert!((sleep.awake_min.unwrap() - 40.10).abs() < 0.01);
    let _ = fs::remove_file(path);
}

#[test]
fn keeps_a_later_daytime_nap_separate_from_the_primary_overnight_sleep() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM sleepSession", []).unwrap();
    // Overnight is judged by the Mac's local hour, so the fixture is written in local time.
    let local = |day, hour, minute| {
        Local
            .with_ymd_and_hms(2026, 9, day, hour, minute, 0)
            .unwrap()
            .timestamp()
    };
    let night_start = local(6, 23, 30);
    let night_end = local(7, 7, 30);
    let nap_start = local(7, 13, 30);
    let nap_end = local(7, 17, 0);
    conn.execute(
        "INSERT INTO sleepSession VALUES ('my-whoop-noop', ?1, ?2, .91, 61, 82, NULL)",
        params![night_start, night_end],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sleepSession VALUES ('my-whoop-noop', ?1, ?2, .88, 64, 73, NULL)",
        params![nap_start, nap_end],
    )
    .unwrap();
    conn.execute(
            "INSERT INTO dailyMetric VALUES ('my-whoop-noop', '2026-09-07', 480, .91, 60, 90, 330, 61, 82, NULL, 8, NULL, NULL, 14)",
            [],
        )
        .unwrap();
    drop(conn);

    let now = DateTime::<Utc>::from_timestamp(nap_end + 2 * 60 * 60, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    let latest = snapshot.latest_sleep.as_ref().unwrap();
    let night = snapshot.latest_night.as_ref().unwrap();
    assert_eq!(latest.started_at, timestamp_iso(nap_start));
    assert_eq!(latest.ended_at, timestamp_iso(nap_end));
    assert_eq!(night.started_at, timestamp_iso(night_start));
    assert_eq!(night.ended_at, timestamp_iso(night_end));
    assert_eq!(night.duration_min, 480.0);
    let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
    assert!(brief.contains("Latest NOOP overnight sleep: 8h 00m"));
    assert!(brief.contains("Latest additional sleep session: 3h 30m"));
    let _ = fs::remove_file(path);
}

#[test]
fn decodes_imported_sleep_stage_minute_dictionary() {
    let stages =
        decode_sleep_stage_minutes(Some(r#"{"awake":22.5,"light":245,"deep":71.5,"rem":96}"#))
            .unwrap();
    assert_eq!(stages.awake, 22.5);
    assert_eq!(stages.light, 245.0);
    assert_eq!(stages.deep, 71.5);
    assert_eq!(stages.rem, 96.0);
    assert_eq!(stages.asleep(), 412.5);
}

#[test]
fn current_battery_outranks_future_dated_history() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO battery VALUES ('my-whoop', 1787700300, 69, NULL)",
        [],
    )
    .unwrap();
    drop(conn);

    let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert_eq!(
        snapshot.battery.as_ref().map(|battery| battery.percentage),
        Some(14.0)
    );
    let _ = fs::remove_file(path);
}

#[test]
fn rejects_a_future_only_noop_battery_timestamp() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM battery", []).unwrap();
    conn.execute(
        "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
        [],
    )
    .unwrap();
    drop(conn);

    let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert!(snapshot.battery.is_none());
    let _ = fs::remove_file(path);
}

#[test]
fn accepts_a_future_battery_timestamp_after_a_matching_local_confirmation() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM battery", []).unwrap();
    conn.execute(
        "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
        [],
    )
    .unwrap();
    drop(conn);

    let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
    let confirmation_path = std::env::temp_dir().join(format!(
        "terrence-noop-confirmation-{}.json",
        Uuid::new_v4()
    ));
    fs::write(
            &confirmation_path,
            format!(
                r#"{{"battery_clock_probe":{{"record_key":"1800000000:9.0:not_charging","confirmed_at":"{}"}}}}"#,
                now.to_rfc3339()
            ),
        )
        .unwrap();

    let snapshot =
        snapshot_from_database(&path, "my-whoop".to_string(), Some(&confirmation_path), now)
            .unwrap();
    assert_eq!(
        snapshot.battery.as_ref().map(|battery| battery.percentage),
        Some(9.0)
    );
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(confirmation_path);
}

/// A fake home with the given stores: (kind, latest day or None for an
/// empty dailyMetric, or "garbage" for a non-SQLite file).
fn fake_home(stores: &[(NoopStoreKind, Option<&str>)]) -> NoopPaths {
    let home = std::env::temp_dir().join(format!("nexus-noop-home-{}", Uuid::new_v4()));
    let paths = NoopPaths {
        explicit: None,
        home,
    };
    for (kind, day) in stores {
        let path = paths
            .candidates()
            .into_iter()
            .find(|(k, _)| k == kind)
            .unwrap()
            .1;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if *day == Some("garbage") {
            fs::write(&path, "not sqlite").unwrap();
            continue;
        }
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE dailyMetric (deviceId TEXT, day TEXT);")
            .unwrap();
        if let Some(day) = day {
            conn.execute(
                "INSERT INTO dailyMetric VALUES ('my-whoop-noop', ?1)",
                params![day],
            )
            .unwrap();
        }
    }
    paths
}

fn set_mtime(path: &Path, secs_ago: u64) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn path_of(paths: &NoopPaths, kind: NoopStoreKind) -> PathBuf {
    paths
        .candidates()
        .into_iter()
        .find(|(k, _)| *k == kind)
        .unwrap()
        .1
}

#[test]
fn picks_the_store_with_the_newest_day_over_the_container() {
    // The real layout on 2026-10-28: a stale container and a live app-support store.
    let paths = fake_home(&[
        (NoopStoreKind::Container, Some("2026-09-20")),
        (NoopStoreKind::AppSupport, Some("2026-10-28")),
    ]);
    // The stale store being touched more recently must not matter.
    set_mtime(&path_of(&paths, NoopStoreKind::AppSupport), 3600);
    let (kind, path) = paths.resolve("my-whoop").unwrap();
    assert_eq!(kind, NoopStoreKind::AppSupport);
    assert_eq!(path, path_of(&paths, NoopStoreKind::AppSupport));
    let _ = fs::remove_dir_all(&paths.home);
}

#[test]
fn equal_days_fall_back_to_the_newer_db_or_wal_mtime() {
    let paths = fake_home(&[
        (NoopStoreKind::Container, Some("2026-10-28")),
        (NoopStoreKind::AppSupport, Some("2026-10-28")),
    ]);
    let container = path_of(&paths, NoopStoreKind::Container);
    let app = path_of(&paths, NoopStoreKind::AppSupport);
    set_mtime(&container, 7200);
    set_mtime(&app, 3600);
    assert_eq!(
        paths.resolve("my-whoop").unwrap().0,
        NoopStoreKind::AppSupport
    );
    // A fresh -wal makes the container newer even though its main file is older.
    let mut wal = container.as_os_str().to_owned();
    wal.push("-wal");
    fs::write(PathBuf::from(wal), "").unwrap();
    assert_eq!(
        paths.resolve("my-whoop").unwrap().0,
        NoopStoreKind::Container
    );
    let _ = fs::remove_dir_all(&paths.home);
}

#[test]
fn an_unreadable_or_empty_store_loses_to_a_readable_one() {
    let paths = fake_home(&[
        (NoopStoreKind::Container, Some("garbage")),
        (NoopStoreKind::AppSupport, None),
    ]);
    set_mtime(&path_of(&paths, NoopStoreKind::AppSupport), 7200);
    // Both have no day; the newer mtime (the garbage file) wins the tie...
    assert_eq!(
        paths.resolve("my-whoop").unwrap().0,
        NoopStoreKind::Container
    );
    let paths2 = fake_home(&[
        (NoopStoreKind::Container, Some("garbage")),
        (NoopStoreKind::AppSupport, Some("2026-01-01")),
    ]);
    // ...but any real day beats it.
    assert_eq!(
        paths2.resolve("my-whoop").unwrap().0,
        NoopStoreKind::AppSupport
    );
    let _ = fs::remove_dir_all(&paths.home);
    let _ = fs::remove_dir_all(&paths2.home);
}

#[test]
fn explicit_path_overrides_and_staging_is_never_a_candidate() {
    let paths = fake_home(&[
        (NoopStoreKind::Container, Some("2026-09-20")),
        (NoopStoreKind::AppSupport, Some("2026-10-28")),
    ]);
    let staging = paths.home.join(
            "Library/Containers/com.noopapp.noop.staging/Data/Library/Application Support/OpenWhoop/whoop.sqlite",
        );
    assert!(paths
        .candidates()
        .iter()
        .all(|(_, p)| !p.starts_with(staging.parent().unwrap())));
    assert!(paths
        .candidates()
        .iter()
        .all(|(_, p)| !p.to_string_lossy().contains(".staging")));
    let explicit = NoopPaths {
        explicit: Some(path_of(&paths, NoopStoreKind::Container)),
        home: paths.home.clone(),
    };
    let (kind, path) = explicit.resolve("my-whoop").unwrap();
    assert_eq!(kind, NoopStoreKind::Explicit);
    assert_eq!(path, path_of(&paths, NoopStoreKind::Container));
    let _ = fs::remove_dir_all(&paths.home);
}

#[test]
fn reading_day_skips_empty_rows_for_at_most_two_days() {
    let d = |day: &str| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok();
    fn rows<'a>(
        pairs: &'a [(&'static str, bool)],
    ) -> impl Iterator<Item = (&'static str, bool)> + 'a {
        pairs.iter().copied()
    }
    let filled = ("2026-11-03", true);
    let empty = |day| (day, false);

    assert_eq!(
        reading_day(rows(&[filled, empty("2026-11-04")]), d("2026-11-04")).as_deref(),
        Some("2026-11-03")
    );
    // Exactly two days old counts, three does not (mindBody.test.ts readingDay).
    let two = [filled, empty("2026-11-04"), empty("2026-11-05")];
    assert_eq!(
        reading_day(rows(&two), d("2026-11-05")).as_deref(),
        Some("2026-11-03")
    );
    let three = [filled, empty("2026-11-05"), empty("2026-11-06")];
    assert_eq!(reading_day(rows(&three), d("2026-11-06")), None);
    // The real day counts even when no row exists for it yet.
    assert_eq!(reading_day(rows(&[filled]), d("2026-11-06")), None);
    // Unknown current day: the rows alone decide, as in the frontend.
    assert_eq!(
        reading_day(rows(&[filled]), None).as_deref(),
        Some("2026-11-03")
    );
    // A clock behind the rows cannot make an old reading look fresh.
    assert_eq!(reading_day(rows(&three), d("2026-11-01")), None);
    assert_eq!(reading_day(rows(&[empty("2026-11-05")]), None), None);
    assert_eq!(reading_day(rows(&[]), None), None);
    assert_eq!(reading_day(rows(&[("not-a-day", true)]), None), None);
}

#[test]
fn snapshot_and_brief_label_the_reading_day_not_the_empty_row() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO dailyMetric (deviceId, day) VALUES ('my-whoop', '2026-08-27')",
        [],
    )
    .unwrap();
    drop(conn);
    let now = Utc.with_ymd_and_hms(2026, 8, 27, 12, 0, 0).unwrap();
    let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
    assert_eq!(snapshot.latest_day.as_deref(), Some("2026-08-27"));
    assert_eq!(snapshot.reading_day.as_deref(), Some("2026-08-26"));
    let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
    assert!(brief.contains("latest daily data 2026-08-26"));
    let _ = fs::remove_file(path);
}

#[test]
fn data_two_days_old_is_not_stale_but_three_is() {
    let path = seeded_database();
    let two = Utc.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap();
    let three = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
    let s2 = snapshot_from_database(&path, "my-whoop".to_string(), None, two).unwrap();
    let s3 = snapshot_from_database(&path, "my-whoop".to_string(), None, three).unwrap();
    assert_ne!(s2.status, "stale");
    assert_eq!(s3.status, "stale");
    let _ = fs::remove_file(path);
}

/// Live smoke against the real NOOP store (read-only). Prints only row
/// counts, latestDay, the chosen candidate kind and elapsed ms — never
/// health values. Run:
/// cargo test --manifest-path src-tauri/Cargo.toml noop_live -- --ignored --nocapture
#[test]
#[ignore = "reads the real NOOP store"]
fn noop_live_smoke() {
    let started = std::time::Instant::now();
    let snapshot = health_snapshot();
    let elapsed = started.elapsed().as_millis();
    let device_id = env::var("NOOP_DEVICE_ID").unwrap_or_else(|_| DEFAULT_DEVICE_ID.to_string());
    let (kind, path) = NoopPaths::from_environment().resolve(&device_id).unwrap();
    let conn = open_store(&path).unwrap();
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(-1)
    };
    println!(
            "noop_live_smoke: kind={} dailyMetric={} sleepSession={} battery={} recentDays={} latestDay={} status={} elapsed_ms={}",
            kind.label(),
            count("dailyMetric"),
            count("sleepSession"),
            count("battery"),
            snapshot.recent_days.len(),
            snapshot.latest_day.as_deref().unwrap_or("none"),
            snapshot.status,
            elapsed
        );
    let latest =
        NaiveDate::parse_from_str(snapshot.latest_day.as_deref().unwrap(), "%Y-%m-%d").unwrap();
    let age = (Local::now().date_naive() - latest).num_days();
    assert!(age <= 2, "latestDay is {age} days old");
    assert!(elapsed < 500, "snapshot took {elapsed} ms");
}

#[test]
fn rejects_a_foreign_database_without_writing_to_it() {
    let path =
        std::env::temp_dir().join(format!("terrence-noop-foreign-{}.sqlite", Uuid::new_v4()));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE dailyMetric (deviceId TEXT, day TEXT);")
        .unwrap();
    drop(conn);
    let result = snapshot_from_database(&path, "my-whoop".to_string(), None, Utc::now());
    assert!(result.unwrap_err().contains("migration metadata"));
    assert!(path.exists());
    let _ = fs::remove_file(path);
}

fn workout_db(zones: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("terrence-wk-{}.sqlite", Uuid::new_v4()));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
            "CREATE TABLE workout (deviceId TEXT NOT NULL, startTs INTEGER NOT NULL, endTs INTEGER NOT NULL,
               sport TEXT NOT NULL, source TEXT NOT NULL, durationS REAL, energyKcal REAL, avgHr INTEGER,
               maxHr INTEGER, strain REAL, distanceM REAL, zonesJSON TEXT, notes TEXT, steps INTEGER);",
        )
        .unwrap();
    let now = Utc
        .with_ymd_and_hms(2026, 9, 10, 12, 0, 0)
        .unwrap()
        .timestamp();
    for (dev, ago_days, sport) in [
        ("my-whoop", 1, "run"),
        ("my-whoop-noop", 1, "run"),
        ("my-whoop", 3, "lift"),
        ("my-whoop", 40, "old"),
    ] {
        conn.execute(
                "INSERT INTO workout VALUES (?1, ?2, ?3, ?4, 'x', 1800, 300, 140, 170, 9.5, NULL, ?5, 'private note', NULL)",
                params![dev, now - ago_days * 86_400, now - ago_days * 86_400 + 1800, sport, zones],
            )
            .unwrap();
    }
    path
}

#[test]
fn workouts_are_windowed_deduped_and_zone_minutes_parsed() {
    let path = workout_db(r#"{"z1":60,"z2":120,"z3":0,"z4":600,"z5":"x"}"#);
    let now = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let rows = recent_workouts_in(&path, "my-whoop", 14, now).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].sport, "run");
    assert_eq!(rows[1].sport, "lift");
    assert_eq!(rows[0].zone_minutes, Some(vec![1.0, 2.0, 0.0, 10.0, 0.0]));
    assert_eq!(rows[0].duration_min, Some(30.0));
    // Over-large day counts are capped at 90.
    assert_eq!(
        recent_workouts_in(&path, "my-whoop", 9999, now)
            .unwrap()
            .len(),
        3
    );
    // notes never reach the IPC type.
    let json = serde_json::to_string(&rows).unwrap();
    assert!(!json.contains("notes") && !json.contains("private"));
    let _ = fs::remove_file(path);
}

#[test]
fn zones_decode_rejects_other_shapes() {
    assert_eq!(decode_zone_minutes(None), None);
    assert_eq!(decode_zone_minutes(Some("[1,2]")), None);
    assert_eq!(decode_zone_minutes(Some("not json")), None);
    assert_eq!(decode_zone_minutes(Some(r#"{"z1":0}"#)), None);
}

#[test]
fn missing_workout_table_is_an_empty_list() {
    let path = seeded_database();
    let rows = recent_workouts_in(&path, "my-whoop", 7, Utc::now()).unwrap();
    assert!(rows.is_empty());
    let _ = fs::remove_file(path);
}

#[test]
fn hypnogram_is_sorted_clipped_normalised_and_null_without_segments() {
    let path = seeded_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM sleepSession", []).unwrap();
    let start = 1_788_611_871_i64;
    let end = start + 3_000;
    let stages = format!(
        r#"[{{"start":{b},"end":{c},"stage":"deep"}},
                {{"start":{a},"end":{b},"stage":"awake"}},
                {{"start":{c},"end":{x},"stage":"REM"}},
                {{"start":{c},"end":{c},"stage":"light"}},
                {{"start":{a},"end":{c},"stage":"bogus"}}]"#,
        a = start - 600,
        b = start + 1_000,
        c = start + 2_000,
        x = end + 900
    );
    conn.execute(
        "INSERT INTO sleepSession VALUES ('my-whoop', ?1, ?2, .9, 55, 62, ?3)",
        params![start, end, stages],
    )
    .unwrap();
    let night = hypnogram_in(&path, "my-whoop", None).unwrap().unwrap();
    let got: Vec<(i64, i64, &str)> = night
        .segments
        .iter()
        .map(|s| (s.start, s.end, s.stage.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (start, start + 1_000, "awake"),
            (start + 1_000, start + 2_000, "deep"),
            (start + 2_000, end, "rem"),
        ]
    );
    let by_day = hypnogram_in(&path, "my-whoop", Some(&night.day)).unwrap();
    assert_eq!(by_day.map(|h| h.segments.len()), Some(3));
    assert_eq!(
        hypnogram_in(&path, "my-whoop", Some("2001-01-01")).unwrap(),
        None
    );
    conn.execute("UPDATE sleepSession SET stagesJSON = '{\"deep\":30}'", [])
        .unwrap();
    assert_eq!(hypnogram_in(&path, "my-whoop", None).unwrap(), None);
    let _ = fs::remove_file(path);
}

#[test]
fn core_counts_as_light_in_the_totals_as_in_the_hypnogram() {
    // Before decision 0057 the totals dropped "core" while the hypnogram drew it as light.
    let seg = decode_sleep_stage_minutes(Some(
        r#"[{"start":0,"end":3600,"stage":"core"},{"start":3600,"end":4800,"stage":"Deep"}]"#,
    ))
    .unwrap();
    assert_eq!((seg.light, seg.deep), (60.0, 20.0));
    let dict = decode_sleep_stage_minutes(Some(r#"{"core":200,"deep":50}"#)).unwrap();
    assert_eq!(dict.light, 200.0);
}
