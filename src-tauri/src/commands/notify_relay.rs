//! Telegram relay (HANDOFF-1111s step 6, approved in decision 0014 #3).
//!
//! Settings are `{ enabled, kinds }` (no quiet hours, no rate limit: Taisun's choice) plus two pins written
//! at approval time (a hash of the approved chat id, and the kinds approved). Token and chat id live in the
//! Keychain (`com.taisun.nexus.telegram_*`, written through `secret_set`); the UI only ever sees set / not
//! set. Nothing here logs a token, a chat id, message text or a health value.
//!
//! Guard order for every send: outbound kill switch / navy, then settings and pins, then the Keychain read
//! (the chat id actually posted must hash to the approved pin), then the kill switch again, then the single
//! POST to `api.telegram.org` (which checks the switch once more as its last step).
//!
//! Message text: the LOCAL light tier writes prose around placeholders like {RECOVERY}; Rust validates it and
//! substitutes the pre-formatted values. Anything unexpected falls back to a plain line built by Rust.
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
/// Telegram bot tokens are about 46 characters; anything longer is not one.
const MAX_TOKEN_CHARS: usize = 100;
const MAX_CHAT_CHARS: usize = 64;
const MAX_MESSAGE_CHARS: usize = 280;
const MAX_SENTENCES: usize = 3;
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
const SAVE_FAILED: &str = "Couldn't save the relay settings.";
const CHAT_CHANGED: &str = "chat-changed: the Telegram chat is not the one you approved. Turn the relay off and on again to approve it.";
const KINDS_CHANGED: &str =
    "kinds-changed: this alert was not approved. Turn the relay off and on again to approve it.";

/// Live kill-switch check (`outbound_disabled()` in the app, a flag in tests).
type Gate<'a> = &'a (dyn Fn() -> bool + Sync);
type Log<'a> = &'a (dyn Fn(&str) + Sync);

// ---------------------------------------------------------------- settings

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RelaySettings {
    pub enabled: bool,
    pub kinds: BTreeMap<String, bool>,
    /// sha256 of the chat id approved when the relay was switched on. Never sent to the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_pin: Option<String>,
    /// The (connected) kinds that were on at that approval. A kind outside this list is not sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_kinds: Vec<String>,
}

impl Default for RelaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            chat_pin: None,
            approved_kinds: vec![],
            // Kinds the app can't see yet default to off.
            kinds: KINDS
                .iter()
                .map(|k| ((*k).to_string(), CONNECTED.contains(k)))
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

    /// Connected kinds currently switched on, sorted.
    fn enabled_connected(&self) -> Vec<String> {
        self.kinds
            .iter()
            .filter(|(k, on)| **on && CONNECTED.contains(&k.as_str()))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// True when some enabled connected kind is outside the approved list.
    fn grew_since_approval(&self) -> bool {
        self.enabled_connected()
            .iter()
            .any(|k| !self.approved_kinds.contains(k))
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

/// Private (0600), atomic. Errors become one fixed string: a path or OS error never reaches the UI.
fn write_private(path: &Path, json: &str) -> Result<(), String> {
    let go = || -> std::io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        crate::commands::continuity::atomic_write_with_mode(path, json.as_bytes(), Some(0o600))
    };
    go().map_err(|_| SAVE_FAILED.to_string())
}

pub(crate) fn load_settings_in(dir: &Path) -> RelaySettings {
    std::fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<RelaySettings>(&s).ok())
        .unwrap_or_default()
        .normalized()
}

pub(crate) fn save_settings_in(dir: &Path, s: &RelaySettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|_| SAVE_FAILED.to_string())?;
    write_private(&settings_path(dir), &json)
}

/// Apply a patch. Unknown or not-connected kinds can't be switched on; switching the relay on is never
/// done here (it goes through `enable_core` and its approval). Off clears the pins.
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
        if k.iter()
            .any(|(k, on)| *on && !CONNECTED.contains(&k.as_str()))
        {
            return Err("This alert isn't connected yet.".to_string());
        }
    }
    if enabled == Some(true) && !keys_set {
        return Err(KEYS_MSG.to_string());
    }
    if enabled == Some(false) {
        s.enabled = false;
        s.chat_pin = None;
        s.approved_kinds.clear();
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
    /// kind -> last day claimed
    days: BTreeMap<String, String>,
    last_sent_ms: Option<i64>,
}

/// Missing file is an empty ledger; an unreadable or corrupt one is an error (fail closed).
fn load_ledger_strict(dir: &Path) -> Result<Ledger, ()> {
    match std::fs::read_to_string(ledger_path(dir)) {
        Ok(s) => serde_json::from_str(&s).map_err(|_| ()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(_) => Err(()),
    }
}

fn write_ledger(dir: &Path, l: &Ledger) -> Result<(), String> {
    let json = serde_json::to_string(l).map_err(|_| SAVE_FAILED.to_string())?;
    write_private(&ledger_path(dir), &json)
}

fn last_sent_label(ms: Option<i64>) -> Option<String> {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms?)
        .single()
        .map(|t| t.format("%-I:%M %p").to_string())
}

// ---------------------------------------------------------------- events + prose validation

