//! Amara chat bridge: Nexus → Hermes's OpenAI-compatible API server (127.0.0.1:8642).
//!
//! Hermes (`~/.hermes`, persona `SOUL.md`) is Amara's only identity and memory; Nexus adds a
//! policy/context system message and never a persona of its own.
//!
//! The previous bridge spawned `python -m hermes_cli.nexus_runtime`, a module that only existed
//! in a Hermes fork commit and is missing from the current checkout, so every Chat send failed.
//! This one speaks plain HTTP:
//!
//! - `API_SERVER_KEY` is read from `~/.hermes/.env` on every request and never logged.
//! - The full visible history goes in `messages` (Hermes takes history from the body when no
//!   `X-Hermes-Session-Id` is sent). `X-Hermes-Session-Key` pins one long-term-memory scope.
//! - Requests are serialized: Hermes on this Mac runs one agent turn at a time.
//!
//! Tier (Fast/Deep) is decided only by the Amara Router. Hermes builds its own reply and does
//! not pass the router's `X-Amara-*` headers or `amara` JSON through, so the tier is taken from
//! (1) `amara` metadata if a future Hermes forwards it, else (2) the router's own route log for
//! this turn, else (3) the router's `/amara/route` dry run, and only when Hermes's provider is
//! the router. Otherwise the tier is unknown and the UI shows no badge.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::sync::Mutex;

pub const HERMES_API_BASE: &str = "http://127.0.0.1:8642";
/// One long-term-memory scope for the app. A new key cold-starts Hermes (minutes on this Mac).
pub const DEFAULT_SESSION_KEY: &str = "nexus-chat";
const SESSION_KEY_ENV: &str = "NEXUS_HERMES_SESSION_KEY";
/// A cold Hermes session can take ~20 min on this Mac; keep the old bridge's ceiling.
const HERMES_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const ROUTER_DRY_RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// How much of the router log to read when confirming a turn's tier.
const ROUTE_LOG_TAIL_BYTES: u64 = 64 * 1024;
const MAX_CACHED_SYSTEM_PROMPTS: usize = 16;

const HERMES_DOWN: &str =
    "Amara isn't reachable: the Hermes API server at 127.0.0.1:8642 is not running. Start the Hermes gateway and try again.";
const HERMES_NO_KEY: &str =
    "Amara can't sign in to Hermes: API_SERVER_KEY is missing from ~/.hermes/.env.";
const HERMES_BAD_KEY: &str =
    "Hermes refused the request: the API_SERVER_KEY in ~/.hermes/.env does not match the running Hermes.";
const HERMES_BUSY: &str = "Hermes is busy with another request. Try again in a moment.";
const HERMES_FAILED: &str = "Hermes could not complete the request.";

#[derive(Default)]
pub struct NexusRuntimeBridge {
    /// Serializes turns (Hermes is single-request here).
    turn: Mutex<()>,
    /// conversation id → stable system prefix (policy only, no per-turn context). Hermes derives
    /// its session id from the system prompt + first user message, so a conversation keeps one
    /// prefix to stay one session; fresh context travels in a separate message each turn.
    system_prompts: std::sync::Mutex<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// Where the shown tier came from. Never guessed: unknown means no badge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AmaraTier {
    /// "fast" | "deep" | "claude" (router names).
    pub tier: String,
    pub reason: String,
    /// "hermes_metadata" | "router_log" | "router_dry_run" | "router_reply"
    pub source: String,
}

/// Where the router says this reply was served, from its response headers (sanitized).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    /// "fast" | "deep" | "claude": the tier that actually answered (`X-Amara-Route`).
    pub tier: String,
    /// `X-Amara-Model`, else the JSON `model` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `X-Amara-Route-Reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug)]
pub struct HermesReply {
    pub content: String,
    pub session_id: Option<String>,
    /// `amara` metadata, if Hermes ever passes the router's through.
    pub tier: Option<AmaraTier>,
    pub route: Option<RouteInfo>,
}

/// A failed turn: fixed user-facing text plus any route headers the error response carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesError {
    pub message: String,
    pub route: Option<RouteInfo>,
}

impl From<String> for HermesError {
    fn from(message: String) -> Self {
        Self {
            message,
            route: None,
        }
    }
}

/// One chat turn as Hermes will see it.
pub struct HermesTurn<'a> {
    pub system: String,
    /// This turn's background sections, rebuilt every turn (empty = none). Sent as a system
    /// message just before the new user message, so the cached prefix never changes.
    pub context: String,
    pub history: &'a [ChatTurn],
    pub message: &'a str,
}

// ─── Pure helpers (unit-tested) ───────────────────────────────────────────────

