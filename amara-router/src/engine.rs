//! Routing + upstream calls: classifier, Ollama streaming with `<think>` stripping, the
//! fallback plan (claude → deep → fast, deep → fast), model warming, and the route log.

use crate::config::{Config, KeepAlive, Secrets};
use crate::route::{self, CloudGate, Decision, Hints, PreRoute, Tier};
use crate::think::ThinkStripper;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One normalised reply event.
#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    Delta {
        content: String,
        tool_calls: Vec<Value>,
    },
    Done {
        done_reason: String,
        prompt_tokens: u64,
        completion_tokens: u64,
    },
}

pub type EvStream = BoxStream<'static, Result<Ev, String>>;

/// Why an upstream could not be opened.
#[derive(Debug, Clone)]
pub enum UpstreamError {
    /// Connection refused / DNS / timeout before a response.
    Unreachable(String),
    /// Upstream answered with an error.
    Failed(String),
    /// No first byte within `loading.first_token_timeout_ms` (the model is still loading).
    Loading,
}

/// Fixed client-facing message for a first-token timeout (never includes upstream text).
pub const LOADING_MSG: &str = "Model loading, try again shortly.";

/// How long a background warm-up may take (a cold load on low RAM was seen at 142 s).
const WARM_TIMEOUT: Duration = Duration::from_secs(300);

pub struct State {
    pub cfg: Config,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    pub outbound_disabled: bool,
    rewarm_running: AtomicBool,
    warming_fast: AtomicBool,
    warming_deep: AtomicBool,
    log_lock: Mutex<()>,
}

/// Route metadata exposed to clients (header `X-Amara-Tier` + an `amara` JSON field).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteMeta {
    pub tier: Tier,
    pub reason: String,
    pub routing_ms: u64,
}

