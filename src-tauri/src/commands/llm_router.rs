/**
 * LLM Router — one Amara identity over three model tiers. Only the model changes.
 *
 * Tiers:
 *   light  (default) — Gemma 12B through Ollama at the fixed loopback address
 *   deep            — Qwen 3.5-family through Ollama at the same address; chosen by the Deep
 *                     toggle, "think deeper", or a simple heuristic (long or multi-step prompts)
 *   claude (opt-in) — Anthropic. Reachable only when "Allow Claude" is on in Settings (saved
 *                     config, not the caller's), NEXUS_OUTBOUND_DISABLED is not set, a key exists,
 *                     mode is not local and the daily cap is not spent. Forcing the provider
 *                     never skips these checks.
 *
 * Fallback: claude → deep → light; deep → light. Fallback never makes a network call.
 * Every tier gets the same system prompt (Hermes SOUL.md persona + identity guard), and
 * responses carry the tier only, never a model or provider name.
 *
 * The former "gateway" (OpenClaw) mode is retired: a saved "gateway" mode is treated as local.
 *
 * Usage logging is written to:
 *   ~/Library/Application Support/com.taisun.nexus/llm_usage.json
 */
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RouterConfig {
    pub mode: String, // "local" | "hybrid" | "api"
    pub ollama_model: String,
    pub anthropic_model: String,
    pub token_threshold: u32,
    pub daily_cap_usd: Option<f64>,
    pub per_task_cap_usd: Option<f64>,
    pub show_cost_estimate: bool,
}

/// Telegram chat's model (gemma4-only, 576e5a2). The chat tiers use LIGHT_MODEL / DEEP_MODEL.
pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:latest";
/// Light tier: Gemma 12B, the same model Hermes/Amara runs on.
pub const LIGHT_MODEL: &str = "gemma4-12b-uncensored:latest";
/// Deep tier: Qwen 3.5-family 27B (Ollama architecture `qwen35`).
pub const DEEP_MODEL: &str = "qwen3.8-uncensored:latest";
/// Fixed loopback Ollama endpoint for both local tiers (no env override, by design).
const OLLAMA_CHAT_URL: &str = "http://127.0.0.1:11434/api/chat";
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
/// Retired OpenClaw compute mode; mapped to local Ollama wherever it appears.
const RETIRED_GATEWAY_MODE: &str = "gateway";
/// Deep heuristic: prompts over ~1,600 characters.
pub const DEEP_TOKEN_THRESHOLD: u32 = 400;
/// Deep heuristic: this many numbered or bulleted lines make a multi-step task.
const DEEP_STEP_LINES: usize = 3;
const PERSONA_MAX_BYTES: u64 = 64 * 1024;
const UNAVAILABLE: &str = "Amara isn't responding right now. Check that Ollama is running.";

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            // Standing rule: local Ollama only.
            mode: "local".to_string(),
            ollama_model: DEFAULT_OLLAMA_MODEL.to_string(),
            anthropic_model: "claude-haiku-4-5-20251001".to_string(),
            token_threshold: 8000,
            daily_cap_usd: Some(5.0),
            per_task_cap_usd: Some(0.10),
            show_cost_estimate: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Light,
    Deep,
    Claude,
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

// ─── Tier selection ───────────────────────────────────────────────────────────

/// Why the deep tier is wanted, or None. Simple on purpose: the toggle, an explicit ask,
/// a long prompt, or a multi-step task.
pub fn deep_reason(text: &str, deep_toggle: bool) -> Option<&'static str> {
    if deep_toggle {
        return Some("deep_toggle");
    }
    let lower = text.to_lowercase();
    if [
        "think deeper",
        "think harder",
        "think deeply",
        "think more deeply",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        return Some("asked_to_think_deeper");
    }
    if estimate_tokens(text) > DEEP_TOKEN_THRESHOLD {
        return Some("long_prompt");
    }
    let step_lines = text
        .lines()
        .map(str::trim_start)
        .filter(|l| {
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            (digits > 0 && matches!(l[digits..].chars().next(), Some('.' | ')')))
                || l.starts_with("- ")
                || l.starts_with("* ")
                || l.starts_with("• ")
        })
        .count();
    if step_lines >= DEEP_STEP_LINES || lower.contains("step by step") {
        return Some("multi_step");
    }
    None
}

