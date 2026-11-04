/**
 * LLM Router — thin client for the Amara Router (`amara-router/`, default
 * http://127.0.0.1:8787). The router is the ONLY place that picks a model: fast (resident) or
 * deep (on demand), plus the optional cloud tier. This module only:
 *   - builds the one Amara system prompt (Hermes SOUL.md persona + identity guard),
 *   - forwards caller hints as headers (deep toggle, forced provider, tags, compute mode),
 *   - forwards the "Allow Claude" gate as `X-Amara-Allow-Claude: 0|1`,
 *   - logs usage (tier, tokens, cost) locally.
 *
 * Compute modes (all go through the router):
 *   amara  (default) — router decides; Claude never requested.
 *   local            — same as amara (saved "gateway" maps here too).
 *   hybrid / api     — the router is asked for Claude (api: always; hybrid: escalation tags).
 *                      Still only when Settings › Allow Claude is on, NEXUS_OUTBOUND_DISABLED
 *                      is unset and the daily cap is not spent; otherwise the app sends
 *                      `X-Amara-Allow-Claude: 0` and the router answers deep instead. The
 *                      router also needs its own cloud flag and key (off by default).
 *
 * Responses and usage carry the tier only (light|deep|claude), never a model or provider name.
 *
 * Usage logging is written to:
 *   ~/Library/Application Support/com.taisun.nexus/llm_usage.json
 */
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RouterConfig {
    pub mode: String, // "amara" | "local" | "hybrid" | "api"
    pub ollama_model: String,
    pub anthropic_model: String,
    pub token_threshold: u32,
    pub daily_cap_usd: Option<f64>,
    pub per_task_cap_usd: Option<f64>,
    pub show_cost_estimate: bool,
    /// Amara Router base URL. Loopback only; None or anything else = DEFAULT_ROUTER_URL.
    #[serde(default)]
    pub router_url: Option<String>,
}

/// Telegram chat's model (gemma4-only, 576e5a2); not used by the router client.
pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:latest";
pub const DEFAULT_ROUTER_URL: &str = "http://127.0.0.1:8787";
pub const DEFAULT_MODE: &str = "amara";
/// Retired OpenClaw compute mode; treated like "local".
const RETIRED_GATEWAY_MODE: &str = "gateway";
const PERSONA_MAX_BYTES: u64 = 64 * 1024;
const ROUTER_DOWN: &str =
    "Amara isn't responding right now. Check that the Amara router is running.";
const OLLAMA_DOWN: &str = "Amara isn't responding right now. Check that Ollama is running.";
const ROUTER_FAILED: &str = "Amara couldn't answer right now. Details are in the Amara router log.";

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            mode: DEFAULT_MODE.to_string(),
            ollama_model: DEFAULT_OLLAMA_MODEL.to_string(),
            anthropic_model: "claude-haiku-4-5-20251001".to_string(),
            token_threshold: 8000,
            daily_cap_usd: Some(5.0),
            per_task_cap_usd: Some(0.10),
            show_cost_estimate: true,
            router_url: None,
        }
    }
}

/// Amara's tier as the app reports it. The router's `fast` is shown as `light`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Light,
    Deep,
    Claude,
}

