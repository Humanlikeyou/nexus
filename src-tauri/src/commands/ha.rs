//! Home Assistant backend: states, service calls, Assist conversation, the HA container, and
//! live `state_changed` events over the HA websocket.
//!
//! - Precedence (resolved on every call, including `ha_live_*` and `ha_states`):
//!   - URL: app config `ha_url` (`<data_dir>/config.json`), then the `HA_URL` env var, then
//!     legacy `HA_URL` in `~/projects/amara-voice/.env`, then `http://127.0.0.1:8123`.
//!   - Token: `HA_TOKEN` in the app-data `.env` (`<data_dir>/.env`, written by
//!     `ha_set_config`), then the `HA_TOKEN` env var, then legacy `~/projects/amara-voice/.env`.
//!   - The app-data `.env` therefore wins over an env var. A present-but-unsafe app-data `.env`
//!     (symlink, wrong owner, group/other bits) fails closed (`unsafe_storage`); it never falls
//!     through to a weaker source.
//! - The URL must be a clean loopback `http://host[:port]`. `NEXUS_OUTBOUND_DISABLED` does not
//!   apply: HA never leaves the Mac.
//! - The token is held in a type with no `Serialize`/`Display`, a redacted `Debug`, zeroized on
//!   drop, and is never returned, logged or emitted. All errors are fixed text.
//! - Container start/stop run `docker compose` with fixed args (no shell), one at a time, with
//!   a timeout.

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_HA_URL: &str = "http://127.0.0.1:8123";
const ENV_MAX_BYTES: u64 = 16 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const DOCKER_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CONVERSATION_CHARS: usize = 1_000;
const MAX_STATES: usize = 5_000;
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(60);
/// Max HTTP response body for /api/states and /api/conversation/process.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const WS_MAX_MESSAGE: usize = 1024 * 1024;
const WS_MAX_FRAME: usize = 256 * 1024;
/// TCP connect + websocket handshake must finish within this.
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// No frame for this long → treat the socket as dead and reconnect with backoff.
const WS_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// auth_required → auth → auth_ok must complete within this.
const WS_AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Client ping cadence once subscribed.
const WS_PING_INTERVAL: Duration = Duration::from_secs(30);
/// Climate set_temperature bounds (covers °C and °F setpoints).
const MIN_TEMPERATURE: f64 = 5.0;
const MAX_TEMPERATURE: f64 = 95.0;
const HVAC_MODES: &[&str] = &[
    "off",
    "heat",
    "cool",
    "heat_cool",
    "auto",
    "dry",
    "fan_only",
];

pub const STATE_CHANGED_EVENT: &str = "ha://state_changed";
pub const LIVE_STATUS_EVENT: &str = "ha://live_status";

// ─── error kinds: every user-facing error is "<kind>: <text>" ────────────────

#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_NOT_CONFIGURED: &str = "not_configured";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_BAD_URL: &str = "bad_url";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_AUTH_REJECTED: &str = "auth_rejected";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_UNREACHABLE: &str = "unreachable";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_DOCKER_UNAVAILABLE: &str = "docker_unavailable";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_ALREADY_RUNNING: &str = "already_running";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_TIMED_OUT: &str = "timed_out";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_TOO_LARGE: &str = "too_large";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_NOT_ALLOWED: &str = "not_allowed";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_NEEDS_APPROVAL: &str = "needs_approval";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_DECLINED: &str = "declined";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_DOMAIN_MISMATCH: &str = "domain_mismatch";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_INVALID_INPUT: &str = "invalid_input";
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const KIND_UPSTREAM_ERROR: &str = "upstream_error";
/// The app-data `.env` or its folder is a symlink, wrong owner, or has group/other bits.
pub const KIND_UNSAFE_STORAGE: &str = "unsafe_storage";
/// Writing settings (app-data `.env` or config) failed.
pub const KIND_STORAGE_ERROR: &str = "storage_error";
/// A connection test with an unsaved URL must also send the token to use with it.
pub const KIND_TOKEN_REQUIRED: &str = "token_required";
/// All kinds, for the frontend and tests.
#[allow(dead_code)] // stable contract for the frontend; asserted in tests
pub const ERROR_KINDS: &[&str] = &[
    KIND_NOT_CONFIGURED,
    KIND_BAD_URL,
    KIND_AUTH_REJECTED,
    KIND_UNREACHABLE,
    KIND_DOCKER_UNAVAILABLE,
    KIND_ALREADY_RUNNING,
    KIND_TIMED_OUT,
    KIND_TOO_LARGE,
    KIND_NOT_ALLOWED,
    KIND_NEEDS_APPROVAL,
    KIND_DECLINED,
    KIND_DOMAIN_MISMATCH,
    KIND_INVALID_INPUT,
    KIND_UPSTREAM_ERROR,
    KIND_UNSAFE_STORAGE,
    KIND_STORAGE_ERROR,
    KIND_TOKEN_REQUIRED,
];

const ERR_CONFIG: &str =
    "not_configured: Home Assistant is not configured (check HA_URL and HA_TOKEN).";
const ERR_TOKEN_REQUIRED: &str =
    "token_required: Enter the token to test a URL that is not the saved one.";
const ERR_URL: &str = "bad_url: HA_URL must be a loopback http address.";
const ERR_UNREACHABLE: &str = "unreachable: Home Assistant is not reachable.";
const ERR_AUTH: &str = "auth_rejected: Home Assistant rejected the token.";
const ERR_FAILED: &str = "upstream_error: Home Assistant returned an error.";
const ERR_BAD_REPLY: &str = "upstream_error: Home Assistant sent an unexpected reply.";
const ERR_INVALID: &str = "invalid_input: Invalid domain, service or entity id.";
const ERR_TEXT: &str = "invalid_input: Message is empty or too long.";
const ERR_BUSY: &str = "already_running: A Home Assistant container action is already running.";
const ERR_DOCKER: &str = "docker_unavailable: Docker is not available.";
const ERR_DOCKER_FAILED: &str = "upstream_error: The Home Assistant container command failed.";
const ERR_DOCKER_TIMEOUT: &str = "timed_out: The Home Assistant container command timed out.";
const ERR_TOO_LARGE: &str = "too_large: Home Assistant sent a reply that is too large.";
const ERR_NOT_ALLOWED: &str = "not_allowed: That Home Assistant action is not allowed.";
const ERR_DOMAIN_MISMATCH: &str =
    "domain_mismatch: The entity does not belong to the service's domain.";
const ERR_BAD_DATA: &str =
    "invalid_input: Invalid or missing data for this Home Assistant service.";
const ERR_NEEDS_APPROVAL: &str =
    "needs_approval: This Home Assistant action needs approval, and the approval dialog could not be shown.";
const ERR_BUSY_APPROVAL: &str =
    "already_running: An approval for this Home Assistant domain is already in progress.";
const ERR_DECLINED: &str = "declined: The Home Assistant action was not approved.";

// ─── types (camelCase for the frontend) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HaEntity {
    pub entity_id: String,
    pub state: String,
    pub friendly_name: Option<String>,
    pub domain: String,
    pub last_changed: Option<String>,
    /// HA attributes minus anything token-like (see `sanitize_attributes`).
    pub attributes: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HaConversationResult {
    Match { speech: String },
    NoMatch,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HaContainerStatus {
    /// false when the container doesn't exist or isn't running.
    pub running: bool,
    /// Docker's status line, e.g. "Up 3 hours" or "Exited (0) 2 minutes ago"; empty if absent.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HaStateChanged {
    pub entity_id: String,
    pub new_state: Option<HaEntity>,
    pub old_state: Option<HaEntity>,
    pub time_fired: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HaLiveStatus {
    /// "connecting" | "connected" | "authFailed" | "disconnected" | "stopped"
    pub state: &'static str,
}

/// Optional typed data for `climate.set_temperature` / `climate.set_hvac_mode` only.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HaServiceData {
    pub temperature: Option<f64>,
    pub hvac_mode: Option<String>,
}

/// What `ha_call_service` may do with a domain/service pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    Allowed,
    NeedsApproval,
}

/// The HA token. No `Serialize`, no `Display`; `Debug` is redacted.
#[derive(Clone)]
struct HaToken(String);

impl std::fmt::Debug for HaToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HaToken(<redacted>)")
    }
}

impl Drop for HaToken {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

/// File text that may contain the token; zeroized on drop. Redacted `Debug` (the inner
/// `Zeroizing` would print its contents).
struct SecretText(zeroize::Zeroizing<String>);

impl SecretText {
    fn with_capacity(n: usize) -> Self {
        SecretText(zeroize::Zeroizing::new(String::with_capacity(n)))
    }
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

#[derive(Debug, Clone)]
struct HaConfig {
    base: String,
    token: HaToken,
}

/// Logs fixed text only: `&'static str` means no runtime value (token, body) can be logged.
fn log(msg: &'static str) {
    eprintln!("[ha] {msg}");
}

// ─── config ──────────────────────────────────────────────────────────────────

fn voice_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("projects/amara-voice"))
}

fn compose_file() -> Option<PathBuf> {
    voice_dir().map(|d| d.join("homeassistant/docker-compose.yml"))
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Same rule as llm_router's private `loopback_base` (copied, not shared, because llm_router.rs
/// is Integrations-owned): `http://<loopback host>[:port]`, rebuilt from the parsed URL; userinfo,
/// path, query and fragment refused.
fn loopback_base(raw: &str) -> Option<String> {
    let u = reqwest::Url::parse(raw.trim()).ok()?;
    let host = u.host_str()?.to_string();
    let loopback = host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        return None;
    }
    let clean = u.scheme() == "http"
        && u.username().is_empty()
        && u.password().is_none()
        && matches!(u.path(), "" | "/")
        && u.query().is_none()
        && u.fragment().is_none();
    clean.then(|| match u.port() {
        Some(port) => format!("http://{host}:{port}"),
        None => format!("http://{host}"),
    })
}

/// Reads only `HA_URL` and `HA_TOKEN` from `file` inside `dir`. The file is opened once with
/// O_NOFOLLOW|O_NONBLOCK and must be a regular file owned by `expected_uid`, mode without
/// group/other bits, at most 16 KiB. Errors are fixed text.
fn read_config_at(dir: &Path, file: &Path, expected_uid: u32) -> Result<HaConfig, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let cfg_err = || ERR_CONFIG.to_string();
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(cfg_err)?;
    let inside = crate::commands::path_safety::safe_join(dir, name).map_err(|_| cfg_err())?;
    let parent = file
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .ok_or_else(cfg_err)?;
    if parent.join(name) != inside {
        return Err(cfg_err());
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&inside)
        .map_err(|_| cfg_err())?;
    let meta = f.metadata().map_err(|_| cfg_err())?;
    if !meta.is_file() || meta.uid() != expected_uid || meta.mode() & 0o077 != 0 {
        return Err(cfg_err());
    }
    let text = read_secret(&mut f, meta.len(), ENV_MAX_BYTES).map_err(|_| cfg_err())?;
    parse_config(text.as_str())
}

/// `KEY=value` lines; only `HA_URL` and `HA_TOKEN` are looked at (last one wins).
fn parse_config(text: &str) -> Result<HaConfig, String> {
    let mut url = None;
    let mut token = None;
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
        match k.trim() {
            "HA_URL" => url = Some(v.to_string()),
            "HA_TOKEN" => token = Some(v.to_string()),
            _ => {}
        }
    }
    let url = url.filter(|u| !u.is_empty());
    let base = loopback_base(url.as_deref().unwrap_or(DEFAULT_HA_URL))
        .ok_or_else(|| ERR_URL.to_string())?;
    let token = token
        .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_graphic()))
        .ok_or_else(|| ERR_CONFIG.to_string())?;
    Ok(HaConfig {
        base,
        token: HaToken(token),
    })
}

