//! Telegram relay (HANDOFF-1111s step 6, approved in decision 0014 #3).
//!
//! Settings are only `{ enabled, kinds }` (no quiet hours, no rate limit: Taisun's choice). Token and chat id
//! live in the Keychain (`com.taisun.nexus.telegram_*`, written through `secret_set`); the UI only ever sees
//! set / not set. Nothing here logs a token, a chat id, message text or a health value.
//!
//! Guard order for every send: outbound kill switch / navy first, then (test only) the native approval
//! dialog, then the Keychain read, then the single POST to `api.telegram.org`. Message text is written by
//! the LOCAL light tier in Amara's words; every number in it must match the source row, else a plain
//! factual line is sent instead.
//!
//! Event intake: only what the app can see today. The NOOP snapshot gives sleep and recovery. The other
//! kinds NOOP.app raises itself (effort, strap battery, sync problems, Lucid cues, illness signs) are
//! listed as "not connected yet" until Phase 10 lands; nothing is faked for them.

use super::keychain::{service_for, Keychain, KeychainError, MacKeychain};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// The only host this module ever contacts off the Mac.
pub(crate) const TELEGRAM_HOST: &str = "api.telegram.org";
const TOKEN_KEY: &str = "telegram_bot_token";
const CHAT_KEY: &str = "telegram_chat_id";
const MAX_MESSAGE_CHARS: usize = 600;
const TEST_TEXT: &str =
    "Test from Amara OS. If you can read this, the Telegram relay is connected.";

pub(crate) const KINDS: [&str; 7] = [
    "sleep", "recovery", "effort", "battery", "sync", "lucid", "illness",
];
/// Kinds the app can build an event for today.
pub(crate) const CONNECTED: [&str; 2] = ["sleep", "recovery"];

const OFF_MSG: &str =
    "Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED or local-only mode).";
const KEYS_MSG: &str = "Telegram bot token and chat are not both set.";
const SEND_FAILED: &str = "Telegram API returned an error. Check your bot token and chat ID.";

// ---------------------------------------------------------------- settings

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RelaySettings {
    pub enabled: bool,
    pub kinds: BTreeMap<String, bool>,
}

impl Default for RelaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kinds: KINDS
                .iter()
                .map(|k| ((*k).to_string(), *k != "lucid"))
                .collect(),
        }
    }
}

impl RelaySettings {
    /// Every known kind present, unknown kinds dropped.
    fn normalized(mut self) -> Self {
        let d = Self::default();
        self.kinds.retain(|k, _| KINDS.contains(&k.as_str()));
        for (k, v) in d.kinds {
            self.kinds.entry(k).or_insert(v);
        }
        self
    }
}

fn dir() -> PathBuf {
    crate::app_paths::data_dir()
}
fn settings_path(dir: &Path) -> PathBuf {
    dir.join("notify_relay.json")
}
fn ledger_path(dir: &Path) -> PathBuf {
    dir.join("notify_relay_sent.json")
}

fn write_private(path: &Path, json: &str) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    crate::commands::continuity::atomic_write_with_mode(path, json.as_bytes(), Some(0o600))
        .map_err(|e| e.to_string())
}

pub(crate) fn load_settings_in(dir: &Path) -> RelaySettings {
    std::fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<RelaySettings>(&s).ok())
        .unwrap_or_default()
        .normalized()
}

pub(crate) fn save_settings_in(dir: &Path, s: &RelaySettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    write_private(&settings_path(dir), &json)
}

