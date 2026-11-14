//! Services supervisor, phase 8, first ship: OBSERVE ONLY (plan tasks/proposed/phase8-supervisor.md).
//!
//! One background task probes every service on a tick (10 s, 5 s while anything is not healthy), feeds
//! the pure state machine, and keeps a cached snapshot plus an event ring. The commands below only
//! read that cache and never block on a probe. The only processes spawned are read-only
//! `docker inspect` / `docker info`. Every mutating path (auto-restart, start-on-launch, stop-on-quit)
//! sits behind `act::effective()` = the compile-time `SUPERVISOR_ACT_ENABLED` AND the saved,
//! device-tagged setting; with the const false the runner in use refuses mutating verbs regardless.
//! A user-approved manual restart (`service_restart`) is the one mutating path that does not need act.

pub mod act;
pub mod events;
pub mod lifecycle;
pub mod machine;
pub mod probe;
pub mod registry;
pub mod runner;

use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use futures::future::join_all;
use serde::Serialize;

use events::{EventKind, EventRing, ServiceEvent};
use machine::{Action, Machine, Observation, ReasonCode, ServiceState, MAX_ATTEMPTS};
use probe::{Probe, RealProbe};
use registry::{Registry, Runtime, ServiceDef, ServiceId};
use runner::{ActOffRunner, RealRunner, RunError, RunPolicy, Runner, Verb, SUPERVISOR_ACT_ENABLED};