impl State {
    pub fn new(cfg: Config, secrets: Secrets, outbound_disabled: bool) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            secrets,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .build()
                .expect("http client"),
            outbound_disabled,
            rewarm_running: AtomicBool::new(false),
            warming_fast: AtomicBool::new(false),
            warming_deep: AtomicBool::new(false),
            log_lock: Mutex::new(()),
        })
    }

    pub fn gate(&self, hints: &Hints) -> CloudGate {
        let key = &self.secrets.anthropic_key;
        CloudGate {
            enabled: self.cfg.cloud.anthropic_enabled,
            outbound_disabled: self.outbound_disabled,
            has_key: key.is_some(),
            caller_allows: hints.allow_claude != Some(false),
        }
    }

    pub fn model_for(&self, tier: Tier) -> &str {
        match tier {
            Tier::Fast => &self.cfg.models.fast,
            Tier::Deep => &self.cfg.models.deep,
            Tier::Claude => &self.cfg.cloud.anthropic_model,
        }
    }

    fn keep_alive(&self, tier: Tier) -> &KeepAlive {
        match tier {
            Tier::Deep => &self.cfg.models.deep_keep_alive,
            _ => &self.cfg.models.fast_keep_alive,
        }
    }

    fn num_ctx(&self, tier: Tier) -> u32 {
        match tier {
            Tier::Deep => self.cfg.models.deep_num_ctx,
            _ => self.cfg.models.fast_num_ctx,
        }
    }

    // ── routing ──────────────────────────────────────────────────────────────

    /// Applies the rules to the latest user message (cleaned in place) and returns the decision.
    pub async fn decide(self: &Arc<Self>, text: &mut String, hints: &Hints) -> (Decision, u64) {
        let start = Instant::now();
        let d = match route::pre_route(&self.cfg.routing, text, hints, &self.gate(hints)) {
            PreRoute::Decided(d) => d,
            PreRoute::Classify => match self.fast_resident().await {
                Ok(true) => self.classify(text).await,
                // Rules 1-3 (commands, phrases, tokens, code) already ran and did not pick
                // deep, so the heuristic answer is fast. Start loading it now.
                not_resident => {
                    self.spawn_warm(Tier::Fast);
                    Decision {
                        tier: Tier::Fast,
                        reason: if not_resident.is_ok() {
                            "classifier_skipped_fast_not_resident".into()
                        } else {
                            "classifier_skipped_ps_unavailable".into()
                        },
                    }
                }
            },
        };
        (d, start.elapsed().as_millis() as u64)
    }

    /// EASY/HARD on the fast model: temperature 0, num_predict 3, hard timeout. Falls back to fast.
    pub async fn classify(&self, text: &str) -> Decision {
        let excerpt: String = text
            .chars()
            .take(self.cfg.routing.classifier_max_chars)
            .collect();
        let options =
            json!({"temperature": 0, "num_predict": 3, "num_ctx": self.cfg.models.fast_num_ctx});
        let body = json!({
            "model": self.cfg.models.fast,
            "messages": [
                {"role": "user", "content": format!("{}{excerpt}\nAnswer:", route::CLASSIFIER_PROMPT)}
            ],
            "format": {"type": "string", "enum": ["EASY", "HARD"]},
            "stream": false,
            "think": false,
            "keep_alive": self.cfg.models.fast_keep_alive.to_json(),
            "options": options
        });
        let timeout = Duration::from_millis(self.cfg.routing.classifier_timeout_ms);
        let call = async {
            let resp = self
                .http
                .post(self.cfg.ollama("/api/chat"))
                .json(&body)
                .send()
                .await
                .map_err(|_| "unreachable".to_string())?;
            let status = resp.status();
            let v: Value = resp
                .json()
                .await
                .map_err(|_| format!("status {status}: unparseable reply"))?;
            if v.get("error").is_some() {
                return Err(format!("status {status}: upstream error"));
            }
            Ok::<String, String>(v["message"]["content"].as_str().unwrap_or("").to_string())
        };
        match tokio::time::timeout(timeout, call).await {
            Ok(Ok(reply)) => route::classify_reply(&crate::think::strip_thinking_tags(&reply)),
            Ok(Err(e)) => {
                eprintln!("[amara-router] classifier error, using fast: {e}");
                Decision {
                    tier: Tier::Fast,
                    reason: "classifier_error".into(),
                }
            }
            Err(_) => Decision {
                tier: Tier::Fast,
                reason: "classifier_timeout".into(),
            },
        }
    }

    // ── upstream ─────────────────────────────────────────────────────────────

    /// Opens a reply stream for `tier`. `body` is a native chat body without `model`.
    async fn open_tier(
        self: &Arc<Self>,
        tier: Tier,
        body: &Value,
    ) -> Result<EvStream, UpstreamError> {
        match tier {
            Tier::Claude => self.open_anthropic(body).await,
            _ => self.open_ollama(tier, body).await,
        }
    }

    async fn open_ollama(
        self: &Arc<Self>,
        tier: Tier,
        body: &Value,
    ) -> Result<EvStream, UpstreamError> {
        let mut b = body.clone();
        b["model"] = json!(self.model_for(tier));
        b["stream"] = json!(true);
        b["keep_alive"] = self.keep_alive(tier).to_json();
        let think = if tier == Tier::Deep {
            self.cfg.models.deep_think
        } else {
            self.cfg.models.fast_think
        };
        if let (true, Some(t)) = (b["think"].is_null(), think) {
            b["think"] = json!(t);
        }
        if !b["options"].is_object() {
            b["options"] = json!({});
        }
        b["options"]["num_ctx"] = json!(self.num_ctx(tier));
        let first_byte = async {
            let resp = self
                .http
                .post(self.cfg.ollama("/api/chat"))
                .json(&b)
                .send()
                .await
                .map_err(|_| UpstreamError::Unreachable("Ollama: unreachable".into()))?;
            if !resp.status().is_success() {
                // Never log or return the upstream body: status plus a fixed reason only.
                let status = resp.status();
                return Err(UpstreamError::Failed(format!(
                    "Ollama {status}: request rejected"
                )));
            }
            let mut bytes = resp.bytes_stream();
            let first = bytes.next().await;
            Ok((first, bytes))
        };
        let timeout = Duration::from_millis(self.cfg.loading.first_token_timeout_ms);
        match tokio::time::timeout(timeout, first_byte).await {
            Ok(Ok((first, rest))) => Ok(ndjson_events(
                futures::stream::iter(first).chain(rest).boxed(),
            )),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Dropping our request may cancel Ollama's load; keep loading on our own.
                self.spawn_warm(tier);
                Err(UpstreamError::Loading)
            }
        }
    }

    async fn open_anthropic(&self, body: &Value) -> Result<EvStream, UpstreamError> {
        // The gate was checked when routing; check again right before the network call.
        if self.outbound_disabled || !self.cfg.cloud.anthropic_enabled {
            return Err(UpstreamError::Failed("cloud disabled".into()));
        }
        let key = self
            .secrets
            .anthropic_key
            .clone()
            .ok_or_else(|| UpstreamError::Failed("no Anthropic key".into()))?;
        let (system, messages) = anthropic_messages(body);
        let mut req = json!({
            "model": self.cfg.cloud.anthropic_model,
            "max_tokens": body["options"]["num_predict"].as_u64().unwrap_or(4096),
            "messages": messages,
        });
        if !system.is_empty() {
            req["system"] = json!(system);
        }
        let resp = self
            .http
            .post(&self.cfg.cloud.anthropic_url)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .json(&req)
            .timeout(Duration::from_secs(90))
            .send()
            .await
            .map_err(|_| UpstreamError::Unreachable("Anthropic: unreachable".into()))?;
        let status = resp.status();
        let v: Value = resp
            .json()
            .await
            .map_err(|_| UpstreamError::Failed(format!("Anthropic {status}: unparseable reply")))?;
        if !status.is_success() || v.get("error").is_some() {
            return Err(UpstreamError::Failed(format!(
                "Anthropic {status}: request rejected"
            )));
        }
        let text: String = v["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| b["text"].as_str())
            .collect();
        let evs = vec![
            Ok(Ev::Delta {
                content: text,
                tool_calls: vec![],
            }),
            Ok(Ev::Done {
                done_reason: if v["stop_reason"] == "max_tokens" {
                    "length"
                } else {
                    "stop"
                }
                .into(),
                prompt_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
                completion_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
            }),
        ];
        Ok(futures::stream::iter(evs).boxed())
    }

    /// Tries `decision.tier` then its fallbacks. Returns the stream (think-stripped), the tier
    /// that answered and the final reason; or a client-facing error.
    pub async fn open(
        self: &Arc<Self>,
        decision: &Decision,
        body: &Value,
    ) -> Result<(EvStream, Tier, String), (u16, String)> {
        let plan: &[Tier] = match decision.tier {
            Tier::Claude => &[Tier::Claude, Tier::Deep, Tier::Fast],
            Tier::Deep => &[Tier::Deep, Tier::Fast],
            Tier::Fast => &[Tier::Fast],
        };
        let mut reason = decision.reason.clone();
        let mut all_unreachable = true;
        let mut last = String::new();
        for &tier in plan {
            match self.open_tier(tier, body).await {
                Ok(s) => return Ok((strip_stream(s), tier, reason)),
                Err(e) => {
                    let (unreach, msg) = match e {
                        UpstreamError::Unreachable(m) => (true, m),
                        UpstreamError::Failed(m) => (false, m),
                        UpstreamError::Loading => {
                            // Do not start a second cold load (it would fight for RAM).
                            eprintln!(
                                "[amara-router] {} tier: no first token in time; still loading",
                                tier.as_str()
                            );
                            return Err((503, LOADING_MSG.to_string()));
                        }
                    };
                    // Anthropic being unreachable says nothing about Ollama.
                    if tier != Tier::Claude {
                        all_unreachable &= unreach;
                    }
                    eprintln!("[amara-router] {} tier failed: {msg}", tier.as_str());
                    reason.push_str(&format!("; {}_failed", tier.as_str()));
                    last = msg;
                }
            }
        }
        if all_unreachable {
            Err((503, format!(
                "Ollama is not reachable at {}. Start Ollama (open the Ollama app or run `ollama serve`) and try again.",
                self.cfg.ollama_url
            )))
        } else {
            Err((502, format!("The model backend returned an error: {last}")))
        }
    }

    // ── loading ──────────────────────────────────────────────────────────────

    /// Loads the fast model (empty generate with its keep_alive and context).
    pub async fn warm_fast(&self) -> Result<(), String> {
        self.warm(Tier::Fast).await
    }

    /// Loads `tier`'s model: an empty-prompt generate with its keep_alive and `num_ctx`.
    pub async fn warm(&self, tier: Tier) -> Result<(), String> {
        if tier == Tier::Claude {
            return Ok(());
        }
        let body = json!({
            "model": self.model_for(tier),
            "prompt": "",
            "stream": false,
            "keep_alive": self.keep_alive(tier).to_json(),
            "options": {"num_ctx": self.num_ctx(tier)},
        });
        let resp = self
            .http
            .post(self.cfg.ollama("/api/generate"))
            .json(&body)
            .timeout(WARM_TIMEOUT)
            .send()
            .await
            .map_err(|_| format!("warm {}: unreachable or timed out", tier.as_str()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("warm {}: HTTP {}", tier.as_str(), resp.status()))
        }
    }

    fn warming_flag(&self, tier: Tier) -> &AtomicBool {
        if tier == Tier::Deep {
            &self.warming_deep
        } else {
            &self.warming_fast
        }
    }

    /// Starts a background warm-up of `tier` unless one is already running. Never blocks.
    pub fn spawn_warm(self: &Arc<Self>, tier: Tier) {
        if tier == Tier::Claude || self.warming_flag(tier).swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(e) = me.warm(tier).await {
                eprintln!("[amara-router] background warm failed: {e}");
            }
            me.warming_flag(tier).store(false, Ordering::SeqCst);
        });
    }

    /// Startup pre-warm (background): fast, then deep if `warm_deep_on_start`. Sequential so
    /// two cold loads never compete for RAM. Returns immediately.
    pub fn prewarm_on_start(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>> {
        let (fast, deep) = (
            self.cfg.loading.warm_fast_on_start,
            self.cfg.loading.warm_deep_on_start,
        );
        if !fast && !deep {
            return None;
        }
        let me = Arc::clone(self);
        Some(tokio::spawn(async move {
            for (on, tier) in [(fast, Tier::Fast), (deep, Tier::Deep)] {
                if !on || me.warming_flag(tier).swap(true, Ordering::SeqCst) {
                    continue;
                }
                if let Err(e) = me.warm(tier).await {
                    eprintln!("[amara-router] could not pre-warm yet: {e}");
                }
                me.warming_flag(tier).store(false, Ordering::SeqCst);
            }
        }))
    }

    /// Whether the fast model is loaded, per `/api/ps` within `residency_check_timeout_ms`.
    pub async fn fast_resident(&self) -> Result<bool, String> {
        let t = Duration::from_millis(self.cfg.routing.residency_check_timeout_ms);
        let loaded = self.loaded_models_within(t).await?;
        Ok(loaded.iter().any(|m| same_model(m, &self.cfg.models.fast)))
    }

    /// Names of the models Ollama has loaded.
    pub async fn loaded_models(&self) -> Result<Vec<String>, String> {
        self.loaded_models_within(Duration::from_secs(3)).await
    }

    async fn loaded_models_within(&self, timeout: Duration) -> Result<Vec<String>, String> {
        let v: Value = self
            .http
            .get(self.cfg.ollama("/api/ps"))
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                m["model"]
                    .as_str()
                    .or_else(|| m["name"].as_str())
                    .map(str::to_string)
            })
            .collect())
    }

    /// After a deep answer: re-warm fast right away (background). If that fails, wait until
    /// deep unloads, then reload fast if it was evicted.
    pub fn schedule_rewarm(self: &Arc<Self>) {
        if !self.cfg.loading.rewarm_fast_after_deep
            || self.rewarm_running.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            if !me.warming_fast.swap(true, Ordering::SeqCst) {
                let r = me.warm(Tier::Fast).await;
                me.warming_fast.store(false, Ordering::SeqCst);
                match r {
                    Ok(()) => {
                        me.rewarm_running.store(false, Ordering::SeqCst);
                        return;
                    }
                    Err(e) => eprintln!("[amara-router] re-warm after deep failed: {e}"),
                }
            }
            let poll = Duration::from_secs(me.cfg.loading.poll_secs.max(1));
            // Give up after ~3 h so a pinned deep model never keeps a task alive forever.
            for _ in 0..(3 * 3600 / poll.as_secs().max(1)) {
                tokio::time::sleep(poll).await;
                let Ok(loaded) = me.loaded_models().await else {
                    continue;
                };
                if loaded.iter().any(|m| same_model(m, &me.cfg.models.deep)) {
                    continue;
                }
                if !loaded.iter().any(|m| same_model(m, &me.cfg.models.fast)) {
                    match me.warm_fast().await {
                        Ok(()) => eprintln!("[amara-router] deep unloaded; fast re-warmed"),
                        Err(e) => eprintln!("[amara-router] re-warm failed: {e}"),
                    }
                }
                break;
            }
            me.rewarm_running.store(false, Ordering::SeqCst);
        });
    }

    // ── log ──────────────────────────────────────────────────────────────────

    /// Appends one JSONL line. Never includes message content.
    pub fn log_route(&self, entry: &Value) {
        let path = crate::config::expand_home(&self.cfg.log.path);
        let _guard = self.log_lock.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut f) = opts.open(&path) {
            let _ = writeln!(f, "{entry}");
        }
    }
}

