//! Live bridge from Amara Voice (~/projects/amara-voice) to the webview.
//!
//! - `amara-voice://state`: derived from what amara-voice already writes (`logs/state`, one word,
//!   via `status_icon.StatusIcon.set`) plus the same process check the Voice panel uses
//!   (`voice::voice_status`). Emitted on change, with a light heartbeat.
//! - `amara-voice://level`: consumed from an optional loopback-only websocket feed that amara-voice
//!   can publish (`ws://127.0.0.1:<port>/level`, ~30 Hz). Nothing listening → no events, no logs.
//!
//! Both run as one background pair, started/stopped with `voice_bridge_start` / `voice_bridge_stop`
//! (idempotent, same pattern as `ha_live_start` / `ha_live_stop`).

use super::shared::time::now_ms;
use futures::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

pub const STATE_EVENT: &str = "amara-voice://state";
pub const LEVEL_EVENT: &str = "amara-voice://level";

/// How often logs/state is read (cheap: one small file).
const STATE_POLL: Duration = Duration::from_millis(150);
/// How often the process check (`ps`) runs.
const PROC_POLL: Duration = Duration::from_secs(2);
/// Re-emit an unchanged state this often so a late-mounting UI catches up.
const HEARTBEAT: Duration = Duration::from_secs(5);

/// Default level-feed port; override with `NEXUS_AMARA_VOICE_LEVEL_PORT`. Host is always 127.0.0.1.
const DEFAULT_LEVEL_PORT: u16 = 8791;
const LEVEL_MAX_MESSAGE: usize = 4 * 1024;
const LEVEL_MAX_FRAME: usize = 4 * 1024;
const LEVEL_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// No frame for this long → treat the feed as gone and reconnect.
const LEVEL_IDLE: Duration = Duration::from_secs(5);
/// ≤30 Hz re-emit.
const LEVEL_MIN_GAP: Duration = Duration::from_millis(33);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_CAP: Duration = Duration::from_secs(30);
const MAX_UTTERANCE_ID: usize = 64;
/// Subprotocol prefix carrying the per-launch token (`amara-launch.<64 hex>`).
const LEVEL_TOKEN_SUBPROTOCOL: &str = "amara-launch.";
/// DEFAULT OFF. tungstenite fails the client handshake (`NoSubProtocol`) when the server does not
/// echo an offered `Sec-WebSocket-Protocol`, and today's unpatched amara-voice does not. Flip to
/// true only after Taisun applies `tasks/proposed/amara-voice-token.diff` (server echoes it).
/// Retrying without the header on failure is deliberately NOT done: it would hand the token to
/// whatever process squats on the port.
const LEVEL_SEND_LAUNCH_TOKEN: bool = false;

