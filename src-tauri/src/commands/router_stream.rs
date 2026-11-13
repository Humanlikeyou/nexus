//! Router streaming client (turn-pipeline plan, lane L1; decision 0019).
//!
//! `stream_chat` POSTs a streamed chat completion to the loopback Amara router and reads the SSE
//! reply with `Response::chunk()` (no new crates). Typed chat never reaches Claude: the request
//! always carries `X-Amara-Allow-Claude: 0` and `X-Amara-Mode: local`, and a reply that reports the
//! Claude tier is refused. Only the tier (`fast`|`deep`) is ever returned, never a model or provider.
//! Nothing calls this yet; the pipeline lane (L2) does.
#![allow(dead_code)] // consumed by the pipeline lane (L2)

use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Notify;

use super::loopback_http::{
    is_loopback_url, loopback_base, loopback_client, read_capped, BodyError,
    LOOPBACK_CONNECT_TIMEOUT,
};

/// Wait for the first byte: covers a cold deep-model load (same as `ask_router`).
pub(crate) const ROUTER_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(600);
/// Longest silence between chunks before the stream is called stalled.
pub(crate) const ROUTER_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Hard cap on one direct turn.
pub(crate) const ROUTER_TURN_CEILING: Duration = Duration::from_secs(15 * 60);
/// Total streamed body cap.
pub(crate) const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
/// Longest single SSE line.
pub(crate) const MAX_SSE_LINE_BYTES: usize = 64 * 1024;
/// An error reply body is read only to this size, then dropped.
const MAX_ERROR_BODY_BYTES: usize = 4 * 1024;
/// Malformed `data:` lines tolerated before the stream is refused.
const MAX_MALFORMED_LINES: usize = 16;

#[derive(Debug, Clone, Copy)]
pub(crate) struct StreamLimits {
    pub first_byte: Duration,
    pub idle: Duration,
    pub ceiling: Duration,
    pub max_bytes: usize,
    pub max_line: usize,
}

impl Default for StreamLimits {
    fn default() -> Self {
        Self {
            first_byte: ROUTER_FIRST_BYTE_TIMEOUT,
            idle: ROUTER_STREAM_IDLE_TIMEOUT,
            ceiling: ROUTER_TURN_CEILING,
            max_bytes: MAX_STREAM_BYTES,
            max_line: MAX_SSE_LINE_BYTES,
        }
    }
}

/// Why a stream did not complete. `message` is fixed copy: no upstream text, model or provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamError {
    Cancelled,
    /// Navy local-only (or the outbound gate) was on at send time.
    Blocked,
    NotLoopback,
    ClaudeTier,
    TooLarge,
    LineTooLong,
    Malformed,
    Truncated,
    Timeout,
    Stalled,
    Unreachable,
    /// The router answered with this HTTP status (a 503 also carries its Retry-After seconds).
    Http(u16, Option<u64>),
    /// The router sent an in-stream error event.
    Router,
}

impl StreamError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Blocked => "local_only",
            Self::NotLoopback => "not_loopback",
            Self::ClaudeTier => "claude_refused",
            Self::TooLarge => "too_large",
            Self::LineTooLong => "line_too_long",
            Self::Malformed => "malformed",
            Self::Truncated => "truncated",
            Self::Timeout => "timeout",
            Self::Stalled => "stalled",
            Self::Unreachable => "unreachable",
            Self::Http(..) => "http",
            Self::Router => "router_error",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "Stopped.",
            Self::Blocked => "Amara is in local-only mode, so this was not sent.",
            Self::ClaudeTier | Self::NotLoopback => "Amara couldn't answer that locally.",
            Self::Timeout | Self::Stalled => "Amara took too long to answer.",
            Self::Unreachable => {
                "Amara isn't responding right now. Check that the Amara router is running."
            }
            _ => "Amara couldn't answer right now. Details are in the Amara router log.",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct StreamUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct StreamOutcome {
    /// All delta text, in order.
    pub text: String,
    /// `fast` or `deep` when the router said so.
    pub tier: Option<&'static str>,
    pub usage: Option<StreamUsage>,
}

