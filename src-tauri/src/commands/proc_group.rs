//! Signalling a whole process group the app itself started (`process_group(0)` makes the group id
//! equal to the child's pid). Used only for the bundled runtime's serve and the download window:
//! `ollama serve` forks runner children, so signalling the pid alone would orphan them. A group id
//! at or below 1 is never signalled, and callers pass only the pid of a child they spawned.

use std::time::Duration;

/// Escalation delay from SIGTERM to SIGKILL.
pub const FORCE_AFTER: Duration = Duration::from_secs(2);

fn valid(pgid: u32) -> bool {
    pgid > 1 && i32::try_from(pgid).is_ok()
}

/// True when no process is left in the group (or the id is not a usable group).
pub fn group_empty(pgid: u32) -> bool {
    if !valid(pgid) {
        return true;
    }
    // SAFETY: signal 0 only checks that the group exists; it delivers nothing.
    let r = unsafe { libc::kill(-(pgid as i32), 0) };
    r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn signal(pgid: u32, sig: i32) -> bool {
    if !valid(pgid) {
        return false;
    }
    // SAFETY: a plain killpg on a group the caller created with `process_group(0)`.
    unsafe { libc::killpg(pgid as i32, sig) == 0 }
}

/// SIGKILL to the whole group now (used when a group outlives its stop deadline).
pub fn force(pgid: u32) {
    signal(pgid, libc::SIGKILL);
}

/// SIGTERM now; a detached thread sends SIGKILL after `FORCE_AFTER` if the group is still there.
/// Safe to call from `Drop`. Idempotent.
pub fn terminate(pgid: u32) {
    if !signal(pgid, libc::SIGTERM) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FORCE_AFTER);
        if !group_empty(pgid) {
            signal(pgid, libc::SIGKILL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    #[test]
    fn invalid_group_ids_are_never_signalled() {
        for g in [0u32, 1, u32::MAX] {
            assert!(!signal(g, 0), "{g}");
            assert!(group_empty(g));
        }
    }

    #[test]
    fn terminate_ends_the_whole_group_including_children() {
        // A shell in its own group that starts a background child, both harmless sleeps.
        let mut c = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 30 & /bin/sleep 30; wait"])
            .env_clear()
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = c.id();
        assert!(!group_empty(pgid));
        terminate(pgid);
        let _ = c.wait(); // reap the leader; the group must still be judged by its members
        let start = std::time::Instant::now();
        while !group_empty(pgid) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(group_empty(pgid), "group still has members");
    }
}