/// One alert built from a source row. The model sees only the placeholder names; Rust holds the values.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelayEvent {
    pub kind: &'static str,
    pub day: String,
    /// (placeholder name, pre-formatted value, what it means)
    pub vars: Vec<(&'static str, String, &'static str)>,
    /// A message without this placeholder is rejected.
    pub required: &'static str,
    /// The plain factual line, built entirely by Rust.
    pub plain: String,
}

impl RelayEvent {
    /// What the model is told: the placeholders it may write, never their values.
    fn prompt(&self) -> String {
        let mut s = format!(
            "Alert: {}. Placeholders you can write, exactly as shown:\n",
            self.kind
        );
        for (n, _, what) in &self.vars {
            s.push_str(&format!("{{{n}}} = {what}\n"));
        }
        s.push_str(&format!("You must use {{{}}}.", self.required));
        s
    }
}

fn hm(min: f64) -> String {
    let t = min.round() as i64;
    format!("{}h {}m", t / 60, t % 60)
}

fn row_for(snap: &super::noop::NoopHealthSnapshot) -> Option<&super::noop::NoopDailyMetric> {
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
        let mut vars = vec![("SLEEP_TOTAL", hm(min), "total sleep last night")];
        if let Some(v) = row.deep_min.filter(|v| v.is_finite() && *v >= 0.0) {
            vars.push((
                "DEEP",
                format!("{} minutes", v.round() as i64),
                "deep sleep",
            ));
        }
        if let Some(v) = row.rem_min.filter(|v| v.is_finite() && *v >= 0.0) {
            vars.push(("REM", format!("{} minutes", v.round() as i64), "REM sleep"));
        }
        out.push(RelayEvent {
            kind: "sleep",
            day: row.day.clone(),
            vars,
            required: "SLEEP_TOTAL",
            plain: format!("Sleep: {} last night.", hm(min)),
        });
    }
    if let Some(rec) = row.recovery.filter(|r| r.is_finite() && *r >= 0.0) {
        let mut vars = vec![(
            "RECOVERY",
            format!("{}%", rec.round() as i64),
            "recovery score",
        )];
        if let Some(h) = row.avg_hrv.filter(|v| v.is_finite() && *v > 0.0) {
            vars.push((
                "HRV",
                format!("{} ms", h.round() as i64),
                "heart rate variability",
            ));
        }
        if let Some(h) = row.resting_hr.filter(|v| *v > 0) {
            vars.push(("RHR", format!("{h} bpm"), "resting heart rate"));
        }
        out.push(RelayEvent {
            kind: "recovery",
            day: row.day.clone(),
            vars,
            required: "RECOVERY",
            plain: format!("Recovery: {}%.", rec.round() as i64),
        });
    }
    out
}

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "billion",
    "half",
    "dozen",
    "once",
    "twice",
    "double",
    "triple",
];
const LINK_BITS: &[&str] = &["http", "www", "t.me", "://", "@", "/", "#"];

/// Accept the model's prose or say why not. On success the placeholders are replaced by Rust's values.
/// Only ASCII letters, spaces and `. , ; : ' % ( ) -` may appear outside placeholders, so a digit
/// (any script), a link, a mention, a command or markup can never get through.
pub(crate) fn validate_prose(text: &str, ev: &RelayEvent) -> Result<String, &'static str> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_MESSAGE_CHARS {
        return Err("length");
    }
    let lower = t.to_lowercase();
    if LINK_BITS.iter().any(|b| lower.contains(b)) {
        return Err("link");
    }
    let mut prose = String::new();
    let mut used: Vec<&str> = vec![];
    let mut chars = t.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c.is_ascii_uppercase() || c == '_' => name.push(c),
                        _ => return Err("token"),
                    }
                }
                match ev.vars.iter().find(|(n, _, _)| *n == name) {
                    Some((n, _, _)) => used.push(n),
                    None => return Err("token"),
                }
                prose.push_str("VALUE");
            }
            '}' => return Err("token"),
            c => prose.push(c),
        }
    }
    if !used.contains(&ev.required) {
        return Err("missing placeholder");
    }
    if !prose
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == ' ' || ".,;:'%()-".contains(c))
    {
        return Err("characters");
    }
    let words = prose.split(|c: char| !c.is_ascii_alphabetic());
    if words
        .filter(|w| !w.is_empty())
        .any(|w| NUMBER_WORDS.contains(&w.to_ascii_lowercase().as_str()))
    {
        return Err("number word");
    }
    if prose.split('.').filter(|s| !s.trim().is_empty()).count() > MAX_SENTENCES {
        return Err("sentences");
    }
    let mut out = t.to_string();
    for (n, v, _) in &ev.vars {
        out = out.replace(&format!("{{{n}}}"), v);
    }
    Ok(out)
}

// ---------------------------------------------------------------- ports (real impls below; tests use mocks)

pub(crate) trait LocalWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String>;
}

pub(crate) trait TgTransport {
    async fn post(&self, url: &str, chat_id: &str, text: &str) -> Result<(), String>;
}

struct RouterWriter;
impl LocalWriter for RouterWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String> {
        super::llm_router::local_complete(instruction, facts).await
    }
}

/// The sendMessage body: no parse_mode (no markup), link previews off.
pub(crate) fn send_body(chat_id: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "link_preview_options": { "is_disabled": true }
    })
}

