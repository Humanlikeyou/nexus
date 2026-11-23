//! `service_restart(id)`: restart ONE service from Settings > Services. An ACTION, so every call goes
//! through the native approval dialog (`approval_gate`), exactly like `restart_services`.
//!
//! Scope, deliberately small:
//! - Only Docker containers (`docker restart`, the same `system::docker_restart` that `restart_services.rs`
//!   uses): n8n, postgres, letta, verba, crew. Each name is verified in `stack/docker-compose.yml`, see
//!   `container_for`. Ids are the system-map node ids the Services rows use.
//! - Everything else the map knows (router, voice, hermes: LaunchAgent-managed; models, crews, memory,
//!   health, home, backups) returns `unsupported`. Unknown ids return `unknown_service`.
//! - Phase 8 (supervisor lane S4): the docker restarts now go through the supervisor `Runner` (the
//!   allowlisted verb enum, names from the registry), and two supervisor ids are added, `amara-router`
//!   and `hermes-gateway`, restarted by a launchd kickstart of a job that is already loaded (never
//!   loaded or unloaded here). Same native approval, same try budget, same error codes. A manual
//!   restart is a user action: it works with the supervisor's act switch off, and a success resets the
//!   supervisor's attempt counter for that service. There is deliberately no start/stop command yet.
//! - Nothing is restarted unless the user clicks Allow in the dialog. Deny, busy and refused change
//!   nothing and do not count as a try. No `kill`, no signals, no network.
//! - Max 3 consecutive failed tries per service in this launch; the 4th is refused with `needs_you:`
//!   without a dialog (the UI treats that as "Needs you"). A success clears the count.
//!
//! Error strings (stable codes; Docker's own output is never passed on):
//! `unknown_service`, `unsupported`, `needs_you: ...`, `restart_failed`, plus the approval gate's
//! `declined:` / `busy:` / `refused:` prefixes.

use super::approval_gate;
use super::supervisor::registry::ServiceId;
use super::supervisor::runner::{self, RealRunner, RunError, RunPolicy, Runner, Verb};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Consecutive failed restarts before the service becomes "Needs you".
pub const MAX_TRIES: u8 = 3;

pub const UNKNOWN_SERVICE: &str = "unknown_service";
pub const UNSUPPORTED: &str = "unsupported";
pub const RESTART_FAILED: &str = "restart_failed";
pub const NEEDS_YOU_PREFIX: &str = "needs_you:";

/// Node ids of `system_map.rs` NODES that have a service behind them (app/external/Telegram excluded).
pub(crate) const KNOWN_IDS: &[&str] = &[
    "router", "ollama", "hermes", "noop", "muse", "crew", "n8n", "postgres", "letta", "verba",
    "ha", "voice", "backups",
];