fn load_config() -> Result<HaConfig, String> {
    resolve(&Sources::live())
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

// ─── validation ──────────────────────────────────────────────────────────────

fn is_slug(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `^[a-z0-9_]{1,64}$`
fn valid_name(s: &str) -> bool {
    is_slug(s, 64)
}

/// `^[a-z0-9_]+\.[a-z0-9_]{1,128}$` (domain part capped at 64 too).
fn valid_entity_id(s: &str) -> bool {
    match s.split_once('.') {
        Some((d, o)) => is_slug(d, 64) && is_slug(o, 128),
        None => false,
    }
}

// ─── sanitizing ──────────────────────────────────────────────────────────────

/// Attribute keys dropped (any depth): HA puts signed `?token=` URLs in `entity_picture`
/// and cameras expose `access_token`.
const SENSITIVE_DOMAINS: &[&str] = &[
    "lock",
    "alarm_control_panel",
    "cover",
    "script",
    "shell_command",
    "python_script",
    "hassio",
    "homeassistant",
    "automation",
];

/// Allowlist check. Everything not listed is refused; sensitive domains need a native confirm.
/// Returns the JSON body to POST.
fn authorize(
    domain: &str,
    service: &str,
    entity_id: &str,
    data: Option<&HaServiceData>,
) -> Result<(Policy, Value), String> {
    if !valid_name(domain) || !valid_name(service) || !valid_entity_id(entity_id) {
        return Err(ERR_INVALID.to_string());
    }
    if entity_id.split('.').next() != Some(domain) {
        return Err(ERR_DOMAIN_MISMATCH.to_string());
    }
    let mut body = Map::new();
    body.insert("entity_id".into(), Value::String(entity_id.into()));
    let policy = match (domain, service) {
        ("light" | "switch" | "fan" | "media_player", "turn_on" | "turn_off" | "toggle")
        | ("scene", "turn_on")
        | ("climate", "turn_on" | "turn_off") => Policy::Allowed,
        ("climate", "set_temperature") => {
            let t = data
                .filter(|d| d.hvac_mode.is_none())
                .and_then(|d| d.temperature)
                .filter(|t| t.is_finite() && (MIN_TEMPERATURE..=MAX_TEMPERATURE).contains(t))
                .ok_or_else(|| ERR_BAD_DATA.to_string())?;
            body.insert("temperature".into(), json!(t));
            return Ok((Policy::Allowed, Value::Object(body)));
        }
        ("climate", "set_hvac_mode") => {
            let m = data
                .filter(|d| d.temperature.is_none())
                .and_then(|d| d.hvac_mode.as_deref())
                .filter(|m| HVAC_MODES.contains(m))
                .ok_or_else(|| ERR_BAD_DATA.to_string())?;
            body.insert("hvac_mode".into(), json!(m));
            return Ok((Policy::Allowed, Value::Object(body)));
        }
        (d, _) if SENSITIVE_DOMAINS.contains(&d) => Policy::NeedsApproval,
        _ => return Err(ERR_NOT_ALLOWED.to_string()),
    };
    if data.is_some() {
        return Err(ERR_BAD_DATA.to_string());
    }
    Ok((policy, Value::Object(body)))
}

/// Read a JSON body, refusing anything over `cap` bytes (Content-Length first, then streamed).
async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Value, String> {
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(ERR_TOO_LARGE.to_string());
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| ERR_BAD_REPLY.to_string())? {
        if buf.len() + chunk.len() > cap {
            return Err(ERR_TOO_LARGE.to_string());
        }
        buf.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&buf).map_err(|_| ERR_BAD_REPLY.to_string())
}

fn sensitive_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "token",
        "password",
        "secret",
        "api_key",
        "apikey",
        "auth",
        "entity_picture",
    ]
    .iter()
    .any(|s| k.contains(s))
}

fn sanitize_attributes(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !sensitive_key(k))
                .map(|(k, v)| (k.clone(), sanitize_attributes(v)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(sanitize_attributes).collect()),
        Value::String(s) if s.contains("token=") => Value::String(String::new()),
        other => other.clone(),
    }
}

fn entity_from(v: &Value) -> Option<HaEntity> {
    let entity_id = v["entity_id"].as_str()?.to_string();
    if !valid_entity_id(&entity_id) {
        return None;
    }
    let domain = entity_id.split('.').next().unwrap_or("").to_string();
    Some(HaEntity {
        state: v["state"].as_str().unwrap_or("").to_string(),
        friendly_name: v["attributes"]["friendly_name"]
            .as_str()
            .map(str::to_string),
        domain,
        last_changed: v["last_changed"].as_str().map(str::to_string),
        attributes: sanitize_attributes(&v["attributes"]),
        entity_id,
    })
}

fn state_changed_from(event: &Value) -> Option<HaStateChanged> {
    let data = &event["data"];
    let entity_id = data["entity_id"].as_str()?.to_string();
    if !valid_entity_id(&entity_id) {
        return None;
    }
    Some(HaStateChanged {
        entity_id,
        new_state: entity_from(&data["new_state"]),
        old_state: entity_from(&data["old_state"]),
        time_fired: event["time_fired"].as_str().map(str::to_string),
    })
}

fn status_error(status: reqwest::StatusCode) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        ERR_AUTH.to_string()
    } else {
        ERR_FAILED.to_string()
    }
}

// ─── HTTP calls (pure over config, tested against a mock) ────────────────────

async fn states_with(cfg: &HaConfig) -> Result<Vec<HaEntity>, String> {
    let resp = client()
        .get(format!("{}/api/states", cfg.base))
        .bearer_auth(&cfg.token.0)
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    let v = read_capped(resp, MAX_BODY_BYTES).await?;
    let list = v.as_array().ok_or_else(|| ERR_BAD_REPLY.to_string())?;
    Ok(list
        .iter()
        .take(MAX_STATES)
        .filter_map(entity_from)
        .collect())
}

async fn call_service_with(
    cfg: &HaConfig,
    domain: &str,
    service: &str,
    body: &Value,
) -> Result<(), String> {
    if !valid_name(domain) || !valid_name(service) {
        return Err(ERR_INVALID.to_string());
    }
    let resp = client()
        .post(format!("{}/api/services/{domain}/{service}", cfg.base))
        .bearer_auth(&cfg.token.0)
        .json(body)
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    Ok(())
}

async fn conversation_with(cfg: &HaConfig, text: &str) -> Result<HaConversationResult, String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_CONVERSATION_CHARS {
        return Err(ERR_TEXT.to_string());
    }
    let resp = client()
        .post(format!("{}/api/conversation/process", cfg.base))
        .bearer_auth(&cfg.token.0)
        .json(&json!({
            "text": text,
            "language": "en",
            "agent_id": "conversation.home_assistant",
        }))
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    let v = read_capped(resp, MAX_BODY_BYTES).await?;
    conversation_result(&v)
}

fn conversation_result(v: &Value) -> Result<HaConversationResult, String> {
    let r = &v["response"];
    // Only a handled intent is a match; "error" (any code) or unknown/missing is noMatch.
    if !matches!(
        r["response_type"].as_str(),
        Some("action_done" | "query_answer")
    ) {
        return Ok(HaConversationResult::NoMatch);
    }
    let speech = r["speech"]["plain"]["speech"]
        .as_str()
        .ok_or_else(|| ERR_BAD_REPLY.to_string())?;
    Ok(HaConversationResult::Match {
        speech: speech.to_string(),
    })
}

// ─── commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn ha_states() -> Result<Vec<HaEntity>, String> {
    states_with(&load_config()?).await
}

/// Map approval_gate's errors onto our fixed kinds (its text is not passed through).
fn approval_error(e: &str) -> String {
    if e.starts_with(super::approval_gate::DECLINED_PREFIX) {
        ERR_DECLINED.to_string()
    } else if e.starts_with(super::approval_gate::BUSY_PREFIX) {
        ERR_BUSY_APPROVAL.to_string()
    } else {
        ERR_NEEDS_APPROVAL.to_string()
    }
}

#[tauri::command]
pub async fn ha_call_service(
    app: AppHandle,
    domain: String,
    service: String,
    entity_id: String,
    data: Option<HaServiceData>,
) -> Result<(), String> {
    let (policy, body) = authorize(&domain, &service, &entity_id, data.as_ref())?;
    let _guard = if policy == Policy::NeedsApproval {
        // Validated slugs only, so the dialog text is safe to show.
        let text = super::approval_gate::approval_text(
            &format!("Home Assistant: {domain}.{service}"),
            &format!("Entity: {entity_id}\n\nThis is a sensitive Home Assistant action."),
        )
        .map_err(|_| ERR_NEEDS_APPROVAL.to_string())?;
        let key = super::approval_gate::service_key(&format!("ha:{domain}"));
        Some(
            super::approval_gate::require_approval(&app, &[key], text)
                .await
                .map_err(|e| approval_error(&e))?,
        )
    } else {
        None
    };
    call_service_with(&load_config()?, &domain, &service, &body).await
}

#[tauri::command]
pub async fn ha_conversation(text: String) -> Result<HaConversationResult, String> {
    conversation_with(&load_config()?, &text).await
}

// ─── container ───────────────────────────────────────────────────────────────

static DOCKER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// GUI apps on macOS don't inherit the shell PATH, so docker is looked up at fixed paths.
pub(super) fn docker_bin() -> Option<&'static str> {
    [
        "/usr/local/bin/docker",
        "/opt/homebrew/bin/docker",
        "/Applications/Docker.app/Contents/Resources/bin/docker",
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
}

async fn run_docker(args: &[&str]) -> Result<String, String> {
    let bin = docker_bin().ok_or_else(|| ERR_DOCKER.to_string())?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(DOCKER_TIMEOUT, cmd.output())
        .await
        .map_err(|_| ERR_DOCKER_TIMEOUT.to_string())?
        .map_err(|_| ERR_DOCKER.to_string())?;
    if !out.status.success() {
        return Err(ERR_DOCKER_FAILED.to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_container_status(stdout: &str) -> HaContainerStatus {
    let status: String = stdout
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    HaContainerStatus {
        running: status.starts_with("Up"),
        status,
    }
}

async fn container_status() -> Result<HaContainerStatus, String> {
    let out = run_docker(&[
        "ps",
        "--all",
        "--filter",
        "name=^homeassistant$",
        "--format",
        "{{.Status}}",
    ])
    .await?;
    Ok(parse_container_status(&out))
}

#[tauri::command]
pub async fn ha_container_status() -> Result<HaContainerStatus, String> {
    container_status().await
}

async fn compose(action: &[&str]) -> Result<HaContainerStatus, String> {
    let _guard = DOCKER_LOCK.try_lock().map_err(|_| ERR_BUSY.to_string())?;
    let file = compose_file().ok_or_else(|| ERR_DOCKER.to_string())?;
    let file = file.to_str().ok_or_else(|| ERR_DOCKER.to_string())?;
    let mut args = vec!["compose", "-f", file];
    args.extend_from_slice(action);
    run_docker(&args).await?;
    container_status().await
}

/// `docker compose -f ~/projects/amara-voice/homeassistant/docker-compose.yml up -d`.
/// No approval dialog (per the brief); see the report for routing it through approval_gate.
#[tauri::command]
pub async fn ha_container_start() -> Result<HaContainerStatus, String> {
    compose(&["up", "-d"]).await
}

/// `docker compose -f … stop`.
#[tauri::command]
pub async fn ha_container_stop() -> Result<HaContainerStatus, String> {
    compose(&["stop"]).await
}

// ─── live websocket ──────────────────────────────────────────────────────────

static LIVE_TASK: StdMutex<Option<tokio::task::JoinHandle<()>>> = StdMutex::new(None);

#[derive(Debug, PartialEq)]
enum SessionEnd {
    AuthFailed,
    Closed,
}

fn ws_url(base: &str) -> String {
    format!("{}/api/websocket", base.replacen("http://", "ws://", 1))
}

/// One websocket session: auth_required → auth → auth_ok → subscribe state_changed, then
/// forward every event to `on_change` until the socket closes. `on_connected` fires on auth_ok.
async fn live_session<S>(
    mut ws: S,
    token: &HaToken,
    t: WsTimings,
    mut on_connected: impl FnMut(),
    mut on_change: impl FnMut(HaStateChanged),
) -> SessionEnd
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures::Sink<Message>
        + Unpin,
{
    /// Next text frame as JSON; non-text frames (pings/pongs) are skipped. None on close/error.
    async fn next_json<S>(ws: &mut S) -> Option<Value>
    where
        S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            match ws.next().await? {
                Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => continue,
            }
        }
    }
    // Auth phase (auth_required → auth → auth_ok) is bounded as a whole.
    let auth = async {
        match next_json(&mut ws).await {
            Some(v) if v["type"] == "auth_required" => {}
            _ => return Err(SessionEnd::Closed),
        }
        let auth = json!({"type": "auth", "access_token": token.0}).to_string();
        if ws.send(Message::Text(auth.into())).await.is_err() {
            return Err(SessionEnd::Closed);
        }
        match next_json(&mut ws).await {
            Some(v) if v["type"] == "auth_ok" => Ok(()),
            Some(v) if v["type"] == "auth_invalid" => Err(SessionEnd::AuthFailed),
            _ => Err(SessionEnd::Closed),
        }
    };
    match tokio::time::timeout(t.auth, auth).await {
        Ok(Ok(())) => {}
        Ok(Err(end)) => return end,
        Err(_) => return SessionEnd::Closed,
    }
    let sub = json!({"id": 1, "type": "subscribe_events", "event_type": "state_changed"});
    if ws
        .send(Message::Text(sub.to_string().into()))
        .await
        .is_err()
    {
        return SessionEnd::Closed;
    }
    on_connected();
    // Ping every `t.ping`; any received frame (pongs included) resets the `t.idle` deadline.
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + t.ping, t.ping);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut deadline = tokio::time::Instant::now() + t.idle;
    loop {
        tokio::select! {
            frame = ws.next() => {
                deadline = tokio::time::Instant::now() + t.idle;
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                        if v["type"] == "event" && v["event"]["event_type"] == "state_changed" {
                            if let Some(change) = state_changed_from(&v["event"]) {
                                on_change(change);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return SessionEnd::Closed,
                    Some(Ok(_)) => {}
                }
            }
            _ = ping.tick() => {
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return SessionEnd::Closed;
                }
            }
            _ = tokio::time::sleep_until(deadline) => return SessionEnd::Closed,
        }
    }
}