/// Router tier word to the only values we return. `None` for the Claude tier is signalled by
/// `Err(ClaudeTier)`; an unknown word is dropped rather than echoed.
fn normalize_tier(word: &str) -> Result<Option<&'static str>, StreamError> {
    let w = word.trim().to_ascii_lowercase();
    if w.contains("claude") || w.contains("anthropic") {
        return Err(StreamError::ClaudeTier);
    }
    Ok(match w.as_str() {
        "fast" | "light" => Some("fast"),
        "deep" => Some("deep"),
        _ => None,
    })
}

/// What one SSE line meant.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SseEvent {
    Delta(String),
    Tier(Option<&'static str>),
    Usage(StreamUsage),
    Done,
    RouterError,
    /// Not a `data:` line (comment, `event:`, blank) or a harmless chunk with nothing to use.
    Ignore,
    Malformed,
    ClaudeTier,
}

/// Parse one complete SSE line (no trailing newline). Never panics on bad input.
pub(crate) fn parse_sse_line(line: &[u8]) -> Vec<SseEvent> {
    let Ok(line) = std::str::from_utf8(line) else {
        return vec![SseEvent::Malformed];
    };
    let line = line.trim_end_matches('\r');
    let Some(rest) = line.strip_prefix("data:") else {
        return vec![SseEvent::Ignore];
    };
    let rest = rest.trim();
    if rest == "[DONE]" {
        return vec![SseEvent::Done];
    }
    let Ok(v) = serde_json::from_str::<Value>(rest) else {
        return vec![SseEvent::Malformed];
    };
    if v.get("error").is_some() {
        return vec![SseEvent::RouterError];
    }
    let mut out = Vec::new();
    if let Some(t) = v.pointer("/amara/tier").and_then(Value::as_str) {
        match normalize_tier(t) {
            Err(_) => return vec![SseEvent::ClaudeTier],
            Ok(tier) => out.push(SseEvent::Tier(tier)),
        }
    }
    if let Some(t) = v
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
    {
        out.push(SseEvent::Delta(t.to_string()));
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        out.push(SseEvent::Usage(StreamUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            completion_tokens: u
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        }));
    }
    if out.is_empty() {
        out.push(SseEvent::Ignore);
    }
    out
}

/// The request body: `stream: true`, model alias `amara` (the router picks the tier).
pub(crate) fn stream_body(messages: &[Value]) -> Value {
    json!({
        "model": "amara",
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
    })
}

