//! Private identity, profile and session-log storage (decision 0050).
//!
//! Private data lives only in the application-support folder (`app_paths::data_dir()`), never in the source
//! repository, in packaged and development builds alike:
//! - `<data>/config/terrance.identity.md`: Amara's identity as Taisun wrote it.
//! - `<data>/config/terrance.profile.md`: his operational profile.
//! - `<data>/session_logs/*.json` and `<data>/archive/`: session summaries (continuity.rs).
//!
//! The repository only ships generic defaults (`config/defaults/*.default.md`), embedded at compile time, so
//! loading never depends on where the binary sits or on a checkout being present.
//!
//! `migrate_in` is a one-time, idempotent COPY of what older builds left in the repository tree
//! (`config/terrance.*.md`, `memory/session_logs/*.json`, `memory/archive/*.json`). It never overwrites an
//! existing file, never follows a symlink, never deletes or changes the source, and writes 0600 files in 0700
//! folders. Nothing it reads or writes is ever printed: the report carries counts only.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const IDENTITY_FILE: &str = "terrance.identity.md";
pub const PROFILE_FILE: &str = "terrance.profile.md";
const MARKER: &str = ".private_migrated_v1";
/// A private identity or profile larger than this is ignored (the default is used) and never copied.
const MAX_DOC_BYTES: u64 = 256 * 1024;
/// A session log larger than this is skipped by the migration.
const MAX_LOG_BYTES: u64 = 256 * 1024;
/// At most this many session logs and archived logs are copied in one run.
const MAX_LOGS: usize = 20_000;

pub const DEFAULT_IDENTITY: &str = include_str!("../../../config/defaults/identity.default.md");
pub const DEFAULT_PROFILE: &str = include_str!("../../../config/defaults/profile.default.md");

pub fn config_dir_in(data: &Path) -> PathBuf {
    data.join("config")
}

pub fn session_logs_dir_in(data: &Path) -> PathBuf {
    data.join("session_logs")
}

pub fn archive_dir_in(data: &Path) -> PathBuf {
    data.join("archive")
}

/// Reads a regular, non-empty file of at most `max` bytes without following a symlink. `None` otherwise.
fn read_small(path: &Path, max: u64) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() == 0 || m.len() > max {
        return None;
    }
    let mut s = String::new();
    f.take(max + 1).read_to_string(&mut s).ok()?;
    (!s.trim().is_empty()).then_some(s)
}

/// The private document if one exists and is readable, else the embedded generic default.
fn private_or_default(data: &Path, name: &str, default: &str) -> String {
    read_small(&config_dir_in(data).join(name), MAX_DOC_BYTES)
        .unwrap_or_else(|| default.to_string())
}

pub fn identity_in(data: &Path) -> String {
    private_or_default(data, IDENTITY_FILE, DEFAULT_IDENTITY)
}

pub fn profile_in(data: &Path) -> String {
    private_or_default(data, PROFILE_FILE, DEFAULT_PROFILE)
}

/// What a migration did. Counts only: no paths to private content, no content.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct MigrationReport {
    pub identity_copied: bool,
    pub profile_copied: bool,
    pub logs_copied: u32,
    pub archive_copied: u32,
    /// Targets that already existed (kept as they are; never overwritten).
    pub kept_existing: u32,
    /// Sources skipped: symlinks, non-regular files, oversize, bad names.
    pub skipped: u32,
    /// The run found the marker and did nothing.
    pub already_done: bool,
}

/// Copies `src` to a NEW file `dst` (0600). `Ok(false)` when `dst` already exists (left untouched).
fn copy_new(src: &Path, dst: &Path, max: u64) -> Result<Option<bool>, ()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if fs::symlink_metadata(dst).is_ok() {
        return Ok(Some(false));
    }
    let Some(text) = read_small(src, max) else {
        return Ok(None);
    };
    let mut out = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dst)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(Some(false)),
        Err(_) => return Err(()),
    };
    if out
        .write_all(text.as_bytes())
        .and_then(|_| out.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(dst); // only the partial copy we just created; the source is never touched
        return Err(());
    }
    Ok(Some(true))
}

fn safe_log_name(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| super::path_safety::validate_name(stem).is_ok())
}

