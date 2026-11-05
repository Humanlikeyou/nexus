//! End-to-end tests against a mocked Ollama (and a mocked Anthropic). No real models run.

use amara_router::config::{Config, Secrets};
use amara_router::engine::State;
use amara_router::server;
use axum::body::Bytes;
use axum::extract::State as AxState;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post as http_post};
use axum::Router;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FAST: &str = "fast-model:latest";
const DEEP: &str = "deep-model:latest";

#[derive(Default)]
struct Mock {
    /// Every /api/chat body (classifier and generation).
    chats: Vec<Value>,
    generates: Vec<Value>,
    anthropic_calls: u32,
    classifier_reply: String,
    classifier_delay_ms: u64,
    classifier_fails: bool,
    /// Content pieces streamed for a generation.
    pieces: Vec<String>,
    tool_call: bool,
    fail_models: Vec<String>,
    ps_models: Vec<String>,
    /// Delay before a generation (non-classifier /api/chat) answers: a cold model load.
    gen_delay_ms: u64,
    /// Delay between the byte chunks of a generation: a slow answer.
    chunk_delay_ms: u64,
    /// Delay before /api/ps answers.
    ps_delay_ms: u64,
    /// size_vram (bytes) reported per model by /api/ps (missing = 0).
    ps_vram: std::collections::HashMap<String, u64>,
    /// /api/ps answers 500.
    ps_fails: bool,
    /// Models told to unload (`keep_alive: 0`); they also leave `ps_models`.
    unloads: Vec<String>,
    /// How long a warm-up takes. Over 0, a finished warm-up joins `ps_models`.
    warm_delay_ms: u64,
    /// Warm-ups that ran to the end, so were not cancelled.
    warms_done: Vec<String>,
    /// Warm-ups of these models answer 500.
    warm_fails: Vec<String>,
}

type M = Arc<Mutex<Mock>>;

fn is_classifier(b: &Value) -> bool {
    b["options"]["num_predict"] == 3 && b["stream"] == false
}

async fn mock_chat(AxState(m): AxState<M>, body: Bytes) -> Response {
    let b: Value = serde_json::from_slice(&body).unwrap();
    let (classifier, delay, reply, fails, pieces, tool, fail_models, gen_delay, chunk_delay) = {
        let mut g = m.lock().unwrap();
        g.chats.push(b.clone());
        (
            is_classifier(&b),
            g.classifier_delay_ms,
            g.classifier_reply.clone(),
            g.classifier_fails,
            g.pieces.clone(),
            g.tool_call,
            g.fail_models.clone(),
            g.gen_delay_ms,
            g.chunk_delay_ms,
        )
    };
    if !classifier && gen_delay > 0 {
        tokio::time::sleep(Duration::from_millis(gen_delay)).await;
    }
    if classifier {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if fails {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": "boom"})),
            )
                .into_response();
        }
        return axum::Json(
            json!({"message": {"role": "assistant", "content": reply}, "done": true}),
        )
        .into_response();
    }
    let model = b["model"].as_str().unwrap_or("").to_string();
    if fail_models.contains(&model) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error": "model failed to load"})),
        )
            .into_response();
    }
    let mut out = String::new();
    for p in pieces {
        out.push_str(
            &json!({"model": model, "message": {"role": "assistant", "content": p}, "done": false})
                .to_string(),
        );
        out.push('\n');
    }
    if tool {
        out.push_str(&json!({"model": model, "message": {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "light_on", "arguments": {"room": "office"}}}]}, "done": false}).to_string());
        out.push('\n');
    }
    out.push_str(&json!({"model": model, "message": {"role": "assistant", "content": ""}, "done": true, "done_reason": "stop", "prompt_eval_count": 11, "eval_count": 7}).to_string());
    out.push('\n');
    // Split into small byte chunks so tags are cut mid-way.
    let chunks: Vec<Result<Bytes, std::io::Error>> = out
        .as_bytes()
        .chunks(9)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    let paced = futures::stream::iter(chunks).then(move |c| async move {
        tokio::time::sleep(Duration::from_millis(chunk_delay)).await;
        c
    });
    Response::new(axum::body::Body::from_stream(paced))
}

async fn start_mock(m: M) -> String {
    let app = Router::new()
        .route("/api/chat", http_post(mock_chat))
        .route("/api/generate", http_post(|AxState(m): AxState<M>, body: Bytes| async move {
            let b: Value = serde_json::from_slice(&body).unwrap();
            let model = b["model"].as_str().unwrap_or("").to_string();
            if b["keep_alive"] == 0 {
                let mut g = m.lock().unwrap();
                g.ps_models.retain(|n| *n != model);
                g.unloads.push(model);
                return axum::Json(json!({"done": true})).into_response();
            }
            let (delay, fails) = {
                let mut g = m.lock().unwrap();
                g.generates.push(b);
                (g.warm_delay_ms, g.warm_fails.contains(&model))
            };
            tokio::time::sleep(Duration::from_millis(delay)).await;
            if fails {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no").into_response();
            }
            let mut g = m.lock().unwrap();
            g.warms_done.push(model.clone());
            if delay > 0 {
                g.ps_models.push(model);
            }
            axum::Json(json!({"done": true})).into_response()
        }))
        .route("/api/ps", get(|AxState(m): AxState<M>| async move {
            let delay = m.lock().unwrap().ps_delay_ms;
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let g = m.lock().unwrap();
            if g.ps_fails {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "ps broke").into_response();
            }
            let models: Vec<Value> = g.ps_models.iter().map(|n| json!({"name": n, "model": n, "size_vram": g.ps_vram.get(n).copied().unwrap_or(0)})).collect();
            axum::Json(json!({"models": models})).into_response()
        }))
        .route("/api/version", get(|| async { axum::Json(json!({"version": "9.9.9"})) }))
        .route("/v1/messages", http_post(|AxState(m): AxState<M>| async move {
            m.lock().unwrap().anthropic_calls += 1;
            axum::Json(json!({"content": [{"type": "text", "text": "from claude"}], "stop_reason": "end_turn", "usage": {"input_tokens": 3, "output_tokens": 2}}))
        }))
        .with_state(m);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

