//! `services_start_all`: the first-run "start everything" step. With act not effective (today:
//! `SUPERVISOR_ACT_ENABLED` is false) it only looks: rows already healthy are `adopted`, the two
//! sidecar rows are `skipped_act_off`, and nothing is ever spawned. With act effective it asks ONE
//! native approval (only when something would be started), then starts the model runtime and the
//! router through `Verb::SidecarStart` and adopts the rest. Never Docker, never launchd here.
//! Outcomes are fixed codes; no path, output or error text leaves this module.

use futures::future::BoxFuture;
use serde::Serialize;

use super::approval_gate;
use super::pull_window;
use super::supervisor::act;
use super::supervisor::machine::Observation;
use super::supervisor::probe::Probe;
use super::supervisor::registry::{sidecar_runtime, Registry, ServiceId};
use super::supervisor::runner::{
    PortProbe, PortState, ProcSpawner, ProcWindow, RealPortProbe, RunError, SidecarOutcome,
    SidecarRunner, Spawner, WindowProc,
};

/// The fixed error a second concurrent call gets.
pub(crate) const BUSY: &str = "busy";

/// One start-all at a time, process-wide: a second call while one runs returns `busy`.
static START_ALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Fixed outcome codes (serialised snake_case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StartAllOutcome {
    /// Already answering: nothing started.
    Adopted,
    Started,
    /// Act is not effective, so nothing was started.
    SkippedActOff,
    /// Not ours to start here (Docker rows, LaunchAgents, voice) and not healthy.
    Skipped,
    /// The port is held by something unhealthy: the user has to look.
    NeedsYou,
    /// No usable bundled binary yet.
    BinaryMissing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StartAllRow {
    pub id: ServiceId,
    pub outcome: StartAllOutcome,
}

/// The only read-only view of a non-sidecar row: healthy or not.
pub(crate) trait RowHealth: Send + Sync {
    fn healthy<'a>(
        &'a self,
        def: &'a super::supervisor::registry::ServiceDef,
    ) -> BoxFuture<'a, bool>;
}

struct ProbeHealth<'p>(&'p dyn Probe);
impl RowHealth for ProbeHealth<'_> {
    fn healthy<'a>(
        &'a self,
        def: &'a super::supervisor::registry::ServiceDef,
    ) -> BoxFuture<'a, bool> {
        Box::pin(
            async move { matches!(self.0.check(def).await, Observation::Up | Observation::Slow) },
        )
    }
}

/// The testable body. `approve` is awaited at most once, and only when act is effective and at
/// least one sidecar row is free to start.
pub(crate) async fn start_all_with<S, P, F, Fut, G>(
    lock: &tokio::sync::Mutex<()>,
    act_effective: bool,
    registry: &Registry,
    runner: &SidecarRunner<S, P>,
    health: &dyn RowHealth,
    approve: F,
) -> Result<Vec<StartAllRow>, String>
where
    S: Spawner,
    P: PortProbe,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<G, String>>,
{
    // Serialised: a concurrent second call never reaches the probes or the spawns.
    let Ok(_serial) = lock.try_lock() else {
        return Err(BUSY.to_string());
    };
    // Never overlaps a model download window (handoff 25a M1): held for the whole call.
    let _slot = runner
        .slot()
        .try_acquire(pull_window::Holder::StartAll)
        .map_err(|h| pull_window::start_all_refusal(h).to_string())?;
    // Read-only pass first.
    let mut rows: Vec<StartAllRow> = Vec::new();
    let mut free: Vec<ServiceId> = Vec::new();
    for def in registry.defs() {
        let outcome = if sidecar_runtime(def.id).is_some() {
            match runner.port_state(def.id).await {
                Some(PortState::Healthy) => StartAllOutcome::Adopted,
                Some(PortState::HeldUnhealthy) => StartAllOutcome::NeedsYou,
                _ if !act_effective => StartAllOutcome::SkippedActOff,
                _ => {
                    free.push(def.id);
                    StartAllOutcome::Skipped // replaced below once started
                }
            }
        } else if health.healthy(def).await {
            StartAllOutcome::Adopted
        } else {
            StartAllOutcome::Skipped
        };
        rows.push(StartAllRow {
            id: def.id,
            outcome,
        });
    }
    if !act_effective || free.is_empty() {
        return Ok(rows);
    }
    // The approval guard lives until every spawn below has finished.
    let _approval = approve().await?;
    // Registry order is dependency order: runtime, then router. `start` is the handler of
    // `Verb::SidecarStart` (policy, allowlist, probe, then spawn), called exactly once per row.
    for id in free {
        let outcome = match runner.start(id).await {
            Ok(SidecarOutcome::Started) => StartAllOutcome::Started,
            Ok(SidecarOutcome::Adopted) => StartAllOutcome::Adopted,
            Ok(SidecarOutcome::NeedsYou) => StartAllOutcome::NeedsYou,
            Ok(SidecarOutcome::BinaryMissing) => StartAllOutcome::BinaryMissing,
            Err(RunError::ActDisabled) => StartAllOutcome::SkippedActOff,
            Err(_) => StartAllOutcome::Failed,
        };
        if let Some(r) = rows.iter_mut().find(|r| r.id == id) {
            r.outcome = outcome;
        }
    }
    Ok(rows)
}

