//! path_safety: names, file names and safe_join reject traversal and symlink escapes.
use std::fs;

use nexus_lib::test_support::{safe_join, validate_file_name, validate_name};

mod common;

#[test]
fn names_reject_traversal_and_separators() {
    for bad in [
        "",
        "..",
        "../x",
        "a/b",
        "a\\b",
        ".hidden",
        "a b",
        "x".repeat(129).as_str(),
    ] {
        assert!(validate_name(bad).is_err(), "{bad:?} must be rejected");
    }
    assert!(validate_name("daily-brief_2").is_ok());
    assert!(validate_file_name("report.json", &["json"]).is_ok());
    for bad in [
        "../report.json",
        "report",
        "report.sh",
        "a/report.json",
        "..json",
    ] {
        assert!(
            validate_file_name(bad, &["json"]).is_err(),
            "{bad:?} must be rejected"
        );
    }
}

#[test]
fn safe_join_rejects_dot_dot_and_separators() {
    let base = common::tmp("join");
    for bad in ["..", ".", "../escape.json", "a/b.json", "a\\b.json", ""] {
        assert!(safe_join(&base, bad).is_err(), "{bad:?} must be rejected");
    }
    let ok = safe_join(&base, "ok.json").unwrap();
    assert_eq!(ok.parent().unwrap(), base.canonicalize().unwrap());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn safe_join_rejects_symlinks_that_leave_the_base() {
    let root = common::tmp("symlink");
    let base = root.join("base");
    let outside = root.join("outside");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.json"), "{}").unwrap();
    std::os::unix::fs::symlink(outside.join("secret.json"), base.join("link.json")).unwrap();
    assert!(safe_join(&base, "link.json").is_err());
    // A symlinked base directory itself is resolved first, then checked.
    std::os::unix::fs::symlink(&outside, root.join("base-link")).unwrap();
    let via_link = safe_join(&root.join("base-link"), "secret.json").unwrap();
    assert_eq!(
        via_link,
        outside.canonicalize().unwrap().join("secret.json")
    );
    let _ = fs::remove_dir_all(root);
}