struct Rig {
    url: String,
    mock: M,
    log: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

fn base_config(ollama: &str, log: &std::path::Path, extra: &str) -> Config {
    let text = format!(
        r#"
ollama_url = "{ollama}"
bind = "127.0.0.1:8787"
{extra_top}
[models]
fast = "{FAST}"
deep = "{DEEP}"
fast_num_ctx = 4096
deep_num_ctx = 2048
[routing]
classifier_timeout_ms = 300
[loading]
warm_fast_on_start = false
rewarm_fast_after_deep = {rewarm}
poll_secs = 1
[filler]
enabled = {filler}
[cloud]
anthropic_enabled = {cloud}
anthropic_url = "{ollama}/v1/messages"
[log]
path = "{log}"
"#,
        extra_top = "",
        rewarm = extra.contains("rewarm"),
        filler = extra.contains("filler"),
        cloud = extra.contains("cloud"),
        log = log.display(),
    );
    Config::parse(&text).unwrap()
}

async fn rig_with(
    mock: Mock,
    extra: &str,
    ollama_override: Option<String>,
    outbound_disabled: bool,
) -> Rig {
    rig_cfg(mock, extra, ollama_override, outbound_disabled, |_| {}).await
}

async fn rig_cfg(
    mock: Mock,
    extra: &str,
    ollama_override: Option<String>,
    outbound_disabled: bool,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let mock = Arc::new(Mutex::new(mock));
    let mock_url = start_mock(Arc::clone(&mock)).await;
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("routes.jsonl");
    let mut cfg = base_config(&ollama_override.unwrap_or(mock_url.clone()), &log, extra);
    cfg.cloud.anthropic_url = format!("{mock_url}/v1/messages");
    tweak(&mut cfg);
    let secrets = Secrets {
        api_key: None,
        anthropic_key: extra.contains("cloud").then(|| "test-key".to_string()),
    };
    let state = State::new(cfg, secrets, outbound_disabled);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let app = server::local_router(state);
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    Rig {
        url,
        mock,
        log,
        _dir: dir,
    }
}

fn mock(reply: &str, pieces: &[&str]) -> Mock {
    Mock {
        classifier_reply: reply.into(),
        pieces: pieces.iter().map(|s| s.to_string()).collect(),
        // Both resident unless a test says otherwise (the classifier runs only with fast, and
        // deep is served only when resident).
        ps_models: vec![FAST.to_string(), DEEP.to_string()],
        ..Default::default()
    }
}

async fn rig(m: Mock) -> Rig {
    rig_with(m, "", None, false).await
}

async fn post(url: &str, body: Value, headers: &[(&str, &str)]) -> reqwest::Response {
    let mut r = reqwest::Client::new().post(url).json(&body);
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send().await.unwrap()
}

fn oa(text: &str, stream: bool) -> Value {
    json!({"model": "amara", "stream": stream, "messages": [{"role": "system", "content": "You are Amara."}, {"role": "user", "content": text}]})
}

fn generations(r: &Rig) -> Vec<Value> {
    r.mock
        .lock()
        .unwrap()
        .chats
        .iter()
        .filter(|b| !is_classifier(b))
        .cloned()
        .collect()
}
fn classifier_calls(r: &Rig) -> Vec<Value> {
    r.mock
        .lock()
        .unwrap()
        .chats
        .iter()
        .filter(|b| is_classifier(b))
        .cloned()
        .collect()
}

// ── both API formats ─────────────────────────────────────────────────────────

#[tokio::test]
async fn openai_non_streamed_simple_question_goes_fast_via_classifier() {
    let r = rig(mock("EASY", &["<think>hmm</think>", "Paris."])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("Capital of France?", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_easy");
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["model"], FAST);
    assert_eq!(v["choices"][0]["message"]["content"], "Paris.");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(
        v["usage"],
        json!({"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18})
    );
    assert_eq!(v["amara"]["tier"], "fast");
    // Classifier: fast model, temperature 0, num_predict 3, thinking off.
    let c = classifier_calls(&r);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["model"], FAST);
    assert_eq!(c[0]["options"]["temperature"], 0);
    assert_eq!(c[0]["think"], false);
    // Generation: full history and system prompt passed through, routing knobs applied.
    let g = generations(&r);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0]["model"], FAST);
    assert_eq!(g[0]["keep_alive"], -1);
    assert_eq!(g[0]["options"]["num_ctx"], 4096);
    assert_eq!(
        g[0]["messages"][0],
        json!({"role": "system", "content": "You are Amara."})
    );
    assert_eq!(g[0]["messages"][1]["content"], "Capital of France?");
}

#[tokio::test]
async fn openai_streamed_strips_think_across_chunks_and_tags_first_and_last_chunk() {
    let r = rig(mock(
        "EASY",
        &["<thi", "nk>private", " reasoning</th", "ink>\n\nHel", "lo!"],
    ))
    .await;
    let mut body = oa("hi", true);
    body["stream_options"] = json!({"include_usage": true});
    let resp = post(&format!("{}/v1/chat/completions", r.url), body, &[]).await;
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    let text = resp.text().await.unwrap();
    assert!(
        !text.contains("private") && !text.contains("think"),
        "{text}"
    );
    let chunks: Vec<Value> = text
        .split("\n\n")
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|l| *l != "[DONE]")
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(text.trim_end().ends_with("data: [DONE]"));
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(chunks[0]["amara"]["tier"], "fast");
    let content: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(content, "Hello!");
    let fin = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"] == "stop")
        .unwrap();
    assert_eq!(fin["amara"]["tier"], "fast");
    assert_eq!(fin["amara"]["reason"], "classifier_easy");
    let usage = chunks.last().unwrap();
    assert_eq!(usage["usage"]["total_tokens"], 18);
}

