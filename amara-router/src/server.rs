//! HTTP surface: OpenAI `/v1/chat/completions` + `/v1/models`, Ollama `/api/chat`,
//! `/api/tags`, `/api/show`, `/api/version`, `/api/pull`, plus `/health` and the dry-run
//! `/amara/route`.
//!
//! Route metadata for clients (e.g. the Nexus Chat Fast/Deep badge):
//! * headers `X-Amara-Route: fast|deep|claude` (and the older alias `X-Amara-Tier`),
//!   `X-Amara-Model: <model id>`, `X-Amara-Route-Reason`, `X-Amara-Routing-Ms` on every chat
//!   reply once a tier is chosen: streamed, non-streamed, and errors (502/503) alike. After a
//!   fallback they name the tier and model that actually answered.
//! * the JSON `model` field (OpenAI object and every chunk, Ollama object and every line) is
//!   the real model id that answered, not the virtual name the caller sent.
//! * an `amara` object `{tier, reason, routing_ms}` in the JSON reply, in the first and the
//!   last (finish_reason) SSE chunk, and in the final (`done: true`) Ollama NDJSON line.

use crate::engine::{Ev, RouteMeta, State};
use crate::route::{parse_tier, Hints, Tier};
use crate::translate;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State as AxState};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

type St = Arc<State>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Api {
    OpenAi,
    Ollama,
}

impl Api {
    fn name(self) -> &'static str {
        match self {
            Api::OpenAi => "openai",
            Api::Ollama => "ollama",
        }
    }
}

pub const VIRTUAL_MODELS: [&str; 3] = ["amara:latest", "amara-fast:latest", "amara-deep:latest"];

pub fn router(state: St) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/models", get(openai_models))
        .route("/api/chat", post(ollama_chat))
        .route("/api/tags", get(ollama_tags))
        .route("/api/show", post(ollama_show))
        .route("/api/version", get(ollama_version))
        .route("/api/pull", post(ollama_pull))
        .route("/amara/route", post(dry_route))
        .with_state(state)
}

/// The loopback listener. It has no key, so it refuses what a web page could send it: a Host other
/// than loopback (DNS rebinding) and any request carrying an Origin header (cross-site fetches from a
/// browser). HA, Hermes and Nexus's Rust client send neither.
pub fn local_router(state: St) -> Router {
    router(state).layer(middleware::from_fn(|req: Request, next: Next| async move {
        let headers = req.headers();
        if headers.contains_key(header::ORIGIN) || !loopback_host(headers) {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(json!({"error": {"message":
                "amara-router only accepts local, non-browser requests on this port"}})),
            )
                .into_response();
        }
        next.run(req).await
    }))
}

fn loopback_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    name.eq_ignore_ascii_case("localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The LAN listener: same routes, every request needs `Authorization: Bearer <key>`.
pub fn lan_router(state: St, key: String) -> Router {
    let key = Arc::new(key);
    router(state).layer(middleware::from_fn(move |req: Request, next: Next| {
        let key = Arc::clone(&key);
        async move {
            let given = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or("");
            if constant_time_eq(given.as_bytes(), key.as_bytes()) {
                next.run(req).await
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(json!({"error": "unauthorized"})),
                )
                    .into_response()
            }
        }
    }))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn hints_from(headers: &HeaderMap, model: &str) -> Hints {
    let h = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    };
    let route = h("x-amara-route").and_then(parse_tier);
    Hints {
        route,
        route_source: if route.is_some() { "header" } else { "" },
        allow_claude: h("x-amara-allow-claude")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")),
        tags: h("x-amara-tags")
            .map(|v| {
                v.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        mode: h("x-amara-mode").map(|v| v.to_ascii_lowercase()),
    }
    .from_model(model)
}

fn error_response(api: Api, status: u16, msg: &str) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = match api {
        Api::OpenAi => {
            json!({"error": {"message": msg, "type": "amara_router_error", "code": status}})
        }
        Api::Ollama => json!({"error": msg}),
    };
    let mut r = (code, axum::Json(body)).into_response();
    if status == 503 && msg == crate::engine::LOADING_MSG {
        r.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from(crate::engine::RETRY_AFTER_SECS),
        );
    }
    r
}