static RUNNER: std::sync::OnceLock<SidecarRunner<ProcSpawner, RealPortProbe>> =
    std::sync::OnceLock::new();

/// The one process-wide runner (also used by the pull lifecycle to stop and restore the serve).
pub(crate) fn shared_runner() -> &'static SidecarRunner<ProcSpawner, RealPortProbe> {
    RUNNER.get_or_init(|| SidecarRunner::real(crate::app_paths::data_dir()))
}

static WINDOW: std::sync::OnceLock<ProcWindow> = std::sync::OnceLock::new();

/// The one process-wide download window handle, so a quit can end it wherever the pull stands.
pub(crate) fn shared_window() -> &'static ProcWindow {
    WINDOW.get_or_init(ProcWindow::new)
}

/// Quit hook: ends the download window (whole process group) and waits until it is gone. Independent
/// of `act` (the window is the download lane's own, never the supervisor's). True if none is left.
pub(crate) fn stop_window_blocking() -> bool {
    let Some(w) = WINDOW.get() else {
        return true;
    };
    w.stop();
    let live = w.liveness();
    let start = std::time::Instant::now();
    while live() {
        if start.elapsed() >= std::time::Duration::from_secs(5) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

/// Whether the app itself started a sidecar (or the download window) that is still alive. False
/// (and nothing is built) when neither ever ran.
pub(crate) fn any_started_sidecars() -> bool {
    RUNNER.get().is_some_and(|r| r.spawner.any_started())
        || WINDOW.get().is_some_and(|w| (w.liveness())())
}

/// Stop-on-quit hook for the sidecars the app itself started (adopted ones are never recorded, so
/// never touched). Runs only when `act` is true.
pub(crate) fn stop_started_sidecars(act: bool) -> Vec<ServiceId> {
    // The download window ends on every quit, act or not.
    stop_window_blocking();
    if !act {
        return Vec::new();
    }
    RUNNER
        .get()
        .map_or_else(Vec::new, |r| r.spawner.stop_started())
}

/// Start (or adopt) the bundled services. Act off: looks only, spawns nothing. Act on: one native
/// approval, then the runtime and the router. Docker rows and LaunchAgents are never touched.
#[tauri::command]
#[specta::specta]
pub async fn services_start_all(app: tauri::AppHandle) -> Result<Vec<StartAllRow>, String> {
    let registry = Registry::builtin();
    let probe = super::supervisor::probe::RealProbe::new();
    let defs: Vec<_> = registry.defs().to_vec();
    probe.prepare(&defs).await;
    let approve = || async {
        let text = approval_gate::approval_text(
            "Start Amara OS's services",
            "This starts the language-model runtime and the router that came with Amara OS, on \
             this Mac only. Anything already running is left alone and is never stopped by this.",
        )?;
        approval_gate::require_approval(
            &app,
            &[approval_gate::service_key("services-start-all")],
            text,
        )
        .await
    };
    let runner = shared_runner();
    start_all_with(
        &START_ALL_LOCK,
        act::effective(),
        &registry,
        runner,
        &ProbeHealth(&probe),
        approve,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::supervisor::registry::ServiceDef;
    use crate::commands::supervisor::runner::{RunPolicy, SpawnSpec};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// The shared window is process-wide: tests that touch it run one at a time.
    static WINDOW_TEST: Mutex<()> = Mutex::new(());

    #[derive(Default, Clone)]
    struct FakeSpawner(Arc<Mutex<Vec<ServiceId>>>);
    impl Spawner for FakeSpawner {
        fn spawn(&self, id: ServiceId, _spec: &SpawnSpec) -> Result<(), RunError> {
            self.0.lock().unwrap().push(id);
            Ok(())
        }
    }
    struct Ports(PortState);
    impl PortProbe for Ports {
        fn state<'a>(&'a self, _d: &'a ServiceDef) -> BoxFuture<'a, PortState> {
            let s = self.0;
            Box::pin(async move { s })
        }
    }
    struct Health(bool);
    impl RowHealth for Health {
        fn healthy<'a>(&'a self, _d: &'a ServiceDef) -> BoxFuture<'a, bool> {
            let h = self.0;
            Box::pin(async move { h })
        }
    }

    fn setup(
        port: PortState,
        tag: &str,
    ) -> (SidecarRunner<FakeSpawner, Ports>, FakeSpawner, PathBuf) {
        let d = std::env::temp_dir().join(format!("sup-sa-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sp = FakeSpawner::default();
        let r = SidecarRunner::with(
            sp.clone(),
            Ports(port),
            Registry::builtin(),
            RunPolicy::Full,
            Box::new(|n| Some(PathBuf::from(format!("/fake/MacOS/{n}")))),
            d.clone(),
            None,
        );
        (r, sp, d)
    }

    fn get(rows: &[StartAllRow], id: ServiceId) -> StartAllOutcome {
        rows.iter().find(|r| r.id == id).unwrap().outcome
    }

    async fn no_approval() -> Result<(), String> {
        panic!("approval must not be asked")
    }

    #[tokio::test]
    async fn act_off_spawns_nothing_and_skips_the_sidecar_rows() {
        let (r, sp, d) = setup(PortState::Free, "off");
        let rows = start_all_with(
            &tokio::sync::Mutex::new(()),
            false,
            &Registry::builtin(),
            &r,
            &Health(false),
            no_approval,
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), ServiceId::ALL.len());
        assert_eq!(
            get(&rows, ServiceId::ModelRuntime),
            StartAllOutcome::SkippedActOff
        );
        assert_eq!(
            get(&rows, ServiceId::AmaraRouter),
            StartAllOutcome::SkippedActOff
        );
        assert_eq!(get(&rows, ServiceId::N8n), StartAllOutcome::Skipped);
        assert!(sp.0.lock().unwrap().is_empty());
        assert!(
            !d.join("router").exists(),
            "no config written when act is off"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn start_all_is_refused_while_a_pull_holds_the_slot_and_spawns_nothing() {
        use crate::commands::pull_window::{Holder, PULL_IN_PROGRESS};
        for act in [false, true] {
            let (r, sp, d) = setup(PortState::Free, "pull");
            let pull = r.slot().try_acquire(Holder::Download).unwrap();
            let e = start_all_with(
                &tokio::sync::Mutex::new(()),
                act,
                &Registry::builtin(),
                &r,
                &Health(false),
                no_approval,
            )
            .await
            .unwrap_err();
            assert_eq!(e, PULL_IN_PROGRESS);
            assert!(sp.0.lock().unwrap().is_empty());
            // The runner itself refuses the sandboxed serve too.
            assert_eq!(
                r.start(ServiceId::ModelRuntime).await.err(),
                Some(RunError::PullInProgress)
            );
            drop(pull);
            assert!(start_all_with(
                &tokio::sync::Mutex::new(()),
                false,
                &Registry::builtin(),
                &r,
                &Health(false),
                no_approval,
            )
            .await
            .is_ok());
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[tokio::test]
    async fn quit_stops_a_running_window_and_waits_until_it_is_gone() {
        use crate::commands::supervisor::runner::WindowProc;
        let _g = WINDOW_TEST.lock().unwrap_or_else(|p| p.into_inner());
        let w = shared_window();
        w.spawn(&SpawnSpec {
            program: PathBuf::from("/bin/sleep"),
            args: vec!["30".to_string()],
            env: vec![("PATH", "/usr/bin:/bin".to_string())],
        })
        .unwrap();
        assert!(any_started_sidecars(), "a live window counts");
        // Mid-pull: a slot registered for it refuses new holders until it is gone.
        let (r, _sp, d) = setup(PortState::Free, "quit");
        r.slot().set_window(w.liveness());
        assert!(r.slot().window_may_be_alive());
        // act off: the window is still ended.
        assert!(stop_started_sidecars(false).is_empty());
        assert!(!w.alive());
        assert!(!(w.liveness())());
        assert!(!any_started_sidecars());
        assert!(!r.slot().window_may_be_alive());
        assert!(stop_window_blocking());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn act_off_reports_healthy_rows_as_adopted() {
        let (r, sp, d) = setup(PortState::Healthy, "adopt");
        let rows = start_all_with(
            &tokio::sync::Mutex::new(()),
            false,
            &Registry::builtin(),
            &r,
            &Health(true),
            no_approval,
        )
        .await
        .unwrap();
        assert!(rows.iter().all(|r| r.outcome == StartAllOutcome::Adopted));
        assert!(sp.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn this_build_never_acts() {
        // The real gate: the compile-time switch is off, so the command passes `false`.
        assert!(!act::effective());
    }

    #[tokio::test]
    async fn act_on_asks_once_then_starts_runtime_then_router_only() {
        let (r, sp, d) = setup(PortState::Free, "on");
        let asked = Arc::new(Mutex::new(0));
        let a2 = asked.clone();
        let rows = start_all_with(
            &tokio::sync::Mutex::new(()),
            true,
            &Registry::builtin(),
            &r,
            &Health(false),
            || async move {
                *a2.lock().unwrap() += 1;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*asked.lock().unwrap(), 1);
        assert_eq!(
            *sp.0.lock().unwrap(),
            vec![ServiceId::ModelRuntime, ServiceId::AmaraRouter]
        );
        assert_eq!(
            get(&rows, ServiceId::ModelRuntime),
            StartAllOutcome::Started
        );
        assert_eq!(get(&rows, ServiceId::AmaraRouter), StartAllOutcome::Started);
        assert_eq!(get(&rows, ServiceId::Docker), StartAllOutcome::Skipped);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_declined_approval_starts_nothing() {
        let (r, sp, d) = setup(PortState::Free, "decl");
        let e = start_all_with(
            &tokio::sync::Mutex::new(()),
            true,
            &Registry::builtin(),
            &r,
            &Health(false),
            || async { Err::<(), String>("declined".to_string()) },
        )
        .await
        .unwrap_err();
        assert_eq!(e, "declined");
        assert!(sp.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn act_on_adopts_and_flags_held_ports_without_asking() {
        for (port, want) in [
            (PortState::Healthy, StartAllOutcome::Adopted),
            (PortState::HeldUnhealthy, StartAllOutcome::NeedsYou),
        ] {
            let (r, sp, d) = setup(port, "nsk");
            let rows = start_all_with(
                &tokio::sync::Mutex::new(()),
                true,
                &Registry::builtin(),
                &r,
                &Health(false),
                no_approval,
            )
            .await
            .unwrap();
            assert_eq!(get(&rows, ServiceId::ModelRuntime), want);
            assert_eq!(get(&rows, ServiceId::AmaraRouter), want);
            assert!(sp.0.lock().unwrap().is_empty());
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn source_pins() {
        let src = include_str!("services_start_all.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        for bad in [
            "launchctl",
            "Prog::",
            "Verb::Docker",
            "tauri_plugin_shell",
            "/Users/",
            "process::Command",
        ] {
            assert!(!prod.contains(bad), "{bad}");
        }
        assert!(prod.contains("approval_gate::require_approval"));
    }

    #[test]
    fn outcome_codes_are_fixed_snake_case() {
        let all = [
            StartAllOutcome::Adopted,
            StartAllOutcome::Started,
            StartAllOutcome::SkippedActOff,
            StartAllOutcome::Skipped,
            StartAllOutcome::NeedsYou,
            StartAllOutcome::BinaryMissing,
            StartAllOutcome::Failed,
        ];
        let s: Vec<String> = all
            .iter()
            .map(|o| {
                serde_json::to_value(o)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            s.join(","),
            "adopted,started,skipped_act_off,skipped,needs_you,binary_missing,failed"
        );
        let row = serde_json::to_value(StartAllRow {
            id: ServiceId::AmaraRouter,
            outcome: StartAllOutcome::SkippedActOff,
        })
        .unwrap();
        assert_eq!(
            row,
            serde_json::json!({"id":"amara-router","outcome":"skipped_act_off"})
        );
    }

    #[tokio::test]
    async fn concurrent_calls_spawn_once_per_id_and_the_second_is_busy() {
        let (r, sp, d) = setup(PortState::Free, "busy");
        let lock = tokio::sync::Mutex::new(());
        let reg = Registry::builtin();
        let slow_approve = || async {
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            Ok::<(), String>(())
        };
        let (a, b) = tokio::join!(
            start_all_with(&lock, true, &reg, &r, &Health(false), slow_approve),
            start_all_with(&lock, true, &reg, &r, &Health(false), slow_approve),
        );
        let (ok, busy): (Vec<_>, Vec<_>) = [a, b].into_iter().partition(|x| x.is_ok());
        assert_eq!(ok.len(), 1);
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].as_ref().unwrap_err(), "busy");
        assert_eq!(
            *sp.0.lock().unwrap(),
            vec![ServiceId::ModelRuntime, ServiceId::AmaraRouter]
        );
        // The lock is free again afterwards.
        assert!(lock.try_lock().is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn the_approval_guard_is_held_until_the_spawns_finish() {
        struct Guard(Arc<Mutex<Vec<&'static str>>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.lock().unwrap().push("guard_dropped");
            }
        }
        struct LogSpawner(Arc<Mutex<Vec<&'static str>>>);
        impl Spawner for LogSpawner {
            fn spawn(&self, _id: ServiceId, _s: &SpawnSpec) -> Result<(), RunError> {
                self.0.lock().unwrap().push("spawn");
                Ok(())
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let d = std::env::temp_dir().join(format!("sup-sa-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let r = SidecarRunner::with(
            LogSpawner(log.clone()),
            Ports(PortState::Free),
            Registry::builtin(),
            RunPolicy::Full,
            Box::new(|n| Some(PathBuf::from(format!("/fake/MacOS/{n}")))),
            d.clone(),
            None,
        );
        let l2 = log.clone();
        start_all_with(
            &tokio::sync::Mutex::new(()),
            true,
            &Registry::builtin(),
            &r,
            &Health(false),
            || async move { Ok(Guard(l2)) },
        )
        .await
        .unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            vec!["spawn", "spawn", "guard_dropped"]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stop_on_quit_does_nothing_when_act_is_off() {
        let _g = WINDOW_TEST.lock().unwrap_or_else(|p| p.into_inner());
        // Nothing was ever started in this process, and act off never reaches the runner at all.
        assert!(!any_started_sidecars());
        assert!(stop_started_sidecars(false).is_empty());
        assert!(stop_started_sidecars(true).is_empty());
    }
}