impl Tier {
    /// The router's `amara.tier` / `X-Amara-Tier` value.
    pub fn from_router(s: &str) -> Tier {
        match s {
            "deep" => Tier::Deep,
            "claude" => Tier::Claude,
            _ => Tier::Light,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RouterResponse {
    pub content: String,
    pub tier: Tier,
    pub latency_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub estimated_cost_usd: f64,
    pub routing_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmUsageRecord {
    pub id: String,
    pub timestamp: u64,
    /// Older records carried provider/model instead; they read back as light (serde ignores
    /// the unknown fields).
    #[serde(default)]
    pub tier: Tier,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub estimated_cost_usd: f64,
    pub latency_ms: u64,
    pub routing_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CostEstimate {
    pub tier: Tier,
    pub estimated_input_tokens: u32,
    pub estimated_cost_usd: f64,
    pub routing_reason: String,
}

// ─── Storage helpers ──────────────────────────────────────────────────────────

/// `{app data}/llm_usage.json`; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn usage_log_path() -> PathBuf {
    crate::app_paths::data_dir().join("llm_usage.json")
}

fn load_usage_records() -> Vec<LlmUsageRecord> {
    let path = usage_log_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn append_usage_record(record: LlmUsageRecord) {
    let mut records = load_usage_records();
    records.push(record);
    if records.len() > 1000 {
        let drain = records.len() - 1000;
        records.drain(0..drain);
    }
    if let Ok(json) = serde_json::to_string_pretty(&records) {
        let path = usage_log_path();
        if let Some(parent) = path.parent() {
            let _ = crate::app_paths::create_private_dir_all(parent);
        }
        let _ = crate::app_paths::write_private(&path, json);
    }
}

// ─── Cost / token helpers ─────────────────────────────────────────────────────

/// Rough estimate: ~1 token per 4 characters.
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as f64 / 4.0).ceil() as u32
}

/// Returns (input_cost_per_1k, output_cost_per_1k) in USD for Anthropic models.
fn anthropic_cost_per_1k(model: &str) -> (f64, f64) {
    if model.contains("opus") {
        (0.015, 0.075)
    } else if model.contains("sonnet") {
        (0.003, 0.015)
    } else {
        // haiku / unknown — use cheapest
        (0.00025, 0.00125)
    }
}

pub fn estimate_cost(provider: &str, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    if provider != "anthropic" {
        return 0.0;
    }
    let (ir, or_) = anthropic_cost_per_1k(model);
    (input_tokens as f64 / 1000.0) * ir + (output_tokens as f64 / 1000.0) * or_
}

fn tier_cost(tier: Tier, config: &RouterConfig, input_tokens: u32, output_tokens: u32) -> f64 {
    match tier {
        Tier::Claude => estimate_cost(
            "anthropic",
            &config.anthropic_model,
            input_tokens,
            output_tokens,
        ),
        Tier::Light | Tier::Deep => 0.0,
    }
}

fn daily_spend_usd(records: &[LlmUsageRecord]) -> f64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let day_ms = 86_400_000u64;
    records
        .iter()
        .filter(|r| now_ms.saturating_sub(r.timestamp) < day_ms)
        .map(|r| r.estimated_cost_usd)
        .sum()
}

// ─── The Claude gate (forwarded to the router) ────────────────────────────────

/// Everything the app must allow before the router may use the cloud. The router adds its own
/// checks (cloud flag, key, its own NEXUS_OUTBOUND_DISABLED).
#[derive(Clone, Copy, Debug)]
pub struct ClaudeGate {
    pub allow_claude: bool,
    pub outbound_disabled: bool,
    pub local_mode: bool,
    pub over_daily_cap: bool,
}

impl ClaudeGate {
    /// The first failing check, or None when the router may be allowed to call Claude.
    pub fn blocked(&self) -> Option<&'static str> {
        if self.outbound_disabled {
            Some("outbound_disabled")
        } else if self.local_mode {
            Some("local_mode")
        } else if !self.allow_claude {
            Some("claude_not_allowed")
        } else if self.over_daily_cap {
            Some("daily_cap")
        } else {
            None
        }
    }
}

fn is_cloud_mode(mode: &str) -> bool {
    mode == "api" || mode == "hybrid"
}

fn claude_gate(config: &RouterConfig, daily_spent: f64) -> ClaudeGate {
    ClaudeGate {
        allow_claude: crate::commands::telegram::load_config()
            .allow_claude
            .unwrap_or(false),
        outbound_disabled: crate::commands::telegram::outbound_disabled(),
        local_mode: !is_cloud_mode(&config.mode),
        over_daily_cap: config.daily_cap_usd.is_some_and(|cap| daily_spent >= cap),
    }
}

// ─── Request building ─────────────────────────────────────────────────────────

/// The router base URL: the caller's, else the saved one, else the default. Loopback only, so
/// the app never sends notes to another machine through this setting.
pub fn router_base(passed: Option<&str>, saved: Option<&str>) -> String {
    [passed, saved]
        .into_iter()
        .flatten()
        .find_map(loopback_base)
        .unwrap_or_else(|| DEFAULT_ROUTER_URL.to_string())
}

/// `http://<loopback host>[:port]`, rebuilt from the parsed URL, or None. Parsing (rather than
/// splitting the string) closes userinfo tricks like `http://127.0.0.1@evil.example` and
/// `http://localhost:1@evil.example:8787`; any path, query or fragment is refused too.
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

/// The router is loopback-only, so a redirect from it is never legitimate: don't follow one off the Mac.
fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// Routing hints for the router as header pairs. The router decides; these only ask.
pub fn router_headers(
    config: &RouterConfig,
    tags: &[String],
    deep_toggle: bool,
    force_provider: Option<&str>,
    gate: &ClaudeGate,
) -> Vec<(&'static str, String)> {
    let mode = match config.mode.as_str() {
        RETIRED_GATEWAY_MODE | "" => "local",
        m => m,
    };
    let mut h = vec![
        (
            "X-Amara-Allow-Claude",
            if gate.blocked().is_none() { "1" } else { "0" }.to_string(),
        ),
        ("X-Amara-Mode", mode.to_string()),
    ];
    let route = match force_provider {
        Some("anthropic" | "claude") => Some("claude"),
        Some("deep") => Some("deep"),
        Some("light" | "fast" | "ollama" | RETIRED_GATEWAY_MODE) => Some("fast"),
        _ if deep_toggle => Some("deep"),
        _ => None,
    };
    if let Some(r) = route {
        h.push(("X-Amara-Route", r.to_string()));
    }
    let tags: Vec<&str> = tags
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && !t.contains([',', '\r', '\n']))
        .collect();
    if !tags.is_empty() {
        h.push(("X-Amara-Tags", tags.join(",")));
    }
    h
}