/// Index of the latest user message and its text.
fn last_user(body: &Value) -> Option<(usize, String)> {
    body["messages"]
        .as_array()?
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, m)| {
            (m["role"] == "user").then(|| (i, translate::content_text(&m["content"])))
        })
}

fn set_user_text(body: &mut Value, idx: usize, text: &str) {
    let content = &mut body["messages"][idx]["content"];
    match content {
        Value::Array(parts) => {
            // Replace the first text part, drop the other text parts (they were joined).
            let mut first = true;
            parts.retain_mut(|p| {
                if p["type"] != "text" {
                    return true;
                }
                if first {
                    first = false;
                    p["text"] = json!(text);
                    true
                } else {
                    false
                }
            });
        }
        _ => *content = json!(text),
    }
}

async fn openai_chat(AxState(st): AxState<St>, headers: HeaderMap, body: Bytes) -> Response {
    let req: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(Api::OpenAi, 400, &format!("invalid JSON: {e}")),
    };
    let (native, stream, include_usage) = match translate::openai_to_native(&req) {
        Ok(x) => x,
        Err(e) => return error_response(Api::OpenAi, 400, &e),
    };
    let model = req["model"].as_str().unwrap_or("amara").to_string();
    chat(
        st,
        &headers,
        Api::OpenAi,
        native,
        model,
        stream,
        include_usage,
    )
    .await
}

async fn ollama_chat(AxState(st): AxState<St>, headers: HeaderMap, body: Bytes) -> Response {
    let mut req: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(Api::Ollama, 400, &format!("invalid JSON: {e}")),
    };
    if !req["messages"].is_array() {
        return error_response(Api::Ollama, 400, "`messages` must be an array");
    }
    let model = req["model"].as_str().unwrap_or("amara").to_string();
    // Ollama's default is streaming.
    let stream = req["stream"].as_bool().unwrap_or(true);
    if let Some(o) = req.as_object_mut() {
        for k in ["model", "stream", "keep_alive"] {
            o.remove(k);
        }
    }
    chat(st, &headers, Api::Ollama, req, model, stream, false).await
}