/// Copies every `*.json` log in `src_dir` into `dst_dir` (new files only). Returns (copied, kept, skipped).
fn copy_logs(src_dir: &Path, dst_dir: &Path, budget: &mut usize) -> (u32, u32, u32) {
    let (mut copied, mut kept, mut skipped) = (0, 0, 0);
    let Ok(meta) = fs::symlink_metadata(src_dir) else {
        return (0, 0, 0);
    };
    if !meta.is_dir() {
        return (0, 0, 1);
    }
    let Ok(rd) = fs::read_dir(src_dir) else {
        return (0, 0, 0);
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    if !names.is_empty() && crate::app_paths::create_private_dir_all(dst_dir).is_err() {
        return (0, 0, names.len() as u32);
    }
    for name in names {
        if *budget == 0 {
            skipped += 1;
            continue;
        }
        if !safe_log_name(&name) {
            skipped += 1;
            continue;
        }
        match copy_new(&src_dir.join(&name), &dst_dir.join(&name), MAX_LOG_BYTES) {
            Ok(Some(true)) => {
                copied += 1;
                *budget -= 1;
            }
            Ok(Some(false)) => kept += 1,
            Ok(None) | Err(()) => skipped += 1,
        }
    }
    (copied, kept, skipped)
}

/// One-time copy of private data that older builds kept in the repository tree. `roots` are candidate
/// repository roots (each may hold `config/` and `memory/`). Idempotent: once the marker exists it does
/// nothing, and even without the marker it never overwrites what is already in `data`.
pub fn migrate_in(data: &Path, roots: &[PathBuf]) -> MigrationReport {
    let mut r = MigrationReport::default();
    let cfg = config_dir_in(data);
    if cfg.join(MARKER).exists() {
        r.already_done = true;
        return r;
    }
    if crate::app_paths::create_private_dir_all(&cfg).is_err() {
        return r;
    }
    let mut budget = MAX_LOGS;
    for root in roots {
        for (name, flag) in [(IDENTITY_FILE, 0u8), (PROFILE_FILE, 1u8)] {
            let src = root.join("config").join(name);
            if fs::symlink_metadata(&src).is_err() {
                continue;
            }
            match copy_new(&src, &cfg.join(name), MAX_DOC_BYTES) {
                Ok(Some(true)) => {
                    if flag == 0 {
                        r.identity_copied = true
                    } else {
                        r.profile_copied = true
                    }
                }
                Ok(Some(false)) => r.kept_existing += 1,
                Ok(None) | Err(()) => r.skipped += 1,
            }
        }
        let (c, k, s) = copy_logs(
            &root.join("memory").join("session_logs"),
            &session_logs_dir_in(data),
            &mut budget,
        );
        r.logs_copied += c;
        r.kept_existing += k;
        r.skipped += s;
        let (c, k, s) = copy_logs(
            &root.join("memory").join("archive"),
            &archive_dir_in(data),
            &mut budget,
        );
        r.archive_copied += c;
        r.kept_existing += k;
        r.skipped += s;
    }
    // Counts only (no content, no private paths). Written last, so a crash mid-way just re-runs the copy,
    // which is safe because nothing is ever overwritten.
    let summary = serde_json::to_string(&r).unwrap_or_default();
    let _ = crate::app_paths::write_private(&cfg.join(MARKER), summary);
    r
}

/// Repository roots older builds may have used: the checkout the binary runs from (development builds only:
/// a parent holding CLAUDE.md, at most 6 levels up) and the two known checkouts in the home folder. Only
/// read, never written.
pub fn legacy_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(repo_root_above(&exe));
    }
    if let Some(home) = dirs::home_dir() {
        for rel in ["projects/nexus", "projects/nexus-amara"] {
            let p = home.join(rel);
            if p.join("CLAUDE.md").exists() && !roots.contains(&p) {
                roots.push(p);
            }
        }
    }
    roots
}

/// The repository root above `exe` (a folder holding CLAUDE.md, at most 6 levels up), if any. A packaged app in
/// /Applications has none.
pub fn repo_root_above(exe: &Path) -> Option<PathBuf> {
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..6 {
        if dir.join("CLAUDE.md").exists() {
            return Some(dir);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Called once at startup, after the legacy data-dir migration. Logs counts only.
pub fn migrate_on_startup() {
    let r = migrate_in(&crate::app_paths::data_dir(), &legacy_roots());
    if !r.already_done {
        println!(
            "[private-store] migration: identity {} profile {} logs {} archive {} kept {} skipped {}",
            r.identity_copied, r.profile_copied, r.logs_copied, r.archive_copied, r.kept_existing, r.skipped
        );
    }
}

#[cfg(test)]
#[path = "private_store/tests.rs"]
mod tests;
