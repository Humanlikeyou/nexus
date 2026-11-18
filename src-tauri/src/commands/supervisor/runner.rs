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
    resolve_sidecar, sidecar_runtime, ProbeKind, Registry, Runtime, ServiceDef, ServiceId,
    CONTAINER_FREQTRADE, LABEL_HERMES, LABEL_ROUTER, SIDECAR_OLLAMA, SIDECAR_ROUTER,
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
    /// Start the bundled sidecar of a row (`ollama`, `amara-router`): allowlisted for exactly those
    /// two ids, handled only by `SidecarRunner` (the docker/launchctl `RealRunner` refuses it).
    SidecarStart(ServiceId),
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
    /// A model download window is open (or being opened): the sandboxed serve may not start.
    PullInProgress,
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
            // This runner only shells out to docker/launchctl/open; sidecars have their own runner.
            Verb::SidecarStart(_) => Err(RunError::NotAllowed),
        }
    }
}

impl<E: Exec> Runner for RealRunner<E> {
    fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
        Box::pin(self.run_inner(verb))
    }
}

// ---- sidecar layer: start a bundled binary (never docker/launchctl) ----

/// What owns a service's port right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortState {
    /// Answers its health check: adopt, start nothing.
    Healthy,
    /// Something holds the port (or the connect hung) and it is not healthy: the user's call.
    HeldUnhealthy,
    /// Connection refused: nobody is listening, so a spawn cannot collide.
    Free,
}

pub trait PortProbe: Send + Sync {
    fn state<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, PortState>;
}

/// The port a row's probe talks to (always 127.0.0.1).
pub(crate) fn probe_port(def: &ServiceDef) -> Option<u16> {
    match def.probe {
        ProbeKind::Tcp { port } => Some(port),
        ProbeKind::Http { base, .. } => reqwest::Url::parse(base).ok()?.port_or_known_default(),
        _ => None,
    }
}

/// Real port probe: a loopback connect decides free vs held, then the row's own read-only probe
/// decides healthy vs not. Only `ConnectionRefused` counts as free.
pub struct RealPortProbe(super::probe::RealProbe);

impl RealPortProbe {
    pub fn new() -> Self {
        Self(super::probe::RealProbe::new())
    }
}

impl Default for RealPortProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl PortProbe for RealPortProbe {
    fn state<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, PortState> {
        use super::probe::Probe;
        Box::pin(async move {
            let Some(port) = probe_port(def) else {
                return PortState::HeldUnhealthy;
            };
            let conn = tokio::time::timeout(
                super::probe::PROBE_TIMEOUT,
                tokio::net::TcpStream::connect(("127.0.0.1", port)),
            )
            .await;
            match conn {
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    return PortState::Free
                }
                Ok(Ok(_)) => {}
                _ => return PortState::HeldUnhealthy,
            }
            match self.0.check(def).await {
                super::machine::Observation::Up | super::machine::Observation::Slow => {
                    PortState::Healthy
                }
                _ => PortState::HeldUnhealthy,
            }
        })
    }
}

/// Exactly what a sidecar is started with: no shell, program + argv + the only environment it sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    /// Applied after `env_clear`.
    pub env: Vec<(&'static str, String)>,
}

/// The loopback address the long-running, sandboxed `ollama serve` binds.
pub(crate) const OLLAMA_HOST_LOOPBACK: &str = "127.0.0.1:11434";
/// The loopback address of the short-lived, UNSANDBOXED model-download window (decision 0031 section 4).
pub(crate) const OLLAMA_PULL_HOST_LOOPBACK: &str = "127.0.0.1:11435";
/// macOS sandbox launcher (deprecated by Apple but present on every supported macOS).
pub(crate) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// SBPL string literal for a path: refuses anything that could end the literal or smuggle a rule
/// (quote, backslash, control characters, empty, non-absolute).
fn sbpl_path(p: &std::path::Path) -> Option<String> {
    let s = p.to_str()?;
    if !s.starts_with('/') || s.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return None;
    }
    Some(format!("\"{s}\""))
}

/// Mach services the sandboxed runtime may look up (handoff 25a, M2). Found by live runs of the
/// bundled ollama 0.40.14 (2026-11-15): with every lookup denied, llama-server aborts with "failed to
/// create library" and ollama falls back to the CPU. `com.apple.bsd.dirhelper` (it only hands out the
/// per-user temp and cache folder paths) is the one service that restores Metal; this build uses its
/// embedded Metal library, so `MTLCompilerService` is not needed. No DNS, keychain, TCC, pasteboard,
/// Launch Services or Apple Events service is allowed.
pub(crate) const OLLAMA_SANDBOX_MACH_SERVICES: &[&str] = &["com.apple.bsd.dirhelper"];

/// Pure: the sandbox profile for the long-running `ollama serve` (decision 0031 section 4, L9;
/// handoff 25a M1/M2). Starts from `(allow default)` so Metal and the Go runtime keep working,
/// then closes what ollama must not do (SBPL: the last matching rule wins):
///  - network: outbound only to loopback TCP/UDP (no unix sockets, so no Docker socket, ssh-agent or
///    DNS); bind/inbound only on loopback. A model download can therefore not reach
///    `registry.ollama.ai` from this process; downloads run in the separate download window.
///    OPEN (handoff 25a M1): any loopback port stays reachable. The runner's random port cannot be
///    pinned, and live tests (2026-11-15) showed a port-specific deny does NOT override the
///    `localhost:*` allow in either order, so the download window must never run while this serve
///    can reach it, or must be authenticated.
///  - process-exec: only binaries inside the bundled runtime folder (`ollama` itself, which
///    `sandbox-exec` execs after applying the profile, and its `llama-server`). No shell, no
///    `open`, no `osascript`.
///  - lsopen and appleevent-send: denied (no opening apps or URLs, no scripting other apps).
///  - mach-lookup: denied except [`OLLAMA_SANDBOX_MACH_SERVICES`].
///  - file writes: only the model folder, the per-user temp/cache area, /private/tmp and /dev.
///    The bundled runtime folder is readable (default) but NOT writable.
///  - file reads: no keys, cloud credentials, Hermes data or the app's own data folder.
///
/// Validated with `sandbox-exec` against harmless commands in the tests, and live (2026-11-15) with
/// the bundled ollama 0.40.14 `serve` on a spare port: an embed ran with Metal (all layers on GPU).
pub(crate) fn ollama_sandbox_profile(
    bin: &std::path::Path,
    model_dir: Option<&std::path::Path>,
    data_dir: &std::path::Path,
    home: Option<&std::path::Path>,
) -> Option<String> {
    // Resolve symlinks when the path exists, so the checks below see the real folder.
    let parent = bin.parent()?;
    let exec_buf = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let exec_dir = exec_buf.as_path();
    // The exec allowlist is the runtime folder: never a system folder or a near-root path, which
    // would let the sandbox run any tool (shells, `open`, `osascript`).
    let exact = ["/Library", "/private", "/Applications", "/opt", "/Users"];
    if exec_dir.components().count() < 3
        || exact.iter().any(|d| exec_dir == std::path::Path::new(d))
        || ["/bin", "/sbin", "/usr", "/System"]
            .iter()
            .any(|d| exec_dir.starts_with(d))
    {
        return None;
    }
    // Never inside (or containing) a folder the profile lets it write: a planted binary could run.
    let writable = [
        "/private/var/folders",
        "/private/tmp",
        "/var/folders",
        "/tmp",
        "/dev",
    ];
    if writable
        .iter()
        .any(|w| exec_dir.starts_with(w) || std::path::Path::new(w).starts_with(exec_dir))
        || model_dir.is_some_and(|m| exec_dir.starts_with(m) || m.starts_with(exec_dir))
    {
        return None;
    }
    sandbox_profile_with(exec_dir, model_dir, data_dir, home)
}

