//! `whoop_status`: READ-ONLY WHOOP ownership and freshness for Settings > Devices (handover 1111s step 5,
//! status half only). No takeover, no strap action, no Bluetooth, no process control, no writes.
//!
//! - `owner`:
//!   - `amara`  if the marker file `whoop-owner` in the app data folder says `amara` (written later by the
//!     takeover stage; this module only reads it, so today it is never `amara`);
//!   - `legacy` if the old health app's store is being written right now (database or its `-wal` modified
//!     within `LEGACY_WINDOW_SECS`), i.e. the other app is holding the strap;
//!   - `none`   otherwise (no store, or nothing wrote to it recently).
//!
//!   Legacy detection is deliberately file-based and read-only (no `ps`, no process list).
//! - `last_sample_at`: newest heart-rate sample time (epoch seconds), from the store opened read-only.
//! - `gaps_14d`: days (of the last 14, in 24 h buckets ending now) with no heart-rate samples.
//!   None if the store can't be read; never guessed.
//!
//! Never logs or returns a health value, only a timestamp and a count.

use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The other app counts as holding the strap if its store changed this recently.
pub const LEGACY_WINDOW_SECS: i64 = 10 * 60;
const OWNER_MARKER: &str = "whoop-owner";
const DAY: i64 = 86_400;
const GAP_DAYS: i64 = 14;
const BUSY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WhoopOwner {
    Amara,
    Legacy,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WhoopStatus {
    pub owner: WhoopOwner,
    pub store_found: bool,
    pub last_sample_at: Option<i64>,
    pub gaps_14d: Option<u32>,
}

fn marker_says_amara(data_dir: &Path) -> bool {
    std::fs::read_to_string(data_dir.join(OWNER_MARKER))
        .map(|s| s.trim() == "amara")
        .unwrap_or(false)
}

/// Newest mtime (epoch seconds) of the database or its -wal.
fn store_mtime_secs(db: &Path) -> Option<i64> {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    [db.to_path_buf(), std::path::PathBuf::from(wal)]
        .iter()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .max()
}

fn open_read_only(db: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY)?;
    Ok(conn)
}

fn read_facts(db: &Path, device_id: &str, now: i64) -> rusqlite::Result<(Option<i64>, u32)> {
    let conn = open_read_only(db)?;
    let last: Option<i64> = conn.query_row(
        "SELECT MAX(ts) FROM hrSample WHERE deviceId = ?1 AND ts <= ?2",
        params![device_id, now + 300],
        |r| r.get(0),
    )?;
    let from = now - GAP_DAYS * DAY;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT (ts - ?3) / 86400 FROM hrSample WHERE deviceId = ?1 AND ts >= ?3 AND ts <= ?2",
    )?;
    let days = stmt
        .query_map(params![device_id, now, from], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let covered = days.iter().filter(|d| (0..GAP_DAYS).contains(*d)).count() as i64;
    Ok((last, (GAP_DAYS - covered).max(0) as u32))
}

