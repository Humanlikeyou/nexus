//! onboarding.rs, goals.rs and packager.rs resolve the data dir through
//! app_paths::data_dir(), so NEXUS_DATA_DIR keeps them inside a temp dir.
//! One test function: the env var is process-wide.
use nexus_lib::test_support::{
    check_onboarding, complete_onboarding, packager_data_dir, read_goals, save_goals, DATA_DIR_ENV,
};

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

    std::env::remove_var(DATA_DIR_ENV);
}
