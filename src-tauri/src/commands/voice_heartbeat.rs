//! Voice only while Amara OS is open (decision 0024).
//!
//! While the app runs it writes `<voice dir>/app_alive` = "<pid> <unix ms>\n" every 5 s, atomically
//! (tmp + rename, mode 0600). The patched Amara Voice loop (tasks/proposed/amara-voice-fixes-2026-11-14.diff)
//! ignores the wake word unless that file is fresh (< 15 s) and its pid is alive. On a normal exit the
//! file is removed (best effort); after a crash it simply goes stale. File only: no Command, no
//! launchctl, no network, no LaunchAgent change.
//!
//! Same safe-dir rules as the speak/dictate hand-off: the voice dir must be a real directory (never a
//! symlink) owned by this user with no group/other write, otherwise nothing is written.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::voice::{hand_off_dir_ok_for, voice_paths};

pub(crate) const HEARTBEAT_FILE: &str = "app_alive";
const HEARTBEAT_TMP: &str = "app_alive.tmp";
pub(crate) const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Write one heartbeat into `dir` for `uid`. Returns false (writing nothing) when the dir is unsafe.
pub(crate) fn beat_in(dir: &Path, uid: u32, pid: u32, now_ms: u64) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    if !hand_off_dir_ok_for(dir, uid) {
        return false;
    }
    let tmp = dir.join(HEARTBEAT_TMP);
    // remove_file never follows a symlink; create_new (O_EXCL) refuses one that appears meanwhile.
    let _ = fs::remove_file(&tmp);
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(format!("{pid} {now_ms}\n").as_bytes()))
        .and_then(|()| fs::rename(&tmp, dir.join(HEARTBEAT_FILE)));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}

/// Remove our heartbeat (only if it names `pid`, so a second instance's file is left alone).
/// Best effort; returns whether a file was removed.
pub(crate) fn remove_in(dir: &Path, uid: u32, pid: u32) -> bool {
    if !hand_off_dir_ok_for(dir, uid) {
        return false;
    }
    let target = dir.join(HEARTBEAT_FILE);
    let ours = fs::read_to_string(&target)
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .is_some_and(|p| p == pid.to_string());
    ours && fs::remove_file(&target).is_ok()
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

fn voice_dir() -> PathBuf {
    voice_paths().dir
}

/// Start the 5 s heartbeat. Called once from the app's `setup`.
pub fn start() {
    let pid = std::process::id();
    let _ = std::thread::Builder::new()
        .name("voice-heartbeat".into())
        .spawn(move || loop {
            beat_in(&voice_dir(), current_uid(), pid, unix_ms());
            std::thread::sleep(HEARTBEAT_EVERY);
        });
}

/// Called on `RunEvent::Exit`.
pub fn stop() {
    remove_in(&voice_dir(), current_uid(), std::process::id());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("amara-hb-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        d
    }

    #[test]
    fn heartbeat_is_written_and_updated_with_a_fake_clock() {
        let d = tmpdir("write");
        let uid = current_uid();
        assert!(beat_in(&d, uid, 4242, 1_000));
        let f = d.join(HEARTBEAT_FILE);
        assert_eq!(fs::read_to_string(&f).unwrap(), "4242 1000\n");
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(beat_in(&d, uid, 4242, 6_000));
        assert_eq!(fs::read_to_string(&f).unwrap(), "4242 6000\n");
        assert!(!d.join(HEARTBEAT_TMP).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn heartbeat_is_removed_on_exit_only_if_it_is_ours() {
        let d = tmpdir("remove");
        let uid = current_uid();
        assert!(beat_in(&d, uid, 4242, 1_000));
        assert!(!remove_in(&d, uid, 99), "another pid's file stays");
        assert!(d.join(HEARTBEAT_FILE).exists());
        assert!(remove_in(&d, uid, 4242));
        assert!(!d.join(HEARTBEAT_FILE).exists());
        assert!(!remove_in(&d, uid, 4242), "already gone is fine");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn heartbeat_refuses_an_unsafe_dir() {
        let uid = current_uid();
        // group/other writable
        let d = tmpdir("unsafe");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!beat_in(&d, uid, 1, 1));
        assert!(!d.join(HEARTBEAT_FILE).exists());
        // wrong owner
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!beat_in(&d, uid.wrapping_add(1), 1, 1));
        // symlinked dir
        let real = tmpdir("real");
        let link = std::env::temp_dir().join(format!("amara-hb-link-{}", std::process::id()));
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(!beat_in(&link, uid, 1, 1));
        assert!(!real.join(HEARTBEAT_FILE).exists());
        // missing dir is not created
        assert!(!beat_in(&d.join("nope"), uid, 1, 1));
        assert!(!d.join("nope").exists());
        let _ = fs::remove_file(&link);
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&real);
    }

    #[test]
    fn heartbeat_replaces_a_symlink_at_the_target_without_following_it() {
        let d = tmpdir("tgt");
        let victim = d.join("victim");
        fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, d.join(HEARTBEAT_FILE)).unwrap();
        assert!(beat_in(&d, current_uid(), 7, 5));
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
        assert_eq!(fs::read_to_string(d.join(HEARTBEAT_FILE)).unwrap(), "7 5\n");
        let _ = fs::remove_dir_all(&d);
    }
}
