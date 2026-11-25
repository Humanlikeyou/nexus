//! The one Amara inference interface (decision 0051).
//!
//! Callers describe *what* they need (a capability class, a provider policy, messages, an output
//! shape, tools, parameters, limits). This module turns that into one request to the Amara Router,
//! which stays the only place that picks a model. Nothing here names a model, so switching models
//! is a router config change and never touches business logic.
//!
//! Responsibilities (kept apart on purpose):
//! - **Inference (this module):** policy check, request building, timeouts, cancellation, retries,
//!   output validation, usage logging. It never executes anything a model proposes.
//! - **Intelligence (callers, e.g. `prepare_amara_turn`):** assembles context and the task
//!   instruction. Retrieved context enters as [`AmaraContextSection`]s and passes the existing guard.
//! - **Tool authorization ([`tools`]):** validates proposed tool calls before anything runs.
//! - **Deterministic services:** do the work behind a [`tools::ToolExecutor`].
//!
//! Provider policy is enforced here, in Rust: local is the default, the cloud needs
//! [`ProviderPolicy::CloudIfAuthorized`] *and* the existing Claude gate (Settings › Allow Claude,
//! cloud compute mode, daily cap, `NEXUS_OUTBOUND_DISABLED` unset, navy local-only off). A local
//! failure never falls back to the cloud: the router's fallbacks only ever go down (claude → deep →
//! fast), and a local request always sends `X-Amara-Allow-Claude: 0`, which the router honours.

pub(crate) mod schema;
pub(crate) mod tools;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::llm_router::{self, ClaudeGate, RouterTransport, Tier};
use super::loopback_http::loopback_base;
use super::terrence_runtime::{nexus_context_message, AmaraContextSection};

// ─── Request ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool call a model proposed. Proposals only: see [`tools::authorize`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn system(c: impl Into<String>) -> Self {
        Self::new(Role::System, c)
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self::new(Role::User, c)
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self::new(Role::Assistant, c)
    }
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_call_id: Some(call_id.into()),
            ..Self::new(Role::Tool, content)
        }
    }
}

/// What the caller needs, not which model: the router maps a class to its configured model.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelClass {
    /// The resident local model (router `fast`; initial target Gemma 4 E4B).
    #[default]
    Fast,
    /// The on-demand local model (router `deep`).
    Deep,
    /// The cloud tier. Needs [`ProviderPolicy::CloudIfAuthorized`] and an open Claude gate.
    Cloud,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPolicy {
    /// Never leaves the Mac. The default.
    #[default]
    LocalOnly,
    /// The cloud only when the configured permission and budget checks pass.
    CloudIfAuthorized,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum OutputSpec {
    #[default]
    Text,
    /// Constrained by the runtime where it can (Ollama `format`), then validated here regardless.
    Json { name: String, schema: Value },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct InferenceParams {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stop: Vec<String>,
    pub seed: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Limits {
    /// Whole call, retries included. The deep model may have to load first.
    pub timeout: Duration,
    /// Extra attempts after the first, for retryable failures only.
    pub retries: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(600),
            retries: 1,
        }
    }
}

/// Cancels an in-flight call (and its retries). Cheap to clone.
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<tokio::sync::watch::Sender<bool>>);

impl Default for CancelToken {
    fn default() -> Self {
        Self(Arc::new(tokio::sync::watch::channel(false).0))
    }
}

impl CancelToken {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|c| *c).await;
    }
}

#[derive(Clone, Debug)]
pub struct InferenceRequest {
    /// Short fixed label for logs and usage (`goal_route.detect`), never user text.
    pub purpose: &'static str,
    pub class: ModelClass,
    pub policy: ProviderPolicy,
    /// Prepend Amara's one system prompt (persona + identity guard). On by default.
    pub persona: bool,
    /// Task instruction from the intelligence layer, appended to the system prompt.
    pub instruction: Option<String>,
    /// Retrieved context. Unknown tags are dropped; each section is injection-scanned and escaped.
    pub context: Vec<AmaraContextSection>,
    /// Conversation so far, ending with the new turn.
    pub messages: Vec<Message>,
    pub output: OutputSpec,
    /// Tools the model may *propose*. Executing one is [`tools::run_with_tools`]'s job.
    pub tools: Vec<tools::ToolSpec>,
    pub params: InferenceParams,
    pub limits: Limits,
    pub cancel: Option<CancelToken>,
}