/// `NAME=value` from a dotenv text (`export`, quotes and trailing comments handled).
pub fn env_value(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let value = value.trim();
        let value = if let Some(inner) = value
            .strip_prefix('"')
            .and_then(|v| v.split_once('"'))
            .map(|(v, _)| v)
        {
            inner
        } else if let Some(inner) = value
            .strip_prefix('\'')
            .and_then(|v| v.split_once('\''))
            .map(|(v, _)| v)
        {
            inner
        } else {
            value.split(" #").next().unwrap_or("").trim()
        };
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Loopback base of the Hermes API server; `API_SERVER_PORT` in `.env` may move the port.
pub fn hermes_base(env_text: &str) -> String {
    match env_value(env_text, "API_SERVER_PORT").and_then(|p| p.parse::<u16>().ok()) {
        Some(port) if port != 0 => format!("http://127.0.0.1:{port}"),
        _ => HERMES_API_BASE.to_string(),
    }
}

/// `X-Hermes-Session-Key`: the env override (for tests) if it is a safe header value.
pub fn session_key(override_value: Option<&str>) -> String {
    override_value
        .map(str::trim)
        .filter(|k| !k.is_empty() && k.len() <= 128 && k.chars().all(|c| c.is_ascii_graphic()))
        .map(ToString::to_string)
        .unwrap_or_else(|| DEFAULT_SESSION_KEY.to_string())
}

/// Leading `/deep`, `/fast` or `/claude` (router prefixes), as the router reads them.
pub fn route_prefix(message: &str) -> Option<&'static str> {
    let first = message.split_whitespace().next()?;
    match first.to_ascii_lowercase().as_str() {
        "/deep" => Some("deep"),
        "/fast" => Some("fast"),
        "/claude" => Some("claude"),
        _ => None,
    }
}

/// The message without a leading route prefix.
pub fn strip_route_prefix(message: &str) -> String {
    if route_prefix(message).is_none() {
        return message.trim().to_string();
    }
    let trimmed = message.trim_start();
    let rest = trimmed
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .unwrap_or("");
    rest.trim().to_string()
}

/// The text Hermes receives. Deep toggle → `/deep ` prefix, which the router honours and strips.
/// When Hermes's provider is not the router nothing would read a prefix, so it is removed
/// rather than shown to the model.
pub fn outgoing_message(message: &str, deep: bool, routed: bool) -> String {
    if !routed {
        return strip_route_prefix(message);
    }
    if deep && route_prefix(message).is_none() {
        format!("/deep {}", message.trim())
    } else {
        message.trim().to_string()
    }
}

/// OpenAI chat body: the stable system prefix, the full history, this turn's fresh context
/// (if any) as a system message, then the new user message.
pub fn chat_body(system: &str, context: &str, history: &[ChatTurn], message: &str) -> Value {
    let mut messages = Vec::with_capacity(history.len() + 2);
    if !system.trim().is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    for turn in history {
        let role = match turn.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            _ => continue,
        };
        if turn.content.trim().is_empty() {
            continue;
        }
        messages.push(json!({"role": role, "content": turn.content}));
    }
    if !context.trim().is_empty() {
        messages.push(json!({"role": "system", "content": context}));
    }
    messages.push(json!({"role": "user", "content": message}));
    json!({"stream": false, "messages": messages})
}

/// `amara` metadata on a chat completion (the router's shape), if present.
pub fn tier_from_metadata(v: &Value, header_tier: Option<&str>) -> Option<AmaraTier> {
    let tier = v["amara"]["tier"].as_str().or(header_tier)?;
    matches!(tier, "fast" | "deep" | "claude").then(|| AmaraTier {
        tier: tier.to_string(),
        reason: v["amara"]["reason"].as_str().unwrap_or("").to_string(),
        source: "hermes_metadata".to_string(),
    })
}

const MODEL_MAX: usize = 128;
const REASON_MAX: usize = 160;
const RETRY_AFTER_DEFAULT: u64 = 10;
const RETRY_AFTER_MAX: u64 = 300;

fn safe_field(v: Option<&str>, max: usize, extra: &[u8]) -> Option<String> {
    let v = v?.trim();
    (!v.is_empty()
        && v.len() <= max
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b)))
    .then(|| v.to_string())
}

/// Route headers (`X-Amara-Route`, `X-Amara-Model`, `X-Amara-Route-Reason`; keys lowercase).
/// Unknown tier → None; an unsafe model or reason → that field None.
pub fn route_info(headers: &HashMap<String, String>, v: &Value) -> Option<RouteInfo> {
    let tier = headers.get("x-amara-route")?.trim().to_ascii_lowercase();
    if !matches!(tier.as_str(), "fast" | "deep" | "claude") {
        return None;
    }
    let model_extra = b"._:/@+-";
    let model = safe_field(
        headers.get("x-amara-model").map(String::as_str),
        MODEL_MAX,
        model_extra,
    )
    .or_else(|| safe_field(v["model"].as_str(), MODEL_MAX, model_extra));
    let reason = safe_field(
        headers.get("x-amara-route-reason").map(String::as_str),
        REASON_MAX,
        b" ._:;,=+/-",
    );
    Some(RouteInfo {
        tier,
        model,
        reason,
    })
}

