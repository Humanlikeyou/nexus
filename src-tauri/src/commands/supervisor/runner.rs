//! The verb allowlist, the `Runner` trait and the real runner. Every command the supervisor can ever
//! run is one `Verb`; names come from the registry by `ServiceId` (validated with
//! `validate_container_name`), labels are registry constants, `gui/<uid>` comes from `getuid`.
//! stderr is classified into a `RunError` and never kept; no command output leaves this module.
//!
//! Two gates sit in front of the real runner: `SUPERVISOR_ACT_ENABLED` (compile-time false in the
//! first ship, spelled in `act::effective`) decides whether the app builds an acting runner at all, and
//! `RunPolicy` decides which verbs a given runner may run (`ManualRestart` is what a user-approved
//! `service_restart` gets, `ReadOnly` is what an acting-off app gets). Everything shells out through the
//! `Exec` trait, so tests use a recording fake and never start a process.

use std::time::Duration;

use futures::future::BoxFuture;

use super::registry::{
    Registry, Runtime, ServiceDef, ServiceId, CONTAINER_FREQTRADE, LABEL_HERMES, LABEL_ROUTER,
};

/// Gates auto-restart, start-on-launch and stop-on-quit. Flipping it needs a decision entry.
pub const SUPERVISOR_ACT_ENABLED: bool = false;

pub const READ_TIMEOUT: Duration = Duration::from_secs(5);
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
pub const STOP_TIMEOUT: Duration = Duration::from_secs(30);
pub const KICKSTART_TIMEOUT: Duration = Duration::from_secs(10);
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const OUT_CAP: usize = 64 * 1024;

/// Every command the supervisor may ever run. Names come from the registry by `ServiceId`, never
/// from free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // the read-only verbs are used by tests and the next lane; voice is not built
pub enum Verb {
    DockerInspect(ServiceId),
    DockerInfo,
    DockerStart(ServiceId),
    DockerStop(ServiceId),
    DockerRestart(ServiceId),
    /// Restart one named container of a row (the Verba container of the Weaviate row). The name
    /// must be one of that row's registry containers, else the runner refuses it.
    DockerRestartOne(ServiceId, &'static str),
    LaunchctlKickstartK(ServiceId),
    OpenDockerHidden,
    /// Not implemented: voice stays observe-only. TODO(open question, plan section 6): does
    /// `./amara start|stop` touch launchd, and is voice unloaded on purpose (decision 0024)? Until
    /// Taisun answers, the real runner refuses this with `NotAllowed`.
    VoiceScript {
        start: bool,
    },
}

impl Verb {
    /// Read-only verbs are the only ones that may run while act is off.
    pub fn is_mutating(&self) -> bool {
        !matches!(self, Verb::DockerInspect(_) | Verb::DockerInfo)
    }

    fn is_restart(&self) -> bool {
        matches!(
            self,
            Verb::DockerRestart(_) | Verb::DockerRestartOne(..) | Verb::LaunchctlKickstartK(_)
        )
    }
}

/// Which verbs one runner instance may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPolicy {
    /// Inspect and info only.
    #[allow(dead_code)]
    // the tick loop uses `ActOffRunner`; kept for callers that need real reads
    ReadOnly,
    /// Read-only plus restart (docker restart, launchd kickstart): a user-approved manual restart.
    ManualRestart,
    /// Every allowlisted verb: only built when act is effectively on.
    Full,
}

