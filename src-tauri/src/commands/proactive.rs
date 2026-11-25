//! Telegram check-ins that Nexus sends itself (decision 0045 item 2): the daily check-in, a habit
//! reminder and the weekly money check-in. Hermes owns the bot and receives the replies; its
//! `nexus_inbox` tools drop files that `inbox.rs` applies. Nexus only asks.
//!
//! - Fixed text templates, no LLM, no numbers from his data. Plain text (no parse_mode).
//! - One send per kind per day (money: per week). The kill switch (`telegram::outbound_disabled`)
//!   turns a due send into "suppressed" once: it is recorded and never retried that day, so there is
//!   no error loop. A failed send is retried once, 30 minutes later.
//! - Each sent question is also written to `<data>/asks/open.json` (0600, entries older than 36 h
//!   dropped on every write) so Hermes' `open_asks` can tell what a short reply answers.
//! - Config `<data>/proactive.json`, once-a-day state `<data>/proactive_state.json`.

use super::local_store::{day_of, iso, read_json, write_json_atomic};
use super::needs_you::{habits_now, HabitIn, HABIT_DEFAULT_REMIND};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 64 * 1024;
const ASK_TTL_HOURS: i64 = 36;
const RETRY_AFTER_MIN: i64 = 30;
const MAX_ATTEMPTS: u32 = 2;
const TICK: std::time::Duration = std::time::Duration::from_secs(60);
const FIRST_DELAY: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_ASKS: usize = 10;

pub(crate) const CHECKIN_TEXT: &str = "Hi Taisun, how are you doing today? Reply with how you feel and your energy, in your own words.";
pub(crate) const MONEY_TEXT: &str = "Quick money check-in: how much do you have now, and anything new in income or spending this week?";

static LOCK: Mutex<()> = Mutex::new(());

