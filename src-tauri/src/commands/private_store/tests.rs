use super::*;
use crate::commands::local_store::tests::temp_dir;
use std::os::unix::fs::PermissionsExt;

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// A fake old repository tree with private files where older builds kept them.
fn legacy_repo(tag: &str) -> PathBuf {
    let root = temp_dir(tag);
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(root.join("CLAUDE.md"), "# repo").unwrap();
    fs::write(
        root.join("config").join(IDENTITY_FILE),
        "# Private identity v1",
    )
    .unwrap();
    fs::write(
        root.join("config").join(PROFILE_FILE),
        "# Private profile v1",
    )
    .unwrap();
    let logs = root.join("memory").join("session_logs");
    fs::create_dir_all(&logs).unwrap();
    for i in 0..3 {
        fs::write(
            logs.join(format!("2026-11-0{i}T10-00-00.json")),
            format!("{{\"id\":\"{i}\"}}"),
        )
        .unwrap();
    }
    let arch = root.join("memory").join("archive");
    fs::create_dir_all(&arch).unwrap();
    fs::write(arch.join("old.json"), "{}").unwrap();
    root
}

#[test]
fn first_launch_with_no_memory_uses_the_generic_defaults_and_creates_nothing_private() {
    let data = temp_dir("ps-first");
    assert_eq!(identity_in(&data), DEFAULT_IDENTITY);
    assert_eq!(profile_in(&data), DEFAULT_PROFILE);
    let r = migrate_in(&data, &[]);
    assert_eq!(r, MigrationReport::default());
    assert!(!config_dir_in(&data).join(IDENTITY_FILE).exists());
    assert!(!session_logs_dir_in(&data).exists());
    assert_eq!(mode(&config_dir_in(&data)), 0o700);
    // the defaults carry no personal information
    for d in [DEFAULT_IDENTITY, DEFAULT_PROFILE] {
        assert!(
            !d.to_lowercase().contains("taisun"),
            "a default must stay generic"
        );
    }
    let _ = fs::remove_dir_all(data);
}