/// Apply a patch. Unknown kinds are refused; switching on needs both keys set.
pub(crate) fn apply_patch(
    mut s: RelaySettings,
    enabled: Option<bool>,
    kinds: Option<BTreeMap<String, bool>>,
    keys_set: bool,
) -> Result<RelaySettings, String> {
    if let Some(k) = &kinds {
        if let Some(bad) = k.keys().find(|k| !KINDS.contains(&k.as_str())) {
            return Err(format!(
                "unknown alert kind: {}",
                bad.chars().take(24).collect::<String>()
            ));
        }
    }
    if enabled == Some(true) && !keys_set {
        return Err(KEYS_MSG.to_string());
    }
    if let Some(e) = enabled {
        s.enabled = e;
    }
    if let Some(k) = kinds {
        s.kinds.extend(k);
    }
    Ok(s.normalized())
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelayView {
    pub enabled: bool,
    pub kinds: BTreeMap<String, bool>,
    pub last_sent: Option<String>,
    /// Kinds the app can build an event for. The rest are "not connected yet".
    pub connected: Vec<String>,
    pub not_connected: Vec<String>,
}

fn view_of(s: RelaySettings, last_sent: Option<String>) -> RelayView {
    RelayView {
        enabled: s.enabled,
        kinds: s.kinds,
        last_sent,
        connected: CONNECTED.iter().map(|k| (*k).to_string()).collect(),
        not_connected: KINDS
            .iter()
            .filter(|k| !CONNECTED.contains(k))
            .map(|k| (*k).to_string())
            .collect(),
    }
}

// ---------------------------------------------------------------- sent ledger (dedupe + lastSent)

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
struct Ledger {
    /// kind -> last day sent
    days: BTreeMap<String, String>,
    last_sent_ms: Option<i64>,
}

fn load_ledger(dir: &Path) -> Ledger {
    std::fs::read_to_string(ledger_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn last_sent_label(ms: Option<i64>) -> Option<String> {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms?)
        .single()
        .map(|t| t.format("%-I:%M %p").to_string())
}

// ---------------------------------------------------------------- events + number check

/// One alert built from a source row. `allowed` is every number the message may contain.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelayEvent {
    pub kind: &'static str,
    pub day: String,
    /// What the model is shown.
    pub facts: String,
    /// The plain factual fallback line.
    pub plain: String,
    pub allowed: Vec<f64>,
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn push_minutes(allowed: &mut Vec<f64>, min: f64) {
    let total = min.round();
    allowed.extend([
        total,
        r1(min),
        (total / 60.0).floor(),
        total % 60.0,
        r1(min / 60.0),
    ]);
}

fn hm(min: f64) -> String {
    let t = min.round() as i64;
    format!("{}h {}m", t / 60, t % 60)
}

fn row_for<'a>(
    snap: &'a super::noop::NoopHealthSnapshot,
) -> Option<&'a super::noop::NoopDailyMetric> {
    let day = snap.reading_day.as_deref()?;
    snap.today
        .iter()
        .chain(snap.recent_days.iter())
        .find(|r| r.day == day)
}

/// Sleep and recovery events from the NOOP snapshot's reading-day row. Values missing from the row
/// produce no event (never a guess).
pub(crate) fn events_from_snapshot(snap: &super::noop::NoopHealthSnapshot) -> Vec<RelayEvent> {
    let Some(row) = row_for(snap) else {
        return vec![];
    };
    let mut out = vec![];
    if let Some(min) = row.total_sleep_min.filter(|m| m.is_finite() && *m > 0.0) {
        let mut allowed = vec![];
        push_minutes(&mut allowed, min);
        let mut facts = format!(
            "Sleep last night: {} total ({} minutes).",
            hm(min),
            min.round() as i64
        );
        for (label, v) in [("Deep sleep", row.deep_min), ("REM sleep", row.rem_min)] {
            if let Some(v) = v.filter(|v| v.is_finite() && *v >= 0.0) {
                allowed.extend([v.round(), r1(v)]);
                facts.push_str(&format!(" {label}: {} minutes.", v.round() as i64));
            }
        }
        out.push(RelayEvent {
            kind: "sleep",
            day: row.day.clone(),
            facts,
            plain: format!("Sleep: {} last night.", hm(min)),
            allowed,
        });
    }
    if let Some(rec) = row.recovery.filter(|r| r.is_finite() && *r >= 0.0) {
        let mut allowed = vec![rec.round(), r1(rec)];
        let mut facts = format!("Recovery: {}%.", rec.round() as i64);
        if let Some(h) = row.avg_hrv.filter(|v| v.is_finite() && *v > 0.0) {
            allowed.extend([h.round(), r1(h)]);
            facts.push_str(&format!(" HRV: {} ms.", h.round() as i64));
        }
        if let Some(h) = row.resting_hr.filter(|v| *v > 0) {
            allowed.push(h as f64);
            facts.push_str(&format!(" Resting heart rate: {h} bpm."));
        }
        out.push(RelayEvent {
            kind: "recovery",
            day: row.day.clone(),
            facts,
            plain: format!("Recovery: {}%.", rec.round() as i64),
            allowed,
        });
    }
    out
}