// ---- settings ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProactiveCheckin {
    pub enabled: bool,
    pub at: String,
}
impl Default for ProactiveCheckin {
    fn default() -> Self {
        Self {
            enabled: true,
            at: "22:00".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProactiveHabits {
    pub enabled: bool,
}
impl Default for ProactiveHabits {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProactiveMoney {
    pub enabled: bool,
    pub weekday: String,
    pub at: String,
}
impl Default for ProactiveMoney {
    fn default() -> Self {
        Self {
            enabled: true,
            weekday: "Sun".into(),
            at: "19:00".into(),
        }
    }
}

/// Life suggestions brought up by Amara (decision 0056; rules in `life_nudges.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProactiveNudges {
    pub enabled: bool,
    /// Also send them on Telegram (the kill switch still applies).
    pub telegram: bool,
    /// New suggestions a JST day, app and Telegram together (0-10).
    pub max_per_day: u32,
    /// Quiet hours (JST, `HH:MM`); equal ends mean none.
    pub quiet_from: String,
    pub quiet_to: String,
}
impl Default for ProactiveNudges {
    fn default() -> Self {
        Self {
            enabled: true,
            telegram: true,
            max_per_day: 2,
            quiet_from: "22:00".into(),
            quiet_to: "07:00".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ProactiveSettings {
    pub checkin: ProactiveCheckin,
    pub habits: ProactiveHabits,
    pub money: ProactiveMoney,
    /// Older files have no `nudges` and read as the defaults.
    pub nudges: ProactiveNudges,
}

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("Mon", Weekday::Mon),
    ("Tue", Weekday::Tue),
    ("Wed", Weekday::Wed),
    ("Thu", Weekday::Thu),
    ("Fri", Weekday::Fri),
    ("Sat", Weekday::Sat),
    ("Sun", Weekday::Sun),
];

fn weekday_of(s: &str) -> Option<Weekday> {
    WEEKDAYS.iter().find(|(n, _)| *n == s).map(|(_, w)| *w)
}

/// Validates times (`HH:MM`) and the weekday; returns the normalised settings.
pub(crate) fn validate(s: &ProactiveSettings) -> Result<ProactiveSettings, String> {
    let bad = || "invalid input".to_string();
    let mut out = s.clone();
    out.checkin.at = super::goals::valid_hhmm(&s.checkin.at).ok_or_else(bad)?;
    out.money.at = super::goals::valid_hhmm(&s.money.at).ok_or_else(bad)?;
    weekday_of(&s.money.weekday).ok_or_else(bad)?;
    out.nudges.quiet_from = super::goals::valid_hhmm(&s.nudges.quiet_from).ok_or_else(bad)?;
    out.nudges.quiet_to = super::goals::valid_hhmm(&s.nudges.quiet_to).ok_or_else(bad)?;
    if s.nudges.max_per_day > 10 {
        return Err(bad());
    }
    Ok(out)
}

fn settings_file(data: &Path) -> PathBuf {
    data.join("proactive.json")
}
fn state_file(data: &Path) -> PathBuf {
    data.join("proactive_state.json")
}

/// Stored settings; a missing or hand-broken file reads as the defaults.
pub(crate) fn load_settings_in(data: &Path) -> ProactiveSettings {
    read_json::<ProactiveSettings>(&settings_file(data), MAX_BYTES)
        .ok()
        .and_then(|s| validate(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_settings_in(
    data: &Path,
    s: &ProactiveSettings,
) -> Result<ProactiveSettings, String> {
    let v = validate(s)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    write_json_atomic(&settings_file(data), &v, MAX_BYTES)?;
    Ok(v)
}

// ---- asks file ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ask {
    pub kind: String,
    pub text: String,
    pub at: String,
}

fn asks_file(data: &Path) -> PathBuf {
    data.join("asks").join("open.json")
}

fn fresh(a: &Ask, now: DateTime<FixedOffset>) -> bool {
    DateTime::parse_from_rfc3339(&a.at).is_ok_and(|t| now - t < Duration::hours(ASK_TTL_HOURS))
}

pub(crate) fn read_asks_in(data: &Path) -> Vec<Ask> {
    read_json::<Vec<Ask>>(&asks_file(data), MAX_BYTES).unwrap_or_default()
}

/// Adds (or replaces) the open question of this kind and drops everything older than 36 h.
pub(crate) fn write_ask_in(
    data: &Path,
    kind: &str,
    text: &str,
    now: DateTime<FixedOffset>,
) -> Result<(), String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut asks: Vec<Ask> = read_asks_in(data)
        .into_iter()
        .filter(|a| a.kind != kind && fresh(a, now))
        .collect();
    asks.push(Ask {
        kind: kind.into(),
        text: text.into(),
        at: iso(&now),
    });
    let skip = asks.len().saturating_sub(MAX_ASKS);
    let asks: Vec<Ask> = asks.into_iter().skip(skip).collect();
    write_json_atomic(&asks_file(data), &asks, MAX_BYTES)
}

/// The reply was applied: that question is no longer open. Expired entries go too.
pub(crate) fn remove_asks_in(
    data: &Path,
    kind: &str,
    now: DateTime<FixedOffset>,
) -> Result<(), String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !asks_file(data).exists() {
        return Ok(());
    }
    let asks: Vec<Ask> = read_asks_in(data)
        .into_iter()
        .filter(|a| a.kind != kind && fresh(a, now))
        .collect();
    write_json_atomic(&asks_file(data), &asks, MAX_BYTES)
}

// ---- state + decision ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Checkin,
    Habits,
    Money,
}
impl Kind {
    fn ask_kind(self) -> &'static str {
        match self {
            Kind::Checkin => "checkin",
            Kind::Habits => "habit",
            Kind::Money => "money",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Outcome {
    Sent,
    Suppressed,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct KindState {
    /// JST day of the last attempt.
    #[serde(default)]
    pub day: Option<String>,
    /// "sent", "suppressed" or "failed".
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_try: Option<String>,
    #[serde(default)]
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct State {
    #[serde(default)]
    pub checkin: KindState,
    #[serde(default)]
    pub habits: KindState,
    #[serde(default)]
    pub money: KindState,
}
impl State {
    fn of(&mut self, k: Kind) -> &mut KindState {
        match k {
            Kind::Checkin => &mut self.checkin,
            Kind::Habits => &mut self.habits,
            Kind::Money => &mut self.money,
        }
    }
    fn get(&self, k: Kind) -> &KindState {
        match k {
            Kind::Checkin => &self.checkin,
            Kind::Habits => &self.habits,
            Kind::Money => &self.money,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Decision {
    Wait,
    Send,
    /// Due, but the kill switch is on: record it, send nothing, do not retry today.
    Suppress,
}

pub(crate) struct DecideIn<'a> {
    pub now: DateTime<FixedOffset>,
    pub enabled: bool,
    /// `HH:MM` JST the question may go out from; `None` = any time of day (habits: each habit has its own).
    pub at: Option<&'a str>,
    /// Money only: the one weekday it goes out on, asked once per week.
    pub weekday: Option<Weekday>,
    /// There is something to ask about (not checked in yet, habits due, money stale).
    pub needed: bool,
    pub state: &'a KindState,
    pub disabled: bool,
}

fn minutes_of(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

fn week_start(d: NaiveDate) -> NaiveDate {
    super::habits::week_start(d)
}

pub(crate) fn decide(i: &DecideIn) -> Decision {
    if !i.enabled || !i.needed {
        return Decision::Wait;
    }
    if let Some(at) = i.at {
        match minutes_of(at) {
            Some(m) if i.now.hour() * 60 + i.now.minute() >= m => {}
            _ => return Decision::Wait,
        }
    }
    if i.weekday.is_some_and(|w| w != i.now.weekday()) {
        return Decision::Wait;
    }
    let today = day_of(&i.now);
    let st = i.state;
    let concluded = matches!(st.result.as_deref(), Some("sent" | "suppressed"));
    if i.weekday.is_some() && concluded {
        let same_week = st
            .day
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .is_some_and(|d| week_start(d) == week_start(today));
        if same_week {
            return Decision::Wait;
        }
    }
    if st.day.as_deref() == Some(today.format("%Y-%m-%d").to_string().as_str()) {
        if concluded {
            return Decision::Wait;
        }
        if st.attempts >= MAX_ATTEMPTS {
            return Decision::Wait;
        }
        let recent = st
            .last_try
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .is_some_and(|t| i.now - t < Duration::minutes(RETRY_AFTER_MIN));
        if recent {
            return Decision::Wait;
        }
    }
    if i.disabled {
        Decision::Suppress
    } else {
        Decision::Send
    }
}

pub(crate) fn record_outcome(st: &mut KindState, now: DateTime<FixedOffset>, o: Outcome) {
    let today = day_of(&now).format("%Y-%m-%d").to_string();
    if st.day.as_deref() != Some(today.as_str()) {
        st.attempts = 0;
    }
    st.day = Some(today);
    st.attempts += 1;
    st.last_try = Some(iso(&now));
    st.result = Some(
        match o {
            Outcome::Sent => "sent",
            Outcome::Suppressed => "suppressed",
            Outcome::Failed => "failed",
        }
        .into(),
    );
    if o == Outcome::Sent {
        st.sent_at = Some(iso(&now));
    }
}

fn load_state(data: &Path) -> State {
    read_json::<State>(&state_file(data), MAX_BYTES).unwrap_or_default()
}

/// Habits whose reminder time has passed, are active and are unmarked today (the Needs-you rule).
pub(crate) fn due_habit_titles(hs: &[HabitIn], now: DateTime<FixedOffset>) -> Vec<String> {
    let minutes = now.hour() * 60 + now.minute();
    hs.iter()
        .filter(|h| !h.paused && !h.marked_today)
        .filter(|h| {
            let at = h.remind_at.as_deref().unwrap_or(HABIT_DEFAULT_REMIND);
            super::goals::valid_hhmm(at)
                .or_else(|| super::goals::valid_hhmm(HABIT_DEFAULT_REMIND))
                .and_then(|t| minutes_of(&t))
                .is_some_and(|m| minutes >= m)
        })
        .map(|h| h.title.clone())
        .collect()
}

/// One short message for every due habit: at most 5 names of at most 40 characters.
pub(crate) fn habit_text(titles: &[String]) -> String {
    let names: Vec<String> = titles
        .iter()
        .take(5)
        .map(|t| {
            super::local_store::clean_text(t, 1, 400)
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect();
    let more = titles.len().saturating_sub(5);
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        "Did you do: {}{tail}? Reply and I will mark them.",
        names.join(", ")
    )
}

pub(crate) struct Planned {
    pub kind: Kind,
    pub decision: Decision,
    pub text: String,
}

/// Which questions are due at `now`. The cheap pass (time, weekday, state) runs first; a store is
/// read only for a question that is due by time.
pub(crate) fn plan_in(data: &Path, now: DateTime<FixedOffset>, disabled: bool) -> Vec<Planned> {
    let cfg = load_settings_in(data);
    let state = load_state(data);
    let today = day_of(&now);
    let mut out = Vec::new();
    let mut consider = |kind: Kind,
                        enabled: bool,
                        at: Option<&str>,
                        weekday: Option<Weekday>,
                        text: &dyn Fn() -> Option<String>| {
        let d = DecideIn {
            now,
            enabled,
            at,
            weekday,
            needed: true,
            state: state.get(kind),
            disabled,
        };
        if decide(&d) == Decision::Wait {
            return;
        }
        let Some(text) = text() else { return };
        out.push(Planned {
            kind,
            decision: decide(&d),
            text,
        });
    };
    consider(
        Kind::Checkin,
        cfg.checkin.enabled,
        Some(cfg.checkin.at.as_str()),
        None,
        &|| {
            (!super::checkin::done_on(&super::checkin::dir_in(data), today).unwrap_or(true))
                .then(|| CHECKIN_TEXT.to_string())
        },
    );
    consider(Kind::Habits, cfg.habits.enabled, None, None, &|| {
        let hs = habits_now(data, today).ok()?;
        let due = due_habit_titles(&hs, now);
        (!due.is_empty()).then(|| habit_text(&due))
    });
    consider(
        Kind::Money,
        cfg.money.enabled,
        Some(cfg.money.at.as_str()),
        weekday_of(&cfg.money.weekday),
        &|| super::money_entries::money_checkin_due(data, today).then(|| MONEY_TEXT.to_string()),
    );
    out
}

/// Plan, send each due question through `send` (a closure: tests never touch Telegram), record the
/// outcome and, for a sent question, write its ask.
/// In-memory send guard: if the state file cannot be written, a sent question would otherwise go out again
/// every minute. Keyed by data dir and kind; `i64::MAX` holds the kind until the app restarts.
static TRIED: Mutex<Vec<(PathBuf, &'static str, i64)>> = Mutex::new(Vec::new());
const RETRY_GUARD_SECS: i64 = 25 * 60;

fn tried_recently(data: &Path, kind: Kind, now: DateTime<FixedOffset>) -> bool {
    let g = TRIED.lock().unwrap_or_else(|e| e.into_inner());
    g.iter().any(|(d, k, t)| {
        d == data
            && *k == kind.ask_kind()
            && (*t == i64::MAX || (now.timestamp() - *t).abs() < RETRY_GUARD_SECS)
    })
}

fn mark_tried(data: &Path, kind: Kind, at: i64) {
    let mut g = TRIED.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|(d, k, _)| !(d == data && *k == kind.ask_kind()));
    g.push((data.to_path_buf(), kind.ask_kind(), at));
}

pub(crate) async fn run_in<F, Fut>(
    data: &Path,
    now: DateTime<FixedOffset>,
    disabled: bool,
    send: F,
) -> Vec<(Kind, Outcome)>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Outcome>,
{
    let mut done = Vec::new();
    for p in plan_in(data, now, disabled) {
        let outcome = match p.decision {
            Decision::Wait => continue,
            Decision::Suppress => Outcome::Suppressed,
            Decision::Send => {
                if tried_recently(data, p.kind, now) {
                    continue;
                }
                let o = send(p.text.clone()).await;
                mark_tried(data, p.kind, now.timestamp());
                o
            }
        };
        {
            let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = load_state(data);
            record_outcome(state.of(p.kind), now, outcome);
            if write_json_atomic(&state_file(data), &state, MAX_BYTES).is_err() {
                // Without the record it would retry every minute: hold this kind until the app restarts.
                mark_tried(data, p.kind, i64::MAX);
                break;
            }
        }
        if outcome == Outcome::Sent {
            let _ = write_ask_in(data, p.kind.ask_kind(), &p.text, now);
        }
        println!("[proactive] {} {outcome:?}", p.kind.ask_kind());
        done.push((p.kind, outcome));
    }
    done
}

/// Started once from `setup`: a first look 15 s after launch (catch-up), then every minute.
pub fn start() {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            let data = crate::app_paths::data_dir();
            let disabled = super::telegram::outbound_disabled();
            run_in(
                &data,
                super::shared::time::now_jst(),
                disabled,
                |text| async move {
                    match super::telegram::send_plain_gated(
                        text,
                        super::telegram::outbound_disabled(),
                    )
                    .await
                    {
                        Ok(()) => Outcome::Sent,
                        Err(_) => Outcome::Failed,
                    }
                },
            )
            .await;
            let nudges = load_settings_in(&data).nudges;
            super::life_nudges::telegram_in(
                &data,
                &nudges,
                super::shared::time::now_jst(),
                super::telegram::outbound_disabled(),
                |text| async move {
                    match super::telegram::send_plain_gated(
                        text,
                        super::telegram::outbound_disabled(),
                    )
                    .await
                    {
                        Ok(()) => super::life_nudges::TgOutcome::Sent,
                        Err(_) => super::life_nudges::TgOutcome::Failed,
                    }
                },
            )
            .await;
            tokio::time::sleep(TICK).await;
        }
    });
}

// ---- status for the UI ----

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AskSent {
    /// "checkin", "habit" or "money".
    pub kind: String,
    pub at: String,
    /// "sent", "suppressed" or "failed".
    pub result: String,
}

/// What was tried today (JST); money shows its last try of the week.
pub(crate) fn sent_today_in(data: &Path, today: NaiveDate) -> Vec<AskSent> {
    let st = load_state(data);
    let key = today.format("%Y-%m-%d").to_string();
    [Kind::Checkin, Kind::Habits, Kind::Money]
        .into_iter()
        .filter_map(|k| {
            let s = st.get(k);
            if s.day.as_deref() != Some(key.as_str()) {
                return None;
            }
            Some(AskSent {
                kind: k.ask_kind().into(),
                at: s.last_try.clone()?,
                result: s.result.clone()?,
            })
        })
        .collect()
}

// ---- commands ----

#[tauri::command]
#[specta::specta]
pub async fn proactive_settings_get() -> Result<ProactiveSettings, String> {
    super::shared::task::blocking(super::shared::task::JoinErr::Plain, || {
        Ok(load_settings_in(&crate::app_paths::data_dir()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn proactive_settings_set(
    settings: ProactiveSettings,
) -> Result<ProactiveSettings, String> {
    super::shared::task::blocking(super::shared::task::JoinErr::Plain, move || {
        save_settings_in(&crate::app_paths::data_dir(), &settings)
    })
    .await
}

#[cfg(test)]
mod tests;
