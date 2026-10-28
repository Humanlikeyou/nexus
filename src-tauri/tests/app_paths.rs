//! app_paths: NEXUS_DATA_DIR override, the one-time legacy migration and the
//! config.json key merge, all against temp dirs.
use std::fs;
use std::os::unix::fs::PermissionsExt;

use nexus_lib::test_support::{
    data_dir, merge_legacy_config_in, migrate_legacy_data_dir_in, DATA_DIR_ENV, DATA_DIR_NAME,
};

mod common;

#[test]
fn data_dir_follows_nexus_data_dir() {
    let dir = common::tmp("datadir");
    // Only test in this binary that touches the env.
    std::env::set_var(DATA_DIR_ENV, &dir);
    assert_eq!(data_dir(), dir);
    std::env::set_var(DATA_DIR_ENV, "");
    assert!(data_dir().ends_with(DATA_DIR_NAME));
    std::env::remove_var(DATA_DIR_ENV);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn migration_copies_legacy_data_once_without_overwriting() {
    let base = common::tmp("migrate");
    let old = base.join("com.taisun.electron");
    let new = base.join(DATA_DIR_NAME);
    fs::create_dir_all(old.join("sessions")).unwrap();
    fs::write(old.join("sessions/s1.json"), "legacy").unwrap();
    fs::write(old.join("goals.json"), "legacy goals").unwrap();
    fs::write(
        old.join("config.json"),
        r#"{"telegram_chat_id":"42","gateway_url":"http://x"}"#,
    )
    .unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(new.join("goals.json"), "new goals").unwrap();
    fs::write(
        new.join("config.json"),
        r#"{"telegram_chat_id":"","extra":"keep"}"#,
    )
    .unwrap();

    let mut copied = migrate_legacy_data_dir_in(&base).unwrap();
    copied.sort();
    assert_eq!(copied, vec!["sessions".to_string()]);
    assert_eq!(
        fs::read_to_string(new.join("sessions/s1.json")).unwrap(),
        "legacy"
    );
    assert_eq!(
        fs::read_to_string(new.join("goals.json")).unwrap(),
        "new goals"
    );
    assert!(
        old.join("goals.json").exists(),
        "legacy dir is left in place"
    );
    assert!(
        migrate_legacy_data_dir_in(&base).unwrap().is_empty(),
        "runs once"
    );

    // The key merge then fills what the file-level copy skipped.
    let mut merged = merge_legacy_config_in(&base).unwrap();
    merged.sort();
    assert_eq!(
        merged,
        vec!["gateway_url".to_string(), "telegram_chat_id".to_string()]
    );
    let cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(new.join("config.json")).unwrap()).unwrap();
    assert_eq!(cfg["telegram_chat_id"], "42");
    assert_eq!(cfg["extra"], "keep");
    assert_eq!(
        fs::metadata(new.join("config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        merge_legacy_config_in(&base).unwrap().is_empty(),
        "merge runs once"
    );
    let _ = fs::remove_dir_all(base);
}