impl InferenceRequest {
    /// Local-only, persona on, text out, default limits.
    pub fn new(purpose: &'static str, class: ModelClass) -> Self {
        Self {
            purpose,
            class,
            policy: ProviderPolicy::LocalOnly,
            persona: true,
            instruction: None,
            context: vec![],
            messages: vec![],
            output: OutputSpec::Text,
            tools: vec![],
            params: InferenceParams::default(),
            limits: Limits::default(),
            cancel: None,
        }
    }
}

// ─── Result and errors ────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Serialize, Clone, Debug)]
pub struct InferenceOutcome {
    pub content: String,
    /// The validated value when `output` was [`OutputSpec::Json`].
    pub json: Option<Value>,
    pub tool_calls: Vec<ToolCall>,
    /// The tier that answered (never a model or provider name).
    pub tier: Tier,
    pub reason: String,
    /// Summed over every attempt, so retries are counted.
    pub usage: Usage,
    pub cost_usd: f64,
    pub latency_ms: u64,
    pub attempts: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InferenceError {
    /// The policy forbids this request; nothing was sent.
    PolicyDenied(&'static str),
    /// The router base isn't loopback; nothing was sent.
    NotLoopback,
    BadRequest(String),
    RouterUnreachable,
    /// Ollama down or the model still loading (router 503).
    BackendUnavailable,
    BackendFailed(u16),
    /// The reply had no content and no tool call.
    EmptyReply,
    /// Structured output or tool arguments failed validation (path and rule only).
    InvalidOutput(String),
    /// A local-only request came back from the cloud tier. Refused.
    UnexpectedTier,
    ToolLoopLimit,
    Timeout,
    Cancelled,
}

impl InferenceError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::PolicyDenied(_) => "policy_denied",
            Self::NotLoopback => "not_loopback",
            Self::BadRequest(_) => "bad_request",
            Self::RouterUnreachable => "router_unreachable",
            Self::BackendUnavailable => "backend_unavailable",
            Self::BackendFailed(_) => "backend_failed",
            Self::EmptyReply => "empty_reply",
            Self::InvalidOutput(_) => "invalid_output",
            Self::UnexpectedTier => "unexpected_tier",
            Self::ToolLoopLimit => "tool_loop_limit",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RouterUnreachable
                | Self::BackendUnavailable
                | Self::BackendFailed(_)
                | Self::EmptyReply
                | Self::InvalidOutput(_)
        )
    }

    /// Fixed text for the UI: the same wording `chat_with_router` has always used.
    pub fn user_text(&self) -> String {
        match self {
            Self::RouterUnreachable => llm_router::ROUTER_DOWN.to_string(),
            Self::BackendUnavailable => llm_router::OLLAMA_DOWN.to_string(),
            Self::PolicyDenied(_) => {
                "Amara didn't send that: it needs the cloud, and the cloud isn't allowed right now."
                    .to_string()
            }
            Self::Cancelled => "Stopped.".to_string(),
            Self::Timeout => "Amara took too long to answer, so it was stopped.".to_string(),
            _ => llm_router::ROUTER_FAILED.to_string(),
        }
    }
}

impl std::fmt::Display for InferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_text())
    }
}

// ─── Policy ───────────────────────────────────────────────────────────────────

/// Everything the policy needs, read once per call so tests can set it directly.
#[derive(Clone, Copy, Debug)]
pub struct PolicyInputs {
    pub gate: ClaudeGate,
    /// Navy local-only switch.
    pub local_only_switch: bool,
}

