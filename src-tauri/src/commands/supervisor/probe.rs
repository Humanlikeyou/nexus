//! Probes behind the `Probe` trait. Everything here is read-only: an HTTP GET to a validated
//! loopback base, a bare TCP connect to 127.0.0.1, an in-process read, and `docker inspect` /
//! `docker info` with a fixed `--format` (so container environment variables are never read).
//! Each row is bounded to 2 s. Reasons are fixed codes; no response body, error text, URL or path is
//! kept.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use serde_json::Value;

use super::machine::{Observation, ReasonCode};
use super::registry::{ProbeKind, ServiceDef};
use super::runner::{classify_stderr, run_capped, StderrClass};
use crate::commands::whoop_status::{WhoopOwner, WhoopStatus};

pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// An answer slower than this counts as "slow".
pub const SLOW_MS: u64 = 1_000;
/// Longest one read-only docker call may take (the tick bounds it tighter).
const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a batched container answer is trusted.
const CACHE_TTL: Duration = Duration::from_secs(3);
/// How long the saved router URL is reused before `config.json` is read again.
const ROUTER_URL_TTL: Duration = Duration::from_secs(60);
/// `config.json` holds bot and gateway secrets; only a bounded read of it is ever made.
const CONFIG_READ_CAP: u64 = 256 * 1024;
/// health-sync: a heart-rate sample newer than this is "flowing".
const SYNC_FRESH_SECS: i64 = 6 * 3600;

pub trait Probe: Send + Sync {
    /// Called once per tick before the per-row checks, with every row. The real probe uses it to read
    /// all containers in ONE `docker inspect`; the default does nothing.
    fn prepare<'a>(&'a self, _defs: &'a [ServiceDef]) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn check<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, Observation>;
}

// ---- pure classification (unit-tested) ----

/// 2xx = up (slow if late). 401/403 = something answered behind an auth wall, which is alive.
pub(crate) fn http_observation(status: u16, elapsed_ms: u64) -> Observation {
    match status {
        200..=299 | 401 | 403 => {
            if elapsed_ms >= SLOW_MS {
                Observation::Slow
            } else {
                Observation::Up
            }
        }
        _ => Observation::Down(ReasonCode::BadStatus),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerFacts {
    pub status: String,
    pub exit_code: i64,
    pub health: Option<String>,
    /// HostIp of every published port (empty string = all interfaces).
    pub host_ips: Vec<String>,
}

/// Parse the fixed-format inspect output. None if it is not the JSON we asked for.
pub(crate) fn parse_inspect(text: &str) -> Option<ContainerFacts> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    facts_from_value(&v)
}

/// One line of the batched inspect: the same JSON plus `"name"` (docker prints it as `/name`).
pub(crate) fn parse_inspect_named(line: &str) -> Option<(String, ContainerFacts)> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let name = v.get("name")?.as_str()?.trim_start_matches('/').to_string();
    Some((name, facts_from_value(&v)?))
}

