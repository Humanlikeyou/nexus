//! Start-on-launch and drain-on-quit. Both are no-ops unless act is on (`act::effective()`, which needs
//! the compile-time `SUPERVISOR_ACT_ENABLED` as well as the saved setting), and both reach the machine
//! only through the `Runner` trait, so tests use fakes and an injected clock.
//!
//! Start order (plan section 3): Docker Desktop (hidden open, poll `docker info`, 120 s), Postgres,
//! Letta and Weaviate (+ Verba), n8n, crew, Home Assistant and Freqtrade, then the router and Hermes
//! (adopt, or kickstart only if launchd has the job loaded). A healthy service is ADOPTED with zero
//! verbs. A stopped container gets `docker start`. A missing container is `needs_you`: never created,
//! never compose. Voice, Open WebUI, Ollama and the built-in rows are never touched.
//!
//! Quit: only the services THIS launch started (`started_by_app`) are stopped, in reverse start order,
//! and only docker rows: adopted services, Open WebUI, Ollama, Docker Desktop, LaunchAgent jobs and
//! Freqtrade are never stopped. The crew drains first (poll its active runs, 60 s cap) so a running
//! crew is not cut off. The whole sequence has an overall cap of 30 s.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;

use super::machine::{Observation, ReasonCode};
use super::probe::Probe;
use super::registry::{Registry, Runtime, ServiceDef, ServiceId, LAUNCHD_GRACE_MS};
use super::runner::{RunError, Runner, Verb};

/// Dependency order, grouped. Within a group rows start one after another.
pub const START_ORDER: &[&[ServiceId]] = &[
    &[ServiceId::Docker],
    &[ServiceId::Postgres],
    &[ServiceId::Letta, ServiceId::Weaviate],
    &[ServiceId::N8n],
    &[ServiceId::CrewService],
    &[ServiceId::Homeassistant, ServiceId::Freqtrade],
    &[ServiceId::AmaraRouter, ServiceId::HermesGateway],
];

pub const DOCKER_READY: Duration = Duration::from_secs(120);
pub const SERVICE_READY: Duration = Duration::from_secs(60);
pub const POLL: Duration = Duration::from_secs(2);
pub const CREW_DRAIN_CAP: Duration = Duration::from_secs(60);
pub const QUIT_CAP: Duration = Duration::from_secs(30);
/// Kept free of the drain so at least one stop fits inside the overall cap.
pub const STOP_RESERVE: Duration = Duration::from_secs(10);

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
    fn sleep<'a>(&'a self, d: Duration) -> BoxFuture<'a, ()>;
}

pub struct SysClock(std::time::Instant);

impl SysClock {
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Default for SysClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SysClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn sleep<'a>(&'a self, d: Duration) -> BoxFuture<'a, ()> {
        Box::pin(tokio::time::sleep(d))
    }
}

/// How many crew runs are executing right now. None = the crew service could not be asked.
pub trait CrewRuns: Send + Sync {
    fn active_runs<'a>(&'a self) -> BoxFuture<'a, Option<u32>>;
}

/// Asks the existing crew client (`crew_list`: loopback base, crew token, launch token header).
pub struct RealCrewRuns;

impl CrewRuns for RealCrewRuns {
    fn active_runs<'a>(&'a self) -> BoxFuture<'a, Option<u32>> {
        Box::pin(async {
            let rows = crate::commands::crew::crew_list().await.ok()?;
            // A run waiting for an approval is parked on a person, not working: it is not drained.
            Some(rows.iter().filter(|r| r.status == "running").count() as u32)
        })
    }
}