/// `Retry-After` in whole seconds, clamped to 1-300; missing or non-integer → 10.
pub fn retry_after_secs(headers: &HashMap<String, String>) -> u64 {
    headers
        .get("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|n| n.clamp(1, RETRY_AFTER_MAX))
        .unwrap_or(RETRY_AFTER_DEFAULT)
}

/// Hermes's reply → content/session, or a fixed, user-facing error (never a raw body).
pub fn parse_hermes_reply(
    status: u16,
    headers: &HashMap<String, String>,
    v: &Value,
) -> Result<HermesReply, String> {
    match status {
        200..=299 => {}
        401 | 403 => return Err(HERMES_BAD_KEY.to_string()),
        429 => return Err(HERMES_BUSY.to_string()),
        503 => {
            return Err(format!(
                "model_loading: retry_after={}",
                retry_after_secs(headers)
            ))
        }
        _ => {
            let message = v["error"]["message"].as_str().unwrap_or("").trim();
            let message = super::continuity::redact_sensitive_text(message);
            return Err(if message.is_empty() || message.contains('\n') {
                HERMES_FAILED.to_string()
            } else {
                message.chars().take(600).collect()
            });
        }
    }
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        return Err("Hermes completed without a response.".to_string());
    }
    Ok(HermesReply {
        content,
        session_id: headers
            .get("x-hermes-session-id")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tier: tier_from_metadata(v, headers.get("x-amara-tier").map(String::as_str)),
        route: route_info(headers, v),
    })
}

/// True when Hermes's `model.base_url` is the Amara Router (same loopback port).
pub fn base_url_is_router(hermes_base_url: &str, router_base: &str) -> bool {
    let (Ok(h), Ok(r)) = (
        reqwest::Url::parse(hermes_base_url.trim()),
        reqwest::Url::parse(router_base.trim()),
    ) else {
        return false;
    };
    let loopback = |u: &reqwest::Url| match u.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    };
    loopback(&h) && loopback(&r) && h.port_or_known_default() == r.port_or_known_default()
}

/// The router's dry-run answer (`POST /amara/route`).
pub fn tier_from_dry_run(v: &Value) -> Option<AmaraTier> {
    let tier = v["tier"].as_str()?;
    matches!(tier, "fast" | "deep" | "claude").then(|| AmaraTier {
        tier: tier.to_string(),
        reason: v["reason"].as_str().unwrap_or("").to_string(),
        source: "router_dry_run".to_string(),
    })
}

/// The tier the router actually served during [since, until] (RFC 3339), from routes.jsonl.
/// Only finished OpenAI-API lines count (Hermes's wire): an error or cancelled line served
/// nothing. Another client can hit the router in
/// the same window, so mixed tiers are only resolved when the dry run names one of them.
pub fn tier_from_route_log(
    log: &str,
    since: &str,
    until: &str,
    predicted: Option<&AmaraTier>,
) -> Option<AmaraTier> {
    let (Ok(since), Ok(until)) = (
        chrono::DateTime::parse_from_rfc3339(since),
        chrono::DateTime::parse_from_rfc3339(until),
    ) else {
        return None;
    };
    let mut served: Vec<(String, String)> = Vec::new();
    for line in log.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(ts) = v["ts"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        else {
            continue;
        };
        let unserved = v["status"] == "error" || v["status"] == "cancelled";
        if ts < since || ts > until || v["api"] != "openai" || unserved {
            continue;
        }
        if let Some(route) = v["route"].as_str() {
            served.push((
                route.to_string(),
                v["reason"].as_str().unwrap_or("").to_string(),
            ));
        }
    }
    let first = served.first()?.clone();
    let tier = if served.iter().all(|(t, _)| *t == first.0) {
        first
    } else {
        let p = predicted?;
        served.into_iter().find(|(t, _)| *t == p.tier)?
    };
    Some(AmaraTier {
        tier: tier.0,
        reason: tier.1,
        source: "router_log".to_string(),
    })
}

// ─── Transport ────────────────────────────────────────────────────────────────

/// HTTP POST; tests swap in a mock.
pub(crate) trait JsonTransport {
    /// Ok((status, lower-cased headers, body)); Err on connection failure (detail kept private).
    async fn post(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, HashMap<String, String>, Value), String>;
}

pub(crate) struct HttpTransport {
    client: reqwest::Client,
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self {
            // Both servers are loopback-only; a redirect off the Mac is never legitimate.
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
        }
    }
}

impl JsonTransport for HttpTransport {
    async fn post(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, HashMap<String, String>, Value), String> {
        let mut req = self.client.post(url).json(body).timeout(timeout);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                "timeout".to_string()
            } else {
                "connect".to_string()
            }
        })?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                Some((
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().ok()?.to_string(),
                ))
            })
            .collect();
        let v = resp.json().await.unwrap_or(Value::Null);
        Ok((status, headers, v))
    }
}

