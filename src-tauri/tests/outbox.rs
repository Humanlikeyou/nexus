//! outbox: get_outbox_status() on an empty data dir.
use nexus_lib::test_support::{get_outbox_status, DATA_DIR_ENV};

mod common;

// events.rs and telegram.rs resolve the data dir via app_paths::data_dir(), so
// NEXUS_DATA_DIR keeps this test inside a temp dir.
#[tokio::test]
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