// ─── payloads (camelCase) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VoiceState {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceReason {
    Unavailable,
    /// Part of the payload contract; amara-voice's `logs/state` can't tell a TTS failure from a
    /// Hermes one yet, so nothing sets it today.
    #[allow(dead_code)]
    TtsFailed,
    ModelLoading,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStateEvent {
    /// ms since the Unix epoch (same clock as JS `Date.now()`).
    pub ts: u64,
    pub state: VoiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utterance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<VoiceReason>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceLevelEvent {
    /// ms since the Unix epoch, stamped on receipt.
    pub ts: u64,
    /// 0..1
    pub rms: f64,
    /// 0..1
    pub peak: f64,
    pub state: VoiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utterance_id: Option<String>,
}

// ─── state: mapping + change detection (pure, tested) ───────────────────────

/// Map the Voice panel's normalized word (see `voice::normalize_state`) to the event state.
/// Down / stopped / not installed → `error` + `unavailable` (see module docs for why not `idle`).
fn map_state(word: &str) -> (VoiceState, Option<VoiceReason>) {
    match word {
        "idle" => (VoiceState::Idle, None),
        "listening" | "heard" => (VoiceState::Listening, None),
        "thinking" => (VoiceState::Thinking, None),
        "speaking" => (VoiceState::Speaking, None),
        "loading" => (VoiceState::Idle, Some(VoiceReason::ModelLoading)),
        // amara-voice writes "error" when a turn failed (Hermes or the voice); it doesn't say which.
        "error" => (VoiceState::Error, None),
        _ => (VoiceState::Error, Some(VoiceReason::Unavailable)),
    }
}

/// Emits on change, and re-emits an unchanged state every `heartbeat`.
#[derive(Debug)]
struct StateTracker {
    last: Option<(VoiceState, Option<VoiceReason>)>,
    last_emit: Option<std::time::Instant>,
    heartbeat: Duration,
}

impl StateTracker {
    fn new(heartbeat: Duration) -> Self {
        Self {
            last: None,
            last_emit: None,
            heartbeat,
        }
    }

    fn observe(
        &mut self,
        now: std::time::Instant,
        mapped: (VoiceState, Option<VoiceReason>),
    ) -> Option<VoiceStateEvent> {
        let changed = self.last != Some(mapped);
        let due = self
            .last_emit
            .is_none_or(|t| now.duration_since(t) >= self.heartbeat);
        if !(changed || due) {
            return None;
        }
        self.last = Some(mapped);
        self.last_emit = Some(now);
        Some(VoiceStateEvent {
            ts: now_ms(),
            state: mapped.0,
            utterance_id: None,
            reason: mapped.1,
        })
    }
}

async fn state_loop(app: AppHandle) {
    let mut tracker = StateTracker::new(HEARTBEAT);
    let mut installed = false;
    let mut running = false;
    let mut last_proc: Option<std::time::Instant> = None;
    loop {
        let now = std::time::Instant::now();
        if last_proc.is_none_or(|t| now.duration_since(t) >= PROC_POLL) {
            // Same check as the Voice panel (`voice_status`): installed + amara_voice.py process.
            if let Ok(s) = super::voice::voice_status().await {
                installed = s.installed;
                running = s.running;
            }
            last_proc = Some(now);
        }
        let paths = super::voice::voice_paths();
        let word = if installed {
            let raw = tokio::fs::read_to_string(paths.dir.join("logs").join("state"))
                .await
                .ok();
            super::voice::normalize_state(raw.as_deref(), running)
        } else {
            "not_installed".to_string()
        };
        if let Some(ev) = tracker.observe(now, map_state(&word)) {
            let _ = app.emit(STATE_EVENT, ev);
        }
        tokio::time::sleep(STATE_POLL).await;
    }
}

// ─── level feed: validation (pure, tested) ──────────────────────────────────

fn clamp01(v: &Value) -> Option<f64> {
    let x = v.as_f64()?;
    x.is_finite().then(|| x.clamp(0.0, 1.0))
}

fn valid_utterance_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_UTTERANCE_ID
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parse one feed frame `{ts, rms, peak, state, utteranceId?}`. Malformed → None (dropped).
/// rms/peak are clamped to 0..1; the publisher's `ts` must be a number but we restamp on receipt.
fn parse_level(text: &str) -> Option<VoiceLevelEvent> {
    let v: Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    obj.get("ts")?.as_f64().filter(|t| t.is_finite())?;
    let rms = clamp01(obj.get("rms")?)?;
    let peak = clamp01(obj.get("peak")?)?;
    let state = match obj.get("state")?.as_str()? {
        "idle" => VoiceState::Idle,
        "listening" => VoiceState::Listening,
        "thinking" => VoiceState::Thinking,
        "speaking" => VoiceState::Speaking,
        "error" => VoiceState::Error,
        _ => return None,
    };
    let utterance_id = match obj.get("utteranceId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if valid_utterance_id(s) => Some(s.clone()),
        Some(_) => return None,
    };
    Some(VoiceLevelEvent {
        ts: now_ms(),
        rms,
        peak: peak.max(rms),
        state,
        utterance_id,
    })
}

fn level_port() -> u16 {
    std::env::var("NEXUS_AMARA_VOICE_LEVEL_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|p| *p >= 1024)
        .unwrap_or(DEFAULT_LEVEL_PORT)
}

/// Only ever 127.0.0.1; the port is the one configurable part.
fn level_url(port: u16) -> String {
    format!("ws://127.0.0.1:{port}/level")
}

/// The handshake request for the level feed. The token (when given and the URL is loopback) rides
/// in `Sec-WebSocket-Protocol`: unlike a `?t=` query it never lands in URLs, access logs, error
/// strings or `Debug` output of the URL, and browsers cannot set it without the server echoing it.
/// The value is never logged.
fn level_request(
    url: &str,
    token: Option<&str>,
) -> Result<Request, tokio_tungstenite::tungstenite::Error> {
    let mut req = url.into_client_request()?;
    if let Some(t) = token.filter(|_| super::loopback_http::is_loopback_url(url)) {
        let v = format!("{LEVEL_TOKEN_SUBPROTOCOL}{t}");
        if let Ok(hv) = v.parse() {
            req.headers_mut().insert("Sec-WebSocket-Protocol", hv);
        }
    }
    Ok(req)
}

/// The token to offer, or None (gate off, or no/invalid token file).
fn level_token() -> Option<String> {
    LEVEL_SEND_LAUNCH_TOKEN
        .then(super::launch_token::current)
        .flatten()
}

fn level_ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(LEVEL_MAX_MESSAGE))
        .max_frame_size(Some(LEVEL_MAX_FRAME))
}

