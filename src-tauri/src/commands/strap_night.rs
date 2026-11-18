//! "Heart-rate-only night" check for the Needs-you list (decision 0034).
//!
//! After a strap power loss NOOP can stay on the plain heart-rate link, so the night has heart rate
//! rows but no motion, temperature or breathing rows, and recovery cannot be scored. This module
//! opens the NOOP store READ-ONLY (the same `open_store` as Home: read-only flags, busy timeout) and
//! COUNTS rows in a fixed window. It never reads a heart-rate, motion or metric value; the only
//! numbers it handles are row counts and a "recovery is NULL" flag. Never writes, checkpoints or copies.
//!
//! Window: the night ends at 10:00 JST (most nights are over by then; a late riser at 09:00 still has
//! a full night) and starts 14 hours earlier, 20:00 the evening before (covers an early bedtime). Before
//! 10:00 the previous night's window is used, so the pointer stays until the next morning's night
//! has arrived, then clears itself.

use super::noop::{has_column, open_store, resolve_store_path, table_names};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone};
use rusqlite::Connection;
use std::path::Path;

/// Night ends at this hour JST.
pub const NIGHT_END_HOUR: u32 = 10;
/// Night window length in hours (20:00 to 10:00).
pub const NIGHT_HOURS: i64 = 14;
/// Heart-rate rows that count as "substantial": one hour at one row a second.
pub const MIN_HR_ROWS: i64 = 3600;
/// Motion coverage under this share (percent) of the heart-rate rows counts as missing.
pub const MOTION_MIN_PERCENT: i64 = 5;

pub const TITLE: &str = "Last night came through as heart rate only";
pub const DETAIL: &str = "Motion and temperature weren't recorded, so recovery can't be scored. Open NOOP and check the strap is fully synced (after a dead battery, sync once before sleep).";
pub const DETAIL_NO_RECOVERY: &str = "Motion and temperature weren't recorded, so recovery can't be scored, and today has no recovery yet because of it. Open NOOP and check the strap is fully synced (after a dead battery, sync once before sleep).";

/// Row counts for the night window. Counts only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NightCounts {
    pub hr: i64,
    pub motion: i64,
    pub skin_temp: i64,
    pub resp: i64,
}

/// What the check found. No health values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrapNight {
    pub hr_only: bool,
    /// Today's dailyMetric row exists and its recovery is NULL.
    pub recovery_missing: bool,
}

/// The night window (start, end) as unix seconds plus the day it ends on, for `now` in JST.
pub fn night_window(now: DateTime<FixedOffset>) -> (i64, i64, NaiveDate) {
    let tz = *now.offset();
    let mut day = now.date_naive();
    let end_today = tz
        .from_local_datetime(&day.and_hms_opt(NIGHT_END_HOUR, 0, 0).unwrap())
        .unwrap();
    if now < end_today {
        day -= Duration::days(1);
    }
    let end = tz
        .from_local_datetime(&day.and_hms_opt(NIGHT_END_HOUR, 0, 0).unwrap())
        .unwrap();
    let start = end - Duration::hours(NIGHT_HOURS);
    (start.timestamp(), end.timestamp(), day)
}

/// Heart-rate rows are substantial and motion is missing (under 5% of heart-rate coverage).
pub fn is_hr_only(c: NightCounts) -> bool {
    c.hr >= MIN_HR_ROWS && c.motion * 100 < c.hr * MOTION_MIN_PERCENT
}

fn count(conn: &Connection, table: &str, start: i64, end: i64) -> Result<i64, String> {
    // `table` is one of this module's constants, never input.
    conn.query_row(
        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE ts >= ?1 AND ts < ?2"),
        [start, end],
        |r| r.get::<_, i64>(0),
    )
    .map_err(|_| "strap night count failed".to_string())
}