/// Stream one chat completion. `base` must be a loopback router base; `blocked` is the navy /
/// outbound check, called again right before the send. `on_delta` gets each text piece in order.
/// Setting `cancel` (`Notify::notify_one`) drops the connection and returns `Cancelled`.
pub(crate) async fn stream_chat(
    base: &str,
    messages: &[Value],
    deep: bool,
    blocked: &dyn Fn() -> bool,
    cancel: &Notify,
    limits: StreamLimits,
    mut on_delta: impl FnMut(&str),
) -> Result<StreamOutcome, StreamError> {
    let base = loopback_base(base).ok_or(StreamError::NotLoopback)?;
    let url = format!("{base}/v1/chat/completions");
    if !is_loopback_url(&url) {
        return Err(StreamError::NotLoopback);
    }
    let client = loopback_client(LOOPBACK_CONNECT_TIMEOUT);
    let mut req = client
        .post(&url)
        .header("Accept", "text/event-stream")
        .header("X-Amara-Allow-Claude", "0")
        .header("X-Amara-Mode", "local")
        .json(&stream_body(messages));
    if deep {
        req = req.header("X-Amara-Route", "deep");
    }

    // Re-checked here, right before the send, not only at the start of the turn.
    if blocked() {
        return Err(StreamError::Blocked);
    }
    let started = tokio::time::Instant::now();
    let sent = tokio::select! {
        biased;
        _ = cancel.notified() => return Err(StreamError::Cancelled),
        r = tokio::time::timeout(limits.first_byte, req.send()) => r,
    };
    let mut resp = match sent {
        Err(_) => return Err(StreamError::Timeout),
        Ok(Err(_)) => return Err(StreamError::Unreachable),
        Ok(Ok(r)) => r,
    };
    let status = resp.status();
    if !status.is_success() {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        // Read (and drop) a bounded error body; its text is never surfaced.
        let _ = read_capped_limited(resp).await;
        return Err(StreamError::Http(status.as_u16(), retry_after));
    }
    let mut out = StreamOutcome::default();
    if let Some(r) = resp
        .headers()
        .get("x-amara-route")
        .and_then(|v| v.to_str().ok())
    {
        out.tier = normalize_tier(r)?;
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let mut malformed = 0usize;
    let mut done = false;
    while !done {
        if started.elapsed() > limits.ceiling {
            return Err(StreamError::Timeout);
        }
        let chunk = tokio::select! {
            biased;
            _ = cancel.notified() => return Err(StreamError::Cancelled),
            r = tokio::time::timeout(limits.idle, resp.chunk()) => match r {
                Err(_) => return Err(StreamError::Stalled),
                Ok(Err(_)) => return Err(StreamError::Truncated),
                Ok(Ok(c)) => c,
            },
        };
        let Some(chunk) = chunk else { break };
        total += chunk.len();
        if total > limits.max_bytes {
            return Err(StreamError::TooLarge);
        }
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if line.len() > limits.max_line + 1 {
                return Err(StreamError::LineTooLong);
            }
            let line = &line[..line.len() - 1];
            for ev in parse_sse_line(line) {
                match ev {
                    SseEvent::Delta(t) => {
                        out.text.push_str(&t);
                        on_delta(&t);
                    }
                    SseEvent::Tier(t) => {
                        if t.is_some() {
                            out.tier = t;
                        }
                    }
                    SseEvent::Usage(u) => out.usage = Some(u),
                    SseEvent::Done => done = true,
                    SseEvent::RouterError => return Err(StreamError::Router),
                    SseEvent::ClaudeTier => return Err(StreamError::ClaudeTier),
                    SseEvent::Malformed => {
                        malformed += 1;
                        if malformed > MAX_MALFORMED_LINES {
                            return Err(StreamError::Malformed);
                        }
                    }
                    SseEvent::Ignore => {}
                }
            }
        }
        if buf.len() > limits.max_line {
            return Err(StreamError::LineTooLong);
        }
    }
    if !done {
        return Err(StreamError::Truncated);
    }
    Ok(out)
}

async fn read_capped_limited(resp: reqwest::Response) -> Result<Vec<u8>, BodyError> {
    read_capped(resp, MAX_ERROR_BODY_BYTES).await
}

/// Holds back text that could be the start of the `<nexus_action>` protocol tag so no part of it
/// reaches a visible delta. A lone `<` that turns out not to be the tag is released.
#[derive(Debug, Default)]
pub(crate) struct TagHoldback {
    held: String,
    in_tag: bool,
}

const ACTION_TAG: &str = "<nexus_action>";

impl TagHoldback {
    pub(crate) fn push(&mut self, text: &str) -> String {
        if self.in_tag {
            return String::new();
        }
        self.held.push_str(text);
        let mut out = String::new();
        loop {
            let Some(i) = self.held.find('<') else {
                out.push_str(&self.held);
                self.held.clear();
                return out;
            };
            out.push_str(&self.held[..i]);
            self.held.drain(..i);
            if self.held.starts_with(ACTION_TAG) {
                self.in_tag = true;
                self.held.clear();
                return out;
            }
            if ACTION_TAG.starts_with(self.held.as_str()) {
                return out; // could still become the tag: keep holding
            }
            out.push('<');
            self.held.drain(..1);
        }
    }

