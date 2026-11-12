//! System map (decision 0011): `system_map_status` returns the nodes and paths of the Settings >
//! Services map, each path with a status from a LOCAL, READ-ONLY probe.
//!
//! Rules this module keeps:
//! - Loopback only (the router URL goes through `llm_router::router_base`, which refuses anything
//!   else). No new outbound host, nothing that writes, starts, stops or restarts a service.
//! - Probes run concurrently, each with a 2 s timeout, so the whole call returns in about 2 s.
//! - `reason` and `fix` are fixed strings per outcome. They are chosen from typed probe facts, so
//!   no response body, error text, path, URL or token can reach them.
//! - `lastOkAt` is an in-memory map (process lifetime, no disk).
//! - Where no probe is possible a path is "unknown" with a plain reason, never a guessed "ok".

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::ha::HaContainerStatus;
use super::kb_status::KbState;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const LETTA_ADDR: &str = "127.0.0.1:8283";
/// A backup older than this is "stale" on the map.
const BACKUP_STALE_HOURS: i64 = 48;

const TIMED_OUT: &str = "This check took too long to answer";
const TIMED_OUT_FIX: &str = "Wait a moment. If it stays red, restart that service";
const NOT_SET_UP: &str = "Not set up";
const DOCKER_FIX: &str = "Open Docker and start the stack";
const HERMES_FIX: &str = "Open Terminal and run: hermes gateway restart";
/// Documented commands (TAISUN-STEPS.md, amara-voice NOTE): fixed text, the only fixes that name a path.
const VOICE_RESTART_FIX: &str =
    "In Terminal, in ~/projects/amara-voice, run: ./amara stop then ./amara start";
const VOICE_START_FIX: &str = "In Terminal, in ~/projects/amara-voice, run: ./amara start";
const ROUTER_RESTART_FIX: &str =
    "In Terminal run: launchctl kickstart -k gui/$(id -u)/com.taisun.amara-router";
#[cfg(test)]
const COMMAND_FIXES: [&str; 3] = [VOICE_RESTART_FIX, VOICE_START_FIX, ROUTER_RESTART_FIX];

