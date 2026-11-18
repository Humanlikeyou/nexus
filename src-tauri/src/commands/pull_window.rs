//! The model-download window lifecycle (decision 0031 section 4, handoff 25a M1).
//!
//! The pull window is an UNSANDBOXED `ollama serve` on :11435 (it must reach registry.ollama.ai:443).
//! The sandbox profile of the long-running serve cannot deny single loopback ports, so a sandboxed
//! serve could reach the window. The mitigation is mutual exclusion: the window may only run while
//! the sandboxed serve is NOT running.
//!
//! Order of one approved pull (after every model_pull gate and the native approval):
//! take the slot, refuse a foreign runtime (`runtime_busy`, never killed), stop the sandboxed serve
//! if the app started it, spawn the window, wait for it, (the pull runs), stop the window and wait
//! until it is gone, restart the serve only if it was running before. A drop guard kills the window
//! on any early exit or cancel. One slot serialises pulls and start-all (`pull_in_progress`).

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures::future::BoxFuture;

use super::supervisor::registry::ServiceId;
use super::supervisor::runner::{PortProbe, PortState, SidecarRunner, Spawner, WindowProc};

pub const PULL_IN_PROGRESS: &str = "pull_in_progress";
pub const RUNTIME_BUSY: &str = "runtime_busy";
const BUSY: &str = "busy";

const SERVE_STOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Download,
    StartAll,
}

/// A live view of "some process of the download window's group may still exist".
pub type Liveness = Arc<dyn Fn() -> bool + Send + Sync>;

/// One holder at a time, process-wide: a pull, or a start-all, never both. Even after the holder is
/// gone, the slot refuses new holders and the sandboxed serve while the window may still be alive
/// (a cancel, or a window that is slow to die).
#[derive(Default)]
pub struct PullSlot {
    holder: Mutex<Option<Holder>>,
    window: Mutex<Option<Liveness>>,
}

pub struct SlotGuard(Arc<PullSlot>);

impl PullSlot {
    fn cell(&self) -> std::sync::MutexGuard<'_, Option<Holder>> {
        self.holder.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Registers the window just spawned; it stays registered until its group is seen empty.
    pub fn set_window(&self, probe: Liveness) {
        *self.window.lock().unwrap_or_else(|p| p.into_inner()) = Some(probe);
    }
    /// True while a window spawned through this slot may still have a live process.
    pub fn window_may_be_alive(&self) -> bool {
        let mut w = self.window.lock().unwrap_or_else(|p| p.into_inner());
        match w.as_ref() {
            Some(f) if f() => true,
            Some(_) => {
                *w = None;
                false
            }
            None => false,
        }
    }
    /// Takes the slot, or says who holds it.
    pub fn try_acquire(self: &Arc<Self>, h: Holder) -> Result<SlotGuard, Holder> {
        if self.window_may_be_alive() {
            return Err(Holder::Download);
        }
        let mut g = self.cell();
        match *g {
            Some(cur) => Err(cur),
            None => {
                *g = Some(h);
                Ok(SlotGuard(self.clone()))
            }
        }
    }
    pub fn held_by(&self) -> Option<Holder> {
        *self.cell()
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        *self.0.cell() = None;
    }
}

/// The process-wide slot the real runner and the pull command share.
pub fn global_slot() -> Arc<PullSlot> {
    static SLOT: OnceLock<Arc<PullSlot>> = OnceLock::new();
    SLOT.get_or_init(Default::default).clone()
}

/// The fixed code a start-all gets when the slot is taken.
pub fn start_all_refusal(h: Holder) -> &'static str {
    match h {
        Holder::Download => PULL_IN_PROGRESS,
        Holder::StartAll => BUSY,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeState {
    /// Nothing answers or holds :11434.
    Absent,
    /// The app started the sandboxed serve and it is alive.
    Ours,
    /// Something else holds :11434 (for example Ollama.app). Never touched.
    Foreign,
}

/// Everything the lifecycle does to the outside world, so tests use fakes and start no process.
pub trait PullHost: Send + Sync {
    fn slot(&self) -> &Arc<PullSlot>;
    fn serve_state(&self) -> BoxFuture<'_, ServeState>;
    fn window_port_free(&self) -> BoxFuture<'_, bool>;
    /// Stops the serve the app started and waits until :11434 is free.
    fn stop_serve(&self) -> BoxFuture<'_, Result<(), &'static str>>;
    fn spawn_window(&self) -> Result<(), &'static str>;
    fn window_ready(&self) -> BoxFuture<'_, bool>;
    /// Whether the window child is running right now.
    fn window_alive(&self) -> bool;
    fn window_liveness(&self) -> Liveness;
    /// Synchronous and idempotent (used by the drop guard).
    fn kill_window(&self);
    /// True once the window process has exited.
    fn wait_window_gone(&self) -> BoxFuture<'_, bool>;
    fn start_serve(&self) -> BoxFuture<'_, Result<(), &'static str>>;
}

/// A live window lifecycle. `finish` is the normal end; `Drop` is the backstop (cancel, panic).
pub struct PullSession<'a> {
    host: &'a dyn PullHost,
    window_up: bool,
    restart_serve: bool,
    // Declared last: released after `Drop::drop` has killed the window.
    _slot: SlotGuard,
}