/// `gemma:latest` and `gemma` name the same model.
pub fn same_model(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        if s.contains(':') {
            s.to_string()
        } else {
            format!("{s}:latest")
        }
    };
    norm(a) == norm(b)
}

/// System text + Anthropic-shaped messages (text only, alternating, starting with user).
fn anthropic_messages(body: &Value) -> (String, Vec<Value>) {
    let mut system = Vec::new();
    let mut msgs: Vec<Value> = Vec::new();
    for m in body["messages"].as_array().into_iter().flatten() {
        let text = crate::translate::content_text(&m["content"]);
        let role = match m["role"].as_str().unwrap_or("user") {
            "system" => {
                system.push(text);
                continue;
            }
            "assistant" => "assistant",
            _ => "user",
        };
        if text.is_empty() {
            continue;
        }
        match msgs.last_mut() {
            Some(last) if last["role"] == role => {
                let joined = format!("{}\n\n{text}", last["content"].as_str().unwrap_or(""));
                last["content"] = json!(joined);
            }
            _ => msgs.push(json!({"role": role, "content": text})),
        }
    }
    if msgs.first().is_some_and(|m| m["role"] == "assistant") {
        msgs.insert(0, json!({"role": "user", "content": "(continue)"}));
    }
    (system.join("\n\n"), msgs)
}

