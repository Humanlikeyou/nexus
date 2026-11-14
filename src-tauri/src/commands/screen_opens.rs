//! Local screen-open counter: which top-level screen was opened and when (screen id and
//! timestamp only, no content). One read command returns counts per screen per JST week.

use super::shared::sqlite::open_private_wal;
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, jst, now_ms};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Same ids as SYSTEMS in src-console/app/routes.ts.
const SCREENS: [&str; 7] = [
    "terrance",
    "life",
    "mindbody",
    "enterprise",
    "chat",
    "smarthome",
    "settings",
];
const MAX_ROWS: i64 = 500_000;
const DEFAULT_WEEKS: u32 = 12;
const MAX_WEEKS: u32 = 104;
const DB_FILE: &str = "screen_opens.db";

const CREATE_SQL: &str = "CREATE TABLE IF NOT EXISTS screen_opens (id INTEGER PRIMARY KEY, screen TEXT NOT NULL, at_ms INTEGER NOT NULL)";
const INDEX_SQL: &str = "CREATE INDEX IF NOT EXISTS idx_screen_opens_at ON screen_opens(at_ms)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScreenWeekCount {
    /// "YYYY-MM-DD", the Monday of the JST week.
    pub week_start: String,
    pub screen: String,
    pub count: u32,
}

fn store_err<E>(_: E) -> String {
    "store_error".to_string()
}

fn open_in(dir: &Path) -> Result<Connection, String> {
    let conn = open_private_wal(&dir.join(DB_FILE)).map_err(store_err)?;
    conn.execute(CREATE_SQL, []).map_err(store_err)?;
    conn.execute(INDEX_SQL, []).map_err(store_err)?;
    Ok(conn)
}

fn record_in(dir: &Path, screen: &str, now: u64) -> Result<(), String> {
    if !SCREENS.contains(&screen) {
        return Err("screen_unknown".to_string());
    }
    let conn = open_in(dir)?;
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM screen_opens", [], |r| r.get(0))
        .map_err(store_err)?;
    if rows >= MAX_ROWS {
        return Err("store_full".to_string());
    }
    conn.execute(
        "INSERT INTO screen_opens (screen, at_ms) VALUES (?1, ?2)",
        params![screen, now as i64],
    )
    .map_err(store_err)?;
    Ok(())
}