/// Websocket timings; production uses `WsTimings::default()`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WsTimings {
    auth: Duration,
    ping: Duration,
    idle: Duration,
}

impl Default for WsTimings {
    fn default() -> Self {
        Self {
            auth: WS_AUTH_TIMEOUT,
            ping: WS_PING_INTERVAL,
            idle: WS_IDLE_TIMEOUT,
        }
    }
}

fn ws_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(WS_MAX_MESSAGE))
        .max_frame_size(Some(WS_MAX_FRAME))
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect + handshake with size limits, bounded by `limit`.
async fn connect_ws(url: &str, limit: Duration) -> Option<WsStream> {
    match tokio::time::timeout(
        limit,
        tokio_tungstenite::connect_async_with_config(url, Some(ws_config()), false),
    )
    .await
    {
        Ok(Ok((ws, _))) => Some(ws),
        _ => None,
    }
}

fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(BACKOFF_CAP)
}

async fn live_loop(app: AppHandle) {
    let status = |state: &'static str| {
        let _ = app.emit(LIVE_STATUS_EVENT, HaLiveStatus { state });
    };
    let mut backoff = BACKOFF_START;
    loop {
        status("connecting");
        // Re-read the config every attempt so a rotated token or new URL is picked up.
        match load_config() {
            Ok(cfg) => match connect_ws(&ws_url(&cfg.base), WS_CONNECT_TIMEOUT).await {
                Some(ws) => {
                    let mut connected = false;
                    let end = live_session(
                        ws,
                        &cfg.token,
                        WsTimings::default(),
                        || {
                            connected = true;
                            status("connected");
                        },
                        |change| {
                            let _ = app.emit(STATE_CHANGED_EVENT, change);
                        },
                    )
                    .await;
                    if connected {
                        backoff = BACKOFF_START;
                    }
                    if end == SessionEnd::AuthFailed {
                        log("websocket auth failed");
                        status("authFailed");
                        backoff = BACKOFF_CAP;
                    } else {
                        status("disconnected");
                    }
                }
                None => {
                    log("websocket connect failed or timed out");
                    status("disconnected");
                }
            },
            Err(_) => {
                log("config unavailable for websocket");
                status("disconnected");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

/// Starts the live `ha://state_changed` stream (async so it runs inside the Tokio runtime). Idempotent: returns false if already running.
#[tauri::command]
pub async fn ha_live_start(app: AppHandle) -> bool {
    let mut slot = LIVE_TASK.lock().unwrap_or_else(|p| p.into_inner());
    if slot.as_ref().is_some_and(|h| !h.is_finished()) {
        return false;
    }
    *slot = Some(tokio::spawn(live_loop(app)));
    true
}

/// Stops the live stream. Returns false if it wasn't running.
#[tauri::command]
pub async fn ha_live_stop(app: AppHandle) -> bool {
    let task = LIVE_TASK.lock().unwrap_or_else(|p| p.into_inner()).take();
    match task {
        Some(h) => {
            h.abort();
            let _ = app.emit(LIVE_STATUS_EVENT, HaLiveStatus { state: "stopped" });
            true
        }
        None => false,
    }
}

// ─── settings: sources, precedence, app-data .env ────────────────────────────

const APP_ENV_FILE: &str = ".env";
const APP_ENV_MAX_BYTES: u64 = 64 * 1024;
const TOKEN_KEY: &str = "HA_TOKEN";
const TOKEN_MIN: usize = 32;
const TOKEN_MAX: usize = 512;
const VERSION_MAX: usize = 32;
const TEST_BODY_MAX: usize = 64 * 1024;
const TEST_MIN_GAP: Duration = Duration::from_secs(2);

const ERR_STORAGE: &str =
    "unsafe_storage: The app settings file is not safe to use (symlink, owner or permissions).";
const ERR_TOKEN_FORMAT: &str =
    "invalid_input: The token must be 32-512 characters of letters, digits, '.', '_' or '-'.";
const ERR_NOTHING: &str = "invalid_input: Nothing to save.";
const ERR_SAVE: &str = "storage_error: The settings could not be saved.";

/// Everything `resolve` looks at, so tests can point it at temp dirs.
struct Sources {
    /// Folder holding the app-data `.env` (`app_paths::data_dir()` live).
    app_dir: Option<PathBuf>,
    /// Folder holding the legacy amara-voice `.env` (read-only fallback).
    legacy_dir: Option<PathBuf>,
    uid: u32,
    cfg_url: Option<String>,
    env_url: Option<String>,
    env_token: Option<HaToken>,
}

impl Sources {
    fn live() -> Self {
        Sources {
            app_dir: Some(app_env_dir()),
            legacy_dir: voice_dir(),
            uid: current_uid(),
            cfg_url: super::telegram::load_config().ha_url,
            env_url: std::env::var("HA_URL").ok(),
            env_token: std::env::var(TOKEN_KEY).ok().map(HaToken),
        }
    }
}

fn token_ok_for_use(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_graphic())
}

/// Strict format for a token we store: `[A-Za-z0-9._-]{32,512}`. This excludes CR, LF, NUL,
/// '=', '#', quotes and spaces, so a stored value can never add or split an env line.
fn valid_new_token(t: &str) -> bool {
    (TOKEN_MIN..=TOKEN_MAX).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// Reads an already-checked file into a buffer pre-sized to `min(len, max) + 1` (so
/// `read_to_end` does not reallocate and leave copies), zeroizing it on every error path.
fn read_secret(f: &mut std::fs::File, len: u64, max: u64) -> Result<SecretText, ()> {
    use std::io::Read;
    use zeroize::{Zeroize, Zeroizing};
    let mut buf = Zeroizing::new(Vec::with_capacity(len.min(max) as usize + 1));
    f.take(max + 1).read_to_end(&mut buf).map_err(|_| ())?;
    if buf.len() as u64 > max {
        return Err(()); // `buf` zeroized on drop
    }
    match String::from_utf8(std::mem::take(&mut *buf)) {
        Ok(s) => Ok(SecretText(Zeroizing::new(s))),
        Err(e) => {
            e.into_bytes().zeroize();
            Err(())
        }
    }
}

/// Opens `dir/name` with O_NOFOLLOW|O_NONBLOCK and checks the open handle: regular file, owned by
/// `uid`, no `mode_mask` bits, at most `max` bytes. `Ok(None)` only when the file is absent.
fn read_private_text(
    dir: &Path,
    name: &str,
    uid: u32,
    max: u64,
    mode_mask: u32,
) -> Result<Option<SecretText>, ()> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let path = dir.join(name);
    let mut f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let meta = f.metadata().map_err(|_| ())?;
    if !meta.is_file() || meta.uid() != uid || meta.mode() & mode_mask != 0 {
        return Err(());
    }
    read_secret(&mut f, meta.len(), max).map(Some)
}

/// Token from the app-data `.env`. `Err` = the file exists but is unsafe (fail closed).
fn app_env_token(dir: &Path, uid: u32) -> Result<Option<HaToken>, String> {
    let Some(text) = read_private_text(dir, APP_ENV_FILE, uid, APP_ENV_MAX_BYTES, 0o077)
        .map_err(|_| ERR_STORAGE.to_string())?
    else {
        return Ok(None);
    };
    Ok(
        super::terrence_runtime_bridge::env_value(text.as_str(), TOKEN_KEY)
            .map(HaToken)
            .filter(|t| token_ok_for_use(&t.0)),
    )
}

/// `(HA_URL, HA_TOKEN)` from the legacy amara-voice `.env`, via the same checked reader as
/// before (`read_config_at`). Absent, unsafe or invalid (bad URL / no token) = none.
fn legacy_values(src: &Sources) -> (Option<String>, Option<HaToken>) {
    let Some(dir) = src.legacy_dir.as_deref() else {
        return (None, None);
    };
    match read_config_at(dir, &dir.join(".env"), src.uid) {
        Ok(HaConfig { base, token }) => (Some(base), Some(token)),
        Err(_) => (None, None),
    }
}

fn non_empty(v: &Option<String>) -> Option<String> {
    v.as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string)
}

/// The URL a source explicitly sets (config, env, legacy), or `None` for the default.
fn configured_url(src: &Sources, legacy_url: &Option<String>) -> Option<String> {
    non_empty(&src.cfg_url)
        .or_else(|| non_empty(&src.env_url))
        .or_else(|| non_empty(legacy_url))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenSource {
    App,
    Env,
    Legacy,
}

fn resolve_token_with_source(
    src: &Sources,
    legacy: Option<HaToken>,
) -> Result<(HaToken, TokenSource), String> {
    if let Some(dir) = src.app_dir.as_deref() {
        if let Some(t) = app_env_token(dir, src.uid)? {
            return Ok((t, TokenSource::App));
        }
    }
    if let Some(t) = src.env_token.as_ref().filter(|t| token_ok_for_use(&t.0)) {
        return Ok((t.clone(), TokenSource::Env));
    }
    legacy
        .filter(|t| token_ok_for_use(&t.0))
        .map(|t| (t, TokenSource::Legacy))
        .ok_or_else(|| ERR_CONFIG.to_string())
}

fn resolve_token(src: &Sources, legacy: Option<HaToken>) -> Result<HaToken, String> {
    resolve_token_with_source(src, legacy).map(|(t, _)| t)
}

/// Precedence documented at the top of the file.
fn resolve(src: &Sources) -> Result<HaConfig, String> {
    let (legacy_url, legacy_token) = legacy_values(src);
    let url = configured_url(src, &legacy_url);
    let base = loopback_base(url.as_deref().unwrap_or(DEFAULT_HA_URL))
        .ok_or_else(|| ERR_URL.to_string())?;
    let token = resolve_token(src, legacy_token)?;
    Ok(HaConfig { base, token })
}

/// True when `line` assigns `key` (`KEY=…` or `export KEY=…`).
fn is_key_line(line: &str, key: &str) -> bool {
    let l = line.trim_start();
    let l = l.strip_prefix("export ").unwrap_or(l);
    l.split_once('=').is_some_and(|(k, _)| k.trim() == key)
}

/// Line-by-line merge: every other line keeps its exact bytes and order. The first `key` line
/// is replaced (or dropped when `value` is `None`); later duplicates are dropped; a missing key
/// is appended.
fn merge_env(text: &str, key: &str, value: Option<&str>) -> SecretText {
    let mut out = SecretText::with_capacity(text.len() + TOKEN_MAX + 16);
    let mut done = false;
    for seg in text.split_inclusive('\n') {
        if is_key_line(seg, key) {
            if !done {
                if let Some(v) = value {
                    out.0.push_str(key);
                    out.0.push('=');
                    out.0.push_str(v);
                    if seg.ends_with("\r\n") {
                        out.0.push_str("\r\n");
                    } else if seg.ends_with('\n') {
                        out.0.push('\n');
                    }
                }
            }
            done = true;
            continue;
        }
        out.0.push_str(seg);
    }
    if !done {
        if let Some(v) = value {
            if !out.0.is_empty() && !out.0.ends_with('\n') {
                out.0.push('\n');
            }
            out.0.push_str(key);
            out.0.push('=');
            out.0.push_str(v);
            out.0.push('\n');
        }
    }
    out
}

/// Atomically sets (or removes, `value: None`) one key in `dir/.env`.
///
/// The folder is opened O_DIRECTORY|O_NOFOLLOW and checked on the handle (directory, owned by
/// `uid`, not group/world-writable, same inode as the path). The existing file is opened
/// O_NOFOLLOW and checked on the handle (regular file, owned by `uid`, not group/world-writable).
/// The new content goes to a temp file in the same folder (O_CREAT|O_EXCL|O_NOFOLLOW, 0600),
/// is fsynced, renamed over the target, and the folder is fsynced. Errors carry no value.
static ENV_WRITE_LOCK: StdMutex<()> = StdMutex::new(());

fn write_env_key(dir: &Path, uid: u32, key: &str, value: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    // One writer at a time, so a save and a clear can't interleave read-merge-rename.
    let _guard = ENV_WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let unsafe_err = || ERR_STORAGE.to_string();
    let dir_fd = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(dir)
        .map_err(|_| unsafe_err())?;
    let dm = dir_fd.metadata().map_err(|_| unsafe_err())?;
    if !dm.is_dir() || dm.uid() != uid || dm.mode() & 0o022 != 0 {
        return Err(unsafe_err());
    }
    let lm = std::fs::symlink_metadata(dir).map_err(|_| unsafe_err())?;
    if lm.file_type().is_symlink() || lm.ino() != dm.ino() || lm.dev() != dm.dev() {
        return Err(unsafe_err());
    }
    let target = dir.join(APP_ENV_FILE);
    let existing = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&target)
    {
        Ok(mut f) => {
            let m = f.metadata().map_err(|_| unsafe_err())?;
            // Same rule as the reader: no group/other bits at all.
            if !m.is_file() || m.uid() != uid || m.mode() & 0o077 != 0 {
                return Err(unsafe_err());
            }
            Some(read_secret(&mut f, m.len(), APP_ENV_MAX_BYTES).map_err(|_| unsafe_err())?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unsafe_err()),
    };
    if existing.is_none() && value.is_none() {
        return Ok(()); // nothing to remove
    }
    let merged = merge_env(existing.as_ref().map_or("", |t| t.as_str()), key, value);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".env.tmp-{}-{nanos}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_CREAT|O_EXCL
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        f.write_all(merged.as_str().as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &target)?;
        dir_fd.sync_all()
    };
    write().map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        ERR_SAVE.to_string()
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaSettingsView {
    url: Option<String>,
    token_set: bool,
    /// Where the token in use comes from; `null` when none is usable.
    token_source: Option<TokenSource>,
}

fn get_config_with(src: &Sources) -> HaSettingsView {
    let (legacy_url, legacy_token) = legacy_values(src);
    let source = resolve_token_with_source(src, legacy_token)
        .ok()
        .map(|(_, s)| s);
    HaSettingsView {
        url: configured_url(src, &legacy_url).and_then(|u| loopback_base(&u)),
        token_set: source.is_some(),
        token_source: source,
    }
}

/// Validates everything first, then writes the token, then saves the URL via `save_url`
/// (`Some("")` clears the saved URL).
fn set_config_with(
    app_dir: &Path,
    uid: u32,
    url: Option<String>,
    token: Option<HaToken>,
    save_url: impl FnOnce(Option<String>) -> Result<(), String>,
) -> Result<(), String> {
    if url.is_none() && token.is_none() {
        return Err(ERR_NOTHING.to_string());
    }
    let url = match url.as_deref().map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(u) => Some(Some(loopback_base(u).ok_or_else(|| ERR_URL.to_string())?)),
    };
    if let Some(t) = &token {
        if !valid_new_token(&t.0) {
            return Err(ERR_TOKEN_FORMAT.to_string());
        }
    }
    if let Some(t) = &token {
        write_env_key(app_dir, uid, TOKEN_KEY, Some(&t.0))?;
    }
    drop(token); // wiped
    if let Some(u) = url {
        save_url(u)?;
    }
    Ok(())
}

/// Folder holding the app-data `.env`: the app data dir (follows `NEXUS_DATA_DIR`, and
/// `NEXUS_CONFIG_DIR` in dev builds).
pub(crate) fn app_env_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn live_app_dir() -> Result<PathBuf, String> {
    let dir = app_env_dir();
    crate::app_paths::create_private_dir_all(&dir).map_err(|_| ERR_SAVE.to_string())?;
    Ok(dir)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct HaTestResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

fn fail(e: &str) -> HaTestResult {
    let k = e.split(':').next().unwrap_or("");
    HaTestResult {
        ok: false,
        kind: Some(
            ERROR_KINDS
                .iter()
                .copied()
                .find(|x| *x == k)
                .unwrap_or(KIND_UPSTREAM_ERROR),
        ),
        version: None,
    }
}

fn safe_version(v: &str) -> Option<String> {
    (!v.is_empty()
        && v.len() <= VERSION_MAX
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+')))
    .then(|| v.to_string())
}

async fn test_with(cfg: &HaConfig) -> HaTestResult {
    let r: Result<Option<String>, String> = async {
        let resp = client()
            .get(format!("{}/api/config", cfg.base))
            .bearer_auth(&cfg.token.0)
            .send()
            .await
            .map_err(|_| ERR_UNREACHABLE.to_string())?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status()));
        }
        let v = read_capped(resp, TEST_BODY_MAX).await?;
        if !v.is_object() {
            return Err(ERR_BAD_REPLY.to_string());
        }
        Ok(v.get("version")
            .and_then(Value::as_str)
            .and_then(safe_version))
    }
    .await;
    match r {
        Ok(version) => HaTestResult {
            ok: true,
            kind: None,
            version,
        },
        Err(e) => fail(&e),
    }
}

