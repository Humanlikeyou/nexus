//! Update install order (handover Phase 15 step 3), as pure code over a host trait so the order,
//! the abort points and the rollback are testable without touching a real app, sidecar or network.
//!
//! Order: verify signature -> pre-update backup -> wait for crews and Muse -> stop sidecars ->
//! swap -> start -> health check -> (on any failure after the sidecars stop) rollback.
//!
//! Nothing here is wired to a real host yet (`updater::update_install_now` refuses until Taisun's
//! key and manifest host exist). A Muse recording in progress makes `wait_idle` return an error, so
//! the install aborts before anything is stopped (recordings are sacred).

#![cfg_attr(not(test), allow(dead_code))] // inert until the Phase 15 gate wires a real host

use base64::Engine;
use minisign_verify::{PublicKey, Signature};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Verify,
    TeamId,
    Backup,
    WaitIdle,
    StopSidecars,
    Swap,
    Start,
    HealthCheck,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// Nothing was changed on disk or stopped.
    Aborted {
        step: Step,
        reason: String,
    },
    /// The new version failed after the old one was stopped; the previous app was restored.
    RolledBack {
        failed: Step,
        reason: String,
    },
    /// Rollback itself failed. Needs a person; the backup from step 2 is the way back.
    RollbackFailed {
        failed: Step,
        reason: String,
        rollback_error: String,
    },
}

#[derive(Debug)]
pub struct InstallReport {
    pub outcome: Outcome,
    pub steps: Vec<Step>,
}

pub trait InstallHost {
    /// The staged new app must be validly signed by the SAME codesign Team ID as the running app
    /// (`codesign -dv` TeamIdentifier). Runs before anything is changed; Err aborts the install.
    fn check_team_id(&mut self) -> Result<(), String>;
    fn backup(&mut self) -> Result<(), String>;
    /// Block until no crew run or Muse recording is active, or return an error on timeout.
    fn wait_idle(&mut self) -> Result<(), String>;
    fn stop_sidecars(&mut self) -> Result<(), String>;
    /// Put the new app in place, keeping the previous one aside for rollback.
    fn swap(&mut self) -> Result<(), String>;
    fn start(&mut self) -> Result<(), String>;
    fn health_check(&mut self) -> Result<(), String>;
    /// Restore the previous app and start it.
    fn rollback(&mut self) -> Result<(), String>;
}

fn b64_text(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|_| "bad base64".to_string())?;
    String::from_utf8(bytes).map_err(|_| "not utf-8".to_string())
}

/// Minisign verification in the same format Tauri's updater uses (public key and signature are
/// each the base64 of the minisign text). Refuses on any decode error: fail closed.
pub fn verify_artifact(pubkey_b64: &str, sig_b64: &str, data: &[u8]) -> Result<(), String> {
    let pk = PublicKey::decode(&b64_text(pubkey_b64)?).map_err(|_| "bad public key".to_string())?;
    let sig = Signature::decode(&b64_text(sig_b64)?).map_err(|_| "bad signature".to_string())?;
    pk.verify(data, &sig, true)
        .map_err(|_| "signature does not match".to_string())
}

/// The candidate must be strictly newer (semver) than the running version: no re-install, no
/// downgrade, and unparsable versions fail closed.
pub fn check_newer(current: &str, candidate: &str) -> Result<(), String> {
    let cur = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|_| "bad current version".to_string())?;
    let new = semver::Version::parse(candidate.trim_start_matches('v'))
        .map_err(|_| "bad update version".to_string())?;
    if new > cur {
        Ok(())
    } else {
        Err("update is not newer".into())
    }
}

pub fn valid_pubkey(pubkey_b64: &str) -> bool {
    b64_text(pubkey_b64).is_ok_and(|t| PublicKey::decode(&t).is_ok())
}

