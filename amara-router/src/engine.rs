//! Routing + upstream calls: classifier, Ollama streaming with `<think>` stripping, the
//! fallback plan (claude → deep → fast, deep → fast), model warming, and the route log.

use crate::config::{Config, KeepAlive, Secrets};
use crate::route::{self, CloudGate, Decision, Hints, PreRoute, Tier};
use crate::think::ThinkStripper;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

/// `Retry-After` (seconds) sent with the [`LOADING_MSG`] 503.
pub const RETRY_AFTER_SECS: u64 = 10;

/// How long a background warm-up may take (a cold load on low RAM was seen at 142 s).
const WARM_TIMEOUT: Duration = Duration::from_secs(300);
/// How long a model may take to leave `/api/ps` once told to unload.
const UNLOAD_WAIT: Duration = Duration::from_secs(30);
const SWAP_POLL: Duration = Duration::from_millis(100);
/// After a failed deep load, deep requests go straight to fast this long before trying again.
const DEEP_RETRY_AFTER: Duration = Duration::from_secs(60);
/// The last words of amara-voice's keep_warm turn (`amara_voice.py`, `Amara.warm`).
const VOICE_WARM_PING: &str = "Voice mode is starting up. Reply with just: ready.";

pub struct State {
    pub cfg: Config,
    pub secrets: Secrets,
    pub http: reqwest::Client,
    pub outbound_disabled: bool,
    warming_fast: AtomicBool,
    /// The one deep load is running.
    warming_deep: AtomicBool,
    log_lock: Mutex<()>,
    /// Fast replies open right now.
    fast_in_flight: Arc<AtomicUsize>,
    /// Deep requests loading or answering right now.
    deep_in_flight: Arc<AtomicUsize>,
    /// Held while deep is unloaded for fast, so a deep request can't start mid-swap.
    restoring_fast: tokio::sync::Mutex<()>,
    deep_retry_at: Mutex<Option<Instant>>,
    /// When the last deep reply (or load) ended; a fast request leaves deep alone for a grace period.
    last_deep_use: Arc<Mutex<Option<Instant>>>,
    /// Fast's `size_vram` (bytes) the last time `/api/ps` listed it, for the headroom check.
    fast_vram_seen: AtomicU64,
    fast_watch_running: AtomicBool,
}

/// Counts one open reply until dropped.
struct InFlight(Arc<AtomicUsize>, Option<Arc<Mutex<Option<Instant>>>>);