#[tokio::test]
async fn ollama_api_streamed_and_not() {
    let r = rig(mock("EASY", &["<think>x</think>", "Hi ", "there"])).await;
    let body = json!({"model": "amara:latest", "messages": [{"role": "user", "content": "hello"}], "keep_alive": "1h", "options": {"num_ctx": 8192}});
    // Ollama streams by default.
    let resp = post(&format!("{}/api/chat", r.url), body.clone(), &[]).await;
    assert_eq!(resp.headers()["content-type"], "application/x-ndjson");
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    let lines: Vec<Value> = resp
        .text()
        .await
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let content: String = lines
        .iter()
        .filter_map(|l| l["message"]["content"].as_str())
        .collect();
    assert_eq!(content, "Hi there");
    let last = lines.last().unwrap();
    assert_eq!(last["done"], true);
    assert_eq!(last["model"], FAST);
    assert_eq!(last["amara"]["tier"], "fast");
    assert_eq!(last["eval_count"], 7);
    // Router policy overrides the caller's keep_alive/num_ctx so the resident model never reloads.
    let g = generations(&r);
    assert_eq!(g[0]["keep_alive"], -1);
    assert_eq!(g[0]["options"]["num_ctx"], 4096);

    let mut b = body;
    b["stream"] = json!(false);
    let v: Value = post(&format!("{}/api/chat", r.url), b, &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(v["message"]["content"], "Hi there");
    assert_eq!(v["done"], true);
    assert_eq!(v["amara"]["tier"], "fast");
}

#[tokio::test]
async fn tool_calls_round_trip_in_both_formats() {
    let mut m = mock("EASY", &[]);
    m.tool_call = true;
    let r = rig(m).await;
    let mut body = oa("turn on the office light", false);
    body["tools"] = json!([{"type": "function", "function": {"name": "light_on", "parameters": {"type": "object"}}}]);
    let v: Value = post(&format!("{}/v1/chat/completions", r.url), body, &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
    let call = &v["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "light_on");
    assert_eq!(call["function"]["arguments"], "{\"room\":\"office\"}");
    assert_eq!(
        generations(&r)[0]["tools"][0]["function"]["name"],
        "light_on"
    );

    let ol = json!({"model": "amara", "stream": false, "messages": [{"role": "user", "content": "light"}]});
    let v: Value = post(&format!("{}/api/chat", r.url), ol, &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        v["message"]["tool_calls"][0]["function"]["arguments"],
        json!({"room": "office"})
    );
}

#[tokio::test]
async fn gemma_call_text_becomes_tool_calls_in_every_format() {
    let leaked = [
        "mcp__followups__item_done{item:<|\"|>x, y<|\"|>,no",
        "te:<|\"|>z<|\"|>} mcp__followups__mute_topic{topic:<|\"|>x<|\"|>}<tool_call|>",
    ];
    let r = rig(mock("EASY", &leaked)).await;
    let tools = json!([
        {"type": "function", "function": {"name": "mcp__followups__item_done"}},
        {"type": "function", "function": {"name": "mcp__followups__mute_topic"}},
    ]);
    let done = "{\"item\":\"x, y\",\"note\":\"z\"}";

    let mut body = oa("done", false);
    body["tools"] = tools.clone();
    let url = format!("{}/v1/chat/completions", r.url);
    let v: Value = post(&url, body, &[]).await.json().await.unwrap();
    let choice = &v["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["content"], "");
    let calls = choice["message"]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["function"]["arguments"], done);
    assert_eq!(calls[1]["function"]["name"], "mcp__followups__mute_topic");

    let mut body = oa("done", true);
    body["tools"] = tools.clone();
    let sse = post(&url, body, &[]).await.text().await.unwrap();
    assert!(!sse.contains("<|"), "{sse}");
    assert!(sse.contains("\"finish_reason\":\"tool_calls\""));
    assert_eq!(sse.matches("\"type\":\"function\"").count(), 2);

    for stream in [false, true] {
        let ol = json!({"model": "amara", "stream": stream, "tools": tools,
                        "messages": [{"role": "user", "content": "done"}]});
        let text = post(&format!("{}/api/chat", r.url), ol, &[])
            .await
            .text()
            .await
            .unwrap();
        assert!(!text.contains("<|"), "{text}");
        assert!(text.contains("mcp__followups__mute_topic"));
    }

    // The same text with those tools not offered is left alone.
    let v: Value = post(&url, oa("done", false), &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], leaked.concat());
    assert!(v["choices"][0]["message"]["tool_calls"].is_null());
}

#[tokio::test]
async fn gemma_call_with_trailing_prose_keeps_the_prose_as_content() {
    let leaked = "mcp__followups__item_done{item:<|\"|>bike repair receipt<|\"|>,note:<|\"|>sorted<|\"|>} All good on that front.";
    let r = rig(mock("EASY", &[leaked])).await;
    let tools = json!([{"type": "function", "function": {"name": "mcp__followups__item_done"}}]);
    let url = format!("{}/v1/chat/completions", r.url);

    let mut body = oa("done", false);
    body["tools"] = tools.clone();
    let v: Value = post(&url, body, &[]).await.json().await.unwrap();
    let choice = &v["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["content"], "All good on that front.");
    assert_eq!(
        choice["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"item\":\"bike repair receipt\",\"note\":\"sorted\"}"
    );

    let mut body = oa("done", true);
    body["tools"] = tools;
    let sse = post(&url, body, &[]).await.text().await.unwrap();
    assert!(!sse.contains("<|"), "{sse}");
    assert!(sse.contains("All good on that front."));
    assert!(sse.contains("\"finish_reason\":\"tool_calls\""));
}

#[tokio::test]
async fn a_terminal_echo_stays_text_in_every_format() {
    let echo = "terminal{command:<|\"|>rm -rf x<|\"|>}<tool_call|>";
    let r = rig(mock("EASY", &[echo])).await;
    let tools = json!([{"type": "function", "function": {"name": "terminal"}}]);
    let url = format!("{}/v1/chat/completions", r.url);

    let mut body = oa("done", false);
    body["tools"] = tools.clone();
    let v: Value = post(&url, body, &[]).await.json().await.unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], echo);
    assert!(v["choices"][0]["message"]["tool_calls"].is_null());
    assert_eq!(v["choices"][0]["finish_reason"], "stop");

    let mut body = oa("done", true);
    body["tools"] = tools.clone();
    let sse = post(&url, body, &[]).await.text().await.unwrap();
    assert!(!sse.contains("tool_calls"), "{sse}");
    let mut text = String::new();
    for line in sse.lines().filter_map(|l| l.strip_prefix("data: ")) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            text.push_str(v["choices"][0]["delta"]["content"].as_str().unwrap_or(""));
        }
    }
    assert_eq!(text, echo);

    let ol = |stream: bool| {
        json!({"model": "amara", "stream": stream, "tools": tools,
               "messages": [{"role": "user", "content": "done"}]})
    };
    let url = format!("{}/api/chat", r.url);
    let v: Value = post(&url, ol(false), &[]).await.json().await.unwrap();
    assert_eq!(v["message"]["content"], echo);
    assert!(v["message"]["tool_calls"].is_null());

    let lines = post(&url, ol(true), &[]).await.text().await.unwrap();
    assert!(!lines.contains("tool_calls"), "{lines}");
    let text: String = lines
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .map(|v| v["message"]["content"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(text, echo);
}

// ── each routing rule ────────────────────────────────────────────────────────

#[tokio::test]
async fn slash_deep_goes_deep_stripped_without_classifier() {
    let r = rig(mock("EASY", &["Plan."])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep plan my week", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "deep");
    assert_eq!(resp.headers()["x-amara-route-reason"], "slash_deep");
    let g = generations(&r);
    assert_eq!(g[0]["model"], DEEP);
    assert_eq!(g[0]["keep_alive"], "5m");
    assert_eq!(g[0]["options"]["num_ctx"], 2048);
    assert_eq!(g[0]["think"], false, "deep thinking is off by default");
    assert_eq!(g[0]["messages"][1]["content"], "plan my week");
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn trigger_phrase_goes_deep_and_is_stripped() {
    let r = rig(mock("EASY", &["ok"])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("Think harder: should I buy the akiya?", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "deep");
    assert_eq!(
        generations(&r)[0]["messages"][1]["content"],
        "should I buy the akiya?"
    );
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn slash_fast_goes_fast_without_classifier() {
    let r = rig(mock("HARD", &["4"])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast prove the Riemann hypothesis", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(
        generations(&r)[0]["messages"][1]["content"],
        "prove the Riemann hypothesis"
    );
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn long_message_and_code_block_go_deep_without_classifier() {
    let r = rig(mock("EASY", &["ok"])).await;
    let long = "word ".repeat(1700); // ~8,500 chars > 2,000 tokens
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa(&long, false),
        &[],
    )
    .await;
    assert_eq!(
        resp.headers()["x-amara-route-reason"],
        "over_token_threshold"
    );
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("fix:\n```py\nprint(1\n```", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "code_block");
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn hard_question_reaches_deep_through_the_classifier() {
    let r = rig(mock("HARD", &["Deep answer"])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa(
            "Compare three mortgage strategies for an akiya purchase",
            false,
        ),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "deep");
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_hard");
    assert_eq!(generations(&r)[0]["model"], DEEP);
}

#[tokio::test]
async fn header_and_model_alias_hints() {
    let r = rig(mock("EASY", &["ok"])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hi", false),
        &[("X-Amara-Route", "deep")],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "header_deep");
    let mut b = oa("hi", false);
    b["model"] = json!("amara-fast");
    let resp = post(&format!("{}/v1/chat/completions", r.url), b, &[]).await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "model_fast");
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hi", false),
        &[("X-Amara-Tags", "x, research")],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "tag=research");
}

// ── classifier timeout and fallback ──────────────────────────────────────────

#[tokio::test]
async fn classifier_timeout_falls_back_to_fast_quickly() {
    let mut m = mock("HARD", &["quick"]);
    m.classifier_delay_ms = 3000;
    let r = rig(m).await;
    let t = Instant::now();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hello", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_timeout");
    let routing_ms: u64 = resp.headers()["x-amara-routing-ms"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(routing_ms < 1000, "routing took {routing_ms}ms");
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn classifier_error_falls_back_to_fast() {
    let mut m = mock("HARD", &["ok"]);
    m.classifier_fails = true;
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hello", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_error");
}

#[tokio::test]
async fn deep_failure_falls_back_to_fast() {
    let mut m = mock("EASY", &["from fast"]);
    m.fail_models = vec![DEEP.into()];
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep x", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(
        resp.headers()["x-amara-route-reason"],
        "slash_deep; deep_failed"
    );
    let models: Vec<Value> = generations(&r).iter().map(|g| g["model"].clone()).collect();
    assert_eq!(models, vec![json!(DEEP), json!(FAST)]);
}

#[tokio::test]
async fn backend_error_on_every_tier_is_a_clean_502() {
    let mut m = mock("EASY", &[]);
    m.fail_models = vec![DEEP.into(), FAST.into()];
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep x", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 502);
    let v: Value = resp.json().await.unwrap();
    let msg = v["error"]["message"].as_str().unwrap();
    // Status plus a fixed reason only: the upstream body is never echoed.
    assert!(
        msg.contains("500") && msg.contains("request rejected"),
        "{msg}"
    );
    assert!(!msg.contains("model failed to load"), "{msg}");
}

// ── Ollama down ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn ollama_down_gives_clear_error_and_no_crash() {
    // A port with nothing listening.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let r = rig_with(mock("EASY", &[]), "", Some(dead), false).await;
    for stream in [false, true] {
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("hello", stream),
            &[],
        )
        .await;
        assert_eq!(resp.status(), 503);
        let v: Value = resp.json().await.unwrap();
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains("Ollama is not reachable"), "{msg}");
        let resp = post(&format!("{}/api/chat", r.url), json!({"model": "amara", "stream": stream, "messages": [{"role": "user", "content": "/deep hi"}]}), &[]).await;
        assert_eq!(resp.status(), 503);
        let v: Value = resp.json().await.unwrap();
        assert!(v["error"].as_str().unwrap().contains("ollama serve"));
    }
    let h: Value = reqwest::get(format!("{}/health", r.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["status"], "degraded");
    assert_eq!(h["ollama"], "down");
    // The log records the failure without content.
    let log = std::fs::read_to_string(&r.log).unwrap();
    assert!(log.contains("\"status\":\"error\"") && !log.contains("hello"));
}

// ── side endpoints, log, loading, filler, auth, cloud ────────────────────────

#[tokio::test]
async fn ha_endpoints_tags_show_pull_version_health() {
    let r = rig(mock("EASY", &[])).await;
    let tags: Value = reqwest::get(format!("{}/api/tags", r.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = tags["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["model"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["amara:latest", "amara-fast:latest", "amara-deep:latest"]
    );
    assert_eq!(tags["models"][0]["name"], "amara:latest");
    let show: Value = post(
        &format!("{}/api/show", r.url),
        json!({"model": "amara:latest"}),
        &[],
    )
    .await
    .json()
    .await
    .unwrap();
    assert!(show["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("tools")));
    let pull = post(
        &format!("{}/api/pull", r.url),
        json!({"model": "amara:latest", "stream": false}),
        &[],
    )
    .await;
    assert_eq!(pull.json::<Value>().await.unwrap()["status"], "success");
    let pull = post(
        &format!("{}/api/pull", r.url),
        json!({"model": "llama3:latest"}),
        &[],
    )
    .await;
    assert_eq!(pull.status(), 404);
    let ver: Value = reqwest::get(format!("{}/api/version", r.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ver["version"], "9.9.9");
    let models: Value = reqwest::get(format!("{}/v1/models", r.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], "amara");
    let h: Value = reqwest::get(format!("{}/health", r.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["status"], "ok");
}

#[tokio::test]
async fn dry_run_route_endpoint() {
    let r = rig(mock("HARD", &[])).await;
    let v: Value = post(
        &format!("{}/amara/route", r.url),
        json!({"text": "/deep plan my week"}),
        &[],
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(v["tier"], "deep");
    assert_eq!(v["model"], DEEP);
    assert_eq!(v["stripped"], true);
    assert!(generations(&r).is_empty(), "dry run must not generate");
}

#[tokio::test]
async fn jsonl_log_has_fields_and_no_content() {
    let r = rig(mock("EASY", &["SECRET-ANSWER"])).await;
    post(
        &format!("{}/v1/chat/completions", r.url),
        oa("SECRET-QUESTION", false),
        &[],
    )
    .await
    .text()
    .await
    .unwrap();
    let log = std::fs::read_to_string(&r.log).unwrap();
    let line: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    for k in [
        "ts",
        "route",
        "reason",
        "model",
        "latency_ms",
        "routing_ms",
        "prompt_tokens",
        "completion_tokens",
        "api",
        "stream",
        "status",
    ] {
        assert!(line.get(k).is_some(), "missing {k}: {line}");
    }
    assert_eq!(line["route"], "fast");
    assert_eq!(line["model"], FAST);
    assert!(!log.contains("SECRET"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&r.log).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test]
async fn filler_line_streams_before_a_deep_answer_only() {
    let r = rig_with(mock("EASY", &["Answer"]), "filler", None, false).await;
    let text = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep x", true),
        &[],
    )
    .await
    .text()
    .await
    .unwrap();
    let first_content = text.find("Give me a moment").unwrap();
    assert!(first_content < text.find("Answer").unwrap());
    let text = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast x", true),
        &[],
    )
    .await
    .text()
    .await
    .unwrap();
    assert!(!text.contains("Give me a moment"));
}

#[tokio::test]
async fn lan_listener_requires_bearer() {
    let m = Arc::new(Mutex::new(mock("EASY", &[])));
    let mock_url = start_mock(m).await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = base_config(&mock_url, &dir.path().join("l.jsonl"), "");
    let state = State::new(cfg, Secrets::default(), false);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let app = server::lan_router(state, "k".repeat(32));
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    assert_eq!(
        reqwest::get(format!("{url}/health"))
            .await
            .unwrap()
            .status(),
        401
    );
    let ok = reqwest::Client::new()
        .get(format!("{url}/health"))
        .bearer_auth("k".repeat(32))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
}

#[tokio::test]
async fn cloud_only_when_enabled_asked_and_allowed() {
    let r = rig_with(mock("EASY", &["local"]), "cloud", None, false).await;
    let v: Value = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/claude hi", false),
        &[],
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(v["amara"]["tier"], "claude");
    assert_eq!(v["choices"][0]["message"]["content"], "from claude");
    assert_eq!(r.mock.lock().unwrap().anthropic_calls, 1);
    // The caller (Nexus "Allow Claude" off) forbids it: deep instead, no network call.
    let v: Value = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/claude hi", false),
        &[("X-Amara-Allow-Claude", "0")],
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(v["amara"]["tier"], "deep");
    assert_eq!(
        v["amara"]["reason"],
        "slash_claude; claude_blocked=claude_not_allowed"
    );
    // Not asked: never Claude.
    let v: Value = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hi", false),
        &[],
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(v["amara"]["tier"], "fast");
    assert_eq!(r.mock.lock().unwrap().anthropic_calls, 1);
}

#[tokio::test]
async fn no_cloud_call_under_outbound_disabled_or_when_off() {
    for (extra, outbound) in [("cloud", true), ("", false)] {
        let r = rig_with(mock("EASY", &["local"]), extra, None, outbound).await;
        let v: Value = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("hi", false),
            &[("X-Amara-Mode", "api"), ("X-Amara-Allow-Claude", "1")],
        )
        .await
        .json()
        .await
        .unwrap();
        assert_eq!(v["amara"]["tier"], "deep");
        let want = if outbound {
            "outbound_disabled"
        } else {
            "cloud_disabled"
        };
        assert_eq!(
            v["amara"]["reason"],
            format!("mode=api; claude_blocked={want}")
        );
        assert_eq!(r.mock.lock().unwrap().anthropic_calls, 0);
    }
}

#[tokio::test]
async fn bad_requests_are_400() {
    let r = rig(mock("EASY", &[])).await;
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", r.url))
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp = post(
        &format!("{}/api/chat", r.url),
        json!({"model": "amara"}),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn loopback_port_refuses_browser_and_rebinding_requests() {
    let rig = rig(Mock::default()).await;
    let c = reqwest::Client::new();
    let body = json!({"model": "amara", "messages": [{"role": "user", "content": "hi"}]});
    let from_page = c
        .post(format!("{}/v1/chat/completions", rig.url))
        .header("Origin", "https://evil.example")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(from_page.status(), 403);
    let rebound = c
        .get(format!("{}/health", rig.url))
        .header("Host", "evil.example:8787")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound.status(), 403);
    let ok = c.get(format!("{}/health", rig.url)).send().await.unwrap();
    assert_eq!(ok.status(), 200);
}

// ── residency, warm-ups, num_ctx, first-token timeout ───────────────────────

async fn wait_for_generates(r: &Rig, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        if r.mock.lock().unwrap().generates.len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    r.mock.lock().unwrap().generates.clone()
}

#[tokio::test]
async fn fast_not_resident_skips_classifier_uses_heuristic_and_warms_fast() {
    let mut m = mock("HARD", &["ok"]);
    m.ps_models = vec![DEEP.to_string()]; // just after a deep reply: fast was evicted
    m.warm_delay_ms = 10;
    m.classifier_delay_ms = 3000;
    let r = rig(m).await;
    let t = Instant::now();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("why is the sky blue?", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "fast");
    assert_eq!(
        resp.headers()["x-amara-route-reason"],
        "classifier_skipped_fast_not_resident"
    );
    assert!(t.elapsed() < Duration::from_secs(1));
    assert!(
        classifier_calls(&r).is_empty(),
        "classifier must be skipped"
    );
    let g = wait_for_generates(&r, 1).await;
    assert_eq!(g[0]["model"], FAST);
    assert_eq!(g[0]["options"]["num_ctx"], 4096);

    // The heuristic rules still apply: a code block goes deep without the classifier.
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("fix ```fn main(){}```", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-tier"], "deep");
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn slow_ps_skips_classifier_within_the_residency_timeout() {
    let mut m = mock("HARD", &["ok"]);
    m.ps_delay_ms = 2000;
    let r = rig_cfg(m, "", None, false, |c| {
        c.routing.residency_check_timeout_ms = 100;
    })
    .await;
    let t = Instant::now();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hello", false),
        &[],
    )
    .await;
    assert_eq!(
        resp.headers()["x-amara-route-reason"],
        "classifier_skipped_ps_unavailable"
    );
    assert!(t.elapsed() < Duration::from_secs(1));
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn configured_classifier_timeout_is_honoured() {
    // 600 ms classifier: inside a 1500 ms timeout it decides, inside 300 ms it times out.
    let mut m = mock("HARD", &["ok"]);
    m.classifier_delay_ms = 600;
    let r = rig_cfg(m, "", None, false, |c| {
        c.routing.classifier_timeout_ms = 1500
    })
    .await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hello", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_hard");
    assert_eq!(resp.headers()["x-amara-tier"], "deep");

    let mut m = mock("HARD", &["ok"]);
    m.classifier_delay_ms = 600;
    let r = rig(m).await; // base config: 300 ms
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("hello", false),
        &[],
    )
    .await;
    assert_eq!(resp.headers()["x-amara-route-reason"], "classifier_timeout");
}

#[tokio::test]
async fn num_ctx_is_in_every_ollama_request_body() {
    let r = rig_with(mock("HARD", &["ok"]), "rewarm", None, false).await;
    for text in ["hello", "/deep x", "/fast y"] {
        post(
            &format!("{}/v1/chat/completions", r.url),
            oa(text, false),
            &[],
        )
        .await
        .text()
        .await
        .unwrap();
    }
    let g = r.mock.lock().unwrap();
    assert!(g.chats.len() >= 4);
    for b in &g.chats {
        let want = if b["model"] == DEEP { 2048 } else { 4096 };
        assert_eq!(b["options"]["num_ctx"], want, "body without num_ctx: {b}");
    }
    // Warm-up bodies are checked in the re-warm and pre-warm tests.
    for b in &g.generates {
        assert!(b["options"]["num_ctx"].is_u64(), "warm-up without num_ctx");
    }
}

#[tokio::test]
async fn first_token_timeout_is_a_fixed_503_and_keeps_warming() {
    let mut m = mock("EASY", &["ok"]);
    m.gen_delay_ms = 3000;
    let r = rig_cfg(m, "", None, false, |c| {
        c.loading.first_token_timeout_ms = 300;
        c.loading.first_token_timeout_resident_ms = 300;
    })
    .await;
    for stream in [false, true] {
        let t = Instant::now();
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("/fast hi", stream),
            &[],
        )
        .await;
        assert_eq!(resp.status(), 503);
        assert!(t.elapsed() < Duration::from_secs(2));
        let body = resp.text().await.unwrap();
        assert!(body.contains(amara_router::engine::LOADING_MSG), "{body}");
        assert!(!body.contains("model failed"), "upstream text leaked");
    }
    // Ollama API too.
    let resp = post(
        &format!("{}/api/chat", r.url),
        json!({"model": "amara-fast", "messages": [{"role": "user", "content": "hi"}]}),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 503);
    assert!(resp
        .text()
        .await
        .unwrap()
        .contains(amara_router::engine::LOADING_MSG));
    let g = wait_for_generates(&r, 1).await;
    assert_eq!(g[0]["model"], FAST, "load continues in the background");
    // A silent deep falls to fast; only with fast silent too does the client get a 503.
    let before = generations(&r).len();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep hi", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 503);
    let gens = generations(&r);
    assert_eq!(gens.len(), before + 2);
    assert_eq!(gens[before]["model"], DEEP);
    assert_eq!(gens[before + 1]["model"], FAST);
}

#[tokio::test]
async fn startup_prewarm_is_background_and_sends_num_ctx() {
    let m = Arc::new(Mutex::new(mock("EASY", &[])));
    let url = start_mock(Arc::clone(&m)).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = base_config(&url, &dir.path().join("r.jsonl"), "");
    cfg.loading.warm_fast_on_start = true;
    cfg.loading.warm_deep_on_start = true;
    let state = State::new(cfg, Secrets::default(), false);
    let t = Instant::now();
    let h = state.prewarm_on_start().expect("pre-warm task");
    assert!(t.elapsed() < Duration::from_millis(50), "must not block");
    h.await.unwrap();
    let g = m.lock().unwrap().generates.clone();
    assert_eq!(g.len(), 2);
    assert_eq!(g[0]["model"], FAST);
    assert_eq!(g[0]["options"]["num_ctx"], 4096);
    assert_eq!(g[0]["keep_alive"], -1);
    assert_eq!(g[1]["model"], DEEP);
    assert_eq!(g[1]["options"]["num_ctx"], 2048);

    let mut cfg = base_config(&url, &dir.path().join("r.jsonl"), "");
    cfg.loading.warm_fast_on_start = false;
    assert!(State::new(cfg, Secrets::default(), false)
        .prewarm_on_start()
        .is_none());
}

// ── /api/ps slow or down: hard-looking heuristic ─────────────────────────────

async fn route_of(r: &Rig, text: &str) -> (String, String) {
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa(text, false),
        &[],
    )
    .await;
    let h = |k: &str| resp.headers()[k].to_str().unwrap().to_string();
    (h("x-amara-tier"), h("x-amara-route-reason"))
}

#[tokio::test]
async fn slow_ps_within_the_default_timeout_still_runs_the_classifier() {
    let mut m = mock("HARD", &["ok"]);
    m.ps_delay_ms = 600; // a model swap: 340-620 ms
    let r = rig(m).await; // residency_check_timeout_ms default 1000
    let (tier, reason) = route_of(&r, "hello").await;
    assert_eq!(
        (tier.as_str(), reason.as_str()),
        ("deep", "classifier_hard")
    );
    assert_eq!(classifier_calls(&r).len(), 1);
}

#[tokio::test]
async fn ps_unavailable_hard_prompt_goes_deep() {
    let mut m = mock("EASY", &["ok"]);
    m.ps_fails = true;
    let r = rig(m).await;
    for text in [
        "Explain why my build fails",
        "compare these two plans",
        "what is X? how does it relate to Y?",
    ] {
        let (tier, reason) = route_of(&r, text).await;
        assert_eq!(
            (tier.as_str(), reason.as_str()),
            ("fast", "ps_unavailable_hard_to_deep; deep_unavailable"),
            "{text}"
        );
    }
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn ps_unavailable_easy_prompt_goes_fast() {
    let mut m = mock("HARD", &["ok"]);
    m.ps_delay_ms = 2000;
    let r = rig_cfg(m, "", None, false, |c| {
        c.routing.residency_check_timeout_ms = 100
    })
    .await;
    let (tier, reason) = route_of(&r, "turn on the office light").await;
    assert_eq!(
        (tier.as_str(), reason.as_str()),
        ("fast", "classifier_skipped_ps_unavailable")
    );
    assert!(classifier_calls(&r).is_empty());
}

#[tokio::test]
async fn fast_not_resident_hard_prompt_goes_deep_whether_or_not_deep_is_loaded() {
    let mut m = mock("EASY", &["ok"]);
    m.ps_models = vec![DEEP.into()];
    m.warm_delay_ms = 10;
    let r = rig(m).await;
    let (tier, reason) = route_of(&r, "debug this for me").await;
    assert_eq!(
        (tier.as_str(), reason.as_str()),
        ("deep", "fast_not_resident_hard_to_deep")
    );
    r.mock.lock().unwrap().ps_models.clear(); // nothing loaded: deep loads and answers
    let (tier, reason) = route_of(&r, "debug this for me").await;
    assert_eq!(
        (tier.as_str(), reason.as_str()),
        ("deep", "fast_not_resident_hard_to_deep")
    );
    assert!(classifier_calls(&r).is_empty());
    let g = wait_for_generates(&r, 1).await;
    assert_eq!(g.len(), 1);
    assert_eq!(g[0]["model"], DEEP);
    // An easy prompt stays fast and starts loading fast.
    let (tier, reason) = route_of(&r, "turn on the light").await;
    assert_eq!(
        (tier.as_str(), reason.as_str()),
        ("fast", "classifier_skipped_fast_not_resident")
    );
    let g = wait_for_generates(&r, 2).await;
    assert_eq!(g[1]["model"], FAST);
}

// ── first-byte timeout: short while loading, long when resident ─────────────
// Scaled timings: short = 300 ms (stands for 20 s), resident = 1500 ms (90 s).

async fn ft_rig(m: Mock) -> Rig {
    rig_cfg(m, "", None, false, |c| {
        c.loading.first_token_timeout_ms = 300;
        c.loading.first_token_timeout_resident_ms = 1500;
    })
    .await
}

fn assert_loading_503(resp: &reqwest::Response) {
    assert_eq!(resp.status(), 503);
    assert_eq!(resp.headers()["retry-after"], "10");
}

#[tokio::test]
async fn resident_model_survives_a_long_silent_prompt_eval() {
    let mut m = mock("EASY", &["answer"]);
    m.gen_delay_ms = 800; // > short timeout, < resident timeout (25 s vs 20 s / 90 s)
    let r = ft_rig(m).await; // fast resident
    for stream in [false, true] {
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("/fast hi", stream),
            &[],
        )
        .await;
        assert_eq!(resp.status(), 200);
        assert!(resp.text().await.unwrap().contains("answer"));
    }
}

#[tokio::test]
async fn non_resident_model_past_short_timeout_is_503_with_retry_after() {
    let mut m = mock("EASY", &["answer"]);
    m.gen_delay_ms = 800;
    m.ps_models = vec![]; // nothing loaded: it is a cold load
    let r = ft_rig(m).await;
    for stream in [false, true] {
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("/fast hi", stream),
            &[],
        )
        .await;
        assert_loading_503(&resp);
        let body = resp.text().await.unwrap();
        assert!(body.contains(amara_router::engine::LOADING_MSG));
    }
    for stream in [false, true] {
        let resp = post(
            &format!("{}/api/chat", r.url),
            json!({"model": "amara-fast", "stream": stream, "messages": [{"role": "user", "content": "hi"}]}),
            &[],
        )
        .await;
        assert_loading_503(&resp);
        assert!(resp
            .text()
            .await
            .unwrap()
            .contains(amara_router::engine::LOADING_MSG));
    }
}

#[tokio::test]
async fn resident_model_past_long_timeout_is_503() {
    let mut m = mock("EASY", &["answer"]);
    m.gen_delay_ms = 2500;
    let r = ft_rig(m).await;
    let t = Instant::now();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_loading_503(&resp);
    assert!(
        t.elapsed() >= Duration::from_millis(1400),
        "used the short timeout"
    );
}

#[tokio::test]
async fn ps_unavailable_uses_the_short_timeout() {
    let mut m = mock("EASY", &["answer"]);
    m.gen_delay_ms = 800;
    m.ps_fails = true;
    let r = ft_rig(m).await;
    let t = Instant::now();
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_loading_503(&resp);
    assert!(t.elapsed() < Duration::from_millis(800));
}

#[tokio::test]
async fn other_errors_have_no_retry_after() {
    let mut m = mock("EASY", &["answer"]);
    m.fail_models = vec![FAST.into()];
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 502);
    assert!(resp.headers().get("retry-after").is_none());
}

// ── background deep load ─────────────────────────────────────────────────────

/// Fast resident, deep not; a warm-up takes `warm_ms`.
fn cold_deep(warm_ms: u64, pieces: &[&str]) -> Mock {
    let mut m = mock("EASY", pieces);
    m.ps_models = vec![FAST.to_string()];
    m.warm_delay_ms = warm_ms;
    m
}

async fn eventually(r: &Rig, what: &str, done: impl Fn(&Mock) -> bool) {
    for _ in 0..100 {
        if done(&r.mock.lock().unwrap()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never happened: {what}");
}

fn route_headers(resp: &reqwest::Response) -> (String, String) {
    let text = |k: &str| resp.headers()[k].to_str().unwrap().to_string();
    (text("x-amara-route"), text("x-amara-route-reason"))
}

async fn deep_ask(r: &Rig, stream: bool) -> reqwest::Response {
    let url = format!("{}/v1/chat/completions", r.url);
    post(&url, oa("/deep hi", stream), &[]).await
}

#[tokio::test]
async fn cold_deep_loads_and_answers_on_deep_in_the_same_request() {
    let r = rig(cold_deep(50, &["deep answer"])).await;
    for stream in [false, true] {
        let resp = deep_ask(&r, stream).await;
        assert_eq!(resp.status(), 200);
        let (route, reason) = route_headers(&resp);
        assert_eq!(route, "deep");
        assert!(!reason.contains("deep_"), "{reason}");
        assert_eq!(resp.headers()["x-amara-model"], DEEP);
        assert!(resp.text().await.unwrap().contains("deep answer"));
    }
    let g = r.mock.lock().unwrap();
    assert_eq!(g.unloads, [FAST], "fast is unloaded by hand first");
    assert_eq!(
        g.generates.len(),
        1,
        "the second request found deep resident"
    );
    assert_eq!(g.generates[0]["model"], DEEP);
    assert_eq!(g.generates[0]["keep_alive"], "5m");
    assert!(g.generates[0]["options"]["num_ctx"].is_u64());
    assert!(g.chats.iter().all(|b| b["model"] == DEEP));
}

#[tokio::test]
async fn long_prompt_evaluation_after_the_load_has_the_resident_timeout() {
    let mut m = cold_deep(20, &["deep answer"]);
    m.gen_delay_ms = 800; // past the cold timeouts, inside the resident one
    let r = rig_cfg(m, "", None, false, |c| {
        c.loading.first_token_timeout_ms = 300;
        c.loading.deep_load_timeout_ms = 300;
        c.loading.first_token_timeout_resident_ms = 3000;
    })
    .await;
    let resp = deep_ask(&r, false).await;
    assert_eq!(route_headers(&resp).0, "deep");
}

#[tokio::test]
async fn the_load_starts_only_after_the_fast_reply_ends() {
    let mut m = cold_deep(50, &["a long enough fast reply to take a while"]);
    m.chunk_delay_ms = 20;
    let r = rig(m).await;
    let fast = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", true),
        &[],
    )
    .await;
    let deep = tokio::spawn({
        let url = format!("{}/v1/chat/completions", r.url);
        async move { post(&url, oa("/deep hi", false), &[]).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        r.mock.lock().unwrap().unloads.is_empty(),
        "fast was evicted mid-reply"
    );
    fast.text().await.unwrap();
    assert_eq!(route_headers(&deep.await.unwrap()).0, "deep");
}

#[tokio::test]
async fn the_load_survives_clients_disconnecting() {
    let r = rig(cold_deep(400, &["ok"])).await;
    let gone = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", r.url))
        .json(&oa("/deep hi", false))
        .timeout(Duration::from_millis(150))
        .send()
        .await;
    assert!(gone.is_err(), "the client should have given up");
    assert!(r.mock.lock().unwrap().warms_done.is_empty());
    eventually(&r, "deep loaded", |m| m.warms_done == [DEEP]).await;
}

#[tokio::test]
async fn concurrent_cold_deep_requests_share_one_load() {
    let r = rig(cold_deep(200, &["deep answer"])).await;
    let replies = futures::future::join_all((0..4).map(|_| async {
        let resp = deep_ask(&r, false).await;
        assert_eq!(resp.status(), 200);
        assert_eq!(route_headers(&resp).0, "deep");
    }));
    replies.await;
    assert_eq!(r.mock.lock().unwrap().generates.len(), 1);
}

#[tokio::test]
async fn a_resident_deep_is_served_with_no_load() {
    let r = rig(mock("EASY", &["deep answer"])).await;
    let resp = deep_ask(&r, false).await;
    let (route, reason) = route_headers(&resp);
    assert_eq!(route, "deep");
    assert!(!reason.contains("deep_"), "{reason}");
    assert!(resp.text().await.unwrap().contains("deep answer"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let g = r.mock.lock().unwrap();
    assert!(g.generates.is_empty() && g.unloads.is_empty());
}

#[tokio::test]
async fn a_fast_request_waits_for_deep_then_restores_fast() {
    let r = rig(cold_deep(500, &["answer"])).await;
    let deep = tokio::spawn({
        let url = format!("{}/v1/chat/completions", r.url);
        async move { post(&url, oa("/deep hi", false), &[]).await }
    });
    eventually(&r, "load started", |m| !m.generates.is_empty()).await;
    let fast = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_eq!(fast.status(), 200);
    assert_eq!(route_headers(&fast).0, "fast");
    let deep = deep.await.unwrap();
    assert_eq!(route_headers(&deep).0, "deep", "the load was not cancelled");
    let g = r.mock.lock().unwrap();
    assert_eq!(g.warms_done, [DEEP]);
    assert_eq!(
        g.unloads,
        [FAST, DEEP],
        "deep was moved out for fast by hand"
    );
    assert_eq!(g.chats.last().unwrap()["model"], FAST);
}

#[tokio::test]
async fn a_fast_request_stops_waiting_with_a_503_at_the_limit() {
    let mut m = mock("EASY", &["a deep reply that takes its time to finish"]);
    m.chunk_delay_ms = 50;
    let r = rig_cfg(m, "", None, false, |c| c.loading.fast_wait_ms = 200).await;
    let deep = deep_ask(&r, true).await;
    let t = Instant::now();
    let fast = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_loading_503(&fast);
    assert!(t.elapsed() < Duration::from_millis(800));
    assert!(deep.text().await.unwrap().contains("deep reply"));
}

#[tokio::test]
async fn the_next_fast_request_after_deep_moves_deep_out() {
    let mut m = mock("EASY", &["ok"]);
    m.ps_models = vec![DEEP.into()];
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast hi", false),
        &[],
    )
    .await;
    assert_eq!(route_headers(&resp).0, "fast");
    assert_eq!(r.mock.lock().unwrap().unloads, [DEEP]);
}

const VOICE_PING: &str = "Speak plainly.\nVoice mode is starting up. Reply with just: ready.";

#[tokio::test]
async fn the_voice_warm_ping_never_evicts_deep() {
    let mut m = mock("EASY", &["from the model"]);
    m.ps_models = vec![DEEP.into()];
    let r = rig(m).await;
    let url = format!("{}/v1/chat/completions", r.url);
    for stream in [false, true] {
        let resp = post(&url, oa(VOICE_PING, stream), &[]).await;
        let (route, reason) = route_headers(&resp);
        assert_eq!(route, "fast");
        assert!(reason.contains("warm_ping_skipped"), "{reason}");
        assert!(resp.text().await.unwrap().contains("ready."));
    }
    let g = r.mock.lock().unwrap();
    assert!(g.unloads.is_empty() && g.chats.is_empty());
}

#[tokio::test]
async fn the_voice_warm_ping_reaches_fast_when_deep_is_away() {
    let r = rig(cold_deep(0, &["from the model"])).await;
    let url = format!("{}/v1/chat/completions", r.url);
    let resp = post(&url, oa(VOICE_PING, false), &[]).await;
    assert!(resp.text().await.unwrap().contains("from the model"));
}

#[tokio::test]
async fn a_failed_load_answers_on_fast_and_waits_before_trying_again() {
    let mut m = cold_deep(10, &["ok"]);
    m.warm_fails = vec![DEEP.to_string()];
    let r = rig(m).await;
    let resp = deep_ask(&r, false).await;
    assert_eq!(resp.status(), 200);
    let (route, reason) = route_headers(&resp);
    assert_eq!(route, "fast");
    assert!(reason.contains("deep_unavailable"), "{reason}");
    assert_eq!(resp.headers()["x-amara-model"], FAST);
    let t = Instant::now();
    let resp = deep_ask(&r, false).await;
    assert_eq!(route_headers(&resp).0, "fast");
    assert!(t.elapsed() < Duration::from_millis(500), "no second wait");
    let deep_loads = |m: &Mock| m.generates.iter().filter(|b| b["model"] == DEEP).count();
    assert_eq!(deep_loads(&r.mock.lock().unwrap()), 1);
}

#[tokio::test]
async fn a_load_past_its_window_answers_on_fast() {
    let r = rig_cfg(cold_deep(5000, &["ok"]), "", None, false, |c| {
        c.loading.deep_load_timeout_ms = 200;
    })
    .await;
    let t = Instant::now();
    let resp = deep_ask(&r, false).await;
    assert_eq!(resp.status(), 200);
    let (route, reason) = route_headers(&resp);
    assert_eq!(route, "fast");
    assert!(reason.contains("deep_unavailable"), "{reason}");
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn resident_deep_silent_past_its_timeout_answers_on_fast_not_503() {
    let mut m = mock("EASY", &["ok"]);
    m.gen_delay_ms = 400;
    m.fail_models = vec![];
    let r = rig_cfg(m, "", None, false, |c| {
        c.loading.first_token_timeout_resident_ms = 300;
    })
    .await;
    assert_loading_503(&deep_ask(&r, false).await); // fast is just as slow here
    r.mock.lock().unwrap().gen_delay_ms = 0;
    assert_eq!(deep_ask(&r, false).await.status(), 200);
}

#[tokio::test]
async fn fast_is_rewarmed_once_deep_has_unloaded() {
    let r = rig_cfg(cold_deep(10, &["ok"]), "rewarm", None, false, |c| {
        c.loading.poll_secs = 1;
    })
    .await;
    deep_ask(&r, false).await.text().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        r.mock.lock().unwrap().generates.len(),
        1,
        "no re-warm while deep is resident"
    );
    r.mock.lock().unwrap().ps_models.clear(); // keep-alive ended
    eventually(&r, "fast re-warmed", |m| {
        m.warms_done.contains(&FAST.to_string())
    })
    .await;
}

#[tokio::test]
async fn a_503_stays_only_when_fast_cannot_answer_either() {
    let mut m = cold_deep(0, &["ok"]);
    m.ps_models.clear();
    m.gen_delay_ms = 800;
    let r = ft_rig(m).await;
    assert_loading_503(&deep_ask(&r, false).await);
}

// ── route headers and real model ids ─────────────────────────────────────────

fn assert_route(resp: &reqwest::Response, tier: &str, model: &str, reason: &str) {
    let h = resp.headers();
    assert_eq!(h["x-amara-route"], tier);
    assert_eq!(h["x-amara-tier"], tier);
    assert_eq!(h["x-amara-model"], model);
    assert_eq!(h["x-amara-route-reason"], reason);
}

#[tokio::test]
async fn every_chat_reply_states_route_and_real_model() {
    let r = rig(mock("EASY", &["Hi ", "there"])).await;
    // OpenAI, non-streamed and streamed.
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep a", false),
        &[],
    )
    .await;
    assert_route(&resp, "deep", DEEP, "slash_deep");
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["model"], DEEP);
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/fast a", true),
        &[],
    )
    .await;
    assert_route(&resp, "fast", FAST, "slash_fast");
    let text = resp.text().await.unwrap();
    let chunks: Vec<Value> = text
        .split("\n\n")
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|l| *l != "[DONE]")
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(chunks.len() > 2);
    assert!(chunks.iter().all(|c| c["model"] == FAST), "{text}");
    // Ollama, streamed and not.
    for stream in [true, false] {
        let body = json!({"model": "amara:latest", "stream": stream, "messages": [{"role": "user", "content": "/deep hi"}]});
        let resp = post(&format!("{}/api/chat", r.url), body, &[]).await;
        assert_eq!(resp.status(), 200);
        assert_route(&resp, "deep", DEEP, "slash_deep");
        let lines: Vec<Value> = resp
            .text()
            .await
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| l["model"] == DEEP));
    }
    // Dry run carries the same headers.
    let resp = post(
        &format!("{}/amara/route", r.url),
        json!({"text": "/deep x"}),
        &[],
    )
    .await;
    assert_route(&resp, "deep", DEEP, "slash_deep");
}

#[tokio::test]
async fn fallback_headers_and_model_name_the_tier_that_answered() {
    let mut m = mock("EASY", &["from fast"]);
    m.fail_models = vec![DEEP.into()];
    let r = rig(m).await;
    for stream in [false, true] {
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("/deep x", stream),
            &[],
        )
        .await;
        assert_eq!(resp.status(), 200);
        assert_route(&resp, "fast", FAST, "slash_deep; deep_failed");
        let text = resp.text().await.unwrap();
        assert!(text.contains(&format!("\"model\":\"{FAST}\"")), "{text}");
        assert!(!text.contains(DEEP), "{text}");
    }
}

#[tokio::test]
async fn errors_after_routing_carry_route_headers() {
    // 502: every tier fails.
    let mut m = mock("EASY", &[]);
    m.fail_models = vec![DEEP.into(), FAST.into()];
    let r = rig(m).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("/deep x", false),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 502);
    assert!(resp.headers().contains_key("x-amara-route"));
    assert!(resp.headers().contains_key("x-amara-model"));
    assert!(resp.headers().contains_key("x-amara-route-reason"));

    // 503: Ollama down, streamed and not, both APIs.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let r = rig_with(mock("EASY", &[]), "", Some(dead), false).await;
    for stream in [false, true] {
        let resp = post(
            &format!("{}/v1/chat/completions", r.url),
            oa("/fast hi", stream),
            &[],
        )
        .await;
        assert_eq!(resp.status(), 503);
        assert_route(&resp, "fast", FAST, "slash_fast");
        let resp = post(&format!("{}/api/chat", r.url), json!({"model": "amara", "stream": stream, "messages": [{"role": "user", "content": "/deep hi"}]}), &[]).await;
        assert_eq!(resp.status(), 503);
        assert_eq!(resp.headers()["x-amara-route"], "deep");
        assert!(resp.headers().contains_key("x-amara-model"));
    }

    // 400s happen before routing: no route headers.
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        json!({"messages": "nope"}),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 400);
    assert!(!resp.headers().contains_key("x-amara-route"));
}

