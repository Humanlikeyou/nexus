//! events.db (events.rs, arm.rs) and terrence_core.db: an older 0644 db and its -wal/-shm become
//! 0600 when the app opens them. One test function: NEXUS_DATA_DIR is process-wide.
#![cfg(unix)]
use nexus_lib::test_support::{open_private_dbs, DATA_DIR_ENV};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

mod common;

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn events_arm_and_terrence_core_dbs_and_sidecars_are_0600_after_open() {
    let dir = common::tmp("sqlite-modes");
    std::env::set_var(DATA_DIR_ENV, &dir);

    // Fresh install: created 0600 with sidecars 0600.
    open_private_dbs().unwrap();
    let names = ["events.db", "terrence_core.db"];
    let all = |n: &str| ["", "-wal", "-shm"].map(|s| dir.join(format!("{n}{s}")));
    for n in names {
        for p in all(n) {
            if p.exists() {
                assert_eq!(mode(&p), 0o600, "{}", p.display());
            }
        }
        assert_eq!(mode(&dir.join(n)), 0o600, "{n}");
    }

    // Older 0644 files (pre-bdc8722 installs) are tightened on the next open.
    for n in names {
        for p in all(n) {
            if p.exists() {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
    }
    open_private_dbs().unwrap();
    for n in names {
        for p in all(n) {
            if p.exists() {
                assert_eq!(mode(&p), 0o600, "{} after reopen", p.display());
            }
        }
    }
    std::env::remove_var(DATA_DIR_ENV);
}
