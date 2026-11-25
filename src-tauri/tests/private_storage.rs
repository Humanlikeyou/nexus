//! Decision 0050: private identity, profile, context and session logs live only in the app-data folder.
//! Drives the real commands with NEXUS_DATA_DIR pointing at a temp dir. One test function: the env var is
//! process-wide.
use nexus_lib::test_support::{
    list_session_logs, load_terrance_context, private_store, read_identity_file, read_profile_file,
    save_terrance_context, store_session_log, SessionLogEntry, TerranceContext, DATA_DIR_ENV,
};
use std::fs;

mod common;

fn entry(id: &str, ts: u64) -> SessionLogEntry {
    SessionLogEntry {
        id: id.into(),
        timestamp: ts,
        message_count: 4,
        summary: "test summary".into(),
        provider: "local".into(),
        model: "fast".into(),
    }
}

#[test]
fn private_data_stays_in_the_app_data_folder_across_launches() {
    let data = common::tmp("private-data");
    let repo = common::tmp("private-repo");
    std::env::set_var(DATA_DIR_ENV, &data);

    // First launch, nothing stored yet: generic defaults, no private files, no logs.
    assert_eq!(
        read_identity_file().unwrap(),
        private_store::DEFAULT_IDENTITY
    );
    assert_eq!(read_profile_file().unwrap(), private_store::DEFAULT_PROFILE);
    assert!(list_session_logs().unwrap().is_empty());

    // An older build left private files in a checkout: they are copied once, the checkout is untouched.
    fs::create_dir_all(repo.join("config")).unwrap();
    fs::create_dir_all(repo.join("memory/session_logs")).unwrap();
    fs::write(repo.join("CLAUDE.md"), "").unwrap();
    fs::write(repo.join("config/terrance.profile.md"), "# My profile").unwrap();
    fs::write(repo.join("config/terrance.identity.md"), "# My Amara").unwrap();
    let old = serde_json::json!({"id":"old-1","timestamp":1,"messageCount":2,"summary":"s","provider":"p","model":"m"});
    fs::write(repo.join("memory/session_logs/old-1.json"), old.to_string()).unwrap();
    let r = private_store::migrate_in(&data, std::slice::from_ref(&repo));
    assert!(r.profile_copied && r.identity_copied && r.logs_copied == 1);
    assert_eq!(read_profile_file().unwrap(), "# My profile");
    assert_eq!(read_identity_file().unwrap(), "# My Amara");
    assert!(
        repo.join("config/terrance.profile.md").exists(),
        "source kept"
    );

    // Session log persistence: written under the data folder, never into the checkout.
    store_session_log(entry("new-1", 10)).unwrap();
    assert!(data.join("session_logs/new-1.json").is_file());
    assert!(!repo.join("memory/session_logs/new-1.json").exists());
    let ids: Vec<String> = list_session_logs()
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(
        ids,
        ["new-1", "old-1"],
        "newest first, migrated log included"
    );

    // Existing context retrieval round-trips through the data folder.
    let ctx = TerranceContext {
        active_goals: vec!["Ship".into()],
        last_session_summary: Some("done".into()),
        ..Default::default()
    };
    save_terrance_context(ctx).unwrap();
    assert!(data.join("terrance_context.json").is_file());

    // Restart and reload: the migration is a no-op, and everything reads back the same.
    let again = private_store::migrate_in(&data, std::slice::from_ref(&repo));
    assert!(again.already_done);
    let ctx = load_terrance_context().unwrap();
    assert_eq!(ctx.active_goals, ["Ship"]);
    assert_eq!(ctx.last_session_summary.as_deref(), Some("done"));
    assert_eq!(list_session_logs().unwrap().len(), 2);
    assert_eq!(read_profile_file().unwrap(), "# My profile");

    std::env::remove_var(DATA_DIR_ENV);
    let _ = fs::remove_dir_all(&data);
    let _ = fs::remove_dir_all(&repo);
}