struct HttpTelegram;
impl TgTransport for HttpTelegram {
    async fn post(&self, url: &str, chat_id: &str, text: &str) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .https_only(true)
            .connect_timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| "Network error contacting Telegram.".to_string())?;
        let req = client.post(url).json(&send_body(chat_id, text));
        if super::telegram::outbound_disabled() {
            return Err(OFF_MSG.to_string());
        }
        // The URL carries the token: never format a reqwest error (it can include the URL).
        let resp = req
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

const INSTRUCTION: &str = "Write a short Telegram message to Taisun about the alert below, in your own words. Write at most three short sentences using only plain letters, spaces and . , ; : ' % ( ) - . Never write a digit or a number word; refer to every value only by its placeholder, such as {RECOVERY}. No links, mentions, commands, emojis or markup.";

pub(crate) struct Composed {
    pub text: String,
    pub fell_back: bool,
}

/// Amara's words from the local tier, or the plain line when the writer fails or the prose is rejected.
/// The log line carries the kind and a reason code only.
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
    let raw = match w.write(INSTRUCTION, &ev.prompt()).await {
        Ok(t) => t,
        Err(_) => return fall("local writer unavailable"),
    };
    match validate_prose(&raw, ev) {
        Ok(text) => Composed {
            text,
            fell_back: false,
        },
        Err(why) => fall(&format!("prose rejected ({why})")),
    }
}

// ---------------------------------------------------------------- keys + send

fn read_secret(
    kc: &dyn Keychain,
    name: &str,
    max_chars: usize,
) -> Result<Zeroizing<String>, String> {
    match kc.get(&service_for(name)) {
        Ok(v) => {
            let s = String::from_utf8(v)
                .map(Zeroizing::new)
                .map_err(|_| KEYS_MSG.to_string())?;
            if s.trim().chars().count() > max_chars {
                return Err("The saved Telegram value is not valid.".to_string());
            }
            Ok(s)
        }
        Err(KeychainError::NotFound) => Err(KEYS_MSG.to_string()),
        Err(KeychainError::Other(_)) => Err("Keychain is unavailable.".to_string()),
    }
}

fn read_token(kc: &dyn Keychain) -> Result<Zeroizing<String>, String> {
    read_secret(kc, TOKEN_KEY, MAX_TOKEN_CHARS)
}
fn read_chat(kc: &dyn Keychain) -> Result<Zeroizing<String>, String> {
    read_secret(kc, CHAT_KEY, MAX_CHAT_CHARS)
}

fn chat_hash(chat: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(chat.trim().as_bytes()))
}

