//! outbox: get_outbox_status() on an empty data dir.
use nexus_lib::test_support::{get_outbox_status, DATA_DIR_ENV};

mod common;

// BUG: commands/events.rs:6-17 builds its own path from dirs::data_dir()
// (`nexus_data_dir()` → <Application Support>/com.taisun.nexus/events.db) and
// ignores NEXUS_DATA_DIR (app_paths::data_dir()). Running this test would open
// and create tables in the user's REAL events.db, so it stays ignored until
// events.rs uses app_paths::data_dir(). (commands/telegram.rs:5 has the same
// pattern for config.json.)
#[tokio::test]
#[ignore = "events.rs ignores NEXUS_DATA_DIR and would open the real events.db (events.rs:6)"]
async fn outbox_status_on_an_empty_data_dir() {
    let dir = common::tmp("outbox");
    std::env::set_var(DATA_DIR_ENV, &dir);
    let status = get_outbox_status().await.unwrap();
    std::env::remove_var(DATA_DIR_ENV);
    assert_eq!(status.queue_depth, 0);
    assert_eq!(status.total_delivered, 0);
    assert!(status.last_delivered_at.is_none());
    assert!(status.last_error.is_none());
    assert!(
        dir.join("events.db").exists(),
        "the db must be created inside NEXUS_DATA_DIR"
    );
    let _ = std::fs::remove_dir_all(dir);
}