impl InFlight {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(count), None)
    }

    /// Like [`Self::new`], and stamps `last_use` when dropped (the end of a deep reply).
    fn stamped(count: &Arc<AtomicUsize>, last_use: &Arc<Mutex<Option<Instant>>>) -> Self {
        let mut f = Self::new(count);
        f.1 = Some(Arc::clone(last_use));
        f
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(stamp) = &self.1 {
            *stamp.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
        }
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What a fast request does about a resident deep model.
#[derive(Debug, PartialEq)]
enum Restore {
    /// Leave deep alone (not loaded, fast already there, or both fit the GPU budget).
    Keep,
    /// Deep was used within the grace period and does not fit beside fast: wait this long.
    Wait(Duration),
    /// Move deep out of memory.
    Unload,
}

/// Decides [`Restore`] from `/api/ps` (`(model, size_vram bytes)`), the time since deep was last
/// used (None: never, e.g. loaded before this router started), the grace period, fast's last seen
/// size (0 = unknown, so no headroom is assumed) and `gpu_budget_gb`.
fn plan_restore(
    ps: &[(String, u64)],
    (fast, deep): (&str, &str),
    since_deep_use: Option<Duration>,
    grace: Duration,
    fast_vram_seen: u64,
    gpu_budget_gb: f64,
) -> Restore {
    let has = |m: &str| ps.iter().any(|(n, _)| same_model(n, m));
    if !has(deep) || has(fast) {
        return Restore::Keep;
    }
    let loaded: u64 = ps.iter().map(|(_, b)| *b).sum();
    let budget = (gpu_budget_gb * 1e9) as u64;
    if fast_vram_seen > 0 && loaded.saturating_add(fast_vram_seen) <= budget {
        return Restore::Keep;
    }
    match since_deep_use {
        Some(age) if age < grace => Restore::Wait(grace - age),
        _ => Restore::Unload,
    }
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
            warming_fast: AtomicBool::new(false),
            warming_deep: AtomicBool::new(false),
            log_lock: Mutex::new(()),
            fast_in_flight: Arc::new(AtomicUsize::new(0)),
            deep_in_flight: Arc::new(AtomicUsize::new(0)),
            restoring_fast: tokio::sync::Mutex::new(()),
            deep_retry_at: Mutex::new(None),
            last_deep_use: Arc::new(Mutex::new(None)),
            fast_vram_seen: AtomicU64::new(0),
            fast_watch_running: AtomicBool::new(false),
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
            PreRoute::Classify => {
                let t = Duration::from_millis(self.cfg.routing.residency_check_timeout_ms);
                let resident = self.ps_within(t).await.map(|ps| {
                    let has = |m: &str| ps.iter().any(|(n, _)| same_model(n, m));
                    (has(&self.cfg.models.fast), has(&self.cfg.models.deep))
                });
                let hard = self.cfg.routing.auto_deep && route::looks_hard(text).is_some();
                let decided = |tier, reason: &str| Decision {
                    tier,
                    reason: reason.into(),
                };
                match resident {
                    Ok((true, _)) => {
                        let d = self.classify(text).await;
                        if !self.cfg.routing.auto_deep && d.tier == Tier::Deep {
                            decided(Tier::Fast, "classifier_hard_auto_deep_off")
                        } else {
                            d
                        }
                    }
                    // Fast not loaded: the classifier would only time out.
                    // A load is needed either way, so load the right model: deep for a
                    // hard-looking prompt (no parallel fast warm competing for RAM), else fast.
                    Ok((false, _)) => {
                        if hard {
                            decided(Tier::Deep, "fast_not_resident_hard_to_deep")
                        } else {
                            self.spawn_warm(Tier::Fast);
                            decided(Tier::Fast, "classifier_skipped_fast_not_resident")
                        }
                    }
                    // /api/ps slow or down (e.g. mid model swap): residency unknown.
                    Err(_) => {
                        if hard {
                            decided(Tier::Deep, "ps_unavailable_hard_to_deep")
                        } else {
                            decided(Tier::Fast, "classifier_skipped_ps_unavailable")
                        }
                    }
                }
            }
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
        // Held until the reply ends: deep work holds fast back, and the other way round.
        let in_flight = if tier == Tier::Fast {
            self.enter_fast().await?
        } else {
            InFlight::stamped(&self.deep_in_flight, &self.last_deep_use)
        };
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
        // Short timeout only while the model is (or may be) loading; a resident model can be
        // silent for a long prompt evaluation.
        let resident = self
            .ps_within(Duration::from_millis(
                self.cfg.routing.residency_check_timeout_ms,
            ))
            .await
            .is_ok_and(|ps| ps.iter().any(|(n, _)| same_model(n, self.model_for(tier))));
        let timeout = Duration::from_millis(if resident {
            self.cfg.loading.first_token_timeout_resident_ms
        } else {
            self.cfg.loading.first_token_timeout_ms
        });
        match tokio::time::timeout(timeout, first_byte).await {
            Ok(Ok((first, rest))) => {
                let events = ndjson_events(futures::stream::iter(first).chain(rest).boxed());
                Ok(events
                    .map(move |ev| {
                        let _held_until_the_reply_ends = &in_flight;
                        ev
                    })
                    .boxed())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Dropping our request may cancel Ollama's load; keep loading on our own.
                if tier == Tier::Fast {
                    self.spawn_warm(tier);
                }
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
        if decision.tier == Tier::Fast
            && is_voice_warm_ping(body)
            && (self.deep_busy() || self.deep_resident().await)
        {
            let reason = format!("{}; warm_ping_skipped", decision.reason);
            return Ok((ready_reply(), Tier::Fast, reason));
        }
        let plan: &[Tier] = match decision.tier {
            Tier::Claude => &[Tier::Claude, Tier::Deep, Tier::Fast],
            Tier::Deep => &[Tier::Deep, Tier::Fast],
            Tier::Fast => &[Tier::Fast],
        };
        let mut reason = decision.reason.clone();
        let mut all_unreachable = true;
        let mut last = String::new();
        for &tier in plan {
            // Counted from here, so fast requests hold back through the load and the answer.
            let _deep_work = (tier == Tier::Deep)
                .then(|| InFlight::stamped(&self.deep_in_flight, &self.last_deep_use));
            if tier == Tier::Deep && !self.ensure_deep().await {
                reason.push_str("; deep_unavailable");
                continue;
            }
            match self.open_tier(tier, body).await {
                Ok(s) => return Ok((strip_stream(s), tier, reason)),
                Err(e) => {
                    let (unreach, msg) = match e {
                        UpstreamError::Unreachable(m) => (true, m),
                        UpstreamError::Failed(m) => (false, m),
                        UpstreamError::Loading if tier == Tier::Deep => {
                            eprintln!("[amara-router] deep tier: no first token in time");
                            reason.push_str("; deep_unavailable");
                            continue;
                        }
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
        self.warm_within(tier, WARM_TIMEOUT).await
    }

    async fn warm_within(&self, tier: Tier, timeout: Duration) -> Result<(), String> {
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
            .timeout(timeout)
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
        // While deep works, fast loads only for a reply, and that reply waits for deep first.
        if tier != Tier::Fast
            || self.deep_busy()
            || self.warming_flag(tier).swap(true, Ordering::SeqCst)
        {
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

    /// Deep work is loading or answering, so fast must stay out of memory.
    fn deep_busy(&self) -> bool {
        self.warming_deep.load(Ordering::SeqCst) || self.deep_in_flight.load(Ordering::SeqCst) > 0
    }

    async fn deep_resident(&self) -> bool {
        let timeout = Duration::from_millis(self.cfg.routing.residency_check_timeout_ms);
        self.ps_within(timeout)
            .await
            .is_ok_and(|ps| ps.iter().any(|(n, _)| same_model(n, &self.cfg.models.deep)))
    }

    /// Waits for deep to be resident, loading it if needed. False once the load window has passed
    /// or the last load just failed. The caller may disconnect: the load does not depend on it.
    async fn ensure_deep(self: &Arc<Self>) -> bool {
        drop(self.restoring_fast.lock().await);
        if self.deep_resident().await {
            return true;
        }
        self.spawn_deep_load();
        let window = Duration::from_millis(self.cfg.loading.deep_load_timeout_ms);
        let deadline = Instant::now() + window + Duration::from_secs(1);
        while self.warming_deep.load(Ordering::SeqCst) && Instant::now() < deadline {
            tokio::time::sleep(SWAP_POLL).await;
        }
        self.deep_resident().await
    }

    /// Starts the one deep load, unless it is running or just failed. It runs in its own task, so
    /// no client disconnecting can cancel it. Never blocks.
    fn spawn_deep_load(self: &Arc<Self>) {
        let retry_at = *self.deep_retry_at.lock().unwrap_or_else(|p| p.into_inner());
        if retry_at.is_some_and(|at| Instant::now() < at)
            || self.warming_deep.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let loaded = me.load_deep().await;
            me.warming_deep.store(false, Ordering::SeqCst);
            match loaded {
                Ok(()) => me.watch_for_deep_unload(),
                Err(e) => {
                    eprintln!("[amara-router] deep load failed, using fast: {e}");
                    *me.deep_retry_at.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some(Instant::now() + DEEP_RETRY_AFTER);
                    // Fast may be out of memory by now. With the requester still counted this
                    // does nothing, and the watcher brings fast back if it never asks for it.
                    me.spawn_warm(Tier::Fast);
                    me.watch_for_deep_unload();
                }
            }
        });
    }

    /// Fast and deep do not fit together, and Ollama's own eviction can hang: asked to load deep
    /// while fast (keep_alive -1) is in use, it waits for an unload that a later fast request
    /// calls off, for hours, with fast still answering. So the swap is done by hand: wait for fast
    /// to go quiet, unload it, confirm it left, and only then ask for deep.
    async fn load_deep(&self) -> Result<(), String> {
        let window = Duration::from_millis(self.cfg.loading.deep_load_timeout_ms);
        let deadline = Instant::now() + window;
        // A fast reply counts itself before checking `deep_busy`, so one sees the other.
        while self.fast_in_flight.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                return Err("fast stayed busy".into());
            }
            tokio::time::sleep(SWAP_POLL).await;
        }
        let ps = self.ps_within(Duration::from_secs(3)).await?;
        if ps.iter().any(|(n, _)| same_model(n, &self.cfg.models.deep)) {
            return Ok(());
        }
        if ps.iter().any(|(n, _)| same_model(n, &self.cfg.models.fast)) {
            self.unload_and_confirm(Tier::Fast).await?;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        self.warm_within(Tier::Deep, left).await
    }

    /// Waits for deep work to end, then makes sure fast can load. Returns with the reply counted.
    /// Past `fast_wait_ms` the caller gets the 503 with `Retry-After`.
    async fn enter_fast(&self) -> Result<InFlight, UpstreamError> {
        let deadline = Instant::now() + Duration::from_millis(self.cfg.loading.fast_wait_ms);
        loop {
            let in_flight = InFlight::new(&self.fast_in_flight);
            // A deep request counts itself before it takes this lock, so it can't slip past.
            let restoring = self.restoring_fast.lock().await;
            if !self.deep_busy() && self.restore_fast().await.is_none() {
                return Ok(in_flight);
            }
            drop(restoring);
            drop(in_flight);
            if Instant::now() >= deadline {
                eprintln!("[amara-router] fast reply gave up waiting for deep to finish");
                return Err(UpstreamError::Loading);
            }
            tokio::time::sleep(SWAP_POLL).await;
        }
    }

    /// Deep is idle but in memory and fast is not: move deep out by hand, for the same reason as
    /// [`Self::load_deep`], unless it was used within `deep_grace_secs` or both fit
    /// `gpu_budget_gb`. Returns how long the caller should wait before asking again.
    async fn restore_fast(&self) -> Option<Duration> {
        let timeout = Duration::from_millis(self.cfg.routing.residency_check_timeout_ms);
        let Ok(ps) = self.ps_within(timeout).await else {
            return None;
        };
        let since = self
            .last_deep_use
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map(|t| t.elapsed());
        let plan = plan_restore(
            &ps,
            (&self.cfg.models.fast, &self.cfg.models.deep),
            since,
            Duration::from_secs(self.cfg.loading.deep_grace_secs),
            self.fast_vram_seen.load(Ordering::SeqCst),
            self.cfg.loading.gpu_budget_gb,
        );
        match plan {
            Restore::Keep => None,
            Restore::Wait(d) => Some(d),
            Restore::Unload => {
                if let Err(e) = self.unload_and_confirm(Tier::Deep).await {
                    eprintln!("[amara-router] could not unload deep for fast: {e}");
                }
                None
            }
        }
    }

    async fn unload_and_confirm(&self, tier: Tier) -> Result<(), String> {
        self.unload(tier).await?;
        let model = self.model_for(tier);
        let deadline = Instant::now() + UNLOAD_WAIT;
        loop {
            let ps = self.ps_within(Duration::from_secs(3)).await?;
            if !ps.iter().any(|(n, _)| same_model(n, model)) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("{} did not unload", tier.as_str()));
            }
            tokio::time::sleep(SWAP_POLL).await;
        }
    }

    /// Asks Ollama to drop `tier`'s model now.
    async fn unload(&self, tier: Tier) -> Result<(), String> {
        let body = json!({"model": self.model_for(tier), "keep_alive": 0});
        let resp = self
            .http
            .post(self.cfg.ollama("/api/generate"))
            .json(&body)
            .timeout(UNLOAD_WAIT)
            .send()
            .await
            .map_err(|_| format!("unload {}: unreachable or timed out", tier.as_str()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("unload {}: HTTP {}", tier.as_str(), resp.status()))
        }
    }

    /// Once deep's keep-alive ends, nothing asks for fast until the next message: bring it back.
    pub fn watch_for_deep_unload(self: &Arc<Self>) {
        if !self.cfg.loading.rewarm_fast_after_deep
            || self.fast_watch_running.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let me = Arc::clone(self);
        let every = Duration::from_secs(self.cfg.loading.poll_secs.max(1));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Ok(ps) = me.ps_within(Duration::from_secs(3)).await else {
                    continue;
                };
                let has = |m: &str| ps.iter().any(|(n, _)| same_model(n, m));
                if !has(&me.cfg.models.deep) && !me.deep_busy() {
                    if !has(&me.cfg.models.fast) {
                        me.spawn_warm(Tier::Fast);
                    }
                    break;
                }
            }
            me.fast_watch_running.store(false, Ordering::SeqCst);
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

    /// Names of the models Ollama has loaded.
    pub async fn loaded_models(&self) -> Result<Vec<String>, String> {
        self.loaded_models_within(Duration::from_secs(3)).await
    }

    async fn loaded_models_within(&self, timeout: Duration) -> Result<Vec<String>, String> {
        Ok(self
            .ps_within(timeout)
            .await?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    /// `/api/ps` as (model name, size_vram bytes).
    async fn ps_within(&self, timeout: Duration) -> Result<Vec<(String, u64)>, String> {
        let resp = self
            .http
            .get(self.cfg.ollama("/api/ps"))
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| "ps: unreachable or timed out".to_string())?;
        if !resp.status().is_success() {
            return Err(format!("ps: HTTP {}", resp.status()));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|_| "ps: unparseable reply".to_string())?;
        let ps: Vec<(String, u64)> = v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let name = m["model"].as_str().or_else(|| m["name"].as_str())?;
                Some((name.to_string(), m["size_vram"].as_u64().unwrap_or(0)))
            })
            .collect();
        if let Some((_, bytes)) = ps
            .iter()
            .find(|(n, b)| *b > 0 && same_model(n, &self.cfg.models.fast))
        {
            self.fast_vram_seen.store(*bytes, Ordering::SeqCst);
        }
        Ok(ps)
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

/// amara-voice's keep_warm turn, the latest user message of the request.
fn is_voice_warm_ping(body: &Value) -> bool {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|m| m["role"] == "user")
        .is_some_and(|m| crate::translate::content_text(&m["content"]).contains(VOICE_WARM_PING))
}

/// The answer the ping asks for, without touching a model.
fn ready_reply() -> EvStream {
    let delta = Ev::Delta {
        content: "ready.".into(),
        tool_calls: vec![],
    };
    let done = Ev::Done {
        done_reason: "stop".into(),
        prompt_tokens: 0,
        completion_tokens: 0,
    };
    futures::stream::iter([Ok(delta), Ok(done)]).boxed()
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

    fn ps_(i: &[(&str, u64)]) -> Vec<(String, u64)> {
        i.iter().map(|(n, b)| (n.to_string(), *b)).collect()
    }

    #[test]
    fn plan_restore_rules() {
        const G: u64 = 1_000_000_000;
        let secs = Duration::from_secs;
        let plan = |ps: &[(&str, u64)], since, seen, budget| {
            plan_restore(&ps_(ps), ("fast", "deep"), since, secs(60), seen, budget)
        };
        // Fast already resident, or deep absent: nothing to do.
        assert_eq!(
            plan(&[("fast", G), ("deep", G)], None, 0, 21.0),
            Restore::Keep
        );
        assert_eq!(plan(&[], None, 0, 21.0), Restore::Keep);
        // Deep just used, fast size unknown: wait out the rest of the grace.
        assert_eq!(
            plan(&[("deep", 10 * G)], Some(secs(20)), 0, 21.0),
            Restore::Wait(secs(40))
        );
        // Grace over, or deep never used by this router: unload.
        assert_eq!(
            plan(&[("deep", 10 * G)], Some(secs(61)), 0, 21.0),
            Restore::Unload
        );
        assert_eq!(plan(&[("deep", 10 * G)], None, 0, 21.0), Restore::Unload);
        // Fits together: keep, even inside the grace; one byte over: do not.
        assert_eq!(
            plan(&[("deep", 10 * G)], Some(secs(1)), 5 * G, 15.0),
            Restore::Keep
        );
        assert_eq!(
            plan(&[("deep", 10 * G)], None, 5 * G + 1, 15.0),
            Restore::Unload
        );
    }

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