fn tail4(chat: &str) -> String {
    let t = chat.trim();
    let n = t.chars().count();
    t.chars().skip(n.saturating_sub(4)).collect()
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

/// The one place a message leaves. The chat id read here is the value that is posted, and it must hash
/// to `approved_chat_hash`. The kill switch is checked after the Keychain reads, right before the post.
async fn send_text<T: TgTransport>(
    t: &T,
    kc: &dyn Keychain,
    text: &str,
    off: Gate<'_>,
    approved_chat_hash: Option<&str>,
) -> Result<(), String> {
    let token = read_token(kc)?;
    let chat = read_chat(kc)?;
    match approved_chat_hash {
        Some(h) if h == chat_hash(&chat) => {}
        _ => return Err(CHAT_CHANGED.to_string()),
    }
    if off() {
        return Err(OFF_MSG.to_string());
    }
    let url = Zeroizing::new(send_url(token.trim())?);
    t.post(&url, chat.trim(), text).await
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Dispatch {
    Sent,
    Skipped(&'static str),
}

/// Switch the relay on (or re-approve): needs both keys, then the native approval (which shows the last 4
/// of the chat and the kinds), then pins the approved chat hash and kinds.
pub(crate) async fn enable_core<G, A>(
    dir: &Path,
    kc: &dyn Keychain,
    approve: impl FnOnce(String, Vec<String>) -> A,
) -> Result<RelaySettings, String>
where
    A: std::future::Future<Output = Result<G, String>>,
{
    read_token(kc)?;
    let chat = read_chat(kc)?;
    let mut s = load_settings_in(dir);
    let kinds = s.enabled_connected();
    let _held = approve(tail4(&chat), kinds.clone()).await?;
    s.enabled = true;
    s.chat_pin = Some(chat_hash(&chat));
    s.approved_kinds = kinds;
    save_settings_in(dir, &s)?;
    Ok(s)
}

/// One dispatch at a time, process-wide: the ledger claim and the send are a single critical section.
static DISPATCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Send one event. Kill switch first, then settings and pins, then claim the day in the ledger BEFORE
/// sending (an unreadable or unwritable ledger means no send), roll the claim back if the send fails.
pub(crate) async fn dispatch_event<T: TgTransport, W: LocalWriter>(
    off: Gate<'_>,
    dir: &Path,
    kc: &dyn Keychain,
    t: &T,
    w: &W,
    ev: &RelayEvent,
    log: Log<'_>,
) -> Result<Dispatch, String> {
    if off() {
        return Err(OFF_MSG.to_string());
    }
    let _one_at_a_time = DISPATCH.lock().await;
    let s = load_settings_in(dir);
    if !s.enabled {
        return Ok(Dispatch::Skipped("relay is off"));
    }
    if !s.kinds.get(ev.kind).copied().unwrap_or(false) {
        return Ok(Dispatch::Skipped("kind is off"));
    }
    if !s.approved_kinds.iter().any(|k| k == ev.kind) {
        return Err(KINDS_CHANGED.to_string());
    }
    let Ok(mut ledger) = load_ledger_strict(dir) else {
        return Ok(Dispatch::Skipped("ledger unavailable"));
    };
    if ledger.days.get(ev.kind) == Some(&ev.day) {
        return Ok(Dispatch::Skipped("already sent for that day"));
    }
    let msg = compose(w, ev, log).await;
    let before = ledger.clone();
    ledger.days.insert(ev.kind.to_string(), ev.day.clone());
    if write_ledger(dir, &ledger).is_err() {
        return Ok(Dispatch::Skipped("ledger unavailable"));
    }
    match send_text(t, kc, &msg.text, off, s.chat_pin.as_deref()).await {
        Ok(()) => {
            ledger.last_sent_ms = Some(chrono::Utc::now().timestamp_millis());
            let _ = write_ledger(dir, &ledger);
            Ok(Dispatch::Sent)
        }
        Err(e) => {
            let _ = write_ledger(dir, &before);
            Err(e)
        }
    }
}

/// Send Test: kill switch first (no dialog when blocked), then the approval (which shows the last 4 of the
/// chat), then the send, which must still hash to the chat that was shown.
pub(crate) async fn send_test_core<T, G, A>(
    off: Gate<'_>,
    kc: &dyn Keychain,
    t: &T,
    approve: impl FnOnce(String) -> A,
) -> Result<(), String>
where
    T: TgTransport,
    A: std::future::Future<Output = Result<G, String>>,
{
    if off() {
        return Err(OFF_MSG.to_string());
    }
    read_token(kc)?;
    let chat = read_chat(kc)?;
    let shown = chat_hash(&chat);
    let _held = approve(tail4(&chat)).await?;
    send_text(t, kc, TEST_TEXT, off, Some(&shown)).await
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

fn live_off() -> bool {
    super::telegram::outbound_disabled()
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
    let last = load_ledger_strict(&d).ok().and_then(|l| l.last_sent_ms);
    view_of(load_settings_in(&d), last_sent_label(last))
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
pub async fn notify_settings_set(
    app: tauri::AppHandle,
    patch: RelayPatch,
) -> Result<RelayView, String> {
    let turning_on = patch.enabled == Some(true);
    let have_keys = keys_set().await;
    // Kinds and "off" apply directly; "on" (and a kind growing past the approved list) goes through the approval.
    let rest = (patch.enabled.filter(|e| !e), patch.kinds);
    let grew = tokio::task::spawn_blocking(move || {
        let d = dir();
        let next = apply_patch(load_settings_in(&d), rest.0, rest.1, have_keys)?;
        save_settings_in(&d, &next)?;
        Ok::<bool, String>(next.enabled && next.grew_since_approval())
    })
    .await
    .map_err(|e| e.to_string())??;
    if turning_on || grew {
        let key = super::approval_gate::service_key("notify-enable");
        enable_core(&dir(), &MacKeychain, |tail, kinds| {
            let text = super::approval_gate::approval_text(
                "Turn on the Telegram relay",
                &format!("Amara will send your {} alerts in her own words to the Telegram chat ending in {tail}, via api.telegram.org. If that chat changes later, sends stop until you approve again.", kinds.join(" and ")),
            );
            let app = app.clone();
            async move { super::approval_gate::require_approval(&app, &[key], text?).await }
        })
        .await?;
    }
    tokio::task::spawn_blocking(view_now)
        .await
        .map_err(|e| e.to_string())
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
    let key = super::approval_gate::service_key("notify-test");
    send_test_core(&live_off, &MacKeychain, &HttpTelegram, |tail| {
        let text = super::approval_gate::approval_text(
            "Send a test message to your Telegram",
            &format!("This sends exactly: \"{TEST_TEXT}\"\n\nIt goes to the saved chat ending in {tail} via api.telegram.org."),
        );
        async move { super::approval_gate::require_approval(&app, std::slice::from_ref(&key), text?).await }
    })
    .await
}

/// Send the due sleep and recovery alerts (once per day per kind), if the relay is on. Returns how
/// many went out. Not called by any timer yet.
#[tauri::command]
pub async fn notify_dispatch_due() -> Result<u32, String> {
    if live_off() {
        return Err(OFF_MSG.to_string());
    }
    let snap = tokio::task::spawn_blocking(super::noop::health_snapshot)
        .await
        .map_err(|e| e.to_string())?;
    let d = dir();
    let mut sent = 0;
    for ev in events_from_snapshot(&snap) {
        let r = dispatch_event(
            &live_off,
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
    use std::sync::atomic::{AtomicBool, Ordering};
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
        fn count(&self) -> usize {
            self.sent.borrow().len()
        }
    }
    impl TgTransport for MockTg {
        async fn post(&self, url: &str, chat: &str, text: &str) -> Result<(), String> {
            tokio::task::yield_now().await;
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
            tokio::task::yield_now().await;
            self.0.clone()
        }
    }
    /// A writer that flips the kill switch while the message is being composed.
    struct FlipW<'a>(&'a AtomicBool, String);
    impl LocalWriter for FlipW<'_> {
        async fn write(&self, _: &str, _: &str) -> Result<String, String> {
            self.0.store(true, Ordering::SeqCst);
            Ok(self.1.clone())
        }
    }

    fn never() -> bool {
        false
    }
    fn always() -> bool {
        true
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
    /// Relay on and approved for the connected kinds that are on, pinned to CHAT.
    fn on(d: &Path, kinds: &[(&str, bool)]) {
        let mut s = RelaySettings {
            enabled: true,
            chat_pin: Some(chat_hash(CHAT)),
            ..Default::default()
        };
        s.approved_kinds = vec!["sleep".into(), "recovery".into()];
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
    const REC: &str = "Recovery is {RECOVERY} today, a solid base.";
    const SLP: &str = "You got {SLEEP_TOTAL} of sleep.";
    fn ok_msg(kind: &str) -> MockW {
        MockW(Ok(if kind == "sleep" { SLP } else { REC }.into()))
    }

    #[tokio::test]
    async fn no_send_under_the_outbound_flag_or_navy() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        let r = dispatch_event(
            &always,
            &d,
            &kc,
            &t,
            &ok_msg("recovery"),
            &ev("recovery"),
            &nolog,
        )
        .await;
        assert!(r.unwrap_err().contains("disabled"));
        let asked = RefCell::new(false);
        let r = send_test_core(&always, &kc, &t, |_| {
            *asked.borrow_mut() = true;
            async { Ok::<(), String>(()) }
        })
        .await;
        assert!(r.is_err());
        assert!(!*asked.borrow(), "no dialog while outbound is disabled");
        assert_eq!(t.count(), 0);
        // navy mode is part of outbound_disabled() (telegram.rs); every live path goes through it
        let src = include_str!("notify_relay.rs");
        assert!(src.contains("fn live_off() -> bool {\n    super::telegram::outbound_disabled()"));
        assert!(src.matches("live_off").count() >= 4);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn kill_switch_flipped_mid_flight_means_zero_posts() {
        // flipped while the text is composed
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        let flag = AtomicBool::new(false);
        let off = || flag.load(Ordering::SeqCst);
        let w = FlipW(&flag, REC.into());
        let r = dispatch_event(&off, &d, &kc, &t, &w, &ev("recovery"), &nolog).await;
        assert!(r.unwrap_err().contains("disabled"));
        assert_eq!(t.count(), 0);
        // and the day was released again, so a later send isn't lost
        flag.store(false, Ordering::SeqCst);
        let r = dispatch_event(
            &off,
            &d,
            &kc,
            &t,
            &ok_msg("recovery"),
            &ev("recovery"),
            &nolog,
        )
        .await;
        assert_eq!(r.unwrap(), Dispatch::Sent);
        // Send Test approved, then flipped
        let flag2 = AtomicBool::new(false);
        let off2 = || flag2.load(Ordering::SeqCst);
        let t2 = MockTg::new();
        let r = send_test_core(&off2, &kc, &t2, |_| {
            flag2.store(true, Ordering::SeqCst);
            async { Ok::<(), String>(()) }
        })
        .await;
        assert!(r.unwrap_err().contains("disabled"));
        assert_eq!(t2.count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn approval_denied_means_no_send_and_dialog_shows_the_chat_tail() {
        let kc = keys();
        let t = MockTg::new();
        let r = send_test_core(&never, &kc, &t, |_| async {
            Err::<(), String>("declined: no".into())
        })
        .await;
        assert!(r.unwrap_err().starts_with("declined:"));
        assert_eq!(t.count(), 0);
        let tail = RefCell::new(String::new());
        send_test_core(&never, &kc, &t, |x| {
            *tail.borrow_mut() = x;
            async { Ok::<(), String>(()) }
        })
        .await
        .unwrap();
        assert_eq!(*tail.borrow(), "5521");
        let s = t.sent.borrow();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].2, TEST_TEXT);
        assert!(s[0].0.starts_with("https://api.telegram.org/bot"));
    }

    #[tokio::test]
    async fn chat_swapped_between_dialog_and_post_sends_nothing() {
        let kc = keys();
        let t = MockTg::new();
        let r = send_test_core(&never, &kc, &t, |_| {
            set_with(&kc, CHAT_KEY, "SENTINEL-attacker-chat".into()).unwrap();
            async { Ok::<(), String>(()) }
        })
        .await;
        assert!(r.unwrap_err().starts_with("chat-changed:"));
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn prose_table() {
        let r = ev("recovery");
        let s = ev("sleep");
        // accepted, with Rust's values substituted
        assert_eq!(
            validate_prose(REC, &r).unwrap(),
            "Recovery is 74% today, a solid base."
        );
        assert_eq!(
            validate_prose("Slept {SLEEP_TOTAL}; deep was {DEEP} and REM {REM}.", &s).unwrap(),
            "Slept 7h 32m; deep was 80 minutes and REM 100 minutes."
        );
        // rejected
        let bad: Vec<(&str, &RelayEvent, &str)> = vec![
            ("Recovery is seventy-four {RECOVERY}.", &r, "number word"),
            ("Recovery is {RECOVERY} and thirty more.", &r, "number word"),
            (
                "Recovery is {RECOVERY}, twice yesterday.",
                &r,
                "number word",
            ),
            ("Recovery is \u{FF17}\u{FF14} {RECOVERY}.", &r, "characters"),
            ("Recovery is 74 {RECOVERY}.", &r, "characters"),
            ("See http://x for {RECOVERY}.", &r, "link"),
            ("Hi @bob {RECOVERY}.", &r, "link"),
            ("/start {RECOVERY}.", &r, "link"),
            ("Open t.me/x {RECOVERY}.", &r, "link"),
            ("Tag #health {RECOVERY}.", &r, "link"),
            ("Recovery is 80% today.", &s, "missing placeholder"),
            ("Recovery is 80% and {SLEEP_TOTAL}.", &s, "characters"),
            (
                "You got {SLEEP_TOTAL} and recovery is {RECOVERY}.",
                &s,
                "token",
            ),
            ("Recovery is {NOPE}.", &r, "token"),
            ("Recovery is {RECOVERY.", &r, "token"),
            ("A short note with no value.", &r, "missing placeholder"),
            ("Recovery is {RECOVERY}!", &r, "characters"),
            ("Recovery is {RECOVERY}\nok", &r, "characters"),
            ("**Recovery** is {RECOVERY}.", &r, "characters"),
            ("A. B. C. D {RECOVERY}.", &r, "sentences"),
        ];
        for (text, e, why) in bad {
            assert_eq!(validate_prose(text, e).unwrap_err(), why, "{text}");
        }
        assert_eq!(
            validate_prose(&format!("{{RECOVERY}} {}", "x".repeat(300)), &r).unwrap_err(),
            "length"
        );
        assert_eq!(validate_prose("  ", &r).unwrap_err(), "length");
        // the model never sees a value
        let p = r.prompt();
        assert!(p.contains("{RECOVERY}") && !p.contains("74"));
    }

    #[tokio::test]
    async fn rejection_falls_back_to_the_rust_line_and_logs_no_text() {
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
        let c = compose(&ok_msg("sleep"), &ev("recovery"), &log).await;
        assert!(c.fell_back, "a sleep placeholder in a recovery message");
        let c = compose(&ok_msg("recovery"), &ev("recovery"), &log).await;
        assert!(!c.fell_back);
        let c = compose(&MockW(Err("router down".into())), &ev("recovery"), &log).await;
        assert!(c.fell_back);
        let all = logs.lock().unwrap().join("\n");
        assert!(all.contains("prose rejected"));
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
            &never,
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
        let e2 = send_test_core(
            &never,
            &FakeKeychain::default(),
            &MockTg::new(),
            |_| async { Ok::<(), String>(()) },
        )
        .await
        .unwrap_err();
        assert!(!e2.contains("SENTINEL"));
        let v = serde_json::to_string(&view_of(load_settings_in(&d), None)).unwrap();
        let p = serde_json::to_string(
            &preview_core("recovery", &snap(), &ok_msg("recovery"), &log)
                .await
                .unwrap(),
        )
        .unwrap();
        for s in [&v, &p, &logs.lock().unwrap().join("\n")] {
            assert!(!s.contains("SENTINEL"));
        }
        for e in std::fs::read_dir(&d).unwrap() {
            let b = std::fs::read(e.unwrap().path()).unwrap();
            assert!(!b.windows(8).any(|w| w == b"SENTINEL"));
        }
        // a save failure is a fixed string, never a path
        let blocked = d.join("notify_relay.json");
        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir(&blocked).unwrap();
        let e = save_settings_in(&d, &RelaySettings::default()).unwrap_err();
        assert_eq!(e, SAVE_FAILED);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn token_and_chat_are_length_capped() {
        let kc = FakeKeychain::default();
        set_with(&kc, TOKEN_KEY, "1".repeat(101)).unwrap();
        set_with(&kc, CHAT_KEY, CHAT.into()).unwrap();
        assert!(read_token(&kc).is_err());
        set_with(&kc, TOKEN_KEY, "1".repeat(100)).unwrap();
        assert!(read_token(&kc).is_ok());
        set_with(&kc, CHAT_KEY, "9".repeat(65)).unwrap();
        assert!(read_chat(&kc).is_err());
    }

    #[tokio::test]
    async fn kinds_filter_defaults_and_enable_gating() {
        let d = tmp();
        let kc = keys();
        let t = MockTg::new();
        let d0 = RelaySettings::default();
        assert!(d0.kinds["sleep"] && d0.kinds["recovery"]);
        assert!(["effort", "battery", "sync", "lucid", "illness"]
            .iter()
            .all(|k| !d0.kinds[*k]));
        assert_eq!(
            dispatch_event(
                &never,
                &d,
                &kc,
                &t,
                &ok_msg("recovery"),
                &ev("recovery"),
                &nolog
            )
            .await
            .unwrap(),
            Dispatch::Skipped("relay is off")
        );
        on(&d, &[("recovery", false), ("sleep", true)]);
        assert_eq!(
            dispatch_event(
                &never,
                &d,
                &kc,
                &t,
                &ok_msg("recovery"),
                &ev("recovery"),
                &nolog
            )
            .await
            .unwrap(),
            Dispatch::Skipped("kind is off")
        );
        assert_eq!(t.count(), 0);
        assert_eq!(
            dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
                .await
                .unwrap(),
            Dispatch::Sent
        );
        assert!(matches!(
            dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
                .await
                .unwrap(),
            Dispatch::Skipped(_)
        ));
        assert_eq!(t.count(), 1);
        assert!(apply_patch(
            RelaySettings::default(),
            None,
            Some([("nope".to_string(), true)].into()),
            true
        )
        .is_err());
        assert!(apply_patch(
            RelaySettings::default(),
            None,
            Some([("lucid".to_string(), true)].into()),
            true
        )
        .is_err());
        assert!(apply_patch(RelaySettings::default(), Some(true), None, false).is_err());
        assert!(
            !apply_patch(RelaySettings::default(), Some(true), None, true)
                .unwrap()
                .enabled
        );
        let v = view_of(RelaySettings::default(), None);
        assert_eq!(
            v.not_connected,
            ["effort", "battery", "sync", "lucid", "illness"]
        );
        assert!(preview_core("lucid", &snap(), &ok_msg("sleep"), &nolog)
            .await
            .is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn turning_on_needs_approval_and_pins_chat_and_kinds() {
        let d = tmp();
        let kc = keys();
        let r = enable_core(&d, &kc, |_, _| async {
            Err::<(), String>("declined: no".into())
        })
        .await;
        assert!(r.unwrap_err().starts_with("declined:"));
        let s = load_settings_in(&d);
        assert!(!s.enabled && s.chat_pin.is_none() && s.approved_kinds.is_empty());
        let asked = RefCell::new(false);
        let r = enable_core(&d, &FakeKeychain::default(), |_, _| {
            *asked.borrow_mut() = true;
            async { Ok::<(), String>(()) }
        })
        .await;
        assert!(r.is_err() && !*asked.borrow());
        let seen = RefCell::new((String::new(), vec![]));
        let s = enable_core(&d, &kc, |t, k| {
            *seen.borrow_mut() = (t, k);
            async { Ok::<(), String>(()) }
        })
        .await
        .unwrap();
        assert_eq!(seen.borrow().0, "5521");
        assert_eq!(seen.borrow().1, ["recovery", "sleep"]);
        assert!(s.enabled && s.approved_kinds == ["recovery", "sleep"]);
        let disk = std::fs::read_to_string(settings_path(&d)).unwrap();
        assert!(disk.contains(&chat_hash(CHAT)) && !disk.contains("SENTINEL"));
        let off = apply_patch(s, Some(false), None, true).unwrap();
        assert!(!off.enabled && off.chat_pin.is_none() && off.approved_kinds.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn changed_chat_or_grown_kinds_block_sends_until_reapproved() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        assert_eq!(
            dispatch_event(
                &never,
                &d,
                &kc,
                &t,
                &ok_msg("recovery"),
                &ev("recovery"),
                &nolog
            )
            .await
            .unwrap(),
            Dispatch::Sent
        );
        set_with(&kc, CHAT_KEY, "SENTINEL-other-chat-9".into()).unwrap();
        let err = dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
            .await
            .unwrap_err();
        assert!(err.starts_with("chat-changed:") && !err.contains("SENTINEL"));
        assert_eq!(t.count(), 1, "nothing went to the new chat");
        set_with(&kc, CHAT_KEY, CHAT.into()).unwrap();
        // a kind outside the approved list (as when CONNECTED grows) is refused
        let mut s = load_settings_in(&d);
        s.approved_kinds = vec!["recovery".into()];
        assert!(s.grew_since_approval());
        save_settings_in(&d, &s).unwrap();
        let err = dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
            .await
            .unwrap_err();
        assert!(err.starts_with("kinds-changed:"));
        assert_eq!(t.count(), 1);
        // no pin at all also blocks; re-approval pins and sends resume
        s.chat_pin = None;
        s.approved_kinds = vec!["sleep".into(), "recovery".into()];
        save_settings_in(&d, &s).unwrap();
        assert!(
            dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
                .await
                .is_err()
        );
        enable_core(&d, &kc, |_, _| async { Ok::<(), String>(()) })
            .await
            .unwrap();
        assert_eq!(
            dispatch_event(&never, &d, &kc, &t, &ok_msg("sleep"), &ev("sleep"), &nolog)
                .await
                .unwrap(),
            Dispatch::Sent
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn concurrent_dispatch_sends_once() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        let w = ok_msg("recovery");
        let e = ev("recovery");
        let (a, b, c) = tokio::join!(
            dispatch_event(&never, &d, &kc, &t, &w, &e, &nolog),
            dispatch_event(&never, &d, &kc, &t, &w, &e, &nolog),
            dispatch_event(&never, &d, &kc, &t, &w, &e, &nolog),
        );
        let sent = [a, b, c]
            .iter()
            .filter(|r| matches!(r, Ok(Dispatch::Sent)))
            .count();
        assert_eq!(sent, 1);
        assert_eq!(t.count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn failing_ledger_means_no_send_and_a_failed_send_releases_the_claim() {
        let d = tmp();
        on(&d, &[]);
        let kc = keys();
        let t = MockTg::new();
        // ledger path is a directory: unreadable, so fail closed
        std::fs::create_dir(ledger_path(&d)).unwrap();
        let r = dispatch_event(
            &never,
            &d,
            &kc,
            &t,
            &ok_msg("recovery"),
            &ev("recovery"),
            &nolog,
        )
        .await;
        assert_eq!(r.unwrap(), Dispatch::Skipped("ledger unavailable"));
        assert_eq!(t.count(), 0);
        std::fs::remove_dir(ledger_path(&d)).unwrap();
        // corrupt ledger: also closed
        std::fs::write(ledger_path(&d), "{not json").unwrap();
        let r = dispatch_event(
            &never,
            &d,
            &kc,
            &t,
            &ok_msg("recovery"),
            &ev("recovery"),
            &nolog,
        )
        .await;
        assert_eq!(r.unwrap(), Dispatch::Skipped("ledger unavailable"));
        assert_eq!(t.count(), 0);
        std::fs::remove_file(ledger_path(&d)).unwrap();
        // a failing send rolls the claim back; a retry then sends
        let mut bad = MockTg::new();
        bad.fail = true;
        assert!(dispatch_event(
            &never,
            &d,
            &kc,
            &bad,
            &ok_msg("recovery"),
            &ev("recovery"),
            &nolog
        )
        .await
        .is_err());
        assert!(load_ledger_strict(&d).unwrap().days.is_empty());
        assert_eq!(
            dispatch_event(
                &never,
                &d,
                &kc,
                &t,
                &ok_msg("recovery"),
                &ev("recovery"),
                &nolog
            )
            .await
            .unwrap(),
            Dispatch::Sent
        );
        assert_eq!(
            load_ledger_strict(&d).unwrap().days["recovery"],
            "2026-11-13"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn preview_sends_nothing_and_settings_file_is_private() {
        let t = MockTg::new();
        let p = preview_core("sleep", &snap(), &ok_msg("sleep"), &nolog)
            .await
            .unwrap();
        assert!(!p.used_plain_line && p.text == "You got 7h 32m of sleep.");
        assert_eq!(t.count(), 0);
        let d = tmp();
        save_settings_in(&d, &RelaySettings::default()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(settings_path(&d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings_path(&d)).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn send_body_has_no_parse_mode_and_no_link_preview() {
        let b = send_body("1", "hi");
        assert!(b.get("parse_mode").is_none());
        assert_eq!(b["link_preview_options"]["is_disabled"], true);
    }

    #[test]
    fn source_pins_the_client_the_host_and_the_last_check() {
        let src = include_str!("notify_relay.rs");
        let prod = &src[..src.find("\n#[cfg(test)]").unwrap()];
        // only host: telegram
        let mut hosts = vec![];
        for (i, _) in prod.match_indices("://") {
            let host: String = prod[i + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || ".-{}_".contains(*c))
                .collect();
            hosts.push(host);
        }
        assert!(!hosts.is_empty());
        // "" is the `"://"` literal in LINK_BITS (the blocklist), not a URL
        assert!(
            hosts.iter().all(|h| h.is_empty() || h == "{TELEGRAM_HOST}"),
            "{hosts:?}"
        );
        assert_eq!(hosts.iter().filter(|h| !h.is_empty()).count(), 1);
        assert_eq!(TELEGRAM_HOST, "api.telegram.org");
        // the HTTP client
        let imp = &prod[prod.find("impl TgTransport for HttpTelegram").unwrap()..];
        let imp = &imp[..imp.find("\nconst INSTRUCTION").unwrap()];
        for needle in [
            ".no_proxy()",
            ".https_only(true)",
            ".connect_timeout(std::time::Duration::from_secs(5))",
            "Policy::none()",
            ".timeout(std::time::Duration::from_secs(15))",
            "send_body(",
        ] {
            assert!(imp.contains(needle), "{needle}");
        }
        assert!(!prod.contains("danger_accept") && !imp.contains("parse_mode"));
        // the kill switch is the last statement before .send()
        let chk = imp.find("super::telegram::outbound_disabled()").unwrap();
        let send = imp.find(".send()").unwrap();
        assert!(chk < send);
        let between = &imp[chk..send];
        assert!(
            !between.contains(".await") && !between.contains("build("),
            "{between}"
        );
        assert_eq!(
            between.matches(';').count(),
            1, // the guard's own `return Err(OFF_MSG...);`
            "only the guard `if` sits between the check and the send: {between}"
        );
        // send_text re-checks after the Keychain reads
        let st = &prod[prod.find("async fn send_text").unwrap()..];
        let st = &st[..st.find("\n}\n").unwrap()];
        assert!(st.find("read_chat(kc)").unwrap() < st.find("off()").unwrap());
        assert!(st.find("off()").unwrap() < st.find("t.post(").unwrap());
        // the pin is checked on the value that is posted
        assert!(st.contains("chat_hash(&chat)") && st.contains("chat.trim()"));
        // send_url refuses a token that would change the URL
        assert!(send_url("abc@evil.example/").is_err());
        assert!(send_url("123:ABC")
            .unwrap()
            .starts_with("https://api.telegram.org/bot"));
    }
}