/// Ollama NDJSON bytes → events. The `thinking` field is dropped.
fn ndjson_events<S>(bytes: S) -> EvStream
where
    S: futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + 'static,
{
    async_stream::stream! {
        let mut buf: Vec<u8> = Vec::new();
        let mut done = false;
        futures::pin_mut!(bytes);
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => { yield Err(format!("stream interrupted: {e}")); return; }
            };
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                for ev in parse_line(&line) {
                    if matches!(ev, Ok(Ev::Done { .. })) { done = true; }
                    yield ev;
                }
            }
        }
        for ev in parse_line(&buf) {
            if matches!(ev, Ok(Ev::Done { .. })) { done = true; }
            yield ev;
        }
        if !done {
            yield Err("stream ended before the model finished".to_string());
        }
    }
    .boxed()
}

fn parse_line(line: &[u8]) -> Vec<Result<Ev, String>> {
    let Some(s) = std::str::from_utf8(line)
        .ok()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return vec![];
    };
    let v: Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(e) => return vec![Err(format!("bad upstream line: {e}"))],
    };
    if let Some(err) = v["error"].as_str() {
        return vec![Err(err.to_string())];
    }
    let mut out = Vec::new();
    let content = v["message"]["content"].as_str().unwrap_or("").to_string();
    let tool_calls = v["message"]["tool_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Thinking-only or empty chunks produce no delta.
    if !content.is_empty() || !tool_calls.is_empty() {
        out.push(Ok(Ev::Delta {
            content,
            tool_calls,
        }));
    }
    if v["done"].as_bool() == Some(true) {
        out.push(Ok(Ev::Done {
            done_reason: v["done_reason"].as_str().unwrap_or("stop").to_string(),
            prompt_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
            completion_tokens: v["eval_count"].as_u64().unwrap_or(0),
        }));
    }
    out
}