/// OpenAI chat body: the one system prompt + the user text, non-streamed.
pub fn router_body(system: &str, text: &str) -> Value {
    json!({
        "model": "amara",
        "stream": false,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": text}
        ]
    })
}

/// (content, tier, reason, prompt_tokens, completion_tokens) from the router's JSON reply.
pub fn parse_router_reply(v: &Value) -> Result<(String, Tier, String, u32, u32), String> {
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("router reply has no content")?;
    Ok((
        content.trim().to_string(),
        Tier::from_router(v["amara"]["tier"].as_str().unwrap_or("fast")),
        v["amara"]["reason"].as_str().unwrap_or("").to_string(),
        v["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        v["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
    ))
}

// ─── Identity ─────────────────────────────────────────────────────────────────

const IDENTITY_GUARD: &str = "You are Amara, and only Amara, whichever model runs you. Never say you are Gemma, Qwen, Claude, or any other model, and never name a model or AI company as yourself. If asked what model you are, say you are Amara.";
const FALLBACK_PERSONA: &str =
    "You are Amara, Taisun's private assistant inside Amara OS. Be direct, warm and concise.";

/// The Amara persona from the same source as Hermes (`~/.hermes/SOUL.md`), capped at 64 KiB,
/// with a short inline fallback.
fn amara_persona() -> String {
    dirs::home_dir()
        .map(|h| h.join(".hermes").join("SOUL.md"))
        .and_then(|p| {
            let meta = fs::metadata(&p).ok()?;
            (meta.is_file() && meta.len() <= PERSONA_MAX_BYTES).then_some(p)
        })
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| FALLBACK_PERSONA.to_string())
}

/// The one system prompt every tier gets.
pub fn system_prompt(persona: &str) -> String {
    format!("{persona}\n\n{IDENTITY_GUARD}")
}

// ─── Transport ────────────────────────────────────────────────────────────────

/// POSTs JSON to the router. Tests swap in a mock to see exactly what is sent.
pub(crate) trait RouterTransport {
    /// Ok((status, body)); Err on connection failure.
    async fn post(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
        body: &Value,
        timeout_secs: u64,
    ) -> Result<(u16, Value), String>;
}

struct HttpTransport {
    client: reqwest::Client,
}

impl RouterTransport for HttpTransport {
    async fn post(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
        body: &Value,
        timeout_secs: u64,
    ) -> Result<(u16, Value), String> {
        let mut req = self
            .client
            .post(url)
            .json(body)
            .timeout(std::time::Duration::from_secs(timeout_secs));
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("router unreachable: {e}"))?;
        let status = resp.status().as_u16();
        let v = resp.json().await.unwrap_or(Value::Null);
        Ok((status, v))
    }
}