/// `Ok(None)` when the store lacks the heart-rate or motion table or their `ts` column (unwired).
/// `Err` when the store can't be opened or read (failed).
pub fn read_in(path: &Path, now: DateTime<FixedOffset>) -> Result<Option<StrapNight>, String> {
    let conn = open_store(path).map_err(|_| "strap night store did not open".to_string())?;
    let tables = table_names(&conn).map_err(|_| "strap night store unreadable".to_string())?;
    let ts_ok = |t: &str| -> Result<bool, String> {
        if !tables.contains(t) {
            return Ok(false);
        }
        has_column(&conn, t, "ts")
    };
    if !ts_ok("hrSample")? || !ts_ok("gravitySample")? {
        return Ok(None);
    }
    let (start, end, day) = night_window(now);
    let mut counts = NightCounts {
        hr: count(&conn, "hrSample", start, end)?,
        motion: count(&conn, "gravitySample", start, end)?,
        ..Default::default()
    };
    if ts_ok("skinTempSample")? {
        counts.skin_temp = count(&conn, "skinTempSample", start, end)?;
    }
    if ts_ok("respSample")? {
        counts.resp = count(&conn, "respSample", start, end)?;
    }
    let hr_only = is_hr_only(counts);
    let recovery_missing = hr_only
        && tables.contains("dailyMetric")
        && has_column(&conn, "dailyMetric", "day")?
        && has_column(&conn, "dailyMetric", "recovery")?
        && conn
            .query_row(
                "SELECT COUNT(*) FROM dailyMetric WHERE day = ?1 AND recovery IS NULL",
                [day.format("%Y-%m-%d").to_string()],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|_| "strap night recovery check failed".to_string())?
            > 0;
    Ok(Some(StrapNight {
        hr_only,
        recovery_missing,
    }))
}