/// The headers for one request, or why it may not be sent. A forced cloud class can't get past
/// the gate: the gate is checked here, in Rust, whatever the caller asked for.
pub(crate) fn policy_headers(
    class: ModelClass,
    policy: ProviderPolicy,
    inputs: &PolicyInputs,
) -> Result<Vec<(&'static str, String)>, InferenceError> {
    match class {
        ModelClass::Fast => Ok(llm_router::local_tier_headers()),
        ModelClass::Deep => Ok(llm_router::local_deep_headers()),
        ModelClass::Cloud => {
            if policy == ProviderPolicy::LocalOnly {
                return Err(InferenceError::PolicyDenied("local_only"));
            }
            if inputs.local_only_switch {
                return Err(InferenceError::PolicyDenied("local_only_switch"));
            }
            if let Some(block) = inputs.gate.blocked() {
                return Err(InferenceError::PolicyDenied(block));
            }
            Ok(vec![
                ("X-Amara-Allow-Claude", "1".to_string()),
                ("X-Amara-Mode", "api".to_string()),
                ("X-Amara-Route", "claude".to_string()),
            ])
        }
    }
}

fn cloud_authorized(headers: &[(&'static str, String)]) -> bool {
    headers
        .iter()
        .any(|(k, v)| *k == "X-Amara-Allow-Claude" && v == "1")
}

// ─── Request body ─────────────────────────────────────────────────────────────

fn message_json(m: &Message) -> Value {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut o = Map::new();
    o.insert("role".into(), json!(role));
    o.insert("content".into(), json!(m.content));
    if !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({"id": c.id, "type": "function",
                       "function": {"name": c.name, "arguments": c.arguments.to_string()}})
            })
            .collect();
        o.insert("tool_calls".into(), Value::Array(calls));
    }
    if let Some(id) = &m.tool_call_id {
        o.insert("tool_call_id".into(), json!(id));
    }
    Value::Object(o)
}

/// Added to the system prompt whenever tools are offered. Live probe 2026-11-25 (Gemma 4, fast
/// tier): with Amara's persona and identity guard and no such line, 0/10 requests called the tool;
/// with it, 9/10 did, the same as with no system prompt at all.
pub(crate) const TOOL_USE_HINT: &str =
    "When a tool can answer from the user's own data, call it instead of guessing.";

/// The OpenAI-shaped body the router expects. `persona` is passed in so tests needn't read
/// `~/.hermes`.
pub(crate) fn request_body(req: &InferenceRequest, persona: &str) -> Result<Value, InferenceError> {
    let mut system = String::new();
    if req.persona {
        system.push_str(&llm_router::system_prompt(persona));
    }
    if let Some(i) = req.instruction.as_deref().filter(|i| !i.trim().is_empty()) {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(i);
    }
    if !req.tools.is_empty() {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(TOOL_USE_HINT);
    }
    let mut messages = vec![];
    if !system.is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    let context = nexus_context_message(&req.context);
    if !context.is_empty() {
        messages.push(json!({"role": "system", "content": context}));
    }
    if req.messages.is_empty() {
        return Err(InferenceError::BadRequest("no messages".into()));
    }
    messages.extend(req.messages.iter().map(message_json));

    let mut body = json!({"model": "amara", "stream": false, "messages": messages});
    let p = &req.params;
    if let Some(t) = p.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(t) = p.top_p {
        body["top_p"] = json!(t);
    }
    if let Some(n) = p.max_tokens {
        body["max_tokens"] = json!(n);
    }
    if !p.stop.is_empty() {
        body["stop"] = json!(p.stop);
    }
    if let Some(s) = p.seed {
        body["seed"] = json!(s);
    }
    if !req.tools.is_empty() {
        for t in &req.tools {
            schema::check_supported(&t.parameters).map_err(InferenceError::BadRequest)?;
        }
        body["tools"] = Value::Array(req.tools.iter().map(tools::ToolSpec::openai_json).collect());
    }
    if let OutputSpec::Json { name, schema: s } = &req.output {
        schema::check_supported(s).map_err(InferenceError::BadRequest)?;
        body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": name, "schema": s, "strict": true}});
    }
    Ok(body)
}

// ─── Reply ────────────────────────────────────────────────────────────────────

struct Reply {
    content: String,
    tool_calls: Vec<ToolCall>,
    tier: Tier,
    reason: String,
    usage: Usage,
}

