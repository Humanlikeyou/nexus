//! packager: unpack_backup's checked extraction (tar-slip guard) and its
//! restore allow-list, on archives built in the test.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use nexus_lib::test_support::{plan_restore, restore_allowed, unpack_archive_checked};

mod common;

/// Raw header with any name/type, bypassing tar::Builder's own path checks.
fn raw_entry(name: &str, kind: tar::EntryType, data: &[u8]) -> Vec<u8> {
    let mut h = tar::Header::new_gnu();
    {
        let bytes = h.as_old_mut();
        bytes.name = [0; 100];
        bytes.name[..name.len()].copy_from_slice(name.as_bytes());
    }
    h.set_entry_type(kind);
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_cksum();
    let mut out = h.as_bytes().to_vec();
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
}

fn gz(dir: &Path, entries: &[Vec<u8>]) -> PathBuf {
    let mut raw: Vec<u8> = entries.concat();
    raw.extend([0u8; 1024]);
    let path = dir.join("pack.tar.gz");
    let mut enc = flate2::write::GzEncoder::new(
        fs::File::create(&path).unwrap(),
        flate2::Compression::fast(),
    );
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    path
}

#[test]
fn tar_slip_entries_are_rejected_and_nothing_escapes() {
    for (name, kind) in [
        ("../evil.txt", tar::EntryType::Regular),
        ("pack/../../evil.txt", tar::EntryType::Regular),
        ("/tmp/nexus-it-evil.txt", tar::EntryType::Regular),
        ("pack/link", tar::EntryType::Symlink),
        ("pack/hard", tar::EntryType::Link),
    ] {
        let dir = common::tmp("slip");
        let dest = dir.join("out");
        fs::create_dir(&dest).unwrap();
        let archive = gz(
            &dir,
            &[
                raw_entry("pack/goals.json", tar::EntryType::Regular, b"[]"),
                raw_entry(name, kind, b"x"),
            ],
        );
        assert!(
            unpack_archive_checked(&archive, &dest, 1 << 20, 100).is_err(),
            "{name} must be rejected"
        );
        assert!(!dir.join("evil.txt").exists());
        assert!(!Path::new("/tmp/nexus-it-evil.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn size_and_entry_limits_are_enforced() {
    let dir = common::tmp("limits");
    let dest = dir.join("out");
    fs::create_dir(&dest).unwrap();
    let entries: Vec<_> = (0..3)
        .map(|i| raw_entry(&format!("pack/f{i}.json"), tar::EntryType::Regular, b"{}"))
        .collect();
    let archive = gz(&dir, &entries);
    assert!(
        unpack_archive_checked(&archive, &dest, 1 << 20, 2).is_err(),
        "entry cap"
    );
    let dest2 = dir.join("out2");
    fs::create_dir(&dest2).unwrap();
    assert!(
        unpack_archive_checked(&archive, &dest2, 3, 100).is_err(),
        "byte cap"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn restore_allow_list() {
    for ok in [
        &["goals.json"][..],
        &["events.db"],
        &["sessions", "s1.json"],
        &["nexus-config", "config.json"],
        &["electron-config", "kv", "a"],
    ] {
        assert!(restore_allowed(ok), "{ok:?} should restore");
    }
    // Old backups' `openclaw/` folder is never restored (OpenClaw is uninstalled).
    for bad in [
        &["random.txt"][..],
        &["openclaw", "openclaw.json"],
        &["openclaw", "skills", "brief.md"],
        &["openclaw", "cron", "jobs.json"],
        &["openclaw", "memory", "2026", "10", "note.md"],
        &["openclaw", "skills", "run.sh"],
        &["openclaw", "cron", "jobs.sh"],
        &["openclaw", "bin", "openclaw"],
        &["openclaw", "memory", "a", "b", "c", "d.md"],
        &["nexus-config", "restore-backups", "x"],
        &["openclaw"],
    ] {
        assert!(!restore_allowed(bad), "{bad:?} must be skipped");
    }
}

#[test]
fn a_normal_pack_unpacks_and_plans_only_allowed_files() {
    let dir = common::tmp("plan");
    let dest = dir.join("out");
    fs::create_dir(&dest).unwrap();
    let archive = gz(
        &dir,
        &[
            raw_entry("manifest.json", tar::EntryType::Regular, b"{}"),
            raw_entry("goals.json", tar::EntryType::Regular, b"[]"),
            raw_entry(
                "openclaw/skills/brief.md",
                tar::EntryType::Regular,
                b"# brief",
            ),
            raw_entry(
                "openclaw/skills/run.sh",
                tar::EntryType::Regular,
                b"#!/bin/sh",
            ),
            raw_entry("notes.txt", tar::EntryType::Regular, b"x"),
        ],
    );
    assert_eq!(
        unpack_archive_checked(&archive, &dest, 1 << 20, 100).unwrap(),
        5
    );
    let (mut restored, skipped) = plan_restore(&dest).unwrap();
    restored.sort();
    assert_eq!(restored, vec!["goals.json".to_string()]);
    assert_eq!(skipped.len(), 3, "{skipped:?}");
    assert!(skipped
        .iter()
        .any(|s| s.starts_with("openclaw/skills/brief.md")));
    assert!(skipped
        .iter()
        .any(|s| s.starts_with("openclaw/skills/run.sh")));
    assert!(skipped.iter().any(|s| s.starts_with("notes.txt")));
    let _ = fs::remove_dir_all(dir);
}