#[test]
fn migration_copies_private_files_and_logs_and_never_touches_the_source() {
    let data = temp_dir("ps-mig");
    let root = legacy_repo("ps-mig-repo");
    let before_profile = fs::read(root.join("config").join(PROFILE_FILE)).unwrap();
    let r = migrate_in(&data, std::slice::from_ref(&root));
    assert!(r.identity_copied && r.profile_copied);
    assert_eq!((r.logs_copied, r.archive_copied, r.skipped), (3, 1, 0));
    let cfg = config_dir_in(&data);
    assert_eq!(
        fs::read_to_string(cfg.join(PROFILE_FILE)).unwrap(),
        "# Private profile v1"
    );
    assert_eq!(mode(&cfg.join(PROFILE_FILE)), 0o600);
    assert_eq!(mode(&cfg.join(IDENTITY_FILE)), 0o600);
    assert_eq!(fs::read_dir(session_logs_dir_in(&data)).unwrap().count(), 3);
    assert!(archive_dir_in(&data).join("old.json").exists());
    // existing context retrieval: the private copies are what loads now
    assert_eq!(identity_in(&data), "# Private identity v1");
    assert_eq!(profile_in(&data), "# Private profile v1");
    // the source is left exactly as it was
    assert_eq!(
        fs::read(root.join("config").join(PROFILE_FILE)).unwrap(),
        before_profile
    );
    assert_eq!(
        fs::read_dir(root.join("memory").join("session_logs"))
            .unwrap()
            .count(),
        3
    );
    let _ = fs::remove_dir_all(data);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn migration_is_idempotent_and_never_overwrites_an_edited_private_copy() {
    let data = temp_dir("ps-idem");
    let root = legacy_repo("ps-idem-repo");
    migrate_in(&data, std::slice::from_ref(&root));
    // restart and reload: the marker makes the second run a no-op
    let again = migrate_in(&data, std::slice::from_ref(&root));
    assert!(again.already_done);
    // without the marker (e.g. a crash before it was written) nothing is overwritten either
    fs::remove_file(config_dir_in(&data).join(MARKER)).unwrap();
    fs::write(
        config_dir_in(&data).join(PROFILE_FILE),
        "# Edited in the app",
    )
    .unwrap();
    let r = migrate_in(&data, std::slice::from_ref(&root));
    assert!(!r.profile_copied && !r.identity_copied);
    assert_eq!(r.logs_copied, 0);
    assert_eq!(r.kept_existing, 2 + 3 + 1);
    assert_eq!(profile_in(&data), "# Edited in the app");
    let _ = fs::remove_dir_all(data);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn symlinks_bad_names_empty_and_oversize_files_are_skipped_not_followed() {
    let data = temp_dir("ps-skip");
    let root = temp_dir("ps-skip-repo");
    fs::create_dir_all(root.join("config")).unwrap();
    let outside = temp_dir("ps-skip-outside");
    fs::write(outside.join("secret.md"), "outside").unwrap();
    std::os::unix::fs::symlink(
        outside.join("secret.md"),
        root.join("config").join(PROFILE_FILE),
    )
    .unwrap();
    fs::write(root.join("config").join(IDENTITY_FILE), "").unwrap();
    let logs = root.join("memory").join("session_logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("bad name.json"), "{}").unwrap();
    fs::write(
        logs.join("big.json"),
        vec![b'x'; (MAX_LOG_BYTES + 1) as usize],
    )
    .unwrap();
    fs::write(logs.join("ok.json"), "{}").unwrap();
    let r = migrate_in(&data, std::slice::from_ref(&root));
    assert!(!r.profile_copied && !r.identity_copied);
    assert_eq!(r.logs_copied, 1);
    assert_eq!(
        r.skipped,
        2 + 2,
        "symlink + empty doc, bad name + oversize log"
    );
    assert_eq!(
        profile_in(&data),
        DEFAULT_PROFILE,
        "a symlink is never followed"
    );
    for p in [&data, &root, &outside] {
        let _ = fs::remove_dir_all(p);
    }
}

#[test]
fn an_unreadable_or_oversize_private_doc_falls_back_to_the_default() {
    let data = temp_dir("ps-fallback");
    let cfg = config_dir_in(&data);
    crate::app_paths::create_private_dir_all(&cfg).unwrap();
    fs::write(
        cfg.join(IDENTITY_FILE),
        vec![b'x'; (MAX_DOC_BYTES + 1) as usize],
    )
    .unwrap();
    fs::write(cfg.join(PROFILE_FILE), "   \n").unwrap();
    assert_eq!(identity_in(&data), DEFAULT_IDENTITY);
    assert_eq!(profile_in(&data), DEFAULT_PROFILE);
    let _ = fs::remove_dir_all(data);
}

#[test]
fn a_packaged_app_has_no_repository_root_and_a_dev_build_finds_its_checkout() {
    // packaged: /Applications/Amara OS.app/Contents/MacOS/nexus has no CLAUDE.md above it
    let apps = temp_dir("ps-pkg");
    let exe = apps
        .join("Amara OS.app")
        .join("Contents")
        .join("MacOS")
        .join("nexus");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "").unwrap();
    assert_eq!(repo_root_above(&exe), None);
    // dev: <repo>/src-tauri/target/debug/nexus
    let repo = temp_dir("ps-dev");
    fs::write(repo.join("CLAUDE.md"), "").unwrap();
    let dev = repo
        .join("src-tauri")
        .join("target")
        .join("debug")
        .join("nexus");
    fs::create_dir_all(dev.parent().unwrap()).unwrap();
    fs::write(&dev, "").unwrap();
    assert_eq!(repo_root_above(&dev), Some(repo.clone()));
    // either way, private paths come from the data folder only
    let data = temp_dir("ps-pkg-data");
    assert_eq!(session_logs_dir_in(&data), data.join("session_logs"));
    assert_eq!(config_dir_in(&data), data.join("config"));
    for p in [&apps, &repo, &data] {
        let _ = fs::remove_dir_all(p);
    }
}

/// Source pin: nothing in continuity.rs or here resolves a private path from the binary's location or a
/// checkout (the only repo-relative reads are the migration's legacy roots, which are read-only).
#[test]
fn no_private_path_is_resolved_from_the_repository() {
    let continuity = include_str!("../continuity.rs");
    assert!(
        !continuity.contains("CLAUDE.md"),
        "continuity must not look for a checkout"
    );
    assert!(
        !continuity.contains("current_exe"),
        "continuity must not resolve paths from the binary"
    );
    assert!(
        !continuity.contains("projects/nexus"),
        "no hard-coded checkout path"
    );
    assert!(continuity.contains("private_store::session_logs_dir_in"));
}
