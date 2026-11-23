//! Unified operational log under `~/Library/Logs/com.taisun.nexus/` (Phase 7 step 5).
//!
//! * `app.log`, rotated by size (`app.log.1` ... `app.log.<keep-1>`), dir 0700, files 0600.
//! * Every line goes through `service_logs::redact_line` (bearer/keys/home paths/health words).
//! * Callers pass operational facts only. Never prompt bodies, secrets or health values: if a
//!   message would carry such content, drop the content at the call site (redaction is the
//!   second line of defence, not the first).
//! * Legacy router logs are COPIED (never moved or deleted) into `legacy/` once.
//! * Under `cfg(test)` the global logger writes to a temp dir, never the real log dir.

use super::service_logs::redact_line;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

pub const LOG_DIR_NAME: &str = "com.taisun.nexus";
pub const LOG_FILE: &str = "app.log";
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const DEFAULT_KEEP: usize = 7;
const LEGACY_DIR: &str = "legacy";
const LEGACY_MARKER: &str = ".legacy-copied";

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

pub struct Logger {
    dir: PathBuf,
    max_bytes: u64,
    keep: usize,
}

fn clean_source(source: &str) -> String {
    let s: String = source
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(32)
        .collect();
    if s.is_empty() {
        "app".to_string()
    } else {
        s
    }
}

impl Logger {
    pub fn new(dir: PathBuf, max_bytes: u64, keep: usize) -> Self {
        Self {
            dir,
            max_bytes: max_bytes.max(1),
            keep: keep.max(1),
        }
    }

    fn rotated(&self, n: usize) -> PathBuf {
        self.dir.join(format!("{LOG_FILE}.{n}"))
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        crate::app_paths::create_private_dir_all(&self.dir)
    }

    fn rotate(&self) -> std::io::Result<()> {
        // Files kept: app.log plus app.log.1 .. app.log.(keep-1).
        if self.keep == 1 {
            return fs::remove_file(self.dir.join(LOG_FILE));
        }
        let oldest = self.rotated(self.keep - 1);
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for n in (1..self.keep - 1).rev() {
            let from = self.rotated(n);
            if from.exists() {
                fs::rename(&from, self.rotated(n + 1))?;
            }
        }
        fs::rename(self.dir.join(LOG_FILE), self.rotated(1))
    }

    /// Append one redacted line. Best effort for callers; returns the IO error for tests.
    pub fn write(&self, source: &str, level: Level, msg: &str) -> std::io::Result<()> {
        self.ensure_dir()?;
        let line = format!(
            "{} {} [{}] {}\n",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            level.tag(),
            clean_source(source),
            redact_line(msg)
        );
        let path = self.dir.join(LOG_FILE);
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.len() > 0 && meta.len() + line.len() as u64 > self.max_bytes {
                self.rotate()?;
            }
        }
        let mut opts = crate::app_paths::private_open_options();
        opts.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Refuse a log file swapped for a symlink (ELOOP).
            opts.custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = opts.open(&path)?;
        f.write_all(line.as_bytes())
    }

    /// Copy (never move or delete) every regular file in `src` into `<dir>/legacy/`, once.
    /// Existing destination files are not overwritten. Returns the number copied.
    pub fn copy_legacy_from(&self, src: &Path) -> std::io::Result<usize> {
        let marker = self.dir.join(LEGACY_MARKER);
        if marker.exists() || !src.is_dir() {
            return Ok(0);
        }
        let dest = self.dir.join(LEGACY_DIR);
        crate::app_paths::create_private_dir_all(&dest)?;
        let mut copied = 0;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if !meta.is_file() {
                continue;
            }
            let target = dest.join(entry.file_name());
            if target.exists() {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            crate::app_paths::write_private(&target, bytes)?;
            copied += 1;
        }
        crate::app_paths::write_private(&marker, b"legacy logs copied; originals left in place\n")?;
        Ok(copied)
    }
}

#[cfg(not(test))]
fn default_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join("Library/Logs").join(LOG_DIR_NAME))
}

#[cfg(test)]
fn default_dir() -> Option<PathBuf> {
    Some(
        std::env::temp_dir()
            .join(format!("nexus-unified-log-test-{}", std::process::id()))
            .join(LOG_DIR_NAME),
    )
}

static GLOBAL: LazyLock<Option<Mutex<Logger>>> = LazyLock::new(|| {
    let dir = default_dir()?;
    let logger = Logger::new(dir, DEFAULT_MAX_BYTES, DEFAULT_KEEP);
    #[cfg(not(test))]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            let legacy = PathBuf::from(home).join("Library/Logs/amara-router");
            let _ = logger
                .ensure_dir()
                .and_then(|_| logger.copy_legacy_from(&legacy));
        }
    }
    Some(Mutex::new(logger))
});