/// One test at a time, at least `TEST_MIN_GAP` between starts.
static TEST_GATE: tokio::sync::Mutex<Option<std::time::Instant>> =
    tokio::sync::Mutex::const_new(None);

fn gap_ok(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= TEST_MIN_GAP)
}

/// `{url, tokenSet}`; `url` is the effective configured loopback URL (null = default).
#[tauri::command]
pub fn ha_get_config() -> HaSettingsView {
    get_config_with(&Sources::live())
}

/// Saves the URL (app config) and/or the token (app-data `.env`). `url: ""` clears the saved
/// URL. Rejects with `bad_url`, `invalid_input`, `not_allowed` (unsafe file) or
/// `upstream_error` (write failed).
#[tauri::command]
pub fn ha_set_config(url: Option<String>, token: Option<String>) -> Result<(), String> {
    let token = token.map(HaToken);
    let dir = live_app_dir()?;
    set_config_with(&dir, current_uid(), url, token, |u| {
        super::telegram::update_config(|c| c.ha_url = u).map_err(|_| ERR_SAVE.to_string())
    })
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HaClearResult {
    /// True when a token is still usable after the clear (env var or legacy file).
    fallback_active: bool,
}

fn clear_token_with(src: &Sources) -> Result<HaClearResult, String> {
    let dir = src.app_dir.as_deref().ok_or_else(|| ERR_SAVE.to_string())?;
    write_env_key(dir, src.uid, TOKEN_KEY, None)?;
    let (_, legacy_token) = legacy_values(src);
    Ok(HaClearResult {
        fallback_active: resolve_token(src, legacy_token).is_ok(),
    })
}

/// Removes only the `HA_TOKEN` line from the app-data `.env` (same atomic write) and reports
/// whether an env-var or legacy token still applies.
#[tauri::command]
pub fn ha_clear_token() -> Result<HaClearResult, String> {
    live_app_dir()?;
    clear_token_with(&Sources::live())
}

/// GET `/api/config` with the resolved settings. Never rejects; failures are `{ok:false, kind}`.
#[tauri::command]
pub async fn ha_test_connection(url: Option<String>, token: Option<String>) -> HaTestResult {
    // Wrap at once so the typed token is zeroized on every path.
    let token = token.map(HaToken);
    let overrides = match test_overrides(url, token) {
        Ok(o) => o,
        Err(e) => return fail(e),
    };
    let Ok(mut last) = TEST_GATE.try_lock() else {
        return fail(ERR_BUSY);
    };
    let now = std::time::Instant::now();
    if !gap_ok(*last, now) {
        return fail(ERR_BUSY);
    }
    *last = Some(now);
    test_connection_with(&Sources::live(), overrides).await
}

/// Unsaved form values for a connection test; `None` = use the stored setting.
#[derive(Debug, Default)]
struct TestOverrides {
    base: Option<String>,
    token: Option<HaToken>,
}

/// Validates form values with `ha_set_config`'s rules. Blank fields count as not provided.
fn test_overrides(
    url: Option<String>,
    token: Option<HaToken>,
) -> Result<TestOverrides, &'static str> {
    let base = match url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(loopback_base(u).ok_or(ERR_URL)?),
        None => None,
    };
    let token = match token {
        Some(t) if t.0.trim().is_empty() => None,
        Some(t) => {
            if !valid_new_token(&t.0) {
                return Err(ERR_TOKEN_FORMAT);
            }
            Some(t)
        }
        None => None,
    };
    Ok(TestOverrides { base, token })
}

/// Stored config with `o` applied on top; nothing is written anywhere.
fn config_with_overrides(src: &Sources, o: TestOverrides) -> Result<HaConfig, String> {
    let (legacy_url, legacy_token) = legacy_values(src);
    let saved_base = loopback_base(
        configured_url(src, &legacy_url)
            .as_deref()
            .unwrap_or(DEFAULT_HA_URL),
    );
    // The saved token only ever goes to the saved base: an unsaved URL needs its own token,
    // or any IPC caller could send the real token to another loopback port.
    if o.token.is_none() && o.base.is_some() && o.base != saved_base {
        return Err(ERR_TOKEN_REQUIRED.to_string());
    }
    let base = match o.base {
        Some(b) => b,
        None => saved_base.ok_or_else(|| ERR_URL.to_string())?,
    };
    let token = match o.token {
        Some(t) => t,
        None => resolve_token(src, legacy_token)?,
    };
    Ok(HaConfig { base, token })
}