/// One chat turn through the router. Errors are logged with detail; the caller only ever
/// sees fixed text, so no model or provider name leaks into the UI.
pub(crate) async fn ask_router(
    transport: &impl RouterTransport,
    base: &str,
    headers: &[(&'static str, String)],
    body: &Value,
) -> Result<(String, Tier, String, u32, u32), String> {
    let url = format!("{base}/v1/chat/completions");
    // Generous: the deep model may have to load first.
    let (status, v) = transport
        .post(&url, headers, body, 600)
        .await
        .map_err(|e| {
            eprintln!("[llm_router] {e}");
            ROUTER_DOWN.to_string()
        })?;
    if !(200..300).contains(&status) {
        let detail = v["error"]["message"].as_str().unwrap_or("");
        eprintln!("[llm_router] router HTTP {status}: {detail}");
        return Err(if status == 503 {
            OLLAMA_DOWN
        } else {
            ROUTER_FAILED
        }
        .to_string());
    }
    parse_router_reply(&v).map_err(|e| {
        eprintln!("[llm_router] {e}");
        ROUTER_FAILED.to_string()
    })
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn chat_with_router(
    text: String,
    config: RouterConfig,
    tags: Vec<String>,
    force_provider: Option<String>,
    anthropic_api_key: Option<String>,
    deep: Option<bool>,
) -> Result<RouterResponse, String> {
    // The cloud key now lives in the router's 0600 secrets file; a passed key is ignored.
    let _ = anthropic_api_key;
    let saved = crate::commands::telegram::load_config();
    let base = router_base(
        config.router_url.as_deref(),
        saved.amara_router_url.as_deref(),
    );
    let gate = claude_gate(&config, daily_spend_usd(&load_usage_records()));
    let headers = router_headers(
        &config,
        &tags,
        deep.unwrap_or(false),
        force_provider.as_deref(),
        &gate,
    );
    let body = router_body(&system_prompt(&amara_persona()), &text);

    let start = Instant::now();
    let transport = HttpTransport {
        client: no_redirect_client(),
    };
    let (content, tier, mut reason, prompt_tokens, completion_tokens) =
        ask_router(&transport, &base, &headers, &body).await?;
    let latency_ms = start.elapsed().as_millis() as u64;
    let cost = tier_cost(tier, &config, prompt_tokens, completion_tokens);
    if let Some(block) = gate.blocked().filter(|_| is_cloud_mode(&config.mode)) {
        if !reason.contains("claude_blocked") {
            reason.push_str(&format!("; app_claude_gate={block}"));
        }
    }

    append_usage_record(LlmUsageRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        tier,
        prompt_tokens,
        completion_tokens,
        estimated_cost_usd: cost,
        latency_ms,
        routing_reason: reason.clone(),
    });

    Ok(RouterResponse {
        content,
        tier,
        latency_ms,
        prompt_tokens,
        completion_tokens,
        estimated_cost_usd: cost,
        routing_reason: reason,
    })
}

/// Asks the router's dry-run endpoint which tier it would use (may run its classifier).
#[tauri::command]
pub async fn get_cost_estimate(
    text: String,
    config: RouterConfig,
    tags: Vec<String>,
    deep: Option<bool>,
) -> Result<CostEstimate, String> {
    let estimated_tokens = estimate_tokens(&text);
    let saved = crate::commands::telegram::load_config();
    let base = router_base(
        config.router_url.as_deref(),
        saved.amara_router_url.as_deref(),
    );
    let gate = claude_gate(&config, daily_spend_usd(&load_usage_records()));
    let headers = router_headers(&config, &tags, deep.unwrap_or(false), None, &gate);
    let transport = HttpTransport {
        client: no_redirect_client(),
    };
    let (tier, reason) = match transport
        .post(
            &format!("{base}/amara/route"),
            &headers,
            &json!({ "text": text }),
            5,
        )
        .await
    {
        Ok((200, v)) => (
            Tier::from_router(v["tier"].as_str().unwrap_or("fast")),
            v["reason"].as_str().unwrap_or("").to_string(),
        ),
        _ => (Tier::Light, "router_unreachable".to_string()),
    };
    Ok(CostEstimate {
        tier,
        estimated_input_tokens: estimated_tokens,
        estimated_cost_usd: tier_cost(tier, &config, estimated_tokens, estimated_tokens / 2),
        routing_reason: reason,
    })
}

/// Settings › "Allow Claude". Only this flag crosses the bridge; keys and the rest of the
/// saved config never do.
#[tauri::command]
pub fn get_allow_claude() -> bool {
    crate::commands::telegram::load_config()
        .allow_claude
        .unwrap_or(false)
}

#[tauri::command]
pub fn set_allow_claude(enabled: bool) -> Result<bool, String> {
    crate::commands::telegram::update_config(|c| c.allow_claude = Some(enabled))?;
    Ok(enabled)
}

#[tauri::command]
pub fn get_llm_usage() -> Result<Vec<LlmUsageRecord>, String> {
    Ok(load_usage_records())
}

#[tauri::command]
pub fn clear_llm_usage() -> Result<(), String> {
    let path = usage_log_path();
    if let Some(parent) = path.parent() {
        let _ = crate::app_paths::create_private_dir_all(parent);
    }
    crate::app_paths::write_private(&path, "[]").map_err(|e| e.to_string())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn cfg(mode: &str) -> RouterConfig {
        RouterConfig {
            mode: mode.to_string(),
            ..RouterConfig::default()
        }
    }

    fn open_gate() -> ClaudeGate {
        ClaudeGate {
            allow_claude: true,
            outbound_disabled: false,
            local_mode: false,
            over_daily_cap: false,
        }
    }

    fn header<'a>(h: &'a [(&'static str, String)], k: &str) -> Option<&'a str> {
        h.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str())
    }

    /// (url, headers, body) of one request.
    type Seen = (String, Vec<(&'static str, String)>, Value);

    /// Records every request and answers with a canned status/body.
    struct MockTransport {
        reply: Result<(u16, Value), String>,
        seen: RefCell<Vec<Seen>>,
    }

    impl MockTransport {
        fn new(reply: Result<(u16, Value), String>) -> Self {
            Self {
                reply,
                seen: RefCell::new(vec![]),
            }
        }
    }

    impl RouterTransport for MockTransport {
        async fn post(
            &self,
            url: &str,
            headers: &[(&'static str, String)],
            body: &Value,
            _t: u64,
        ) -> Result<(u16, Value), String> {
            self.seen
                .borrow_mut()
                .push((url.to_string(), headers.to_vec(), body.clone()));
            self.reply.clone()
        }
    }

    fn router_ok(tier: &str) -> Value {
        json!({
            "choices": [{"message": {"role": "assistant", "content": " hi "}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2},
            "amara": {"tier": tier, "reason": "classifier_easy", "routing_ms": 300}
        })
    }

    // ── defaults ──

    #[test]
    fn default_mode_is_amara_via_the_router() {
        let c = RouterConfig::default();
        assert_eq!(c.mode, "amara");
        assert_eq!(router_base(None, None), "http://127.0.0.1:8787");
        // A config saved by an older build (no router_url) still deserialises.
        let old: RouterConfig = serde_json::from_value(json!({
            "mode": "local", "ollama_model": "gemma4:latest", "anthropic_model": "x",
            "token_threshold": 1, "daily_cap_usd": null, "per_task_cap_usd": null,
            "show_cost_estimate": true
        }))
        .unwrap();
        assert!(old.router_url.is_none());
    }

    #[test]
    fn router_url_is_configurable_but_loopback_only() {
        assert_eq!(
            router_base(Some("http://127.0.0.1:9999/"), None),
            "http://127.0.0.1:9999"
        );
        assert_eq!(
            router_base(None, Some("http://localhost:8788")),
            "http://localhost:8788"
        );
        assert_eq!(
            router_base(Some("http://192.168.1.5:8787"), None),
            DEFAULT_ROUTER_URL
        );
        assert_eq!(
            router_base(Some("https://evil.example"), Some("http://127.0.0.1:1")),
            "http://127.0.0.1:1"
        );
        assert_eq!(
            router_base(Some("http://127.0.0.1.evil.com"), None),
            DEFAULT_ROUTER_URL
        );
        for trick in [
            "http://127.0.0.1@evil.example",
            "http://localhost:1@evil.example:8787",
            "http://user:pw@127.0.0.1:8787",
            "http://127.0.0.1:8787/v1?x=1",
            "http://127.0.0.1:8787#frag",
        ] {
            assert_eq!(
                router_base(Some(trick), None),
                "http://127.0.0.1:8787",
                "{trick}"
            );
        }
        assert_eq!(
            router_base(Some("http://[::1]:8787"), None),
            "http://[::1]:8787"
        );
    }

    // ── hints forwarded (no model decisions in the app) ──

    #[test]
    fn plain_request_sends_no_route_hint() {
        let h = router_headers(&cfg("amara"), &[], false, None, &open_gate());
        assert_eq!(header(&h, "X-Amara-Route"), None);
        assert_eq!(header(&h, "X-Amara-Mode"), Some("amara"));
        // Long or multi-step text is the router's call now; the app adds nothing.
        let b = router_body("sys", &"1. a\n2. b\n3. c ".repeat(500));
        assert_eq!(b["model"], "amara");
        assert_eq!(b["messages"][0]["content"], "sys");
    }

    #[test]
    fn deep_toggle_force_provider_and_tags_become_headers() {
        let g = open_gate();
        let c = cfg("amara");
        assert_eq!(
            header(&router_headers(&c, &[], true, None, &g), "X-Amara-Route"),
            Some("deep")
        );
        assert_eq!(
            header(
                &router_headers(&c, &[], false, Some("deep"), &g),
                "X-Amara-Route"
            ),
            Some("deep")
        );
        assert_eq!(
            header(
                &router_headers(&c, &[], true, Some("light"), &g),
                "X-Amara-Route"
            ),
            Some("fast")
        );
        assert_eq!(
            header(
                &router_headers(&c, &[], false, Some("anthropic"), &g),
                "X-Amara-Route"
            ),
            Some("claude")
        );
        let tags = vec!["heavy".to_string(), "bad,tag".to_string(), " ".to_string()];
        assert_eq!(
            header(&router_headers(&c, &tags, false, None, &g), "X-Amara-Tags"),
            Some("heavy")
        );
    }

    #[test]
    fn retired_gateway_mode_is_local() {
        let h = router_headers(&cfg("gateway"), &[], false, Some("gateway"), &open_gate());
        assert_eq!(header(&h, "X-Amara-Mode"), Some("local"));
        assert_eq!(header(&h, "X-Amara-Route"), Some("fast"));
    }

    // ── the Claude gate, forwarded ──

    type BreakGate = fn(&mut ClaudeGate);

    #[test]
    fn gate_blocks_each_missing_condition_and_forwards_zero() {
        assert_eq!(open_gate().blocked(), None);
        let h = router_headers(&cfg("api"), &[], false, Some("anthropic"), &open_gate());
        assert_eq!(header(&h, "X-Amara-Allow-Claude"), Some("1"));
        let cases: [(BreakGate, &str); 4] = [
            (|g| g.outbound_disabled = true, "outbound_disabled"),
            (|g| g.local_mode = true, "local_mode"),
            (|g| g.allow_claude = false, "claude_not_allowed"),
            (|g| g.over_daily_cap = true, "daily_cap"),
        ];
        for (break_it, want) in cases {
            let mut g = open_gate();
            break_it(&mut g);
            assert_eq!(g.blocked(), Some(want));
            // Forcing the provider never bypasses the gate.
            let h = router_headers(&cfg("api"), &[], false, Some("anthropic"), &g);
            assert_eq!(header(&h, "X-Amara-Allow-Claude"), Some("0"), "{want}");
        }
    }

    #[test]
    fn amara_and_local_modes_count_as_local_for_the_gate() {
        for m in ["amara", "local", "gateway", ""] {
            assert!(!is_cloud_mode(m));
        }
        assert!(is_cloud_mode("api") && is_cloud_mode("hybrid"));
    }

    // ── transport ──

    #[tokio::test]
    async fn sends_one_request_to_the_router_and_maps_the_tier() {
        let t = MockTransport::new(Ok((200, router_ok("fast"))));
        let headers = router_headers(&cfg("amara"), &[], false, None, &open_gate());
        let body = router_body("sys", "hi");
        let (content, tier, reason, p, c) =
            ask_router(&t, "http://127.0.0.1:8787", &headers, &body)
                .await
                .unwrap();
        assert_eq!(
            (content.as_str(), tier, reason.as_str(), p, c),
            ("hi", Tier::Light, "classifier_easy", 5, 2)
        );
        let seen = t.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "http://127.0.0.1:8787/v1/chat/completions");
        assert_eq!(seen[0].2["messages"][1]["content"], "hi");
        assert_eq!(Tier::from_router("deep"), Tier::Deep);
        assert_eq!(Tier::from_router("claude"), Tier::Claude);
    }

    #[tokio::test]
    async fn router_or_ollama_down_gives_fixed_text() {
        let body = router_body("s", "t");
        let down = MockTransport::new(Err("connection refused".into()));
        assert_eq!(
            ask_router(&down, "http://x", &[], &body).await.unwrap_err(),
            ROUTER_DOWN
        );
        let ollama = MockTransport::new(Ok((
            503,
            json!({"error": {"message": "Ollama is not reachable"}}),
        )));
        assert_eq!(
            ask_router(&ollama, "http://x", &[], &body)
                .await
                .unwrap_err(),
            OLLAMA_DOWN
        );
        let bad = MockTransport::new(Ok((502, json!({"error": {"message": "qwen3.8 failed"}}))));
        let err = ask_router(&bad, "http://x", &[], &body).await.unwrap_err();
        assert_eq!(err, ROUTER_FAILED);
        assert!(!err.contains("gemma") && !err.contains("qwen"));
    }

    // ── identity ──

    #[test]
    fn system_prompt_keeps_persona_and_forbids_model_names() {
        let p = system_prompt("PERSONA");
        assert!(p.starts_with("PERSONA"));
        assert!(p.contains("You are Amara"));
        for name in ["Gemma", "Qwen", "Claude"] {
            assert!(p.contains(name), "guard must name {name}");
        }
    }

    #[test]
    fn response_and_usage_carry_tier_not_model() {
        let r = RouterResponse {
            content: "x".into(),
            tier: Tier::Deep,
            latency_ms: 1,
            prompt_tokens: 1,
            completion_tokens: 1,
            estimated_cost_usd: 0.0,
            routing_reason: "slash_deep".into(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["tier"], "deep");
        assert!(json.get("model").is_none() && json.get("provider").is_none());
        let old: LlmUsageRecord = serde_json::from_str(
            r#"{"id":"a","timestamp":1,"provider":"ollama","model":"qwen3.5","prompt_tokens":1,"completion_tokens":1,"estimated_cost_usd":0,"latency_ms":1,"routing_reason":"mode=local"}"#,
        )
        .unwrap();
        assert_eq!(old.tier, Tier::Light);
    }

    // ── constants and costs ──

    #[test]
    fn constants() {
        assert_eq!(DEFAULT_OLLAMA_MODEL, "gemma4:latest");
        assert!(DEFAULT_ROUTER_URL.starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn estimate_tokens_rounds_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
    }

    #[test]
    fn costs() {
        assert_eq!(estimate_cost("ollama", "x", 1000, 500), 0.0);
        let haiku = estimate_cost("anthropic", "claude-haiku-4-5", 1000, 500);
        assert!((haiku - 0.000875).abs() < 1e-9);
        let sonnet = estimate_cost("anthropic", "claude-sonnet-4", 1000, 500);
        assert!((sonnet - 0.0105).abs() < 1e-9);
        let opus = estimate_cost("anthropic", "claude-opus-4", 1000, 500);
        assert!((opus - 0.0525).abs() < 1e-9);
        assert_eq!(tier_cost(Tier::Deep, &cfg("api"), 1000, 500), 0.0);
        assert!(tier_cost(Tier::Claude, &cfg("api"), 1000, 500) > 0.0);
    }
}