impl PullSession<'_> {
    /// Stops the window (and waits for it), then restarts the serve only if it ran before and the
    /// window is really gone, so the two are never up together.
    pub async fn finish(mut self) {
        let mut gone = true;
        if self.window_up {
            self.host.kill_window();
            gone = self.host.wait_window_gone().await;
            self.window_up = false;
        }
        if self.restart_serve && gone {
            let _ = self.host.start_serve().await;
        }
        self.restart_serve = false;
    }
}

impl Drop for PullSession<'_> {
    fn drop(&mut self) {
        if self.window_up {
            self.host.kill_window();
        }
    }
}

/// Opens the window: slot, foreign check, stop serve, spawn, wait. On any failure everything it did
/// is undone (window stopped, serve restored) before the fixed code is returned.
pub async fn begin(host: &dyn PullHost) -> Result<PullSession<'_>, &'static str> {
    let slot = host
        .slot()
        .try_acquire(Holder::Download)
        .map_err(|h| match h {
            Holder::Download => PULL_IN_PROGRESS,
            Holder::StartAll => BUSY,
        })?;
    let mut s = PullSession {
        host,
        window_up: false,
        restart_serve: false,
        _slot: slot,
    };
    let state = host.serve_state().await;
    if state == ServeState::Foreign || !host.window_port_free().await {
        return Err(RUNTIME_BUSY);
    }
    if state == ServeState::Ours {
        s.restart_serve = true;
        if let Err(e) = host.stop_serve().await {
            s.finish().await;
            return Err(e);
        }
    }
    // Re-probe :11434 right before the window starts: something else may have taken it.
    if host.serve_state().await != ServeState::Absent {
        s.finish().await;
        return Err(RUNTIME_BUSY);
    }
    if let Err(e) = host.spawn_window() {
        s.finish().await;
        return Err(e);
    }
    s.window_up = true;
    // From here the slot refuses new holders and the serve until this window's group is gone.
    host.slot().set_window(host.window_liveness());
    if !host.window_ready().await {
        s.finish().await;
        return Err(super::model_pull::RUNTIME_UNAVAILABLE);
    }
    Ok(s)
}

/// The production host: the sidecar runner (serve) plus a window process.
pub struct RunnerHost<'a, S: Spawner, P: PortProbe, W: WindowProc> {
    pub runner: &'a SidecarRunner<S, P>,
    pub window: &'a W,
}

