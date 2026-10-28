//! continuity: atomic_write replaces content without leaving temp files and
//! never widens permissions or follows a symlink.
use std::fs;
use std::os::unix::fs::PermissionsExt;

use nexus_lib::test_support::atomic_write;

mod common;

#[test]
fn atomic_write_replaces_content_and_leaves_no_temp_files() {
    let dir = common::tmp("atomic");
    let path = dir.join("terrance_context.json");
    atomic_write(&path, b"{\"a\":1}").unwrap();
    atomic_write(&path, b"{\"a\":2}").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":2}");
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("terrance_context.json")]
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600,
        "new files are 0600"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn atomic_write_keeps_a_restrictive_mode_and_refuses_symlinks() {
    let dir = common::tmp("atomic-mode");
    let path = dir.join("f.json");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    atomic_write(&path, b"new").unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );

    let target = dir.join("target.json");
    fs::write(&target, "keep").unwrap();
    let link = dir.join("link.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(atomic_write(&link, b"evil").is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
    let _ = fs::remove_dir_all(dir);
}