/// Pure over its inputs (tests pass a fixture folder, store and clock).
pub(crate) fn status_in(
    data_dir: &Path,
    store: Option<(String, std::path::PathBuf)>,
    now: i64,
) -> WhoopStatus {
    let Some((device_id, db)) = store else {
        return WhoopStatus {
            owner: if marker_says_amara(data_dir) {
                WhoopOwner::Amara
            } else {
                WhoopOwner::None
            },
            store_found: false,
            last_sample_at: None,
            gaps_14d: None,
        };
    };
    let owner = if marker_says_amara(data_dir) {
        WhoopOwner::Amara
    } else if store_mtime_secs(&db).is_some_and(|m| now - m <= LEGACY_WINDOW_SECS) {
        WhoopOwner::Legacy
    } else {
        WhoopOwner::None
    };
    let facts = read_facts(&db, &device_id, now).ok();
    WhoopStatus {
        owner,
        store_found: true,
        last_sample_at: facts.and_then(|f| f.0),
        gaps_14d: facts.map(|f| f.1),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn whoop_status() -> Result<WhoopStatus, String> {
    tokio::task::spawn_blocking(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        status_in(
            &crate::app_paths::data_dir(),
            super::noop::current_store(),
            now,
        )
    })
    .await
    .map_err(|_| "status_unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-whoop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn store(dir: &Path, samples: &[i64]) -> std::path::PathBuf {
        let db = dir.join("whoop.sqlite");
        let c = Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE hrSample (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, bpm INTEGER, PRIMARY KEY (deviceId, ts));").unwrap();
        for ts in samples {
            c.execute("INSERT INTO hrSample VALUES ('d', ?1, 60)", params![ts])
                .unwrap();
        }
        db
    }

    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn no_store_is_none_and_unknown_not_zero() {
        let d = tmp("nostore");
        let s = status_in(&d, None, now_secs());
        assert_eq!(
            s,
            WhoopStatus {
                owner: WhoopOwner::None,
                store_found: false,
                last_sample_at: None,
                gaps_14d: None
            }
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_store_written_just_now_means_legacy_holds_the_strap() {
        let d = tmp("legacy");
        let now = now_secs();
        let db = store(&d, &[now - 60, now - 3600]);
        let s = status_in(&d, Some(("d".into(), db)), now);
        assert_eq!(s.owner, WhoopOwner::Legacy);
        assert_eq!(s.last_sample_at, Some(now - 60));
        assert!(s.store_found);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_quiet_store_is_none_and_gaps_count_days_without_samples() {
        let d = tmp("quiet");
        let now = now_secs();
        // Samples on 3 distinct 24 h buckets of the last 14.
        let db = store(&d, &[now - 100, now - 2 * DAY - 100, now - 5 * DAY - 100]);
        // Look from far in the future of the file mtime: the store hasn't changed for > 10 min.
        let s = status_in(&d, Some(("d".into(), db)), now + LEGACY_WINDOW_SECS + 60);
        assert_eq!(s.owner, WhoopOwner::None);
        assert_eq!(s.gaps_14d, Some(11));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn marker_file_means_amara_and_nothing_else_does() {
        let d = tmp("marker");
        let now = now_secs();
        let db = store(&d, &[now - 10]);
        std::fs::write(d.join(OWNER_MARKER), "amara\n").unwrap();
        assert_eq!(
            status_in(&d, Some(("d".into(), db.clone())), now).owner,
            WhoopOwner::Amara
        );
        std::fs::write(d.join(OWNER_MARKER), "something else").unwrap();
        assert_eq!(
            status_in(&d, Some(("d".into(), db)), now).owner,
            WhoopOwner::Legacy
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unreadable_store_reports_unknown_facts_not_zeros() {
        let d = tmp("bad");
        let db = d.join("whoop.sqlite");
        std::fs::write(&db, b"not a database").unwrap();
        let s = status_in(&d, Some(("d".into(), db)), now_secs());
        assert!(s.store_found && s.gaps_14d.is_none() && s.last_sample_at.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_store_is_not_modified_by_a_status_read() {
        let d = tmp("ro");
        let now = now_secs();
        let db = store(&d, &[now - 10]);
        let before = std::fs::read(&db).unwrap();
        let _ = status_in(&d, Some(("d".into(), db.clone())), now);
        assert_eq!(std::fs::read(&db).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn owner_serialises_as_the_ui_words() {
        for (o, w) in [
            (WhoopOwner::Amara, "amara"),
            (WhoopOwner::Legacy, "legacy"),
            (WhoopOwner::None, "none"),
        ] {
            assert_eq!(serde_json::to_value(o).unwrap(), serde_json::json!(w));
        }
    }

    /// Source pin: this module never talks to the strap, starts a process, or writes anything.
    #[test]
    fn source_pin_no_ble_no_command_no_write() {
        let src = include_str!("whoop_status.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let code: String = code
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "btleplug",
            "ble_common",
            "Peripheral",
            "Command",
            "std::process",
            "tokio::process",
            "fs::write",
            "File::create",
            "OpenOptions",
            "remove_file",
            "remove_dir",
            "fs::rename",
            "fs::copy",
            "create_dir",
            ".execute(",
            "execute_batch",
            "SQLITE_OPEN_READ_WRITE",
            "SQLITE_OPEN_CREATE",
            "kill",
            "launchctl",
            "reqwest",
        ] {
            assert!(
                !code.contains(banned),
                "whoop_status.rs must not contain {banned}"
            );
        }
        assert!(code.contains("SQLITE_OPEN_READ_ONLY"));
    }
}