fn week_start_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn counts_in(dir: &Path, weeks: Option<u32>, now: u64) -> Result<Vec<ScreenWeekCount>, String> {
    let weeks = weeks.unwrap_or(DEFAULT_WEEKS).clamp(1, MAX_WEEKS);
    let now_dt: DateTime<Utc> = Utc
        .timestamp_millis_opt(now as i64)
        .single()
        .ok_or_else(|| "store_error".to_string())?;
    let first = week_start_of(day_key(&now_dt)) - Duration::days(7 * (weeks as i64 - 1));
    let from_ms = first
        .and_hms_opt(0, 0, 0)
        .and_then(|n| jst().from_local_datetime(&n).single())
        .ok_or_else(|| "store_error".to_string())?
        .timestamp_millis();
    let conn = open_in(dir)?;
    let mut stmt = conn
        .prepare("SELECT screen, at_ms FROM screen_opens WHERE at_ms >= ?1 AND at_ms <= ?2")
        .map_err(store_err)?;
    let rows = stmt
        .query_map(params![from_ms, now as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(store_err)?;
    let mut buckets: BTreeMap<(NaiveDate, String), u32> = BTreeMap::new();
    for row in rows {
        let (screen, at_ms) = row.map_err(store_err)?;
        let Some(t) = Utc.timestamp_millis_opt(at_ms).single() else {
            continue;
        };
        let wk = week_start_of(day_key(&t));
        *buckets.entry((wk, screen)).or_insert(0) += 1;
    }
    Ok(buckets
        .into_iter()
        .map(|((wk, screen), count)| ScreenWeekCount {
            week_start: wk.format("%Y-%m-%d").to_string(),
            screen,
            count,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn screen_open_record(screen: String) -> Result<(), String> {
    if !SCREENS.contains(&screen.as_str()) {
        return Err("screen_unknown".to_string());
    }
    blocking(JoinErr::Code("store_error"), move || {
        record_in(&crate::app_paths::data_dir(), &screen, now_ms())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn screen_open_counts(weeks: Option<u32>) -> Result<Vec<ScreenWeekCount>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        counts_in(&crate::app_paths::data_dir(), weeks, now_ms())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("amara-screens-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ms(rfc: &str) -> u64 {
        DateTime::parse_from_rfc3339(rfc)
            .unwrap()
            .timestamp_millis() as u64
    }

    // 2026-11-14 is a Saturday; its JST week starts Monday 2026-11-09.
    const NOW: &str = "2026-11-14T03:00:00Z";

    fn row(w: &str, s: &str, c: u32) -> ScreenWeekCount {
        ScreenWeekCount {
            week_start: w.into(),
            screen: s.into(),
            count: c,
        }
    }

    #[test]
    fn unknown_empty_oversized_and_case_variants_are_refused_and_nothing_written() {
        let dir = temp_dir("refuse");
        let long = "a".repeat(10_000);
        for bad in [
            "",
            "nope",
            "Chat",
            "CHAT",
            " chat",
            "chat ",
            "chat\0",
            long.as_str(),
        ] {
            assert_eq!(
                record_in(&dir, bad, ms(NOW)),
                Err("screen_unknown".to_string())
            );
        }
        assert!(!dir.join(DB_FILE).exists(), "db must not even be opened");
        assert!(counts_in(&dir, None, ms(NOW)).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn record_then_counts() {
        let dir = temp_dir("count");
        for s in ["chat", "chat", "life"] {
            record_in(&dir, s, ms(NOW)).unwrap();
        }
        assert_eq!(
            counts_in(&dir, None, ms(NOW)).unwrap(),
            vec![row("2026-11-09", "chat", 2), row("2026-11-09", "life", 1)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_row_after_now_is_not_counted() {
        // A clock set back after a write must not show a future week.
        let dir = temp_dir("future");
        record_in(&dir, "chat", ms(NOW)).unwrap();
        record_in(&dir, "life", ms("2026-11-20T03:00:00Z")).unwrap();
        assert_eq!(
            counts_in(&dir, None, ms(NOW)).unwrap(),
            vec![row("2026-11-09", "chat", 1)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn week_boundary_is_monday_midnight_jst() {
        let dir = temp_dir("boundary");
        // Sunday 23:59:59 JST and Monday 00:00:00 JST (= 15:00Z Sunday).
        record_in(&dir, "chat", ms("2026-11-08T14:59:59Z")).unwrap();
        record_in(&dir, "chat", ms("2026-11-08T15:00:00Z")).unwrap();
        assert_eq!(
            counts_in(&dir, Some(2), ms(NOW)).unwrap(),
            vec![row("2026-11-02", "chat", 1), row("2026-11-09", "chat", 1)]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn weeks_default_twelve_and_clamp() {
        let dir = temp_dir("weeks");
        let now = ms(NOW);
        let week = 7 * 24 * 3600 * 1000u64;
        // Current week (Mon 2026-11-09) minus 11 weeks = 2026-08-24 is the oldest included.
        record_in(&dir, "chat", ms("2026-08-23T14:59:59Z")).unwrap(); // Sun 23:59 JST, 2026-08-23: out
        record_in(&dir, "life", ms("2026-08-23T15:00:00Z")).unwrap(); // Mon 2026-08-24: in
        let d = counts_in(&dir, None, now).unwrap();
        assert_eq!(d, vec![row("2026-08-24", "life", 1)]);
        assert_eq!(counts_in(&dir, Some(12), now).unwrap(), d);
        // 0 clamps to 1: only the current week, so nothing here.
        assert!(counts_in(&dir, Some(0), now).unwrap().is_empty());
        // Huge clamps to 104: includes both rows (about 1 year back is within 104 weeks).
        assert_eq!(counts_in(&dir, Some(u32::MAX), now).unwrap().len(), 2);
        // 104 weeks vs 105 behave the same.
        record_in(&dir, "settings", now.saturating_sub(103 * week)).unwrap();
        assert_eq!(
            counts_in(&dir, Some(104), now).unwrap(),
            counts_in(&dir, Some(500), now).unwrap()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sorted_by_week_then_screen_and_zero_rows_omitted() {
        let dir = temp_dir("sort");
        record_in(&dir, "settings", ms("2026-11-10T00:00:00Z")).unwrap();
        record_in(&dir, "chat", ms("2026-11-03T00:00:00Z")).unwrap();
        record_in(&dir, "terrance", ms("2026-11-10T00:00:00Z")).unwrap();
        record_in(&dir, "life", ms("2026-11-10T00:00:00Z")).unwrap();
        let got = counts_in(&dir, Some(3), ms(NOW)).unwrap();
        assert_eq!(
            got,
            vec![
                row("2026-11-02", "chat", 1),
                row("2026-11-09", "life", 1),
                row("2026-11-09", "settings", 1),
                row("2026-11-09", "terrance", 1),
            ]
        );
        assert!(got.iter().all(|r| r.count > 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn store_full_refuses_without_inserting() {
        let dir = temp_dir("full");
        record_in(&dir, "chat", ms(NOW)).unwrap();
        {
            let conn = open_in(&dir).unwrap();
            conn.execute(
                "INSERT INTO screen_opens (screen, at_ms) \
                 WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < ?1) \
                 SELECT 'chat', 0 FROM n",
                params![MAX_ROWS - 1],
            )
            .unwrap();
        }
        assert_eq!(
            record_in(&dir, "chat", ms(NOW)),
            Err("store_full".to_string())
        );
        let conn = open_in(&dir).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM screen_opens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, MAX_ROWS);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn db_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perm");
        record_in(&dir, "chat", ms(NOW)).unwrap();
        let m = std::fs::metadata(dir.join(DB_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn source_pin_stores_only_screen_and_at_ms() {
        assert_eq!(
            CREATE_SQL,
            "CREATE TABLE IF NOT EXISTS screen_opens (id INTEGER PRIMARY KEY, screen TEXT NOT NULL, at_ms INTEGER NOT NULL)"
        );
        let dir = temp_dir("cols");
        let conn = open_in(&dir).unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('screen_opens') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(cols, ["id", "screen", "at_ms"]);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