/// Every number (digits, optional thousands commas and one decimal point) in `text`.
pub(crate) fn numbers_in(text: &str) -> Vec<f64> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut s = String::new();
            let mut dot = false;
            while i < chars.len() {
                let c = chars[i];
                let next_digit = chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
                if c.is_ascii_digit() {
                    s.push(c);
                } else if c == '.' && !dot && next_digit {
                    dot = true;
                    s.push(c);
                } else if c == ',' && next_digit {
                    // thousands separator
                } else {
                    break;
                }
                i += 1;
            }
            if let Ok(v) = s.parse::<f64>() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// True when every number in `text` equals one of `allowed`.
pub(crate) fn numbers_match(text: &str, allowed: &[f64]) -> bool {
    numbers_in(text)
        .iter()
        .all(|n| allowed.iter().any(|a| (a - n).abs() < 1e-9))
}

// ---------------------------------------------------------------- ports (real impls below; tests use mocks)

pub(crate) trait LocalWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String>;
}

pub(crate) trait TgTransport {
    async fn post(&self, url: &str, chat_id: &str, text: &str) -> Result<(), String>;
}

type Log<'a> = &'a (dyn Fn(&str) + Sync);

struct RouterWriter;
impl LocalWriter for RouterWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String> {
        super::llm_router::local_complete(instruction, facts).await
    }
}

struct HttpTelegram;
impl TgTransport for HttpTelegram {
    async fn post(&self, url: &str, chat_id: &str, text: &str) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| "Network error contacting Telegram.".to_string())?;
        // The URL carries the token: never format a reqwest error (it can include the URL).
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "chat_id": chat_id, "text": text }))
            .send()
            .await
            .map_err(|_| "Network error contacting Telegram.".to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(SEND_FAILED.to_string())
        }
    }
}

const INSTRUCTION: &str = "Write one short Telegram message to Taisun about the health facts below, in your own words. Use only the numbers given, exactly as written; add no other numbers, dates or times. No emojis, no markdown, no greeting list. Two sentences at most.";

pub(crate) struct Composed {
    pub text: String,
    pub fell_back: bool,
}

/// Amara's words from the local tier, or the plain line when the writer fails or any number differs
/// from the source row. The log line carries the kind and a reason code only.
pub(crate) async fn compose<W: LocalWriter>(w: &W, ev: &RelayEvent, log: Log<'_>) -> Composed {
    let fall = |reason: &str| {
        log(&format!(
            "[notify] {}: {reason}; using the plain line",
            ev.kind
        ));
        Composed {
            text: ev.plain.clone(),
            fell_back: true,
        }
    };
    let text = match w.write(INSTRUCTION, &ev.facts).await {
        Ok(t) => t.trim().to_string(),
        Err(_) => return fall("local writer unavailable"),
    };
    if text.is_empty() || text.chars().count() > MAX_MESSAGE_CHARS {
        return fall("unusable reply");
    }
    if !numbers_match(&text, &ev.allowed) {
        return fall("number check failed");
    }
    Composed {
        text,
        fell_back: false,
    }
}