async fn test_connection_with(src: &Sources, o: TestOverrides) -> HaTestResult {
    match config_with_overrides(src, o) {
        Ok(cfg) => test_with(&cfg).await,
        Err(e) => fail(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TOKEN: &str = "TEST-HA-TOKEN-zz9-must-never-leak";

    fn cfg(base: &str) -> HaConfig {
        HaConfig {
            base: base.to_string(),
            token: HaToken(TOKEN.into()),
        }
    }

    /// Records every request (method, path, has-right-bearer, body) and answers by path.
    /// Error bodies echo the token to prove it never reaches our errors.
    type Seen = std::sync::Arc<StdMutex<Vec<(String, String, bool, String)>>>;
    async fn mock(routes: Vec<(&'static str, u16, String)>) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Seen = Default::default();
        let s2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                let seen = s2.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let mut n = 0;
                    // Read headers + body (content-length).
                    loop {
                        let r = sock.read(&mut buf[n..]).await.unwrap_or(0);
                        if r == 0 {
                            break;
                        }
                        n += r;
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        if let Some(i) = text.find("\r\n\r\n") {
                            let len = text
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if n >= i + 4 + len {
                                break;
                            }
                        }
                    }
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let mut first = req.split_whitespace();
                    let method = first.next().unwrap_or("").to_string();
                    let path = first.next().unwrap_or("/").to_string();
                    let bearer = req.contains(&format!("authorization: Bearer {TOKEN}"))
                        || req.contains(&format!("Authorization: Bearer {TOKEN}"));
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    seen.lock()
                        .unwrap()
                        .push((method, path.clone(), bearer, body));
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, s, b)| (*s, b.clone()))
                        .unwrap_or((404, format!("{{\"message\":\"no {TOKEN}\"}}")));
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn states_body() -> String {
        json!([
            {"entity_id": "light.kitchen", "state": "on", "last_changed": "2026-11-04T04:00:00Z",
             "attributes": {"friendly_name": "Kitchen", "brightness": 200,
                            "entity_picture": format!("/api/camera_proxy/x?token={TOKEN}"),
                            "nested": {"access_token": TOKEN, "ok": 1}}},
            {"entity_id": "camera.door", "state": "idle",
             "attributes": {"access_token": TOKEN, "friendly_name": "Door"}},
            {"entity_id": "BAD ID", "state": "x", "attributes": {}}
        ])
        .to_string()
    }

    #[tokio::test]
    async fn states_are_parsed_sanitized_and_carry_no_token() {
        let (base, seen) = mock(vec![("/api/states", 200, states_body())]).await;
        let list = states_with(&cfg(&base)).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].entity_id, "light.kitchen");
        assert_eq!(list[0].domain, "light");
        assert_eq!(list[0].friendly_name.as_deref(), Some("Kitchen"));
        assert_eq!(list[0].attributes["brightness"], 200);
        assert_eq!(list[0].attributes["nested"]["ok"], 1);
        let out = serde_json::to_string(&list).unwrap();
        assert!(!out.contains(TOKEN), "{out}");
        assert!(out.contains("\"entityId\"") && out.contains("\"lastChanged\""));
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "GET");
        assert!(seen[0].2, "bearer token sent");
    }

    #[tokio::test]
    async fn call_service_posts_entity_id_and_validates() {
        let (base, seen) = mock(vec![("/api/services/light/turn_on", 200, "[]".into())]).await;
        let (_, body) = authorize("light", "turn_on", "light.kitchen", None).unwrap();
        call_service_with(&cfg(&base), "light", "turn_on", &body)
            .await
            .unwrap();
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen[0].0, "POST");
            assert!(seen[0].2);
            let body: Value = serde_json::from_str(&seen[0].3).unwrap();
            assert_eq!(body, json!({"entity_id": "light.kitchen"}));
        }
        for (d, s, e) in [
            ("Light", "turn_on", "light.kitchen"),
            ("light/../x", "turn_on", "light.kitchen"),
            ("light", "turn on", "light.kitchen"),
            ("light", "turn_on", "light"),
            ("light", "turn_on", "light.kitchen;rm"),
            ("light", "turn_on", ".kitchen"),
            ("light", "turn_on", "light."),
            ("", "turn_on", "light.kitchen"),
        ] {
            let r = authorize(d, s, e, None);
            assert_eq!(r.unwrap_err(), ERR_INVALID, "{d} {s} {e}");
        }
        assert!(!valid_name(&"a".repeat(65)));
        assert!(valid_entity_id(&format!("a.{}", "b".repeat(128))));
        assert!(!valid_entity_id(&format!("a.{}", "b".repeat(129))));
        assert_eq!(seen.lock().unwrap().len(), 1, "rejected calls never hit HA");
    }

    #[tokio::test]
    async fn conversation_match_no_match_and_cap() {
        let matched = json!({"response": {"response_type": "action_done",
            "speech": {"plain": {"speech": "Turned on the light"}}}})
        .to_string();
        let (base, seen) = mock(vec![("/api/conversation/process", 200, matched)]).await;
        let r = conversation_with(&cfg(&base), "turn on the kitchen light")
            .await
            .unwrap();
        assert_eq!(
            r,
            HaConversationResult::Match {
                speech: "Turned on the light".into()
            }
        );
        let body: Value = serde_json::from_str(&seen.lock().unwrap()[0].3).unwrap();
        assert_eq!(body["language"], "en");
        assert_eq!(body["agent_id"], "conversation.home_assistant");
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"kind": "match", "speech": "Turned on the light"})
        );

        let nomatch = json!({"response": {"response_type": "error",
            "data": {"code": "no_intent_match"},
            "speech": {"plain": {"speech": "Sorry, I couldn't understand that"}}}})
        .to_string();
        let (base2, _) = mock(vec![("/api/conversation/process", 200, nomatch)]).await;
        let r = conversation_with(&cfg(&base2), "sing").await.unwrap();
        assert_eq!(r, HaConversationResult::NoMatch);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"kind": "noMatch"})
        );

        let long = "a".repeat(MAX_CONVERSATION_CHARS + 1);
        assert_eq!(
            conversation_with(&cfg(&base), &long).await.unwrap_err(),
            ERR_TEXT
        );
        assert_eq!(
            conversation_with(&cfg(&base), "  ").await.unwrap_err(),
            ERR_TEXT
        );
    }

    #[tokio::test]
    async fn errors_are_fixed_text_and_never_echo_the_token() {
        let (base, _) = mock(vec![
            (
                "/api/states",
                401,
                format!("{{\"message\":\"bad {TOKEN}\"}}"),
            ),
            (
                "/api/services/light/turn_on",
                500,
                format!("{{\"message\":\"{TOKEN}\"}}"),
            ),
            ("/api/conversation/process", 200, format!("\"{TOKEN}\"")),
        ])
        .await;
        let c = cfg(&base);
        let errs = [
            states_with(&c).await.unwrap_err(),
            call_service_with(&c, "light", "turn_on", &json!({"entity_id": "light.x"}))
                .await
                .unwrap_err(),
        ];
        assert_eq!(errs[0], ERR_AUTH);
        assert_eq!(errs[1], ERR_FAILED);
        // A token-bearing reply with no response_type is noMatch and carries nothing back.
        let conv = conversation_with(&c, "hi").await.unwrap();
        assert_eq!(conv, HaConversationResult::NoMatch);
        assert!(!serde_json::to_string(&conv).unwrap().contains(TOKEN));
        for e in &errs {
            assert!(!e.contains(TOKEN));
        }
        // Nothing listening.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let down = cfg(&format!("http://127.0.0.1:{port}"));
        assert_eq!(states_with(&down).await.unwrap_err(), ERR_UNREACHABLE);
        assert!(!format!("{c:?}").contains(TOKEN), "Debug is redacted");
    }

    #[test]
    fn url_must_be_loopback_and_only_two_keys_are_read() {
        let ok = parse_config(&format!(
            "HERMES_KEY=zzz\nexport HA_URL=\"http://localhost:8123\"\nHA_TOKEN='{TOKEN}'\n"
        ))
        .unwrap();
        assert_eq!(ok.base, "http://localhost:8123");
        assert_eq!(ok.token.0, TOKEN);
        let default = parse_config(&format!("HA_TOKEN={TOKEN}")).unwrap();
        assert_eq!(default.base, DEFAULT_HA_URL);
        for bad in [
            "http://192.168.1.10:8123",
            "http://homeassistant.local:8123",
            "https://127.0.0.1:8123",
            "http://127.0.0.1@evil.com:8123",
            "http://127.0.0.1:8123/api",
            "ws://127.0.0.1:8123",
        ] {
            let e = parse_config(&format!("HA_URL={bad}\nHA_TOKEN={TOKEN}")).unwrap_err();
            assert_eq!(e, ERR_URL, "{bad}");
        }
        assert_eq!(
            parse_config("HA_URL=http://127.0.0.1:8123").unwrap_err(),
            ERR_CONFIG
        );
        assert_eq!(
            parse_config("HA_TOKEN=has space\n").unwrap_err(),
            ERR_CONFIG
        );
    }

    #[test]
    fn env_file_checks_mode_owner_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("nexus-ha-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let f = dir.join(".env");
        std::fs::write(&f, format!("HA_TOKEN={TOKEN}\n")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        let uid = current_uid();
        assert_eq!(read_config_at(&dir, &f, uid).unwrap_err(), ERR_CONFIG);
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_config_at(&dir, &f, uid).is_ok());
        assert_eq!(read_config_at(&dir, &f, uid + 1).unwrap_err(), ERR_CONFIG);
        let link = dir.join("link.env");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        assert_eq!(read_config_at(&dir, &link, uid).unwrap_err(), ERR_CONFIG);
        let big = dir.join("big.env");
        std::fs::write(&big, "x".repeat(ENV_MAX_BYTES as usize + 1)).unwrap();
        std::fs::set_permissions(&big, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_config_at(&dir, &big, uid).unwrap_err(), ERR_CONFIG);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn websocket_auth_subscribe_and_state_changed_events() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
                .await
                .unwrap();
            let auth: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(auth["type"], "auth");
            assert_eq!(auth["access_token"], TOKEN);
            ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into()))
                .await
                .unwrap();
            let sub: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(sub["type"], "subscribe_events");
            assert_eq!(sub["event_type"], "state_changed");
            let ev = json!({"id": 1, "type": "event", "event": {
                "event_type": "state_changed", "time_fired": "2026-11-04T04:00:01Z",
                "data": {"entity_id": "light.kitchen",
                    "old_state": {"entity_id": "light.kitchen", "state": "off", "attributes": {}},
                    "new_state": {"entity_id": "light.kitchen", "state": "on",
                        "attributes": {"friendly_name": "Kitchen", "entity_picture": format!("/x?token={TOKEN}")}}}}});
            ws.send(Message::Text(ev.to_string().into())).await.unwrap();
            let other = json!({"id": 1, "type": "event", "event": {"event_type": "call_service", "data": {}}});
            ws.send(Message::Text(other.to_string().into()))
                .await
                .unwrap();
            ws.close(None).await.unwrap();
        });
        let (ws, _) = tokio_tungstenite::connect_async(ws_url(&format!("http://{addr}")))
            .await
            .unwrap();
        let mut connected = false;
        let mut changes = Vec::new();
        let end = live_session(
            ws,
            &HaToken(TOKEN.into()),
            WsTimings::default(),
            || connected = true,
            |c| changes.push(c),
        )
        .await;
        server.await.unwrap();
        assert!(connected);
        assert_eq!(end, SessionEnd::Closed);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, "light.kitchen");
        assert_eq!(changes[0].new_state.as_ref().unwrap().state, "on");
        assert_eq!(changes[0].old_state.as_ref().unwrap().state, "off");
        let payload = serde_json::to_string(&changes[0]).unwrap();
        assert!(!payload.contains(TOKEN), "{payload}");
        assert!(payload.contains("\"newState\"") && payload.contains("\"timeFired\""));
    }

    #[tokio::test]
    async fn websocket_auth_invalid_ends_the_session() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
                .await
                .unwrap();
            let _ = ws.next().await;
            ws.send(Message::Text(
                r#"{"type":"auth_invalid","message":"x"}"#.into(),
            ))
            .await
            .unwrap();
        });
        let (ws, _) = tokio_tungstenite::connect_async(ws_url(&format!("http://{addr}")))
            .await
            .unwrap();
        let mut connected = false;
        let end = live_session(
            ws,
            &HaToken(TOKEN.into()),
            WsTimings::default(),
            || connected = true,
            |_| {},
        )
        .await;
        assert_eq!(end, SessionEnd::AuthFailed);
        assert!(!connected);
    }

    #[test]
    fn backoff_doubles_to_a_cap_and_container_status_parses() {
        let mut b = BACKOFF_START;
        let mut seen = vec![];
        for _ in 0..10 {
            seen.push(b.as_secs());
            b = next_backoff(b);
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 32, 60, 60, 60, 60]);
        assert_eq!(
            parse_container_status("Up 3 hours\n"),
            HaContainerStatus {
                running: true,
                status: "Up 3 hours".into()
            }
        );
        assert!(!parse_container_status("Exited (0) 2 minutes ago").running);
        assert_eq!(
            parse_container_status(""),
            HaContainerStatus {
                running: false,
                status: String::new()
            }
        );
        assert_eq!(
            ws_url("http://127.0.0.1:8123"),
            "ws://127.0.0.1:8123/api/websocket"
        );
    }

    #[test]
    fn allowlist_domain_match_unknown_and_sensitive() {
        for (d, s) in [
            ("light", "turn_on"),
            ("light", "toggle"),
            ("switch", "turn_off"),
            ("fan", "toggle"),
            ("media_player", "turn_on"),
            ("scene", "turn_on"),
            ("climate", "turn_on"),
            ("climate", "turn_off"),
        ] {
            let (p, body) = authorize(d, s, &format!("{d}.x"), None).unwrap();
            assert_eq!(p, Policy::Allowed, "{d}.{s}");
            assert_eq!(body, json!({"entity_id": format!("{d}.x")}));
        }
        // Domain mismatch.
        assert_eq!(
            authorize("light", "turn_on", "switch.x", None).unwrap_err(),
            ERR_DOMAIN_MISMATCH
        );
        assert_eq!(
            authorize("lock", "unlock", "light.x", None).unwrap_err(),
            ERR_DOMAIN_MISMATCH
        );
        // Unknown domain / service refused outright.
        for (d, s) in [
            ("light", "turn_off_all"),
            ("scene", "turn_off"),
            ("scene", "apply"),
            ("vacuum", "start"),
            ("notify", "send"),
            ("climate", "set_fan_mode"),
            ("persistent_notification", "create"),
        ] {
            assert_eq!(
                authorize(d, s, &format!("{d}.x"), None).unwrap_err(),
                ERR_NOT_ALLOWED,
                "{d}.{s}"
            );
        }
        // Sensitive domains need the native confirm.
        for d in SENSITIVE_DOMAINS {
            let (p, _) = authorize(d, "turn_on", &format!("{d}.x"), None).unwrap();
            assert_eq!(p, Policy::NeedsApproval, "{d}");
        }
        assert_eq!(
            authorize("lock", "unlock", "lock.front", None).unwrap().0,
            Policy::NeedsApproval
        );
        // Climate data: typed, bounded, only for the two services.
        let t = |t: f64| HaServiceData {
            temperature: Some(t),
            hvac_mode: None,
        };
        let m = |m: &str| HaServiceData {
            temperature: None,
            hvac_mode: Some(m.into()),
        };
        let (p, body) =
            authorize("climate", "set_temperature", "climate.lr", Some(&t(21.5))).unwrap();
        assert_eq!(p, Policy::Allowed);
        assert_eq!(
            body,
            json!({"entity_id": "climate.lr", "temperature": 21.5})
        );
        let (_, body) = authorize(
            "climate",
            "set_hvac_mode",
            "climate.lr",
            Some(&m("heat_cool")),
        )
        .unwrap();
        assert_eq!(
            body,
            json!({"entity_id": "climate.lr", "hvac_mode": "heat_cool"})
        );
        for bad in [
            t(4.9),
            t(95.1),
            t(f64::NAN),
            t(f64::INFINITY),
            HaServiceData::default(),
            HaServiceData {
                temperature: Some(20.0),
                hvac_mode: Some("heat".into()),
            },
        ] {
            assert_eq!(
                authorize("climate", "set_temperature", "climate.lr", Some(&bad)).unwrap_err(),
                ERR_BAD_DATA
            );
        }
        assert_eq!(
            authorize("climate", "set_temperature", "climate.lr", None).unwrap_err(),
            ERR_BAD_DATA
        );
        for bad in [m("turbo"), m("HEAT"), m(""), t(20.0)] {
            assert_eq!(
                authorize("climate", "set_hvac_mode", "climate.lr", Some(&bad)).unwrap_err(),
                ERR_BAD_DATA
            );
        }
        assert_eq!(
            authorize("light", "turn_on", "light.x", Some(&t(20.0))).unwrap_err(),
            ERR_BAD_DATA
        );
        // Unknown data fields are rejected at deserialization.
        assert!(serde_json::from_value::<HaServiceData>(json!({"brightness": 255})).is_err());
        assert!(serde_json::from_value::<HaServiceData>(json!({"hvacMode": "cool"})).is_ok());
    }

    #[tokio::test]
    async fn oversized_bodies_are_refused_with_fixed_text() {
        let big = format!("[{}]", "0,".repeat(MAX_BODY_BYTES / 2) + "0");
        assert!(big.len() > MAX_BODY_BYTES);
        let (base, _) = mock(vec![
            ("/api/states", 200, big.clone()),
            ("/api/conversation/process", 200, big),
        ])
        .await;
        assert_eq!(states_with(&cfg(&base)).await.unwrap_err(), ERR_TOO_LARGE);
        assert_eq!(
            conversation_with(&cfg(&base), "hi").await.unwrap_err(),
            ERR_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn streamed_body_without_length_is_capped() {
        // Chunked reply (no Content-Length) exercises the streaming cap.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let chunk = " ".repeat(1000);
                    let mut out = String::from("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n");
                    for _ in 0..3 {
                        out.push_str(&format!("{:x}\r\n{chunk}\r\n", chunk.len()));
                    }
                    out.push_str("1\r\n1\r\n0\r\n\r\n");
                    let _ = sock.write_all(out.as_bytes()).await;
                });
            }
        });
        let get = || client().get(format!("http://{addr}/")).send();
        let resp = get().await.unwrap();
        assert!(resp.content_length().is_none());
        assert_eq!(read_capped(resp, 2500).await.unwrap_err(), ERR_TOO_LARGE);
        let v = read_capped(get().await.unwrap(), 4000).await.unwrap();
        assert!(v.is_number());
    }

    #[tokio::test]
    async fn websocket_config_connect_timeout_and_idle_timeout() {
        let c = ws_config();
        assert_eq!(c.max_message_size, Some(1024 * 1024));
        assert_eq!(c.max_frame_size, Some(256 * 1024));
        assert_eq!(WS_CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(WS_IDLE_TIMEOUT, Duration::from_secs(90));

        // A server that accepts TCP but never answers the handshake → connect times out.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = silent.local_addr().unwrap();
        let hold = tokio::spawn(async move {
            let (_sock, _) = silent.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let t0 = std::time::Instant::now();
        assert!(connect_ws(
            &format!("ws://{addr}/api/websocket"),
            Duration::from_millis(200)
        )
        .await
        .is_none());
        assert!(t0.elapsed() < Duration::from_secs(5));
        hold.abort();

        // Server goes silent after auth_ok → the idle timeout ends the session.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Message::Text(r#"{"type":"auth_required"}"#.into()))
                .await
                .unwrap();
            let _ = ws.next().await;
            ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into()))
                .await
                .unwrap();
            let _ = ws.next().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let ws = connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
            .await
            .unwrap();
        let mut connected = false;
        let t0 = std::time::Instant::now();
        let end = live_session(
            ws,
            &HaToken(TOKEN.into()),
            WsTimings {
                idle: Duration::from_millis(300),
                ..WsTimings::default()
            },
            || connected = true,
            |_| {},
        )
        .await;
        assert!(connected);
        assert_eq!(end, SessionEnd::Closed);
        assert!(t0.elapsed() < Duration::from_secs(5));
        server.abort();

        // Oversized message (> 1 MiB) is rejected by the client config → session ends.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Message::Text("x".repeat(2 * 1024 * 1024).into()))
                .await
                .ok();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let ws = connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
            .await
            .unwrap();
        let end = live_session(
            ws,
            &HaToken(TOKEN.into()),
            WsTimings::default(),
            || {},
            |_| {},
        )
        .await;
        assert_eq!(end, SessionEnd::Closed);
    }

    #[test]
    fn every_error_starts_with_a_known_kind() {
        let all = [
            ERR_CONFIG,
            ERR_URL,
            ERR_UNREACHABLE,
            ERR_AUTH,
            ERR_FAILED,
            ERR_BAD_REPLY,
            ERR_INVALID,
            ERR_TEXT,
            ERR_BUSY,
            ERR_DOCKER,
            ERR_DOCKER_FAILED,
            ERR_DOCKER_TIMEOUT,
            ERR_TOO_LARGE,
            ERR_NOT_ALLOWED,
            ERR_DOMAIN_MISMATCH,
            ERR_BAD_DATA,
            ERR_DECLINED,
            ERR_NEEDS_APPROVAL,
            ERR_BUSY_APPROVAL,
            ERR_STORAGE,
            ERR_SAVE,
            ERR_TOKEN_FORMAT,
            ERR_NOTHING,
            ERR_TOKEN_REQUIRED,
        ];
        assert_eq!(ERROR_KINDS.len(), 17);
        for e in all {
            let (kind, text) = e.split_once(": ").expect(e);
            assert!(ERROR_KINDS.contains(&kind), "{e}");
            assert!(!text.is_empty() && !text.contains(TOKEN));
        }
        // Approval-gate errors map to fixed kinds.
        assert_eq!(
            approval_error(&super::super::approval_gate::denied_message()),
            ERR_DECLINED
        );
        assert_eq!(approval_error("busy: x"), ERR_BUSY_APPROVAL);
        assert_eq!(
            approval_error("Approval dialog failed: x"),
            ERR_NEEDS_APPROVAL
        );
        // Every kind is used by at least one error.
        for k in ERROR_KINDS {
            assert!(
                all.iter().any(|e| e.starts_with(&format!("{k}: "))),
                "{k} unused"
            );
        }
    }

    /// Accept one ws client; run `script` on the server side.
    async fn ws_pair<F, Fut>(script: F) -> WsStream
    where
        F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            script(tokio_tungstenite::accept_async(sock).await.unwrap()).await;
        });
        connect_ws(&ws_url(&format!("http://{addr}")), WS_CONNECT_TIMEOUT)
            .await
            .unwrap()
    }

    async fn auth_ok_then<S>(ws: &mut S)
    where
        S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
            + futures::Sink<Message>
            + Unpin,
    {
        let _ = ws
            .send(Message::Text(r#"{"type":"auth_required"}"#.into()))
            .await;
        let _ = ws.next().await;
        let _ = ws.send(Message::Text(r#"{"type":"auth_ok"}"#.into())).await;
        let _ = ws.next().await; // subscribe
    }

    #[tokio::test]
    async fn auth_phase_is_bounded_by_the_auth_timeout() {
        assert_eq!(WS_AUTH_TIMEOUT, Duration::from_secs(10));
        assert_eq!(WS_PING_INTERVAL, Duration::from_secs(30));
        assert_eq!(WsTimings::default().auth, WS_AUTH_TIMEOUT);
        // Scaled 10 s → 300 ms (tokio paused time needs the `test-util` feature, which would
        // mean editing Cargo.toml). Server never sends auth_required.
        let ws = ws_pair(|ws| async move {
            let _keep_open = ws;
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
        .await;
        let t = WsTimings {
            auth: Duration::from_millis(300),
            ..WsTimings::default()
        };
        let t0 = std::time::Instant::now();
        let end = live_session(ws, &HaToken(TOKEN.into()), t, || {}, |_| {}).await;
        assert_eq!(end, SessionEnd::Closed);
        let took = t0.elapsed();
        assert!(
            took >= Duration::from_millis(300) && took < Duration::from_secs(3),
            "{took:?}"
        );
        // The bound covers the whole phase: auth_required arrives, auth_ok never does.
        let ws = ws_pair(|mut ws| async move {
            let _ = ws
                .send(Message::Text(r#"{"type":"auth_required"}"#.into()))
                .await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
        .await;
        let mut connected = false;
        let t0 = std::time::Instant::now();
        let end = live_session(ws, &HaToken(TOKEN.into()), t, || connected = true, |_| {}).await;
        assert_eq!(end, SessionEnd::Closed);
        assert!(!connected);
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn pings_keep_a_silent_healthy_server_alive_past_the_idle_limit() {
        // Scaled: ping 30 s → 100 ms, idle 90 s → 300 ms. Run 4× the idle limit.
        let t = WsTimings {
            auth: Duration::from_secs(5),
            ping: Duration::from_millis(100),
            idle: Duration::from_millis(300),
        };
        let pings = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let p2 = pings.clone();
        // Healthy: sends no events, but keeps reading, so tungstenite auto-replies with pongs.
        let ws = ws_pair(move |mut ws| async move {
            auth_ok_then(&mut ws).await;
            while let Some(Ok(m)) = ws.next().await {
                if m.is_ping() {
                    p2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
        .await;
        let mut connected = false;
        let r = tokio::time::timeout(
            Duration::from_millis(1200),
            live_session(ws, &HaToken(TOKEN.into()), t, || connected = true, |_| {}),
        )
        .await;
        assert!(r.is_err(), "session must still be alive past 4× idle");
        assert!(connected);
        assert!(pings.load(std::sync::atomic::Ordering::SeqCst) >= 5);

        // Unhealthy: never reads, so no pongs → closed by the idle limit.
        let ws = ws_pair(|mut ws| async move {
            auth_ok_then(&mut ws).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
        .await;
        let t0 = std::time::Instant::now();
        let end = live_session(ws, &HaToken(TOKEN.into()), t, || {}, |_| {}).await;
        assert_eq!(end, SessionEnd::Closed);
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn conversation_match_only_for_action_done_or_query_answer() {
        let reply = |rt: Option<&str>, code: Option<&str>| {
            let mut r = json!({"speech": {"plain": {"speech": "Sorry, I am not aware of any device called 2+2"}}});
            if let Some(rt) = rt {
                r["response_type"] = json!(rt);
            }
            if let Some(code) = code {
                r["data"] = json!({"code": code});
            }
            json!({"response": r})
        };
        let m = HaConversationResult::Match {
            speech: "Sorry, I am not aware of any device called 2+2".into(),
        };
        assert_eq!(
            conversation_result(&reply(Some("action_done"), None)).unwrap(),
            m
        );
        assert_eq!(
            conversation_result(&reply(Some("query_answer"), None)).unwrap(),
            m
        );
        for code in [
            "no_valid_targets",
            "no_intent_match",
            "failed_to_handle",
            "unknown",
        ] {
            assert_eq!(
                conversation_result(&reply(Some("error"), Some(code))).unwrap(),
                HaConversationResult::NoMatch,
                "{code}"
            );
        }
        assert_eq!(
            conversation_result(&reply(None, None)).unwrap(),
            HaConversationResult::NoMatch
        );
        assert_eq!(
            conversation_result(&reply(Some("weird"), None)).unwrap(),
            HaConversationResult::NoMatch
        );
        assert_eq!(
            conversation_result(&json!({})).unwrap(),
            HaConversationResult::NoMatch
        );
    }

    // ─── settings ────────────────────────────────────────────────────────────

    const NEW_TOKEN: &str = "abcDEF0123456789._-abcDEF0123456789xyz";

    fn tmpdir() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("nexus-ha-set-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir.canonicalize().unwrap()
    }

    fn src(app: &Path) -> Sources {
        Sources {
            app_dir: Some(app.to_path_buf()),
            legacy_dir: None,
            uid: current_uid(),
            cfg_url: None,
            env_url: None,
            env_token: None,
        }
    }

    fn set_token(dir: &Path, t: &str) -> Result<(), String> {
        set_config_with(
            dir,
            current_uid(),
            None,
            Some(HaToken(t.into())),
            |_| Ok(()),
        )
    }

    #[test]
    fn token_format_rules() {
        assert!(valid_new_token(NEW_TOKEN));
        for bad in [
            format!("{NEW_TOKEN}\nEVIL=1"),
            format!("{NEW_TOKEN}\r"),
            format!("{NEW_TOKEN}\0"),
            format!("{NEW_TOKEN}=x"),
            format!("{NEW_TOKEN}#x"),
            format!(" {NEW_TOKEN}"),
            format!("{NEW_TOKEN} "),
            "a".repeat(31),
            "a".repeat(513),
            String::new(),
        ] {
            assert!(!valid_new_token(&bad));
        }
        assert!(valid_new_token(&"a".repeat(32)) && valid_new_token(&"a".repeat(512)));
    }

    #[test]
    fn newline_or_equals_token_refused_and_file_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let f = dir.join(".env");
        std::fs::write(&f, "A=1\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        for bad in [format!("{NEW_TOKEN}\nX=1"), format!("{NEW_TOKEN}=1")] {
            let e = set_token(&dir, &bad).unwrap_err();
            assert_eq!(e, ERR_TOKEN_FORMAT);
            assert!(!e.contains(NEW_TOKEN));
        }
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "A=1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn symlink_target_and_folder_refused() {
        let dir = tmpdir();
        let real = dir.join("real.env");
        std::fs::write(&real, "A=1\n").unwrap();
        std::os::unix::fs::symlink(&real, dir.join(".env")).unwrap();
        assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
        assert_eq!(
            write_env_key(&dir, current_uid(), TOKEN_KEY, None).unwrap_err(),
            ERR_STORAGE
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "A=1\n");
        // reading through the symlink fails closed
        assert_eq!(resolve(&src(&dir)).unwrap_err(), ERR_STORAGE);

        let other = tmpdir();
        let link_dir = dir.join("linkdir");
        std::os::unix::fs::symlink(&other, &link_dir).unwrap();
        assert_eq!(set_token(&link_dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
        assert!(!other.join(".env").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn group_or_world_writable_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let f = dir.join(".env");
        std::fs::write(&f, "A=1\n").unwrap();
        for mode in [0o620, 0o602, 0o666, 0o640, 0o604] {
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), "A=1\n");
        }
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        for mode in [0o770, 0o707] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(set_token(&dir, NEW_TOKEN).unwrap_err(), ERR_STORAGE);
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        // wrong owner is refused (checked against the handle's uid)
        assert_eq!(
            write_env_key(&dir, current_uid() + 1, TOKEN_KEY, Some(NEW_TOKEN)).unwrap_err(),
            ERR_STORAGE
        );
        // the writer applies the reader's rule (no group/other bits); result is 0600
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        set_token(&dir, NEW_TOKEN).unwrap();
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_keys_byte_identical_after_set_and_clear() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let f = dir.join(".env");
        let original = "# header\r\nA=1\nexport B='x y'  \n\nHA_TOKEN=oldoldoldoldoldoldoldoldoldoldold\nC=\"q\"\nHA_TOKEN=dup\nLAST=no-newline";
        std::fs::write(&f, original).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        let others: Vec<&str> = original
            .split_inclusive('\n')
            .filter(|l| !l.starts_with("HA_TOKEN"))
            .collect();

        set_token(&dir, NEW_TOKEN).unwrap();
        let after = std::fs::read_to_string(&f).unwrap();
        let after_others: Vec<&str> = after
            .split_inclusive('\n')
            .filter(|l| !l.starts_with("HA_TOKEN"))
            .collect();
        assert_eq!(after_others, others);
        assert_eq!(after.matches("HA_TOKEN=").count(), 1);
        assert!(after.contains(&format!("\nHA_TOKEN={NEW_TOKEN}\n")));
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(resolve(&src(&dir)).unwrap().token.0, NEW_TOKEN);

        write_env_key(&dir, current_uid(), TOKEN_KEY, None).unwrap();
        let cleared = std::fs::read_to_string(&f).unwrap();
        assert_eq!(cleared, others.concat());
        assert!(!cleared.contains("HA_TOKEN"));
        // no temp files left behind
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![".env".to_string()]);

        // append to a fresh file; clear on a missing file is a no-op
        let d2 = tmpdir();
        write_env_key(&d2, current_uid(), TOKEN_KEY, None).unwrap();
        assert!(!d2.join(".env").exists());
        set_token(&d2, NEW_TOKEN).unwrap();
        assert_eq!(
            std::fs::read_to_string(d2.join(".env")).unwrap(),
            format!("HA_TOKEN={NEW_TOKEN}\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&d2);
    }

    #[test]
    fn get_config_never_contains_token_bytes() {
        let dir = tmpdir();
        set_token(&dir, NEW_TOKEN).unwrap();
        let mut s = src(&dir);
        s.cfg_url = Some("http://localhost:8123/".into());
        let view = get_config_with(&s);
        let json = serde_json::to_string(&view).unwrap();
        assert_eq!(
            json,
            r#"{"url":"http://localhost:8123","tokenSet":true,"tokenSource":"app"}"#
        );
        assert!(!json.contains(NEW_TOKEN) && !json.contains(&NEW_TOKEN[..8]));
        assert!(!format!("{view:?}").contains(NEW_TOKEN));
        let cfg = resolve(&s).unwrap();
        assert!(!format!("{cfg:?}").contains(NEW_TOKEN));
        assert_eq!(
            serde_json::to_string(&get_config_with(&src(&tmpdir()))).unwrap(),
            r#"{"url":null,"tokenSet":false,"tokenSource":null}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_config_url_rules() {
        let dir = tmpdir();
        let mut saved = None;
        set_config_with(
            &dir,
            current_uid(),
            Some(" http://127.0.0.1:8123 ".into()),
            None,
            |u| {
                saved = Some(u);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(saved, Some(Some("http://127.0.0.1:8123".to_string())));
        let mut saved = None;
        set_config_with(&dir, current_uid(), Some(String::new()), None, |u| {
            saved = Some(u);
            Ok(())
        })
        .unwrap();
        assert_eq!(saved, Some(None));
        for bad in [
            "https://example.com",
            "http://user:p@127.0.0.1",
            "ftp://127.0.0.1",
        ] {
            let e = set_config_with(&dir, current_uid(), Some(bad.into()), None, |_| {
                panic!("must not save")
            })
            .unwrap_err();
            assert_eq!(e, ERR_URL);
        }
        // bad URL rejects before the token is written
        let e = set_config_with(
            &dir,
            current_uid(),
            Some("http://10.0.0.1".into()),
            Some(HaToken(NEW_TOKEN.into())),
            |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(e, ERR_URL);
        assert!(!dir.join(".env").exists());
        assert_eq!(
            set_config_with(&dir, current_uid(), None, None, |_| Ok(())).unwrap_err(),
            ERR_NOTHING
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn precedence_app_env_then_env_var_then_legacy() {
        use std::os::unix::fs::PermissionsExt;
        let app = tmpdir();
        let legacy = tmpdir();
        let lf = legacy.join(".env");
        std::fs::write(&lf, "HA_URL=http://127.0.0.1:9000\nHA_TOKEN=legacytoken\n").unwrap();
        std::fs::set_permissions(&lf, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut s = src(&app);
        s.legacy_dir = Some(legacy.clone());
        let c = resolve(&s).unwrap();
        assert_eq!(
            (c.base.as_str(), c.token.0.as_str()),
            ("http://127.0.0.1:9000", "legacytoken")
        );
        s.env_token = Some(HaToken("envtoken".into()));
        s.env_url = Some("http://127.0.0.1:9001".into());
        let c = resolve(&s).unwrap();
        assert_eq!(
            (c.base.as_str(), c.token.0.as_str()),
            ("http://127.0.0.1:9001", "envtoken")
        );
        set_token(&app, NEW_TOKEN).unwrap();
        s.cfg_url = Some("http://localhost:9002".into());
        let c = resolve(&s).unwrap();
        assert_eq!(
            (c.base.as_str(), c.token.0.as_str()),
            ("http://localhost:9002", NEW_TOKEN)
        );
        s.cfg_url = Some("http://192.168.1.2:8123".into());
        assert_eq!(resolve(&s).unwrap_err(), ERR_URL);
        let _ = std::fs::remove_dir_all(&app);
        let _ = std::fs::remove_dir_all(&legacy);
    }

    #[tokio::test]
    async fn test_connection_errors_never_carry_the_token() {
        // mock() echoes TOKEN on unknown routes; these routes echo it too.
        let (base, _) = mock(vec![("/api/config", 500, format!("bad {TOKEN}"))]).await;
        let cfg = HaConfig {
            base,
            token: HaToken(TOKEN.into()),
        };
        let r = test_with(&cfg).await;
        assert_eq!(r.kind, Some(KIND_UPSTREAM_ERROR));
        let all = format!("{} {r:?}", serde_json::to_string(&r).unwrap());
        assert!(!all.contains(TOKEN));

        let (base, _) = mock(vec![(
            "/api/config",
            200,
            format!("{{\"version\":\"{TOKEN}\"}}"),
        )])
        .await;
        let cfg = HaConfig {
            base,
            token: HaToken(TOKEN.into()),
        };
        let r = test_with(&cfg).await;
        assert!(r.ok && r.version.is_none()); // > 32 chars or unsafe charset: dropped
        assert!(!serde_json::to_string(&r).unwrap().contains(TOKEN));

        let (base, _) = mock(vec![("/api/config", 401, TOKEN.to_string())]).await;
        let cfg = HaConfig {
            base,
            token: HaToken(TOKEN.into()),
        };
        let r = test_with(&cfg).await;
        assert_eq!(r.kind, Some("auth_rejected"));
        assert!(!serde_json::to_string(&r).unwrap().contains(TOKEN));

        let (base, _) = mock(vec![(
            "/api/config",
            200,
            r#"{"version":"2026.10.4"}"#.into(),
        )])
        .await;
        let cfg = HaConfig {
            base,
            token: HaToken(TOKEN.into()),
        };
        assert_eq!(
            serde_json::to_string(&test_with(&cfg).await).unwrap(),
            r#"{"ok":true,"version":"2026.10.4"}"#
        );
        let cfg = HaConfig {
            base: "http://127.0.0.1:1".into(),
            token: HaToken(TOKEN.into()),
        };
        assert_eq!(test_with(&cfg).await.kind, Some(KIND_UNREACHABLE));
    }

    #[test]
    fn test_connection_rate_limit_and_versions() {
        let now = std::time::Instant::now();
        assert!(gap_ok(None, now));
        assert!(!gap_ok(Some(now), now + Duration::from_millis(1999)));
        assert!(gap_ok(Some(now), now + Duration::from_secs(2)));
        assert_eq!(safe_version("2026.10.4").as_deref(), Some("2026.10.4"));
        assert!(safe_version("1.0 <script>").is_none());
        assert!(safe_version(&"1".repeat(33)).is_none());
        assert!(safe_version("").is_none());
        assert_eq!(fail(ERR_BUSY).kind, Some("already_running"));
        assert_eq!(fail("weird text").kind, Some(KIND_UPSTREAM_ERROR));
    }

    #[test]
    fn token_redacted_and_secret_text_redacted() {
        let t = HaToken(NEW_TOKEN.into());
        assert_eq!(format!("{t:?}"), "HaToken(<redacted>)");
        let st = SecretText(zeroize::Zeroizing::new(NEW_TOKEN.to_string()));
        assert_eq!(format!("{st:?}"), "SecretText(<redacted>)");
    }

    #[test]
    fn get_config_reports_token_source() {
        use std::os::unix::fs::PermissionsExt;
        let app = tmpdir();
        let legacy = tmpdir();
        let lf = legacy.join(".env");
        std::fs::write(&lf, "HA_TOKEN=legacytoken\n").unwrap();
        std::fs::set_permissions(&lf, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut s = src(&app);
        let json = |s: &Sources| serde_json::to_string(&get_config_with(s)).unwrap();
        assert!(json(&s).ends_with(r#""tokenSet":false,"tokenSource":null}"#));
        s.legacy_dir = Some(legacy.clone());
        assert!(json(&s).ends_with(r#""tokenSet":true,"tokenSource":"legacy"}"#));
        s.env_token = Some(HaToken("envtoken".into()));
        assert!(json(&s).ends_with(r#""tokenSet":true,"tokenSource":"env"}"#));
        set_token(&app, NEW_TOKEN).unwrap();
        let j = json(&s);
        assert!(j.ends_with(r#""tokenSet":true,"tokenSource":"app"}"#));
        assert!(!j.contains(NEW_TOKEN) && !j.contains("envtoken") && !j.contains("legacytoken"));
        // unsafe app .env: nothing usable is reported
        std::fs::set_permissions(app.join(".env"), std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(json(&s).ends_with(r#""tokenSet":false,"tokenSource":null}"#));
        let _ = std::fs::remove_dir_all(&app);
        let _ = std::fs::remove_dir_all(&legacy);
    }

    #[test]
    fn clear_token_reports_fallback() {
        let app = tmpdir();
        let mut s = src(&app);
        set_token(&app, NEW_TOKEN).unwrap();
        let r = clear_token_with(&s).unwrap();
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"fallbackActive":false}"#
        );
        assert!(!std::fs::read_to_string(app.join(".env"))
            .unwrap()
            .contains("HA_TOKEN"));
        set_token(&app, NEW_TOKEN).unwrap();
        s.env_token = Some(HaToken("envtoken".into()));
        assert_eq!(
            clear_token_with(&s).unwrap(),
            HaClearResult {
                fallback_active: true
            }
        );
        assert_eq!(get_config_with(&s).token_source, Some(TokenSource::Env));
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn concurrent_set_and_clear_leave_a_consistent_file() {
        use std::os::unix::fs::PermissionsExt;
        let app = tmpdir();
        let f = app.join(".env");
        std::fs::write(&f, "A=1\nB=2\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        let handles: Vec<_> = (0..16)
            .map(|i| {
                let app = app.clone();
                std::thread::spawn(move || {
                    if i % 2 == 0 {
                        set_token(&app, NEW_TOKEN).unwrap();
                    } else {
                        write_env_key(&app, current_uid(), TOKEN_KEY, None).unwrap();
                    }
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.starts_with("A=1\nB=2\n"));
        assert!(text.matches("HA_TOKEN=").count() <= 1);
        assert_eq!(std::fs::read_dir(&app).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn legacy_non_utf8_file_is_a_config_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let f = dir.join(".env");
        let mut bytes = format!("HA_TOKEN={TOKEN}\n").into_bytes();
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
        std::fs::write(&f, bytes).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read_config_at(&dir, &f, current_uid()).unwrap_err(),
            ERR_CONFIG
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().to_string(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }

    fn ov(url: Option<&str>, token: Option<&str>) -> TestOverrides {
        test_overrides(url.map(str::to_string), token.map(|t| HaToken(t.into()))).unwrap()
    }

    #[tokio::test]
    async fn test_connection_other_url_with_saved_token_is_token_required() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        // Raw listener: counts connections, so "no request" also means "no connect".
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let other = format!("http://{}", listener.local_addr().unwrap());
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        tokio::spawn(async move {
            while listener.accept().await.is_ok() {
                h2.fetch_add(1, Ordering::SeqCst);
            }
        });
        let dir = tmpdir();
        set_token(&dir, TOKEN).unwrap();
        let mut s = src(&dir);
        s.cfg_url = Some("http://127.0.0.1:1".into());
        let before = snapshot(&dir);
        for url in [other.as_str(), "http://localhost:1", "http://127.0.0.1:2"] {
            let r = test_connection_with(&s, ov(Some(url), None)).await;
            assert!(!r.ok);
            assert_eq!(r.kind, Some(KIND_TOKEN_REQUIRED), "{url}");
        }
        // No saved URL either: the default base is the only one the saved token may reach.
        s.cfg_url = None;
        let r = test_connection_with(&s, ov(Some(&other), None)).await;
        assert_eq!(r.kind, Some(KIND_TOKEN_REQUIRED));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the other port saw no connection"
        );
        assert_eq!(snapshot(&dir), before, "nothing persisted");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_connection_same_url_with_saved_token_is_allowed() {
        let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
        let port = base.rsplit(':').next().unwrap().to_string();
        let dir = tmpdir();
        set_token(&dir, TOKEN).unwrap();
        let mut s = src(&dir);
        s.cfg_url = Some(base.clone());
        let before = snapshot(&dir);
        // Same base after loopback_base: exact, trailing slash, upper-case scheme/host spelling.
        for url in [
            base.clone(),
            format!("{base}/"),
            format!(" HTTP://127.0.0.1:{port}/ "),
        ] {
            let r = test_connection_with(&s, ov(Some(&url), None)).await;
            assert!(r.ok, "{url}: {r:?}");
        }
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(
            seen.iter().all(|r| r.2),
            "saved token sent to the saved base"
        );
        assert_eq!(snapshot(&dir), before, "nothing persisted");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_connection_override_token_only_uses_stored_url() {
        let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
        let dir = tmpdir();
        set_token(&dir, NEW_TOKEN).unwrap();
        let mut s = src(&dir);
        s.cfg_url = Some(base.clone());
        let before = snapshot(&dir);
        let r = test_connection_with(&s, ov(None, Some(TOKEN))).await;
        assert!(r.ok, "{r:?}");
        assert!(seen.lock().unwrap()[0].2, "override token sent");
        assert_eq!(snapshot(&dir), before, "stored token unchanged");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_connection_override_both_needs_no_stored_config() {
        let (base, seen) = mock(vec![("/api/config", 200, "{\"version\":\"1\"}".into())]).await;
        let dir = tmpdir();
        let s = src(&dir);
        // Without overrides there is nothing to test with.
        assert!(!test_connection_with(&s, ov(None, None)).await.ok);
        let r = test_connection_with(&s, ov(Some(&base), Some(TOKEN))).await;
        assert!(r.ok, "{r:?}");
        assert!(seen.lock().unwrap()[0].2);
        assert!(snapshot(&dir).is_empty(), "no .env created");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_connection_overrides_are_validated() {
        let bad_url = |u: &str| test_overrides(Some(u.into()), None).unwrap_err();
        assert_eq!(bad_url("http://example.com:8123"), ERR_URL);
        assert_eq!(bad_url("not a url"), ERR_URL);
        assert_eq!(fail(ERR_URL).kind, Some("bad_url"));
        let bad_tok = |t: &str| test_overrides(None, Some(HaToken(t.into()))).unwrap_err();
        assert_eq!(bad_tok("short"), ERR_TOKEN_FORMAT);
        assert_eq!(bad_tok(&format!("{TOKEN} x")), ERR_TOKEN_FORMAT);
        assert_eq!(fail(ERR_TOKEN_FORMAT).kind, Some("invalid_input"));
        let o = ov(Some("  "), Some(""));
        assert!(
            o.base.is_none() && o.token.is_none(),
            "blank = not provided"
        );
        let dbg = format!("{:?}", ov(None, Some(TOKEN)));
        assert!(!dbg.contains(TOKEN), "{dbg}");
    }
}