pub const TICK_HEALTHY: Duration = Duration::from_secs(10);
pub const TICK_UNHEALTHY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    User,
    Supervisor,
    Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Can {
    pub restart: bool,
    pub start: bool,
    pub stop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: ServiceId,
    pub state: ServiceState,
    pub reason_code: ReasonCode,
    pub since_ms: u64,
    pub checked_ms: u64,
    pub attempt: u8,
    pub max_attempts: u8,
    pub owner: Owner,
    pub adopted: bool,
    pub can: Can,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServicesSummary {
    pub total: u32,
    pub healthy: u32,
    /// needs_you + degraded + restarting: what should catch the user's eye.
    pub attention: u32,
    pub needs_you: u32,
    pub degraded: u32,
    pub restarting: u32,
    pub stopped: u32,
    pub separate: u32,
    pub unknown: u32,
    pub checked_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorInfo {
    /// Probes, adoption and events: always on.
    pub observing: bool,
    /// Whether the supervisor may act right now: the build switch AND the saved setting.
    pub act_enabled: bool,
    /// The compile-time switch (false in the first ship).
    pub act_build: bool,
    /// The saved, device-tagged setting (`supervisor_set_enabled`).
    pub act_setting: bool,
    pub services: u32,
    pub events: u32,
    pub ticks: u64,
    pub last_tick_ms: Option<u64>,
}

pub struct Supervisor {
    registry: Registry,
    machines: Vec<Machine>,
    events: EventRing,
    ticks: u64,
    last_tick_ms: Option<u64>,
    /// started_by_app: what THIS launch started (start order). Only these are ever stopped on quit.
    started: Vec<ServiceId>,
}

impl Supervisor {
    pub fn new(registry: Registry, now_ms: u64) -> Self {
        let machines = registry
            .defs()
            .iter()
            .map(|d| Machine::new(d, now_ms))
            .collect();
        Self {
            registry,
            machines,
            events: EventRing::new(),
            ticks: 0,
            last_tick_ms: None,
            started: Vec::new(),
        }
    }

    /// Record what start-on-launch started (start order) and log one event each.
    pub fn record_started(&mut self, ids: &[ServiceId], now_ms: u64) {
        for id in ids {
            if self.started.contains(id) {
                continue;
            }
            self.started.push(*id);
            if let Some(i) = self.registry.defs().iter().position(|d| d.id == *id) {
                let m = &self.machines[i];
                self.events
                    .push(now_ms, *id, EventKind::Started, m.state(), m.reason());
            }
        }
    }

    pub fn record_stopped(&mut self, id: ServiceId, now_ms: u64) {
        if let Some(i) = self.registry.defs().iter().position(|d| d.id == id) {
            let m = &self.machines[i];
            self.events
                .push(now_ms, id, EventKind::Stopped, m.state(), m.reason());
        }
        self.started.retain(|s| *s != id);
    }

    /// A manual restart by the user clears that service's automatic-restart budget.
    pub fn manual_restart_done(&mut self, id: ServiceId, now_ms: u64) {
        if let Some(i) = self.registry.defs().iter().position(|d| d.id == id) {
            self.machines[i].manual_reset();
            let m = &self.machines[i];
            self.events.push(
                now_ms,
                id,
                EventKind::RestartRequested,
                m.state(),
                m.reason(),
            );
        }
    }

    pub fn status(&self) -> Vec<ServiceStatus> {
        self.registry
            .defs()
            .iter()
            .zip(&self.machines)
            .map(|(d, m)| status_of(d, m, self.started.contains(&d.id)))
            .collect()
    }

    pub fn summary(&self) -> ServicesSummary {
        let mut s = ServicesSummary {
            total: 0,
            healthy: 0,
            attention: 0,
            needs_you: 0,
            degraded: 0,
            restarting: 0,
            stopped: 0,
            separate: 0,
            unknown: 0,
            checked_ms: self.last_tick_ms.unwrap_or(0),
        };
        for m in &self.machines {
            s.total += 1;
            match m.state() {
                ServiceState::Healthy => s.healthy += 1,
                ServiceState::NeedsYou => s.needs_you += 1,
                ServiceState::Degraded => s.degraded += 1,
                ServiceState::Restarting => s.restarting += 1,
                ServiceState::Stopped => s.stopped += 1,
                ServiceState::Separate => s.separate += 1,
                ServiceState::Unknown | ServiceState::Starting => s.unknown += 1,
            }
        }
        s.attention = s.needs_you + s.degraded + s.restarting;
        s
    }

    pub fn info(&self) -> SupervisorInfo {
        SupervisorInfo {
            observing: true,
            act_enabled: act::effective(),
            act_build: SUPERVISOR_ACT_ENABLED,
            act_setting: act::setting_on(),
            services: self.machines.len() as u32,
            events: self.events.len() as u32,
            ticks: self.ticks,
            last_tick_ms: self.last_tick_ms,
        }
    }

    pub fn events(&self, since: Option<u64>, id: Option<ServiceId>) -> Vec<ServiceEvent> {
        self.events.query(since, id)
    }

    /// True when a row is in trouble or in motion: only then does the tick speed up to 5 s. Stopped
    /// (optional voice, health rows with no store) and Separate rows are normal and never do.
    pub fn wants_fast_tick(&self) -> bool {
        self.machines.iter().any(|m| {
            matches!(
                m.state(),
                ServiceState::NeedsYou
                    | ServiceState::Degraded
                    | ServiceState::Restarting
                    | ServiceState::Starting
            )
        })
    }
}

fn status_of(d: &ServiceDef, m: &Machine, started_by_app: bool) -> ServiceStatus {
    let owner = match m.state() {
        ServiceState::Restarting | ServiceState::Starting => Owner::Transition,
        _ if started_by_app => Owner::Supervisor,
        _ => Owner::User,
    };
    ServiceStatus {
        id: d.id,
        state: m.state(),
        reason_code: m.reason(),
        since_ms: m.since_ms(),
        checked_ms: m.checked_ms(),
        attempt: m.attempt(),
        max_attempts: MAX_ATTEMPTS,
        owner,
        adopted: m.adopted(),
        can: Can {
            // A manual restart is a user action with its own approval: no act needed.
            restart: d.manual_restart,
            // start/stop commands are not built yet; never offered.
            start: false,
            stop: false,
        },
    }
}

fn restart_verb(d: &ServiceDef) -> Option<Verb> {
    match d.runtime {
        Runtime::Launchd(_) => Some(Verb::LaunchctlKickstartK(d.id)),
        Runtime::Docker => Some(Verb::DockerRestart(d.id)),
        _ => None,
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn lock(m: &Mutex<Supervisor>) -> MutexGuard<'_, Supervisor> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// One tick: probe every row concurrently, apply the observations, then run any permitted restart.
/// The lock is never held across an await, so the commands never wait on a probe.
pub async fn run_tick(
    sup: &Mutex<Supervisor>,
    probe: &dyn Probe,
    runner: &dyn Runner,
    act: bool,
    now_ms: u64,
) {
    let defs: Vec<ServiceDef> = lock(sup).registry.defs().to_vec();
    // One batched read per tick (the real probe inspects every container in one call).
    probe.prepare(&defs).await;
    let obs: Vec<Observation> = join_all(defs.iter().map(|d| probe.check(d))).await;

    let mut restarts: Vec<(ServiceId, Verb, u8)> = Vec::new();
    {
        let mut s = lock(sup);
        let Supervisor {
            registry,
            machines,
            events,
            ticks,
            last_tick_ms,
            ..
        } = &mut *s;
        for ((def, m), o) in registry.defs().iter().zip(machines.iter_mut()).zip(obs) {
            let (was, was_adopted) = (m.state(), m.adopted());
            let action = m.observe(now_ms, o, act);
            if !was_adopted && m.adopted() {
                events.push(now_ms, def.id, EventKind::Adopted, m.state(), m.reason());
            }
            if m.state() != was {
                events.push(
                    now_ms,
                    def.id,
                    EventKind::StateChanged,
                    m.state(),
                    m.reason(),
                );
                if m.gave_up() && m.state() == ServiceState::NeedsYou {
                    events.push(now_ms, def.id, EventKind::GaveUp, m.state(), m.reason());
                }
            }
            if let (Action::Restart(n), Some(v)) = (action, restart_verb(def)) {
                events.push(
                    now_ms,
                    def.id,
                    EventKind::RestartRequested,
                    m.state(),
                    m.reason(),
                );
                restarts.push((def.id, v, n));
            }
        }
        *ticks += 1;
        *last_tick_ms = Some(now_ms);
    }

    for (id, verb, _n) in restarts {
        // `act` is false in the shipped build, so this loop is empty there; a fake runner in tests
        // shows the same path. A refusal is recorded, never retried within the tick.
        let result: Result<(), RunError> = runner.run(verb).await;
        if result.is_err() {
            let mut s = lock(sup);
            let st = s
                .registry
                .defs()
                .iter()
                .position(|d| d.id == id)
                .map(|i| (s.machines[i].state(), s.machines[i].reason()));
            if let Some((state, reason)) = st {
                s.events
                    .push(now_ms, id, EventKind::RestartFailed, state, reason);
            }
        }
    }
}

static SUPERVISOR: LazyLock<Mutex<Supervisor>> =
    LazyLock::new(|| Mutex::new(Supervisor::new(Registry::builtin(), unix_ms())));

/// May this tick auto-restart anything? Only with act effective, start-on-launch finished, and no
/// quit drain under way (a restart must never fight the stop sequence).
pub(crate) fn tick_act(effective: bool, launch_done: bool, quit_started: bool) -> bool {
    effective && launch_done && !quit_started
}

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// True once start-on-launch has finished (immediately when act is off). The tick loop does not
/// auto-restart anything before then, so the two never race over the same service.
static LAUNCH_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What this launch started (start order), for the quit hook.
pub(crate) fn started_snapshot() -> Vec<ServiceId> {
    lock(&SUPERVISOR).started.clone()
}

/// A manual restart succeeded: clear the machine's attempt counter for that service.
pub(crate) fn manual_restart_done(id: ServiceId) {
    lock(&SUPERVISOR).manual_restart_done(id, unix_ms());
}

/// Start-on-launch with the real probe, runner and crew client; records what it started. Only called
/// when `act::effective()`.
async fn launch_services() {
    use lifecycle::{start_on_launch, Deps, RealCrewRuns, SysClock};
    let registry = Registry::builtin();
    let probe = RealProbe::new();
    let runner = RealRunner::new(RunPolicy::Full);
    let clock = SysClock::new();
    let deps = Deps {
        probe: &probe,
        runner: &runner,
        clock: &clock,
        crew: &RealCrewRuns,
        registry: &registry,
    };
    let report = start_on_launch(&deps, act::effective()).await;
    lock(&SUPERVISOR).record_started(&report.started, unix_ms());
}

/// Start the probe task (and, only when act is effectively on, start-on-launch). Idempotent. Called
/// once from the app's `setup`, after `launch_token::mint`.
pub fn start() {
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    act::init();
    if act::effective() {
        tauri::async_runtime::spawn(async {
            launch_services().await;
            LAUNCH_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    } else {
        LAUNCH_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    tauri::async_runtime::spawn(async {
        let probe = RealProbe::new();
        let mut acting: Option<RealRunner<runner::SysExec>> = None;
        let mut logged = 0u64;
        loop {
            let act = tick_act(
                act::effective(),
                LAUNCH_DONE.load(std::sync::atomic::Ordering::SeqCst),
                lifecycle::quit_started(),
            );
            // With act off the runner is `ActOffRunner`: it refuses every mutating verb. The acting
            // runner is only built inside this branch.
            let runner: &dyn Runner = if act {
                &*acting.get_or_insert_with(|| RealRunner::new(RunPolicy::Full))
            } else {
                &ActOffRunner
            };
            run_tick(&SUPERVISOR, &probe, runner, act, unix_ms()).await;
            let (interval, fresh) = {
                let s = lock(&SUPERVISOR);
                (
                    if s.wants_fast_tick() {
                        TICK_UNHEALTHY
                    } else {
                        TICK_HEALTHY
                    },
                    s.events(Some(logged), None),
                )
            };
            for e in &fresh {
                // One fixed line per event: id, kind, state, reason code. No command output.
                super::unified_log::log_line(
                    "supervisor",
                    super::unified_log::Level::Info,
                    &format!("{:?} {:?} {:?} {:?}", e.id, e.kind, e.state, e.reason_code),
                );
                logged = e.seq;
            }
            tokio::time::sleep(interval).await;
        }
    });
}

// ---- commands ----

/// The cached snapshot; never probes, never blocks on one.
#[tauri::command]
#[specta::specta]
pub async fn services_status() -> Vec<ServiceStatus> {
    lock(&SUPERVISOR).status()
}

/// Events with `seq > since` (oldest first), optionally for one service. Fixed texts only.
#[tauri::command]
#[specta::specta]
pub async fn service_events(since: Option<u64>, id: Option<ServiceId>) -> Vec<ServiceEvent> {
    lock(&SUPERVISOR).events(since, id)
}

#[tauri::command]
#[specta::specta]
pub async fn services_summary() -> ServicesSummary {
    lock(&SUPERVISOR).summary()
}

#[tauri::command]
#[specta::specta]
pub async fn supervisor_get() -> SupervisorInfo {
    lock(&SUPERVISOR).info()
}

/// Turn the act setting on or off. Enabling asks the native approval; disabling never asks. It only
/// takes effect in a build where `SUPERVISOR_ACT_ENABLED` is true, and start-on-launch only runs at the
/// next launch.
#[tauri::command]
#[specta::specta]
pub async fn supervisor_set_enabled(
    app: tauri::AppHandle,
    on: bool,
) -> Result<SupervisorInfo, String> {
    act::set_enabled(&app, on).await?;
    Ok(lock(&SUPERVISOR).info())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use std::collections::HashMap;

    struct FakeProbe(Mutex<HashMap<ServiceId, Observation>>);
    impl FakeProbe {
        fn all(o: Observation) -> Self {
            Self(Mutex::new(ServiceId::ALL.iter().map(|i| (*i, o)).collect()))
        }
        fn set(&self, id: ServiceId, o: Observation) {
            self.0.lock().unwrap().insert(id, o);
        }
    }
    impl Probe for FakeProbe {
        fn check<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, Observation> {
            let o = self.0.lock().unwrap()[&def.id];
            Box::pin(async move { o })
        }
    }

    #[derive(Default)]
    struct FakeRunner(Mutex<Vec<Verb>>);
    impl Runner for FakeRunner {
        fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
            self.0.lock().unwrap().push(verb);
            Box::pin(async { Ok(()) })
        }
    }

    fn sup() -> Mutex<Supervisor> {
        Mutex::new(Supervisor::new(Registry::builtin(), 0))
    }

    fn state(s: &Mutex<Supervisor>, id: ServiceId) -> ServiceStatus {
        lock(s).status().into_iter().find(|x| x.id == id).unwrap()
    }

    const DOWN: Observation = Observation::Down(ReasonCode::Unreachable);

    #[tokio::test]
    async fn before_any_tick_every_row_is_unknown_and_not_checked() {
        let s = sup();
        let all = lock(&s).status();
        assert_eq!(all.len(), 15);
        assert!(all
            .iter()
            .all(|r| r.state == ServiceState::Unknown && r.reason_code == ReasonCode::NotChecked));
        assert!(lock(&s).events(None, None).is_empty());
    }

    #[tokio::test]
    async fn a_healthy_start_adopts_everything_with_zero_verbs() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        run_tick(&s, &p, &r, true, 1_000).await;
        let all = lock(&s).status();
        assert!(all.iter().all(|x| x.adopted));
        assert_eq!(state(&s, ServiceId::N8n).state, ServiceState::Healthy);
        assert_eq!(
            state(&s, ServiceId::OpenWebui).state,
            ServiceState::Separate
        );
        assert!(r.0.lock().unwrap().is_empty());
        let ev = lock(&s).events(None, Some(ServiceId::N8n));
        assert_eq!(ev.len(), 2); // adopted + healthy
        assert_eq!(ev[0].kind, EventKind::Adopted);
        let sum = lock(&s).summary();
        assert_eq!(
            (sum.total, sum.healthy, sum.separate, sum.attention),
            (15, 14, 1, 0)
        );
    }

    #[tokio::test]
    async fn act_off_a_full_failure_scenario_runs_zero_mutating_verbs_but_states_update() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        run_tick(&s, &p, &r, false, 0).await;
        for id in ServiceId::ALL {
            p.set(id, DOWN);
        }
        let mut t = 5_000;
        for _ in 0..60 {
            run_tick(&s, &p, &r, false, t).await;
            t += 5_000;
        }
        assert!(r.0.lock().unwrap().iter().all(|v| !v.is_mutating()));
        assert!(r.0.lock().unwrap().is_empty());
        assert_eq!(state(&s, ServiceId::N8n).state, ServiceState::NeedsYou);
        assert_eq!(
            state(&s, ServiceId::AmaraRouter).state,
            ServiceState::NeedsYou
        );
        assert_eq!(
            state(&s, ServiceId::AmaraVoice).state,
            ServiceState::Stopped
        );
        assert_eq!(
            state(&s, ServiceId::OpenWebui).state,
            ServiceState::Separate
        );
        assert_eq!(state(&s, ServiceId::N8n).attempt, 0);
        assert!(lock(&s).summary().needs_you >= 10);
    }

    #[tokio::test]
    async fn act_on_with_a_fake_runner_restarts_three_times_then_needs_you() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        run_tick(&s, &p, &r, true, 0).await;
        p.set(ServiceId::N8n, DOWN);
        p.set(ServiceId::OpenWebui, DOWN);
        p.set(ServiceId::AmaraVoice, DOWN);
        let mut t = 1_000;
        for _ in 0..80 {
            run_tick(&s, &p, &r, true, t).await;
            t += 5_000;
        }
        let verbs = r.0.lock().unwrap().clone();
        assert_eq!(verbs, vec![Verb::DockerRestart(ServiceId::N8n); 3]);
        let n8n = state(&s, ServiceId::N8n);
        assert_eq!(
            (n8n.state, n8n.reason_code),
            (ServiceState::NeedsYou, ReasonCode::RestartFailed)
        );
        let kinds: Vec<EventKind> = lock(&s)
            .events(None, Some(ServiceId::N8n))
            .iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == EventKind::RestartRequested)
                .count(),
            3
        );
        assert!(kinds.contains(&EventKind::GaveUp));
    }

    #[tokio::test]
    async fn a_bad_bind_is_needs_you_and_never_restarted() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        p.set(
            ServiceId::CrewService,
            Observation::NeedsYou(ReasonCode::BadBind),
        );
        for k in 0..30 {
            run_tick(&s, &p, &r, true, k * 5_000).await;
        }
        let c = state(&s, ServiceId::CrewService);
        assert_eq!(
            (c.state, c.reason_code),
            (ServiceState::NeedsYou, ReasonCode::BadBind)
        );
        assert!(r.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn status_never_waits_for_a_slow_probe() {
        struct Slow;
        impl Probe for Slow {
            fn check<'a>(&'a self, _d: &'a ServiceDef) -> BoxFuture<'a, Observation> {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    Observation::Up
                })
            }
        }
        let s = std::sync::Arc::new(sup());
        let s2 = s.clone();
        let tick = tokio::spawn(async move {
            run_tick(&s2, &Slow, &FakeRunner::default(), false, 1).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let started = std::time::Instant::now();
        let snap = lock(&s).status();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(snap.iter().all(|r| r.state == ServiceState::Unknown));
        tick.await.unwrap();
        assert_eq!(state(&s, ServiceId::N8n).state, ServiceState::Healthy);
    }

    #[test]
    fn can_flags_never_offer_start_or_stop_while_act_is_off() {
        let s = sup();
        for r in lock(&s).status() {
            assert!(!r.can.start && !r.can.stop, "{:?}", r.id);
        }
        assert!(state(&s, ServiceId::CrewService).can.restart);
        // The router and Hermes joined the manual-restart ids (launchd kickstart, with approval).
        assert!(state(&s, ServiceId::AmaraRouter).can.restart);
        assert!(state(&s, ServiceId::HermesGateway).can.restart);
        assert!(!state(&s, ServiceId::AmaraVoice).can.restart);
        let info = lock(&s).info();
        assert!(info.observing && !info.act_enabled);
    }

    #[test]
    fn ipc_shapes_are_camel_case_with_snake_case_enums() {
        let s = sup();
        let v = serde_json::to_value(&lock(&s).status()[0]).unwrap();
        assert_eq!(v["id"], "docker");
        assert_eq!(v["state"], "unknown");
        assert_eq!(v["reasonCode"], "not_checked");
        assert_eq!(v["maxAttempts"], 3);
        assert_eq!(v["can"]["restart"], false);
        assert!(v.get("sinceMs").is_some() && v.get("checkedMs").is_some());
    }

    // ---- source pin: the supervisor never contains a mutating primitive ----

    /// True when `tok` occurs in `src` not glued to a longer word (so `skill` does not hit `kill`,
    /// `kill_on_drop` does not hit `kill`, and `form ` does not hit `rm `).
    fn has_token(src: &str, tok: &str) -> bool {
        let (src, tok) = (src.to_lowercase(), tok.to_lowercase());
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let first_word = tok.chars().next().is_some_and(word);
        let last_word = tok.chars().last().is_some_and(word);
        src.match_indices(&tok).any(|(i, _)| {
            let before_ok = !first_word || !src[..i].chars().last().is_some_and(word);
            let after_ok = !last_word || !src[i + tok.len()..].chars().next().is_some_and(word);
            before_ok && after_ok
        })
    }

    #[test]
    fn the_token_matcher_is_word_aware() {
        assert!(has_token("run kill now", "kill"));
        assert!(!has_token("kill_on_drop(true)", "kill"));
        assert!(!has_token("a skill", "kill"));
        assert!(has_token("sh -c \"rm -rf\"", "rm "));
        assert!(!has_token("perform it", "rm "));
        assert!(has_token("docker run x", "docker run"));
        assert!(has_token("cargo build --build", "--build"));
    }

    #[test]
    fn source_pin_no_mutating_primitive_in_the_supervisor() {
        const FORBIDDEN: &[&str] = &[
            "kill",
            "bootstrap",
            "bootout",
            "unload",
            "launchctl load",
            "submit",
            "rm ",
            "compose up",
            "--build",
            "pull",
            "docker run",
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/supervisor");
        let mut files = 0;
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            if p.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            files += 1;
            let src = std::fs::read_to_string(&p).unwrap();
            // Production code only: the tests (and this list) are cut off.
            let prod = src.split("#[cfg(test)]").next().unwrap();
            for tok in FORBIDDEN {
                assert!(!has_token(prod, tok), "{} contains `{tok}`", p.display());
            }
        }
        assert!(
            files >= 8,
            "expected mod, registry, machine, events, probe, runner, lifecycle, act"
        );
    }

    #[test]
    fn only_the_runner_spawns_a_process_and_the_probe_goes_through_it() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/supervisor");
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") {
                continue;
            }
            let src = std::fs::read_to_string(&p).unwrap();
            let prod = src.split("#[cfg(test)]").next().unwrap();
            let spawns = prod.contains("Command::new");
            assert_eq!(spawns, name == "runner.rs", "{name}");
        }
        // The probe's only docker verbs are inspect and info.
        let probe = include_str!("probe.rs");
        let probe = probe.split("#[cfg(test)]").next().unwrap();
        assert!(probe.contains("run_capped("));
        for bad in ["\"start\"", "\"stop\"", "\"restart\"", "\"kickstart\""] {
            assert!(!probe.contains(bad), "{bad}");
        }
        // Launchd verbs are spelled only in the runner, and only the kickstart plus the loaded check.
        let runner = include_str!("runner.rs");
        let runner = runner.split("#[cfg(test)]").next().unwrap();
        assert_eq!(runner.matches("Prog::Launchctl").count(), 3);
        assert!(runner.contains("\"kickstart\"") && runner.contains("\"list\""));
    }

    #[test]
    fn started_by_app_shows_as_supervisor_owned_and_a_manual_restart_resets_the_counter() {
        let s = sup();
        {
            let mut g = lock(&s);
            g.record_started(&[ServiceId::N8n, ServiceId::N8n], 5);
            assert_eq!(g.started, vec![ServiceId::N8n]);
        }
        let ev = lock(&s).events(None, Some(ServiceId::N8n));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, EventKind::Started);
        assert_eq!(ev[0].text, "Started by Amara OS");
        assert_eq!(state(&s, ServiceId::N8n).owner, Owner::Supervisor);
        assert_eq!(state(&s, ServiceId::Letta).owner, Owner::User);
        lock(&s).record_stopped(ServiceId::N8n, 9);
        assert!(lock(&s).started.is_empty());
        assert_eq!(state(&s, ServiceId::N8n).owner, Owner::User);
    }

    #[tokio::test]
    async fn manual_restart_done_clears_a_spent_attempt_budget() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        run_tick(&s, &p, &r, true, 0).await;
        p.set(ServiceId::N8n, DOWN);
        let mut t = 1_000;
        for _ in 0..80 {
            run_tick(&s, &p, &r, true, t).await;
            t += 5_000;
        }
        let n8n = state(&s, ServiceId::N8n);
        assert_eq!(n8n.state, ServiceState::NeedsYou);
        assert_eq!(n8n.attempt, 3);
        lock(&s).manual_restart_done(ServiceId::N8n, t);
        assert_eq!(state(&s, ServiceId::N8n).attempt, 0);
        // The machine may act again: a fresh failure restarts once more.
        let before = r.0.lock().unwrap().len();
        for _ in 0..30 {
            run_tick(&s, &p, &r, true, t).await;
            t += 5_000;
        }
        assert!(r.0.lock().unwrap().len() > before);
    }

    #[tokio::test]
    async fn the_probe_is_prepared_once_per_tick_before_any_check() {
        use std::sync::atomic::{AtomicU32, Ordering};
        struct Counting(AtomicU32, AtomicU32);
        impl Probe for Counting {
            fn prepare<'a>(&'a self, defs: &'a [ServiceDef]) -> BoxFuture<'a, ()> {
                assert_eq!(defs.len(), 15);
                assert_eq!(
                    self.1.load(Ordering::SeqCst) % 15,
                    0,
                    "prepare before checks"
                );
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async {})
            }
            fn check<'a>(&'a self, _d: &'a ServiceDef) -> BoxFuture<'a, Observation> {
                self.1.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Observation::Up })
            }
        }
        let s = sup();
        let p = Counting(AtomicU32::new(0), AtomicU32::new(0));
        for k in 0..3 {
            run_tick(&s, &p, &FakeRunner::default(), false, k * 1_000).await;
        }
        assert_eq!(p.0.load(Ordering::SeqCst), 3);
        assert_eq!(p.1.load(Ordering::SeqCst), 45);
    }

    #[test]
    fn the_tick_never_acts_once_a_quit_has_begun() {
        assert!(tick_act(true, true, false));
        assert!(!tick_act(true, true, true));
        assert!(!tick_act(true, false, false));
        assert!(!tick_act(false, true, false));
    }

    #[tokio::test]
    async fn a_down_service_during_the_quit_drain_gets_no_verb() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        let r = FakeRunner::default();
        run_tick(&s, &p, &r, true, 0).await;
        p.set(ServiceId::N8n, DOWN);
        // Draining: the loop computes act = false, so nothing is run however long it stays down.
        let act = tick_act(true, true, true);
        let mut t = 1_000;
        for _ in 0..40 {
            run_tick(&s, &p, &r, act, t).await;
            t += 5_000;
        }
        assert!(r.0.lock().unwrap().is_empty());
        // The same scenario with no drain does restart it.
        run_tick(&s, &p, &r, tick_act(true, true, false), t).await;
        for _ in 0..10 {
            t += 5_000;
            run_tick(&s, &p, &r, true, t).await;
        }
        assert!(!r.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_trouble_or_motion_speeds_the_tick_up() {
        let s = sup();
        let p = FakeProbe::all(Observation::Up);
        run_tick(&s, &p, &FakeRunner::default(), false, 0).await;
        assert!(!lock(&s).wants_fast_tick());
        // Optional voice stopped, a health row stopped, Open WebUI separate: still slow.
        p.set(ServiceId::AmaraVoice, DOWN);
        p.set(
            ServiceId::HealthReader,
            Observation::Stopped(ReasonCode::NoStore),
        );
        p.set(ServiceId::OpenWebui, DOWN);
        let mut t = 5_000;
        for _ in 0..20 {
            run_tick(&s, &p, &FakeRunner::default(), false, t).await;
            t += 5_000;
        }
        assert_eq!(
            state(&s, ServiceId::AmaraVoice).state,
            ServiceState::Stopped
        );
        assert_eq!(
            state(&s, ServiceId::HealthReader).state,
            ServiceState::Stopped
        );
        assert!(!lock(&s).wants_fast_tick());
        for o in [
            Observation::Starting(ReasonCode::ContainerStarting),
            Observation::NeedsYou(ReasonCode::BadBind),
        ] {
            p.set(ServiceId::N8n, o);
            run_tick(&s, &p, &FakeRunner::default(), false, t).await;
            assert!(lock(&s).wants_fast_tick(), "{o:?}");
            p.set(ServiceId::N8n, Observation::Up);
            run_tick(&s, &p, &FakeRunner::default(), false, t + 1).await;
            assert!(!lock(&s).wants_fast_tick());
        }
    }

    #[test]
    fn info_reports_the_effective_act_state_and_both_inputs() {
        let s = sup();
        let i = lock(&s).info();
        assert!(!i.act_build && !i.act_enabled);
        let v = serde_json::to_value(&i).unwrap();
        assert_eq!(v["actBuild"], false);
        assert!(v.get("actSetting").is_some());
    }
}