/// Resolve the NOOP store the way Home does. No store yet is unwired, not failed.
pub fn read(now: DateTime<FixedOffset>) -> Result<Option<StrapNight>, String> {
    match resolve_store_path() {
        Err(_) => Ok(None),
        Ok(path) => read_in(&path, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::shared::time::jst;
    use uuid::Uuid;

    fn at(day: u32, h: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, 11, day, h, 0, 0).unwrap()
    }

    /// A temp store with the NOOP column shapes; `hr`/`motion` rows land inside the night ending
    /// 2026-11-18 10:00 JST, one second apart.
    fn store(hr: i64, motion: i64, today_recovery_null: bool, full: bool) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("strap-night-{}.sqlite", Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE hrSample (deviceId TEXT, ts INTEGER NOT NULL, bpm INTEGER);
             CREATE TABLE gravitySample (deviceId TEXT, ts INTEGER NOT NULL, x DOUBLE);
             CREATE TABLE dailyMetric (deviceId TEXT, day TEXT, recovery REAL);",
        )
        .unwrap();
        if full {
            conn.execute_batch(
                "CREATE TABLE skinTempSample (deviceId TEXT, ts INTEGER NOT NULL, raw INTEGER);
                 CREATE TABLE respSample (deviceId TEXT, ts INTEGER NOT NULL, raw INTEGER);",
            )
            .unwrap();
        }
        let (start, _, _) = night_window(at(18, 12));
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..hr {
            tx.execute(
                "INSERT INTO hrSample VALUES ('d', ?1, 60)",
                [start + 3600 + i],
            )
            .unwrap();
        }
        for i in 0..motion {
            tx.execute(
                "INSERT INTO gravitySample VALUES ('d', ?1, 0.0)",
                [start + 3600 + i],
            )
            .unwrap();
        }
        tx.execute(
            "INSERT INTO dailyMetric VALUES ('d', '2026-11-18', ?1)",
            [if today_recovery_null {
                None
            } else {
                Some(70.0)
            }],
        )
        .unwrap();
        tx.commit().unwrap();
        path
    }

    #[test]
    fn window_is_20_to_10_jst_and_rolls_at_ten() {
        let (s, e, d) = night_window(at(18, 12));
        assert_eq!(e - s, 14 * 3600);
        assert_eq!(d.to_string(), "2026-11-18");
        assert_eq!(e, at(18, 10).timestamp());
        assert_eq!(s, at(17, 20).timestamp());
        assert_eq!(night_window(at(18, 9)).2.to_string(), "2026-11-17");
        assert_eq!(night_window(at(18, 10)).2.to_string(), "2026-11-18");
    }

    #[test]
    fn hr_only_fixture_raises_and_explains_missing_recovery() {
        let p = store(7200, 0, true, true);
        let r = read_in(&p, at(18, 15)).unwrap().unwrap();
        assert_eq!(
            r,
            StrapNight {
                hr_only: true,
                recovery_missing: true
            }
        );
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn hr_only_with_recovery_present_still_raises_without_the_recovery_note() {
        let p = store(7200, 100, false, true);
        let r = read_in(&p, at(18, 15)).unwrap().unwrap();
        assert!(r.hr_only && !r.recovery_missing);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_normal_night_does_not_raise() {
        let p = store(7200, 7000, false, true);
        assert!(!read_in(&p, at(18, 15)).unwrap().unwrap().hr_only);
        let _ = std::fs::remove_file(p);
        // Recovery missing alone (motion present) is not this warning.
        let p = store(7200, 7000, true, true);
        let r = read_in(&p, at(18, 15)).unwrap().unwrap();
        assert!(!r.hr_only && !r.recovery_missing);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn thresholds() {
        let c = |hr, motion| NightCounts {
            hr,
            motion,
            ..Default::default()
        };
        assert!(!is_hr_only(c(3599, 0)));
        assert!(is_hr_only(c(3600, 0)));
        assert!(is_hr_only(c(10_000, 499)));
        assert!(!is_hr_only(c(10_000, 500)));
    }

    #[test]
    fn next_morning_with_no_hr_only_rows_clears() {
        let p = store(7200, 0, true, true);
        assert!(read_in(&p, at(18, 15)).unwrap().unwrap().hr_only);
        assert!(!read_in(&p, at(19, 11)).unwrap().unwrap().hr_only);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn missing_tables_columns_or_store_are_unwired_never_a_crash() {
        let p = store(7200, 0, true, false);
        assert!(read_in(&p, at(18, 15)).unwrap().unwrap().hr_only);
        let _ = std::fs::remove_file(p);
        let path = std::env::temp_dir().join(format!("strap-night-{}.sqlite", Uuid::new_v4()));
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE hrSample (deviceId TEXT, ts INTEGER);")
            .unwrap();
        assert_eq!(read_in(&path, at(18, 15)), Ok(None));
        let _ = std::fs::remove_file(&path);
        let path = std::env::temp_dir().join(format!("strap-night-{}.sqlite", Uuid::new_v4()));
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE hrSample (deviceId TEXT, t INTEGER); CREATE TABLE gravitySample (ts INTEGER);",
            )
            .unwrap();
        assert_eq!(read_in(&path, at(18, 15)), Ok(None));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_store_that_cannot_open_or_read_fails() {
        let missing = std::env::temp_dir().join(format!("nope-{}.sqlite", Uuid::new_v4()));
        assert!(read_in(&missing, at(18, 15)).is_err());
        let junk = std::env::temp_dir().join(format!("strap-junk-{}.sqlite", Uuid::new_v4()));
        std::fs::write(
            &junk,
            b"this is not a sqlite database at all, just text padding..",
        )
        .unwrap();
        assert!(read_in(&junk, at(18, 15)).is_err());
        let _ = std::fs::remove_file(junk);
    }

    #[test]
    fn reading_leaves_the_store_byte_identical() {
        let p = store(4000, 0, true, true);
        let before = std::fs::read(&p).unwrap();
        read_in(&p, at(18, 15)).unwrap();
        assert_eq!(before, std::fs::read(&p).unwrap());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn source_is_read_only_counts_only_and_formats_no_values() {
        let src = include_str!("strap_night.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        assert!(body.contains("open_store("));
        for banned in [
            "Connection::open",
            "OPEN_READ_WRITE",
            "execute(",
            "execute_batch",
            "INSERT",
            "UPDATE ",
            "DELETE",
            "CREATE ",
            "VACUUM",
            "wal_checkpoint",
            "bpm",
            "SELECT ts",
            "println!",
            "eprintln!",
            "log::",
            "tracing::",
        ] {
            assert!(!body.contains(banned), "strap_night must not use {banned}");
        }
        for line in body.lines().filter(|l| l.contains("SELECT")) {
            assert!(line.contains("COUNT(*)"), "{line}");
        }
        for t in [TITLE, DETAIL, DETAIL_NO_RECOVERY] {
            assert!(!t.chars().any(|c| c.is_ascii_digit()), "{t}");
        }
    }
}