/// One Hermes turn over `transport`. `api_key` is only ever put in the Authorization header.
pub(crate) async fn send_turn(
    transport: &impl JsonTransport,
    base: &str,
    api_key: &str,
    session_key: &str,
    body: &Value,
) -> Result<HermesReply, HermesError> {
    let headers = [
        ("Authorization", format!("Bearer {api_key}")),
        ("X-Hermes-Session-Key", session_key.to_string()),
    ];
    let (status, headers, v) = transport
        .post(
            &format!("{base}/v1/chat/completions"),
            &headers,
            body,
            HERMES_TIMEOUT,
        )
        .await
        .map_err(|e| match e.as_str() {
            "timeout" => "Hermes did not complete within 30 minutes.".to_string(),
            _ => HERMES_DOWN.to_string(),
        })?;
    parse_hermes_reply(status, &headers, &v).map_err(|message| HermesError {
        message,
        route: route_info(&headers, &v),
    })
}

/// The router's routing decision for exactly this body, without generating.
pub(crate) async fn dry_run_tier(
    transport: &impl JsonTransport,
    router_base: &str,
    body: &Value,
) -> Option<AmaraTier> {
    let (status, _, v) = transport
        .post(
            &format!("{router_base}/amara/route"),
            &[],
            body,
            ROUTER_DRY_RUN_TIMEOUT,
        )
        .await
        .ok()?;
    (200..300)
        .contains(&status)
        .then(|| tier_from_dry_run(&v))
        .flatten()
}

/// The newest `ROUTE_LOG_TAIL_BYTES` of the router's route log (no message content in it).
pub fn read_route_log_tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(ROUTE_LOG_TAIL_BYTES)));
    let mut buf = Vec::new();
    let _ = file.take(ROUTE_LOG_TAIL_BYTES).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    // Drop a partial first line when we started mid-file.
    if len > ROUTE_LOG_TAIL_BYTES {
        text.split_once('\n')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_default()
    } else {
        text
    }
}

pub fn default_route_log_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/Logs/amara-router/routes.jsonl"))
}

impl NexusRuntimeBridge {
    /// The conversation's system message: the first one built for it, so Hermes keeps one session.
    pub fn system_prompt_for(
        &self,
        conversation_id: &str,
        build: impl FnOnce() -> String,
    ) -> String {
        let mut cache = self
            .system_prompts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = cache.get(conversation_id) {
            return existing.clone();
        }
        if cache.len() >= MAX_CACHED_SYSTEM_PROMPTS {
            cache.clear();
        }
        let prompt = build();
        cache.insert(conversation_id.to_string(), prompt.clone());
        prompt
    }