// ── client cancellation ──────────────────────────────────────────────────────

async fn route_log_lines(r: &Rig) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(&r.log) {
            let lines: Vec<Value> = text
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            if !lines.is_empty() || Instant::now() > deadline {
                return lines;
            }
        } else if Instant::now() > deadline {
            return vec![];
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn client_gone_before_first_token_logs_cancelled() {
    let mut m = mock("EASY", &["late"]);
    m.gen_delay_ms = 30_000;
    let r = rig(m).await;
    let sent = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", r.url))
        .timeout(Duration::from_millis(400))
        .json(&oa("/deep hi", false))
        .send()
        .await;
    assert!(sent.is_err(), "the client should have timed out");
    let lines = route_log_lines(&r).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["status"], "cancelled");
    assert_eq!(lines[0]["route"], "deep");
    assert_eq!(lines[0]["model"], DEEP);
    assert_eq!(lines[0]["reason"], "slash_deep");
    assert!(lines[0]["ts"].is_string() && lines[0]["latency_ms"].is_u64());
}

#[tokio::test]
async fn client_gone_mid_stream_logs_cancelled_once() {
    let mut m = mock("EASY", &["one", "two", "three"]);
    m.chunk_delay_ms = 100;
    let r = rig(m).await;
    let mut resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("Capital of France?", true),
        &[],
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert!(resp.chunk().await.unwrap().is_some());
    drop(resp);
    let lines = route_log_lines(&r).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["status"], "cancelled");
    assert_eq!(lines[0]["route"], "fast");
}

#[tokio::test]
async fn finished_reply_logs_one_ok_line_and_no_cancelled_line() {
    let r = rig(mock("EASY", &["Paris."])).await;
    let resp = post(
        &format!("{}/v1/chat/completions", r.url),
        oa("Capital of France?", true),
        &[],
    )
    .await;
    resp.text().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let lines = route_log_lines(&r).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["status"], "ok");
}
