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

// One parsed-loopback rule shared with llm_router: `http://<loopback host>[:port]`, rebuilt from the
// parsed URL; userinfo, path, query and fragment refused.
use super::loopback_http::loopback_base;

const DEFAULT_HA_URL: &str = "http://127.0.0.1:8123";
const ENV_MAX_BYTES: u64 = 16 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const DOCKER_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CONVERSATION_CHARS: usize = 1_000;
const MAX_STATES: usize = 5_000;
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(60);
/// Max HTTP response body for /api/states and /api/conversation/process.
pub(super) const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
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
pub(super) const MIN_TEMPERATURE: f64 = 5.0;
pub(super) const MAX_TEMPERATURE: f64 = 95.0;
pub(super) const HVAC_MODES: &[&str] = &[
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
pub(super) const ERR_UNREACHABLE: &str = "unreachable: Home Assistant is not reachable.";
const ERR_AUTH: &str = "auth_rejected: Home Assistant rejected the token.";
const ERR_FAILED: &str = "upstream_error: Home Assistant returned an error.";
pub(super) const ERR_BAD_REPLY: &str = "upstream_error: Home Assistant sent an unexpected reply.";
pub(super) const ERR_INVALID: &str = "invalid_input: Invalid domain, service or entity id.";
const ERR_TEXT: &str = "invalid_input: Message is empty or too long.";
const ERR_BUSY: &str = "already_running: A Home Assistant container action is already running.";
const ERR_DOCKER: &str = "docker_unavailable: Docker is not available.";
const ERR_DOCKER_FAILED: &str = "upstream_error: The Home Assistant container command failed.";
const ERR_DOCKER_TIMEOUT: &str = "timed_out: The Home Assistant container command timed out.";
pub(super) const ERR_TOO_LARGE: &str = "too_large: Home Assistant sent a reply that is too large.";
pub(super) const ERR_NOT_ALLOWED: &str = "not_allowed: That Home Assistant action is not allowed.";
const ERR_DOMAIN_MISMATCH: &str =
    "domain_mismatch: The entity does not belong to the service's domain.";
pub(super) const ERR_BAD_DATA: &str =
    "invalid_input: Invalid or missing data for this Home Assistant service.";
pub(super) const ERR_NEEDS_APPROVAL: &str =
    "needs_approval: This Home Assistant action needs approval, and the approval dialog could not be shown.";
const ERR_BUSY_APPROVAL: &str =
    "already_running: An approval for this Home Assistant domain is already in progress.";
const ERR_DECLINED: &str = "declined: The Home Assistant action was not approved.";

// ─── types (camelCase for the frontend) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
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

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HaConversationResult {
    Match { speech: String },
    NoMatch,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
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
#[derive(Debug, Clone, Default, Deserialize, PartialEq, specta::Type)]
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
pub(super) struct HaConfig {
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

pub(super) fn load_config() -> Result<HaConfig, String> {
    resolve(&Sources::live())
}

pub(super) fn client() -> reqwest::Client {
    super::shared::http::builder(super::shared::http::HttpSpec::loopback().timeout(HTTP_TIMEOUT))
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
pub(super) async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Value, String> {
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

pub(super) fn status_error(status: reqwest::StatusCode) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        ERR_AUTH.to_string()
    } else {
        ERR_FAILED.to_string()
    }
}

// ─── HTTP calls (pure over config, tested against a mock) ────────────────────

pub(super) async fn states_with(cfg: &HaConfig) -> Result<Vec<HaEntity>, String> {
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

pub(super) async fn call_service_with(
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

/// Renders a Jinja template through HA's REST API (read-only; used for area names, which the plain states
/// list does not carry). The reply is JSON text.
pub(super) async fn render_template_with(cfg: &HaConfig, template: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/api/template", cfg.base))
        .bearer_auth(&cfg.token.0)
        .json(&json!({ "template": template }))
        .send()
        .await
        .map_err(|_| ERR_UNREACHABLE.to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    read_capped(resp, MAX_BODY_BYTES).await
}

/// Test-only config for a mock server.
#[cfg(test)]
pub(super) fn test_config(base: &str, token: &str) -> HaConfig {
    HaConfig {
        base: base.to_string(),
        token: HaToken(token.to_string()),
    }
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
#[specta::specta]
pub async fn ha_states() -> Result<Vec<HaEntity>, String> {
    states_with(&load_config()?).await
}

/// Map approval_gate's errors onto our fixed kinds (its text is not passed through).
pub(super) fn approval_error(e: &str) -> String {
    if e.starts_with(super::approval_gate::DECLINED_PREFIX) {
        ERR_DECLINED.to_string()
    } else if e.starts_with(super::approval_gate::BUSY_PREFIX) {
        ERR_BUSY_APPROVAL.to_string()
    } else {
        ERR_NEEDS_APPROVAL.to_string()
    }
}

#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
pub async fn ha_container_start() -> Result<HaContainerStatus, String> {
    compose(&["up", "-d"]).await
}

/// `docker compose -f … stop`.
#[tauri::command]
#[specta::specta]
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
#[specta::specta]
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
#[specta::specta]
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
pub(crate) fn valid_new_token(t: &str) -> bool {
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

/// The `HA_TOKEN` in the app-data `.env` only (no Keychain, no env), for the migrate tool.
/// `Err` = the file exists but is unsafe.
pub(crate) fn app_env_token_value(
    dir: &Path,
) -> Result<Option<zeroize::Zeroizing<String>>, String> {
    Ok(app_env_token(dir, current_uid())?.map(|t| zeroize::Zeroizing::new(t.0.clone())))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
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
    // Keychain first (`ha_token`), then the app `.env`. Both are "the app's own store", so the
    // reported source stays `App` and the IPC type is unchanged.
    let from_app = super::keychain_migrate::with_live_keychain(|kc| {
        super::keychain_migrate::secret_prefer_keychain_if(
            kc,
            "ha_token",
            token_ok_for_use,
            |v| Ok(Some(HaToken(v.to_string()))),
            || match src.app_dir.as_deref() {
                Some(dir) => app_env_token(dir, src.uid),
                None => Ok(None),
            },
        )
    })?;
    if let Some(t) = from_app.filter(|t| token_ok_for_use(&t.0)) {
        return Ok((t, TokenSource::App));
    }
    if let Some(t) = src.env_token.as_ref().filter(|t| token_ok_for_use(&t.0)) {
        return Ok((t.clone(), TokenSource::Env));
    }
    legacy
        .filter(|t| token_ok_for_use(&t.0))
        .map(|t| (t, TokenSource::Legacy))
        .ok_or_else(|| ERR_CONFIG.to_string())
}

/// Test hook for the Keychain precedence tests (keychain_migrate.rs): token for an app dir.
#[cfg(test)]
pub(crate) fn test_token_for(app: &Path) -> Result<String, String> {
    let src = Sources {
        app_dir: Some(app.to_path_buf()),
        legacy_dir: None,
        uid: current_uid(),
        cfg_url: None,
        env_url: None,
        env_token: None,
    };
    resolve_token_with_source(&src, None).map(|(t, _)| t.0.clone())
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

#[derive(Debug, Serialize, specta::Type)]
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

#[derive(Debug, Serialize, PartialEq, specta::Type)]
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
#[specta::specta]
pub fn ha_get_config() -> HaSettingsView {
    get_config_with(&Sources::live())
}

/// Saves the URL (app config) and/or the token (app-data `.env`). `url: ""` clears the saved
/// URL. Rejects with `bad_url`, `invalid_input`, `not_allowed` (unsafe file) or
/// `upstream_error` (write failed).
#[tauri::command]
#[specta::specta]
pub fn ha_set_config(url: Option<String>, token: Option<String>) -> Result<(), String> {
    let token = token.map(HaToken);
    let dir = live_app_dir()?;
    set_config_with(&dir, current_uid(), url, token, |u| {
        super::telegram::update_config(|c| c.ha_url = u).map_err(|_| ERR_SAVE.to_string())
    })
}

#[derive(Debug, Serialize, PartialEq, specta::Type)]
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
#[specta::specta]
pub fn ha_clear_token() -> Result<HaClearResult, String> {
    live_app_dir()?;
    clear_token_with(&Sources::live())
}

/// GET `/api/config` with the resolved settings. Never rejects; failures are `{ok:false, kind}`.
#[tauri::command]
#[specta::specta]
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
mod tests;