#[derive(Debug, Clone, Copy)]
struct LevelTimings {
    connect: Duration,
    idle: Duration,
    min_gap: Duration,
    backoff_start: Duration,
    backoff_cap: Duration,
}

impl Default for LevelTimings {
    fn default() -> Self {
        Self {
            connect: LEVEL_CONNECT_TIMEOUT,
            idle: LEVEL_IDLE,
            min_gap: LEVEL_MIN_GAP,
            backoff_start: BACKOFF_START,
            backoff_cap: BACKOFF_CAP,
        }
    }
}

/// One connection: forward valid frames, throttled to one per `min_gap`. Returns whether any frame
/// was received (used to reset the backoff).
async fn level_session<S>(
    mut ws: S,
    t: LevelTimings,
    emit: &mut impl FnMut(VoiceLevelEvent),
) -> bool
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let mut got_any = false;
    let mut last_emit: Option<tokio::time::Instant> = None;
    loop {
        let frame = match tokio::time::timeout(t.idle, ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => return got_any, // idle, closed, or error (incl. over-size): reconnect
        };
        let Message::Text(text) = frame else {
            continue;
        };
        let Some(ev) = parse_level(text.as_str()) else {
            continue;
        };
        got_any = true;
        let now = tokio::time::Instant::now();
        if last_emit.is_some_and(|t0| now.duration_since(t0) < t.min_gap) {
            continue;
        }
        last_emit = Some(now);
        emit(ev);
    }
}