/// Removes `<think>` blocks from content deltas, holding back partial tags across chunks.
pub fn strip_stream(s: EvStream) -> EvStream {
    async_stream::stream! {
        let mut st = ThinkStripper::default();
        futures::pin_mut!(s);
        while let Some(ev) = s.next().await {
            match ev {
                Ok(Ev::Delta { content, tool_calls }) => {
                    let content = st.push(&content);
                    if !content.is_empty() || !tool_calls.is_empty() {
                        yield Ok(Ev::Delta { content, tool_calls });
                    }
                }
                Ok(done @ Ev::Done { .. }) => {
                    let rest = st.finish();
                    if !rest.is_empty() {
                        yield Ok(Ev::Delta { content: rest, tool_calls: vec![] });
                    }
                    yield Ok(done);
                }
                Err(e) => yield Err(e),
            }
        }
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_model_ignores_latest() {
        assert!(same_model("qwen:latest", "qwen"));
        assert!(!same_model("qwen:7b", "qwen"));
    }

    #[test]
    fn anthropic_shape() {
        let body = json!({"messages": [
            {"role": "system", "content": "S"},
            {"role": "assistant", "content": "hello"},
            {"role": "user", "content": "a"},
            {"role": "tool", "content": "b"}
        ]});
        let (sys, m) = anthropic_messages(&body);
        assert_eq!(sys, "S");
        assert_eq!(m[0]["role"], "user");
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(m[2]["content"], "a\n\nb");
    }

    #[tokio::test]
    async fn ndjson_parse_and_strip() {
        let lines: [&'static str; 3] = [
            "{\"message\":{\"role\":\"assistant\",\"content\":\"<thi\"},\"done\":false}\n{\"message\":{\"content\":\"nk>x</think>\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\",\"thinking\":\"hidden\"},\"done\":false}\n{\"message\":{\"content\":\"Hi\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":5,\"eval_count\":2}",
        ];
        let src = futures::stream::iter(lines.into_iter().map(|l| Ok(bytes::Bytes::from(l))));
        let evs: Vec<_> = strip_stream(ndjson_events(src)).collect().await;
        assert_eq!(
            evs,
            vec![
                Ok(Ev::Delta {
                    content: "Hi".into(),
                    tool_calls: vec![]
                }),
                Ok(Ev::Done {
                    done_reason: "stop".into(),
                    prompt_tokens: 5,
                    completion_tokens: 2
                }),
            ]
        );
    }

    #[tokio::test]
    async fn truncated_stream_reports_error() {
        let src = futures::stream::iter(vec![Ok(bytes::Bytes::from(
            "{\"message\":{\"content\":\"a\"},\"done\":false}\n",
        ))]);
        let evs: Vec<_> = ndjson_events(src).collect().await;
        assert!(evs.last().unwrap().is_err());
    }
}