/// The profile body: `exec_dir` is the only folder whose binaries may run. Tests call this
/// directly with system folders; production goes through [`ollama_sandbox_profile`].
fn sandbox_profile_with(
    exec_dir: &std::path::Path,
    model_dir: Option<&std::path::Path>,
    data_dir: &std::path::Path,
    home: Option<&std::path::Path>,
) -> Option<String> {
    let runtime_dir = sbpl_path(exec_dir)?;
    let mut out = String::from("(version 1)\n(allow default)\n");
    out.push_str("(deny network-outbound)\n");
    out.push_str("(allow network-outbound (remote ip \"localhost:*\"))\n");
    // No unix sockets at all (ollama and llama-server talk TCP): this closes the Docker socket
    // (root-equivalent), ssh-agent, other apps' sockets, and mDNSResponder (DNS-query exfiltration).
    out.push_str("(deny network-inbound)\n(deny network-bind)\n");
    out.push_str("(allow network-bind (local ip \"localhost:*\"))\n");
    out.push_str("(allow network-inbound (local ip \"localhost:*\"))\n");
    out.push_str("(deny process-exec*)\n");
    out.push_str(&format!("(allow process-exec* (subpath {runtime_dir}))\n"));
    out.push_str("(deny lsopen)\n(deny appleevent-send)\n");
    out.push_str("(deny mach-lookup)\n");
    for name in OLLAMA_SANDBOX_MACH_SERVICES {
        out.push_str(&format!("(allow mach-lookup (global-name \"{name}\"))\n"));
    }
    out.push_str("(deny file-write*)\n");
    if let Some(m) = model_dir {
        out.push_str(&format!(
            "(allow file-write* (subpath {}))\n",
            sbpl_path(m)?
        ));
    }
    for fixed in ["/private/var/folders", "/private/tmp", "/dev"] {
        out.push_str(&format!("(allow file-write* (subpath \"{fixed}\"))\n"));
    }
    // The runtime folder is never writable, even if a write allow above overlapped it.
    out.push_str(&format!("(deny file-write* (subpath {runtime_dir}))\n"));
    for rel in [".ssh", ".aws", ".gnupg", ".hermes", "Library/Keychains"] {
        if let Some(h) = home {
            out.push_str(&format!(
                "(deny file-read* (subpath {}))\n",
                sbpl_path(&h.join(rel))?
            ));
        }
    }
    out.push_str(&format!(
        "(deny file-read* (subpath {}))\n",
        sbpl_path(data_dir)?
    ));
    Some(out)
}

/// The model folder `ollama` uses by default (`~/.ollama/models` lives under it; the folder also
/// holds ollama's own key). Storage stays at the runtime's default.
pub(crate) fn ollama_model_dir(home: &str) -> std::path::PathBuf {
    std::path::Path::new(home).join(".ollama")
}

/// Pure: the spawn spec for one allowlisted row. None for any other id or a non-UTF-8 path.
/// `ollama serve` runs as `/usr/bin/sandbox-exec -p <profile> <ollama> serve`.
pub(crate) fn spawn_spec(
    id: ServiceId,
    bin: &std::path::Path,
    data_dir: &std::path::Path,
    home: Option<&str>,
) -> Option<SpawnSpec> {
    let bin_s = bin.to_str()?;
    let mut env = clean_env(bin_s, home);
    match id {
        ServiceId::ModelRuntime => {
            // No home (never in the real app): the profile has no model-folder write rule, so ollama
            // is contained but cannot store models; it is still never started unsandboxed.
            let model_dir = home.map(ollama_model_dir);
            let profile = ollama_sandbox_profile(
                bin,
                model_dir.as_deref(),
                data_dir,
                home.map(std::path::Path::new),
            )?;
            env.push(("OLLAMA_HOST", OLLAMA_HOST_LOOPBACK.to_string()));
            Some(SpawnSpec {
                program: std::path::PathBuf::from(SANDBOX_EXEC),
                args: vec![
                    "-p".to_string(),
                    profile,
                    bin_s.to_string(),
                    "serve".to_string(),
                ],
                env,
            })
        }
        ServiceId::AmaraRouter => {
            let cfg = data_dir.join("router").join("config.toml");
            Some(SpawnSpec {
                program: bin.to_path_buf(),
                args: vec!["--config".to_string(), cfg.to_str()?.to_string()],
                env,
            })
        }
        _ => None,
    }
}

/// Pure: the spawn spec of the short-lived model-download window: the same bundled `ollama serve`, UNSANDBOXED
/// (it must reach `registry.ollama.ai:443`), on its own loopback port, with the same model folder
/// (no `OLLAMA_MODELS`, so the default store is shared). Its one production caller is
/// `SidecarRunner::pull_window_spec`, used only by the download lifecycle (`pull_window.rs`), which runs
/// the window only while the sandboxed serve is stopped (handoff 25a M1).
pub(crate) fn ollama_pull_window_spec(
    bin: &std::path::Path,
    home: Option<&str>,
) -> Option<SpawnSpec> {
    let bin_s = bin.to_str()?;
    let mut env = clean_env(bin_s, home);
    env.push(("OLLAMA_HOST", OLLAMA_PULL_HOST_LOOPBACK.to_string()));
    Some(SpawnSpec {
        program: bin.to_path_buf(),
        args: vec!["serve".to_string()],
        env,
    })
}

/// The example config, embedded so the bundled router never reads the user's own router config folder.
const ROUTER_CONFIG_EXAMPLE: &str = include_str!("../../../../amara-router/config.example.toml");

/// The router config text the app writes: the embedded example with the launch-token requirement
/// switched on and the token file pointed at this data dir (the app mints it there). Nothing
/// outside this copy changes: the example file and the user's own router config stay as they are.
pub(crate) fn router_config_text(data_dir: &std::path::Path) -> Option<String> {
    const OFF: &str = "require_launch_token = false";
    const TOKEN_LINE_PREFIX: &str = "# launch_token_file = ";
    if ROUTER_CONFIG_EXAMPLE.matches(OFF).count() != 1 {
        return None;
    }
    let token = super::super::launch_token::token_path_in(data_dir);
    let quoted = serde_json::to_string(token.to_str()?).ok()?;
    let mut out = String::with_capacity(ROUTER_CONFIG_EXAMPLE.len() + 64);
    let mut replaced_file = false;
    for line in ROUTER_CONFIG_EXAMPLE.split_inclusive('\n') {
        if line.trim_end() == OFF {
            out.push_str("require_launch_token = true\n");
        } else if !replaced_file && line.starts_with(TOKEN_LINE_PREFIX) {
            out.push_str(&format!("launch_token_file = {quoted}\n"));
            replaced_file = true;
        } else {
            out.push_str(line);
        }
    }
    replaced_file.then_some(out)
}