/// Restartable ids and their Docker containers. `stack/docker-compose.yml` sets no `container_name`
/// for any service, so Compose names each container `<project>-<service>-1`, where the project is the
/// directory name `stack` (the same rule behind `stack-n8n-1` and `stack-postgres-1`). Verified against
/// the compose services (`letta` line 91, `verba` line 154, `crew` line 184) and `docker ps` on this Mac.
/// Anything not listed here stays `unsupported`.
pub(crate) fn container_for(id: &str) -> Option<&'static str> {
    match id {
        "n8n" => Some("stack-n8n-1"),
        "postgres" => Some("stack-postgres-1"),
        "letta" => Some("stack-letta-1"),
        "verba" => Some("stack-verba-1"),
        "crew" => Some("stack-crew-1"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRestartDone {
    pub id: String,
    /// Failed tries so far for this service (0 after a success).
    pub failed_tries: u8,
}

type Tries = Mutex<HashMap<String, u8>>;
static TRIES: LazyLock<Tries> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Id check + try budget, before any dialog.
fn pre_check(id: &str, tries: &Tries) -> Result<&'static str, String> {
    if !KNOWN_IDS.contains(&id) {
        return Err(UNKNOWN_SERVICE.to_string());
    }
    let container = container_for(id).ok_or_else(|| UNSUPPORTED.to_string())?;
    check_budget(id, tries)?;
    Ok(container)
}

/// The try budget: 3 consecutive failed tries, then `needs_you:` without a dialog.
fn check_budget(id: &str, tries: &Tries) -> Result<(), String> {
    let used = *tries
        .lock()
        .map_err(|_| RESTART_FAILED.to_string())?
        .get(id)
        .unwrap_or(&0);
    if used >= MAX_TRIES {
        return Err(format!(
            "{NEEDS_YOU_PREFIX} restart tried {MAX_TRIES} times and it is still not working. Amara won't try again on its own."
        ));
    }
    Ok(())
}

/// The supervisor id behind a restartable system-map id (Verba is a container of the Weaviate row).
fn supervisor_id_for(id: &str) -> Option<ServiceId> {
    match id {
        "n8n" => Some(ServiceId::N8n),
        "postgres" => Some(ServiceId::Postgres),
        "letta" => Some(ServiceId::Letta),
        "verba" => Some(ServiceId::Weaviate),
        "crew" => Some(ServiceId::CrewService),
        _ => None,
    }
}

/// The supervisor ids that restart through a launchd kickstart.
fn launchd_id(id: &str) -> Option<ServiceId> {
    match ServiceId::parse(id) {
        Some(s @ (ServiceId::AmaraRouter | ServiceId::HermesGateway)) => Some(s),
        _ => None,
    }
}

/// One docker restart through the runner. Failures collapse to the fixed `restart_failed` code
/// upstream; no output is passed on.
async fn restart_container_via(
    runner: &dyn Runner,
    sid: ServiceId,
    container: &'static str,
) -> Result<String, String> {
    runner
        .run(Verb::DockerRestartOne(sid, container))
        .await
        .map(|_| format!("Container {container} restarted"))
        .map_err(|_| RESTART_FAILED.to_string())
}

/// `service_restart` for `amara-router` / `hermes-gateway`: same flow as `restart_core` (id check, try
/// budget, native approval held across the run, success clears the count) with a kickstart verb.
async fn restart_launchd_core<Ap, ApFut, G, R, Fut>(
    id: &str,
    tries: &Tries,
    uid: u32,
    approve: Ap,
    run: R,
) -> Result<ServiceRestartDone, String>
where
    Ap: FnOnce(Vec<String>, String) -> ApFut,
    ApFut: std::future::Future<Output = Result<G, String>>,
    R: FnOnce(Verb) -> Fut,
    Fut: std::future::Future<Output = Result<(), RunError>>,
{
    let sid = launchd_id(id).ok_or_else(|| UNKNOWN_SERVICE.to_string())?;
    check_budget(id, tries)?;
    let (operation, detail) = runner::kickstart_prompt(sid, uid).ok_or(UNSUPPORTED.to_string())?;
    let key = runner::kickstart_key(sid).ok_or(UNSUPPORTED.to_string())?;
    let _approval = approve(
        vec![key],
        approval_gate::approval_text(&operation, &detail)?,
    )
    .await?;
    let outcome = run(Verb::LaunchctlKickstartK(sid)).await;
    let mut map = tries.lock().map_err(|_| RESTART_FAILED.to_string())?;
    match outcome {
        Ok(()) => {
            map.remove(id);
            Ok(ServiceRestartDone {
                id: id.to_string(),
                failed_tries: 0,
            })
        }
        Err(_) => {
            *map.entry(id.to_string()).or_insert(0) += 1;
            Err(RESTART_FAILED.to_string())
        }
    }
}

fn dialog_text(container: &str) -> Result<String, String> {
    let (operation, detail) = super::system::docker_restart_prompt(container);
    approval_gate::approval_text(&operation, &detail)
}

/// The whole flow with the gate and the runner injected (tests use fakes).
/// `approve(keys, text)` must return only after an Allow; its guard is held across the restart.
async fn restart_core<Ap, ApFut, G, R, Fut>(
    id: &str,
    tries: &Tries,
    approve: Ap,
    run: R,
) -> Result<ServiceRestartDone, String>
where
    Ap: FnOnce(Vec<String>, String) -> ApFut,
    ApFut: std::future::Future<Output = Result<G, String>>,
    R: FnOnce(&'static str) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let container = pre_check(id, tries)?;
    let keys = vec![approval_gate::service_key(&format!("docker:{container}"))];
    let _approval = approve(keys, dialog_text(container)?).await?;
    let outcome = run(container).await;
    let mut map = tries.lock().map_err(|_| RESTART_FAILED.to_string())?;
    match outcome {
        Ok(_) => {
            map.remove(id);
            Ok(ServiceRestartDone {
                id: id.to_string(),
                failed_tries: 0,
            })
        }
        Err(_) => {
            *map.entry(id.to_string()).or_insert(0) += 1;
            Err(RESTART_FAILED.to_string())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn service_restart(
    app: tauri::AppHandle,
    id: String,
) -> Result<ServiceRestartDone, String> {
    // A manual restart is the user's own action: it runs with the supervisor's act switch off, and
    // the runner only ever gets the restart verbs.
    let runner = RealRunner::new(RunPolicy::ManualRestart);
    let approve = |keys: Vec<String>, text: String| {
        let app = app.clone();
        async move { approval_gate::require_approval(&app, &keys, text).await }
    };
    if let Some(sid) = launchd_id(&id) {
        let done = restart_launchd_core(&id, &TRIES, runner::current_uid(), approve, |verb| {
            runner.run(verb)
        })
        .await?;
        super::supervisor::manual_restart_done(sid);
        return Ok(done);
    }
    let sid = supervisor_id_for(&id);
    let runner_ref = &runner;
    let done = restart_core(&id, &TRIES, approve, |container| async move {
        match sid {
            Some(s) => restart_container_via(runner_ref, s, container).await,
            None => Err(RESTART_FAILED.to_string()),
        }
    })
    .await?;
    if let Some(s) = sid {
        super::supervisor::manual_restart_done(s);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn fresh() -> Tries {
        Mutex::new(HashMap::new())
    }

    #[test]
    fn known_ids_are_the_system_map_nodes() {
        let src = include_str!("system_map.rs");
        for id in KNOWN_IDS {
            assert!(
                src.contains(&format!("(\"{id}\",")),
                "{id} not in system_map"
            );
        }
        // Only docker containers are restartable; LaunchAgent-managed services are not.
        for id in [
            "router", "voice", "hermes", "ollama", "ha", "noop", "muse", "backups",
        ] {
            assert_eq!(container_for(id), None, "{id}");
        }
        assert_eq!(container_for("n8n"), Some("stack-n8n-1"));
        assert_eq!(container_for("postgres"), Some("stack-postgres-1"));
    }

    #[test]
    fn letta_verba_crew_map_to_their_verified_compose_container_names() {
        assert_eq!(container_for("letta"), Some("stack-letta-1"));
        assert_eq!(container_for("verba"), Some("stack-verba-1"));
        assert_eq!(container_for("crew"), Some("stack-crew-1"));
    }

    #[tokio::test]
    async fn letta_verba_crew_still_need_an_approval_every_call() {
        for (id, container) in [
            ("letta", "stack-letta-1"),
            ("verba", "stack-verba-1"),
            ("crew", "stack-crew-1"),
        ] {
            let t = fresh();
            let asked: RefCell<Vec<(Vec<String>, String)>> = Default::default();
            let done = restart_core(
                id,
                &t,
                |k, text| {
                    asked.borrow_mut().push((k, text));
                    async { Ok(()) }
                },
                |c| async move { Ok(format!("Container {c} restarted")) },
            )
            .await
            .unwrap();
            assert_eq!(done.id, id);
            assert_eq!(asked.borrow()[0].0, vec![format!("svc:docker:{container}")]);
            assert!(asked.borrow()[0]
                .1
                .starts_with(&format!("Run: docker restart {container}")));
            let err = restart_core(
                id,
                &t,
                |_, _| async move { Err::<(), _>("declined: no".to_string()) },
                |_| async { panic!("not approved") },
            )
            .await
            .unwrap_err();
            assert!(err.starts_with("declined:"), "{err}");
        }
    }

    #[tokio::test]
    async fn inputs_are_validated_before_any_dialog() {
        let t = fresh();
        for (id, want) in [
            ("", UNKNOWN_SERVICE),
            ("nginx", UNKNOWN_SERVICE),
            ("docker:n8n", UNKNOWN_SERVICE),
            ("n8n; rm -rf /", UNKNOWN_SERVICE),
            ("../n8n", UNKNOWN_SERVICE),
            ("openclaw", UNKNOWN_SERVICE),
            ("router", UNSUPPORTED),
            ("voice", UNSUPPORTED),
        ] {
            let asked = Cell::new(0);
            let err = restart_core(
                id,
                &t,
                |_, _| {
                    asked.set(asked.get() + 1);
                    async { Ok(()) }
                },
                |_| async { panic!("must not run") },
            )
            .await
            .unwrap_err();
            assert_eq!(err, want, "{id:?}");
            assert_eq!(asked.get(), 0, "no dialog for {id:?}");
        }
    }

    #[tokio::test]
    async fn approval_is_required_every_time_and_denial_runs_nothing() {
        let t = fresh();
        let asked: RefCell<Vec<(Vec<String>, String)>> = Default::default();
        // Two successful restarts: the gate is asked twice.
        for _ in 0..2 {
            let done = restart_core(
                "n8n",
                &t,
                |k, text| {
                    asked.borrow_mut().push((k, text));
                    async { Ok(()) }
                },
                |c| async move { Ok(format!("Container {c} restarted")) },
            )
            .await
            .unwrap();
            assert_eq!(done.failed_tries, 0);
        }
        assert_eq!(asked.borrow().len(), 2);
        assert_eq!(
            asked.borrow()[0].0,
            vec!["svc:docker:stack-n8n-1".to_string()]
        );
        assert!(asked.borrow()[0]
            .1
            .starts_with("Run: docker restart stack-n8n-1"));
        for prefix in ["declined:", "busy:", "refused:"] {
            let err = restart_core(
                "n8n",
                &t,
                |_, _| async move { Err::<(), _>(format!("{prefix} x")) },
                |_| async { panic!("not approved") },
            )
            .await
            .unwrap_err();
            assert!(err.starts_with(prefix), "{err}");
        }
        // Denials never count as a try.
        assert!(t.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn three_failures_then_needs_you_without_a_dialog_and_success_resets() {
        let t = fresh();
        for n in 1..=MAX_TRIES {
            let err = restart_core(
                "postgres",
                &t,
                |_, _| async { Ok(()) },
                |_| async { Err("docker said something long and private".to_string()) },
            )
            .await
            .unwrap_err();
            assert_eq!(err, RESTART_FAILED, "try {n}");
        }
        let asked = Cell::new(0);
        let err = restart_core(
            "postgres",
            &t,
            |_, _| {
                asked.set(1);
                async { Ok(()) }
            },
            |_| async { panic!("must not run") },
        )
        .await
        .unwrap_err();
        assert!(err.starts_with(NEEDS_YOU_PREFIX), "{err}");
        assert_eq!(asked.get(), 0);
        // Another service is unaffected; a success clears its own count.
        let _ = restart_core(
            "n8n",
            &t,
            |_, _| async { Ok(()) },
            |_| async { Err("x".into()) },
        )
        .await;
        restart_core(
            "n8n",
            &t,
            |_, _| async { Ok(()) },
            |c| async move { Ok(c.into()) },
        )
        .await
        .unwrap();
        assert!(!t.lock().unwrap().contains_key("n8n"));
    }

    #[test]
    fn source_has_no_kill_launchctl_or_network() {
        let code = include_str!("service_control.rs");
        let code = code.split("#[cfg(test)]").next().unwrap();
        let code: String = code
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "kill",
            "launchctl",
            "SIGKILL",
            "reqwest",
            "TcpStream",
            "-9",
            "LaunchAgents",
            "Command::new",
        ] {
            assert!(!code.contains(banned), "{banned}");
        }
        assert!(code.contains("require_approval"));
    }

    // ---- Phase 8 / S4: runner-routed restarts and the two launchd ids ----

    use crate::commands::supervisor::registry::Registry;
    use crate::commands::supervisor::runner::fakes::{argv, FakeExec};

    fn manual_runner() -> RealRunner<FakeExec> {
        RealRunner::with(
            FakeExec::default(),
            Registry::builtin(),
            RunPolicy::ManualRestart,
            501,
        )
    }

    #[tokio::test]
    async fn every_docker_id_restarts_only_its_own_container_through_the_runner() {
        for (id, container) in [
            ("n8n", "stack-n8n-1"),
            ("postgres", "stack-postgres-1"),
            ("letta", "stack-letta-1"),
            ("verba", "stack-verba-1"),
            ("crew", "stack-crew-1"),
        ] {
            let r = manual_runner();
            let sid = supervisor_id_for(id).unwrap();
            let t = fresh();
            let done = restart_core(
                id,
                &t,
                |_, _| async { Ok(()) },
                |c| async move { restart_container_via(&r, sid, c).await },
            )
            .await
            .unwrap();
            assert_eq!(done.id, id);
            // One container, never the whole Weaviate row when only Verba was asked for.
            let r2 = manual_runner();
            restart_container_via(&r2, sid, container).await.unwrap();
            assert_eq!(argv(&r2.exec), vec![format!("Docker restart {container}")]);
        }
    }

    #[tokio::test]
    async fn a_runner_failure_is_restart_failed_with_no_output_and_counts_a_try() {
        let r = manual_runner();
        *r.exec.docker_fail.lock().unwrap() =
            Some(crate::commands::supervisor::runner::StderrClass::Other);
        let t = fresh();
        let err = restart_core(
            "n8n",
            &t,
            |_, _| async { Ok(()) },
            |c| async move { restart_container_via(&r, ServiceId::N8n, c).await },
        )
        .await
        .unwrap_err();
        assert_eq!(err, RESTART_FAILED);
        assert_eq!(*t.lock().unwrap().get("n8n").unwrap(), 1);
    }

    #[tokio::test]
    async fn router_and_hermes_restart_by_kickstart_with_the_same_approval_and_budget() {
        for (id, label) in [
            ("amara-router", "com.taisun.amara-router"),
            ("hermes-gateway", "ai.hermes.gateway"),
        ] {
            let r = manual_runner();
            r.exec.loaded.lock().unwrap().push(label.to_string());
            let t = fresh();
            let asked: RefCell<Vec<(Vec<String>, String)>> = Default::default();
            let done = restart_launchd_core(
                id,
                &t,
                501,
                |k, text| {
                    asked.borrow_mut().push((k, text));
                    async { Ok(()) }
                },
                |v| r.run(v),
            )
            .await
            .unwrap();
            assert_eq!(done.id, id);
            assert_eq!(asked.borrow()[0].0, vec![format!("svc:launchd:{label}")]);
            assert_eq!(
                asked.borrow()[0].1.lines().next().unwrap(),
                format!("Run: launchctl kickstart -k gui/501/{label}")
            );
            assert_eq!(
                argv(&r.exec),
                vec![
                    format!("Launchctl list {label}"),
                    format!("Launchctl kickstart -k gui/501/{label}")
                ]
            );
            // Every call asks again, and a denial runs nothing and costs no try.
            let denied = restart_launchd_core(
                id,
                &t,
                501,
                |_, _| async { Err::<(), _>("declined: no".to_string()) },
                |_| async { panic!("not approved") },
            )
            .await
            .unwrap_err();
            assert!(denied.starts_with("declined:"));
            assert!(t.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn a_launchd_job_that_is_not_loaded_is_a_failed_try_and_three_make_needs_you() {
        let r = manual_runner(); // nothing loaded
        let t = fresh();
        for _ in 0..MAX_TRIES {
            let err = restart_launchd_core(
                "amara-router",
                &t,
                501,
                |_, _| async { Ok(()) },
                |v| r.run(v),
            )
            .await
            .unwrap_err();
            assert_eq!(err, RESTART_FAILED);
        }
        // The kickstart itself never ran.
        assert!(argv(&r.exec)
            .iter()
            .all(|a| a.starts_with("Launchctl list")));
        let err = restart_launchd_core(
            "amara-router",
            &t,
            501,
            |_, _| async { Err::<(), String>("no dialog expected".into()) },
            |_| async { panic!("must not run") },
        )
        .await
        .unwrap_err();
        assert!(err.starts_with(NEEDS_YOU_PREFIX), "{err}");
    }

    #[tokio::test]
    async fn only_the_two_launchd_ids_take_that_path_and_the_old_codes_are_unchanged() {
        let t = fresh();
        for id in [
            "",
            "router",
            "voice",
            "hermes",
            "amara-voice",
            "open-webui",
            "n8n",
            "docker",
        ] {
            let err = restart_launchd_core(
                id,
                &t,
                501,
                |_, _| async { Err::<(), String>("no dialog expected".into()) },
                |_| async { panic!("must not run") },
            )
            .await
            .unwrap_err();
            assert_eq!(err, UNKNOWN_SERVICE, "{id:?}");
        }
        // The system-map ids keep their old answers on the docker path.
        for id in ["router", "voice", "hermes"] {
            let e = restart_core(
                id,
                &t,
                |_, _| async { Ok(()) },
                |_| async { panic!("must not run") },
            )
            .await
            .unwrap_err();
            assert_eq!(e, UNSUPPORTED);
        }
        assert!(launchd_id("amara-router").is_some() && launchd_id("hermes-gateway").is_some());
        assert!(launchd_id("amara-voice").is_none() && launchd_id("open-webui").is_none());
    }

    #[test]
    fn the_command_routes_through_the_runner_with_the_manual_policy() {
        let code = include_str!("service_control.rs");
        let code = code.split("#[cfg(test)]").next().unwrap();
        assert!(code.contains("RealRunner::new(RunPolicy::ManualRestart)"));
        assert!(!code.contains("docker_restart("));
        assert!(code.contains("manual_restart_done"));
        // No start or stop command exists in this lane.
        assert!(!code.contains("fn service_start") && !code.contains("fn service_stop"));
    }
}