    /// Send one turn to Hermes. `hermes_home` holds `.env`; the key is read here, every time.
    pub async fn request(
        &self,
        hermes_home: &Path,
        turn: HermesTurn<'_>,
    ) -> Result<HermesReply, HermesError> {
        let env_text = std::fs::read_to_string(hermes_home.join(".env")).unwrap_or_default();
        let api_key = env_value(&env_text, "API_SERVER_KEY")
            .ok_or_else(|| HermesError::from(HERMES_NO_KEY.to_string()))?;
        let base = hermes_base(&env_text);
        let key = session_key(std::env::var(SESSION_KEY_ENV).ok().as_deref());
        let body = chat_body(&turn.system, &turn.context, turn.history, turn.message);
        let _guard = self.turn.lock().await;
        send_turn(&HttpTransport::default(), &base, &api_key, &key, &body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// (url, headers, body) of one request the mock saw.
    type Seen = (String, Vec<(String, String)>, Value);

    struct Mock {
        reply: Result<(u16, HashMap<String, String>, Value), String>,
        seen: RefCell<Vec<Seen>>,
    }

    impl Mock {
        fn new(reply: Result<(u16, HashMap<String, String>, Value), String>) -> Self {
            Self {
                reply,
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl JsonTransport for Mock {
        async fn post(
            &self,
            url: &str,
            headers: &[(&str, String)],
            body: &Value,
            _timeout: Duration,
        ) -> Result<(u16, HashMap<String, String>, Value), String> {
            self.seen.borrow_mut().push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                body.clone(),
            ));
            self.reply.clone()
        }
    }

    fn ok_reply(content: &str) -> Value {
        json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
    }

    #[test]
    fn reads_the_api_key_from_dotenv_variants() {
        let env =
            "# comment\nAPI_SERVER_ENABLED=true\nexport API_SERVER_KEY=\"abc 123\"\nOTHER=1\n";
        assert_eq!(env_value(env, "API_SERVER_KEY").as_deref(), Some("abc 123"));
        assert_eq!(
            env_value("API_SERVER_KEY='q'", "API_SERVER_KEY").as_deref(),
            Some("q")
        );
        assert_eq!(
            env_value("API_SERVER_KEY=raw # note", "API_SERVER_KEY").as_deref(),
            Some("raw")
        );
        assert_eq!(env_value("API_SERVER_KEY=", "API_SERVER_KEY"), None);
        assert_eq!(env_value("API_SERVER_KEYX=1", "API_SERVER_KEY"), None);
    }

    #[test]
    fn hermes_base_is_loopback_and_honours_the_port() {
        assert_eq!(hermes_base(""), "http://127.0.0.1:8642");
        assert_eq!(hermes_base("API_SERVER_PORT=9000"), "http://127.0.0.1:9000");
        assert_eq!(
            hermes_base("API_SERVER_PORT=evil.example"),
            "http://127.0.0.1:8642"
        );
    }

    #[test]
    fn session_key_override_must_be_a_safe_header() {
        assert_eq!(session_key(None), "nexus-chat");
        assert_eq!(session_key(Some("nexus-chat-test")), "nexus-chat-test");
        assert_eq!(session_key(Some("bad\r\nkey")), "nexus-chat");
        assert_eq!(session_key(Some("has space")), "nexus-chat");
    }

    #[test]
    fn deep_prefix_only_when_the_router_can_read_it() {
        assert_eq!(
            outgoing_message("plan my week", true, true),
            "/deep plan my week"
        );
        assert_eq!(outgoing_message("/deep plan", true, true), "/deep plan");
        assert_eq!(outgoing_message("/fast hi", true, true), "/fast hi");
        assert_eq!(outgoing_message("hi", false, true), "hi");
        assert_eq!(
            outgoing_message("/deep plan my week", true, false),
            "plan my week"
        );
        assert_eq!(outgoing_message("/DEEP\nplan", false, false), "plan");
        assert_eq!(route_prefix("/deeper thoughts"), None);
    }

    #[test]
    fn body_carries_system_full_history_and_the_new_message() {
        let history = vec![
            ChatTurn {
                role: "user".into(),
                content: "one".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "two".into(),
            },
            ChatTurn {
                role: "tool".into(),
                content: "dropped".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "  ".into(),
            },
        ];
        let body = chat_body("policy", "", &history, "three");
        let roles: Vec<_> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(body["messages"][3]["content"], "three");
        assert_eq!(body["stream"], false);
        assert!(
            body.get("model").is_none(),
            "Hermes picks its own model; Nexus never does"
        );
    }

    #[tokio::test]
    async fn sends_bearer_key_and_session_key_and_reads_the_session_id() {
        let mut headers = HashMap::new();
        headers.insert("x-hermes-session-id".to_string(), "api-123".to_string());
        let mock = Mock::new(Ok((200, headers, ok_reply("Hello."))));
        let reply = send_turn(
            &mock,
            "http://127.0.0.1:8642",
            "k3y",
            "nexus-chat-test",
            &json!({}),
        )
        .await
        .unwrap();
        assert_eq!(reply.content, "Hello.");
        assert_eq!(reply.session_id.as_deref(), Some("api-123"));
        assert!(reply.tier.is_none(), "no router metadata → no tier");
        let seen = mock.seen.borrow();
        assert_eq!(seen[0].0, "http://127.0.0.1:8642/v1/chat/completions");
        assert!(seen[0]
            .1
            .contains(&("Authorization".into(), "Bearer k3y".into())));
        assert!(seen[0]
            .1
            .contains(&("X-Hermes-Session-Key".into(), "nexus-chat-test".into())));
    }

    #[tokio::test]
    async fn errors_are_fixed_text_and_never_echo_the_key() {
        let down = Mock::new(Err("connect".into()));
        let e = send_turn(
            &down,
            "http://127.0.0.1:8642",
            "secret-key",
            "s",
            &json!({}),
        )
        .await
        .unwrap_err()
        .message;
        assert!(e.contains("not running"));
        for status in [401u16, 403] {
            let m = Mock::new(Ok((
                status,
                HashMap::new(),
                json!({"error": {"message": "bad key secret-key"}}),
            )));
            let e = send_turn(&m, "b", "secret-key", "s", &json!({}))
                .await
                .unwrap_err()
                .message;
            assert!(!e.contains("secret-key"));
            assert!(e.contains("API_SERVER_KEY"));
        }
        let m = Mock::new(Ok((
            502,
            HashMap::new(),
            json!({"error": {"message": "line1\nline2"}}),
        )));
        assert_eq!(
            send_turn(&m, "b", "k", "s", &json!({}))
                .await
                .unwrap_err()
                .message,
            HERMES_FAILED
        );
        let empty = Mock::new(Ok((200, HashMap::new(), ok_reply("  "))));
        assert!(send_turn(&empty, "b", "k", "s", &json!({})).await.is_err());
    }

    #[test]
    fn metadata_tier_is_read_if_hermes_ever_forwards_it() {
        let v = json!({"choices": [{"message": {"content": "x"}}], "amara": {"tier": "deep", "reason": "slash_deep"}});
        let t = tier_from_metadata(&v, None).unwrap();
        assert_eq!(
            (t.tier.as_str(), t.source.as_str()),
            ("deep", "hermes_metadata")
        );
        assert_eq!(
            tier_from_metadata(&json!({}), Some("fast")).unwrap().tier,
            "fast"
        );
        assert!(tier_from_metadata(&json!({"amara": {"tier": "huge"}}), None).is_none());
    }

    #[test]
    fn detects_hermes_pointed_at_the_router() {
        let r = "http://127.0.0.1:8787";
        assert!(base_url_is_router("http://127.0.0.1:8787/v1", r));
        assert!(base_url_is_router("http://localhost:8787/v1", r));
        assert!(!base_url_is_router("http://localhost:11434/v1", r));
        assert!(!base_url_is_router("http://10.0.0.2:8787/v1", r));
        assert!(!base_url_is_router("", r));
    }

    #[tokio::test]
    async fn dry_run_posts_the_same_body_and_reads_the_tier() {
        let mock = Mock::new(Ok((
            200,
            HashMap::new(),
            json!({"tier": "deep", "reason": "slash_deep", "routing_ms": 0}),
        )));
        let body = json!({"messages": [{"role": "user", "content": "/deep plan"}]});
        let t = dry_run_tier(&mock, "http://127.0.0.1:8787", &body)
            .await
            .unwrap();
        assert_eq!(
            (t.tier.as_str(), t.reason.as_str(), t.source.as_str()),
            ("deep", "slash_deep", "router_dry_run")
        );
        assert_eq!(mock.seen.borrow()[0].0, "http://127.0.0.1:8787/amara/route");
        assert_eq!(mock.seen.borrow()[0].2, body);
        let down = Mock::new(Err("connect".into()));
        assert!(dry_run_tier(&down, "http://127.0.0.1:8787", &body)
            .await
            .is_none());
        let forbidden = Mock::new(Ok((403, HashMap::new(), json!({"tier": "fast"}))));
        assert!(dry_run_tier(&forbidden, "http://127.0.0.1:8787", &body)
            .await
            .is_none());
    }

    #[test]
    fn route_log_confirms_the_served_tier_in_the_turn_window() {
        let log = [
            r#"{"ts":"2026-11-04T10:00:00.000Z","api":"openai","route":"deep","reason":"before"}"#,
            r#"{"ts":"2026-11-04T10:00:05.000Z","api":"openai","route":"fast","reason":"classifier_easy"}"#,
            r#"{"ts":"2026-11-04T10:00:06.000Z","api":"ollama","route":"deep","reason":"home assistant"}"#,
            r#"{"ts":"2026-11-04T10:00:07.000Z","api":"openai","route":"deep","status":"error"}"#,
            "not json",
        ]
        .join("\n");
        let since = "2026-11-04T10:00:01Z";
        let until = "2026-11-04T10:00:09Z";
        let t = tier_from_route_log(&log, since, until, None).unwrap();
        assert_eq!(
            (t.tier.as_str(), t.reason.as_str(), t.source.as_str()),
            ("fast", "classifier_easy", "router_log")
        );
        assert!(
            tier_from_route_log(&log, "2026-11-04T11:00:00Z", "2026-11-04T11:01:00Z", None)
                .is_none()
        );
    }

    #[test]
    fn mixed_tiers_in_the_window_need_the_dry_run_to_pick() {
        let log = [
            r#"{"ts":"2026-11-04T10:00:02Z","api":"openai","route":"fast","reason":"classifier_easy"}"#,
            r#"{"ts":"2026-11-04T10:00:03Z","api":"openai","route":"deep","reason":"slash_deep"}"#,
        ]
        .join("\n");
        let (s, u) = ("2026-11-04T10:00:00Z", "2026-11-04T10:00:09Z");
        assert!(tier_from_route_log(&log, s, u, None).is_none());
        let predicted = AmaraTier {
            tier: "deep".into(),
            reason: "slash_deep".into(),
            source: "router_dry_run".into(),
        };
        assert_eq!(
            tier_from_route_log(&log, s, u, Some(&predicted))
                .unwrap()
                .tier,
            "deep"
        );
        let claude = AmaraTier {
            tier: "claude".into(),
            ..predicted
        };
        assert!(tier_from_route_log(&log, s, u, Some(&claude)).is_none());
    }

    #[test]
    fn a_conversation_keeps_its_prefix_while_context_refreshes_each_turn() {
        let bridge = NexusRuntimeBridge::default();
        assert_eq!(bridge.system_prompt_for("c1", || "policy".into()), "policy");
        assert_eq!(
            bridge.system_prompt_for("c1", || "changed".into()),
            "policy"
        );
        assert_eq!(bridge.system_prompt_for("c2", || "other".into()), "other");
        // Same prefix and first user message on every turn (one Hermes session), but each
        // turn carries its own fresh context right before the new message.
        let history = vec![
            ChatTurn {
                role: "user".into(),
                content: "one".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "two".into(),
            },
        ];
        let t1 = chat_body(
            "policy",
            "<nexus_priorities>\nold\n</nexus_priorities>",
            &[],
            "one",
        );
        let t2 = chat_body(
            "policy",
            "<nexus_priorities>\nnew\n</nexus_priorities>",
            &history,
            "three",
        );
        assert_eq!(t1["messages"][0], t2["messages"][0]);
        let first_user = |b: &Value| {
            b["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["role"] == "user")
                .cloned()
        };
        assert_eq!(first_user(&t1), first_user(&t2));
        let msgs = t2["messages"].as_array().unwrap();
        let n = msgs.len();
        assert_eq!(msgs[n - 2]["role"], "system");
        assert!(msgs[n - 2]["content"].as_str().unwrap().contains("new"));
        assert!(!t2.to_string().contains("old"));
        assert_eq!(msgs[n - 1]["content"], "three");
        // No context: no extra system message.
        let plain = chat_body("policy", "  ", &[], "hi");
        assert_eq!(plain["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn route_log_tail_drops_a_partial_first_line() {
        let dir = std::env::temp_dir().join(format!("nexus-route-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("routes.jsonl");
        let line = format!("{{\"pad\":\"{}\"}}\n", "x".repeat(1000));
        let mut text = String::new();
        while (text.len() as u64) < ROUTE_LOG_TAIL_BYTES + 5_000 {
            text.push_str(&line);
        }
        text.push_str("{\"last\":true}\n");
        std::fs::write(&path, &text).unwrap();
        let tail = read_route_log_tail(&path);
        assert!(tail
            .lines()
            .all(|l| serde_json::from_str::<Value>(l).is_ok()));
        assert!(tail.ends_with("{\"last\":true}\n"));
        assert_eq!(read_route_log_tail(&dir.join("missing")), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live: one short Hermes turn through the real bridge (reads ~/.hermes/.env, never prints it).
    /// `NEXUS_HERMES_SESSION_KEY=nexus-chat-test cargo test live_hermes -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "live: needs Hermes on 127.0.0.1:8642"]
    async fn live_hermes_turn() {
        let home = dirs::home_dir().unwrap().join(".hermes");
        let bridge = NexusRuntimeBridge::default();
        let started = std::time::Instant::now();
        let reply = bridge
            .request(
                &home,
                HermesTurn {
                    system: "Amara OS live check. Answer in one short sentence.".into(),
                    context: String::new(),
                    history: &[],
                    message: "Say hello to Taisun in five words or fewer.",
                },
            )
            .await
            .expect("Hermes answered");
        println!(
            "hermes ok in {} ms, session={:?}, tier={:?}, reply={:?}",
            started.elapsed().as_millis(),
            reply.session_id,
            reply.tier,
            reply.content
        );
        assert!(!reply.content.is_empty());
    }

    /// Live: the router's dry run and its route log agree on Fast for a short message, and
    /// `/deep` is Deep without loading the deep model. Needs the router on 127.0.0.1:8787 and
    /// `AMARA_ROUTE_LOG` pointing at its routes.jsonl.
    #[tokio::test]
    #[ignore = "live: needs amara-router on 127.0.0.1:8787"]
    async fn live_router_tiers() {
        let t = HttpTransport::default();
        let base = "http://127.0.0.1:8787";
        let deep = chat_body(
            "policy",
            "",
            &[],
            &outgoing_message("plan my week", true, true),
        );
        let d = dry_run_tier(&t, base, &deep).await.expect("router up");
        println!("deep toggle dry run: {d:?}");
        assert_eq!((d.tier.as_str(), d.reason.as_str()), ("deep", "slash_deep"));

        let short = chat_body("policy", "", &[], "Reply with the single word OK.");
        let predicted = dry_run_tier(&t, base, &short).await.expect("router up");
        println!("short dry run: {predicted:?}");
        let since = chrono::Utc::now().to_rfc3339();
        let (status, headers, v) = t
            .post(
                &format!("{base}/v1/chat/completions"),
                &[],
                &short,
                Duration::from_secs(300),
            )
            .await
            .expect("router answered");
        let until = chrono::Utc::now().to_rfc3339();
        println!(
            "router HTTP {status}, X-Amara-Tier={:?}, amara={}, content={:?}",
            headers.get("x-amara-tier"),
            v["amara"],
            v["choices"][0]["message"]["content"]
        );
        let log_path = std::env::var("AMARA_ROUTE_LOG").expect("AMARA_ROUTE_LOG");
        let served = tier_from_route_log(
            &read_route_log_tail(Path::new(&log_path)),
            &since,
            &until,
            Some(&predicted),
        )
        .expect("route log line for this turn");
        println!("route log: {served:?}");
        assert_eq!(served.tier, "fast");
        assert_eq!(
            headers.get("x-amara-tier").map(String::as_str),
            Some("fast")
        );
    }

    fn hdrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn route_info_reads_headers_with_json_model_fallback() {
        let h = hdrs(&[
            ("x-amara-route", "deep"),
            ("x-amara-model", "qwen3:32b"),
            ("x-amara-route-reason", "slash_deep"),
        ]);
        let v = json!({"model": "other"});
        assert_eq!(
            route_info(&h, &v),
            Some(RouteInfo {
                tier: "deep".into(),
                model: Some("qwen3:32b".into()),
                reason: Some("slash_deep".into()),
            })
        );
        let h = hdrs(&[("x-amara-route", "fast")]);
        let r = route_info(&h, &json!({"model": "llama3.2:3b"})).unwrap();
        assert_eq!(r.model.as_deref(), Some("llama3.2:3b"));
        assert!(r.reason.is_none());
        let out = serde_json::to_string(&r).unwrap();
        assert_eq!(out, r#"{"tier":"fast","model":"llama3.2:3b"}"#);
    }

    #[test]
    fn route_info_keeps_a_fallback_reason() {
        let h = hdrs(&[
            ("x-amara-route", "fast"),
            ("x-amara-route-reason", "slash_deep; deep_failed"),
        ]);
        let r = route_info(&h, &Value::Null).unwrap();
        assert_eq!(r.reason.as_deref(), Some("slash_deep; deep_failed"));
    }

    #[test]
    fn route_log_skips_cancelled_lines() {
        let log = [
            r#"{"ts":"2026-11-04T10:00:01Z","api":"openai","route":"deep","reason":"slash_deep","status":"cancelled"}"#,
            r#"{"ts":"2026-11-04T10:00:02Z","api":"openai","route":"fast","reason":"classifier_easy","status":"ok"}"#,
        ]
        .join("\n");
        let t = tier_from_route_log(&log, "2026-11-04T10:00:00Z", "2026-11-04T10:00:03Z", None)
            .unwrap();
        assert_eq!(t.tier, "fast");
    }

    #[test]
    fn route_info_is_sanitized() {
        assert!(route_info(&hdrs(&[("x-amara-route", "turbo")]), &Value::Null).is_none());
        assert!(route_info(&HashMap::new(), &json!({"model": "m"})).is_none());
        let long = "m".repeat(129);
        let h = hdrs(&[
            ("x-amara-route", "claude"),
            ("x-amara-model", &long),
            ("x-amara-route-reason", "<script>"),
        ]);
        let r = route_info(&h, &json!({"model": "bad model\n"})).unwrap();
        assert_eq!(r.tier, "claude");
        assert!(r.model.is_none() && r.reason.is_none());
    }

    #[test]
    fn status_503_is_model_loading_and_429_stays_busy() {
        let e = |status, h: &[(&str, &str)]| {
            parse_hermes_reply(status, &hdrs(h), &Value::Null).unwrap_err()
        };
        assert_eq!(
            e(503, &[("retry-after", "42")]),
            "model_loading: retry_after=42"
        );
        assert_eq!(e(503, &[]), "model_loading: retry_after=10");
        assert_eq!(
            e(503, &[("retry-after", "0")]),
            "model_loading: retry_after=1"
        );
        assert_eq!(
            e(503, &[("retry-after", "9999")]),
            "model_loading: retry_after=300"
        );
        assert_eq!(
            e(503, &[("retry-after", "1.5")]),
            "model_loading: retry_after=10"
        );
        assert_eq!(
            e(503, &[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]),
            "model_loading: retry_after=10"
        );
        assert_eq!(e(429, &[("retry-after", "42")]), HERMES_BUSY);
    }

    #[tokio::test]
    async fn error_responses_keep_the_route() {
        let h = hdrs(&[
            ("x-amara-route", "deep"),
            ("x-amara-model", "qwen3:32b"),
            ("retry-after", "20"),
        ]);
        for (status, msg) in [
            (503u16, "model_loading: retry_after=20"),
            (502, HERMES_FAILED),
        ] {
            let m = Mock::new(Ok((status, h.clone(), json!({"error": {}}))));
            let e = send_turn(&m, "b", "k", "s", &json!({})).await.unwrap_err();
            assert_eq!(e.message, msg);
            let route = e.route.unwrap();
            assert_eq!(
                (route.tier.as_str(), route.model.as_deref()),
                ("deep", Some("qwen3:32b"))
            );
        }
        let ok = Mock::new(Ok((
            200,
            hdrs(&[("x-amara-route", "fast")]),
            ok_reply("hi"),
        )));
        let reply = send_turn(&ok, "b", "k", "s", &json!({})).await.unwrap();
        assert_eq!(reply.route.unwrap().tier, "fast");
    }
}
