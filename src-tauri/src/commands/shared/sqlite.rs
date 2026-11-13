//! One opener for the app's own sqlite stores (events.db, chat_history.db, terrence_core.db).
//!
//! It keeps what every copy did (private folder, 0600 file created before sqlite touches it, WAL,
//! then -wal/-shm restricted to 0600) and adds a busy timeout, so a second connection waits instead
//! of failing at once with "database is locked". The NOOP reader does NOT use this: it opens NOOP's
//! store read-only with its own settings (`noop.rs::open_store`).

use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

/// How long a write waits for another connection's lock before giving up.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// Which step failed. Callers turn this into the error text they always had.
#[derive(Debug)]
pub(crate) enum OpenStep {
    /// Creating the parent folder (also: the path has no parent).
    Dir(String),
    /// Creating the 0600 database file.
    File(String),
    /// sqlite open.
    Open(String),
    /// `journal_mode = WAL` or the busy timeout.
    Configure(String),
    /// Restricting the database, -wal and -shm files to 0600.
    Secure(String),
}

/// Open (creating if needed) a private WAL database at `path` with a busy timeout.
pub(crate) fn open_private_wal(path: &Path) -> Result<Connection, OpenStep> {
    let parent = path
        .parent()
        .ok_or_else(|| OpenStep::Dir("database has no parent directory".to_string()))?;
    crate::app_paths::create_private_dir_all(parent).map_err(|e| OpenStep::Dir(e.to_string()))?;
    crate::app_paths::ensure_private_file(path).map_err(|e| OpenStep::File(e.to_string()))?;
    let conn = Connection::open(path).map_err(|e| OpenStep::Open(e.to_string()))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| OpenStep::Configure(e.to_string()))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| OpenStep::Configure(e.to_string()))?;
    crate::app_paths::restrict_sqlite_files(path).map_err(|e| OpenStep::Secure(e.to_string()))?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("amara-sqlite-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn opens_private_wal_with_a_busy_timeout() {
        let dir = temp_dir("open");
        let path = dir.join("sub").join("t.db");
        let conn = open_private_wal(&path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let busy: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, BUSY_TIMEOUT.as_millis() as i64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, 0o600);
        }
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_writer_waits_instead_of_failing_at_once() {
        let dir = temp_dir("busy");
        let path = dir.join("t.db");
        let a = open_private_wal(&path).unwrap();
        a.execute_batch("CREATE TABLE t (x INTEGER); BEGIN IMMEDIATE; INSERT INTO t VALUES (1);")
            .unwrap();
        let p2 = path.clone();
        let h = std::thread::spawn(move || {
            let b = open_private_wal(&p2).unwrap();
            b.execute("INSERT INTO t VALUES (2)", []).map(|_| ())
        });
        std::thread::sleep(Duration::from_millis(200));
        a.execute_batch("COMMIT;").unwrap();
        assert!(
            h.join().unwrap().is_ok(),
            "the waiting writer should succeed"
        );
        drop(a);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