// ---- contract types (src-console/standalone/systemMap.ts) ----

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    App,
    Backend,
    Service,
    Device,
    Store,
    External,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PathStatus {
    Ok,
    Broken,
    Off,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapPath {
    pub id: String,
    pub from: String,
    pub to: String,
    pub carries: String,
    pub status: PathStatus,
    pub last_ok_at: Option<String>,
    pub reason: Option<String>,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemMap {
    pub nodes: Vec<MapNode>,
    pub paths: Vec<MapPath>,
    pub checked_at: String,
}

// ---- topology (same ids, labels and wording as the sample in systemMap.ts) ----

const NODES: &[(&str, &str, NodeKind)] = &[
    ("app", "Amara OS", NodeKind::App),
    ("router", "Amara Router", NodeKind::Backend),
    ("ollama", "Local models", NodeKind::Backend),
    ("hermes", "Hermes (Chat)", NodeKind::Backend),
    ("noop", "Health sync", NodeKind::Store),
    ("muse", "Muse headset", NodeKind::Device),
    ("crew", "Crews", NodeKind::Service),
    ("n8n", "n8n", NodeKind::Service),
    ("postgres", "Postgres", NodeKind::Store),
    ("letta", "Letta memory", NodeKind::Store),
    ("verba", "Knowledge base", NodeKind::Service),
    ("ha", "Home Assistant", NodeKind::Service),
    ("voice", "Amara Voice", NodeKind::Service),
    ("telegram", "Telegram", NodeKind::External),
    ("backups", "Backups", NodeKind::Store),
];

/// (id, from, to, carries)
const PATHS: &[(&str, &str, &str, &str)] = &[
    ("app-router", "app", "router", "Every model request"),
    (
        "router-ollama",
        "router",
        "ollama",
        "Prompts to the local models",
    ),
    ("app-hermes", "app", "hermes", "Chat messages and replies"),
    ("hermes-router", "hermes", "router", "Amara's thinking"),
    ("noop-app", "noop", "app", "Heart rate, sleep, recovery"),
    ("muse-app", "muse", "app", "EEG while a session records"),
    ("app-crew", "app", "crew", "Crew runs and approvals"),
    ("crew-router", "crew", "router", "Crew model calls"),
    ("n8n-postgres", "n8n", "postgres", "Workflow data"),
    ("crew-n8n", "crew", "n8n", "Knowledge and memory lookups"),
    ("n8n-verba", "n8n", "verba", "Knowledge base queries"),
    ("app-letta", "app", "letta", "Long-term memory reads"),
    ("app-ha", "app", "ha", "Smart home status"),
    ("voice-hermes", "voice", "hermes", "Spoken requests"),
    (
        "app-telegram",
        "app",
        "telegram",
        "Notifications to your phone",
    ),
    ("app-backups", "app", "backups", "Your backups"),
];

// ---- probe facts (typed: nothing free-form survives collection) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fail {
    Timeout,
    Failed,
}
pub(crate) type Probe<T> = Result<T, Fail>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouterFacts {
    /// `Some(false)` when the router answered but says Ollama is down; `None` when it didn't say.
    pub ollama_up: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tg {
    Connected,
    Offline,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HermesFacts {
    NotSetUp,
    Seen {
        running: bool,
        routed_via_router: bool,
        telegram: Tg,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoopState {
    Flowing,
    WaitingForData,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MuseFacts {
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrewState {
    NotConfigured,
    Offline,
    Degraded,
    Operational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HaFacts {
    Running,
    NotSetUp,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VoiceFacts {
    pub installed: bool,
    pub running: bool,
    pub launchd: bool,
    pub errored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackupFacts {
    pub nightly: bool,
    /// Age of the newest backup in whole hours; `None` when there is none.
    pub newest_age_hours: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outbound {
    Open,
    Paused,
    NavyMode,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Probes {
    pub router: Probe<RouterFacts>,
    pub hermes: Probe<HermesFacts>,
    pub noop: Probe<NoopState>,
    pub muse: Probe<MuseFacts>,
    pub crew: Probe<CrewState>,
    pub n8n: Probe<bool>,
    pub kb: Probe<KbState>,
    pub ha: Probe<HaFacts>,
    pub voice: Probe<VoiceFacts>,
    pub letta: Probe<bool>,
    pub backups: Probe<BackupFacts>,
    pub outbound: Outbound,
}

// ---- verdicts: fixed strings only ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub status: PathStatus,
    pub reason: Option<&'static str>,
    pub fix: Option<&'static str>,
}

const fn ok() -> Verdict {
    Verdict {
        status: PathStatus::Ok,
        reason: None,
        fix: None,
    }
}
const fn broken(reason: &'static str, fix: &'static str) -> Verdict {
    Verdict {
        status: PathStatus::Broken,
        reason: Some(reason),
        fix: Some(fix),
    }
}
const fn off(reason: &'static str, fix: Option<&'static str>) -> Verdict {
    Verdict {
        status: PathStatus::Off,
        reason: Some(reason),
        fix,
    }
}
const fn unknown(reason: &'static str, fix: Option<&'static str>) -> Verdict {
    Verdict {
        status: PathStatus::Unknown,
        reason: Some(reason),
        fix,
    }
}

/// A probe that itself failed: a timeout always reads the same; anything else uses the service's own words.
fn failed(f: Fail, reason: &'static str, fix: &'static str) -> Verdict {
    match f {
        Fail::Timeout => broken(TIMED_OUT, TIMED_OUT_FIX),
        Fail::Failed => broken(reason, fix),
    }
}

fn app_router(p: &Probes) -> Verdict {
    match p.router {
        Ok(_) => ok(),
        Err(f) => failed(
            f,
            "Amara Router isn't answering on this Mac",
            ROUTER_RESTART_FIX,
        ),
    }
}

fn router_ollama(p: &Probes) -> Verdict {
    match p.router {
        Ok(RouterFacts {
            ollama_up: Some(true),
        }) => ok(),
        Ok(RouterFacts {
            ollama_up: Some(false),
        }) => broken(
            "The local models aren't answering",
            "Open Ollama and make sure it is running",
        ),
        Ok(RouterFacts { ollama_up: None }) => unknown(
            "Amara Router didn't say whether the local models are up",
            None,
        ),
        Err(_) => unknown(
            "Amara Router isn't answering, so the local models can't be checked",
            None,
        ),
    }
}

fn app_hermes(p: &Probes) -> Verdict {
    match p.hermes {
        Ok(HermesFacts::NotSetUp) => off(NOT_SET_UP, None),
        Ok(HermesFacts::Seen { running: true, .. }) => ok(),
        Ok(HermesFacts::Seen { running: false, .. }) => {
            broken("Hermes isn't answering on this Mac", HERMES_FIX)
        }
        Err(f) => failed(f, "Hermes isn't answering on this Mac", HERMES_FIX),
    }
}

fn hermes_router(p: &Probes) -> Verdict {
    match p.hermes {
        Ok(HermesFacts::NotSetUp) => off(NOT_SET_UP, None),
        Ok(HermesFacts::Seen {
            running: true,
            routed_via_router: true,
            ..
        }) => ok(),
        Ok(HermesFacts::Seen {
            running: true,
            routed_via_router: false,
            ..
        }) => broken(
            "Hermes isn't set up to think through Amara Router",
            "Point Hermes at Amara Router in its settings, then run: hermes gateway restart",
        ),
        Ok(HermesFacts::Seen { running: false, .. }) | Err(_) => {
            unknown("Hermes isn't answering, so this can't be checked", None)
        }
    }
}

fn noop_app(p: &Probes) -> Verdict {
    match p.noop {
        Ok(NoopState::Flowing) => ok(),
        Ok(NoopState::WaitingForData) => unknown(
            "NOOP hasn't recorded any health data yet",
            Some("Open NOOP and let it sync with the strap"),
        ),
        Ok(NoopState::Stale) => broken(
            "The health data is more than 2 days old",
            "Open NOOP and let it sync with the strap",
        ),
        // No NOOP store on this Mac = not set up (grey), not a break (Taisun, decision 0011).
        Ok(NoopState::Unavailable) => off(
            "Health sync isn't set up on this Mac",
            Some("Open NOOP once so it can create its data"),
        ),
        Err(f) => failed(
            f,
            "Amara can't read the NOOP health data on this Mac",
            "Open NOOP once so it can create its data",
        ),
    }
}

fn muse_app(p: &Probes) -> Verdict {
    match p.muse {
        Ok(MuseFacts { connected: true }) => ok(),
        Ok(MuseFacts { connected: false }) => unknown(
            "Not connected right now",
            Some("Turn the headset on and press Connect in Neuro"),
        ),
        // Busy connecting is not a break.
        Err(_) => unknown(
            "The headset is busy, so it can't be checked right now",
            None,
        ),
    }
}

fn app_crew(p: &Probes) -> Verdict {
    match p.crew {
        Ok(CrewState::Operational) => ok(),
        Ok(CrewState::NotConfigured) => off(NOT_SET_UP, None),
        Ok(CrewState::Offline) => broken("The crew service isn't answering", DOCKER_FIX),
        Ok(CrewState::Degraded) => broken(
            "The crew service answered but isn't healthy",
            "Restart the crew container in Docker",
        ),
        Err(f) => failed(f, "The crew service isn't answering", DOCKER_FIX),
    }
}

fn crew_n8n(p: &Probes) -> Verdict {
    match p.n8n {
        Ok(true) => ok(),
        Ok(false) => broken("n8n isn't answering", DOCKER_FIX),
        Err(f) => failed(f, "n8n isn't answering", DOCKER_FIX),
    }
}

fn n8n_verba(p: &Probes) -> Verdict {
    match p.kb {
        Ok(KbState::Running | KbState::Empty) => ok(),
        Ok(KbState::NotRunning) => broken("The knowledge base isn't answering", DOCKER_FIX),
        Ok(KbState::Error) => broken(
            "The knowledge base answered with an error",
            "Restart the knowledge base container in Docker",
        ),
        Err(f) => failed(f, "The knowledge base isn't answering", DOCKER_FIX),
    }
}

fn app_letta(p: &Probes) -> Verdict {
    match p.letta {
        Ok(true) => ok(),
        Ok(false) => off("Letta memory isn't running (not set up yet)", None),
        Err(f) => failed(
            f,
            "Letta memory isn't answering",
            "Open Docker and start the stack",
        ),
    }
}

fn app_ha(p: &Probes) -> Verdict {
    match p.ha {
        Ok(HaFacts::Running) => ok(),
        Ok(HaFacts::NotSetUp) => off(NOT_SET_UP, None),
        Ok(HaFacts::Stopped) => broken(
            "Home Assistant is stopped",
            "Open Docker and start the homeassistant container",
        ),
        Err(f) => failed(
            f,
            "Docker isn't answering, so Home Assistant can't be checked",
            "Open Docker Desktop",
        ),
    }
}

fn voice_hermes(p: &Probes) -> Verdict {
    match p.voice {
        Ok(VoiceFacts {
            installed: false, ..
        }) => off(NOT_SET_UP, None),
        Ok(VoiceFacts { errored: true, .. }) => {
            broken("Amara Voice reported an error", VOICE_RESTART_FIX)
        }
        Ok(VoiceFacts { running: true, .. }) => ok(),
        Ok(VoiceFacts {
            running: false,
            launchd: true,
            ..
        }) => broken("Amara Voice should be running but isn't", VOICE_START_FIX),
        Ok(VoiceFacts {
            running: false,
            launchd: false,
            ..
        }) => off(
            "Amara Voice is stopped",
            Some("Start it from Settings > Services"),
        ),
        Err(f) => failed(f, "Amara Voice isn't answering", VOICE_RESTART_FIX),
    }
}

fn app_telegram(p: &Probes) -> Verdict {
    match p.outbound {
        Outbound::NavyMode => {
            return off(
                "Navy mode is on, so nothing leaves this Mac",
                Some("Turn navy mode off to send again"),
            )
        }
        Outbound::Paused => return off("Sends are paused (outbound is off)", None),
        Outbound::Open => {}
    }
    match p.hermes {
        Ok(HermesFacts::NotSetUp) => off(NOT_SET_UP, None),
        Ok(HermesFacts::Seen {
            telegram: Tg::Connected,
            ..
        }) => ok(),
        Ok(HermesFacts::Seen {
            telegram: Tg::Offline,
            ..
        }) => broken(
            "Telegram can't send because Hermes isn't running",
            HERMES_FIX,
        ),
        Ok(HermesFacts::Seen {
            telegram: Tg::Unknown,
            ..
        })
        | Err(_) => unknown("Telegram's connection isn't confirmed yet", None),
    }
}

fn app_backups(p: &Probes) -> Verdict {
    match p.backups {
        Err(f) => failed(
            f,
            "Amara can't reach the backup folder",
            "Check the backup folder in Settings > Backups",
        ),
        Ok(BackupFacts {
            newest_age_hours: Some(h),
            ..
        }) if h <= BACKUP_STALE_HOURS => ok(),
        Ok(BackupFacts { nightly: false, .. }) => off("Nightly backups are turned off", None),
        Ok(BackupFacts {
            newest_age_hours: Some(_),
            ..
        // The nightly timer isn't built yet (prune needs Taisun's OK), so an old backup is
        // expected, not a break: unknown with the fix, never red.
        }) => unknown(
            "The last backup is more than 2 days old (nightly backups aren't running yet)",
            Some("Open Settings > Backups and press Back up now"),
        ),
        Ok(BackupFacts {
            newest_age_hours: None,
            ..
        }) => unknown(
            "No backup has been made yet",
            Some("Open Settings > Backups and press Back up now"),
        ),
    }
}

/// Paths with no safe local probe: always unknown, never a guessed ok.
fn no_probe(path_id: &str) -> Verdict {
    match path_id {
        "crew-router" => unknown("No safe check exists for the crew's model calls yet", None),
        "n8n-postgres" => unknown("No safe check exists for n8n's database yet", None),
        _ => unknown("No check exists for this connection yet", None),
    }
}

fn verdict_for(path_id: &str, p: &Probes) -> Verdict {
    match path_id {
        "app-router" => app_router(p),
        "router-ollama" => router_ollama(p),
        "app-hermes" => app_hermes(p),
        "hermes-router" => hermes_router(p),
        "noop-app" => noop_app(p),
        "muse-app" => muse_app(p),
        "app-crew" => app_crew(p),
        "crew-n8n" => crew_n8n(p),
        "n8n-verba" => n8n_verba(p),
        "app-letta" => app_letta(p),
        "app-ha" => app_ha(p),
        "voice-hermes" => voice_hermes(p),
        "app-telegram" => app_telegram(p),
        "app-backups" => app_backups(p),
        other => no_probe(other),
    }
}

/// Pure: topology plus a verdict per path. `lastOkAt` is filled in afterwards.
pub(crate) fn build_map(p: &Probes, checked_at: DateTime<Utc>) -> SystemMap {
    let nodes = NODES
        .iter()
        .map(|(id, label, kind)| MapNode {
            id: (*id).to_string(),
            label: (*label).to_string(),
            kind: *kind,
        })
        .collect();
    let paths = PATHS
        .iter()
        .map(|(id, from, to, carries)| {
            let v = verdict_for(id, p);
            MapPath {
                id: (*id).to_string(),
                from: (*from).to_string(),
                to: (*to).to_string(),
                carries: (*carries).to_string(),
                status: v.status,
                last_ok_at: None,
                reason: v.reason.map(str::to_string),
                fix: v.fix.map(str::to_string),
            }
        })
        .collect();
    SystemMap {
        nodes,
        paths,
        checked_at: iso(checked_at),
    }
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Remembers when each path was last seen ok (process lifetime only) and fills `lastOkAt`.
pub(crate) fn apply_last_ok(
    map: &mut SystemMap,
    seen: &mut HashMap<String, DateTime<Utc>>,
    now: DateTime<Utc>,
) {
    for p in &mut map.paths {
        if p.status == PathStatus::Ok {
            seen.insert(p.id.clone(), now);
        }
        p.last_ok_at = seen.get(&p.id).map(|t| iso(*t));
    }
}

static LAST_OK: Mutex<Option<HashMap<String, DateTime<Utc>>>> = Mutex::new(None);

// ---- collectors: map existing internals to typed facts ----

async fn probe<T>(fut: impl std::future::Future<Output = Probe<T>>) -> Probe<T> {
    tokio::time::timeout(PROBE_TIMEOUT, fut)
        .await
        .unwrap_or(Err(Fail::Timeout))
}

async fn probe_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Probe<T> + Send + 'static,
) -> Probe<T> {
    probe(async {
        tokio::task::spawn_blocking(f)
            .await
            .unwrap_or(Err(Fail::Failed))
    })
    .await
}

/// `/health` of the router: 200 with `{"ollama":"up"|"down"}`. Only the one word is kept.
pub(crate) fn router_facts(status: u16, body: &str) -> Probe<RouterFacts> {
    if !(200..300).contains(&status) {
        return Err(Fail::Failed);
    }
    let ollama_up = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| match v.get("ollama").and_then(|o| o.as_str()) {
            Some("up") => Some(true),
            Some("down") => Some(false),
            _ => None,
        });
    Ok(RouterFacts { ollama_up })
}

async fn router_probe() -> Probe<RouterFacts> {
    let saved = super::telegram::load_config().amara_router_url;
    let base = super::llm_router::router_base(None, saved.as_deref());
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|_| Fail::Failed)?;
    let resp = client
        .get(format!("{base}/health"))
        .send()
        .await
        .map_err(|_| Fail::Failed)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    router_facts(status, &body)
}

fn tg_from(s: &str) -> Tg {
    match s {
        "connected" => Tg::Connected,
        "offline" => Tg::Offline,
        _ => Tg::Unknown,
    }
}

async fn hermes_probe() -> Probe<HermesFacts> {
    match super::terrence_runtime::get_terrence_runtime_health().await {
        Ok(h) => Ok(HermesFacts::Seen {
            running: h.gateway_running,
            routed_via_router: h.routed_via_router,
            telegram: tg_from(&h.telegram_status),
        }),
        // The only error is "no ~/.hermes": Hermes was never set up on this Mac.
        Err(_) => Ok(HermesFacts::NotSetUp),
    }
}

pub(crate) fn noop_state(status: &str) -> Probe<NoopState> {
    match status {
        "ready" | "waiting_for_nightly_data" => Ok(NoopState::Flowing),
        "waiting_for_data" => Ok(NoopState::WaitingForData),
        "stale" => Ok(NoopState::Stale),
        "unavailable" => Ok(NoopState::Unavailable),
        _ => Err(Fail::Failed),
    }
}

pub(crate) fn crew_state(s: &str) -> Probe<CrewState> {
    match s {
        "operational" => Ok(CrewState::Operational),
        "not_configured" => Ok(CrewState::NotConfigured),
        "offline" => Ok(CrewState::Offline),
        "degraded" => Ok(CrewState::Degraded),
        _ => Err(Fail::Failed),
    }
}

pub(crate) fn ha_facts(s: &HaContainerStatus) -> HaFacts {
    if s.running {
        HaFacts::Running
    } else if s.status.trim().is_empty() {
        HaFacts::NotSetUp
    } else {
        HaFacts::Stopped
    }
}

pub(crate) fn voice_facts(
    installed: bool,
    running: bool,
    launchd: bool,
    state: &str,
) -> VoiceFacts {
    VoiceFacts {
        installed,
        running,
        launchd,
        errored: state == "error",
    }
}

pub(crate) fn backup_facts(
    nightly: bool,
    newest_made_at: Option<&str>,
    now: DateTime<Utc>,
) -> BackupFacts {
    let newest_age_hours = newest_made_at
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| (now - t.with_timezone(&Utc)).num_hours().max(0));
    BackupFacts {
        nightly,
        newest_age_hours,
    }
}

async fn backups_probe() -> Probe<BackupFacts> {
    let settings = super::backups::backup_settings_get()
        .await
        .map_err(|_| Fail::Failed)?;
    // `backup_list` fails when the folder can't be read: that is the "reachable" check.
    let list = super::backups::backup_list()
        .await
        .map_err(|_| Fail::Failed)?;
    let newest = list.first().map(|b| b.made_at.clone());
    Ok(backup_facts(
        settings.nightly,
        newest.as_deref(),
        Utc::now(),
    ))
}

async fn collect_probes() -> Probes {
    let (router, hermes, noop, muse, crew, n8n, kb, ha, voice, letta, backups) = tokio::join!(
        probe(router_probe()),
        probe(hermes_probe()),
        probe_blocking(|| noop_state(&super::noop::get_noop_health_snapshot().status)),
        probe(async {
            super::muse::get_muse_state()
                .await
                .map(|m| MuseFacts {
                    connected: m.connected,
                })
                .map_err(|_| Fail::Failed)
        }),
        probe(async {
            match super::crew::crew_health().await {
                Ok(s) => crew_state(&s),
                Err(_) => Err(Fail::Failed),
            }
        }),
        probe(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(|_| Fail::Failed)?;
            Ok(super::n8n_status::n8n_healthz(&client, PROBE_TIMEOUT).await)
        }),
        probe(async {
            super::kb_status::kb_status()
                .await
                .map(|k| k.state)
                .map_err(|_| Fail::Failed)
        }),
        probe(async {
            super::ha::ha_container_status()
                .await
                .map(|s| ha_facts(&s))
                .map_err(|_| Fail::Failed)
        }),
        probe(async {
            super::voice::voice_status()
                .await
                .map(|v| voice_facts(v.installed, v.running, v.launchd, &v.state))
                .map_err(|_| Fail::Failed)
        }),
        probe(async {
            // No Letta client exists in this codebase: a bare TCP connect, nothing sent.
            Ok(tokio::net::TcpStream::connect(LETTA_ADDR).await.is_ok())
        }),
        probe(backups_probe()),
    );
    let outbound = if super::telegram::local_only_on() {
        Outbound::NavyMode
    } else if super::telegram::outbound_disabled() {
        Outbound::Paused
    } else {
        Outbound::Open
    };
    Probes {
        router,
        hermes,
        noop,
        muse,
        crew,
        n8n,
        kb,
        ha,
        voice,
        letta,
        backups,
        outbound,
    }
}

/// Read-only status of every path on the system map. Never an error: every state is a status.
#[tauri::command]
pub async fn system_map_status() -> Result<SystemMap, String> {
    let probes = collect_probes().await;
    let now = Utc::now();
    let mut map = build_map(&probes, now);
    if let Ok(mut guard) = LAST_OK.lock() {
        apply_last_ok(&mut map, guard.get_or_insert_with(HashMap::new), now);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 12, 1, 0, 0).unwrap()
    }

    /// Everything healthy.
    fn healthy() -> Probes {
        Probes {
            router: Ok(RouterFacts {
                ollama_up: Some(true),
            }),
            hermes: Ok(HermesFacts::Seen {
                running: true,
                routed_via_router: true,
                telegram: Tg::Connected,
            }),
            noop: Ok(NoopState::Flowing),
            muse: Ok(MuseFacts { connected: true }),
            crew: Ok(CrewState::Operational),
            n8n: Ok(true),
            kb: Ok(KbState::Running),
            ha: Ok(HaFacts::Running),
            voice: Ok(VoiceFacts {
                installed: true,
                running: true,
                launchd: true,
                errored: false,
            }),
            letta: Ok(true),
            backups: Ok(BackupFacts {
                nightly: true,
                newest_age_hours: Some(5),
            }),
            outbound: Outbound::Open,
        }
    }

    fn status_of(p: &Probes, id: &str) -> Verdict {
        verdict_for(id, p)
    }

    #[test]
    fn healthy_world_is_ok_except_paths_with_no_probe() {
        let map = build_map(&healthy(), now());
        for path in &map.paths {
            if path.id == "crew-router" || path.id == "n8n-postgres" {
                assert_eq!(path.status, PathStatus::Unknown, "{}", path.id);
                assert!(path.reason.is_some());
            } else {
                assert_eq!(path.status, PathStatus::Ok, "{}", path.id);
                assert!(path.reason.is_none() && path.fix.is_none(), "{}", path.id);
            }
        }
    }

    #[test]
    fn router_outcomes() {
        let mut p = healthy();
        p.router = Err(Fail::Failed);
        assert_eq!(status_of(&p, "app-router").status, PathStatus::Broken);
        assert_eq!(status_of(&p, "router-ollama").status, PathStatus::Unknown);
        p.router = Ok(RouterFacts {
            ollama_up: Some(false),
        });
        assert_eq!(status_of(&p, "app-router").status, PathStatus::Ok);
        assert_eq!(status_of(&p, "router-ollama").status, PathStatus::Broken);
        p.router = Ok(RouterFacts { ollama_up: None });
        assert_eq!(status_of(&p, "router-ollama").status, PathStatus::Unknown);
    }

    #[test]
    fn hermes_outcomes() {
        let mut p = healthy();
        p.hermes = Ok(HermesFacts::Seen {
            running: false,
            routed_via_router: true,
            telegram: Tg::Offline,
        });
        let v = status_of(&p, "app-hermes");
        assert_eq!(v.status, PathStatus::Broken);
        assert_eq!(v.reason, Some("Hermes isn't answering on this Mac"));
        assert_eq!(v.fix, Some("Open Terminal and run: hermes gateway restart"));
        assert_eq!(status_of(&p, "hermes-router").status, PathStatus::Unknown);
        assert_eq!(status_of(&p, "app-telegram").status, PathStatus::Broken);
        p.hermes = Ok(HermesFacts::Seen {
            running: true,
            routed_via_router: false,
            telegram: Tg::Unknown,
        });
        assert_eq!(status_of(&p, "app-hermes").status, PathStatus::Ok);
        assert_eq!(status_of(&p, "hermes-router").status, PathStatus::Broken);
        assert_eq!(status_of(&p, "app-telegram").status, PathStatus::Unknown);
        p.hermes = Ok(HermesFacts::NotSetUp);
        for id in ["app-hermes", "hermes-router", "app-telegram"] {
            let v = status_of(&p, id);
            assert_eq!(v.status, PathStatus::Off, "{id}");
            assert_eq!(v.reason, Some("Not set up"));
        }
    }

    #[test]
    fn noop_outcomes() {
        let mut p = healthy();
        for (state, want) in [
            (NoopState::Flowing, PathStatus::Ok),
            (NoopState::WaitingForData, PathStatus::Unknown),
            (NoopState::Stale, PathStatus::Broken),
            (NoopState::Unavailable, PathStatus::Off), // not set up = grey
        ] {
            p.noop = Ok(state);
            assert_eq!(status_of(&p, "noop-app").status, want);
        }
    }

    #[test]
    fn muse_not_connected_is_unknown_never_broken() {
        let mut p = healthy();
        p.muse = Ok(MuseFacts { connected: false });
        let v = status_of(&p, "muse-app");
        assert_eq!(v.status, PathStatus::Unknown);
        assert_eq!(v.reason, Some("Not connected right now"));
        // Even a timeout (the headset is mid-connect) is not a break.
        p.muse = Err(Fail::Timeout);
        assert_eq!(status_of(&p, "muse-app").status, PathStatus::Unknown);
    }

    #[test]
    fn crew_n8n_kb_outcomes() {
        let mut p = healthy();
        for (s, want) in [
            (CrewState::Operational, PathStatus::Ok),
            (CrewState::NotConfigured, PathStatus::Off),
            (CrewState::Offline, PathStatus::Broken),
            (CrewState::Degraded, PathStatus::Broken),
        ] {
            p.crew = Ok(s);
            assert_eq!(status_of(&p, "app-crew").status, want);
        }
        p.n8n = Ok(false);
        assert_eq!(status_of(&p, "crew-n8n").status, PathStatus::Broken);
        for (s, want) in [
            (KbState::Running, PathStatus::Ok),
            (KbState::Empty, PathStatus::Ok),
            (KbState::NotRunning, PathStatus::Broken),
            (KbState::Error, PathStatus::Broken),
        ] {
            p.kb = Ok(s);
            assert_eq!(status_of(&p, "n8n-verba").status, want);
        }
    }

    #[test]
    fn letta_ha_voice_outcomes() {
        let mut p = healthy();
        p.letta = Ok(false);
        assert_eq!(status_of(&p, "app-letta").status, PathStatus::Off);
        for (s, want) in [
            (HaFacts::Running, PathStatus::Ok),
            (HaFacts::NotSetUp, PathStatus::Off),
            (HaFacts::Stopped, PathStatus::Broken),
        ] {
            p.ha = Ok(s);
            assert_eq!(status_of(&p, "app-ha").status, want);
        }
        let voice = |installed, running, launchd, errored| VoiceFacts {
            installed,
            running,
            launchd,
            errored,
        };
        for (v, want) in [
            (voice(false, false, false, false), PathStatus::Off),
            (voice(true, true, true, false), PathStatus::Ok),
            (voice(true, true, true, true), PathStatus::Broken),
            (voice(true, false, true, false), PathStatus::Broken),
            (voice(true, false, false, false), PathStatus::Off),
        ] {
            p.voice = Ok(v);
            assert_eq!(status_of(&p, "voice-hermes").status, want, "{v:?}");
        }
    }

    #[test]
    fn telegram_off_with_a_reason_when_paused_on_purpose() {
        let mut p = healthy();
        p.outbound = Outbound::Paused;
        let v = status_of(&p, "app-telegram");
        assert_eq!(v.status, PathStatus::Off);
        assert_eq!(v.reason, Some("Sends are paused (outbound is off)"));
        p.outbound = Outbound::NavyMode;
        let v = status_of(&p, "app-telegram");
        assert_eq!(v.status, PathStatus::Off);
        assert!(v.reason.unwrap().contains("Navy mode"));
        // Paused wins even when Hermes is down.
        p.hermes = Err(Fail::Failed);
        assert_eq!(status_of(&p, "app-telegram").status, PathStatus::Off);
    }

    #[test]
    fn backup_outcomes() {
        let mut p = healthy();
        let b = |nightly, h| BackupFacts {
            nightly,
            newest_age_hours: h,
        };
        for (facts, want) in [
            (b(true, Some(47)), PathStatus::Ok),
            (b(true, Some(48)), PathStatus::Ok),
            (b(true, Some(49)), PathStatus::Unknown), // no nightly timer yet: never red
            (b(false, Some(500)), PathStatus::Off),
            (b(false, Some(3)), PathStatus::Ok),
            (b(true, None), PathStatus::Unknown),
        ] {
            p.backups = Ok(facts);
            assert_eq!(status_of(&p, "app-backups").status, want, "{facts:?}");
        }
        p.backups = Err(Fail::Failed);
        assert_eq!(status_of(&p, "app-backups").status, PathStatus::Broken);
    }

    #[test]
    fn backup_age_is_computed_from_the_newest_timestamp() {
        let n = now();
        let f = backup_facts(true, Some("2026-11-10T00:00:00+00:00"), n);
        assert_eq!(f.newest_age_hours, Some(49));
        assert_eq!(backup_facts(true, None, n).newest_age_hours, None);
        assert_eq!(
            backup_facts(true, Some("not a date"), n).newest_age_hours,
            None
        );
        // A timestamp in the future never goes negative.
        let f = backup_facts(true, Some("2026-11-13T00:00:00+00:00"), n);
        assert_eq!(f.newest_age_hours, Some(0));
    }

    #[test]
    fn timeouts_are_broken_with_one_fixed_reason() {
        let mut p = healthy();
        p.router = Err(Fail::Timeout);
        p.hermes = Err(Fail::Timeout);
        p.noop = Err(Fail::Timeout);
        p.crew = Err(Fail::Timeout);
        p.n8n = Err(Fail::Timeout);
        p.kb = Err(Fail::Timeout);
        p.ha = Err(Fail::Timeout);
        p.voice = Err(Fail::Timeout);
        p.letta = Err(Fail::Timeout);
        p.backups = Err(Fail::Timeout);
        for id in [
            "app-router",
            "app-hermes",
            "noop-app",
            "app-crew",
            "crew-n8n",
            "n8n-verba",
            "app-ha",
            "voice-hermes",
            "app-letta",
            "app-backups",
        ] {
            let v = status_of(&p, id);
            assert_eq!(v.status, PathStatus::Broken, "{id}");
            assert_eq!(v.reason, Some("This check took too long to answer"), "{id}");
        }
    }

    #[tokio::test]
    async fn a_hung_probe_times_out_as_a_timeout_failure() {
        let started = std::time::Instant::now();
        let r: Probe<bool> = probe(async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(true)
        })
        .await;
        assert_eq!(r, Err(Fail::Timeout));
        assert!(started.elapsed() < Duration::from_millis(2_600));
    }

    #[tokio::test]
    async fn a_hung_blocking_probe_times_out_too() {
        let r: Probe<bool> = probe_blocking(|| {
            std::thread::sleep(Duration::from_secs(3));
            Ok(true)
        })
        .await;
        assert_eq!(r, Err(Fail::Timeout));
    }

    #[tokio::test]
    async fn probes_run_concurrently() {
        let started = std::time::Instant::now();
        let slow = || {
            probe(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok::<bool, Fail>(true)
            })
        };
        let _ = tokio::join!(slow(), slow(), slow(), slow(), slow());
        assert!(started.elapsed() < Duration::from_millis(3_000));
    }

    // ---- free-form inputs never reach reason / fix ----

    const HOSTILE: &str =
        "tok_SECRET123 /Users/someone/.hermes https://evil.example/?k=v Bearer abc";

    fn assert_clean(map: &SystemMap) {
        let json = serde_json::to_string(map).unwrap();
        for bad in [
            "SECRET",
            "/Users",
            "evil.example",
            "Bearer",
            ".hermes",
            "http",
        ] {
            assert!(!json.contains(bad), "leaked {bad}");
        }
        for p in &map.paths {
            for text in [p.reason.as_deref(), p.fix.as_deref()]
                .into_iter()
                .flatten()
            {
                if COMMAND_FIXES.contains(&text) {
                    continue; // fixed, documented command text; never built from input
                }
                assert!(!text.contains('/') || text.contains("Settings >"), "{text}");
                assert!(!text.contains("://") && !text.contains('@') && !text.contains('~'));
            }
        }
    }

    #[test]
    fn hostile_input_text_is_dropped_by_every_collector_mapping() {
        let mut p = healthy();
        p.router = router_facts(500, HOSTILE);
        p.noop = noop_state(HOSTILE);
        p.crew = crew_state(HOSTILE);
        p.ha = Ok(ha_facts(&HaContainerStatus {
            running: false,
            status: HOSTILE.to_string(),
        }));
        let voice = voice_facts(true, false, true, HOSTILE);
        p.voice = Ok(voice);
        p.hermes = Ok(HermesFacts::Seen {
            running: true,
            routed_via_router: true,
            telegram: tg_from(HOSTILE),
        });
        assert_clean(&build_map(&p, now()));
        // A 200 with a hostile body still yields only the one parsed word.
        assert_eq!(
            router_facts(200, HOSTILE),
            Ok(RouterFacts { ollama_up: None })
        );
        assert_eq!(
            router_facts(200, r#"{"ollama":"down","error":"/Users/x tok_SECRET"}"#),
            Ok(RouterFacts {
                ollama_up: Some(false)
            })
        );
    }

    #[test]
    fn every_outcome_combination_has_clean_fixed_strings() {
        // Sweep the failure and off variants through the whole map.
        for fail in [Fail::Timeout, Fail::Failed] {
            let p = Probes {
                router: Err(fail),
                hermes: Err(fail),
                noop: Err(fail),
                muse: Err(fail),
                crew: Err(fail),
                n8n: Err(fail),
                kb: Err(fail),
                ha: Err(fail),
                voice: Err(fail),
                letta: Err(fail),
                backups: Err(fail),
                outbound: Outbound::Open,
            };
            let map = build_map(&p, now());
            assert_clean(&map);
            assert!(map.paths.iter().all(|x| x.status != PathStatus::Ok));
        }
        for outbound in [Outbound::Paused, Outbound::NavyMode] {
            let mut p = healthy();
            p.outbound = outbound;
            assert_clean(&build_map(&p, now()));
        }
    }

    #[test]
    fn collectors_map_known_words_and_reject_unknown() {
        assert_eq!(noop_state("ready"), Ok(NoopState::Flowing));
        assert_eq!(
            noop_state("waiting_for_nightly_data"),
            Ok(NoopState::Flowing)
        );
        assert_eq!(noop_state("stale"), Ok(NoopState::Stale));
        assert_eq!(noop_state("who knows"), Err(Fail::Failed));
        assert_eq!(crew_state("operational"), Ok(CrewState::Operational));
        assert_eq!(crew_state("offline"), Ok(CrewState::Offline));
        assert_eq!(crew_state("boom"), Err(Fail::Failed));
        assert_eq!(router_facts(503, "{}"), Err(Fail::Failed));
        assert_eq!(
            router_facts(200, r#"{"status":"ok","ollama":"up"}"#),
            Ok(RouterFacts {
                ollama_up: Some(true)
            })
        );
        let ha = |running, status: &str| {
            ha_facts(&HaContainerStatus {
                running,
                status: status.to_string(),
            })
        };
        assert_eq!(ha(true, "Up 3 hours"), HaFacts::Running);
        assert_eq!(ha(false, ""), HaFacts::NotSetUp);
        assert_eq!(ha(false, "Exited (0) 2 minutes ago"), HaFacts::Stopped);
    }

    // ---- lastOkAt ----

    #[test]
    fn last_ok_at_is_remembered_in_memory_and_never_invented() {
        let mut seen = HashMap::new();
        let t1 = now();
        let mut m1 = build_map(&healthy(), t1);
        apply_last_ok(&mut m1, &mut seen, t1);
        let ok_path = m1.paths.iter().find(|p| p.id == "app-router").unwrap();
        assert_eq!(ok_path.last_ok_at.as_deref(), Some("2026-11-12T01:00:00Z"));
        let never = m1.paths.iter().find(|p| p.id == "crew-router").unwrap();
        assert_eq!(never.last_ok_at, None);

        let t2 = t1 + chrono::Duration::seconds(10);
        let mut broken_world = healthy();
        broken_world.router = Err(Fail::Failed);
        let mut m2 = build_map(&broken_world, t2);
        apply_last_ok(&mut m2, &mut seen, t2);
        let p = m2.paths.iter().find(|p| p.id == "app-router").unwrap();
        assert_eq!(p.status, PathStatus::Broken);
        assert_eq!(p.last_ok_at.as_deref(), Some("2026-11-12T01:00:00Z"));
        // A path that was never ok stays null.
        assert_eq!(
            m2.paths
                .iter()
                .find(|p| p.id == "crew-router")
                .unwrap()
                .last_ok_at,
            None
        );
    }

    // ---- shape and topology ----

    #[test]
    fn serializes_to_the_contract_shape() {
        let map = build_map(&healthy(), now());
        let v = serde_json::to_value(&map).unwrap();
        assert_eq!(v["checkedAt"], "2026-11-12T01:00:00Z");
        assert_eq!(v["nodes"][0]["kind"], "app");
        let path = &v["paths"][0];
        for key in [
            "id", "from", "to", "carries", "status", "lastOkAt", "reason", "fix",
        ] {
            assert!(path.get(key).is_some(), "missing {key}");
        }
        assert_eq!(path["status"], "ok");
    }

    const SAMPLE_TS: &str = include_str!("../../../src-console/standalone/systemMap.ts");

    /// Quoted string arguments of every `<fn_name>("a", "b", ...` call in the sample.
    fn sample_calls(fn_name: &str, args: usize) -> Vec<Vec<String>> {
        let needle = format!("{fn_name}(\"");
        let mut out = Vec::new();
        let mut rest = SAMPLE_TS;
        while let Some(i) = rest.find(&needle) {
            // Skip matches that are the tail of a longer identifier (e.g. `map(`).
            let before = rest[..i].chars().last();
            rest = &rest[i + needle.len() - 1..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let mut vals = Vec::new();
            let mut cur = rest;
            while vals.len() < args {
                let Some(start) = cur.strip_prefix('"') else {
                    break;
                };
                let Some(end) = start.find('"') else { break };
                vals.push(start[..end].to_string());
                cur = start[end + 1..].trim_start_matches(',').trim_start();
            }
            if vals.len() == args {
                out.push(vals);
            }
        }
        out
    }

    #[test]
    fn topology_ids_equal_the_sample() {
        let nodes = sample_calls("n", 3);
        let ids: Vec<&str> = NODES.iter().map(|n| n.0).collect();
        let sample_ids: Vec<&str> = nodes.iter().map(|v| v[0].as_str()).collect();
        assert_eq!(ids, sample_ids);
        let labels: Vec<&str> = NODES.iter().map(|n| n.1).collect();
        assert_eq!(
            labels,
            nodes.iter().map(|v| v[1].as_str()).collect::<Vec<_>>()
        );

        let paths = sample_calls("p", 4);
        assert_eq!(paths.len(), PATHS.len());
        for (mine, theirs) in PATHS.iter().zip(&paths) {
            assert_eq!(
                [mine.0, mine.1, mine.2, mine.3],
                [&theirs[0], &theirs[1], &theirs[2], &theirs[3]]
            );
        }
        // Every path endpoint is a node.
        for (_, from, to, _) in PATHS {
            assert!(ids.contains(from) && ids.contains(to));
        }
    }

    #[test]
    fn module_only_talks_to_loopback() {
        let src = include_str!("system_map.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(!prod.contains("https://"));
        let mut rest = prod;
        while let Some(i) = rest.find("http://") {
            let after = &rest[i + "http://".len()..];
            assert!(
                after.starts_with("127.0.0.1") || after.starts_with("localhost"),
                "non-loopback URL in system_map.rs"
            );
            rest = after;
        }
        // Every address constant is loopback.
        assert!(LETTA_ADDR.starts_with("127.0.0.1:"));
        // The router URL goes through the loopback-only resolver.
        assert!(prod.contains("router_base("));
        // No write-style HTTP verbs or process spawns here.
        for bad in [
            ".post(",
            ".put(",
            ".delete(",
            ".patch(",
            "Command::new",
            "fs::write",
        ] {
            assert!(!prod.contains(bad), "unexpected {bad}");
        }
    }
}