impl RunPolicy {
    pub fn allows(self, verb: &Verb) -> bool {
        match self {
            RunPolicy::ReadOnly => !verb.is_mutating(),
            RunPolicy::ManualRestart => !verb.is_mutating() || verb.is_restart(),
            RunPolicy::Full => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    /// Act is off: the verb was not run.
    ActDisabled,
    /// Not on the allowlist for this service or this runner.
    NotAllowed,
    /// launchd does not have the job loaded: nothing was started.
    NotLoaded,
    NoSuchContainer,
    DockerDown,
    TimedOut,
    Failed,
}

pub trait Runner: Send + Sync {
    fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>>;
}

/// Runs nothing. Refuses every mutating verb with `ActDisabled`.
pub struct ActOffRunner;

impl Runner for ActOffRunner {
    fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
        Box::pin(async move {
            if verb.is_mutating() {
                Err(RunError::ActDisabled)
            } else {
                Err(RunError::NotAllowed)
            }
        })
    }
}

/// The approval dialog text and key for a manual launchd kickstart. Lives here so the command files
/// never spell the tool name; the label is a registry constant.
pub fn kickstart_prompt(id: ServiceId, uid: u32) -> Option<(String, String)> {
    let label = match id {
        ServiceId::AmaraRouter => LABEL_ROUTER,
        ServiceId::HermesGateway => LABEL_HERMES,
        _ => return None,
    };
    Some((
        format!("Run: launchctl kickstart -k gui/{uid}/{label}"),
        "Restarts this background service. Requests it is handling will be interrupted."
            .to_string(),
    ))
}

pub fn kickstart_key(id: ServiceId) -> Option<String> {
    let label = match id {
        ServiceId::AmaraRouter => LABEL_ROUTER,
        ServiceId::HermesGateway => LABEL_HERMES,
        _ => return None,
    };
    Some(crate::commands::approval_gate::service_key(&format!(
        "launchd:{label}"
    )))
}

pub fn current_uid() -> u32 {
    getuid()
}

// ---- process layer (behind a trait so tests never spawn) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prog {
    Docker,
    Launchctl,
    Open,
}

/// What stderr said, as a code. The text itself is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StderrClass {
    Clean,
    NoSuchObject,
    DaemonDown,
    Other,
}