pub fn run_install(
    host: &mut dyn InstallHost,
    artifact: &[u8],
    sig_b64: &str,
    pubkey_b64: &str,
    current_version: &str,
    artifact_version: &str,
) -> InstallReport {
    let mut steps = vec![Step::Verify];
    if let Err(reason) = check_newer(current_version, artifact_version)
        .and_then(|()| verify_artifact(pubkey_b64, sig_b64, artifact))
    {
        return InstallReport {
            outcome: Outcome::Aborted {
                step: Step::Verify,
                reason,
            },
            steps,
        };
    }
    // Before the sidecars stop, a failure changes nothing: abort.
    macro_rules! pre {
        ($step:expr, $call:expr) => {{
            steps.push($step);
            if let Err(reason) = $call {
                return InstallReport {
                    outcome: Outcome::Aborted {
                        step: $step,
                        reason,
                    },
                    steps,
                };
            }
        }};
    }
    pre!(Step::TeamId, host.check_team_id());
    pre!(Step::Backup, host.backup());
    pre!(Step::WaitIdle, host.wait_idle());
    // From here on the old app may be down: a failure rolls back.
    macro_rules! post {
        ($step:expr, $call:expr) => {{
            steps.push($step);
            if let Err(reason) = $call {
                steps.push(Step::Rollback);
                let outcome = match host.rollback() {
                    Ok(()) => Outcome::RolledBack {
                        failed: $step,
                        reason,
                    },
                    Err(rollback_error) => Outcome::RollbackFailed {
                        failed: $step,
                        reason,
                        rollback_error,
                    },
                };
                return InstallReport { outcome, steps };
            }
        }};
    }
    post!(Step::StopSidecars, host.stop_sidecars());
    post!(Step::Swap, host.swap());
    post!(Step::Start, host.start());
    post!(Step::HealthCheck, host.health_check());
    InstallReport {
        outcome: Outcome::Installed,
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Public test vector from the minisign-verify crate's own tests: signs the bytes b"test".
    // It is a public key and a signature, not a secret, and not Amara's key.
    const VEC_PUB: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const VEC_SIG: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[derive(Default)]
    struct Fake {
        log: Vec<&'static str>,
        fail: Option<&'static str>,
        rollback_fails: bool,
    }
    impl Fake {
        fn step(&mut self, name: &'static str) -> Result<(), String> {
            self.log.push(name);
            if self.fail == Some(name) {
                Err(format!("{name} failed"))
            } else {
                Ok(())
            }
        }
    }
    impl InstallHost for Fake {
        fn check_team_id(&mut self) -> Result<(), String> {
            self.step("team_id")
        }
        fn backup(&mut self) -> Result<(), String> {
            self.step("backup")
        }
        fn wait_idle(&mut self) -> Result<(), String> {
            self.step("wait_idle")
        }
        fn stop_sidecars(&mut self) -> Result<(), String> {
            self.step("stop_sidecars")
        }
        fn swap(&mut self) -> Result<(), String> {
            self.step("swap")
        }
        fn start(&mut self) -> Result<(), String> {
            self.step("start")
        }
        fn health_check(&mut self) -> Result<(), String> {
            self.step("health_check")
        }
        fn rollback(&mut self) -> Result<(), String> {
            self.log.push("rollback");
            if self.rollback_fails {
                Err("disk full".into())
            } else {
                Ok(())
            }
        }
    }

    fn run(f: &mut Fake) -> InstallReport {
        run_install(f, b"test", &b64(VEC_SIG), &b64(VEC_PUB), "0.1.0", "0.2.0")
    }

    #[test]
    fn happy_path_runs_in_the_documented_order() {
        let mut f = Fake::default();
        let r = run(&mut f);
        assert_eq!(r.outcome, Outcome::Installed);
        assert_eq!(
            f.log,
            [
                "team_id",
                "backup",
                "wait_idle",
                "stop_sidecars",
                "swap",
                "start",
                "health_check"
            ]
        );
    }

    #[test]
    fn bad_signature_is_refused_and_the_host_is_never_touched() {
        let mut f = Fake::default();
        let r = run_install(
            &mut f,
            b"tampered",
            &b64(VEC_SIG),
            &b64(VEC_PUB),
            "0.1.0",
            "0.2.0",
        );
        assert!(matches!(
            r.outcome,
            Outcome::Aborted {
                step: Step::Verify,
                ..
            }
        ));
        assert!(f.log.is_empty(), "no backup, no stop, no swap");
    }

    #[test]
    fn garbage_key_or_signature_fails_closed() {
        let mut f = Fake::default();
        for (k, s) in [
            ("not base64!".to_string(), b64(VEC_SIG)),
            (b64(VEC_PUB), String::new()),
            (String::new(), b64(VEC_SIG)),
        ] {
            let r = run_install(&mut f, b"test", &s, &k, "0.1.0", "0.2.0");
            assert!(matches!(
                r.outcome,
                Outcome::Aborted {
                    step: Step::Verify,
                    ..
                }
            ));
        }
        assert!(f.log.is_empty());
    }

    #[test]
    fn backup_runs_before_the_swap_and_a_failed_backup_stops_everything() {
        let mut f = Fake::default();
        run(&mut f);
        let pos = |n| f.log.iter().position(|x| *x == n).unwrap();
        assert!(pos("backup") < pos("stop_sidecars") && pos("backup") < pos("swap"));

        let mut f = Fake {
            fail: Some("backup"),
            ..Default::default()
        };
        let r = run(&mut f);
        assert!(matches!(
            r.outcome,
            Outcome::Aborted {
                step: Step::Backup,
                ..
            }
        ));
        assert_eq!(f.log, ["team_id", "backup"]);
    }

    #[test]
    fn busy_crews_or_muse_abort_before_anything_is_stopped() {
        let mut f = Fake {
            fail: Some("wait_idle"),
            ..Default::default()
        };
        let r = run(&mut f);
        assert!(matches!(
            r.outcome,
            Outcome::Aborted {
                step: Step::WaitIdle,
                ..
            }
        ));
        assert_eq!(f.log, ["team_id", "backup", "wait_idle"]);
    }

    #[test]
    fn forced_failed_health_check_rolls_back() {
        let mut f = Fake {
            fail: Some("health_check"),
            ..Default::default()
        };
        let r = run(&mut f);
        assert!(matches!(
            r.outcome,
            Outcome::RolledBack {
                failed: Step::HealthCheck,
                ..
            }
        ));
        assert_eq!(f.log.last(), Some(&"rollback"));
        assert_eq!(r.steps.last(), Some(&Step::Rollback));
    }

    #[test]
    fn failure_after_the_sidecars_stop_always_rolls_back() {
        for name in ["stop_sidecars", "swap", "start"] {
            let mut f = Fake {
                fail: Some(name),
                ..Default::default()
            };
            let r = run(&mut f);
            assert!(matches!(r.outcome, Outcome::RolledBack { .. }), "{name}");
            assert_eq!(f.log.last(), Some(&"rollback"), "{name}");
        }
    }

    #[test]
    fn a_failed_rollback_is_reported_not_hidden() {
        let mut f = Fake {
            fail: Some("health_check"),
            rollback_fails: true,
            ..Default::default()
        };
        let r = run(&mut f);
        assert!(matches!(r.outcome, Outcome::RollbackFailed { .. }));
    }

    #[test]
    fn same_or_older_or_garbage_versions_abort_at_verify_with_a_valid_signature() {
        for (cur, new) in [
            ("0.2.0", "0.2.0"),
            ("0.2.0", "0.1.9"),
            ("1.0.0", "1.0.0-rc.1"),
            ("0.1.0", "not-a-version"),
            ("", "0.2.0"),
        ] {
            let mut f = Fake::default();
            let r = run_install(&mut f, b"test", &b64(VEC_SIG), &b64(VEC_PUB), cur, new);
            assert!(
                matches!(
                    r.outcome,
                    Outcome::Aborted {
                        step: Step::Verify,
                        ..
                    }
                ),
                "{cur} -> {new}"
            );
            assert!(f.log.is_empty(), "{cur} -> {new}");
        }
        let mut f = Fake::default();
        let r = run_install(
            &mut f,
            b"test",
            &b64(VEC_SIG),
            &b64(VEC_PUB),
            "0.9.0",
            "v0.10.0",
        );
        assert_eq!(
            r.outcome,
            Outcome::Installed,
            "semver, not string, ordering"
        );
    }

    #[test]
    fn a_team_id_mismatch_aborts_before_the_backup_or_any_stop() {
        let mut f = Fake {
            fail: Some("team_id"),
            ..Default::default()
        };
        let r = run(&mut f);
        assert!(matches!(
            r.outcome,
            Outcome::Aborted {
                step: Step::TeamId,
                ..
            }
        ));
        assert_eq!(f.log, ["team_id"]);
    }
}