#[allow(clippy::too_many_arguments)]
async fn chat(
    st: St,
    headers: &HeaderMap,
    api: Api,
    mut body: Value,
    model: String,
    stream: bool,
    include_usage: bool,
) -> Response {
    let start = Instant::now();
    let hints = hints_from(headers, &model);
    let user = last_user(&body);
    let mut text = user.as_ref().map(|u| u.1.clone()).unwrap_or_default();
    let (decision, routing_ms) = st.decide(&mut text, &hints).await;
    if let Some((idx, orig)) = &user {
        if &text != orig {
            set_user_text(&mut body, *idx, &text);
        }
    }

    let base_log = json!({
        "api": api.name(),
        "stream": stream,
        "requested": decision.tier,
        "routing_ms": routing_ms,
    });
    let waiting = CancelLog::arm(&st, start, &base_log, decision.tier, &decision.reason);
    let opened = st.open(&decision, &body).await;
    waiting.disarm();
    let (events, tier, reason) = match opened {
        Ok(x) => x,
        Err((status, msg)) => {
            let mut entry = base_log;
            entry["ts"] = json!(now_rfc3339());
            entry["route"] = json!(decision.tier);
            entry["reason"] = json!(decision.reason);
            entry["model"] = json!(st.model_for(decision.tier));
            entry["latency_ms"] = json!(start.elapsed().as_millis() as u64);
            entry["status"] = json!("error");
            entry["http_status"] = json!(status);
            st.log_route(&entry);
            let meta = RouteMeta {
                tier: decision.tier,
                reason: decision.reason.clone(),
                routing_ms,
            };
            return with_meta(
                error_response(api, status, &msg),
                &meta,
                st.model_for(decision.tier),
            );
        }
    };
    let meta = RouteMeta {
        tier,
        reason,
        routing_ms,
    };
    let meta_json = serde_json::to_value(&meta).unwrap_or(Value::Null);
    // The model that actually answers (after any fallback); replaces the caller's virtual name.
    let model = st.model_for(tier).to_string();

    let filler =
        (stream && tier != Tier::Fast && st.cfg.filler.enabled && !st.cfg.filler.text.is_empty())
            .then(|| format!("{}\n\n", st.cfg.filler.text.trim()));
    let events = match filler {
        Some(f) => futures::stream::once(async move {
            Ok(Ev::Delta {
                content: f,
                tool_calls: vec![],
            })
        })
        .chain(events)
        .boxed(),
        None => events,
    };

    // Logged once the reply has finished (or failed mid-stream). A client that disconnects
    // drops this closure unrun, and `cancelled` logs that line instead.
    let finish = {
        let cancelled = CancelLog::arm(&st, start, &base_log, meta.tier, &meta.reason);
        let st = Arc::clone(&st);
        let meta = meta.clone();
        let real_model = st.model_for(tier).to_string();
        move |prompt: u64, completion: u64, err: Option<&str>| {
            cancelled.disarm();
            let mut entry = base_log;
            entry["ts"] = json!(now_rfc3339());
            entry["route"] = json!(meta.tier);
            entry["reason"] = json!(meta.reason);
            entry["model"] = json!(real_model);
            entry["latency_ms"] = json!(start.elapsed().as_millis() as u64);
            entry["prompt_tokens"] = json!(prompt);
            entry["completion_tokens"] = json!(completion);
            entry["status"] = json!(if err.is_some() { "error" } else { "ok" });
            st.log_route(&entry);
            if meta.tier == Tier::Deep {
                st.schedule_rewarm();
            }
        }
    };

    let mut resp = if stream {
        let body = match api {
            Api::OpenAi => openai_sse(events, model.clone(), meta_json, include_usage, finish),
            Api::Ollama => ollama_ndjson(events, model.clone(), meta_json, start, finish),
        };
        let ctype = if api == Api::OpenAi {
            "text/event-stream"
        } else {
            "application/x-ndjson"
        };
        let mut r = Response::new(body);
        r.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        r
    } else {
        let collected = collect(events).await;
        finish(
            collected.prompt,
            collected.completion,
            collected.error.as_deref(),
        );
        if let Some(e) = collected
            .error
            .as_ref()
            .filter(|_| collected.content.is_empty() && collected.tool_calls.is_empty())
        {
            return with_meta(error_response(api, 502, e), &meta, &model);
        }
        let v = match api {
            Api::OpenAi => openai_json(&collected, &model, meta_json),
            Api::Ollama => ollama_json(&collected, &model, meta_json, start),
        };
        axum::Json(v).into_response()
    };
    resp = with_meta(resp, &meta, &model);
    resp
}

/// Writes a `status: "cancelled"` route-log line if dropped while armed. Axum drops the
/// handler or the body stream when the client disconnects, so no other log line is written.
struct CancelLog {
    st: St,
    start: Instant,
    entry: Option<Value>,
}

impl CancelLog {
    fn arm(st: &St, start: Instant, base: &Value, tier: Tier, reason: &str) -> Self {
        let mut entry = base.clone();
        entry["route"] = json!(tier);
        entry["reason"] = json!(reason);
        entry["model"] = json!(st.model_for(tier));
        Self {
            st: Arc::clone(st),
            start,
            entry: Some(entry),
        }
    }

    fn disarm(mut self) {
        self.entry = None;
    }
}

impl Drop for CancelLog {
    fn drop(&mut self) {
        if let Some(mut entry) = self.entry.take() {
            entry["ts"] = json!(now_rfc3339());
            entry["latency_ms"] = json!(self.start.elapsed().as_millis() as u64);
            entry["status"] = json!("cancelled");
            self.st.log_route(&entry);
        }
    }
}