fn parse_reply(v: &Value) -> Result<Reply, InferenceError> {
    let msg = &v["choices"][0]["message"];
    if !msg.is_object() {
        return Err(InferenceError::BackendFailed(200));
    }
    let mut calls = vec![];
    for (i, c) in msg["tool_calls"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let name = c["function"]["name"].as_str().unwrap_or("").to_string();
        if name.is_empty() {
            return Err(InferenceError::InvalidOutput(
                "tool call without a name".into(),
            ));
        }
        let arguments = match &c["function"]["arguments"] {
            Value::String(s) if s.trim().is_empty() => json!({}),
            Value::String(s) => serde_json::from_str(s)
                .map_err(|_| InferenceError::InvalidOutput("tool arguments are not JSON".into()))?,
            Value::Null => json!({}),
            other => other.clone(),
        };
        calls.push(ToolCall {
            id: c["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{i}")),
            name,
            arguments,
        });
    }
    let u32_of = |x: &Value| x.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
    Ok(Reply {
        content: msg["content"].as_str().unwrap_or("").trim().to_string(),
        tool_calls: calls,
        tier: Tier::from_router(v["amara"]["tier"].as_str().unwrap_or("fast")),
        reason: v["amara"]["reason"].as_str().unwrap_or("").to_string(),
        usage: Usage {
            prompt_tokens: u32_of(&v["usage"]["prompt_tokens"]),
            completion_tokens: u32_of(&v["usage"]["completion_tokens"]),
        },
    })
}

fn check_output(req: &InferenceRequest, r: &Reply) -> Result<Option<Value>, InferenceError> {
    if !r.tool_calls.is_empty() {
        return Ok(None);
    }
    match &req.output {
        OutputSpec::Text if r.content.is_empty() => Err(InferenceError::EmptyReply),
        OutputSpec::Text => Ok(None),
        OutputSpec::Json { schema: s, .. } => {
            if r.content.is_empty() {
                return Err(InferenceError::EmptyReply);
            }
            let v = schema::extract_json(&r.content)
                .ok_or_else(|| InferenceError::InvalidOutput("reply is not JSON".into()))?;
            schema::validate(s, &v).map_err(InferenceError::InvalidOutput)?;
            Ok(Some(v))
        }
    }
}

// ─── The call ─────────────────────────────────────────────────────────────────

/// Gemma 4 returns an empty reply at temperature 0 on the turn after a tool result (live probe,
/// 2026-11-25, 3/3). A retry of an empty reply runs at least this warm.
const EMPTY_RETRY_MIN_TEMPERATURE: f32 = 0.3;