// ---------------------------------------------------------------- keys + send

fn read_secret(kc: &dyn Keychain, name: &str) -> Result<Zeroizing<String>, String> {
    match kc.get(&service_for(name)) {
        Ok(v) => String::from_utf8(v)
            .map(Zeroizing::new)
            .map_err(|_| KEYS_MSG.to_string()),
        Err(KeychainError::NotFound) => Err(KEYS_MSG.to_string()),
        Err(KeychainError::Other(_)) => Err("Keychain is unavailable.".to_string()),
    }
}

fn send_url(token: &str) -> Result<String, String> {
    if token.is_empty()
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
    {
        return Err("The bot token is not valid.".to_string());
    }
    let url = format!("https://{TELEGRAM_HOST}/bot{token}/sendMessage");
    match reqwest::Url::parse(&url) {
        Ok(u) if u.host_str() == Some(TELEGRAM_HOST) => Ok(url),
        _ => Err("The bot token is not valid.".to_string()),
    }
}

async fn send_text<T: TgTransport>(t: &T, kc: &dyn Keychain, text: &str) -> Result<(), String> {
    let token = read_secret(kc, TOKEN_KEY)?;
    let chat = read_secret(kc, CHAT_KEY)?;
    let url = Zeroizing::new(send_url(token.trim())?);
    t.post(&url, chat.trim(), text).await
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Dispatch {
    Sent,
    Skipped(&'static str),
}

/// Send one event. Kill switch first, then the settings, then the Keychain.
pub(crate) async fn dispatch_event<T: TgTransport, W: LocalWriter>(
    disabled: bool,
    dir: &Path,
    kc: &dyn Keychain,
    t: &T,
    w: &W,
    ev: &RelayEvent,
    log: Log<'_>,
) -> Result<Dispatch, String> {
    if disabled {
        return Err(OFF_MSG.to_string());
    }
    let s = load_settings_in(dir);
    if !s.enabled {
        return Ok(Dispatch::Skipped("relay is off"));
    }
    if !s.kinds.get(ev.kind).copied().unwrap_or(false) {
        return Ok(Dispatch::Skipped("kind is off"));
    }
    if load_ledger(dir).days.get(ev.kind) == Some(&ev.day) {
        return Ok(Dispatch::Skipped("already sent for that day"));
    }
    let msg = compose(w, ev, log).await;
    send_text(t, kc, &msg.text).await?;
    let mut l = load_ledger(dir);
    l.days.insert(ev.kind.to_string(), ev.day.clone());
    l.last_sent_ms = Some(chrono::Utc::now().timestamp_millis());
    if let Ok(json) = serde_json::to_string(&l) {
        let _ = write_private(&ledger_path(dir), &json);
    }
    Ok(Dispatch::Sent)
}

/// Send Test: kill switch first (no dialog when blocked), then the approval, then the send.
pub(crate) async fn send_test_core<T, G, A>(
    disabled: bool,
    approve: A,
    kc: &dyn Keychain,
    t: &T,
) -> Result<(), String>
where
    T: TgTransport,
    A: std::future::Future<Output = Result<G, String>>,
{
    if disabled {
        return Err(OFF_MSG.to_string());
    }
    let _held = approve.await?;
    send_text(t, kc, TEST_TEXT).await
}

/// Preview: the text that would be sent for `kind` from the given snapshot. Sends nothing.
pub(crate) async fn preview_core<W: LocalWriter>(
    kind: &str,
    snap: &super::noop::NoopHealthSnapshot,
    w: &W,
    log: Log<'_>,
) -> Result<Preview, String> {
    if !KINDS.contains(&kind) {
        return Err("unknown alert kind".to_string());
    }
    if !CONNECTED.contains(&kind) {
        return Err("This alert isn't connected yet.".to_string());
    }
    let ev = events_from_snapshot(snap)
        .into_iter()
        .find(|e| e.kind == kind)
        .ok_or_else(|| "No reading for this alert yet.".to_string())?;
    let c = compose(w, &ev, log).await;
    Ok(Preview {
        text: c.text,
        at: chrono::Local::now().format("%-I:%M %p").to_string(),
        from: format!("From your latest {kind} reading."),
        used_plain_line: c.fell_back,
    })
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub text: String,
    pub at: String,
    pub from: String,
    pub used_plain_line: bool,
}

// ---------------------------------------------------------------- commands

fn log_line(m: &str) {
    eprintln!("{m}");
}

async fn keys_set() -> bool {
    tokio::task::spawn_blocking(|| {
        let kc = MacKeychain;
        [TOKEN_KEY, CHAT_KEY]
            .iter()
            .all(|k| kc.get(&service_for(k)).is_ok())
    })
    .await
    .unwrap_or(false)
}

fn view_now() -> RelayView {
    let d = dir();
    view_of(
        load_settings_in(&d),
        last_sent_label(load_ledger(&d).last_sent_ms),
    )
}

#[tauri::command]
pub async fn notify_settings_get() -> Result<RelayView, String> {
    tokio::task::spawn_blocking(view_now)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct RelayPatch {
    pub enabled: Option<bool>,
    pub kinds: Option<BTreeMap<String, bool>>,
}

#[tauri::command]
pub async fn notify_settings_set(patch: RelayPatch) -> Result<RelayView, String> {
    let have_keys = keys_set().await;
    tokio::task::spawn_blocking(move || {
        let d = dir();
        let next = apply_patch(load_settings_in(&d), patch.enabled, patch.kinds, have_keys)?;
        save_settings_in(&d, &next)?;
        Ok(view_now())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sends nothing. Returns the text that would be sent for `kind`, written by the local tier.
#[tauri::command]
pub async fn notify_preview(kind: String) -> Result<Preview, String> {
    let snap = tokio::task::spawn_blocking(super::noop::health_snapshot)
        .await
        .map_err(|e| e.to_string())?;
    preview_core(&kind, &snap, &RouterWriter, &log_line).await
}

/// Native approval first, then one test message. Refused under the outbound switch or navy.
#[tauri::command]
pub async fn notify_send_test(app: tauri::AppHandle) -> Result<(), String> {
    let text = super::approval_gate::approval_text(
        "Send a test message to your Telegram",
        &format!("This sends exactly: \"{TEST_TEXT}\"\n\nIt goes to your saved chat via api.telegram.org."),
    )?;
    let key = super::approval_gate::service_key("notify-test");
    let approve = super::approval_gate::require_approval(&app, std::slice::from_ref(&key), text);
    send_test_core(
        super::telegram::outbound_disabled(),
        approve,
        &MacKeychain,
        &HttpTelegram,
    )
    .await
}

/// Send the due sleep and recovery alerts (once per day per kind), if the relay is on. Returns how
/// many went out. Not called by any timer yet.
#[tauri::command]
pub async fn notify_dispatch_due() -> Result<u32, String> {
    let disabled = super::telegram::outbound_disabled();
    if disabled {
        return Err(OFF_MSG.to_string());
    }
    let snap = tokio::task::spawn_blocking(super::noop::health_snapshot)
        .await
        .map_err(|e| e.to_string())?;
    let d = dir();
    let mut sent = 0;
    for ev in events_from_snapshot(&snap) {
        let r = dispatch_event(
            super::telegram::outbound_disabled(),
            &d,
            &MacKeychain,
            &HttpTelegram,
            &RouterWriter,
            &ev,
            &log_line,
        )
        .await?;
        if r == Dispatch::Sent {
            sent += 1;
        }
    }
    Ok(sent)
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::super::keychain::{set_with, FakeKeychain};
    use super::super::noop::{NoopDailyMetric, NoopHealthSnapshot};
    use super::*;
    use std::cell::RefCell;
    use std::sync::Mutex;

    const TOKEN: &str = "SENTINEL-tok-7f3a9c1e-do-not-leak";
    const CHAT: &str = "SENTINEL-chat-5521";

    struct MockTg {
        sent: RefCell<Vec<(String, String, String)>>,
        fail: bool,
    }
    impl MockTg {
        fn new() -> Self {
            Self {
                sent: RefCell::new(vec![]),
                fail: false,
            }
        }
    }
    impl TgTransport for MockTg {
        async fn post(&self, url: &str, chat: &str, text: &str) -> Result<(), String> {
            self.sent
                .borrow_mut()
                .push((url.into(), chat.into(), text.into()));
            if self.fail {
                Err(SEND_FAILED.to_string())
            } else {
                Ok(())
            }
        }
    }
    struct MockW(Result<String, String>);
    impl LocalWriter for MockW {
        async fn write(&self, _: &str, _: &str) -> Result<String, String> {
            self.0.clone()
        }
    }

    fn keys() -> FakeKeychain {
        let kc = FakeKeychain::default();
        set_with(&kc, TOKEN_KEY, TOKEN.into()).unwrap();
        set_with(&kc, CHAT_KEY, CHAT.into()).unwrap();
        kc
    }
    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-relay-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn on(d: &Path, kinds: &[(&str, bool)]) {
        let mut s = RelaySettings {
            enabled: true,
            ..Default::default()
        };
        for (k, v) in kinds {
            s.kinds.insert((*k).into(), *v);
        }
        save_settings_in(d, &s).unwrap();
    }
    fn snap() -> NoopHealthSnapshot {
        let row = NoopDailyMetric {
            day: "2026-11-13".into(),
            source: "t".into(),
            total_sleep_min: Some(452.0),
            efficiency: None,
            deep_min: Some(80.0),
            rem_min: Some(100.0),
            light_min: None,
            resting_hr: Some(52),
            avg_hrv: Some(66.4),
            recovery: Some(74.0),
            strain: Some(9.1),
            spo2_pct: None,
            skin_temp_dev_c: None,
            resp_rate_bpm: None,
        };
        NoopHealthSnapshot {
            source: "noop".into(),
            status: "ok".into(),
            generated_at: "x".into(),
            device_id: "d".into(),
            latest_day: Some(row.day.clone()),
            reading_day: Some(row.day.clone()),
            data_age_days: Some(0),
            today: Some(row),
            recent_days: vec![],
            latest_sleep: None,
            latest_night: None,
            battery: None,
            insights: vec![],
            message: None,
        }
    }
    fn ev(kind: &str) -> RelayEvent {
        events_from_snapshot(&snap())
            .into_iter()
            .find(|e| e.kind == kind)
            .unwrap()
    }
    fn nolog(_: &str) {}
    const GOOD: &str = "You slept 7h 32m and recovery is 74%.";

    #[tokio::test]
    async fn no_send_under_the_outbound_flag_or_navy() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        let r = dispatch_event(
            true,
            &d,
            &kc,
            &t,
            &MockW(Ok(GOOD.into())),
            &ev("recovery"),
            &nolog,
        )
        .await;
        assert!(r.unwrap_err().contains("disabled"));
        assert!(t.sent.borrow().is_empty());
        // send test: refused before the dialog is even built
        let asked = RefCell::new(false);
        let r = send_test_core(
            true,
            async {
                *asked.borrow_mut() = true;
                Ok::<(), String>(())
            },
            &kc,
            &t,
        )
        .await;
        assert!(r.is_err());
        assert!(!*asked.borrow(), "no dialog while outbound is disabled");
        assert!(t.sent.borrow().is_empty());
        // navy mode is part of outbound_disabled() (telegram.rs); the dispatch command reads it
        let src = include_str!("notify_relay.rs");
        assert!(src.matches("super::telegram::outbound_disabled()").count() >= 3);
    }

    #[tokio::test]
    async fn approval_denied_means_no_send() {
        let kc = keys();
        let t = MockTg::new();
        let r = send_test_core(
            false,
            async { Err::<(), String>("declined: no".into()) },
            &kc,
            &t,
        )
        .await;
        assert!(r.unwrap_err().starts_with("declined:"));
        assert!(t.sent.borrow().is_empty());
        send_test_core(false, async { Ok::<(), String>(()) }, &kc, &t)
            .await
            .unwrap();
        let s = t.sent.borrow();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].2, TEST_TEXT);
        assert!(s[0].0.starts_with("https://api.telegram.org/bot"));
    }

    #[test]
    fn number_check_matches_the_source_row() {
        let e = ev("sleep");
        assert!(numbers_match("You slept 7h 32m, 452 minutes.", &e.allowed));
        assert!(numbers_match("About 7.5 hours of sleep.", &e.allowed));
        assert!(!numbers_match("You slept 7h 40m.", &e.allowed));
        assert!(!numbers_match("Recovery is 75%.", &ev("recovery").allowed));
        assert!(numbers_match(
            "Recovery is 74%, HRV 66.",
            &ev("recovery").allowed
        ));
        assert_eq!(numbers_in("1,284 and 3.5 and 12"), vec![1284.0, 3.5, 12.0]);
    }

    #[tokio::test]
    async fn mismatch_falls_back_to_the_plain_line_and_logs_no_text() {
        let logs = Mutex::new(vec![]);
        let log = |m: &str| logs.lock().unwrap().push(m.to_string());
        let c = compose(
            &MockW(Ok("Recovery is 99% today!".into())),
            &ev("recovery"),
            &log,
        )
        .await;
        assert!(c.fell_back);
        assert_eq!(c.text, "Recovery: 74%.");
        let c = compose(&MockW(Ok(GOOD.into())), &ev("recovery"), &log).await;
        assert!(c.fell_back, "7 and 32 are not in a recovery row");
        let c = compose(
            &MockW(Ok("Recovery is 74%, a solid base.".into())),
            &ev("recovery"),
            &log,
        )
        .await;
        assert!(!c.fell_back);
        let c = compose(&MockW(Err("router down".into())), &ev("recovery"), &log).await;
        assert!(c.fell_back);
        let all = logs.lock().unwrap().join("\n");
        assert!(all.contains("number check failed"));
        assert!(!all.contains("99") && !all.contains("74"));
    }

    #[tokio::test]
    async fn token_sentinel_never_reaches_logs_errors_or_ipc() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let logs = Mutex::new(vec![]);
        let log = |m: &str| logs.lock().unwrap().push(m.to_string());
        let mut t = MockTg::new();
        t.fail = true;
        let err = dispatch_event(
            false,
            &d,
            &kc,
            &t,
            &MockW(Ok("bad 12345".into())),
            &ev("recovery"),
            &log,
        )
        .await
        .unwrap_err();
        assert!(!err.contains("SENTINEL"));
        // missing keys
        let e2 = send_test_core(
            false,
            async { Ok::<(), String>(()) },
            &FakeKeychain::default(),
            &MockTg::new(),
        )
        .await
        .unwrap_err();
        assert!(!e2.contains("SENTINEL"));
        // IPC values
        let v = serde_json::to_string(&view_of(load_settings_in(&d), None)).unwrap();
        let p = serde_json::to_string(
            &preview_core("recovery", &snap(), &MockW(Ok("Recovery 74%".into())), &log)
                .await
                .unwrap(),
        )
        .unwrap();
        for s in [&v, &p, &logs.lock().unwrap().join("\n")] {
            assert!(!s.contains("SENTINEL"));
        }
        // files under the data dir
        for e in std::fs::read_dir(&d).unwrap() {
            let b = std::fs::read(e.unwrap().path()).unwrap();
            assert!(!b.windows(8).any(|w| w == b"SENTINEL"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn kinds_filter_and_enable_switch() {
        let d = tmp();
        let kc = keys();
        let w = MockW(Ok("Recovery is 74%.".into()));
        let t = MockTg::new();
        // off by default
        assert_eq!(
            dispatch_event(false, &d, &kc, &t, &w, &ev("recovery"), &nolog)
                .await
                .unwrap(),
            Dispatch::Skipped("relay is off")
        );
        on(&d, &[("recovery", false), ("sleep", true)]);
        assert_eq!(
            dispatch_event(false, &d, &kc, &t, &w, &ev("recovery"), &nolog)
                .await
                .unwrap(),
            Dispatch::Skipped("kind is off")
        );
        assert!(t.sent.borrow().is_empty());
        let w2 = MockW(Ok("You slept 7h 32m.".into()));
        assert_eq!(
            dispatch_event(false, &d, &kc, &t, &w2, &ev("sleep"), &nolog)
                .await
                .unwrap(),
            Dispatch::Sent
        );
        assert_eq!(t.sent.borrow().len(), 1);
        // once per day per kind
        assert!(matches!(
            dispatch_event(false, &d, &kc, &t, &w2, &ev("sleep"), &nolog)
                .await
                .unwrap(),
            Dispatch::Skipped(_)
        ));
        assert_eq!(t.sent.borrow().len(), 1);
        // unknown kinds refused, enabling needs keys
        assert!(apply_patch(
            RelaySettings::default(),
            None,
            Some([("nope".to_string(), true)].into()),
            true
        )
        .is_err());
        assert!(apply_patch(RelaySettings::default(), Some(true), None, false).is_err());
        assert!(
            apply_patch(RelaySettings::default(), Some(true), None, true)
                .unwrap()
                .enabled
        );
        // the not-connected kinds are listed, not faked
        let v = view_of(RelaySettings::default(), None);
        assert_eq!(
            v.not_connected,
            ["effort", "battery", "sync", "lucid", "illness"]
        );
        assert!(preview_core("lucid", &snap(), &w, &nolog).await.is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn preview_sends_nothing_and_settings_file_is_private() {
        let t = MockTg::new();
        let p = preview_core(
            "sleep",
            &snap(),
            &MockW(Ok("You slept 7h 32m.".into())),
            &nolog,
        )
        .await
        .unwrap();
        assert!(!p.used_plain_line);
        assert!(t.sent.borrow().is_empty());
        let d = tmp();
        save_settings_in(&d, &RelaySettings::default()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(settings_path(&d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        // only {enabled, kinds} on disk
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings_path(&d)).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn source_names_no_host_but_telegram() {
        let src = include_str!("notify_relay.rs");
        let prod = &src[..src.find("\n#[cfg(test)]").unwrap()];
        let mut hosts = vec![];
        for (i, _) in prod.match_indices("://") {
            let rest = &prod[i + 3..];
            let host: String = rest
                .chars()
                .take_while(|c| {
                    c.is_ascii_alphanumeric()
                        || *c == '.'
                        || *c == '-'
                        || *c == '{'
                        || *c == '}'
                        || *c == '_'
                })
                .collect();
            hosts.push(host);
        }
        assert!(!hosts.is_empty());
        assert!(hosts.iter().all(|h| h == "{TELEGRAM_HOST}"), "{hosts:?}");
        assert_eq!(TELEGRAM_HOST, "api.telegram.org");
        assert!(!prod.contains("api.telegram.org/bot"));
        // send_url refuses a token that would change the host
        assert!(send_url("abc@evil.example/").is_err());
        assert!(send_url("123:ABC")
            .unwrap()
            .starts_with("https://api.telegram.org/bot"));
    }
}