/// Operational log line. Never fails the caller; falls back to stderr (redacted) on IO error.
pub(crate) fn log_line(source: &str, level: Level, msg: &str) {
    let ok = GLOBAL
        .as_ref()
        .and_then(|m| m.lock().ok())
        .map(|l| l.write(source, level, msg).is_ok())
        .unwrap_or(false);
    if !ok {
        eprintln!("[{}] {}", clean_source(source), redact_line(msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-ulog-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_a_symlinked_log_file() {
        let dir = tmp();
        let l = Logger::new(dir.clone(), 10_000, 3);
        l.write("t", Level::Info, "first").unwrap();
        let target = dir.join("victim.txt");
        fs::write(&target, "keep").unwrap();
        fs::remove_file(dir.join(LOG_FILE)).unwrap();
        std::os::unix::fs::symlink(&target, dir.join(LOG_FILE)).unwrap();
        assert!(l.write("t", Level::Info, "second").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_boundary_keeps_n_files() {
        let dir = tmp();
        let l = Logger::new(dir.clone(), 200, 3);
        for i in 0..40 {
            l.write("t", Level::Info, &format!("line number {i}"))
                .unwrap();
        }
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["app.log", "app.log.1", "app.log.2"]);
        for n in &names {
            assert!(fs::metadata(dir.join(n)).unwrap().len() <= 200, "{n}");
        }
        let newest = fs::read_to_string(dir.join("app.log")).unwrap();
        assert!(newest.contains("line number 39"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn exactly_at_limit_does_not_rotate_one_over_does() {
        let dir = tmp();
        let l = Logger::new(dir.clone(), 1_000_000, 3);
        l.write("t", Level::Info, "a").unwrap();
        let size = fs::metadata(dir.join(LOG_FILE)).unwrap().len();
        let l2 = Logger::new(dir.clone(), size * 2, 3);
        l2.write("t", Level::Info, "a").unwrap();
        assert!(!dir.join("app.log.1").exists(), "fits exactly");
        l2.write("t", Level::Info, "a").unwrap();
        assert!(dir.join("app.log.1").exists(), "one over rotates");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn dir_0700_files_0600_including_rotated() {
        let dir = tmp().join("nested");
        let l = Logger::new(dir.clone(), 100, 3);
        for i in 0..10 {
            l.write("t", Level::Warn, &format!("x{i}")).unwrap();
        }
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("app.log")), 0o600);
        assert_eq!(mode(&dir.join("app.log.1")), 0o600);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn redacts_bearer_keys_and_health_and_never_writes_the_sentinel() {
        let dir = tmp();
        let l = Logger::new(dir.clone(), DEFAULT_MAX_BYTES, 3);
        let sentinel = "SENTINEL-9f3a1c7e22b4";
        l.write(
            "t",
            Level::Error,
            &format!("Authorization: Bearer {sentinel}"),
        )
        .unwrap();
        l.write("t", Level::Error, &format!("api_key={sentinel} failed"))
            .unwrap();
        l.write("t", Level::Info, "heart rate 61 bpm today")
            .unwrap();
        l.write("t", Level::Info, "line one\nforged INFO [x] second")
            .unwrap();
        let text = fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        assert!(!text.contains(sentinel), "{text}");
        assert!(!text.contains("61 bpm"), "{text}");
        assert_eq!(text.lines().count(), 4, "newlines cannot forge lines");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_is_sanitised() {
        assert_eq!(clean_source("../x y\n"), "xy");
        assert_eq!(clean_source(""), "app");
    }

    #[test]
    fn legacy_copy_leaves_originals_and_runs_once() {
        let root = tmp();
        let src = root.join("old");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("router.log"), "old line\n").unwrap();
        let l = Logger::new(root.join("new"), DEFAULT_MAX_BYTES, 3);
        assert_eq!(l.copy_legacy_from(&src).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(src.join("router.log")).unwrap(),
            "old line\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("new/legacy/router.log")).unwrap(),
            "old line\n"
        );
        fs::write(src.join("later.log"), "z").unwrap();
        assert_eq!(
            l.copy_legacy_from(&src).unwrap(),
            0,
            "marker stops a second run"
        );
        assert!(src.join("later.log").exists());
        assert_eq!(l.copy_legacy_from(&root.join("absent")).unwrap(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn global_logger_never_targets_the_real_log_dir_in_tests() {
        let d = default_dir().unwrap();
        assert!(d.starts_with(std::env::temp_dir()));
        log_line("test", Level::Info, "global smoke");
    }
}