async fn one_attempt(
    transport: &impl RouterTransport,
    url: &str,
    headers: &[(&'static str, String)],
    body: &Value,
    timeout: Duration,
) -> Result<Reply, InferenceError> {
    let secs = timeout.as_secs().max(1);
    let (status, v) = transport
        .post(url, headers, body, secs)
        .await
        .map_err(|_| InferenceError::RouterUnreachable)?;
    match status {
        200..=299 => parse_reply(&v),
        503 => Err(InferenceError::BackendUnavailable),
        s => Err(InferenceError::BackendFailed(s)),
    }
}

/// One inference call with retries, against an explicit transport and base (tests use a mock).
pub(crate) async fn infer_with(
    transport: &impl RouterTransport,
    base: &str,
    persona: &str,
    req: &InferenceRequest,
    inputs: &PolicyInputs,
) -> Result<InferenceOutcome, InferenceError> {
    let start = Instant::now();
    let mut usage = Usage::default();
    let mut attempts = 0u8;
    let result = infer_inner(
        transport,
        base,
        persona,
        req,
        inputs,
        &mut usage,
        &mut attempts,
    )
    .await;
    let latency_ms = start.elapsed().as_millis() as u64;
    let outcome = result.map(|(mut o, tier)| {
        o.usage = usage;
        o.attempts = attempts;
        o.latency_ms = latency_ms;
        o.cost_usd = cost_of(tier, usage);
        o
    });
    log_call(req, &outcome, usage, attempts, latency_ms);
    outcome
}

fn cost_of(tier: Tier, u: Usage) -> f64 {
    llm_router::tier_cost(
        tier,
        &llm_router::RouterConfig::default(),
        u.prompt_tokens,
        u.completion_tokens,
    )
}

async fn infer_inner(
    transport: &impl RouterTransport,
    base: &str,
    persona: &str,
    req: &InferenceRequest,
    inputs: &PolicyInputs,
    usage: &mut Usage,
    attempts: &mut u8,
) -> Result<(InferenceOutcome, Tier), InferenceError> {
    if loopback_base(base).as_deref() != Some(base) {
        return Err(InferenceError::NotLoopback);
    }
    let headers = policy_headers(req.class, req.policy, inputs)?;
    let may_use_cloud = cloud_authorized(&headers);
    let mut body = request_body(req, persona)?;
    let url = format!("{base}/v1/chat/completions");
    let deadline = Instant::now() + req.limits.timeout;
    let cancel = req.cancel.clone().unwrap_or_default();
    let mut last_err = InferenceError::Timeout;

    for n in 0..=req.limits.retries {
        if cancel.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(InferenceError::Timeout);
        }
        *attempts = n + 1;
        let attempt = one_attempt(transport, &url, &headers, &body, left);
        let r = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(InferenceError::Cancelled),
            r = tokio::time::timeout(left, attempt) => r.map_err(|_| InferenceError::Timeout)?,
        };
        let err = match r {
            Ok(reply) => {
                usage.prompt_tokens = usage
                    .prompt_tokens
                    .saturating_add(reply.usage.prompt_tokens);
                usage.completion_tokens = usage
                    .completion_tokens
                    .saturating_add(reply.usage.completion_tokens);
                if reply.tier == Tier::Claude && !may_use_cloud {
                    // Defence in depth: the router should never do this. Don't use the answer.
                    return Err(InferenceError::UnexpectedTier);
                }
                match check_output(req, &reply) {
                    Ok(json) => {
                        let tier = reply.tier;
                        return Ok((
                            InferenceOutcome {
                                content: reply.content,
                                json,
                                tool_calls: reply.tool_calls,
                                tier,
                                reason: reply.reason,
                                usage: Usage::default(),
                                cost_usd: 0.0,
                                latency_ms: 0,
                                attempts: 0,
                            },
                            tier,
                        ));
                    }
                    Err(e) => {
                        adjust_for_retry(&mut body, &e, &reply.content);
                        e
                    }
                }
            }
            Err(e) => e,
        };
        if !err.retryable() {
            return Err(err);
        }
        last_err = err;
        if n < req.limits.retries {
            let pause = Duration::from_millis(250 * (n as u64 + 1));
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(InferenceError::Cancelled),
                _ = tokio::time::sleep(pause.min(deadline.saturating_duration_since(Instant::now()))) => {}
            }
        }
    }
    Err(last_err)
}

/// Before a retry: an invalid structured reply gets one corrective turn (path and rule only), and
/// an empty reply is retried a little warmer.
fn adjust_for_retry(body: &mut Value, e: &InferenceError, content: &str) {
    match e {
        InferenceError::InvalidOutput(why) => {
            if let Some(m) = body["messages"].as_array_mut() {
                m.push(json!({"role": "assistant", "content": content}));
                m.push(json!({"role": "user", "content": format!(
                    "That reply did not match the required JSON schema ({why}). Reply again with only the JSON object."
                )}));
            }
        }
        InferenceError::EmptyReply => {
            let t = body["temperature"].as_f64().unwrap_or(1.0) as f32;
            if t < EMPTY_RETRY_MIN_TEMPERATURE {
                body["temperature"] = json!(EMPTY_RETRY_MIN_TEMPERATURE);
            }
        }
        _ => {}
    }
}

// ─── Logging and usage (one place) ────────────────────────────────────────────

fn log_call(
    req: &InferenceRequest,
    outcome: &Result<InferenceOutcome, InferenceError>,
    usage: Usage,
    attempts: u8,
    latency_ms: u64,
) {
    // Fixed labels and numbers only: no prompt, reply or schema text.
    let (tier, result, reason) = match outcome {
        Ok(o) => (o.tier, "ok", o.reason.as_str()),
        Err(e) => (Tier::Light, e.kind(), ""),
    };
    eprintln!(
        "[inference] purpose={} class={:?} result={} attempts={} latency_ms={} tokens={}+{}",
        req.purpose,
        req.class,
        result,
        attempts,
        latency_ms,
        usage.prompt_tokens,
        usage.completion_tokens
    );
    // Tokens used by failed attempts still count.
    if usage == Usage::default() {
        return;
    }
    let cost = cost_of(tier, usage);
    record_usage(llm_router::LlmUsageRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        tier,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        estimated_cost_usd: cost,
        latency_ms,
        routing_reason: format!("inference:{}:{}; {}", req.purpose, result, reason)
            .trim_end_matches("; ")
            .to_string(),
    });
}