/// The tier asked for, before the Claude gate. Claude is only ever *requested* here.
pub fn requested_tier(
    config: &RouterConfig,
    text: &str,
    tags: &[String],
    deep_toggle: bool,
    force_provider: Option<&str>,
) -> (Tier, String) {
    match force_provider {
        Some("anthropic" | "claude") => return (Tier::Claude, "force_provider".to_string()),
        Some("deep") => return (Tier::Deep, "force_provider".to_string()),
        Some("light" | "ollama" | RETIRED_GATEWAY_MODE) => {
            return (Tier::Light, "force_provider".to_string())
        }
        _ => {}
    }
    let escalation_tag = tags
        .iter()
        .find(|t| ["heavy", "research", "code", "long"].contains(&t.as_str()));
    let claude_wanted = config.mode == "api"
        || (config.mode == "hybrid"
            && (escalation_tag.is_some() || estimate_tokens(text) > config.token_threshold));
    if claude_wanted {
        return (Tier::Claude, format!("mode={}", config.mode));
    }
    if let Some(reason) = deep_reason(text, deep_toggle) {
        return (Tier::Deep, reason.to_string());
    }
    if let Some(tag) = escalation_tag {
        return (Tier::Deep, format!("tag={tag}"));
    }
    (Tier::Light, "default".to_string())
}

/// Everything that must hold before a request may leave the Mac.
#[derive(Clone, Copy, Debug)]
pub struct ClaudeGate {
    pub allow_claude: bool,
    pub outbound_disabled: bool,
    pub has_key: bool,
    pub local_mode: bool,
    pub over_daily_cap: bool,
}

impl ClaudeGate {
    /// The first failing check, or None when Claude may be called.
    pub fn blocked(&self) -> Option<&'static str> {
        if self.outbound_disabled {
            Some("outbound_disabled")
        } else if self.local_mode {
            Some("local_mode")
        } else if !self.allow_claude {
            Some("claude_not_allowed")
        } else if !self.has_key {
            Some("no_key")
        } else if self.over_daily_cap {
            Some("daily_cap")
        } else {
            None
        }
    }
}

/// Tiers to try, in order. Claude appears only first, and only when the gate is open.
pub fn tier_plan(requested: Tier, gate: &ClaudeGate) -> Vec<Tier> {
    match requested {
        Tier::Claude if gate.blocked().is_none() => vec![Tier::Claude, Tier::Deep, Tier::Light],
        Tier::Claude | Tier::Deep => vec![Tier::Deep, Tier::Light],
        Tier::Light => vec![Tier::Light],
    }
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

// ─── Provider callers ─────────────────────────────────────────────────────────

fn strip_thinking_tags(s: &str) -> String {
    let mut result = s.to_string();
    while let (Some(start), Some(end)) = (result.find("<think>"), result.find("</think>")) {
        if start < end {
            result = format!("{}{}", &result[..start], &result[end + "</think>".len()..]);
        } else {
            break;
        }
    }
    result.trim().to_string()
}

/// (content, prompt_tokens, completion_tokens)
type Reply = (String, u32, u32);

/// The two transports. Tests swap in a mock to prove which requests are (not) made.
pub(crate) trait TierBackend {
    async fn ollama(&self, model: &str, system: &str, text: &str) -> Result<Reply, String>;
    async fn anthropic(
        &self,
        model: &str,
        system: &str,
        text: &str,
        key: &str,
    ) -> Result<Reply, String>;
}

struct HttpBackend {
    client: reqwest::Client,
}

impl TierBackend for HttpBackend {
    async fn ollama(&self, model: &str, system: &str, text: &str) -> Result<Reply, String> {
        let resp = self
            .client
            .post(OLLAMA_CHAT_URL)
            .json(&serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": text}
                ],
                "stream": false
            }))
            .timeout(std::time::Duration::from_secs(180))
            .send()
            .await
            .map_err(|e| format!("Ollama unreachable ({e})"))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Ollama parse error: {e}"))?;
        if let Some(err) = body["error"].as_str() {
            return Err(format!("Ollama: {err}"));
        }
        let content = body["message"]["content"]
            .as_str()
            .ok_or_else(|| "Ollama: no content in response".to_string())?;
        Ok((
            strip_thinking_tags(content),
            body["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            body["eval_count"].as_u64().unwrap_or(0) as u32,
        ))
    }

    async fn anthropic(
        &self,
        model: &str,
        system: &str,
        text: &str,
        key: &str,
    ) -> Result<Reply, String> {
        let resp = self
            .client
            .post(ANTHROPIC_URL)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 4096,
                "system": system,
                "messages": [{"role": "user", "content": text}]
            }))
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| format!("Anthropic unreachable: {e}"))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Anthropic parse error: {e}"))?;
        if let Some(err) = body["error"]["message"].as_str() {
            return Err(format!("Anthropic: {err}"));
        }
        let content = body["content"][0]["text"]
            .as_str()
            .ok_or_else(|| "Anthropic: no content in response".to_string())?;
        Ok((
            content.to_string(),
            body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        ))
    }
}