/// Connect on loopback, consume, reconnect with backoff. Silent when nothing is listening.
async fn level_loop(port: u16, t: LevelTimings, mut emit: impl FnMut(VoiceLevelEvent)) {
    let url = level_url(port);
    let mut backoff = t.backoff_start;
    loop {
        let conn = match level_request(&url, level_token().as_deref()) {
            Ok(req) => Some(
                tokio::time::timeout(
                    t.connect,
                    tokio_tungstenite::connect_async_with_config(
                        req,
                        Some(level_ws_config()),
                        false,
                    ),
                )
                .await,
            ),
            Err(_) => None,
        };
        if let Some(Ok(Ok((ws, _)))) = conn {
            if level_session(ws, t, &mut emit).await {
                backoff = t.backoff_start;
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(t.backoff_cap);
    }
}

// ─── commands ───────────────────────────────────────────────────────────────

static TASKS: StdMutex<Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>> =
    StdMutex::new(None);

/// Start the state + level bridge. Idempotent: false if already running.
#[tauri::command]
#[specta::specta]
pub async fn voice_bridge_start(app: AppHandle) -> bool {
    let mut slot = TASKS.lock().unwrap_or_else(|p| p.into_inner());
    if slot
        .as_ref()
        .is_some_and(|(a, b)| !a.is_finished() || !b.is_finished())
    {
        return false;
    }
    let state = tokio::spawn(state_loop(app.clone()));
    let level = tokio::spawn(level_loop(
        level_port(),
        LevelTimings::default(),
        move |ev| {
            let _ = app.emit(LEVEL_EVENT, ev);
        },
    ));
    *slot = Some((state, level));
    true
}

/// Stop the bridge. False if it wasn't running.
#[tauri::command]
#[specta::specta]
pub async fn voice_bridge_stop() -> bool {
    match TASKS.lock().unwrap_or_else(|p| p.into_inner()).take() {
        Some((a, b)) => {
            a.abort();
            b.abort();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::SinkExt;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn state_mapping_and_unavailable() {
        assert_eq!(map_state("idle"), (VoiceState::Idle, None));
        assert_eq!(map_state("heard"), (VoiceState::Listening, None));
        assert_eq!(map_state("listening"), (VoiceState::Listening, None));
        assert_eq!(map_state("thinking"), (VoiceState::Thinking, None));
        assert_eq!(map_state("speaking"), (VoiceState::Speaking, None));
        assert_eq!(
            map_state("loading"),
            (VoiceState::Idle, Some(VoiceReason::ModelLoading))
        );
        assert_eq!(map_state("error"), (VoiceState::Error, None));
        for down in ["stopped", "not_installed", "", "<script>"] {
            assert_eq!(
                map_state(down),
                (VoiceState::Error, Some(VoiceReason::Unavailable))
            );
        }
        // Dead process with a stale "speaking" file is unavailable (normalize_state handles it).
        let w = super::super::voice::normalize_state(Some("speaking"), false);
        assert_eq!(
            map_state(&w),
            (VoiceState::Error, Some(VoiceReason::Unavailable))
        );
    }

    #[test]
    fn state_payload_shape_is_camel_case() {
        let ev = VoiceStateEvent {
            ts: 1,
            state: VoiceState::Error,
            utterance_id: Some("u1".into()),
            reason: Some(VoiceReason::TtsFailed),
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            json!({"ts": 1, "state": "error", "utteranceId": "u1", "reason": "tts_failed"})
        );
        let ev = VoiceStateEvent {
            ts: 2,
            state: VoiceState::Idle,
            utterance_id: None,
            reason: Some(VoiceReason::ModelLoading),
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            json!({"ts": 2, "state": "idle", "reason": "model_loading"})
        );
        assert!(now_ms() > 1_700_000_000_000);
    }

    #[test]
    fn tracker_emits_on_change_and_heartbeat_only() {
        let mut t = StateTracker::new(Duration::from_secs(5));
        let t0 = std::time::Instant::now();
        let idle = map_state("idle");
        assert!(t.observe(t0, idle).is_some());
        assert!(t.observe(t0 + Duration::from_millis(150), idle).is_none());
        let s = t
            .observe(t0 + Duration::from_millis(300), map_state("speaking"))
            .unwrap();
        assert_eq!(s.state, VoiceState::Speaking);
        assert!(t
            .observe(t0 + Duration::from_secs(4), map_state("speaking"))
            .is_none());
        assert!(t
            .observe(t0 + Duration::from_millis(5300), map_state("speaking"))
            .is_some());
    }

    #[test]
    fn level_frames_validated_and_clamped() {
        let ok = parse_level(
            r#"{"ts":1730700000000,"rms":0.25,"peak":0.5,"state":"speaking","utteranceId":"a1-b_2"}"#,
        )
        .unwrap();
        assert_eq!(
            (ok.rms, ok.peak, ok.state),
            (0.25, 0.5, VoiceState::Speaking)
        );
        assert_eq!(ok.utterance_id.as_deref(), Some("a1-b_2"));
        let c = parse_level(r#"{"ts":1,"rms":7,"peak":-3,"state":"idle"}"#).unwrap();
        assert_eq!((c.rms, c.peak), (1.0, 1.0)); // peak never below rms
        let c =
            parse_level(r#"{"ts":1,"rms":-1,"peak":2,"state":"idle","utteranceId":null}"#).unwrap();
        assert_eq!((c.rms, c.peak, c.utterance_id), (0.0, 1.0, None));
        for bad in [
            "",
            "not json",
            "[]",
            r#"{"rms":0.1,"peak":0.1,"state":"idle"}"#,
            r#"{"ts":"x","rms":0.1,"peak":0.1,"state":"idle"}"#,
            r#"{"ts":1,"rms":"0.1","peak":0.1,"state":"idle"}"#,
            r#"{"ts":1,"rms":0.1,"peak":0.1,"state":"shouting"}"#,
            r#"{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","utteranceId":"has space"}"#,
            r#"{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","utteranceId":5}"#,
            r#"{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","utteranceId":""}"#,
        ] {
            assert!(parse_level(bad).is_none(), "{bad}");
        }
        let long = format!(
            r#"{{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","utteranceId":"{}"}}"#,
            "a".repeat(65)
        );
        assert!(parse_level(&long).is_none());
        let max = format!(
            r#"{{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","utteranceId":"{}"}}"#,
            "a".repeat(64)
        );
        assert!(parse_level(&max).is_some());
        let ev = serde_json::to_value(&ok).unwrap();
        assert!(ev.get("utteranceId").is_some() && ev.get("ts").is_some());
    }

    #[test]
    fn url_is_loopback_only_and_limits_set() {
        assert_eq!(level_url(8791), "ws://127.0.0.1:8791/level");
        let c = level_ws_config();
        assert_eq!(c.max_message_size, Some(4096));
        assert_eq!(c.max_frame_size, Some(4096));
        assert_eq!(LEVEL_MIN_GAP, Duration::from_millis(33));
    }

    fn fast() -> LevelTimings {
        LevelTimings {
            connect: Duration::from_millis(500),
            idle: Duration::from_millis(500),
            min_gap: Duration::from_millis(33),
            backoff_start: Duration::from_millis(50),
            backoff_cap: Duration::from_millis(200),
        }
    }

    /// Mock publisher: each accepted connection gets `frames`, then closes.
    async fn publisher(
        frames: Vec<String>,
        conns: usize,
        gap: Duration,
    ) -> (u16, Arc<Mutex<usize>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(Mutex::new(0));
        let a2 = accepted.clone();
        tokio::spawn(async move {
            for _ in 0..conns {
                let Ok((sock, _)) = listener.accept().await else {
                    return;
                };
                *a2.lock().unwrap() += 1;
                let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                for f in &frames {
                    if ws.send(Message::Text(f.clone().into())).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(gap).await;
                }
                let _ = ws.close(None).await;
            }
        });
        (port, accepted)
    }

    fn collect() -> (
        Arc<Mutex<Vec<VoiceLevelEvent>>>,
        impl FnMut(VoiceLevelEvent) + Send,
    ) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let g2 = got.clone();
        (got, move |e| g2.lock().unwrap().push(e))
    }

    #[tokio::test]
    async fn valid_frames_emitted_malformed_dropped_and_clamped() {
        let frames = vec![
            r#"{"ts":1,"rms":0.2,"peak":0.4,"state":"listening","utteranceId":"u1"}"#.into(),
            "garbage".into(),
            r#"{"ts":1,"rms":0.2,"peak":0.4,"state":"bogus"}"#.into(),
            r#"{"ts":1,"rms":9,"peak":9,"state":"speaking"}"#.into(),
        ];
        let (port, _) = publisher(frames, 1, Duration::from_millis(50)).await;
        let (got, emit) = collect();
        let task = tokio::spawn(level_loop(port, fast(), emit));
        tokio::time::sleep(Duration::from_millis(600)).await;
        task.abort();
        let got = got.lock().unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].utterance_id.as_deref(), Some("u1"));
        assert_eq!((got[1].rms, got[1].peak), (1.0, 1.0));
    }

    #[tokio::test]
    async fn re_emits_are_throttled_to_30hz() {
        let f: String = r#"{"ts":1,"rms":0.1,"peak":0.1,"state":"idle"}"#.into();
        // 100 frames back to back (~no gap) → far fewer than 100 emitted.
        let (port, _) = publisher(vec![f; 100], 1, Duration::from_millis(1)).await;
        let (got, emit) = collect();
        let t0 = std::time::Instant::now();
        let task = tokio::spawn(level_loop(port, fast(), emit));
        tokio::time::sleep(Duration::from_millis(400)).await;
        task.abort();
        let n = got.lock().unwrap().len();
        let elapsed_ms = t0.elapsed().as_millis() as usize;
        assert!(n >= 1 && n <= elapsed_ms / 33 + 1, "{n} in {elapsed_ms} ms");
    }

    #[tokio::test]
    async fn no_publisher_stays_quiet() {
        // Bind then drop to get a port with nothing listening.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let (got, emit) = collect();
        let task = tokio::spawn(level_loop(port, fast(), emit));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!task.is_finished(), "keeps retrying quietly");
        task.abort();
        assert!(got.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reconnects_after_the_publisher_drops() {
        let f: String = r#"{"ts":1,"rms":0.3,"peak":0.3,"state":"speaking"}"#.into();
        let (port, accepted) = publisher(vec![f], 3, Duration::from_millis(10)).await;
        let (got, emit) = collect();
        let task = tokio::spawn(level_loop(port, fast(), emit));
        tokio::time::sleep(Duration::from_millis(800)).await;
        task.abort();
        assert!(*accepted.lock().unwrap() >= 2);
        assert!(got.lock().unwrap().len() >= 2);
    }

    #[tokio::test]
    async fn oversized_and_silent_feeds_end_the_session() {
        // Oversized message (> 4 KiB) → session ends without emitting.
        let big = format!(
            r#"{{"ts":1,"rms":0.1,"peak":0.1,"state":"idle","pad":"{}"}}"#,
            "x".repeat(8192)
        );
        let (port, _) = publisher(vec![big], 1, Duration::from_millis(10)).await;
        let (ws, _) = tokio_tungstenite::connect_async_with_config(
            level_url(port),
            Some(level_ws_config()),
            false,
        )
        .await
        .unwrap();
        let (got, mut emit) = collect();
        assert!(!level_session(ws, fast(), &mut emit).await);
        assert!(got.lock().unwrap().is_empty());
        // Silent feed → idle timeout ends the session.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let _ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        let (ws, _) = tokio_tungstenite::connect_async(level_url(port))
            .await
            .unwrap();
        let t0 = std::time::Instant::now();
        assert!(!level_session(ws, fast(), &mut emit).await);
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    const FIXTURE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn proto(r: &Request) -> Option<String> {
        r.headers()
            .get("Sec-WebSocket-Protocol")
            .map(|v| v.to_str().unwrap().to_string())
    }

    #[test]
    fn request_carries_token_as_subprotocol_not_in_url() {
        let url = level_url(8791);
        let r = level_request(&url, Some(FIXTURE)).unwrap();
        assert_eq!(proto(&r), Some(format!("amara-launch.{FIXTURE}")));
        assert!(!r.uri().to_string().contains(FIXTURE));
    }

    #[test]
    fn request_without_token_or_off_loopback_has_no_header() {
        assert_eq!(proto(&level_request(&level_url(8791), None).unwrap()), None);
        let remote = level_request("ws://example.com:8791/level", Some(FIXTURE)).unwrap();
        assert_eq!(proto(&remote), None);
        let lan = level_request("ws://192.168.1.5:8791/level", Some(FIXTURE)).unwrap();
        assert_eq!(proto(&lan), None);
    }

    #[test]
    fn missing_token_file_yields_no_token() {
        let d = std::env::temp_dir().join(format!("vb-tok-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(super::super::launch_token::read_in(&d), None);
        let t = super::super::launch_token::mint_in(&d).unwrap();
        assert_eq!(super::super::launch_token::read_in(&d), Some(t));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn token_gate_is_off_by_default() {
        assert!(!std::hint::black_box(LEVEL_SEND_LAUNCH_TOKEN));
        assert_eq!(level_token(), None);
    }

    /// Why the gate exists: an unpatched server that ignores the subprotocol fails the handshake.
    #[tokio::test]
    async fn server_not_echoing_subprotocol_fails_handshake_so_gate_stays_off() {
        let (port, _) = publisher(vec![], 1, Duration::from_millis(1)).await;
        let req = level_request(&level_url(port), Some(FIXTURE)).unwrap();
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
    }

    /// A patched server echoes the offered subprotocol and the feed works with it.
    #[tokio::test]
    async fn server_echoing_subprotocol_connects() {
        use tokio_tungstenite::tungstenite::handshake::server::{Request as SReq, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(None::<String>));
        let s2 = seen.clone();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let cb = |req: &SReq, mut resp: Response| {
                let v = req.headers().get("Sec-WebSocket-Protocol").cloned();
                *s2.lock().unwrap() = v.as_ref().map(|h| h.to_str().unwrap().to_string());
                if let Some(v) = v {
                    resp.headers_mut().insert("Sec-WebSocket-Protocol", v);
                }
                Ok(resp)
            };
            let _ws = tokio_tungstenite::accept_hdr_async(sock, cb).await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
        });
        let req = level_request(&level_url(port), Some(FIXTURE)).unwrap();
        assert!(tokio_tungstenite::connect_async(req).await.is_ok());
        assert_eq!(
            seen.lock().unwrap().clone(),
            Some(format!("amara-launch.{FIXTURE}"))
        );
    }
}
