//! onboarding.rs, goals.rs and packager.rs resolve the data dir through
//! app_paths::data_dir(), so NEXUS_DATA_DIR keeps them inside a temp dir.
//! So do the nine compiled modules moved off dirs::data_dir() (replay, terrence_context, arm,
//! terrence_runtime, snapshots, llm_router, sessions, kv, assistant),
//! and with the env var unset they resolve to the same real paths as before.
//! One test function: the env var is process-wide.
use nexus_lib::test_support::{
    check_onboarding, complete_onboarding, module_data_paths, packager_data_dir, read_goals,
    save_goals, DATA_DIR_ENV, DATA_DIR_NAME,
};
use std::path::{Path, PathBuf};

mod common;

#[test]
fn onboarding_goals_and_packager_honour_nexus_data_dir() {
    let dir = common::tmp("datadir-env");
    std::env::set_var(DATA_DIR_ENV, &dir);

    // onboarding.json
    assert!(!check_onboarding().unwrap());
    complete_onboarding().unwrap();
    assert!(check_onboarding().unwrap());
    assert!(dir.join("onboarding.json").is_file());

    // goals.json
    let goals = read_goals().unwrap();
    save_goals(goals.clone()).unwrap();
    assert!(dir.join("goals.json").is_file());
    assert_eq!(read_goals().unwrap().len(), goals.len());

    // packager
    assert_eq!(packager_data_dir(), dir);

    // The nine compiled modules moved onto app_paths::data_dir().
    let leaf = |name: &str, base: &Path| -> PathBuf {
        match name {
            "kv" => base.join("kv"),
            "llm_router" => base.join("llm_usage.json"),
            _ => base.to_path_buf(),
        }
    };
    let paths = module_data_paths();
    assert_eq!(paths.len(), 9);
    for (name, path) in &paths {
        assert_eq!(path, &leaf(name, &dir), "{name} ignores {DATA_DIR_ENV}");
    }

    // Unset: identical to the old dirs::data_dir()/com.taisun.nexus paths.
    std::env::remove_var(DATA_DIR_ENV);
    let legacy = dirs::data_dir().unwrap().join(DATA_DIR_NAME);
    for (name, path) in module_data_paths() {
        assert_eq!(path, leaf(name, &legacy), "{name} moved its real path");
    }
}