/// Runs the plan in order. Errors are logged locally with detail; the caller only ever sees
/// fixed text, so no model or provider name leaks into the UI.
pub(crate) async fn run_plan(
    backend: &impl TierBackend,
    plan: &[Tier],
    config: &RouterConfig,
    system: &str,
    text: &str,
    key: &str,
) -> Result<(Tier, Reply, Vec<Tier>), String> {
    let mut failed = Vec::new();
    for &tier in plan {
        let result = match tier {
            Tier::Light => backend.ollama(LIGHT_MODEL, system, text).await,
            Tier::Deep => backend.ollama(DEEP_MODEL, system, text).await,
            Tier::Claude => {
                backend
                    .anthropic(&config.anthropic_model, system, text, key)
                    .await
            }
        };
        match result {
            Ok(reply) => return Ok((tier, reply, failed)),
            Err(e) => {
                eprintln!("[llm_router] {tier:?} tier failed: {e}");
                failed.push(tier);
            }
        }
    }
    Err(UNAVAILABLE.to_string())
}

fn claude_gate(config: &RouterConfig, key: &str, daily_spent: f64) -> ClaudeGate {
    ClaudeGate {
        allow_claude: crate::commands::telegram::load_config()
            .allow_claude
            .unwrap_or(false),
        outbound_disabled: crate::commands::telegram::outbound_disabled(),
        has_key: !key.trim().is_empty(),
        local_mode: config.mode != "api" && config.mode != "hybrid",
        over_daily_cap: config.daily_cap_usd.is_some_and(|cap| daily_spent >= cap),
    }
}

/// The key from the caller, else the saved one.
fn anthropic_key(passed: Option<String>) -> String {
    passed
        .filter(|k| !k.trim().is_empty())
        .or_else(|| crate::commands::telegram::load_config().anthropic_api_key)
        .unwrap_or_default()
}