    /// End of stream: release a held non-tag fragment; inside a tag, nothing.
    pub(crate) fn finish(&mut self) -> String {
        if self.in_tag {
            self.held.clear();
            return String::new();
        }
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn sse(v: Value) -> String {
        format!("data: {v}\n\n")
    }
    fn delta(t: &str) -> String {
        sse(json!({"choices":[{"delta":{"content": t},"finish_reason":null}]}))
    }
    fn first(tier: &str) -> String {
        sse(
            json!({"choices":[{"delta":{"role":"assistant","content":""}}],"amara":{"tier":tier,"reason":"x","routing_ms":1}}),
        )
    }
    fn usage() -> String {
        sse(json!({"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3}}))
    }

    /// Fake loopback SSE server. Writes `parts` (a short pause between each), then, when `hold`,
    /// keeps the socket open until the client closes it (flag set on close).
    async fn serve(parts: Vec<String>, hold: bool) -> (String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let closed = Arc::new(AtomicBool::new(false));
        let flag = closed.clone();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n")
                .await;
            for p in parts {
                let frame = format!("{:x}\r\n{}\r\n", p.len(), p);
                if s.write_all(frame.as_bytes()).await.is_err() {
                    flag.store(true, Ordering::SeqCst);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            if hold {
                // Keep writing keep-alives until the peer is gone.
                loop {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    if s.write_all(b"1\r\n\n\r\n").await.is_err() {
                        flag.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
            let _ = s.write_all(b"0\r\n\r\n").await;
        });
        (base, closed)
    }

    fn msgs() -> Vec<Value> {
        vec![json!({"role":"user","content":"hi"})]
    }
    async fn run(
        base: &str,
        limits: StreamLimits,
    ) -> (Result<StreamOutcome, StreamError>, Vec<String>) {
        let cancel = Notify::new();
        let mut got = Vec::new();
        let r = stream_chat(base, &msgs(), false, &|| false, &cancel, limits, |t| {
            got.push(t.to_string())
        })
        .await;
        (r, got)
    }

    #[tokio::test]
    async fn deltas_arrive_in_order_with_tier_and_usage() {
        let parts = vec![
            first("light"),
            delta("Hel"),
            delta("lo "),
            delta("wörld"),
            usage(),
            "data: [DONE]\n\n".into(),
        ];
        let (base, _) = serve(parts, false).await;
        let (r, got) = run(&base, StreamLimits::default()).await;
        let out = r.unwrap();
        assert_eq!(got, ["Hel", "lo ", "wörld"]);
        assert_eq!(out.text, "Hello wörld");
        assert_eq!(out.tier, Some("fast"));
        assert_eq!(
            out.usage,
            Some(StreamUsage {
                prompt_tokens: 7,
                completion_tokens: 3
            })
        );
    }

    #[tokio::test]
    async fn line_split_across_chunks_and_utf8_split_mid_char() {
        let whole = delta("héllo");
        let mid = whole.find("llo").unwrap();
        let parts = vec![
            first("deep"),
            whole[..mid].to_string(),
            whole[mid..].to_string(),
            "data: [DONE]\n\n".into(),
        ];
        let (base, _) = serve(parts, false).await;
        let (r, got) = run(&base, StreamLimits::default()).await;
        assert_eq!(got, ["héllo"]);
        assert_eq!(r.unwrap().tier, Some("deep"));
    }

    #[tokio::test]
    async fn cancel_mid_stream_stops_reading_and_closes() {
        let mut parts = vec![first("light"), delta("one")];
        parts.push(delta("two"));
        let (base, closed) = serve(parts, true).await;
        let cancel = Notify::new();
        let mut got: Vec<String> = Vec::new();
        let r = stream_chat(
            &base,
            &msgs(),
            false,
            &|| false,
            &cancel,
            StreamLimits::default(),
            |t| {
                got.push(t.to_string());
                cancel.notify_one(); // cancel on the first delta
            },
        )
        .await;
        assert_eq!(r, Err(StreamError::Cancelled));
        assert_eq!(got, ["one"], "nothing is delivered after the cancel");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(closed.load(Ordering::SeqCst), "server saw the drop");
    }

    #[tokio::test]
    async fn oversize_body_is_refused() {
        let mut parts = vec![first("light")];
        for _ in 0..50 {
            parts.push(delta(&"x".repeat(100)));
        }
        let (base, _) = serve(parts, true).await;
        let limits = StreamLimits {
            max_bytes: 1000,
            ..Default::default()
        };
        let (r, got) = run(&base, limits).await;
        assert_eq!(r, Err(StreamError::TooLarge));
        assert!(got.len() < 50);
    }

    #[tokio::test]
    async fn overlong_line_is_refused() {
        let parts = vec![first("light"), format!("data: {}", "y".repeat(5000))];
        let (base, _) = serve(parts, true).await;
        let limits = StreamLimits {
            max_line: 1000,
            ..Default::default()
        };
        let (r, _) = run(&base, limits).await;
        assert_eq!(r, Err(StreamError::LineTooLong));
    }

    #[tokio::test]
    async fn malformed_line_is_skipped() {
        let parts = vec![
            first("light"),
            "data: {not json\n\n".into(),
            ": keep-alive comment\n\n".into(),
            delta("ok"),
            "data: [DONE]\n\n".into(),
        ];
        let (base, _) = serve(parts, false).await;
        let (r, got) = run(&base, StreamLimits::default()).await;
        assert_eq!(r.unwrap().text, "ok");
        assert_eq!(got, ["ok"]);
    }

    #[tokio::test]
    async fn endless_malformed_lines_are_refused() {
        let mut parts = vec![first("light")];
        for _ in 0..40 {
            parts.push("data: {bad\n\n".into());
        }
        let (base, _) = serve(parts, true).await;
        let (r, _) = run(&base, StreamLimits::default()).await;
        assert_eq!(r, Err(StreamError::Malformed));
    }

    #[tokio::test]
    async fn claude_tier_is_refused_before_any_text() {
        for word in ["claude", "Claude-3", "anthropic"] {
            let parts = vec![first(word), delta("leak"), "data: [DONE]\n\n".into()];
            let (base, _) = serve(parts, false).await;
            let (r, got) = run(&base, StreamLimits::default()).await;
            assert_eq!(r, Err(StreamError::ClaudeTier), "{word}");
            assert!(got.is_empty(), "{word}");
        }
    }

    #[tokio::test]
    async fn in_stream_router_error_is_fixed_copy() {
        let parts = vec![
            first("light"),
            sse(json!({"error":{"message":"model gemma blew up","type":"x"}})),
            "data: [DONE]\n\n".into(),
        ];
        let (base, _) = serve(parts, false).await;
        let (r, _) = run(&base, StreamLimits::default()).await;
        let e = r.unwrap_err();
        assert_eq!(e, StreamError::Router);
        assert!(!e.message().to_lowercase().contains("gemma"));
    }

    #[tokio::test]
    async fn stream_without_done_is_truncated() {
        let (base, _) = serve(vec![first("light"), delta("a")], false).await;
        let (r, _) = run(&base, StreamLimits::default()).await;
        assert_eq!(r, Err(StreamError::Truncated));
    }

    #[tokio::test]
    async fn idle_stream_is_stalled() {
        let (base, _) = serve(vec![first("light")], true).await;
        // keep-alive blank lines don't count as progress for a 0-chunk wait; use a tiny idle.
        let limits = StreamLimits {
            idle: Duration::from_millis(1),
            ..Default::default()
        };
        let (r, _) = run(&base, limits).await;
        assert!(matches!(
            r,
            Err(StreamError::Stalled) | Err(StreamError::Truncated)
        ));
    }

    #[tokio::test]
    async fn non_loopback_base_is_refused_without_connecting() {
        for bad in [
            "http://example.com",
            "http://192.168.1.5:8787",
            "http://127.0.0.1@evil.example",
            "https://127.0.0.1:8787",
        ] {
            let (r, got) = run(bad, StreamLimits::default()).await;
            assert_eq!(r, Err(StreamError::NotLoopback), "{bad}");
            assert!(got.is_empty());
        }
    }

    #[tokio::test]
    async fn blocked_check_stops_the_send() {
        let (base, _) = serve(vec![first("light")], false).await;
        let cancel = Notify::new();
        let r = stream_chat(
            &base,
            &msgs(),
            false,
            &|| true,
            &cancel,
            StreamLimits::default(),
            |_| {},
        )
        .await;
        assert_eq!(r, Err(StreamError::Blocked));
    }

    #[tokio::test]
    async fn http_error_status_is_reported_without_body_text() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = vec![0u8; 8192];
            let _ = s.read(&mut b).await;
            let body = "model qwen is loading";
            let _ = s
                .write_all(
                    format!(
                        "HTTP/1.1 503 Service Unavailable\r\nretry-after: 10\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
        });
        let (r, _) = run(&base, StreamLimits::default()).await;
        assert_eq!(r, Err(StreamError::Http(503, Some(10))));
    }

    #[test]
    fn typed_chat_headers_and_gate_source_pin() {
        let src = include_str!("router_stream.rs");
        let f = &src[src.find("pub(crate) async fn stream_chat").unwrap()..];
        let f = &f[..f.find("let mut out = StreamOutcome::default()").unwrap()];
        assert!(f.contains(r#".header("X-Amara-Allow-Claude", "0")"#));
        assert!(f.contains(r#".header("X-Amara-Mode", "local")"#));
        assert!(f.contains(r#".header("Accept", "text/event-stream")"#));
        assert!(!f.contains("X-Amara-Allow-Claude\", \"1\""));
        // The navy/outbound check sits after the request is built and before the send.
        let chk = f.find("if blocked()").unwrap();
        assert!(chk > f.find(".json(&stream_body").unwrap());
        assert!(chk < f.find("req.send()").unwrap());
        // Built only from the loopback base; no http literal other than the format string.
        assert!(f.contains("loopback_base(base)"));
        assert!(f.contains("loopback_client("));
    }

    #[test]
    fn tier_words_never_echo_model_names() {
        assert_eq!(normalize_tier("light"), Ok(Some("fast")));
        assert_eq!(normalize_tier("fast"), Ok(Some("fast")));
        assert_eq!(normalize_tier("deep"), Ok(Some("deep")));
        assert_eq!(normalize_tier("gemma4:latest"), Ok(None));
        assert_eq!(normalize_tier("claude"), Err(StreamError::ClaudeTier));
    }

    #[test]
    fn tag_holdback_never_leaks_the_tag() {
        let mut h = TagHoldback::default();
        let mut shown = String::new();
        for p in [
            "Sure. <nex",
            "us_act",
            "ion>{\"k\":1}</nexus_action>",
            " after",
        ] {
            shown.push_str(&h.push(p));
        }
        shown.push_str(&h.finish());
        assert_eq!(shown, "Sure. ");
        assert!(!shown.contains('<'));
    }

    #[test]
    fn tag_holdback_releases_lone_angle_bracket() {
        let mut h = TagHoldback::default();
        let mut shown = h.push("1 <");
        assert_eq!(shown, "1 ");
        shown.push_str(&h.push(" 2 and <b> ok <"));
        shown.push_str(&h.finish());
        assert_eq!(shown, "1 < 2 and <b> ok <");
    }
}