fn fixed_err(msg: &'static str) -> std::io::Error {
    std::io::Error::other(msg)
}

/// Make `<data_dir>/router/config.toml` (dir 0700, file 0600, exclusive create, no symlink
/// followed) from `router_config_text` when it is missing. An existing dir or file must be a real
/// dir / regular file owned by `uid` with no group/other access (an owned dir is tightened to 0700);
/// anything else is a fixed error and nothing is written. Only the act-on start path calls this.
pub(crate) fn ensure_router_config_as(
    data_dir: &std::path::Path,
    uid: u32,
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    let dir = data_dir.join("router");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let dm = std::fs::symlink_metadata(&dir)?;
    if !dm.file_type().is_dir() || dm.uid() != uid {
        return Err(fixed_err("router folder is not ours"));
    }
    if dm.mode() & 0o077 != 0 {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("config.toml");
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_file() => {
            if m.uid() != uid || m.mode() & 0o077 != 0 {
                return Err(fixed_err("router config is not private"));
            }
            return Ok(path);
        }
        Ok(_) => return Err(fixed_err("router config is not a regular file")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let text = router_config_text(data_dir).ok_or_else(|| fixed_err("router config template"))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    Ok(path)
}

pub(crate) fn ensure_router_config(
    data_dir: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    // SAFETY: getuid has no preconditions and cannot fail.
    ensure_router_config_as(data_dir, unsafe { libc::getuid() })
}

pub trait Spawner: Send + Sync {
    fn spawn(&self, id: ServiceId, spec: &SpawnSpec) -> Result<(), RunError>;
    /// Whether the app started `id` and it is still alive. Adopted processes are never ours.
    fn is_alive(&self, _id: ServiceId) -> bool {
        false
    }
    /// Ends the child the app started for `id` (nothing adopted is ever recorded). True if one was
    /// signalled.
    fn stop(&self, _id: ServiceId) -> bool {
        false
    }
}

/// The real spawner: a detached, long-running child (own process group, stdio null, not killed on
/// drop). Handles are kept so `stop_started` can end exactly what the app started, on quit.
#[derive(Default)]
pub struct ProcSpawner {
    children: std::sync::Mutex<Vec<(ServiceId, tokio::process::Child)>>,
}

impl ProcSpawner {
    fn guard(&self) -> std::sync::MutexGuard<'_, Vec<(ServiceId, tokio::process::Child)>> {
        self.children.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether the app started `id` and it is still alive.
    pub fn started_by_app(&self, id: ServiceId) -> bool {
        self.guard()
            .iter_mut()
            .any(|(i, c)| *i == id && matches!(c.try_wait(), Ok(None)))
    }

    /// Whether any child the app started is still alive (decides if a quit must wait to stop).
    pub fn any_started(&self) -> bool {
        self.guard()
            .iter_mut()
            .any(|(_, c)| matches!(c.try_wait(), Ok(None)))
    }

    /// End every child the app started, newest first (router before runtime). Anything that
    /// was adopted was never recorded here, so it is never touched.
    pub fn stop_started(&self) -> Vec<ServiceId> {
        let mut stopped = Vec::new();
        let mut g = self.guard();
        while let Some((id, mut c)) = g.pop() {
            if let Ok(None) = c.try_wait() {
                if signal_child_group(&c) {
                    stopped.push(id);
                }
            }
        }
        stopped
    }
}

/// Ends the child's whole process group (SIGTERM, then a forced end after a delay). True if the
/// child was still running. Only ever called with a child the app spawned with `process_group(0)`.
fn signal_child_group(c: &tokio::process::Child) -> bool {
    match c.id() {
        Some(pid) => {
            crate::commands::proc_group::terminate(pid);
            true
        }
        None => false,
    }
}

impl Spawner for ProcSpawner {
    fn is_alive(&self, id: ServiceId) -> bool {
        self.started_by_app(id)
    }

    fn stop(&self, id: ServiceId) -> bool {
        let mut g = self.guard();
        let mut stopped = false;
        g.retain_mut(|(i, c)| {
            if *i != id {
                return true;
            }
            if let Ok(None) = c.try_wait() {
                stopped |= signal_child_group(c);
            }
            false
        });
        stopped
    }

    fn spawn(&self, id: ServiceId, spec: &SpawnSpec) -> Result<(), RunError> {
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(false)
            .process_group(0);
        // Held across the spawn so two callers can never both start the same id.
        let mut g = self.guard();
        if g.iter_mut()
            .any(|(i, c)| *i == id && matches!(c.try_wait(), Ok(None)))
        {
            return Err(RunError::NotAllowed);
        }
        let child = cmd.spawn().map_err(|_| RunError::Failed)?;
        g.retain(|(i, _)| *i != id); // drop a dead handle of the same id
        g.push((id, child));
        Ok(())
    }
}

/// The process edge of the download window (and its loopback port), so tests use a fake.
pub trait WindowProc: Send + Sync {
    fn spawn(&self, spec: &SpawnSpec) -> Result<(), &'static str>;
    /// Synchronous and idempotent: signals the whole process group (used by the drop guard too).
    fn stop(&self);
    /// True once the child has exited AND its process group is empty (bounded wait).
    fn gone(&self) -> BoxFuture<'_, bool>;
    /// True once OUR child (by pid) listens on the window port and is still alive after a short
    /// settle (bounded wait). A listener that belongs to anything else never counts.
    fn ready(&self) -> BoxFuture<'_, bool>;
    fn port_free(&self) -> BoxFuture<'_, bool>;
    /// Whether the child is running right now.
    fn alive(&self) -> bool;
    /// A live view of "some process of the window's group may still exist".
    fn liveness(&self) -> std::sync::Arc<dyn Fn() -> bool + Send + Sync>;
}

const WINDOW_READY_TIMEOUT: Duration = Duration::from_secs(30);
const WINDOW_GONE_TIMEOUT: Duration = Duration::from_secs(10);
/// After the port first answers, the child must still be alive this much later (a child that lost
/// the bind race exits quickly).
const WINDOW_SETTLE: Duration = Duration::from_millis(500);
const LSOF: &str = "/usr/sbin/lsof";

/// The real window: its own child (own process group, stdio null, cleared environment), ended when
/// dropped. Only one at a time. The group id is the child's pid.
pub struct ProcWindow {
    child: std::sync::Mutex<Option<tokio::process::Child>>,
    /// Group id of the last spawn; 0 when none. Kept after the leader is reaped until the group is empty.
    pgid: std::sync::Arc<std::sync::atomic::AtomicU32>,
    addr: &'static str,
    ready_timeout: Duration,
}

impl Default for ProcWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcWindow {
    pub fn new() -> Self {
        Self::with(OLLAMA_PULL_HOST_LOOPBACK, WINDOW_READY_TIMEOUT)
    }
    pub(crate) fn with(addr: &'static str, ready_timeout: Duration) -> Self {
        Self {
            child: std::sync::Mutex::new(None),
            pgid: Default::default(),
            addr,
            ready_timeout,
        }
    }
    fn child(&self) -> std::sync::MutexGuard<'_, Option<tokio::process::Child>> {
        self.child.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn group(&self) -> u32 {
        self.pgid.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn port(&self) -> &str {
        self.addr.rsplit(':').next().unwrap_or_default()
    }
    async fn answers(&self) -> bool {
        matches!(
            tokio::time::timeout(
                Duration::from_millis(500),
                tokio::net::TcpStream::connect(self.addr),
            )
            .await,
            Ok(Ok(_))
        )
    }
    /// Whether the child (this pid) is the one listening on the port. Fails closed.
    async fn child_listens(&self) -> bool {
        let Some(pid) = self.child().as_ref().and_then(|c| c.id()) else {
            return false;
        };
        let mut cmd = tokio::process::Command::new(LSOF);
        cmd.args([
            "-nP",
            &format!("-iTCP:{}", self.port()),
            "-sTCP:LISTEN",
            "-a",
            "-p",
            &pid.to_string(),
            "-t",
        ])
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
        let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(3), cmd.output()).await else {
            return false;
        };
        out.status.success()
            && String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l.trim() == pid.to_string())
    }
}

impl WindowProc for ProcWindow {
    fn spawn(&self, spec: &SpawnSpec) -> Result<(), &'static str> {
        if self.alive() || !crate::commands::proc_group::group_empty(self.group()) {
            return Err("runtime_busy");
        }
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let child = cmd.spawn().map_err(|_| "runtime_unavailable")?;
        self.pgid
            .store(child.id().unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
        *self.child() = Some(child);
        Ok(())
    }
    fn stop(&self) {
        crate::commands::proc_group::terminate(self.group());
    }
    fn gone(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + WINDOW_GONE_TIMEOUT;
            loop {
                // try_wait reaps the leader; the group must also be empty.
                let leader_up = self.alive();
                if !leader_up && crate::commands::proc_group::group_empty(self.group()) {
                    *self.child() = None;
                    self.pgid.store(0, std::sync::atomic::Ordering::SeqCst);
                    return true;
                }
                if tokio::time::Instant::now() >= deadline {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    }
    fn ready(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + self.ready_timeout;
            while self.alive() && tokio::time::Instant::now() < deadline {
                if self.child_listens().await {
                    // Settle, then confirm again: a child that lost the bind race has exited by now.
                    tokio::time::sleep(WINDOW_SETTLE).await;
                    return self.alive() && self.child_listens().await;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            false
        })
    }
    fn port_free(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { !self.answers().await })
    }
    fn alive(&self) -> bool {
        self.child()
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }
    fn liveness(&self) -> std::sync::Arc<dyn Fn() -> bool + Send + Sync> {
        let g = self.pgid.clone();
        std::sync::Arc::new(move || {
            !crate::commands::proc_group::group_empty(g.load(std::sync::atomic::Ordering::SeqCst))
        })
    }
}

/// How one `SidecarStart` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarOutcome {
    /// Already answering: nothing was started.
    Adopted,
    Started,
    /// The port is held by something unhealthy: nothing was started.
    NeedsYou,
    /// No usable bundled binary next to the app.
    BinaryMissing,
}

/// Finds a bundled binary by name.
pub type Resolver = Box<dyn Fn(&str) -> Option<std::path::PathBuf> + Send + Sync>;

pub struct SidecarRunner<S: Spawner, P: PortProbe> {
    pub(crate) spawner: S,
    ports: P,
    registry: Registry,
    policy: RunPolicy,
    resolve: Resolver,
    data_dir: std::path::PathBuf,
    home: Option<String>,
    /// Shared with the download lifecycle: the sandboxed serve is never started while a download holds it.
    slot: std::sync::Arc<crate::commands::pull_window::PullSlot>,
}

impl SidecarRunner<ProcSpawner, RealPortProbe> {
    /// The runner `services_start_all` uses when act is effective.
    pub fn real(data_dir: std::path::PathBuf) -> Self {
        let mut r = Self::with(
            ProcSpawner::default(),
            RealPortProbe::new(),
            Registry::builtin(),
            RunPolicy::Full,
            Box::new(resolve_sidecar),
            data_dir,
            dirs::home_dir().and_then(|h| h.to_str().map(str::to_string)),
        );
        r.slot = crate::commands::pull_window::global_slot();
        r
    }
}

impl<S: Spawner, P: PortProbe> SidecarRunner<S, P> {
    pub fn with(
        spawner: S,
        ports: P,
        registry: Registry,
        policy: RunPolicy,
        resolve: Resolver,
        data_dir: std::path::PathBuf,
        home: Option<String>,
    ) -> Self {
        Self {
            spawner,
            ports,
            registry,
            policy,
            resolve,
            data_dir,
            home,
            slot: Default::default(),
        }
    }

    pub(crate) fn slot(&self) -> &std::sync::Arc<crate::commands::pull_window::PullSlot> {
        &self.slot
    }

    /// The spec of the unsandboxed download window, from the bundled binary. Called only by the
    /// download lifecycle.
    pub(crate) fn pull_window_spec(&self) -> Option<SpawnSpec> {
        let bin = (self.resolve)(SIDECAR_OLLAMA)?;
        ollama_pull_window_spec(&bin, self.home.as_deref())
    }

    /// Port state of an allowlisted sidecar row, read-only.
    pub async fn port_state(&self, id: ServiceId) -> Option<PortState> {
        sidecar_runtime(id)?;
        Some(self.ports.state(self.registry.get(id)?).await)
    }

    /// Probe first (adopt / needs_you), only then resolve the binary and spawn.
    pub async fn start(&self, id: ServiceId) -> Result<SidecarOutcome, RunError> {
        if !self.policy.allows(&Verb::SidecarStart(id)) {
            return Err(RunError::ActDisabled);
        }
        // The sandboxed serve never starts while a download window holds the slot (25a M1).
        if id == ServiceId::ModelRuntime
            && (self.slot.held_by() == Some(crate::commands::pull_window::Holder::Download)
                || self.slot.window_may_be_alive())
        {
            return Err(RunError::PullInProgress);
        }
        self.start_checked(id).await
    }

    /// The restart that ends a download: the download itself holds the slot, so the slot check is skipped;
    /// policy and the allowlist still apply.
    pub(crate) async fn start_after_pull(&self, id: ServiceId) -> Result<SidecarOutcome, RunError> {
        if !self.policy.allows(&Verb::SidecarStart(id)) {
            return Err(RunError::ActDisabled);
        }
        self.start_checked(id).await
    }

    async fn start_checked(&self, id: ServiceId) -> Result<SidecarOutcome, RunError> {
        let Some(Runtime::Sidecar { name }) = sidecar_runtime(id) else {
            return Err(RunError::NotAllowed);
        };
        if name != SIDECAR_OLLAMA && name != SIDECAR_ROUTER {
            return Err(RunError::NotAllowed);
        }
        let def = self.registry.get(id).ok_or(RunError::NotAllowed)?;
        match self.ports.state(def).await {
            PortState::Healthy => return Ok(SidecarOutcome::Adopted),
            PortState::HeldUnhealthy => return Ok(SidecarOutcome::NeedsYou),
            PortState::Free => {}
        }
        let Some(bin) = (self.resolve)(name) else {
            return Ok(SidecarOutcome::BinaryMissing);
        };
        if id == ServiceId::AmaraRouter {
            ensure_router_config(&self.data_dir).map_err(|_| RunError::Failed)?;
        }
        let spec = spawn_spec(id, &bin, &self.data_dir, self.home.as_deref())
            .ok_or(RunError::NotAllowed)?;
        self.spawner.spawn(id, &spec)?;
        Ok(SidecarOutcome::Started)
    }
}

impl<S: Spawner, P: PortProbe> Runner for SidecarRunner<S, P> {
    fn run<'a>(&'a self, verb: Verb) -> BoxFuture<'a, Result<(), RunError>> {
        Box::pin(async move {
            match verb {
                Verb::SidecarStart(id) => match self.start(id).await? {
                    SidecarOutcome::Adopted | SidecarOutcome::Started => Ok(()),
                    SidecarOutcome::NeedsYou | SidecarOutcome::BinaryMissing => {
                        Err(RunError::Failed)
                    }
                },
                _ => Err(RunError::NotAllowed),
            }
        })
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

#[cfg(test)]
mod sidecar_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct FakeSpawner(Arc<Mutex<Vec<(ServiceId, SpawnSpec)>>>);
    impl Spawner for FakeSpawner {
        fn spawn(&self, id: ServiceId, spec: &SpawnSpec) -> Result<(), RunError> {
            self.0.lock().unwrap().push((id, spec.clone()));
            Ok(())
        }
    }

    struct FakePorts(Mutex<std::collections::HashMap<ServiceId, PortState>>);
    impl FakePorts {
        fn all(s: PortState) -> Self {
            Self(Mutex::new(ServiceId::ALL.iter().map(|i| (*i, s)).collect()))
        }
    }
    impl PortProbe for FakePorts {
        fn state<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, PortState> {
            let s = self.0.lock().unwrap()[&def.id];
            Box::pin(async move { s })
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sup-sc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn runner(
        policy: RunPolicy,
        port: PortState,
        bin: bool,
        data: &std::path::Path,
    ) -> (SidecarRunner<FakeSpawner, FakePorts>, FakeSpawner) {
        let sp = FakeSpawner::default();
        let r = SidecarRunner::with(
            sp.clone(),
            FakePorts::all(port),
            Registry::builtin(),
            policy,
            Box::new(move |n| bin.then(|| PathBuf::from(format!("/fake/MacOS/{n}")))),
            data.to_path_buf(),
            Some("/home/x".to_string()),
        );
        (r, sp)
    }

    #[test]
    fn sidecar_start_is_mutating_and_refused_by_the_non_full_policies() {
        let v = Verb::SidecarStart(ServiceId::ModelRuntime);
        assert!(v.is_mutating());
        assert!(!RunPolicy::ReadOnly.allows(&v));
        assert!(!RunPolicy::ManualRestart.allows(&v));
        assert!(RunPolicy::Full.allows(&v));
    }

    #[tokio::test]
    async fn the_act_off_runner_and_manual_policy_refuse_it_and_spawn_nothing() {
        for id in [ServiceId::ModelRuntime, ServiceId::AmaraRouter] {
            assert_eq!(
                ActOffRunner.run(Verb::SidecarStart(id)).await,
                Err(RunError::ActDisabled)
            );
        }
        let d = tmp("pol");
        for policy in [RunPolicy::ReadOnly, RunPolicy::ManualRestart] {
            let (r, sp) = runner(policy, PortState::Free, true, &d);
            assert_eq!(
                r.run(Verb::SidecarStart(ServiceId::ModelRuntime)).await,
                Err(RunError::ActDisabled)
            );
            assert!(sp.0.lock().unwrap().is_empty());
        }
        // The docker/launchctl runner never handles it, even at Full.
        let real = RealRunner::with(
            fakes::FakeExec::default(),
            Registry::builtin(),
            RunPolicy::Full,
            501,
        );
        assert_eq!(
            real.run(Verb::SidecarStart(ServiceId::AmaraRouter)).await,
            Err(RunError::NotAllowed)
        );
        assert!(real.exec.calls.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn only_the_runtime_and_router_ids_are_allowlisted() {
        let d = tmp("ids");
        let (r, sp) = runner(RunPolicy::Full, PortState::Free, true, &d);
        for id in ServiceId::ALL {
            if matches!(id, ServiceId::ModelRuntime | ServiceId::AmaraRouter) {
                continue;
            }
            assert_eq!(
                r.run(Verb::SidecarStart(id)).await,
                Err(RunError::NotAllowed),
                "{id:?}"
            );
        }
        assert!(sp.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_healthy_port_is_adopted_and_nothing_is_spawned() {
        let d = tmp("adopt");
        let (r, sp) = runner(RunPolicy::Full, PortState::Healthy, true, &d);
        for id in [ServiceId::ModelRuntime, ServiceId::AmaraRouter] {
            assert_eq!(r.start(id).await, Ok(SidecarOutcome::Adopted));
        }
        assert!(sp.0.lock().unwrap().is_empty());
        // Adopting never creates the router config either.
        assert!(!d.join("router").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_held_but_unhealthy_port_is_needs_you_and_nothing_is_spawned() {
        let d = tmp("held");
        let (r, sp) = runner(RunPolicy::Full, PortState::HeldUnhealthy, true, &d);
        for id in [ServiceId::ModelRuntime, ServiceId::AmaraRouter] {
            assert_eq!(r.start(id).await, Ok(SidecarOutcome::NeedsYou));
        }
        assert!(sp.0.lock().unwrap().is_empty());
        assert!(!d.join("router").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_free_port_spawns_with_exact_args_and_a_cleared_env() {
        let d = tmp("spawn");
        let (r, sp) = runner(RunPolicy::Full, PortState::Free, true, &d);
        assert_eq!(
            r.start(ServiceId::ModelRuntime).await,
            Ok(SidecarOutcome::Started)
        );
        assert_eq!(
            r.start(ServiceId::AmaraRouter).await,
            Ok(SidecarOutcome::Started)
        );
        let calls = sp.0.lock().unwrap();
        assert_eq!(calls.len(), 2);
        let path = "/usr/bin:/bin:/usr/sbin:/sbin:/fake/MacOS".to_string();
        assert_eq!(
            calls[0],
            (
                ServiceId::ModelRuntime,
                SpawnSpec {
                    program: PathBuf::from("/usr/bin/sandbox-exec"),
                    args: vec![
                        "-p".to_string(),
                        ollama_sandbox_profile(
                            std::path::Path::new("/fake/MacOS/ollama"),
                            Some(std::path::Path::new("/home/x/.ollama")),
                            &d,
                            Some(std::path::Path::new("/home/x"))
                        )
                        .unwrap(),
                        "/fake/MacOS/ollama".to_string(),
                        "serve".to_string()
                    ],
                    env: vec![
                        ("PATH", path.clone()),
                        ("HOME", "/home/x".to_string()),
                        ("OLLAMA_HOST", "127.0.0.1:11434".to_string()),
                    ],
                }
            )
        );
        let cfg = d.join("router").join("config.toml");
        assert_eq!(
            calls[1],
            (
                ServiceId::AmaraRouter,
                SpawnSpec {
                    program: PathBuf::from("/fake/MacOS/amara-router"),
                    args: vec!["--config".to_string(), cfg.to_str().unwrap().to_string()],
                    env: vec![("PATH", path), ("HOME", "/home/x".to_string())],
                }
            )
        );
        // No model-storage variable, no wide bind.
        for (_, spec) in calls.iter() {
            assert!(spec.env.iter().all(|(k, _)| *k != "OLLAMA_MODELS"));
            assert!(spec.env.iter().all(|(_, v)| !v.contains("0.0.0.0")));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    fn test_profile() -> String {
        ollama_sandbox_profile(
            std::path::Path::new("/home/x/rt/ollama"),
            Some(std::path::Path::new("/home/x/.ollama")),
            std::path::Path::new("/home/x/Library/Application Support/com.taisun.nexus"),
            Some(std::path::Path::new("/home/x")),
        )
        .unwrap()
    }

    #[test]
    fn the_sandbox_profile_denies_outbound_and_allows_only_loopback() {
        let p = test_profile();
        assert!(p.starts_with("(version 1)\n(allow default)\n"));
        assert!(p.contains("(deny network-outbound)\n"));
        assert!(p.contains("(allow network-outbound (remote ip \"localhost:*\"))"));
        // Deny comes before the loopback allow (the last matching rule wins).
        assert!(
            p.find("(deny network-outbound)").unwrap()
                < p.find("(allow network-outbound (remote ip").unwrap()
        );
        assert!(p.contains("(deny network-bind)") && p.contains("(deny network-inbound)"));
        assert!(p.contains("(allow network-bind (local ip \"localhost:*\"))"));
        // No wide allow of any network rule.
        for line in p.lines().filter(|l| l.starts_with("(allow network")) {
            assert!(line.contains("localhost:*"), "{line}");
        }
        // M2: exec only from the runtime folder, after the deny.
        assert!(
            p.find("(deny process-exec*)\n").unwrap()
                < p.find("(allow process-exec* (subpath \"/home/x/rt\"))\n")
                    .unwrap()
        );
        assert_eq!(p.matches("(allow process-exec*").count(), 1);
        // Writes: denied, then only the model folder, per-user temp, /private/tmp, /dev.
        assert!(p.contains("(deny file-write*)\n"));
        let writes: Vec<&str> = p
            .lines()
            .filter(|l| l.starts_with("(allow file-write*"))
            .collect();
        assert_eq!(
            writes,
            vec![
                "(allow file-write* (subpath \"/home/x/.ollama\"))",
                "(allow file-write* (subpath \"/private/var/folders\"))",
                "(allow file-write* (subpath \"/private/tmp\"))",
                "(allow file-write* (subpath \"/dev\"))",
            ]
        );
        // Reads: keys, cloud credentials, Hermes, Keychains and the app data folder are closed.
        for d in [
            "/home/x/.ssh",
            "/home/x/.aws",
            "/home/x/.gnupg",
            "/home/x/.hermes",
            "/home/x/Library/Keychains",
            "/home/x/Library/Application Support/com.taisun.nexus",
        ] {
            assert!(
                p.contains(&format!("(deny file-read* (subpath \"{d}\"))")),
                "{d}"
            );
        }
        assert!(!p.contains("0.0.0.0"));
    }

    #[test]
    fn sandbox_profile_paths_cannot_inject_rules() {
        let home = std::path::Path::new("/home/x");
        let data = std::path::Path::new("/home/x/d");
        for bad in ["/h\")(allow network*)(\"", "/h\\x", "rel/path", "", "/h\nx"] {
            assert_eq!(
                ollama_sandbox_profile(
                    std::path::Path::new("/opt/rt/ollama"),
                    Some(std::path::Path::new(bad)),
                    data,
                    Some(home)
                ),
                None,
                "{bad:?}"
            );
            assert_eq!(
                ollama_sandbox_profile(
                    std::path::Path::new("/opt/rt/ollama"),
                    Some(std::path::Path::new("/ok")),
                    std::path::Path::new(bad),
                    Some(home)
                ),
                None,
                "{bad:?}"
            );
        }
    }

    /// The profile for tests: binaries in `exec_dir` may run.
    fn profile_for(exec_dir: &str) -> String {
        let home = std::env::temp_dir();
        sandbox_profile_with(
            std::path::Path::new(exec_dir),
            Some(&home.join(".ollama")),
            &home.join("d"),
            Some(&home),
        )
        .unwrap()
    }

    fn sandboxed(profile: &str, program: &str, args: &[&str]) -> std::process::Command {
        let mut c = std::process::Command::new(SANDBOX_EXEC);
        c.arg("-p").arg(profile).arg(program).args(args);
        c
    }

    fn run_sandboxed(exec_dir: &str, program: &str, args: &[&str]) -> std::process::Output {
        sandboxed(&profile_for(exec_dir), program, args)
            .output()
            .unwrap()
    }

    #[test]
    fn the_sandbox_profile_is_valid_sbpl() {
        // sandbox-exec runs /usr/bin/true (exec folder /usr/bin here), never ollama.
        let out = run_sandboxed("/usr/bin", "/usr/bin/true", &[]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn the_exec_folder_is_never_a_system_or_near_root_folder() {
        // Handoff 25a review L4: `bin.parent()` of `/ollama` would be `/` (exec of everything).
        let d = std::path::Path::new("/data");
        for bin in [
            "/ollama",
            "/x/ollama",
            "/usr/bin/ollama",
            "/bin/ollama",
            "/usr/local/bin/ollama",
            "/System/x/ollama",
            "/Applications/ollama",
            "/opt/ollama",
            "/private/tmp/rt/ollama",
            "/tmp/rt/ollama",
            "/private/var/folders/ab/cd/rt/ollama",
        ] {
            assert_eq!(
                ollama_sandbox_profile(std::path::Path::new(bin), None, d, None),
                None,
                "{bin}"
            );
        }
        assert!(ollama_sandbox_profile(
            std::path::Path::new(
                "/Applications/Amara OS.app/Contents/Resources/ollama-runtime/ollama"
            ),
            None,
            d,
            None
        )
        .is_some());
    }

    #[test]
    fn the_sandbox_denies_exec_outside_the_runtime_folder() {
        // Handoff 25a M2: with the exec folder /usr/bin, nothing in /bin (a shell) may run; with
        // another exec folder, neither may `open` or `osascript`.
        let sh = run_sandboxed("/usr/bin", "/bin/sh", &["-c", "exit 0"]);
        assert!(!sh.status.success(), "a shell ran inside the sandbox");
        for tool in ["/usr/bin/open", "/usr/bin/osascript"] {
            let out = run_sandboxed("/opt/nexus-rt", tool, &["-h"]);
            assert!(!out.status.success(), "{tool} ran inside the sandbox");
        }
        // A child exec from an allowed binary is also confined: /usr/bin/env may not start /bin/sh.
        let child = run_sandboxed("/usr/bin", "/usr/bin/env", &["/bin/sh", "-c", "exit 0"]);
        assert!(
            !child.status.success(),
            "a child shell ran inside the sandbox"
        );
        // Positive control: an allowed binary may start another allowed one.
        assert!(
            run_sandboxed("/usr/bin", "/usr/bin/env", &["/usr/bin/true"])
                .status
                .success()
        );
    }

    #[test]
    fn the_sandbox_cannot_open_unix_sockets() {
        // Handoff 25a review H1: no unix-socket connects (Docker socket, ssh-agent, mDNSResponder).
        let dir = std::env::temp_dir().join(format!("nexus-sbx-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("s");
        let _ = std::fs::remove_file(&sock);
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        // Accept and drop each connection so a successful nc exits at once (never hangs).
        let acceptor = listener.try_clone().unwrap();
        std::thread::spawn(move || {
            for c in acceptor.incoming() {
                drop(c);
            }
        });
        let s = sock.to_str().unwrap();
        // Positive control: the socket accepts outside the sandbox.
        let outside = std::process::Command::new("/usr/bin/nc")
            .args(["-U", s])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            outside.status.success(),
            "control: unix socket not reachable"
        );
        let inside = sandboxed(&profile_for("/usr/bin"), "/usr/bin/nc", &["-U", s])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            !inside.status.success(),
            "the sandbox connected to a unix socket"
        );
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!test_profile().contains("unix-socket"));
    }

    #[test]
    fn the_sandbox_denies_mach_lookups_outside_the_allowlist() {
        // Handoff 25a M2: the profile denies mach-lookup and allows only the listed services.
        let p = test_profile();
        let deny = p.find("(deny mach-lookup)\n").unwrap();
        let allows: Vec<&str> = p
            .lines()
            .filter(|l| l.starts_with("(allow mach-lookup"))
            .collect();
        assert_eq!(allows.len(), OLLAMA_SANDBOX_MACH_SERVICES.len());
        for name in OLLAMA_SANDBOX_MACH_SERVICES {
            let line = format!("(allow mach-lookup (global-name \"{name}\"))");
            assert!(p.find(&line).unwrap() > deny, "{name}");
        }
        for forbidden in [
            "mDNSResponder",
            "SecurityServer",
            "securityd",
            "tccd",
            "pasteboard",
            "launchservicesd",
            "lsd.",
            "appleevents",
            "windowserver",
            "trustd",
            "nsurlsessiond",
        ] {
            assert!(
                OLLAMA_SANDBOX_MACH_SERVICES
                    .iter()
                    .all(|n| !n.contains(forbidden)),
                "{forbidden}"
            );
        }
        assert!(p.contains("(deny lsopen)\n") && p.contains("(deny appleevent-send)\n"));
    }

    #[test]
    fn spawn_specs_for_ollama_are_exact() {
        let bin = std::path::Path::new("/App/Contents/Resources/ollama-runtime/ollama");
        let data = std::path::Path::new("/data");
        let path =
            "/usr/bin:/bin:/usr/sbin:/sbin:/App/Contents/Resources/ollama-runtime".to_string();
        // Long-running: sandbox-exec wraps the binary, env is cleared plus OLLAMA_HOST only.
        let serve = spawn_spec(ServiceId::ModelRuntime, bin, data, Some("/home/x")).unwrap();
        assert_eq!(serve.program, PathBuf::from("/usr/bin/sandbox-exec"));
        assert_eq!(serve.args.len(), 4);
        assert_eq!(serve.args[0], "-p");
        assert_eq!(
            serve.args[1],
            ollama_sandbox_profile(
                bin,
                Some(std::path::Path::new("/home/x/.ollama")),
                data,
                Some(std::path::Path::new("/home/x"))
            )
            .unwrap()
        );
        assert_eq!(serve.args[2], bin.to_str().unwrap());
        assert_eq!(serve.args[3], "serve");
        assert_eq!(
            serve.env,
            vec![
                ("PATH", path.clone()),
                ("HOME", "/home/x".to_string()),
                ("OLLAMA_HOST", "127.0.0.1:11434".to_string()),
            ]
        );
        // Without a home it is still sandboxed, with no model-folder write rule and no HOME.
        let nohome = spawn_spec(ServiceId::ModelRuntime, bin, data, None).unwrap();
        assert_eq!(nohome.program, PathBuf::from("/usr/bin/sandbox-exec"));
        assert!(!nohome.args[1].contains(".ollama"));
        assert!(nohome.env.iter().all(|(k, _)| *k != "HOME"));
        // Pull window: unsandboxed, own port, same model folder (no OLLAMA_MODELS).
        let pull = ollama_pull_window_spec(bin, Some("/home/x")).unwrap();
        assert_eq!(
            pull,
            SpawnSpec {
                program: bin.to_path_buf(),
                args: vec!["serve".to_string()],
                env: vec![
                    ("PATH", path),
                    ("HOME", "/home/x".to_string()),
                    ("OLLAMA_HOST", "127.0.0.1:11435".to_string()),
                ],
            }
        );
        assert_ne!(OLLAMA_HOST_LOOPBACK, OLLAMA_PULL_HOST_LOOPBACK);
        for spec in [&serve, &pull] {
            assert!(spec.env.iter().all(|(k, _)| *k != "OLLAMA_MODELS"));
            assert!(spec.env.iter().all(|(_, v)| !v.contains("0.0.0.0")));
        }
    }

    #[test]
    fn the_runtime_is_only_ever_spawned_through_the_sandbox_in_the_runner() {
        // The long-running serve is built only through `spawn_spec` (sandboxed). The unsandboxed
        // pull window spec has exactly one production caller: `SidecarRunner::pull_window_spec`,
        // which only the pull lifecycle (`pull_window.rs`, mutually exclusive with the serve) calls.
        let src = include_str!("runner.rs");
        let prod = src
            .split("#[cfg(test)]\npub(crate) mod fakes")
            .next()
            .unwrap();
        assert_eq!(
            prod.matches("ollama_pull_window_spec(").count(),
            2,
            "its definition and the one caller"
        );
        assert_eq!(prod.matches("fn pull_window_spec(").count(), 1);
        let caller = prod.split("fn pull_window_spec(").nth(1).unwrap();
        assert!(caller
            .split("\n    }\n")
            .next()
            .unwrap()
            .contains("ollama_pull_window_spec("));
        // `start` (the only serve starter besides the post-pull restart) refuses during a pull.
        assert!(prod.contains("RunError::PullInProgress"));
        assert!(prod.contains("SANDBOX_EXEC"));
        // Tests never start `serve`.
        let tests = src
            .split("#[cfg(test)]\npub(crate) mod fakes")
            .nth(1)
            .unwrap();
        assert!(!tests.contains("Command::new(\"ollama\")"));
        assert!(!tests.contains("\"pull\""));
    }

    #[tokio::test]
    async fn a_missing_binary_spawns_nothing() {
        let d = tmp("nobin");
        let (r, sp) = runner(RunPolicy::Full, PortState::Free, false, &d);
        assert_eq!(
            r.start(ServiceId::ModelRuntime).await,
            Ok(SidecarOutcome::BinaryMissing)
        );
        assert!(sp.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_router_config_is_copied_once_with_private_modes() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("cfg");
        let p = ensure_router_config(&d).unwrap();
        assert_eq!(p, d.join("router").join("config.toml"));
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            router_config_text(&d).unwrap()
        );
        let mode = |q: &std::path::Path| std::fs::metadata(q).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(&d.join("router")), 0o700);
        // An existing file (the user's edits) is never overwritten.
        std::fs::write(&p, "edited").unwrap();
        ensure_router_config(&d).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "edited");
        // A symlink at that path is refused.
        std::fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink("/dev/null", &p).unwrap();
        assert!(ensure_router_config(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sidecar_source_pins() {
        let src = include_str!("runner.rs");
        let prod = src
            .split("#[cfg(test)]\npub(crate) mod fakes")
            .next()
            .unwrap();
        // No shell plugin or shell spawn, no compiled home path, no launchd load/bootstrap.
        for bad in [
            "tauri_plugin_shell",
            "plugin:shell",
            "\"/bin/sh\"",
            "/Users/",
            "launchctl load",
            "\"bootstrap\"",
            "\"load\"",
            ".config/amara-router",
            "libc::kill",
        ] {
            assert!(!prod.contains(bad), "{bad}");
        }
        let side = prod.split("// ---- sidecar layer").nth(1).unwrap();
        assert!(side.contains(".env_clear()"));
        assert!(!side.contains("Prog::Launchctl"));
        assert!(!side.contains("Prog::Docker"));
        let reg = include_str!("registry.rs");
        let reg_prod = reg.split("#[cfg(test)]").next().unwrap();
        assert!(!reg_prod.contains("/Users/"));
    }

    #[test]
    fn the_copied_router_config_requires_the_launch_token() {
        let d = tmp("tok");
        let text = router_config_text(&d).unwrap();
        // Only the [security] switch and the token file change; the rest is the example verbatim.
        assert!(text.contains("require_launch_token = true\n"));
        assert!(!text.contains("require_launch_token = false"));
        let tok = crate::commands::launch_token::token_path_in(&d);
        assert!(text.contains(&format!(
            "launch_token_file = {}",
            serde_json::to_string(tok.to_str().unwrap()).unwrap()
        )));
        let changed: Vec<_> = text
            .lines()
            .zip(ROUTER_CONFIG_EXAMPLE.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert_eq!(text.lines().count(), ROUTER_CONFIG_EXAMPLE.lines().count());
        // The shipped example keeps its default.
        assert!(ROUTER_CONFIG_EXAMPLE.contains("require_launch_token = false"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn existing_router_dir_and_file_must_be_ours_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let uid = unsafe { libc::getuid() };
        let mode = |q: &std::path::Path| std::fs::metadata(q).unwrap().permissions().mode() & 0o777;
        // A loose dir that is ours is tightened to 0700.
        let d = tmp("own1");
        std::fs::create_dir(d.join("router")).unwrap();
        std::fs::set_permissions(d.join("router"), std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_router_config_as(&d, uid).unwrap();
        assert_eq!(mode(&d.join("router")), 0o700);
        // A dir owned by someone else: refused, nothing written.
        let d2 = tmp("own2");
        assert!(ensure_router_config_as(&d2, uid + 1).is_err());
        assert!(!d2.join("router").join("config.toml").exists());
        // A group/world-readable existing file is refused, not rewritten.
        let cfg = d.join("router").join("config.toml");
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ensure_router_config_as(&d, uid).is_err());
        assert_eq!(mode(&cfg), 0o644);
        // A file owned by someone else is refused.
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(ensure_router_config_as(&d, uid + 1).is_err());
        assert!(ensure_router_config_as(&d, uid).is_ok());
        // A symlinked router dir is refused.
        let d3 = tmp("own3");
        std::os::unix::fs::symlink(&d, d3.join("router")).unwrap();
        assert!(ensure_router_config_as(&d3, uid).is_err());
        for x in [d, d2, d3] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    #[tokio::test]
    async fn the_real_spawner_refuses_a_second_live_spawn_of_the_same_id() {
        // A harmless local program stands in for the sidecar.
        let sp = ProcSpawner::default();
        let spec = SpawnSpec {
            program: PathBuf::from("/bin/sleep"),
            args: vec!["30".to_string()],
            env: vec![("PATH", "/usr/bin:/bin".to_string())],
        };
        assert!(!sp.any_started());
        sp.spawn(ServiceId::ModelRuntime, &spec).unwrap();
        assert!(sp.started_by_app(ServiceId::ModelRuntime));
        assert_eq!(
            sp.spawn(ServiceId::ModelRuntime, &spec),
            Err(RunError::NotAllowed)
        );
        assert_eq!(sp.guard().len(), 1);
        assert_eq!(sp.stop_started(), vec![ServiceId::ModelRuntime]);
    }

    fn sleep_spec() -> SpawnSpec {
        SpawnSpec {
            program: PathBuf::from("/bin/sleep"),
            args: vec!["30".to_string()],
            env: vec![("PATH", "/usr/bin:/bin".to_string())],
        }
    }

    #[tokio::test]
    async fn a_listener_that_is_not_our_child_never_makes_the_window_ready() {
        // A squatter (this test) holds the port; the child is a harmless sleep that never listens.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr: &'static str =
            Box::leak(format!("127.0.0.1:{}", l.local_addr().unwrap().port()).into_boxed_str());
        let w = ProcWindow::with(addr, Duration::from_millis(1500));
        assert!(!w.port_free().await, "squatter is seen");
        w.spawn(&sleep_spec()).unwrap();
        assert!(w.alive());
        assert!(!w.ready().await, "someone else's listener is never ours");
        w.stop();
        assert!(w.gone().await);
        assert!(!w.alive());
        drop(l);
    }

    #[tokio::test]
    async fn the_window_stop_ends_the_whole_process_group() {
        let w = ProcWindow::with("127.0.0.1:1", Duration::from_millis(100));
        let spec = SpawnSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".to_string(),
                "/bin/sleep 30 & /bin/sleep 30; wait".to_string(),
            ],
            env: vec![("PATH", "/usr/bin:/bin".to_string())],
        };
        w.spawn(&spec).unwrap();
        let live = w.liveness();
        assert!(live());
        // A second window is refused while one runs.
        assert_eq!(w.spawn(&spec), Err("runtime_busy"));
        w.stop();
        assert!(w.gone().await, "leader and its children are all gone");
        assert!(!live());
    }

    #[tokio::test]
    async fn the_real_spawner_stops_the_serve_group_not_just_the_pid() {
        let sp = ProcSpawner::default();
        let spec = SpawnSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".to_string(),
                "/bin/sleep 30 & /bin/sleep 30; wait".to_string(),
            ],
            env: vec![("PATH", "/usr/bin:/bin".to_string())],
        };
        sp.spawn(ServiceId::ModelRuntime, &spec).unwrap();
        let pgid = sp.guard()[0].1.id().unwrap();
        assert!(sp.stop(ServiceId::ModelRuntime));
        assert!(!sp.is_alive(ServiceId::ModelRuntime));
        let start = std::time::Instant::now();
        while !crate::commands::proc_group::group_empty(pgid)
            && start.elapsed() < Duration::from_secs(5)
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(crate::commands::proc_group::group_empty(pgid));
    }
}