fn facts_from_value(v: &Value) -> Option<ContainerFacts> {
    let status = v.get("status")?.as_str()?.to_string();
    let exit_code = v.get("exit").and_then(Value::as_i64).unwrap_or(0);
    let health = v.get("health").and_then(Value::as_str).map(str::to_string);
    let mut host_ips = Vec::new();
    if let Some(ports) = v.get("ports").and_then(Value::as_object) {
        for binds in ports.values() {
            for b in binds.as_array().into_iter().flatten() {
                host_ips.push(
                    b.get("HostIp")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
    }
    Some(ContainerFacts {
        status,
        exit_code,
        health,
        host_ips,
    })
}

/// `0.0.0.0`, `::` and an empty HostIp all mean "every interface".
pub(crate) fn is_wide_bind(host_ip: &str) -> bool {
    matches!(host_ip.trim(), "" | "0.0.0.0" | "::" | "[::]")
}

pub(crate) fn container_observation(f: &ContainerFacts) -> Observation {
    if f.status != "running" {
        return match f.status.as_str() {
            "created" => Observation::Stopped(ReasonCode::ContainerStopped),
            "exited" if f.exit_code == 0 => Observation::Stopped(ReasonCode::ContainerStopped),
            "restarting" => Observation::Starting(ReasonCode::ContainerStarting),
            _ => Observation::Down(ReasonCode::ContainerDown),
        };
    }
    if f.host_ips.iter().any(|ip| is_wide_bind(ip)) {
        return Observation::NeedsYou(ReasonCode::BadBind);
    }
    match f.health.as_deref() {
        Some("starting") => Observation::Starting(ReasonCode::ContainerStarting),
        Some("unhealthy") => Observation::Down(ReasonCode::Unhealthy),
        _ => Observation::Up,
    }
}

/// stderr is only classified, never kept.
pub(crate) fn inspect_failure(stderr: &str) -> Observation {
    if stderr.contains("No such object") {
        Observation::NeedsYou(ReasonCode::MissingContainer)
    } else {
        Observation::NeedsYou(ReasonCode::DockerUnavailable)
    }
}

/// Every distinct container name of `defs`, in registry order: what one tick inspects in one call.
pub(crate) fn batch_names(defs: &[ServiceDef]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for d in defs {
        for n in d.containers {
            if !names.contains(n) {
                names.push(n);
            }
        }
    }
    names
}

/// One observation per requested name from a single multi-name `docker inspect`. Names that came back
/// are classified from their facts; a name that did not is missing when docker said "No such object",
/// and docker-unavailable otherwise. stderr is passed in as a class, never as text.
pub(crate) fn batch_observations(
    names: &[&str],
    stdout: &str,
    class: StderrClass,
) -> HashMap<String, Observation> {
    let found: HashMap<String, ContainerFacts> =
        stdout.lines().filter_map(parse_inspect_named).collect();
    names
        .iter()
        .map(|n| {
            let o = match found.get(*n) {
                Some(f) => container_observation(f),
                None if class == StderrClass::NoSuchObject => {
                    Observation::NeedsYou(ReasonCode::MissingContainer)
                }
                None => Observation::NeedsYou(ReasonCode::DockerUnavailable),
            };
            (n.to_string(), o)
        })
        .collect()
}

/// Health rows from `whoop_status` (counts and times only).
pub(crate) fn builtin_observation(kind: ProbeKind, s: &WhoopStatus, now_secs: i64) -> Observation {
    match kind {
        ProbeKind::BuiltInReader => {
            if s.store_found {
                Observation::Up
            } else {
                Observation::Stopped(ReasonCode::NoStore)
            }
        }
        ProbeKind::BuiltInSync => match s.last_sample_at {
            Some(t) if now_secs - t <= SYNC_FRESH_SECS => Observation::Up,
            Some(_) if s.owner == WhoopOwner::Legacy => Observation::Up,
            Some(_) => Observation::Slow,
            None => Observation::Stopped(ReasonCode::NoStore),
        },
        _ => Observation::Down(ReasonCode::NotRunning),
    }
}

// ---- the saved router URL (one field of config.json, nothing else kept) ----

#[derive(serde::Deserialize, Default)]
struct RouterUrlOnly {
    #[serde(default)]
    amara_router_url: Option<String>,
}

/// Just `amara_router_url` from a config.json body; every other field is ignored and dropped.
pub(crate) fn router_url_from(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<RouterUrlOnly>(bytes)
        .ok()?
        .amara_router_url
}

/// Capped, symlink-refusing read of the config file; the buffer is zeroed on drop.
fn read_router_url(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = zeroize::Zeroizing::new(Vec::new());
    f.take(CONFIG_READ_CAP).read_to_end(&mut buf).ok()?;
    router_url_from(&buf)
}

// ---- real probes ----

async fn docker_read(args: &[&str]) -> Result<(bool, String, String), ()> {
    let bin = crate::commands::ha::docker_bin().ok_or(())?;
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    // Cleared environment and capped pipes, like every other child the supervisor starts.
    run_capped(bin, &args, DOCKER_TIMEOUT).await.map_err(|_| ())
}

const INSPECT_FORMAT: &str = "{\"status\":{{json .State.Status}},\"exit\":{{json .State.ExitCode}},\"health\":{{if .State.Health}}{{json .State.Health.Status}}{{else}}null{{end}},\"ports\":{{json .NetworkSettings.Ports}}}";

/// The batched variant adds the container name so each output line can be matched to a registry name.
const INSPECT_FORMAT_NAMED: &str = "{\"name\":{{json .Name}},\"status\":{{json .State.Status}},\"exit\":{{json .State.ExitCode}},\"health\":{{if .State.Health}}{{json .State.Health.Status}}{{else}}null{{end}},\"ports\":{{json .NetworkSettings.Ports}}}";

async fn inspect_container(name: &str) -> Observation {
    if crate::commands::system::validate_container_name(name).is_err() {
        return Observation::NeedsYou(ReasonCode::MissingContainer);
    }
    match docker_read(&["inspect", "--format", INSPECT_FORMAT, name]).await {
        Err(()) => Observation::NeedsYou(ReasonCode::DockerUnavailable),
        Ok((true, out, _)) => match parse_inspect(&out) {
            Some(f) => container_observation(&f),
            None => Observation::NeedsYou(ReasonCode::DockerUnavailable),
        },
        Ok((false, _, err)) => inspect_failure(&err),
    }
}

async fn docker_info() -> Observation {
    match docker_read(&["info", "--format", "{{.ServerVersion}}"]).await {
        Ok((true, _, _)) => Observation::Up,
        _ => Observation::Down(ReasonCode::DockerUnavailable),
    }
}

pub(crate) async fn tcp_observation(port: u16) -> Observation {
    let started = Instant::now();
    match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(_) => {
            if started.elapsed() >= Duration::from_millis(SLOW_MS) {
                Observation::Slow
            } else {
                Observation::Up
            }
        }
        Err(_) => Observation::Down(ReasonCode::Unreachable),
    }
}

pub(crate) async fn http_probe(client: &reqwest::Client, url: &str) -> Observation {
    let started = Instant::now();
    match client.get(url).send().await {
        Ok(r) => http_observation(r.status().as_u16(), started.elapsed().as_millis() as u64),
        Err(e) if e.is_timeout() => Observation::Down(ReasonCode::Timeout),
        Err(_) => Observation::Down(ReasonCode::Unreachable),
    }
}

pub struct RealProbe {
    client: reqwest::Client,
    /// Recent container observations, filled by `prepare` (one `docker inspect` for all rows) and
    /// trusted for `CACHE_TTL` only, so a stale answer never outlives a tick.
    containers: Mutex<HashMap<String, (Instant, Observation)>>,
    /// The saved router URL and when it was read.
    router_url: Mutex<Option<(Instant, Option<String>)>>,
}

impl RealProbe {
    pub fn new() -> Self {
        let client = crate::commands::shared::http::builder(
            crate::commands::shared::http::HttpSpec::loopback().timeout(PROBE_TIMEOUT),
        )
        .build()
        .unwrap_or_default();
        Self {
            client,
            containers: Mutex::new(HashMap::new()),
            router_url: Mutex::new(None),
        }
    }

    /// Router base: the saved URL (loopback-only by `router_base`), re-read at most once a minute.
    async fn router_base(&self) -> String {
        let fresh = self
            .router_url
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|(at, _)| at.elapsed() < ROUTER_URL_TTL)
            .map(|(_, u)| u.clone());
        let saved = match fresh {
            Some(u) => u,
            None => {
                let path = crate::commands::telegram::config_path();
                let u = tokio::task::spawn_blocking(move || read_router_url(&path))
                    .await
                    .ok()
                    .flatten();
                *self.router_url.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some((Instant::now(), u.clone()));
                u
            }
        };
        crate::commands::llm_router::router_base(None, saved.as_deref())
    }

    /// One `docker inspect` for every container of `defs`, bounded; the fixed format keeps container
    /// environment variables unread.
    async fn read_all_containers(&self, defs: &[ServiceDef]) {
        let names = batch_names(defs);
        if names.is_empty() {
            return;
        }
        let (valid, invalid): (Vec<&str>, Vec<&str>) = names
            .iter()
            .partition(|n| crate::commands::system::validate_container_name(n).is_ok());
        let mut map: HashMap<String, Observation> = invalid
            .into_iter()
            .map(|n| {
                (
                    n.to_string(),
                    Observation::NeedsYou(ReasonCode::MissingContainer),
                )
            })
            .collect();
        let mut args = vec!["inspect", "--format", INSPECT_FORMAT_NAMED];
        args.extend(valid.iter().copied());
        let read = tokio::time::timeout(PROBE_TIMEOUT, docker_read(&args)).await;
        match read {
            Err(_) => {
                for n in &valid {
                    map.insert(n.to_string(), Observation::Down(ReasonCode::Timeout));
                }
            }
            Ok(Err(())) => {
                for n in &valid {
                    map.insert(
                        n.to_string(),
                        Observation::NeedsYou(ReasonCode::DockerUnavailable),
                    );
                }
            }
            Ok(Ok((_ok, out, err))) => {
                map.extend(batch_observations(&valid, &out, classify_stderr(&err)))
            }
        }
        let now = Instant::now();
        self.containers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(map.into_iter().map(|(k, v)| (k, (now, v))));
    }

    /// The cached answer for `name`, or a single inspect when `prepare` has not run (direct callers).
    async fn container(&self, name: &str) -> Observation {
        let cached = self
            .containers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(name)
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, o)| *o);
        match cached {
            Some(o) => o,
            None => inspect_container(name).await,
        }
    }

    async fn check_inner(&self, def: &ServiceDef) -> Observation {
        let mut worst = Observation::Up;
        for name in def.containers {
            worst = worst.worst(self.container(name).await);
        }
        // Not running, missing, bad bind, or still starting: the network probe adds nothing.
        if worst.rank() >= Observation::Starting(ReasonCode::ContainerStarting).rank() {
            return worst;
        }
        let net = match def.probe {
            ProbeKind::Http { base, path } => {
                let base = if def.id == super::registry::ServiceId::AmaraRouter {
                    // Honour the saved router URL (loopback-only by router_base), like the system map.
                    self.router_base().await
                } else {
                    base.to_string()
                };
                if def.id == super::registry::ServiceId::N8n {
                    if crate::commands::n8n_status::n8n_healthz(&self.client, PROBE_TIMEOUT).await {
                        Observation::Up
                    } else {
                        Observation::Down(ReasonCode::Unreachable)
                    }
                } else {
                    http_probe(&self.client, &format!("{base}{path}")).await
                }
            }
            ProbeKind::Tcp { port } => tcp_observation(port).await,
            ProbeKind::ContainerOnly => Observation::Up,
            ProbeKind::DockerInfo => docker_info().await,
            ProbeKind::BuiltInReader | ProbeKind::BuiltInSync => {
                match crate::commands::whoop_status::whoop_status().await {
                    Ok(s) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        builtin_observation(def.probe, &s, now)
                    }
                    Err(_) => Observation::Down(ReasonCode::NoStore),
                }
            }
        };
        worst.worst(net)
    }
}