pub struct Deps<'a> {
    pub probe: &'a dyn Probe,
    pub runner: &'a dyn Runner,
    pub clock: &'a dyn Clock,
    pub crew: &'a dyn CrewRuns,
    pub registry: &'a Registry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// Already running: nothing was run.
    Adopted,
    /// The app started it; `ready` = it answered within the wait.
    Started { ready: bool },
    /// Only the user can fix it (missing container, wide bind).
    NeedsYou(ReasonCode),
    /// Not attempted: Docker is not up, or launchd does not have the job.
    Skipped(ReasonCode),
    /// A verb was refused or failed.
    Failed(RunError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchReport {
    pub outcomes: Vec<(ServiceId, StartOutcome)>,
    /// started_by_app, in start order.
    pub started: Vec<ServiceId>,
}

fn is_up(o: Observation) -> bool {
    matches!(o, Observation::Up | Observation::Slow)
}

async fn check(d: &Deps<'_>, def: &ServiceDef) -> Observation {
    d.probe.prepare(std::slice::from_ref(def)).await;
    d.probe.check(def).await
}

/// Poll until the row answers or `limit` passes (by the injected clock).
async fn wait_up(d: &Deps<'_>, def: &ServiceDef, limit: Duration) -> bool {
    let end = d.clock.now_ms() + limit.as_millis() as u64;
    loop {
        if is_up(check(d, def).await) {
            return true;
        }
        if d.clock.now_ms() >= end {
            return false;
        }
        d.clock.sleep(POLL).await;
    }
}

fn start_verb(def: &ServiceDef) -> Option<Verb> {
    match def.runtime {
        Runtime::DockerDesktop => Some(Verb::OpenDockerHidden),
        Runtime::Docker => Some(Verb::DockerStart(def.id)),
        Runtime::Launchd(_) => Some(Verb::LaunchctlKickstartK(def.id)),
        _ => None,
    }
}

async fn start_one(d: &Deps<'_>, def: &ServiceDef, docker_up: bool) -> StartOutcome {
    let obs = check(d, def).await;
    if is_up(obs) {
        return StartOutcome::Adopted;
    }
    match obs {
        Observation::NeedsYou(ReasonCode::DockerUnavailable) => {
            return StartOutcome::Skipped(ReasonCode::DockerUnavailable)
        }
        Observation::NeedsYou(r) => return StartOutcome::NeedsYou(r),
        _ => {}
    }
    if def.runtime == Runtime::Docker && !docker_up {
        return StartOutcome::Skipped(ReasonCode::DockerUnavailable);
    }
    if def.runtime == Runtime::Docker && !is_up(obs) && matches!(obs, Observation::Starting(_)) {
        // Someone else is already starting it: wait, never double-start.
        return if wait_up(d, def, SERVICE_READY).await {
            StartOutcome::Adopted
        } else {
            StartOutcome::Skipped(ReasonCode::ContainerStarting)
        };
    }
    if let Runtime::Launchd(_) = def.runtime {
        // launchd may be mid-restart (KeepAlive): give it its grace before kickstarting.
        if wait_up(d, def, Duration::from_millis(LAUNCHD_GRACE_MS)).await {
            return StartOutcome::Adopted;
        }
    }
    let Some(verb) = start_verb(def) else {
        return StartOutcome::Skipped(ReasonCode::NotRunning);
    };
    match d.runner.run(verb).await {
        Ok(()) => {}
        Err(RunError::NotLoaded) => return StartOutcome::Skipped(ReasonCode::NotRunning),
        Err(e) => return StartOutcome::Failed(e),
    }
    let limit = if def.runtime == Runtime::DockerDesktop {
        DOCKER_READY
    } else {
        SERVICE_READY
    };
    StartOutcome::Started {
        ready: wait_up(d, def, limit).await,
    }
}

/// Start-on-launch. Does nothing at all (no probe, no verb) unless `act`.
pub async fn start_on_launch(d: &Deps<'_>, act: bool) -> LaunchReport {
    let mut report = LaunchReport::default();
    if !act {
        return report;
    }
    let mut docker_up = true;
    for group in START_ORDER {
        for id in *group {
            let Some(def) = d.registry.get(*id) else {
                continue;
            };
            let out = start_one(d, def, docker_up).await;
            if def.id == ServiceId::Docker {
                docker_up = matches!(
                    out,
                    StartOutcome::Adopted | StartOutcome::Started { ready: true }
                );
            }
            if matches!(out, StartOutcome::Started { .. }) {
                report.started.push(def.id);
            }
            report.outcomes.push((def.id, out));
        }
    }
    report
}

/// Only docker rows are ever stopped, and never Freqtrade.
pub fn stoppable(def: &ServiceDef) -> bool {
    def.runtime == Runtime::Docker && def.id != ServiceId::Freqtrade
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    NotNeeded,
    Drained,
    TimedOut,
    /// The crew service did not answer; nothing to wait for.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReport {
    pub stopped: Vec<ServiceId>,
    pub failed: Vec<(ServiceId, RunError)>,
    /// Not reached before the overall cap.
    pub skipped_deadline: Vec<ServiceId>,
    pub drain: DrainOutcome,
}

async fn drain_crew(d: &Deps<'_>, limit: Duration) -> DrainOutcome {
    let end = d.clock.now_ms() + limit.as_millis() as u64;
    loop {
        match d.crew.active_runs().await {
            None => return DrainOutcome::Unreachable,
            Some(0) => return DrainOutcome::Drained,
            Some(_) => {}
        }
        if d.clock.now_ms() >= end {
            return DrainOutcome::TimedOut;
        }
        d.clock.sleep(POLL).await;
    }
}

/// Stop what this launch started. `started` is in start order; `act` false does nothing.
pub async fn drain_and_stop(
    d: &Deps<'_>,
    act: bool,
    started: &[ServiceId],
    overall: Duration,
) -> StopReport {
    let mut report = StopReport {
        stopped: Vec::new(),
        failed: Vec::new(),
        skipped_deadline: Vec::new(),
        drain: DrainOutcome::NotNeeded,
    };
    if !act {
        return report;
    }
    let begin = d.clock.now_ms();
    let deadline = begin + overall.as_millis() as u64;
    let to_stop: Vec<&ServiceDef> = started
        .iter()
        .rev()
        .filter_map(|id| d.registry.get(*id))
        .filter(|def| stoppable(def))
        .collect();
    for def in to_stop {
        if d.clock.now_ms() >= deadline {
            report.skipped_deadline.push(def.id);
            continue;
        }
        if def.id == ServiceId::CrewService {
            let remaining = deadline.saturating_sub(d.clock.now_ms());
            let room = remaining.saturating_sub(STOP_RESERVE.as_millis() as u64);
            let limit = CREW_DRAIN_CAP.min(Duration::from_millis(room));
            report.drain = drain_crew(d, limit).await;
        }
        match d.runner.run(Verb::DockerStop(def.id)).await {
            Ok(()) => report.stopped.push(def.id),
            Err(e) => report.failed.push((def.id, e)),
        }
    }
    report
}

// ---- quit hook ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitState {
    Idle = 0,
    Draining = 1,
    Done = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitDecision {
    /// Let the app exit exactly as it always did.
    Allow,
    /// Hold the exit; `begin_drain` is true for the first request only.
    Prevent { begin_drain: bool },
}

/// A drain that has run this long is presumed stuck: the next quit request goes through.
pub const DRAIN_WATCHDOG: Duration = Duration::from_secs(45);

/// With act off (or nothing of ours to stop) this is always `Allow`: quit behaviour is today's.
/// `draining_ms` is how long the current drain has been running (0 when not draining).
pub fn quit_decision(
    act: bool,
    has_stoppable_started: bool,
    state: QuitState,
    draining_ms: u64,
) -> QuitDecision {
    if !act || !has_stoppable_started {
        return QuitDecision::Allow;
    }
    match state {
        QuitState::Done => QuitDecision::Allow,
        QuitState::Draining if draining_ms > DRAIN_WATCHDOG.as_millis() as u64 => {
            QuitDecision::Allow
        }
        QuitState::Draining => QuitDecision::Prevent { begin_drain: false },
        QuitState::Idle => QuitDecision::Prevent { begin_drain: true },
    }
}

static QUIT_STATE: AtomicU8 = AtomicU8::new(QuitState::Idle as u8);

/// True once a quit drain has begun (or finished): the tick loop must stop auto-restarting then.
pub(crate) fn quit_started() -> bool {
    quit_state() != QuitState::Idle
}

static DRAIN_BEGAN: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn draining_ms() -> u64 {
    DRAIN_BEGAN
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .map_or(0, |t| t.elapsed().as_millis() as u64)
}

fn quit_state() -> QuitState {
    match QUIT_STATE.load(Ordering::SeqCst) {
        1 => QuitState::Draining,
        2 => QuitState::Done,
        _ => QuitState::Idle,
    }
}

/// Called from `RunEvent::ExitRequested`. Holds the exit only when act is on AND this launch started a
/// stoppable service; then drains and stops in the background and calls `finish` (which exits again).
pub fn on_exit_requested(api: &tauri::ExitRequestApi, finish: impl FnOnce() + Send + 'static) {
    let started = super::started_snapshot();
    let registry = Registry::builtin();
    let has = started
        .iter()
        .any(|id| registry.get(*id).is_some_and(stoppable));
    match quit_decision(super::act::effective(), has, quit_state(), draining_ms()) {
        QuitDecision::Allow => {}
        QuitDecision::Prevent { begin_drain } => {
            api.prevent_exit();
            if begin_drain {
                QUIT_STATE.store(QuitState::Draining as u8, Ordering::SeqCst);
                *DRAIN_BEGAN.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(std::time::Instant::now());
                tauri::async_runtime::spawn(async move {
                    // The drain runs in its own task so a panic in it is just a join error here:
                    // completion, error or panic all end in Done and a call to `finish`.
                    let drain = tauri::async_runtime::spawn(async move {
                        run_quit_drain(&started).await;
                    });
                    let _ = drain.await;
                    QUIT_STATE.store(QuitState::Done as u8, Ordering::SeqCst);
                    finish();
                });
            }
        }
    }
}

async fn run_quit_drain(started: &[ServiceId]) {
    let registry = Registry::builtin();
    let probe = super::probe::RealProbe::new();
    let runner = super::runner::RealRunner::new(super::runner::RunPolicy::Full);
    let clock = SysClock::new();
    let deps = Deps {
        probe: &probe,
        runner: &runner,
        clock: &clock,
        crew: &RealCrewRuns,
        registry: &registry,
    };
    // The overall cap is also a wall-clock bound, whatever a hung call does.
    let report = tokio::time::timeout(
        QUIT_CAP + Duration::from_secs(2),
        drain_and_stop(&deps, super::act::effective(), started, QUIT_CAP),
    )
    .await;
    if let Ok(report) = report {
        let now = super::unix_ms();
        let mut sup = super::lock(&super::SUPERVISOR);
        for id in report.stopped {
            sup.record_stopped(id, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
        fn sleep<'a>(&'a self, d: Duration) -> BoxFuture<'a, ()> {
            self.0.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    /// A little world: per-row observations that verbs change. One struct is both probe and runner.
    struct World {
        clock: Arc<FakeClock>,
        state: Mutex<HashMap<ServiceId, Observation>>,
        verbs: Mutex<Vec<Verb>>,
        probes: Mutex<u32>,
        /// Verbs advance the fake clock by this much (a slow `docker stop`).
        verb_ms: u64,
        loaded: bool,
    }

    impl World {
        fn new(clock: Arc<FakeClock>) -> Self {
            Self {
                clock,
                state: Mutex::new(HashMap::new()),
                verbs: Mutex::new(Vec::new()),
                probes: Mutex::new(0),
                verb_ms: 0,
                loaded: true,
            }
        }
        fn set(&self, id: ServiceId, o: Observation) {
            self.state.lock().unwrap().insert(id, o);
        }
        fn all_up_except(clock: Arc<FakeClock>, down: &[(ServiceId, Observation)]) -> Self {
            let w = Self::new(clock);
            for id in ServiceId::ALL {
                w.set(id, Observation::Up);
            }
            for (id, o) in down {
                w.set(*id, *o);
            }
            w
        }
        fn verbs(&self) -> Vec<Verb> {
            self.verbs.lock().unwrap().clone()
        }
    }

    impl Probe for World {
        fn check<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, Observation> {
            *self.probes.lock().unwrap() += 1;
            let s = self.state.lock().unwrap();
            let docker_down = !is_up(s[&ServiceId::Docker]);
            let o = if def.runtime == Runtime::Docker && docker_down {
                Observation::NeedsYou(ReasonCode::DockerUnavailable)
            } else {
                s[&def.id]
            };
            Box::pin(async move { o })
        }
    }

    impl Runner for World {
        fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
            self.verbs.lock().unwrap().push(verb);
            self.clock.0.fetch_add(self.verb_ms, Ordering::SeqCst);
            let r = match verb {
                Verb::OpenDockerHidden => {
                    self.set(ServiceId::Docker, Observation::Up);
                    Ok(())
                }
                Verb::DockerStart(id) => {
                    self.set(id, Observation::Up);
                    Ok(())
                }
                Verb::LaunchctlKickstartK(id) => {
                    if self.loaded {
                        self.set(id, Observation::Up);
                        Ok(())
                    } else {
                        Err(RunError::NotLoaded)
                    }
                }
                _ => Ok(()),
            };
            Box::pin(async move { r })
        }
    }

    struct FakeCrew {
        /// Active runs per call; the last value repeats.
        seq: Mutex<Vec<Option<u32>>>,
    }
    impl CrewRuns for FakeCrew {
        fn active_runs<'a>(&'a self) -> BoxFuture<'a, Option<u32>> {
            let mut s = self.seq.lock().unwrap();
            let v = if s.len() > 1 { s.remove(0) } else { s[0] };
            Box::pin(async move { v })
        }
    }

    fn deps<'a>(
        w: &'a World,
        clock: &'a FakeClock,
        crew: &'a FakeCrew,
        reg: &'a Registry,
    ) -> Deps<'a> {
        Deps {
            probe: w,
            runner: w,
            clock,
            crew,
            registry: reg,
        }
    }

    fn crew(seq: Vec<Option<u32>>) -> FakeCrew {
        FakeCrew {
            seq: Mutex::new(seq),
        }
    }

    const STOPPED: Observation = Observation::Stopped(ReasonCode::ContainerStopped);
    const DOWN: Observation = Observation::Down(ReasonCode::Unreachable);

    #[tokio::test]
    async fn a_cold_start_runs_the_plan_order_and_records_what_it_started() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(
            clock.clone(),
            &[
                (
                    ServiceId::Docker,
                    Observation::Down(ReasonCode::DockerUnavailable),
                ),
                (ServiceId::Postgres, STOPPED),
                (ServiceId::Letta, STOPPED),
                (ServiceId::Weaviate, STOPPED),
                (ServiceId::N8n, STOPPED),
                (ServiceId::CrewService, STOPPED),
                (ServiceId::Homeassistant, STOPPED),
                (ServiceId::Freqtrade, STOPPED),
                (ServiceId::AmaraRouter, DOWN),
            ],
        );
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let r = start_on_launch(&deps(&w, &clock, &c, &reg), true).await;
        assert_eq!(
            w.verbs(),
            vec![
                Verb::OpenDockerHidden,
                Verb::DockerStart(ServiceId::Postgres),
                Verb::DockerStart(ServiceId::Letta),
                Verb::DockerStart(ServiceId::Weaviate),
                Verb::DockerStart(ServiceId::N8n),
                Verb::DockerStart(ServiceId::CrewService),
                Verb::DockerStart(ServiceId::Homeassistant),
                Verb::DockerStart(ServiceId::Freqtrade),
                Verb::LaunchctlKickstartK(ServiceId::AmaraRouter),
            ]
        );
        // Hermes was up: adopted, so not in the started list and no verb for it.
        assert_eq!(
            r.started,
            vec![
                ServiceId::Docker,
                ServiceId::Postgres,
                ServiceId::Letta,
                ServiceId::Weaviate,
                ServiceId::N8n,
                ServiceId::CrewService,
                ServiceId::Homeassistant,
                ServiceId::Freqtrade,
                ServiceId::AmaraRouter,
            ]
        );
        assert!(r
            .outcomes
            .contains(&(ServiceId::HermesGateway, StartOutcome::Adopted)));
        // The router got its launchd grace before the kickstart.
        assert!(clock.now_ms() >= LAUNCHD_GRACE_MS);
    }

    #[tokio::test]
    async fn adopting_a_healthy_start_runs_zero_verbs() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(clock.clone(), &[]);
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let r = start_on_launch(&deps(&w, &clock, &c, &reg), true).await;
        assert!(w.verbs().is_empty());
        assert!(r.started.is_empty());
        assert_eq!(r.outcomes.len(), 10);
        assert!(r.outcomes.iter().all(|(_, o)| *o == StartOutcome::Adopted));
    }

    #[tokio::test]
    async fn a_missing_container_is_needs_you_and_never_created() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(
            clock.clone(),
            &[(
                ServiceId::N8n,
                Observation::NeedsYou(ReasonCode::MissingContainer),
            )],
        );
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let r = start_on_launch(&deps(&w, &clock, &c, &reg), true).await;
        assert!(w.verbs().is_empty());
        assert!(r.outcomes.contains(&(
            ServiceId::N8n,
            StartOutcome::NeedsYou(ReasonCode::MissingContainer)
        )));
        // The rest carried on.
        assert!(r
            .outcomes
            .contains(&(ServiceId::CrewService, StartOutcome::Adopted)));
    }

    #[tokio::test]
    async fn a_wide_bind_is_needs_you_and_never_started() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(
            clock.clone(),
            &[(ServiceId::Letta, Observation::NeedsYou(ReasonCode::BadBind))],
        );
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let r = start_on_launch(&deps(&w, &clock, &c, &reg), true).await;
        assert!(w.verbs().is_empty());
        assert!(r.outcomes.contains(&(
            ServiceId::Letta,
            StartOutcome::NeedsYou(ReasonCode::BadBind)
        )));
    }

    #[tokio::test]
    async fn docker_that_never_comes_up_skips_every_container_with_no_verb_for_them() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(
            clock.clone(),
            &[(
                ServiceId::Docker,
                Observation::Down(ReasonCode::DockerUnavailable),
            )],
        );
        // Opening Docker does nothing in this world.
        struct Stuck<'a>(&'a World);
        impl Runner for Stuck<'_> {
            fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
                self.0.verbs.lock().unwrap().push(verb);
                Box::pin(async { Ok(()) })
            }
        }
        let stuck = Stuck(&w);
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let d = Deps {
            probe: &w,
            runner: &stuck,
            clock: &*clock,
            crew: &c,
            registry: &reg,
        };
        let r = start_on_launch(&d, true).await;
        assert_eq!(w.verbs(), vec![Verb::OpenDockerHidden]);
        assert!(clock.now_ms() >= DOCKER_READY.as_millis() as u64);
        assert!(r
            .outcomes
            .contains(&(ServiceId::Docker, StartOutcome::Started { ready: false })));
        assert!(r.outcomes.contains(&(
            ServiceId::Postgres,
            StartOutcome::Skipped(ReasonCode::DockerUnavailable)
        )));
    }

    #[tokio::test]
    async fn kickstart_only_when_launchd_has_the_job_loaded() {
        let clock = Arc::new(FakeClock::default());
        let mut w = World::all_up_except(clock.clone(), &[(ServiceId::HermesGateway, DOWN)]);
        w.loaded = false;
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        let r = start_on_launch(&deps(&w, &clock, &c, &reg), true).await;
        // The runner is asked, says not loaded, and nothing is recorded as started.
        assert_eq!(
            w.verbs(),
            vec![Verb::LaunchctlKickstartK(ServiceId::HermesGateway)]
        );
        assert!(r.started.is_empty());
        assert!(r.outcomes.contains(&(
            ServiceId::HermesGateway,
            StartOutcome::Skipped(ReasonCode::NotRunning)
        )));
    }

    #[tokio::test]
    async fn act_off_start_and_stop_run_zero_verbs_and_zero_probes() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(
            clock.clone(),
            &[(ServiceId::Postgres, STOPPED), (ServiceId::N8n, STOPPED)],
        );
        let reg = Registry::builtin();
        let c = crew(vec![Some(3)]);
        let d = deps(&w, &clock, &c, &reg);
        let r = start_on_launch(&d, false).await;
        assert_eq!(r, LaunchReport::default());
        let s = drain_and_stop(
            &d,
            false,
            &[ServiceId::Postgres, ServiceId::CrewService],
            QUIT_CAP,
        )
        .await;
        assert!(s.stopped.is_empty() && s.failed.is_empty());
        assert_eq!(s.drain, DrainOutcome::NotNeeded);
        assert!(w.verbs().is_empty());
        assert_eq!(*w.probes.lock().unwrap(), 0);
        assert_eq!(clock.now_ms(), 0);
    }

    #[tokio::test]
    async fn only_started_by_app_docker_rows_stop_in_reverse_order() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(clock.clone(), &[]);
        let reg = Registry::builtin();
        let c = crew(vec![Some(0)]);
        // Started this launch, in start order. Docker Desktop, Freqtrade and the router are never
        // stopped; Letta, HA, Open WebUI, Ollama and Hermes were adopted so are not listed at all.
        let started = [
            ServiceId::Docker,
            ServiceId::Postgres,
            ServiceId::N8n,
            ServiceId::CrewService,
            ServiceId::Freqtrade,
            ServiceId::AmaraRouter,
            ServiceId::OpenWebui,
            ServiceId::ModelRuntime,
        ];
        let s = drain_and_stop(&deps(&w, &clock, &c, &reg), true, &started, QUIT_CAP).await;
        assert_eq!(
            w.verbs(),
            vec![
                Verb::DockerStop(ServiceId::CrewService),
                Verb::DockerStop(ServiceId::N8n),
                Verb::DockerStop(ServiceId::Postgres),
            ]
        );
        assert_eq!(s.drain, DrainOutcome::Drained);
        for id in [
            ServiceId::Docker,
            ServiceId::Freqtrade,
            ServiceId::AmaraRouter,
            ServiceId::OpenWebui,
            ServiceId::ModelRuntime,
        ] {
            assert!(!stoppable(reg.get(id).unwrap()), "{id:?}");
        }
    }

    #[tokio::test]
    async fn the_crew_drain_waits_for_the_last_run_then_stops() {
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(clock.clone(), &[]);
        let reg = Registry::builtin();
        let c = crew(vec![Some(2), Some(1), Some(0)]);
        let s = drain_and_stop(
            &deps(&w, &clock, &c, &reg),
            true,
            &[ServiceId::CrewService],
            QUIT_CAP,
        )
        .await;
        assert_eq!(s.drain, DrainOutcome::Drained);
        assert_eq!(clock.now_ms(), 2 * POLL.as_millis() as u64);
        assert_eq!(w.verbs(), vec![Verb::DockerStop(ServiceId::CrewService)]);
    }

    #[tokio::test]
    async fn a_crew_that_never_idles_is_stopped_at_the_drain_cap_inside_the_overall_cap() {
        // Overall 30 s: the drain gets 30 - 10 s of room, then the stop still runs.
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(clock.clone(), &[]);
        let reg = Registry::builtin();
        let c = crew(vec![Some(5)]);
        let s = drain_and_stop(
            &deps(&w, &clock, &c, &reg),
            true,
            &[ServiceId::CrewService],
            QUIT_CAP,
        )
        .await;
        assert_eq!(s.drain, DrainOutcome::TimedOut);
        assert_eq!(w.verbs(), vec![Verb::DockerStop(ServiceId::CrewService)]);
        assert!(
            clock.now_ms() <= QUIT_CAP.as_millis() as u64,
            "{}",
            clock.now_ms()
        );
        assert!(clock.now_ms() >= (QUIT_CAP - STOP_RESERVE).as_millis() as u64 - 2_000);

        // With room to spare the drain's own 60 s cap is the limit.
        let clock = Arc::new(FakeClock::default());
        let w = World::all_up_except(clock.clone(), &[]);
        let c = crew(vec![Some(5)]);
        let s = drain_and_stop(
            &deps(&w, &clock, &c, &reg),
            true,
            &[ServiceId::CrewService],
            Duration::from_secs(300),
        )
        .await;
        assert_eq!(s.drain, DrainOutcome::TimedOut);
        let t = clock.now_ms();
        assert!((60_000..=62_000).contains(&t), "{t}");
    }

    #[tokio::test]
    async fn stops_past_the_overall_cap_are_skipped_and_an_unreachable_crew_does_not_wait() {
        let clock = Arc::new(FakeClock::default());
        let mut w = World::all_up_except(clock.clone(), &[]);
        w.verb_ms = 16_000; // a slow docker stop
        let reg = Registry::builtin();
        let c = crew(vec![None]);
        let s = drain_and_stop(
            &deps(&w, &clock, &c, &reg),
            true,
            &[ServiceId::Postgres, ServiceId::N8n, ServiceId::CrewService],
            QUIT_CAP,
        )
        .await;
        assert_eq!(s.drain, DrainOutcome::Unreachable);
        // crew (16 s), n8n (32 s), then 32 s >= 30 s: postgres is skipped.
        assert_eq!(s.stopped, vec![ServiceId::CrewService, ServiceId::N8n]);
        assert_eq!(s.skipped_deadline, vec![ServiceId::Postgres]);
    }

    #[test]
    fn quit_decision_table() {
        use QuitDecision::*;
        use QuitState::*;
        // Act off: always today's quit, whatever else is true.
        for has in [false, true] {
            for st in [Idle, Draining, Done] {
                assert_eq!(quit_decision(false, has, st, 0), Allow);
            }
        }
        // Act on but nothing of ours to stop: Allow.
        for st in [Idle, Draining, Done] {
            assert_eq!(quit_decision(true, false, st, 0), Allow);
        }
        assert_eq!(
            quit_decision(true, true, Idle, 0),
            Prevent { begin_drain: true }
        );
        assert_eq!(
            quit_decision(true, true, Draining, 1_000),
            Prevent { begin_drain: false }
        );
        // The watchdog: a drain stuck past 45 s no longer blocks the next quit request.
        let wd = DRAIN_WATCHDOG.as_millis() as u64;
        assert_eq!(
            quit_decision(true, true, Draining, wd),
            Prevent { begin_drain: false }
        );
        assert_eq!(quit_decision(true, true, Draining, wd + 1), Allow);
        // ...and never turns an act-off quit into anything but Allow.
        assert_eq!(quit_decision(false, true, Draining, wd + 1), Allow);
        assert_eq!(quit_decision(true, true, Done, 0), Allow);
    }

    #[test]
    fn the_shipped_build_never_holds_the_exit() {
        // `act::effective()` is false while the const is false, so the hook is `Allow`.
        assert_eq!(
            quit_decision(super::super::act::effective(), true, QuitState::Idle, 0),
            QuitDecision::Allow
        );
    }

    #[test]
    fn lib_keeps_voice_stop_and_token_remove_in_exit_and_holds_the_exit_only_via_lifecycle() {
        let lib = include_str!("../../lib.rs");
        let run = &lib[lib.find(".run(|").unwrap()..];
        let exit = &run[run.find("RunEvent::Exit").unwrap()..];
        assert!(exit.contains("commands::voice_heartbeat::stop();"));
        assert!(exit.contains("commands::launch_token::remove();"));
        assert!(run.contains("supervisor::lifecycle::on_exit_requested"));
        // The hold is spelled in lifecycle.rs only: lib.rs itself never prevents an exit.
        assert!(!lib.contains("prevent_exit"));
        let me = include_str!("lifecycle.rs");
        let prod = me.split("#[cfg(test)]").next().unwrap();
        assert_eq!(prod.matches("prevent_exit()").count(), 1);
    }

    #[test]
    fn the_tray_quit_still_goes_through_exit_requested_unchanged() {
        // `AppHandle::exit` requests an exit (RunEvent::ExitRequested, then Exit), so the tray item
        // reaches the same hook as Cmd+Q and window close. It stays exactly `app.exit(0)`.
        let lib = include_str!("../../lib.rs");
        assert!(lib.contains(
            "\"quit\" => {\n                        app.exit(0);\n                    }"
        ));
        assert!(!lib.contains("process::exit"));
        let me = include_str!("lifecycle.rs");
        let prod = me.split("#[cfg(test)]").next().unwrap();
        // The drain runs in its own task and its join result is ignored, so a panic still ends in
        // Done and `finish()`.
        assert!(prod.contains("let _ = drain.await;"));
        let after = &prod[prod.find("let _ = drain.await;").unwrap()..];
        assert!(after.find("QuitState::Done").unwrap() < after.find("finish();").unwrap());
    }

    #[tokio::test]
    async fn a_panicking_task_is_only_a_join_error() {
        let h = tauri::async_runtime::spawn(async {
            panic!("drain blew up");
        });
        assert!(h.await.is_err());
    }

    #[test]
    fn quit_started_follows_the_state() {
        QUIT_STATE.store(QuitState::Idle as u8, Ordering::SeqCst);
        assert!(!quit_started());
        QUIT_STATE.store(QuitState::Draining as u8, Ordering::SeqCst);
        assert!(quit_started());
        QUIT_STATE.store(QuitState::Idle as u8, Ordering::SeqCst);
    }

    #[test]
    fn nothing_here_spawns_a_process() {
        let me = include_str!("lifecycle.rs");
        let prod = me.split("#[cfg(test)]").next().unwrap();
        for banned in ["Command::new", "std::process", "tokio::process", "reqwest"] {
            assert!(!prod.contains(banned), "{banned}");
        }
    }

    #[test]
    fn the_start_order_matches_the_plan_and_has_no_voice_or_separate_rows() {
        let flat: Vec<ServiceId> = START_ORDER.iter().flat_map(|g| g.iter().copied()).collect();
        assert_eq!(
            flat,
            vec![
                ServiceId::Docker,
                ServiceId::Postgres,
                ServiceId::Letta,
                ServiceId::Weaviate,
                ServiceId::N8n,
                ServiceId::CrewService,
                ServiceId::Homeassistant,
                ServiceId::Freqtrade,
                ServiceId::AmaraRouter,
                ServiceId::HermesGateway,
            ]
        );
        for never in [
            ServiceId::AmaraVoice,
            ServiceId::OpenWebui,
            ServiceId::ModelRuntime,
        ] {
            assert!(!flat.contains(&never));
        }
    }
}