pub fn classify_stderr(stderr: &str) -> StderrClass {
    if stderr.contains("No such object") || stderr.contains("No such container") {
        StderrClass::NoSuchObject
    } else if stderr.contains("Cannot connect to the Docker daemon")
        || stderr.contains("Is the docker daemon running")
    {
        StderrClass::DaemonDown
    } else if stderr.trim().is_empty() {
        StderrClass::Clean
    } else {
        StderrClass::Other
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOut {
    pub success: bool,
    pub class: StderrClass,
    /// Capped. Only the read-only inspect path reads it.
    pub stdout: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    /// The program is not there or could not start.
    Spawn,
    Timeout,
}

pub trait Exec: Send + Sync {
    fn exec<'a>(
        &'a self,
        prog: Prog,
        args: Vec<String>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<ExecOut, ExecError>>;
}

/// The only environment a child gets: a fixed PATH (plus the program's own folder) and HOME. Nothing
/// else from the app's environment (tokens, `DOCKER_HOST`, ...) is passed on.
pub(crate) fn clean_env(bin: &str, home: Option<&str>) -> Vec<(&'static str, String)> {
    let mut path = String::from("/usr/bin:/bin:/usr/sbin:/sbin");
    if let Some(dir) = std::path::Path::new(bin).parent().and_then(|d| d.to_str()) {
        if !dir.is_empty() && !path.split(':').any(|p| p == dir) {
            path.push(':');
            path.push_str(dir);
        }
    }
    let mut env = vec![("PATH", path)];
    if let Some(h) = home {
        env.push(("HOME", h.to_string()));
    }
    env
}

/// Run `bin args` with a cleared environment, stdin closed, `kill_on_drop`, a time limit, and both
/// pipes read with `take(OUT_CAP)` so a chatty child can never fill memory (the pipe is closed after
/// the cap, which ends the child with a broken pipe).
pub(crate) async fn run_capped(
    bin: &str,
    args: &[String],
    timeout: Duration,
) -> Result<(bool, String, String), ExecError> {
    use tokio::io::AsyncReadExt;
    let home = dirs::home_dir();
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .env_clear()
        .envs(clean_env(bin, home.as_deref().and_then(|h| h.to_str())))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|_| ExecError::Spawn)?;
    let (so, se) = (child.stdout.take(), child.stderr.take());
    let work = async {
        let (mut ob, mut eb) = (Vec::new(), Vec::new());
        let o = async {
            if let Some(s) = so {
                let _ = s.take(OUT_CAP as u64).read_to_end(&mut ob).await;
            }
        };
        let e = async {
            if let Some(s) = se {
                let _ = s.take(OUT_CAP as u64).read_to_end(&mut eb).await;
            }
        };
        futures::join!(o, e);
        let status = child.wait().await;
        (status, ob, eb)
    };
    let (status, ob, eb) = crate::commands::system::bounded(timeout, work)
        .await
        .map_err(|_| ExecError::Timeout)?;
    let status = status.map_err(|_| ExecError::Spawn)?;
    Ok((
        status.success(),
        String::from_utf8_lossy(&ob).into_owned(),
        String::from_utf8_lossy(&eb).into_owned(),
    ))
}

/// The real process layer: fixed absolute programs, no shell, stdin closed, bounded, `kill_on_drop`.
pub struct SysExec;

impl Exec for SysExec {
    fn exec<'a>(
        &'a self,
        prog: Prog,
        args: Vec<String>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<ExecOut, ExecError>> {
        Box::pin(async move {
            let bin = match prog {
                Prog::Docker => crate::commands::ha::docker_bin().ok_or(ExecError::Spawn)?,
                Prog::Launchctl => "/bin/launchctl",
                Prog::Open => "/usr/bin/open",
            };
            let (success, stdout, stderr) = run_capped(bin, &args, timeout).await?;
            Ok(ExecOut {
                success,
                class: classify_stderr(&stderr),
                stdout,
            })
        })
    }
}

fn getuid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

pub struct RealRunner<E: Exec> {
    pub(crate) exec: E,
    registry: Registry,
    policy: RunPolicy,
    uid: u32,
}

impl RealRunner<SysExec> {
    pub fn new(policy: RunPolicy) -> Self {
        Self::with(SysExec, Registry::builtin(), policy, getuid())
    }
}

impl<E: Exec> RealRunner<E> {
    pub fn with(exec: E, registry: Registry, policy: RunPolicy, uid: u32) -> Self {
        Self {
            exec,
            registry,
            policy,
            uid,
        }
    }

    fn def(&self, id: ServiceId) -> Option<&ServiceDef> {
        self.registry.get(id)
    }

    /// Registry container names, each re-validated. None when the row is not a docker row.
    fn containers(&self, id: ServiceId) -> Option<Vec<&'static str>> {
        let d = self.def(id)?;
        if d.runtime != Runtime::Docker || d.containers.is_empty() {
            return None;
        }
        if d.containers
            .iter()
            .any(|c| crate::commands::system::validate_container_name(c).is_err())
        {
            return None;
        }
        Some(d.containers.to_vec())
    }

    /// Only the two LaunchAgents the plan allows a kickstart for; the label is a registry constant.
    fn kick_label(&self, id: ServiceId) -> Option<&'static str> {
        match self.def(id)?.runtime {
            Runtime::Launchd(l) if l == LABEL_ROUTER || l == LABEL_HERMES => Some(l),
            _ => None,
        }
    }

    async fn run_one(
        &self,
        prog: Prog,
        args: Vec<String>,
        timeout: Duration,
    ) -> Result<(), RunError> {
        match self.exec.exec(prog, args, timeout).await {
            Err(ExecError::Timeout) => Err(RunError::TimedOut),
            Err(ExecError::Spawn) => Err(RunError::DockerDown),
            Ok(o) if o.success => Ok(()),
            Ok(o) => Err(match o.class {
                StderrClass::NoSuchObject => RunError::NoSuchContainer,
                StderrClass::DaemonDown => RunError::DockerDown,
                _ => RunError::Failed,
            }),
        }
    }

    async fn docker_each(
        &self,
        names: Vec<&'static str>,
        verb: &'static str,
        timeout: Duration,
    ) -> Result<(), RunError> {
        for n in names {
            self.run_one(Prog::Docker, vec![verb.to_string(), n.to_string()], timeout)
                .await?;
        }
        Ok(())
    }

    async fn run_inner(&self, verb: Verb) -> Result<(), RunError> {
        if !self.policy.allows(&verb) {
            return Err(if verb.is_mutating() {
                RunError::ActDisabled
            } else {
                RunError::NotAllowed
            });
        }
        match verb {
            Verb::DockerInfo => {
                let args = ["info", "--format", "{{.ServerVersion}}"].map(String::from);
                self.run_one(Prog::Docker, args.to_vec(), READ_TIMEOUT)
                    .await
            }
            Verb::DockerInspect(id) => {
                let names = self.containers(id).ok_or(RunError::NotAllowed)?;
                for n in names {
                    let args = ["inspect", "--format", "{{.State.Status}}", n].map(String::from);
                    self.run_one(Prog::Docker, args.to_vec(), READ_TIMEOUT)
                        .await?;
                }
                Ok(())
            }
            Verb::DockerStart(id) => {
                let names = self.containers(id).ok_or(RunError::NotAllowed)?;
                self.docker_each(names, "start", START_TIMEOUT).await
            }
            Verb::DockerStop(id) => {
                let mut names = self.containers(id).ok_or(RunError::NotAllowed)?;
                // Freqtrade is never stopped by the app, whoever asks.
                if names.contains(&CONTAINER_FREQTRADE) {
                    return Err(RunError::NotAllowed);
                }
                names.reverse(); // dependents first (Verba before Weaviate)
                self.docker_each(names, "stop", STOP_TIMEOUT).await
            }
            Verb::DockerRestart(id) => {
                let names = self.containers(id).ok_or(RunError::NotAllowed)?;
                self.docker_each(
                    names,
                    "restart",
                    crate::commands::system::DOCKER_RESTART_TIMEOUT,
                )
                .await
            }
            Verb::DockerRestartOne(id, name) => {
                let names = self.containers(id).ok_or(RunError::NotAllowed)?;
                let name = names
                    .into_iter()
                    .find(|n| *n == name)
                    .ok_or(RunError::NotAllowed)?;
                self.docker_each(
                    vec![name],
                    "restart",
                    crate::commands::system::DOCKER_RESTART_TIMEOUT,
                )
                .await
            }
            Verb::LaunchctlKickstartK(id) => {
                let label = self.kick_label(id).ok_or(RunError::NotAllowed)?;
                // Read-only check first: a job launchd does not have loaded is never started here.
                let listed = self
                    .exec
                    .exec(
                        Prog::Launchctl,
                        vec!["list".to_string(), label.to_string()],
                        READ_TIMEOUT,
                    )
                    .await;
                match listed {
                    Ok(o) if o.success => {}
                    Ok(_) => return Err(RunError::NotLoaded),
                    Err(ExecError::Timeout) => return Err(RunError::TimedOut),
                    Err(ExecError::Spawn) => return Err(RunError::Failed),
                }
                let target = format!("gui/{}/{}", self.uid, label);
                self.run_one(
                    Prog::Launchctl,
                    vec!["kickstart".to_string(), "-k".to_string(), target],
                    KICKSTART_TIMEOUT,
                )
                .await
            }
            Verb::OpenDockerHidden => {
                let args = ["-g", "-a", "Docker"].map(String::from);
                self.run_one(Prog::Open, args.to_vec(), OPEN_TIMEOUT).await
            }
            // TODO(open question): voice stays observe-only, see `Verb::VoiceScript`.
            Verb::VoiceScript { .. } => Err(RunError::NotAllowed),
        }
    }
}