impl<S: Spawner, P: PortProbe, W: WindowProc> PullHost for RunnerHost<'_, S, P, W> {
    fn slot(&self) -> &Arc<PullSlot> {
        self.runner.slot()
    }
    fn serve_state(&self) -> BoxFuture<'_, ServeState> {
        Box::pin(async move {
            if self.runner.spawner.is_alive(ServiceId::ModelRuntime) {
                return ServeState::Ours;
            }
            match self.runner.port_state(ServiceId::ModelRuntime).await {
                Some(PortState::Free) => ServeState::Absent,
                _ => ServeState::Foreign,
            }
        })
    }
    fn window_port_free(&self) -> BoxFuture<'_, bool> {
        self.window.port_free()
    }
    fn stop_serve(&self) -> BoxFuture<'_, Result<(), &'static str>> {
        Box::pin(async move {
            // Only a child the app started is ever stopped.
            // Wait for the port to be free AND the whole process group (the serve's llama-server
            // runners too) to be gone: a surviving sandboxed runner could reach the window.
            let group = self.runner.spawner.stop_group(ServiceId::ModelRuntime);
            let deadline = tokio::time::Instant::now() + SERVE_STOP_TIMEOUT;
            loop {
                let group_gone = group.is_none_or(super::proc_group::group_empty);
                if group_gone
                    && self.runner.port_state(ServiceId::ModelRuntime).await
                        == Some(PortState::Free)
                {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    // Fail closed: force the group down and refuse the pull either way.
                    if let Some(g) = group {
                        super::proc_group::force(g);
                    }
                    return Err("timed_out");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    }
    fn spawn_window(&self) -> Result<(), &'static str> {
        let spec = self
            .runner
            .pull_window_spec()
            .ok_or("runtime_unavailable")?;
        self.window.spawn(&spec)
    }
    fn window_ready(&self) -> BoxFuture<'_, bool> {
        self.window.ready()
    }
    fn kill_window(&self) {
        self.window.stop();
    }
    fn wait_window_gone(&self) -> BoxFuture<'_, bool> {
        self.window.gone()
    }
    fn window_alive(&self) -> bool {
        self.window.alive()
    }
    fn window_liveness(&self) -> Liveness {
        self.window.liveness()
    }
    fn start_serve(&self) -> BoxFuture<'_, Result<(), &'static str>> {
        Box::pin(async move {
            self.runner
                .start_after_pull(ServiceId::ModelRuntime)
                .await
                .map(|_| ())
                .map_err(|_| "runtime_unavailable")
        })
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A recording host: serve and window are booleans; `overlap` trips if both are ever up.
    pub struct FakeHost {
        pub slot: Arc<PullSlot>,
        pub log: Mutex<Vec<&'static str>>,
        pub cur: Mutex<ServeState>,
        pub serve_up: AtomicBool,
        pub window_up: AtomicBool,
        pub overlap: AtomicBool,
        pub port_busy: bool,
        pub fail_stop: bool,
        pub fail_spawn: bool,
        pub ready: bool,
        /// After the serve is stopped, something else takes :11434.
        pub squat_after_stop: bool,
        /// The window process exits right after it is ready (it lost the bind race).
        pub window_dies: bool,
        /// A stopped window is slow to die: `gone` is false and the group stays alive.
        pub linger: bool,
        pub win_flag: Arc<AtomicBool>,
    }

    impl FakeHost {
        pub fn new(state: ServeState) -> Self {
            Self {
                slot: Default::default(),
                log: Default::default(),
                cur: Mutex::new(state),
                serve_up: AtomicBool::new(state == ServeState::Ours),
                window_up: AtomicBool::new(false),
                overlap: AtomicBool::new(false),
                port_busy: false,
                fail_stop: false,
                fail_spawn: false,
                ready: true,
                squat_after_stop: false,
                window_dies: false,
                linger: false,
                win_flag: Default::default(),
            }
        }
        fn note(&self, e: &'static str) {
            self.log.lock().unwrap().push(e);
        }
        fn check(&self) {
            if self.serve_up.load(Ordering::SeqCst) && self.window_up.load(Ordering::SeqCst) {
                self.overlap.store(true, Ordering::SeqCst);
            }
        }
        pub fn log(&self) -> Vec<&'static str> {
            self.log.lock().unwrap().clone()
        }
    }

    impl PullHost for FakeHost {
        fn slot(&self) -> &Arc<PullSlot> {
            &self.slot
        }
        fn serve_state(&self) -> BoxFuture<'_, ServeState> {
            Box::pin(async move { *self.cur.lock().unwrap() })
        }
        fn window_port_free(&self) -> BoxFuture<'_, bool> {
            Box::pin(async move { !self.port_busy })
        }
        fn stop_serve(&self) -> BoxFuture<'_, Result<(), &'static str>> {
            Box::pin(async move {
                self.note("stop_serve");
                if self.fail_stop {
                    return Err("timed_out");
                }
                self.serve_up.store(false, Ordering::SeqCst);
                *self.cur.lock().unwrap() = if self.squat_after_stop {
                    ServeState::Foreign
                } else {
                    ServeState::Absent
                };
                Ok(())
            })
        }
        fn spawn_window(&self) -> Result<(), &'static str> {
            self.note("spawn_window");
            if self.fail_spawn {
                return Err("runtime_unavailable");
            }
            self.window_up.store(true, Ordering::SeqCst);
            self.win_flag.store(true, Ordering::SeqCst);
            self.check();
            Ok(())
        }
        fn window_ready(&self) -> BoxFuture<'_, bool> {
            Box::pin(async move {
                self.note("window_ready");
                self.ready
            })
        }
        fn kill_window(&self) {
            self.note("kill_window");
            self.window_up.store(false, Ordering::SeqCst);
            if !self.linger {
                self.win_flag.store(false, Ordering::SeqCst);
            }
        }
        fn wait_window_gone(&self) -> BoxFuture<'_, bool> {
            Box::pin(async move {
                self.note("window_gone");
                !self.linger
            })
        }
        fn window_alive(&self) -> bool {
            self.window_up.load(Ordering::SeqCst) && !self.window_dies
        }
        fn window_liveness(&self) -> Liveness {
            let f = self.win_flag.clone();
            Arc::new(move || f.load(Ordering::SeqCst))
        }
        fn start_serve(&self) -> BoxFuture<'_, Result<(), &'static str>> {
            Box::pin(async move {
                self.note("start_serve");
                *self.cur.lock().unwrap() = ServeState::Ours;
                self.serve_up.store(true, Ordering::SeqCst);
                self.check();
                Ok(())
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::FakeHost;
    use super::*;
    use crate::commands::supervisor::registry::{Registry, ServiceDef};
    use crate::commands::supervisor::runner::{RunError, RunPolicy, SpawnSpec};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn order_is_stop_serve_window_up_window_down_serve_back() {
        let h = FakeHost::new(ServeState::Ours);
        let s = begin(&h).await.unwrap();
        assert_eq!(h.log(), ["stop_serve", "spawn_window", "window_ready"]);
        s.finish().await;
        assert_eq!(
            h.log(),
            [
                "stop_serve",
                "spawn_window",
                "window_ready",
                "kill_window",
                "window_gone",
                "start_serve"
            ]
        );
        assert!(!h.overlap.load(Ordering::SeqCst));
        assert!(h.slot.held_by().is_none());
    }

    #[tokio::test]
    async fn serve_is_restarted_only_if_it_was_running() {
        let h = FakeHost::new(ServeState::Absent);
        begin(&h).await.unwrap().finish().await;
        assert!(!h.log().contains(&"stop_serve") && !h.log().contains(&"start_serve"));
    }

    #[tokio::test]
    async fn a_foreign_runtime_is_refused_and_nothing_is_stopped_or_spawned() {
        let h = FakeHost::new(ServeState::Foreign);
        assert_eq!(begin(&h).await.err(), Some(RUNTIME_BUSY));
        assert!(h.log().is_empty());
        assert!(h.slot.held_by().is_none());
        // A stale listener on the window port is refused the same way, before any stop.
        let mut h = FakeHost::new(ServeState::Ours);
        h.port_busy = true;
        assert_eq!(begin(&h).await.err(), Some(RUNTIME_BUSY));
        assert!(h.log().is_empty());
    }

    #[tokio::test]
    async fn begin_failures_restore_the_serve_and_leave_no_window() {
        let mut h = FakeHost::new(ServeState::Ours);
        h.fail_spawn = true;
        assert_eq!(begin(&h).await.err(), Some("runtime_unavailable"));
        assert_eq!(h.log(), ["stop_serve", "spawn_window", "start_serve"]);
        assert!(h.serve_up.load(Ordering::SeqCst) && !h.window_up.load(Ordering::SeqCst));

        let mut h = FakeHost::new(ServeState::Ours);
        h.ready = false;
        assert_eq!(begin(&h).await.err(), Some("runtime_unavailable"));
        assert!(h
            .log()
            .ends_with(&["kill_window", "window_gone", "start_serve"]));
        assert!(!h.window_up.load(Ordering::SeqCst) && h.serve_up.load(Ordering::SeqCst));

        let mut h = FakeHost::new(ServeState::Ours);
        h.fail_stop = true;
        assert_eq!(begin(&h).await.err(), Some("timed_out"));
        assert!(!h.log().contains(&"spawn_window"));
        assert!(!h.overlap.load(Ordering::SeqCst));
        assert!(h.slot.held_by().is_none());
    }

    #[tokio::test]
    async fn dropping_a_session_kills_the_window_and_frees_the_slot() {
        let h = FakeHost::new(ServeState::Ours);
        let s = begin(&h).await.unwrap();
        assert_eq!(h.slot.held_by(), Some(Holder::Download));
        drop(s); // cancel path
        assert!(!h.window_up.load(Ordering::SeqCst));
        assert!(h.slot.held_by().is_none());
    }

    #[tokio::test]
    async fn a_second_pull_or_a_start_all_cannot_overlap() {
        let h = FakeHost::new(ServeState::Absent);
        let s = begin(&h).await.unwrap();
        assert_eq!(begin(&h).await.err(), Some(PULL_IN_PROGRESS));
        assert_eq!(
            h.slot.try_acquire(Holder::StartAll).err(),
            Some(Holder::Download)
        );
        assert_eq!(start_all_refusal(Holder::Download), PULL_IN_PROGRESS);
        s.finish().await;
        // And a start-all in flight refuses a pull.
        let g = h.slot.try_acquire(Holder::StartAll).unwrap();
        assert_eq!(begin(&h).await.err(), Some("busy"));
        drop(g);
        assert!(begin(&h).await.is_ok());
    }

    // ---- the real host over the runner, with fakes only

    #[derive(Default, Clone)]
    struct Fs {
        log: Arc<Mutex<Vec<String>>>,
        alive: Arc<AtomicBool>,
    }
    impl Spawner for Fs {
        fn spawn(&self, id: ServiceId, spec: &SpawnSpec) -> Result<(), RunError> {
            // The restarted serve is the sandboxed one.
            assert_eq!(spec.program, PathBuf::from("/usr/bin/sandbox-exec"));
            self.log.lock().unwrap().push(format!("spawn {id:?}"));
            self.alive.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn is_alive(&self, _id: ServiceId) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn stop(&self, id: ServiceId) -> bool {
            self.log.lock().unwrap().push(format!("stop {id:?}"));
            self.alive.swap(false, Ordering::SeqCst)
        }
    }
    struct Ports(Arc<AtomicBool>, bool);
    impl PortProbe for Ports {
        fn state<'a>(&'a self, _d: &'a ServiceDef) -> BoxFuture<'a, PortState> {
            let held = self.0.load(Ordering::SeqCst) || self.1;
            Box::pin(async move {
                if held {
                    PortState::Healthy
                } else {
                    PortState::Free
                }
            })
        }
    }
    struct Win(Arc<Mutex<Vec<String>>>, Arc<AtomicBool>, Arc<AtomicBool>);
    impl WindowProc for Win {
        fn spawn(&self, spec: &SpawnSpec) -> Result<(), &'static str> {
            assert_eq!(spec.program, PathBuf::from("/fake/MacOS/ollama"));
            assert_eq!(spec.args, ["serve"]);
            assert!(spec
                .env
                .iter()
                .any(|(k, v)| *k == "OLLAMA_HOST" && v == "127.0.0.1:11435"));
            // Never both: the serve must already be gone.
            assert!(!self.1.load(Ordering::SeqCst), "serve still up");
            self.0.lock().unwrap().push("window up".into());
            self.2.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn stop(&self) {
            self.0.lock().unwrap().push("window kill".into());
            self.2.store(false, Ordering::SeqCst);
        }
        fn alive(&self) -> bool {
            self.2.load(Ordering::SeqCst)
        }
        fn liveness(&self) -> Liveness {
            let f = self.2.clone();
            Arc::new(move || f.load(Ordering::SeqCst))
        }
        fn gone(&self) -> BoxFuture<'_, bool> {
            Box::pin(async { true })
        }
        fn ready(&self) -> BoxFuture<'_, bool> {
            Box::pin(async { true })
        }
        fn port_free(&self) -> BoxFuture<'_, bool> {
            Box::pin(async { true })
        }
    }

    fn runner(sp: Fs, foreign: bool) -> SidecarRunner<Fs, Ports> {
        SidecarRunner::with(
            sp.clone(),
            Ports(sp.alive.clone(), foreign),
            Registry::builtin(),
            RunPolicy::Full,
            Box::new(|n| Some(PathBuf::from(format!("/fake/MacOS/{n}")))),
            std::env::temp_dir(),
            Some("/home/x".into()),
        )
    }

    #[tokio::test]
    async fn the_runner_host_stops_our_serve_runs_the_window_and_restarts_the_sandboxed_serve() {
        let sp = Fs::default();
        sp.alive.store(true, Ordering::SeqCst);
        let r = runner(sp.clone(), false);
        let w = Win(sp.log.clone(), sp.alive.clone(), Default::default());
        let host = RunnerHost {
            runner: &r,
            window: &w,
        };
        let s = begin(&host).await.unwrap();
        // The sandboxed serve cannot be started by anyone while the window is open.
        assert_eq!(
            r.start(ServiceId::ModelRuntime).await.err(),
            Some(RunError::PullInProgress)
        );
        s.finish().await;
        assert_eq!(
            *sp.log.lock().unwrap(),
            [
                "stop ModelRuntime",
                "window up",
                "window kill",
                "spawn ModelRuntime"
            ]
        );
        assert!(r.slot().held_by().is_none());
        assert!(!r.slot().window_may_be_alive());
    }

    #[tokio::test]
    async fn a_start_or_pull_while_a_window_is_dying_is_refused_until_it_is_gone() {
        let sp = Fs::default();
        let r = runner(sp.clone(), false);
        let flag = Arc::new(AtomicBool::new(true));
        let f2 = flag.clone();
        // The holder is long gone (a cancel), but the window's group still has a member.
        r.slot()
            .set_window(Arc::new(move || f2.load(Ordering::SeqCst)));
        assert!(r.slot().held_by().is_none());
        assert_eq!(
            r.start(ServiceId::ModelRuntime).await.err(),
            Some(RunError::PullInProgress)
        );
        assert_eq!(
            r.slot().try_acquire(Holder::StartAll).err(),
            Some(Holder::Download)
        );
        assert_eq!(
            r.slot().try_acquire(Holder::Download).err(),
            Some(Holder::Download)
        );
        flag.store(false, Ordering::SeqCst);
        assert!(r.slot().try_acquire(Holder::StartAll).is_ok());
        assert!(r.start(ServiceId::ModelRuntime).await.is_ok());
    }

    #[tokio::test]
    async fn a_cancelled_or_slow_to_die_window_keeps_the_serve_and_new_pulls_out() {
        let mut h = FakeHost::new(ServeState::Ours);
        h.linger = true;
        let s = begin(&h).await.unwrap();
        s.finish().await; // gone() is false: the serve must NOT be restarted
        assert!(!h.log().contains(&"start_serve"));
        assert!(h.slot.window_may_be_alive());
        assert_eq!(begin(&h).await.err(), Some(PULL_IN_PROGRESS));
        h.win_flag.store(false, Ordering::SeqCst);
        assert!(begin(&h).await.is_ok());
        // The drop path (cancel) leaves the same guard in place.
        let mut h = FakeHost::new(ServeState::Absent);
        h.linger = true;
        drop(begin(&h).await.unwrap());
        assert!(h.slot.held_by().is_none());
        assert!(h.slot.window_may_be_alive());
    }

    #[tokio::test]
    async fn something_taking_11434_after_the_stop_is_runtime_busy_and_no_window_starts() {
        let mut h = FakeHost::new(ServeState::Ours);
        h.squat_after_stop = true;
        assert_eq!(begin(&h).await.err(), Some(RUNTIME_BUSY));
        assert!(!h.log().contains(&"spawn_window"));
        assert!(h.log().contains(&"start_serve"), "serve restored");
        assert!(h.slot.held_by().is_none());
    }

    #[tokio::test]
    async fn the_runner_host_refuses_a_runtime_it_did_not_start() {
        let sp = Fs::default(); // not alive: not ours
        let r = runner(sp.clone(), true); // but :11434 answers (Ollama.app)
        let w = Win(sp.log.clone(), sp.alive.clone(), Default::default());
        let host = RunnerHost {
            runner: &r,
            window: &w,
        };
        assert_eq!(begin(&host).await.err(), Some(RUNTIME_BUSY));
        assert!(sp.log.lock().unwrap().is_empty(), "nothing stopped or run");
    }

    #[test]
    fn the_window_spec_has_exactly_one_production_caller_chain() {
        let me = include_str!("pull_window.rs");
        let prod = me
            .split("#[cfg(test)]\npub(crate) mod fakes")
            .next()
            .unwrap();
        assert_eq!(prod.matches(".pull_window_spec()").count(), 1);
        assert!(!prod.contains("ollama_pull_window_spec("));
        for (name, src) in [
            ("services_start_all", include_str!("services_start_all.rs")),
            ("model_download", include_str!("model_download.rs")),
            ("lifecycle", include_str!("supervisor/lifecycle.rs")),
        ] {
            assert!(!src.contains("pull_window_spec"), "{name}");
        }
        // The window child is ended on drop and starts with a cleared environment (in the runner).
        let runner = include_str!("supervisor/runner.rs");
        let runner = runner.split("#[cfg(test)]").next().unwrap();
        let win = runner
            .split("impl WindowProc for ProcWindow")
            .nth(1)
            .unwrap();
        assert!(win.contains(".kill_on_drop(true)") && win.contains(".env_clear()"));
    }
}