fn with_meta(mut r: Response, meta: &RouteMeta, model: &str) -> Response {
    let h = r.headers_mut();
    h.insert(
        "x-amara-route",
        HeaderValue::from_static(meta.tier.as_str()),
    );
    h.insert("x-amara-tier", HeaderValue::from_static(meta.tier.as_str()));
    if let Ok(v) = HeaderValue::from_str(model) {
        h.insert("x-amara-model", v);
    }
    if let Ok(v) = HeaderValue::from_str(&meta.reason) {
        h.insert("x-amara-route-reason", v);
    }
    h.insert("x-amara-routing-ms", HeaderValue::from(meta.routing_ms));
    r
}

#[derive(Default)]
struct Collected {
    content: String,
    tool_calls: Vec<Value>,
    done_reason: String,
    prompt: u64,
    completion: u64,
    error: Option<String>,
}

async fn collect(mut events: crate::engine::EvStream) -> Collected {
    let mut c = Collected::default();
    while let Some(ev) = events.next().await {
        match ev {
            Ok(Ev::Delta {
                content,
                tool_calls,
            }) => {
                c.content.push_str(&content);
                c.tool_calls.extend(tool_calls);
            }
            Ok(Ev::Done {
                done_reason,
                prompt_tokens,
                completion_tokens,
            }) => {
                c.done_reason = done_reason;
                c.prompt = prompt_tokens;
                c.completion = completion_tokens;
            }
            Err(e) => {
                c.error = Some(e);
                break;
            }
        }
    }
    c.content = c.content.trim().to_string();
    c
}

fn completion_id() -> String {
    format!(
        "chatcmpl-amara-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    )
}

fn openai_json(c: &Collected, model: &str, meta: Value) -> Value {
    let id = completion_id();
    let mut message = json!({"role": "assistant", "content": c.content});
    if !c.tool_calls.is_empty() {
        message["tool_calls"] = translate::tool_calls_to_openai(&c.tool_calls, &id, false);
    }
    json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": translate::finish_reason(&c.done_reason, !c.tool_calls.is_empty()),
        }],
        "usage": {"prompt_tokens": c.prompt, "completion_tokens": c.completion, "total_tokens": c.prompt + c.completion},
        "amara": meta,
    })
}

fn ollama_json(c: &Collected, model: &str, meta: Value, start: Instant) -> Value {
    let mut message = json!({"role": "assistant", "content": c.content});
    if !c.tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(c.tool_calls.clone());
    }
    json!({
        "model": model,
        "created_at": now_rfc3339(),
        "message": message,
        "done": true,
        "done_reason": if c.done_reason.is_empty() { "stop" } else { &c.done_reason },
        "total_duration": start.elapsed().as_nanos() as u64,
        "prompt_eval_count": c.prompt,
        "eval_count": c.completion,
        "amara": meta,
    })
}

fn openai_sse<F>(
    mut events: crate::engine::EvStream,
    model: String,
    meta: Value,
    include_usage: bool,
    finish: F,
) -> Body
where
    F: FnOnce(u64, u64, Option<&str>) + Send + 'static,
{
    let s = async_stream::stream! {
        let id = completion_id();
        let created = chrono::Utc::now().timestamp();
        let chunk = |delta: Value, finish_reason: Value, extra: Option<&Value>| {
            let mut v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
            });
            if let Some(m) = extra { v["amara"] = m.clone(); }
            format!("data: {v}\n\n")
        };
        yield Ok::<_, std::convert::Infallible>(chunk(json!({"role": "assistant", "content": ""}), Value::Null, Some(&meta)));
        let mut tool_index = 0usize;
        let mut finish = Some(finish);
        while let Some(ev) = events.next().await {
            match ev {
                Ok(Ev::Delta { content, tool_calls }) => {
                    if !content.is_empty() {
                        yield Ok(chunk(json!({"content": content}), Value::Null, None));
                    }
                    if !tool_calls.is_empty() {
                        let mut calls = translate::tool_calls_to_openai(&tool_calls, &id, true);
                        for c in calls.as_array_mut().into_iter().flatten() {
                            c["index"] = json!(tool_index);
                            c["id"] = json!(format!("call_{id}_{tool_index}"));
                            tool_index += 1;
                        }
                        yield Ok(chunk(json!({"tool_calls": calls}), Value::Null, None));
                    }
                }
                Ok(Ev::Done { done_reason, prompt_tokens, completion_tokens }) => {
                    if let Some(f) = finish.take() { f(prompt_tokens, completion_tokens, None); }
                    let fr = translate::finish_reason(&done_reason, tool_index > 0);
                    yield Ok(chunk(json!({}), json!(fr), Some(&meta)));
                    if include_usage {
                        let u = json!({
                            "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                            "choices": [],
                            "usage": {"prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens},
                        });
                        yield Ok(format!("data: {u}\n\n"));
                    }
                }
                Err(e) => {
                    if let Some(f) = finish.take() { f(0, 0, Some(&e)); }
                    let err = json!({"error": {"message": e, "type": "amara_router_error"}});
                    yield Ok(format!("data: {err}\n\n"));
                    break;
                }
            }
        }
        yield Ok("data: [DONE]\n\n".to_string());
    };
    Body::from_stream(s)
}

