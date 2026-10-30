/**
 * LLM Router — unified routing with multiple provider support.
 *
 * Modes:
 *   local   — always Ollama, never calls any external API
 *   hybrid  — Ollama by default; escalates to Anthropic when warranted
 *   api     — always Anthropic (direct HTTP)
 *   gateway — OpenClaw gateway (routes to whatever model is configured there); when
 *             OpenClaw isn't installed or its gateway is unreachable, local Ollama instead
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

pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:latest";

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            // Standing rule: local Ollama only, gemma4 primary.
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RouterResponse {
    pub content: String,
    pub provider: String,
    pub model: String,
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
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub estimated_cost_usd: f64,
    pub latency_ms: u64,
    pub routing_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CostEstimate {
    pub provider: String,
    pub model: String,
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

// ─── Routing decision ─────────────────────────────────────────────────────────

pub struct RoutingDecision {
    pub provider: String,
    pub reason: String,
}

pub fn decide_provider(
    config: &RouterConfig,
    tags: &[String],
    estimated_tokens: u32,
    force_provider: Option<&str>,
    daily_spent: f64,
) -> RoutingDecision {
    // Explicit override always wins
    if let Some(fp) = force_provider {
        return RoutingDecision {
            provider: fp.to_string(),
            reason: "force_provider".to_string(),
        };
    }

    match config.mode.as_str() {
        "local" => RoutingDecision {
            provider: "ollama".to_string(),
            reason: "mode=local".to_string(),
        },

        "api" => RoutingDecision {
            provider: "anthropic".to_string(),
            reason: "mode=api".to_string(),
        },

        "gateway" => RoutingDecision {
            provider: "gateway".to_string(),
            reason: "mode=gateway".to_string(),
        },

        _ => {
            // ── hybrid ──────────────────────────────────────────────────────
            // 1. Daily budget guard — if exceeded, always stay local
            if let Some(cap) = config.daily_cap_usd {
                if daily_spent >= cap {
                    return RoutingDecision {
                        provider: "ollama".to_string(),
                        reason: format!("daily_cap_exceeded({:.4}>={:.2})", daily_spent, cap),
                    };
                }
            }

            // 2. Determine if escalation is warranted (tags or token threshold)
            let escalation_tags = ["heavy", "research", "code", "long"];
            let tag_trigger = tags.iter().find(|t| escalation_tags.contains(&t.as_str()));
            let token_trigger = estimated_tokens > config.token_threshold;

            let escalation_reason = if let Some(trigger) = tag_trigger {
                Some(format!("tag={}", trigger))
            } else if token_trigger {
                Some(format!("tokens_>{}", config.token_threshold))
            } else {
                None
            };

            // 3. If escalation warranted, apply per-task cap guard
            if let Some(reason) = escalation_reason {
                if let Some(per_task) = config.per_task_cap_usd {
                    let est_out = estimated_tokens / 2;
                    let est_cost = estimate_cost(
                        "anthropic",
                        &config.anthropic_model,
                        estimated_tokens,
                        est_out,
                    );
                    if est_cost > per_task {
                        return RoutingDecision {
                            provider: "ollama".to_string(),
                            reason: format!(
                                "per_task_cap({:.4}>{:.2},would:{})",
                                est_cost, per_task, reason
                            ),
                        };
                    }
                }
                return RoutingDecision {
                    provider: "anthropic".to_string(),
                    reason,
                };
            }

            // 4. Default: stay local
            RoutingDecision {
                provider: "ollama".to_string(),
                reason: "hybrid_default_local".to_string(),
            }
        }
    }
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

async fn call_ollama(
    model: &str,
    text: &str,
    client: &reqwest::Client,
) -> Result<(String, u32, u32), String> {
    let resp = client
        .post("http://127.0.0.1:11434/api/chat")
        .json(&serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": text}],
            "stream": false
        }))
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("Ollama unreachable — is it running? ({})", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Ollama parse error: {}", e))?;

    if let Some(err) = body["error"].as_str() {
        return Err(format!("Ollama: {}", err));
    }

    let content = body["message"]["content"]
        .as_str()
        .ok_or_else(|| "Ollama: no content in response".to_string())?;

    let content = strip_thinking_tags(content);
    let prompt_tokens = body["prompt_eval_count"].as_u64().unwrap_or(0) as u32;
    let completion_tokens = body["eval_count"].as_u64().unwrap_or(0) as u32;

    Ok((content, prompt_tokens, completion_tokens))
}

async fn call_anthropic(
    model: &str,
    text: &str,
    api_key: &str,
    client: &reqwest::Client,
) -> Result<(String, u32, u32), String> {
    if api_key.is_empty() {
        return Err("Anthropic API key not set. Add it in Settings → Compute Mode.".to_string());
    }

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": text}]
        }))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Anthropic unreachable: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Anthropic parse error: {}", e))?;

    if let Some(err) = body["error"]["message"].as_str() {
        return Err(format!("Anthropic: {}", err));
    }

    let content = body["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "Anthropic: no content in response".to_string())?;

    let prompt_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
    let completion_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

    Ok((content.to_string(), prompt_tokens, completion_tokens))
}

/// Prefix of `call_openclaw`'s transport error: the gateway isn't there (fall back).
const GATEWAY_UNREACHABLE: &str = "OpenClaw gateway unreachable:";

type ProviderReply = (String, u32, u32, String, String);

/// The "gateway" route. OpenClaw is not the default path: not installed means local
/// Ollama without trying it, and an unreachable gateway falls back the same way. If
/// Ollama fails too: `unavailable: openclaw not installed` (or the gateway error when
/// it is installed). An OpenClaw API error is returned as before.
async fn via_gateway<O, OF, L, LF>(
    openclaw_installed: bool,
    ollama_model: &str,
    openclaw: O,
    ollama: L,
) -> Result<ProviderReply, String>
where
    O: FnOnce() -> OF,
    OF: std::future::Future<Output = Result<(String, u32, u32, String), String>>,
    L: FnOnce() -> LF,
    LF: std::future::Future<Output = Result<(String, u32, u32), String>>,
{
    let gateway_err = if openclaw_installed {
        match openclaw().await {
            Ok((c, p, o, model)) => return Ok((c, p, o, "gateway".to_string(), model)),
            Err(e) if e.starts_with(GATEWAY_UNREACHABLE) => e,
            Err(e) => return Err(e),
        }
    } else {
        super::openclaw::NOT_INSTALLED_ERROR.to_string()
    };
    eprintln!("[llm_router] OpenClaw unavailable, using local Ollama");
    match ollama().await {
        Ok((c, p, o)) => Ok((c, p, o, "ollama".to_string(), ollama_model.to_string())),
        Err(_) => Err(gateway_err),
    }
}

async fn call_openclaw(
    gateway_url: &str,
    gateway_token: &str,
    text: &str,
    system_prompt: Option<&str>,
    client: &reqwest::Client,
) -> Result<(String, u32, u32, String), String> {
    let url = format!("{}/v1/chat/completions", gateway_url.trim_end_matches('/'));

    let mut messages = Vec::new();
    if let Some(sys) = system_prompt {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.push(serde_json::json!({"role": "user", "content": text}));

    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "messages": messages,
            "max_tokens": 4096
        }))
        .timeout(std::time::Duration::from_secs(120));

    if !gateway_token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", gateway_token));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("{GATEWAY_UNREACHABLE} {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("OpenClaw parse error: {}", e))?;

    if let Some(err) = body["error"]["message"].as_str() {
        return Err(format!("OpenClaw: {}", err));
    }

    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "OpenClaw: no content in response".to_string())?;

    let model = body["model"].as_str().unwrap_or("unknown").to_string();

    let prompt_tokens = body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
    let completion_tokens = body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

    Ok((content.to_string(), prompt_tokens, completion_tokens, model))
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn chat_with_router(
    text: String,
    config: RouterConfig,
    tags: Vec<String>,
    force_provider: Option<String>,
    anthropic_api_key: Option<String>,
    gateway_url: Option<String>,
    gateway_auth_token: Option<String>,
) -> Result<RouterResponse, String> {
    let client = reqwest::Client::new();
    let estimated_tokens = estimate_tokens(&text);

    let records = load_usage_records();
    let daily_spent = daily_spend_usd(&records);

    let decision = decide_provider(
        &config,
        &tags,
        estimated_tokens,
        force_provider.as_deref(),
        daily_spent,
    );

    let start = Instant::now();

    // Dispatch to chosen provider
    let (content, prompt_tokens, completion_tokens, actual_provider, actual_model) =
        match decision.provider.as_str() {
            "gateway" => {
                let url = gateway_url.as_deref().unwrap_or("http://127.0.0.1:18789");
                let token = gateway_auth_token.as_deref().unwrap_or("");
                via_gateway(
                    super::openclaw::InstallPaths::system().installed(),
                    &config.ollama_model,
                    || call_openclaw(url, token, &text, None, &client),
                    || call_ollama(&config.ollama_model, &text, &client),
                )
                .await?
            }
            "anthropic" => {
                let key = anthropic_api_key.as_deref().unwrap_or("");
                match call_anthropic(&config.anthropic_model, &text, key, &client).await {
                    Ok((c, p, o)) => (
                        c,
                        p,
                        o,
                        "anthropic".to_string(),
                        config.anthropic_model.clone(),
                    ),
                    Err(e) if config.mode == "hybrid" => {
                        eprintln!(
                            "[llm_router] Anthropic failed ({}), falling back to Ollama",
                            e
                        );
                        let (c, p, o) = call_ollama(&config.ollama_model, &text, &client).await?;
                        (c, p, o, "ollama".to_string(), config.ollama_model.clone())
                    }
                    Err(e) => return Err(e),
                }
            }
            _ => {
                let (c, p, o) = call_ollama(&config.ollama_model, &text, &client).await?;
                (c, p, o, "ollama".to_string(), config.ollama_model.clone())
            }
        };

    let latency_ms = start.elapsed().as_millis() as u64;
    let cost = estimate_cost(
        &actual_provider,
        &actual_model,
        prompt_tokens,
        completion_tokens,
    );

    // Persist usage record
    append_usage_record(LlmUsageRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        provider: actual_provider.clone(),
        model: actual_model.clone(),
        prompt_tokens,
        completion_tokens,
        estimated_cost_usd: cost,
        latency_ms,
        routing_reason: decision.reason.clone(),
    });

    Ok(RouterResponse {
        content,
        provider: actual_provider,
        model: actual_model,
        latency_ms,
        prompt_tokens,
        completion_tokens,
        estimated_cost_usd: cost,
        routing_reason: decision.reason,
    })
}

#[tauri::command]
pub async fn get_cost_estimate(
    text: String,
    config: RouterConfig,
    tags: Vec<String>,
) -> Result<CostEstimate, String> {
    let estimated_tokens = estimate_tokens(&text);
    let records = load_usage_records();
    let daily_spent = daily_spend_usd(&records);
    let decision = decide_provider(&config, &tags, estimated_tokens, None, daily_spent);

    let (provider, model) = if decision.provider == "anthropic" {
        ("anthropic".to_string(), config.anthropic_model.clone())
    } else {
        ("ollama".to_string(), config.ollama_model.clone())
    };

    let estimated_output = estimated_tokens / 2;
    let cost = estimate_cost(&provider, &model, estimated_tokens, estimated_output);

    Ok(CostEstimate {
        provider,
        model,
        estimated_input_tokens: estimated_tokens,
        estimated_cost_usd: cost,
        routing_reason: decision.reason,
    })
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
    #[test]
    fn default_ollama_model_is_gemma4_latest() {
        assert_eq!(DEFAULT_OLLAMA_MODEL, "gemma4:latest");
        assert_eq!(RouterConfig::default().ollama_model, "gemma4:latest");
    }

    use super::*;

    #[test]
    fn default_is_local_gemma4() {
        let d = RouterConfig::default();
        assert_eq!(
            (d.mode.as_str(), d.ollama_model.as_str()),
            ("local", "gemma4:latest")
        );
    }

    fn cfg(mode: &str) -> RouterConfig {
        RouterConfig {
            mode: mode.to_string(),
            ollama_model: "qwen3.5".to_string(),
            anthropic_model: "claude-haiku-4-5-20251001".to_string(),
            token_threshold: 8000,
            daily_cap_usd: Some(5.0),
            per_task_cap_usd: Some(0.10),
            show_cost_estimate: true,
        }
    }

    // ── Mode constraints ─────────────────────────────────────────────────────

    #[test]
    fn local_mode_never_calls_api_even_with_heavy_tag() {
        let d = decide_provider(&cfg("local"), &["heavy".to_string()], 99_999, None, 0.0);
        assert_eq!(d.provider, "ollama");
        assert_eq!(d.reason, "mode=local");
    }

    #[test]
    fn api_mode_always_anthropic() {
        let d = decide_provider(&cfg("api"), &[], 50, None, 0.0);
        assert_eq!(d.provider, "anthropic");
        assert_eq!(d.reason, "mode=api");
    }

    // ── Hybrid escalation triggers ───────────────────────────────────────────

    #[test]
    fn hybrid_heavy_tag_escalates() {
        let d = decide_provider(&cfg("hybrid"), &["heavy".to_string()], 100, None, 0.0);
        assert_eq!(d.provider, "anthropic");
        assert!(d.reason.contains("heavy"), "reason was: {}", d.reason);
    }

    #[test]
    fn hybrid_research_tag_escalates() {
        let d = decide_provider(&cfg("hybrid"), &["research".to_string()], 100, None, 0.0);
        assert_eq!(d.provider, "anthropic");
        assert!(d.reason.contains("research"));
    }

    #[test]
    fn hybrid_token_threshold_escalates() {
        let d = decide_provider(&cfg("hybrid"), &[], 9_000, None, 0.0);
        assert_eq!(d.provider, "anthropic");
        assert!(d.reason.contains("tokens_>"), "reason was: {}", d.reason);
    }

    #[test]
    fn hybrid_below_threshold_stays_local() {
        let d = decide_provider(&cfg("hybrid"), &[], 100, None, 0.0);
        assert_eq!(d.provider, "ollama");
        assert_eq!(d.reason, "hybrid_default_local");
    }

    // ── Budget guards ────────────────────────────────────────────────────────

    #[test]
    fn daily_cap_blocks_api_escalation() {
        // even with heavy tag, daily cap exceeded → stay local
        let d = decide_provider(
            &cfg("hybrid"),
            &["heavy".to_string()],
            9_000,
            None,
            5.01, // over the $5.00 cap
        );
        assert_eq!(d.provider, "ollama");
        assert!(
            d.reason.contains("daily_cap_exceeded"),
            "reason: {}",
            d.reason
        );
    }

    #[test]
    fn per_task_cap_blocks_expensive_request() {
        // 120_000 tokens → est_cost = (120k*$0.00025 + 60k*$0.00125)/1k = $0.105 > $0.10 cap
        // Escalation would have been triggered by tokens_>8000; per-task cap blocks it
        let d = decide_provider(&cfg("hybrid"), &[], 120_000, None, 0.0);
        assert_eq!(d.provider, "ollama");
        assert!(d.reason.contains("per_task_cap"), "reason: {}", d.reason);
        assert!(
            d.reason.contains("would:tokens_>"),
            "should show blocked reason: {}",
            d.reason
        );
    }

    #[test]
    fn per_task_cap_shows_blocked_tag_escalation() {
        // "heavy" tag would escalate, but 120k tokens makes it too expensive
        let d = decide_provider(&cfg("hybrid"), &["heavy".to_string()], 120_000, None, 0.0);
        assert_eq!(d.provider, "ollama");
        assert!(d.reason.contains("per_task_cap"), "reason: {}", d.reason);
        assert!(
            d.reason.contains("would:tag=heavy"),
            "should show blocked tag: {}",
            d.reason
        );
    }

    // ── Force override ───────────────────────────────────────────────────────

    #[test]
    fn force_provider_overrides_local_mode() {
        let d = decide_provider(&cfg("local"), &[], 100, Some("anthropic"), 0.0);
        assert_eq!(d.provider, "anthropic");
        assert_eq!(d.reason, "force_provider");
    }

    #[test]
    fn force_provider_overrides_daily_cap() {
        let d = decide_provider(&cfg("hybrid"), &[], 100, Some("anthropic"), 99.0);
        assert_eq!(d.provider, "anthropic");
        assert_eq!(d.reason, "force_provider");
    }

    // ── Token estimation ─────────────────────────────────────────────────────

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_four_chars() {
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn estimate_tokens_five_chars_rounds_up() {
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn estimate_tokens_400_chars() {
        assert_eq!(estimate_tokens(&"a".repeat(400)), 100);
    }

    // ── Cost estimation ──────────────────────────────────────────────────────

    #[test]
    fn ollama_cost_is_zero() {
        assert_eq!(estimate_cost("ollama", "qwen3.5", 1000, 500), 0.0);
    }

    #[test]
    fn anthropic_haiku_cost_correct() {
        // (1000/1000)*0.00025 + (500/1000)*0.00125 = 0.000875
        let cost = estimate_cost("anthropic", "claude-haiku-4-5-20251001", 1000, 500);
        assert!((cost - 0.000875).abs() < 1e-9, "cost={}", cost);
    }

    #[test]
    fn anthropic_sonnet_cost_correct() {
        // (1000/1000)*0.003 + (500/1000)*0.015 = 0.0105
        let cost = estimate_cost("anthropic", "claude-sonnet-4-6", 1000, 500);
        assert!((cost - 0.0105).abs() < 1e-9, "cost={}", cost);
    }

    #[test]
    fn anthropic_opus_cost_correct() {
        // (1000/1000)*0.015 + (500/1000)*0.075 = 0.0525
        let cost = estimate_cost("anthropic", "claude-opus-4-6", 1000, 500);
        assert!((cost - 0.0525).abs() < 1e-9, "cost={}", cost);
    }

    #[tokio::test]
    async fn gateway_without_openclaw_falls_back_to_ollama_without_calling_it() {
        let reply = via_gateway(
            false,
            "gemma4:latest",
            || async { panic!("OpenClaw must not be called when not installed") },
            || async { Ok(("hi".to_string(), 1, 2)) },
        )
        .await
        .unwrap();
        assert_eq!(
            reply,
            ("hi".into(), 1, 2, "ollama".into(), "gemma4:latest".into())
        );
    }

    #[tokio::test]
    async fn gateway_without_openclaw_or_ollama_is_unavailable() {
        let err = via_gateway(
            false,
            "gemma4:latest",
            || async { panic!("not installed") },
            || async { Err("Ollama unreachable: refused".to_string()) },
        )
        .await
        .unwrap_err();
        assert_eq!(err, "unavailable: openclaw not installed");
    }

    #[tokio::test]
    async fn installed_gateway_is_used_and_unreachable_falls_back() {
        let ok = via_gateway(
            true,
            "m",
            || async { Ok(("g".to_string(), 3, 4, "minimax".to_string())) },
            || async { panic!("gateway answered") },
        )
        .await
        .unwrap();
        assert_eq!(ok.3, "gateway");
        let fell_back = via_gateway(
            true,
            "m",
            || async { Err(format!("{GATEWAY_UNREACHABLE} connection refused")) },
            || async { Ok(("l".to_string(), 0, 0)) },
        )
        .await
        .unwrap();
        assert_eq!(fell_back.3, "ollama");
        let api_err = via_gateway(
            true,
            "m",
            || async { Err("OpenClaw: model overloaded".to_string()) },
            || async { panic!("API errors don't fall back") },
        )
        .await
        .unwrap_err();
        assert_eq!(api_err, "OpenClaw: model overloaded");
    }
}
