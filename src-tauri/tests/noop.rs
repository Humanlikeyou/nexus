//! noop: health_snapshot() against a small fixture SQLite store via
//! NOOP_DB_PATH. The store is opened read-only and is left byte-for-byte unchanged.
use std::fs;

use nexus_lib::test_support::health_snapshot;

mod common;

#[test]
fn health_snapshot_reads_a_fixture_store_read_only() {
    let dir = common::tmp("noop");
    let db = dir.join("whoop.sqlite");
    let today = chrono::Local::now().date_naive();
    let day = |offset: i64| {
        (today - chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string()
    };
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE grdb_migrations(identifier TEXT);
             CREATE TABLE dailyMetric (
                deviceId TEXT NOT NULL, day TEXT NOT NULL, totalSleepMin REAL, efficiency REAL,
                deepMin REAL, remMin REAL, lightMin REAL, restingHr INTEGER, avgHrv REAL,
                recovery REAL, strain REAL, spo2Pct REAL, skinTempDevC REAL, respRateBpm REAL,
                PRIMARY KEY (deviceId, day));
             CREATE TABLE sleepSession (deviceId TEXT NOT NULL, startTs INTEGER NOT NULL, endTs INTEGER NOT NULL,
                efficiency REAL, restingHr INTEGER, avgHrv REAL, stagesJSON TEXT, PRIMARY KEY (deviceId, startTs));
             CREATE TABLE battery (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, soc REAL, charging INTEGER, PRIMARY KEY (deviceId, ts));",
        )
        .unwrap();
        for (i, d) in [day(2), day(1), day(0)].iter().enumerate() {
            conn.execute(
                "INSERT INTO dailyMetric VALUES ('my-whoop-noop', ?1, 420, .9, 80, 95, 240, 55, 60, ?2, 10, 97, .1, 14.2)",
                rusqlite::params![d, 60.0 + i as f64],
            )
            .unwrap();
        }
    }
    let before = fs::read(&db).unwrap();

    // Only test in this binary: env is process-wide.
    std::env::set_var("NOOP_DB_PATH", &db);
    std::env::set_var("NOOP_DEVICE_ID", "my-whoop");
    let snapshot = health_snapshot();
    std::env::remove_var("NOOP_DB_PATH");
    std::env::remove_var("NOOP_DEVICE_ID");

    assert_eq!(snapshot.source, "noop");
    assert_eq!(snapshot.latest_day.as_deref(), Some(day(0).as_str()));
    assert_eq!(snapshot.recent_days.len(), 3);
    assert_ne!(snapshot.status, "stale");
    assert_ne!(snapshot.status, "unavailable");
    assert_eq!(fs::read(&db).unwrap(), before, "store must not be modified");
    let _ = fs::remove_dir_all(dir);
}