#[cfg(not(test))]
fn record_usage(r: llm_router::LlmUsageRecord) {
    llm_router::append_usage_record(r);
}

// Tests never write the real app-data usage log.
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_USAGE: std::cell::RefCell<Vec<llm_router::LlmUsageRecord>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_usage(r: llm_router::LlmUsageRecord) {
    TEST_USAGE.with(|u| u.borrow_mut().push(r));
}

// ─── Real wiring ──────────────────────────────────────────────────────────────

/// The live policy inputs: the saved Allow Claude setting and compute mode, the kill switch, the
/// daily cap, and the navy switch.
pub(crate) fn live_policy_inputs() -> PolicyInputs {
    let saved = crate::commands::telegram::load_config();
    let mut cfg = llm_router::RouterConfig::default();
    if let Some(m) = saved.compute_mode.filter(|m| !m.trim().is_empty()) {
        cfg.mode = m;
    }
    PolicyInputs {
        gate: llm_router::claude_gate(&cfg, llm_router::daily_spend_today()),
        local_only_switch: crate::commands::telegram::local_only_on(),
    }
}

/// The saved router base (loopback only; anything else falls back to the default).
pub(crate) fn live_base() -> String {
    let saved = crate::commands::telegram::load_config();
    llm_router::router_base(None, saved.amara_router_url.as_deref())
}

/// One call over the real loopback transport.
pub(crate) async fn infer(req: &InferenceRequest) -> Result<InferenceOutcome, InferenceError> {
    let transport = llm_router::loopback_transport(Some(llm_router::DEEP_BODY_CAP));
    let persona = if req.persona {
        llm_router::amara_persona()
    } else {
        String::new()
    };
    infer_with(
        &transport,
        &live_base(),
        &persona,
        req,
        &live_policy_inputs(),
    )
    .await
}

// ─── Capabilities ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TierCapabilities {
    pub available: bool,
    pub local: bool,
    pub tools: bool,
    pub json_schema: bool,
    pub thinking: bool,
    pub vision: bool,
    pub context_length: Option<u64>,
    pub num_ctx: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Capabilities {
    pub fast: TierCapabilities,
    pub deep: TierCapabilities,
    pub claude: TierCapabilities,
}

impl Capabilities {
    pub fn for_class(&self, c: ModelClass) -> &TierCapabilities {
        match c {
            ModelClass::Fast => &self.fast,
            ModelClass::Deep => &self.deep,
            ModelClass::Cloud => &self.claude,
        }
    }
}

/// What each tier can do, as the router measured it (`/amara/capabilities`). `Ok(None)` when the
/// running router predates the endpoint: unknown, not "no".
pub(crate) async fn capabilities_with(
    transport: &impl RouterTransport,
    base: &str,
) -> Result<Option<Capabilities>, InferenceError> {
    if loopback_base(base).as_deref() != Some(base) {
        return Err(InferenceError::NotLoopback);
    }
    let (status, v) = transport
        .post(&format!("{base}/amara/capabilities"), &[], &json!({}), 5)
        .await
        .map_err(|_| InferenceError::RouterUnreachable)?;
    match status {
        200..=299 => serde_json::from_value(v["tiers"].clone())
            .map(Some)
            .map_err(|_| InferenceError::BackendFailed(status)),
        404 | 405 => Ok(None),
        s => Err(InferenceError::BackendFailed(s)),
    }
}

pub(crate) async fn capabilities() -> Result<Option<Capabilities>, InferenceError> {
    let transport = llm_router::loopback_transport(Some(llm_router::DEEP_BODY_CAP));
    capabilities_with(&transport, &live_base()).await
}

#[cfg(test)]
mod tests;