impl<E: Exec> Runner for RealRunner<E> {
    fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
        Box::pin(self.run_inner(verb))
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::sync::Mutex;

    /// Records every call; answers from a script. No process is ever started.
    #[derive(Default)]
    pub struct FakeExec {
        pub calls: Mutex<Vec<(Prog, Vec<String>)>>,
        /// `launchctl list <label>` succeeds only for these labels.
        pub loaded: Mutex<Vec<String>>,
        /// Make every docker call fail with this class.
        pub docker_fail: Mutex<Option<StderrClass>>,
    }

    impl Exec for FakeExec {
        fn exec<'a>(
            &'a self,
            prog: Prog,
            args: Vec<String>,
            _timeout: Duration,
        ) -> BoxFuture<'a, Result<ExecOut, ExecError>> {
            self.calls.lock().unwrap().push((prog, args.clone()));
            let ok = |success| {
                Ok(ExecOut {
                    success,
                    class: StderrClass::Clean,
                    stdout: String::new(),
                })
            };
            let r = match prog {
                Prog::Launchctl if args.first().map(String::as_str) == Some("list") => {
                    ok(self.loaded.lock().unwrap().contains(&args[1]))
                }
                Prog::Docker => match *self.docker_fail.lock().unwrap() {
                    Some(class) => Ok(ExecOut {
                        success: false,
                        class,
                        stdout: String::new(),
                    }),
                    None => ok(true),
                },
                _ => ok(true),
            };
            Box::pin(async move { r })
        }
    }

    pub fn argv(e: &FakeExec) -> Vec<String> {
        e.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(p, a)| format!("{p:?} {}", a.join(" ")))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;

    fn real(policy: RunPolicy) -> RealRunner<FakeExec> {
        RealRunner::with(FakeExec::default(), Registry::builtin(), policy, 501)
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // the point: a flip must fail this test
    fn act_is_compiled_off() {
        assert!(!SUPERVISOR_ACT_ENABLED);
    }

    #[test]
    fn only_inspect_and_info_are_read_only() {
        assert!(!Verb::DockerInspect(ServiceId::N8n).is_mutating());
        assert!(!Verb::DockerInfo.is_mutating());
        for v in [
            Verb::DockerStart(ServiceId::N8n),
            Verb::DockerStop(ServiceId::N8n),
            Verb::DockerRestart(ServiceId::N8n),
            Verb::LaunchctlKickstartK(ServiceId::AmaraRouter),
            Verb::OpenDockerHidden,
            Verb::VoiceScript { start: true },
        ] {
            assert!(v.is_mutating(), "{v:?}");
        }
    }

    #[tokio::test]
    async fn the_act_off_runner_refuses_every_mutating_verb() {
        for v in [
            Verb::DockerRestart(ServiceId::N8n),
            Verb::LaunchctlKickstartK(ServiceId::HermesGateway),
            Verb::OpenDockerHidden,
        ] {
            assert_eq!(ActOffRunner.run(v).await, Err(RunError::ActDisabled));
        }
    }

    #[test]
    fn children_get_only_a_fixed_path_and_home() {
        let env = clean_env("/usr/local/bin/docker", Some("/Users/x"));
        assert_eq!(env.len(), 2);
        assert_eq!(env[0].0, "PATH");
        assert_eq!(env[0].1, "/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin");
        assert_eq!(env[1], ("HOME", "/Users/x".to_string()));
        // A program already in a system folder adds nothing; no HOME means no HOME.
        let env = clean_env("/usr/bin/open", None);
        assert_eq!(
            env,
            vec![("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".to_string())]
        );
    }

    #[tokio::test]
    async fn the_capped_runner_clears_the_environment_and_bounds_output() {
        // Harmless local programs only (no docker, no launchctl, no open).
        let (ok, out, _) = run_capped("/usr/bin/env", &[], Duration::from_secs(5))
            .await
            .unwrap();
        assert!(ok);
        // The app's own environment is large; the child sees exactly PATH and (if known) HOME.
        for line in out.lines() {
            assert!(
                line.starts_with("PATH=") || line.starts_with("HOME="),
                "{line}"
            );
        }
        assert!(out.contains("PATH=/usr/bin:/bin"));
        // Far more output than the cap comes back capped.
        let args = ["-c".to_string(), "yes x | head -c 400000".to_string()];
        let (_, out, _) = run_capped("/bin/sh", &args, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(out.len() <= OUT_CAP, "{}", out.len());
        // A hung child is ended at the limit.
        let args = ["-c".to_string(), "sleep 30".to_string()];
        assert_eq!(
            run_capped("/bin/sh", &args, Duration::from_millis(200))
                .await
                .unwrap_err(),
            ExecError::Timeout
        );
    }

    #[tokio::test]
    async fn docker_verbs_use_registry_names_and_fixed_argv() {
        let r = real(RunPolicy::Full);
        r.run(Verb::DockerStart(ServiceId::N8n)).await.unwrap();
        r.run(Verb::DockerRestart(ServiceId::CrewService))
            .await
            .unwrap();
        r.run(Verb::DockerStop(ServiceId::Weaviate)).await.unwrap();
        r.run(Verb::DockerInspect(ServiceId::Postgres))
            .await
            .unwrap();
        r.run(Verb::DockerInfo).await.unwrap();
        r.run(Verb::OpenDockerHidden).await.unwrap();
        assert_eq!(
            argv(&r.exec),
            vec![
                "Docker start stack-n8n-1",
                "Docker restart stack-crew-1",
                // dependents first
                "Docker stop stack-verba-1",
                "Docker stop stack-weaviate-1",
                "Docker inspect --format {{.State.Status}} stack-postgres-1",
                "Docker info --format {{.ServerVersion}}",
                "Open -g -a Docker",
            ]
        );
    }

    #[tokio::test]
    async fn restart_one_names_only_a_container_of_that_row() {
        let r = real(RunPolicy::ManualRestart);
        r.run(Verb::DockerRestartOne(ServiceId::Weaviate, "stack-verba-1"))
            .await
            .unwrap();
        assert_eq!(argv(&r.exec), vec!["Docker restart stack-verba-1"]);
        for v in [
            Verb::DockerRestartOne(ServiceId::Weaviate, "stack-n8n-1"),
            Verb::DockerRestartOne(ServiceId::N8n, "--help"),
            Verb::DockerRestartOne(ServiceId::AmaraRouter, "stack-n8n-1"),
        ] {
            assert_eq!(r.run(v).await, Err(RunError::NotAllowed), "{v:?}");
        }
        assert_eq!(r.exec.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn kickstart_runs_only_for_a_loaded_label_with_the_uid_domain() {
        let r = real(RunPolicy::Full);
        // Not loaded: the check runs, the kickstart never does.
        assert_eq!(
            r.run(Verb::LaunchctlKickstartK(ServiceId::AmaraRouter))
                .await,
            Err(RunError::NotLoaded)
        );
        assert_eq!(
            argv(&r.exec),
            vec![format!("Launchctl list {LABEL_ROUTER}")]
        );
        r.exec.loaded.lock().unwrap().push(LABEL_HERMES.to_string());
        r.run(Verb::LaunchctlKickstartK(ServiceId::HermesGateway))
            .await
            .unwrap();
        assert_eq!(
            argv(&r.exec)[1..],
            [
                format!("Launchctl list {LABEL_HERMES}"),
                format!("Launchctl kickstart -k gui/501/{LABEL_HERMES}")
            ]
        );
    }

    #[tokio::test]
    async fn rows_without_a_verb_are_refused_before_any_process() {
        let r = real(RunPolicy::Full);
        for v in [
            Verb::LaunchctlKickstartK(ServiceId::AmaraVoice),
            Verb::LaunchctlKickstartK(ServiceId::OpenWebui),
            Verb::LaunchctlKickstartK(ServiceId::N8n),
            Verb::DockerStart(ServiceId::AmaraRouter),
            Verb::DockerRestart(ServiceId::Docker),
            Verb::DockerStop(ServiceId::Freqtrade),
            Verb::VoiceScript { start: true },
            Verb::VoiceScript { start: false },
        ] {
            assert_eq!(r.run(v).await, Err(RunError::NotAllowed), "{v:?}");
        }
        assert!(r.exec.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn policies_gate_the_verbs() {
        let ro = real(RunPolicy::ReadOnly);
        assert_eq!(
            ro.run(Verb::DockerRestart(ServiceId::N8n)).await,
            Err(RunError::ActDisabled)
        );
        assert_eq!(
            ro.run(Verb::OpenDockerHidden).await,
            Err(RunError::ActDisabled)
        );
        ro.run(Verb::DockerInfo).await.unwrap();
        assert_eq!(
            argv(&ro.exec),
            vec!["Docker info --format {{.ServerVersion}}"]
        );

        let manual = real(RunPolicy::ManualRestart);
        for v in [
            Verb::DockerStart(ServiceId::N8n),
            Verb::DockerStop(ServiceId::N8n),
            Verb::OpenDockerHidden,
        ] {
            assert_eq!(manual.run(v).await, Err(RunError::ActDisabled), "{v:?}");
        }
        assert!(manual.exec.calls.lock().unwrap().is_empty());
        manual
            .run(Verb::DockerRestart(ServiceId::N8n))
            .await
            .unwrap();
        assert_eq!(argv(&manual.exec), vec!["Docker restart stack-n8n-1"]);
    }

    #[tokio::test]
    async fn stderr_is_classified_and_never_kept() {
        let r = real(RunPolicy::Full);
        for (class, want) in [
            (StderrClass::NoSuchObject, RunError::NoSuchContainer),
            (StderrClass::DaemonDown, RunError::DockerDown),
            (StderrClass::Other, RunError::Failed),
        ] {
            *r.exec.docker_fail.lock().unwrap() = Some(class);
            assert_eq!(r.run(Verb::DockerStart(ServiceId::N8n)).await, Err(want));
        }
        assert_eq!(
            classify_stderr("Error response from daemon: No such container: x"),
            StderrClass::NoSuchObject
        );
        assert_eq!(
            classify_stderr("Cannot connect to the Docker daemon at unix:///x"),
            StderrClass::DaemonDown
        );
        assert_eq!(classify_stderr("  \n"), StderrClass::Clean);
        assert_eq!(classify_stderr("secret words"), StderrClass::Other);
    }

    #[tokio::test]
    async fn a_multi_container_row_stops_at_the_first_failure() {
        let r = real(RunPolicy::Full);
        *r.exec.docker_fail.lock().unwrap() = Some(StderrClass::Other);
        assert_eq!(
            r.run(Verb::DockerStart(ServiceId::Weaviate)).await,
            Err(RunError::Failed)
        );
        assert_eq!(argv(&r.exec), vec!["Docker start stack-weaviate-1"]);
    }
}