fn ollama_ndjson<F>(
    mut events: crate::engine::EvStream,
    model: String,
    meta: Value,
    start: Instant,
    finish: F,
) -> Body
where
    F: FnOnce(u64, u64, Option<&str>) + Send + 'static,
{
    let s = async_stream::stream! {
        let mut finish = Some(finish);
        while let Some(ev) = events.next().await {
            match ev {
                Ok(Ev::Delta { content, tool_calls }) => {
                    let mut message = json!({"role": "assistant", "content": content});
                    if !tool_calls.is_empty() { message["tool_calls"] = Value::Array(tool_calls); }
                    let v = json!({"model": model, "created_at": now_rfc3339(), "message": message, "done": false});
                    yield Ok::<_, std::convert::Infallible>(format!("{v}\n"));
                }
                Ok(Ev::Done { done_reason, prompt_tokens, completion_tokens }) => {
                    if let Some(f) = finish.take() { f(prompt_tokens, completion_tokens, None); }
                    let v = json!({
                        "model": model, "created_at": now_rfc3339(),
                        "message": {"role": "assistant", "content": ""},
                        "done": true, "done_reason": done_reason,
                        "total_duration": start.elapsed().as_nanos() as u64,
                        "prompt_eval_count": prompt_tokens, "eval_count": completion_tokens,
                        "amara": meta,
                    });
                    yield Ok(format!("{v}\n"));
                }
                Err(e) => {
                    if let Some(f) = finish.take() { f(0, 0, Some(&e)); }
                    yield Ok(format!("{}\n", json!({"error": e})));
                    break;
                }
            }
        }
    };
    Body::from_stream(s)
}

// ── side endpoints ───────────────────────────────────────────────────────────

async fn health(AxState(st): AxState<St>) -> Response {
    let loaded = st.loaded_models().await;
    let is_loaded = |m: &str| {
        loaded
            .as_ref()
            .ok()
            .map(|l| l.iter().any(|x| crate::engine::same_model(x, m)))
    };
    let up = loaded.is_ok();
    axum::Json(json!({
        "status": if up { "ok" } else { "degraded" },
        "ollama": if up { "up" } else { "down" },
        "ollama_url": st.cfg.ollama_url,
        "router_version": env!("CARGO_PKG_VERSION"),
        "models": {"fast": st.cfg.models.fast, "deep": st.cfg.models.deep},
        "loaded": {"fast": is_loaded(&st.cfg.models.fast), "deep": is_loaded(&st.cfg.models.deep)},
        "cloud": {"enabled": st.cfg.cloud.anthropic_enabled, "outbound_disabled": st.outbound_disabled},
        "error": loaded.err().map(|_| format!("Ollama is not reachable at {}", st.cfg.ollama_url)),
    }))
    .into_response()
}

fn model_entry(name: &str) -> Value {
    json!({
        "name": name,
        "model": name,
        "modified_at": "2026-10-31T00:00:00Z",
        "size": 0,
        "digest": format!("amara-router-{}", name.replace(':', "-")),
        "details": {"format": "router", "family": "amara", "families": ["amara"], "parameter_size": "", "quantization_level": ""},
    })
}