impl Default for RealProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe for RealProbe {
    fn prepare<'a>(&'a self, defs: &'a [ServiceDef]) -> BoxFuture<'a, ()> {
        Box::pin(self.read_all_containers(defs))
    }

    fn check<'a>(&'a self, def: &'a ServiceDef) -> BoxFuture<'a, Observation> {
        Box::pin(async move {
            tokio::time::timeout(PROBE_TIMEOUT, self.check_inner(def))
                .await
                .unwrap_or(Observation::Down(ReasonCode::Timeout))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(status: &str, exit: i64, health: Option<&str>, ips: &[&str]) -> ContainerFacts {
        ContainerFacts {
            status: status.into(),
            exit_code: exit,
            health: health.map(str::to_string),
            host_ips: ips.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn http_status_table() {
        assert_eq!(http_observation(200, 10), Observation::Up);
        assert_eq!(http_observation(204, 999), Observation::Up);
        assert_eq!(http_observation(200, 1_000), Observation::Slow);
        assert_eq!(http_observation(401, 5), Observation::Up);
        assert_eq!(http_observation(403, 5), Observation::Up);
        assert_eq!(
            http_observation(404, 5),
            Observation::Down(ReasonCode::BadStatus)
        );
        assert_eq!(
            http_observation(500, 5),
            Observation::Down(ReasonCode::BadStatus)
        );
        assert_eq!(
            http_observation(302, 5),
            Observation::Down(ReasonCode::BadStatus)
        );
    }

    #[test]
    fn parses_the_fixed_inspect_format() {
        let json = r#"{"status":"running","exit":0,"health":"healthy","ports":{"5432/tcp":[{"HostIp":"127.0.0.1","HostPort":"5432"}],"9/tcp":null}}"#;
        let f = parse_inspect(json).unwrap();
        assert_eq!(f, facts("running", 0, Some("healthy"), &["127.0.0.1"]));
        let no_health = r#"{"status":"exited","exit":137,"health":null,"ports":{}}"#;
        assert_eq!(
            parse_inspect(no_health).unwrap(),
            facts("exited", 137, None, &[])
        );
        assert!(parse_inspect("not json").is_none());
        assert!(parse_inspect("{}").is_none());
    }

    #[test]
    fn batched_inspect_lines_are_matched_to_names_and_missing_ones_classified() {
        let up = r#"{"name":"/stack-n8n-1","status":"running","exit":0,"health":null,"ports":{"5678/tcp":[{"HostIp":"127.0.0.1","HostPort":"5678"}]}}"#;
        let wide = r#"{"name":"/stack-letta-1","status":"running","exit":0,"health":"healthy","ports":{"8283/tcp":[{"HostIp":"0.0.0.0","HostPort":"8283"}]}}"#;
        let out = format!("{up}\n{wide}\n");
        let names = ["stack-n8n-1", "stack-letta-1", "stack-crew-1"];
        let m = batch_observations(&names, &out, StderrClass::NoSuchObject);
        assert_eq!(m["stack-n8n-1"], Observation::Up);
        assert_eq!(
            m["stack-letta-1"],
            Observation::NeedsYou(ReasonCode::BadBind)
        );
        assert_eq!(
            m["stack-crew-1"],
            Observation::NeedsYou(ReasonCode::MissingContainer)
        );
        // No output and a daemon error: every name is docker-unavailable, never "missing".
        let m = batch_observations(&names, "", StderrClass::DaemonDown);
        assert!(m
            .values()
            .all(|o| *o == Observation::NeedsYou(ReasonCode::DockerUnavailable)));
        // Garbage lines are ignored.
        let m = batch_observations(&names[..1], "junk\n{}\n", StderrClass::Clean);
        assert_eq!(
            m["stack-n8n-1"],
            Observation::NeedsYou(ReasonCode::DockerUnavailable)
        );
        assert_eq!(
            parse_inspect_named(up).unwrap().0,
            "stack-n8n-1".to_string()
        );
        assert!(parse_inspect_named(r#"{"status":"running"}"#).is_none());
    }

    #[test]
    fn only_the_router_url_is_taken_from_the_config_body() {
        let body = br#"{"telegram_bot_token":"SECRET-TOKEN-1","gateway_auth":"SECRET-2","amara_router_url":"http://127.0.0.1:8788"}"#;
        assert_eq!(
            router_url_from(body).as_deref(),
            Some("http://127.0.0.1:8788")
        );
        assert_eq!(router_url_from(br#"{"telegram_bot_token":"x"}"#), None);
        assert_eq!(router_url_from(b"not json"), None);
        // The probe never calls the full config loader (it carries the bot token and gateway auth).
        let src = include_str!("probe.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(!prod.contains("load_config"));
    }

    #[test]
    fn the_config_read_is_capped_and_refuses_links_and_non_files() {
        let d = std::env::temp_dir().join(format!("sup-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("config.json");
        std::fs::write(&f, r#"{"amara_router_url":"http://127.0.0.1:1"}"#).unwrap();
        assert_eq!(read_router_url(&f).as_deref(), Some("http://127.0.0.1:1"));
        let link = d.join("l.json");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        assert_eq!(read_router_url(&link), None);
        assert_eq!(read_router_url(&d), None);
        // Past the cap the JSON is cut and reads as nothing, never as a huge allocation.
        let mut big = String::from(r#"{"pad":""#);
        big.push_str(&"x".repeat(CONFIG_READ_CAP as usize + 10));
        big.push_str(r#"","amara_router_url":"http://127.0.0.1:2"}"#);
        std::fs::write(&f, big).unwrap();
        assert_eq!(read_router_url(&f), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_tick_reads_every_registry_container_in_one_call() {
        use crate::commands::supervisor::registry::Registry;
        let names = batch_names(Registry::builtin().defs());
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"stack-verba-1") && names.contains(&"homeassistant"));
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 8, "no duplicates");
        // Two docker calls per tick in all: this one, plus `docker info`.
        assert!(INSPECT_FORMAT_NAMED.starts_with("{\"name\":"));
    }

    #[test]
    fn container_states_map_to_observations() {
        use Observation::*;
        let loop_ = ["127.0.0.1"];
        assert_eq!(
            container_observation(&facts("running", 0, None, &loop_)),
            Up
        );
        assert_eq!(
            container_observation(&facts("running", 0, Some("healthy"), &loop_)),
            Up
        );
        assert_eq!(
            container_observation(&facts("running", 0, Some("starting"), &loop_)),
            Starting(ReasonCode::ContainerStarting)
        );
        assert_eq!(
            container_observation(&facts("running", 0, Some("unhealthy"), &loop_)),
            Down(ReasonCode::Unhealthy)
        );
        assert_eq!(
            container_observation(&facts("exited", 0, None, &[])),
            Stopped(ReasonCode::ContainerStopped)
        );
        assert_eq!(
            container_observation(&facts("exited", 1, None, &[])),
            Down(ReasonCode::ContainerDown)
        );
        assert_eq!(
            container_observation(&facts("created", 0, None, &[])),
            Stopped(ReasonCode::ContainerStopped)
        );
        assert_eq!(
            container_observation(&facts("restarting", 0, None, &[])),
            Starting(ReasonCode::ContainerStarting)
        );
        assert_eq!(
            container_observation(&facts("paused", 0, None, &[])),
            Down(ReasonCode::ContainerDown)
        );
    }

    #[test]
    fn a_wide_host_ip_is_needs_you_bad_bind() {
        for ip in ["0.0.0.0", "::", "[::]", ""] {
            assert_eq!(
                container_observation(&facts("running", 0, Some("healthy"), &["127.0.0.1", ip])),
                Observation::NeedsYou(ReasonCode::BadBind),
                "{ip:?}"
            );
        }
        assert!(!is_wide_bind("127.0.0.1"));
        assert!(!is_wide_bind("::1"));
    }

    #[test]
    fn inspect_failures_are_classified_not_kept() {
        assert_eq!(
            inspect_failure("Error: No such object: stack-n8n-1"),
            Observation::NeedsYou(ReasonCode::MissingContainer)
        );
        assert_eq!(
            inspect_failure("Cannot connect to the Docker daemon at unix:///x"),
            Observation::NeedsYou(ReasonCode::DockerUnavailable)
        );
    }

    #[test]
    fn builtin_rows_from_whoop_status() {
        use crate::commands::whoop_status::WhoopOwner::*;
        let st = |owner, found, last| WhoopStatus {
            owner,
            store_found: found,
            last_sample_at: last,
            gaps_14d: Option::None,
        };
        let now = 1_000_000;
        assert_eq!(
            builtin_observation(ProbeKind::BuiltInReader, &st(None, true, Some(1)), now),
            Observation::Up
        );
        assert_eq!(
            builtin_observation(
                ProbeKind::BuiltInReader,
                &st(None, false, Option::None),
                now
            ),
            Observation::Stopped(ReasonCode::NoStore)
        );
        assert_eq!(
            builtin_observation(
                ProbeKind::BuiltInSync,
                &st(Amara, true, Some(now - 60)),
                now
            ),
            Observation::Up
        );
        assert_eq!(
            builtin_observation(
                ProbeKind::BuiltInSync,
                &st(None, true, Some(now - 10 * 3600)),
                now
            ),
            Observation::Slow
        );
        assert_eq!(
            builtin_observation(
                ProbeKind::BuiltInSync,
                &st(Legacy, true, Some(now - 10 * 3600)),
                now
            ),
            Observation::Up
        );
        assert_eq!(
            builtin_observation(ProbeKind::BuiltInSync, &st(None, false, Option::None), now),
            Observation::Stopped(ReasonCode::NoStore)
        );
    }

    #[tokio::test]
    async fn tcp_probe_sees_a_listener_and_a_closed_port() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        assert_eq!(tcp_observation(port).await, Observation::Up);
        drop(l);
        assert_eq!(
            tcp_observation(port).await,
            Observation::Down(ReasonCode::Unreachable)
        );
    }

    #[tokio::test]
    async fn http_probe_reads_only_the_status() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            for code in [200u16, 500] {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = "secret-looking-body";
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            }
        });
        let client = RealProbe::new().client;
        let url = format!("http://127.0.0.1:{port}/health");
        assert_eq!(http_probe(&client, &url).await, Observation::Up);
        assert_eq!(
            http_probe(&client, &url).await,
            Observation::Down(ReasonCode::BadStatus)
        );
        // closed port
        assert_eq!(
            http_probe(&client, &url).await,
            Observation::Down(ReasonCode::Unreachable)
        );
    }
}
