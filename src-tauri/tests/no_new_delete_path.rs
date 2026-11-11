//! Muse recordings are sacred (.claude/rules/muse-recordings.md). Taisun removed the legacy
//! `delete_session` (docs/decisions/0003), so nothing is allow-listed: no command may delete a
//! session, replay or Muse capture, and the recording code has no way to remove or move a file.
//! Source scan, not a behaviour test: it fails the build if one is added.
use std::path::Path;

const FILES: [&str; 5] = [
    "muse.rs",
    "replay.rs",
    "sessions.rs",
    "recordings.rs",
    "recordings_dsp.rs",
];

fn src(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel)).unwrap()
}

/// The file without its `#[cfg(test)] mod x { ... }` blocks (rustfmt puts both ends at column 0).
fn production(text: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    let mut pending_cfg = false;
    for line in text.lines() {
        if skipping {
            skipping = line != "}";
            continue;
        }
        if line == "#[cfg(test)]" {
            pending_cfg = true;
            continue;
        }
        if pending_cfg {
            pending_cfg = false;
            if line.starts_with("mod ") && line.ends_with('{') {
                skipping = true;
                continue;
            }
            out.push_str("#[cfg(test)]\n");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn no_code_path_removes_or_moves_session_replay_or_muse_files() {
    for f in FILES {
        let code = production(&src(&format!("commands/{f}")));
        for token in ["remove_file", "remove_dir", "unlink", "trash"] {
            assert!(!code.contains(token), "{f} contains {token}");
        }
        let renames = code.matches("fs::rename").count();
        // The only rename is the sidecar's temp file over `<replay>.bands.json`.
        assert_eq!(renames, usize::from(f == "recordings.rs"), "{f} renames");
    }
    let sidecar = production(&src("commands/recordings.rs"));
    let at = sidecar.find("fs::rename").unwrap();
    assert!(sidecar[..at]
        .rfind("fn write_sidecar")
        .is_some_and(|f| !sidecar[f..at].contains("\nfn ")));
}

#[test]
fn no_registered_command_can_delete_recordings_or_sessions() {
    let lib = src("lib.rs");
    let start = lib.find("generate_handler![").unwrap();
    let end = start + lib[start..].find("])").unwrap();
    let names: Vec<&str> = lib[start..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .filter_map(|l| l.strip_suffix(','))
        .collect();
    assert!(names.len() > 100, "parsed {} commands", names.len());
    for name in &names {
        assert_ne!(
            *name, "sessions::delete_session",
            "removed by decision 0003"
        );
        let module = name.split("::").next().unwrap();
        if ["replay", "muse", "sessions", "recordings"].contains(&module) {
            for word in [
                "delete", "remove", "purge", "prune", "sweep", "compact", "clear",
            ] {
                assert!(!name.contains(word), "{name} looks like a delete path");
            }
        }
        assert!(!name.contains("remove_confirmed"), "{name}");
    }
    for needed in [
        "replay::mark_recording_aux",
        "recordings::list_recordings",
        "recordings::read_recording_window",
        "recordings::read_body_window",
        "recordings::recording_series",
        "recordings::get_session_samples",
    ] {
        assert!(names.contains(&needed), "{needed} not registered");
    }
}