fn routing_reason(requested: Tier, why: &str, gate: &ClaudeGate, failed: &[Tier]) -> String {
    let mut reason = why.to_string();
    if requested == Tier::Claude {
        if let Some(block) = gate.blocked() {
            reason.push_str(&format!("; claude_blocked={block}"));
        }
    }
    for tier in failed {
        reason.push_str(&format!("; {tier:?}_failed").to_lowercase());
    }
    reason
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
    let key = anthropic_key(anthropic_api_key);
    let daily_spent = daily_spend_usd(&load_usage_records());
    let gate = claude_gate(&config, &key, daily_spent);
    let (requested, why) = requested_tier(
        &config,
        &text,
        &tags,
        deep.unwrap_or(false),
        force_provider.as_deref(),
    );
    let plan = tier_plan(requested, &gate);
    let system = system_prompt(&amara_persona());

    let start = Instant::now();
    let backend = HttpBackend {
        client: reqwest::Client::new(),
    };
    let (tier, (content, prompt_tokens, completion_tokens), failed) =
        run_plan(&backend, &plan, &config, &system, &text, &key).await?;
    let latency_ms = start.elapsed().as_millis() as u64;
    let cost = tier_cost(tier, &config, prompt_tokens, completion_tokens);
    let reason = routing_reason(requested, &why, &gate, &failed);

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

#[tauri::command]
pub async fn get_cost_estimate(
    text: String,
    config: RouterConfig,
    tags: Vec<String>,
    deep: Option<bool>,
) -> Result<CostEstimate, String> {
    let estimated_tokens = estimate_tokens(&text);
    let key = anthropic_key(None);
    let gate = claude_gate(&config, &key, daily_spend_usd(&load_usage_records()));
    let (requested, why) = requested_tier(&config, &text, &tags, deep.unwrap_or(false), None);
    let tier = tier_plan(requested, &gate)[0];
    Ok(CostEstimate {
        tier,
        estimated_input_tokens: estimated_tokens,
        estimated_cost_usd: tier_cost(tier, &config, estimated_tokens, estimated_tokens / 2),
        routing_reason: routing_reason(requested, &why, &gate, &[]),
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
            has_key: true,
            local_mode: false,
            over_daily_cap: false,
        }
    }

    /// Records every request; models listed in `fail` return an error.
    #[derive(Default)]
    struct MockBackend {
        fail: Vec<&'static str>,
        calls: RefCell<Vec<(String, String)>>, // (model, system prompt)
        anthropic_calls: RefCell<u32>,
    }

    impl TierBackend for MockBackend {
        async fn ollama(&self, model: &str, system: &str, _text: &str) -> Result<Reply, String> {
            self.calls
                .borrow_mut()
                .push((model.to_string(), system.to_string()));
            if self.fail.contains(&model) {
                Err("down".into())
            } else {
                Ok((format!("from {model}"), 1, 1))
            }
        }
        async fn anthropic(
            &self,
            model: &str,
            system: &str,
            _text: &str,
            _key: &str,
        ) -> Result<Reply, String> {
            *self.anthropic_calls.borrow_mut() += 1;
            self.calls
                .borrow_mut()
                .push((model.to_string(), system.to_string()));
            if self.fail.contains(&"claude") {
                Err("down".into())
            } else {
                Ok(("from claude".into(), 1, 1))
            }
        }
    }

    // ── tier selection ──

    #[test]
    fn default_is_light_in_local_mode() {
        let (t, why) = requested_tier(&cfg("local"), "hi there", &[], false, None);
        assert_eq!(t, Tier::Light);
        assert_eq!(why, "default");
        assert_eq!(RouterConfig::default().mode, "local");
    }

    #[test]
    fn deep_toggle_and_think_deeper_pick_deep() {
        assert_eq!(
            requested_tier(&cfg("local"), "hi", &[], true, None).0,
            Tier::Deep
        );
        assert_eq!(deep_reason("hi", true), Some("deep_toggle"));
        assert_eq!(
            deep_reason("Can you Think Deeper about this?", false),
            Some("asked_to_think_deeper")
        );
    }

    #[test]
    fn deep_heuristic_long_and_multi_step() {
        assert_eq!(deep_reason(&"a".repeat(1_604), false), Some("long_prompt"));
        assert_eq!(deep_reason(&"a".repeat(1_600), false), None);
        assert_eq!(
            deep_reason("Plan:\n1. book\n2) pack\n3. go", false),
            Some("multi_step")
        );
        assert_eq!(
            deep_reason("- one\n- two\n* three", false),
            Some("multi_step")
        );
        assert_eq!(deep_reason("- one\n- two", false), None);
        assert_eq!(
            deep_reason("walk me through it step by step", false),
            Some("multi_step")
        );
        assert_eq!(deep_reason("what's the weather", false), None);
    }

    #[test]
    fn escalation_tag_goes_deep_in_local_mode() {
        let tags = vec!["heavy".to_string()];
        let (t, why) = requested_tier(&cfg("local"), "hi", &tags, false, None);
        assert_eq!(t, Tier::Deep);
        assert_eq!(why, "tag=heavy");
    }

    #[test]
    fn claude_only_requested_by_api_hybrid_escalation_or_force() {
        assert_eq!(
            requested_tier(&cfg("api"), "hi", &[], false, None).0,
            Tier::Claude
        );
        assert_eq!(
            requested_tier(&cfg("hybrid"), "hi", &["research".into()], false, None).0,
            Tier::Claude
        );
        assert_eq!(
            requested_tier(&cfg("hybrid"), "hi", &[], false, None).0,
            Tier::Light
        );
        assert_eq!(
            requested_tier(&cfg("local"), "hi", &[], false, Some("anthropic")).0,
            Tier::Claude
        );
    }

    #[test]
    fn retired_gateway_is_light() {
        assert_eq!(
            requested_tier(&cfg("gateway"), "hi", &[], false, Some("gateway")).0,
            Tier::Light
        );
    }

    // ── the Claude gate ──

    type BreakGate = fn(&mut ClaudeGate);

    #[test]
    fn gate_blocks_each_missing_condition() {
        assert_eq!(open_gate().blocked(), None);
        let cases: [(BreakGate, &str); 5] = [
            (|g| g.outbound_disabled = true, "outbound_disabled"),
            (|g| g.local_mode = true, "local_mode"),
            (|g| g.allow_claude = false, "claude_not_allowed"),
            (|g| g.has_key = false, "no_key"),
            (|g| g.over_daily_cap = true, "daily_cap"),
        ];
        for (break_it, want) in cases {
            let mut g = open_gate();
            break_it(&mut g);
            assert_eq!(g.blocked(), Some(want));
            assert_eq!(tier_plan(Tier::Claude, &g), vec![Tier::Deep, Tier::Light]);
        }
    }

    async fn forced_claude_requests(gate: ClaudeGate, mode: &str) -> u32 {
        let backend = MockBackend::default();
        let config = cfg(mode);
        let (requested, _) = requested_tier(&config, "hi", &[], false, Some("anthropic"));
        let plan = tier_plan(requested, &gate);
        let (tier, _, _) = run_plan(&backend, &plan, &config, "sys", "hi", "k")
            .await
            .unwrap();
        assert_eq!(tier, Tier::Deep);
        let n = *backend.anthropic_calls.borrow();
        n
    }

    #[tokio::test]
    async fn no_anthropic_request_under_outbound_disabled() {
        let gate = ClaudeGate {
            outbound_disabled: true,
            ..open_gate()
        };
        assert_eq!(forced_claude_requests(gate, "api").await, 0);
    }

    #[tokio::test]
    async fn no_anthropic_request_in_local_mode_even_when_forced() {
        let gate = ClaudeGate {
            local_mode: true,
            ..open_gate()
        };
        assert_eq!(forced_claude_requests(gate, "local").await, 0);
    }

    #[tokio::test]
    async fn no_anthropic_request_without_the_toggle() {
        let gate = ClaudeGate {
            allow_claude: false,
            ..open_gate()
        };
        assert_eq!(forced_claude_requests(gate, "api").await, 0);
    }

    #[tokio::test]
    async fn no_anthropic_request_without_a_key() {
        let gate = ClaudeGate {
            has_key: false,
            ..open_gate()
        };
        assert_eq!(forced_claude_requests(gate, "api").await, 0);
    }

    #[tokio::test]
    async fn open_gate_calls_claude_once() {
        let backend = MockBackend::default();
        let config = cfg("api");
        let plan = tier_plan(Tier::Claude, &open_gate());
        let (tier, reply, failed) = run_plan(&backend, &plan, &config, "sys", "hi", "k")
            .await
            .unwrap();
        assert_eq!((tier, reply.0.as_str()), (Tier::Claude, "from claude"));
        assert!(failed.is_empty());
        assert_eq!(*backend.anthropic_calls.borrow(), 1);
    }

    // ── fallback order ──

    #[tokio::test]
    async fn deep_falls_back_to_light() {
        let backend = MockBackend {
            fail: vec![DEEP_MODEL],
            ..Default::default()
        };
        let plan = tier_plan(Tier::Deep, &open_gate());
        let (tier, _, failed) = run_plan(&backend, &plan, &cfg("local"), "s", "t", "")
            .await
            .unwrap();
        assert_eq!(tier, Tier::Light);
        assert_eq!(failed, vec![Tier::Deep]);
        let models: Vec<String> = backend.calls.borrow().iter().map(|c| c.0.clone()).collect();
        assert_eq!(models, vec![DEEP_MODEL, LIGHT_MODEL]);
    }

    #[tokio::test]
    async fn claude_failure_falls_back_deep_then_light_with_one_network_call() {
        let backend = MockBackend {
            fail: vec!["claude", DEEP_MODEL],
            ..Default::default()
        };
        let config = cfg("api");
        let plan = tier_plan(Tier::Claude, &open_gate());
        assert_eq!(plan, vec![Tier::Claude, Tier::Deep, Tier::Light]);
        let (tier, _, failed) = run_plan(&backend, &plan, &config, "s", "t", "k")
            .await
            .unwrap();
        assert_eq!(tier, Tier::Light);
        assert_eq!(failed, vec![Tier::Claude, Tier::Deep]);
        assert_eq!(*backend.anthropic_calls.borrow(), 1);
        let models: Vec<String> = backend.calls.borrow().iter().map(|c| c.0.clone()).collect();
        assert_eq!(
            models,
            vec![config.anthropic_model.as_str(), DEEP_MODEL, LIGHT_MODEL]
        );
    }

    #[test]
    fn claude_never_appears_after_the_first_slot() {
        for requested in [Tier::Light, Tier::Deep, Tier::Claude] {
            let plan = tier_plan(requested, &open_gate());
            assert!(!plan[1..].contains(&Tier::Claude), "{plan:?}");
        }
    }

    #[tokio::test]
    async fn all_tiers_down_gives_fixed_text() {
        let backend = MockBackend {
            fail: vec![DEEP_MODEL, LIGHT_MODEL],
            ..Default::default()
        };
        let err = run_plan(
            &backend,
            &[Tier::Deep, Tier::Light],
            &cfg("local"),
            "s",
            "t",
            "",
        )
        .await
        .unwrap_err();
        assert_eq!(err, UNAVAILABLE);
        assert!(!err.contains("gemma") && !err.contains("qwen"));
    }

    // ── identity ──

    #[tokio::test]
    async fn every_tier_gets_the_same_system_prompt() {
        let backend = MockBackend {
            fail: vec!["claude", DEEP_MODEL],
            ..Default::default()
        };
        let system = system_prompt("Persona from SOUL.md");
        let plan = tier_plan(Tier::Claude, &open_gate());
        run_plan(&backend, &plan, &cfg("api"), &system, "t", "k")
            .await
            .unwrap();
        let calls = backend.calls.borrow();
        assert_eq!(calls.len(), 3);
        assert!(calls.iter().all(|(_, s)| s == &system));
    }

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
            routing_reason: "deep_toggle".into(),
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

    #[test]
    fn routing_reason_names_the_block_and_failures() {
        let gate = ClaudeGate {
            outbound_disabled: true,
            ..open_gate()
        };
        assert_eq!(
            routing_reason(Tier::Claude, "mode=api", &gate, &[Tier::Deep]),
            "mode=api; claude_blocked=outbound_disabled; deep_failed"
        );
    }

    // ── constants and costs ──

    #[test]
    fn tier_models_and_loopback() {
        assert_eq!(LIGHT_MODEL, "gemma4-12b-uncensored:latest");
        assert_eq!(DEEP_MODEL, "qwen3.8-uncensored:latest");
        assert!(OLLAMA_CHAT_URL.starts_with("http://127.0.0.1:11434/"));
        assert_eq!(DEFAULT_OLLAMA_MODEL, "gemma4:latest");
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