async fn ollama_tags() -> Response {
    axum::Json(json!({"models": VIRTUAL_MODELS.iter().map(|m| model_entry(m)).collect::<Vec<_>>()}))
        .into_response()
}

async fn openai_models() -> Response {
    let data: Vec<Value> = VIRTUAL_MODELS
        .iter()
        .map(|m| json!({"id": m.trim_end_matches(":latest"), "object": "model", "created": 0, "owned_by": "amara-router"}))
        .collect();
    axum::Json(json!({"object": "list", "data": data})).into_response()
}

async fn ollama_show(body: Bytes) -> Response {
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let name = v["model"]
        .as_str()
        .or_else(|| v["name"].as_str())
        .unwrap_or("amara:latest");
    let mut entry = model_entry(name);
    let details = entry["details"].take();
    axum::Json(json!({
        "modelfile": "", "parameters": "", "template": "",
        "details": details,
        "model_info": {},
        "capabilities": ["completion", "tools"],
        "modified_at": entry["modified_at"],
    }))
    .into_response()
}

async fn ollama_version(AxState(st): AxState<St>) -> Response {
    let upstream = async {
        st.http
            .get(st.cfg.ollama("/api/version"))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()
    }
    .await;
    let version = upstream
        .as_ref()
        .and_then(|v| v["version"].as_str())
        .unwrap_or("0.0.0")
        .to_string();
    axum::Json(json!({"version": version, "amara_router": env!("CARGO_PKG_VERSION")}))
        .into_response()
}

/// HA pulls a model that is missing from `/api/tags`. The router's models are virtual, so
/// "pulling" one succeeds at once; anything else is refused.
async fn ollama_pull(body: Bytes) -> Response {
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let name = v["model"]
        .as_str()
        .or_else(|| v["name"].as_str())
        .unwrap_or("");
    let full = if name.contains(':') {
        name.to_string()
    } else {
        format!("{name}:latest")
    };
    if !VIRTUAL_MODELS.contains(&full.as_str()) {
        return error_response(
            Api::Ollama,
            404,
            &format!(
                "the Amara router serves only {}; pull other models in Ollama directly",
                VIRTUAL_MODELS.join(", ")
            ),
        );
    }
    if v["stream"].as_bool().unwrap_or(true) {
        let mut r = Response::new(Body::from("{\"status\":\"success\"}\n"));
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
        );
        r
    } else {
        axum::Json(json!({"status": "success"})).into_response()
    }
}

/// Dry run: the routing decision for a request, without generating. May run the classifier.
/// Accepts an OpenAI or Ollama chat body, or `{"text": "..."}`. Returns no message content.
async fn dry_route(AxState(st): AxState<St>, headers: HeaderMap, body: Bytes) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(Api::OpenAi, 400, &format!("invalid JSON: {e}")),
    };
    let model = v["model"].as_str().unwrap_or("amara");
    let hints = hints_from(&headers, model);
    let mut text = match v["text"].as_str() {
        Some(t) => t.to_string(),
        None => last_user(&v).map(|u| u.1).unwrap_or_default(),
    };
    let before = text.clone();
    let (d, routing_ms) = st.decide(&mut text, &hints).await;
    let meta = RouteMeta {
        tier: d.tier,
        reason: d.reason,
        routing_ms,
    };
    let r = axum::Json(json!({
        "tier": meta.tier,
        "reason": meta.reason,
        "routing_ms": routing_ms,
        "model": st.model_for(meta.tier),
        "stripped": before != text,
    }))
    .into_response();
    with_meta(r, &meta, st.model_for(meta.tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn user_text_replacement() {
        let mut b = json!({"messages": [{"role": "user", "content": "/deep x"}, {"role": "assistant", "content": "a"}, {"role": "user", "content": [{"type": "text", "text": "/deep y"}, {"type": "text", "text": "z"}]}]});
        let (i, t) = last_user(&b).unwrap();
        assert_eq!((i, t.as_str()), (2, "/deep y\nz"));
        set_user_text(&mut b, i, "y\nz");
        assert_eq!(
            b["messages"][2]["content"],
            json!([{"type": "text", "text": "y\nz"}])
        );
    }
}
