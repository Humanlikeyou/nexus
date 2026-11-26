//! The inbox (decision 0045 item 2): Hermes owns the Telegram bot, so a reply reaches Hermes, and its
//! `nexus_inbox` MCP tools drop one small JSON file per reply into `<data>/inbox/`. This module
//! validates each file and applies it through the SAME code the app's own buttons use (check-in
//! save, habit mark, money entry add). Nothing here moves money, and nothing is ever deleted.
//!
//! File: `{"v":1,"kind":"checkin"|"habit"|"money","source":"telegram","at":ISO,"payload":{...}}`.
//!
//! Flow per file: `inbox/x.json` is renamed into `inbox/processing/` (so a second pass can never see
//! it), applied, then renamed into `inbox/done/` or `inbox/rejected/` (with a `.why.txt` reason). If
//! the app stops in between, the leftover in `processing/` is moved to `rejected/` at the next
//! start and is NOT applied again: a reply is never saved twice.
//!
//! Safety: the folder must be ours and not group/world writable; a symlink, a non-regular file, a
//! file owned by someone else or any dotfile is left alone; a file over 8 KB is rejected unread;
//! at most 50 files per pass.

use super::adds::{self, Dirs};
use super::goals::Goal;
use super::local_store::{day_of, iso, read_json, write_json_atomic};
use super::proactive::{remove_asks_in, sent_today_in, AskSent};
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_FILE: u64 = 8 * 1024;
const MAX_PER_PASS: usize = 50;
/// Entries looked at per pass: the rest wait for the next one, so a flood cannot grow memory.
const MAX_LISTED: usize = 1000;
/// Rejected files are kept (never deleted by Amara OS); ingest pauses when this many pile up.
const REJECTED_PAUSE: u32 = 1000;
const TICK: std::time::Duration = std::time::Duration::from_secs(30);
const STATE_MAX: u64 = 16 * 1024;
const COUNT_CAP: usize = 100_000;
/// Tauri event after at least one reply was applied, so open pages re-read their stores.
pub const APPLIED_EVENT: &str = "amara-inbox://applied";

static PASS: Mutex<()> = Mutex::new(());

pub(crate) fn inbox_dir(data: &Path) -> PathBuf {
    data.join("inbox")
}

// ---- envelope + payloads ----

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: u32,
    kind: String,
    source: String,
    at: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckinP {
    mood: i64,
    energy: i64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HabitP {
    habit: String,
    done: bool,
    #[serde(default)]
    day: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoneyP {
    kind: String,
    amount: f64,
    currency: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    date: String,
}

/// What an applied file did. `note` is a fixed code shown next to the result.
#[derive(Debug, PartialEq)]
pub(crate) struct Applied {
    pub kind: &'static str,
    pub note: Option<&'static str>,
}

type Rejected = String;

fn rej<T>(s: &str) -> Result<T, Rejected> {
    Err(s.to_string())
}

fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

// ---- appliers ----

fn apply_checkin(data: &Path, p: Value, now: DateTime<FixedOffset>) -> Result<Applied, Rejected> {
    let p: CheckinP = serde_json::from_value(p).or_else(|_| rej("checkin: bad payload"))?;
    if !(1..=5).contains(&p.mood) || !(1..=5).contains(&p.energy) {
        return rej("checkin: mood and energy must be 1 to 5");
    }
    if p.text.chars().count() > 2000 {
        return rej("checkin: text is over 2000 characters");
    }
    if p.tags.len() > 8 || p.tags.iter().any(|t| t.trim().chars().count() > 40) {
        return rej("checkin: too many or too long tags");
    }
    let tags: Vec<String> = p
        .tags
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let dir = super::checkin::dir_in(data);
    let had_today = super::checkin::done_on(&dir, day_of(&now)).unwrap_or(false);
    let draft = super::checkin::CheckInDraft {
        mood: p.mood as u8,
        energy: p.energy as u8,
        tags,
        text: p.text,
        transcribed: false,
    };
    super::checkin::save_in(&dir, &draft, now).or_else(|_| rej("checkin: could not be saved"))?;
    Ok(Applied {
        kind: "checkin",
        note: had_today.then_some("second_checkin_today"),
    })
}

const STOP: [&str; 11] = [
    "the", "my", "did", "do", "done", "i", "a", "an", "to", "of", "and",
];

fn words(s: &str) -> Vec<String> {
    let mut w: Vec<String> = s
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2 && !STOP.contains(w))
        .map(str::to_string)
        .collect();
    w.sort();
    w.dedup();
    w
}

/// One habit: exact id, else exact title (any case), else the single habit sharing the most title
/// words with the query. A paused habit is never chosen by words; asked for by id it is refused.
pub(crate) fn resolve_habit(goals: &[Goal], q: &str) -> Result<String, Rejected> {
    let q = q.trim();
    if q.is_empty() {
        return rej("habit: no habit named");
    }
    let habits: Vec<&Goal> = goals.iter().filter(|g| g.category == "habit").collect();
    if let Some(g) = habits.iter().find(|g| g.id == q) {
        return if g.paused {
            rej("habit: that habit is paused")
        } else {
            Ok(g.id.clone())
        };
    }
    let active: Vec<&&Goal> = habits.iter().filter(|g| !g.paused).collect();
    let exact: Vec<_> = active
        .iter()
        .filter(|g| g.title.trim().to_lowercase() == q.to_lowercase())
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0].id.clone());
    }
    if exact.len() > 1 {
        return rej("habit: more than one habit matches");
    }
    let qw = words(q);
    let scored: Vec<(usize, &Goal)> = active
        .iter()
        .map(|g| {
            let tw = words(&g.title);
            (qw.iter().filter(|w| tw.contains(w)).count(), **g)
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    let Some(top) = scored.iter().map(|(n, _)| *n).max() else {
        return rej("habit: no active habit matches");
    };
    let best: Vec<_> = scored.iter().filter(|(n, _)| *n == top).collect();
    if best.len() == 1 {
        Ok(best[0].1.id.clone())
    } else {
        rej("habit: more than one habit matches")
    }
}

fn apply_habit(data: &Path, p: Value, now: DateTime<FixedOffset>) -> Result<Applied, Rejected> {
    let p: HabitP = serde_json::from_value(p).or_else(|_| rej("habit: bad payload"))?;
    if p.habit.chars().count() > 120 {
        return rej("habit: name is too long");
    }
    let goals: Vec<Goal> = read_json(&data.join("goals.json"), adds::MAX_MD_BYTES)
        .or_else(|_| rej("habit: habits could not be read"))?;
    let id = resolve_habit(&goals, &p.habit)?;
    let day = if p.day.trim().is_empty() {
        day_key(day_of(&now))
    } else {
        p.day.trim().to_string()
    };
    let dirs = Dirs {
        data: data.to_path_buf(),
        workspace: super::openclaw::workspace_dir_in(data),
        legacy: data.join("legacy-unused"),
    };
    let r = super::habits::mark_in(&dirs, &id, &day, p.done, "telegram", now)?;
    Ok(Applied {
        kind: "habit",
        note: (!r.changed).then_some("already_marked"),
    })
}

fn apply_money(data: &Path, p: Value, now: DateTime<FixedOffset>) -> Result<Applied, Rejected> {
    let p: MoneyP = serde_json::from_value(p).or_else(|_| rej("money: bad payload"))?;
    if !matches!(
        p.kind.as_str(),
        "balance" | "income" | "expense" | "savings" | "bill"
    ) {
        return rej("money: kind must be balance, income, expense, savings or bill");
    }
    if !p.amount.is_finite() || p.amount <= 0.0 || p.amount > 1e9 {
        return rej("money: amount must be a number above 0 and up to 1e9");
    }
    let minor = match p.currency.as_str() {
        "JPY" => p.amount.round() as i64,
        "USD" => (p.amount * 100.0).round() as i64,
        _ => return rej("money: currency must be JPY or USD"),
    };
    if minor <= 0 {
        return rej("money: amount is too small");
    }
    if p.note.chars().count() > 300 {
        return rej("money: note is over 300 characters");
    }
    let category = p.category.trim().to_lowercase();
    if category.chars().count() > 40
        || (!category.is_empty() && !super::money_entries::CATEGORIES.contains(&category.as_str()))
    {
        return rej("money: unknown category");
    }
    let today = day_of(&now);
    let date = if p.date.trim().is_empty() {
        day_key(today)
    } else {
        match NaiveDate::parse_from_str(p.date.trim(), "%Y-%m-%d") {
            Ok(d) if (d - today).num_days().abs() <= 400 => day_key(d),
            Ok(_) => return rej("money: date is more than a year away"),
            Err(_) => return rej("money: date must be YYYY-MM-DD"),
        }
    };
    // The entry store keeps notes to 200 characters.
    let note: String = p.note.chars().take(200).collect();
    let draft = super::money_entries::EntryDraft {
        kind: p.kind.clone(),
        account_id: None,
        amount_minor: minor,
        currency: p.currency,
        due: (p.kind == "bill").then(|| date.clone()),
        date,
        recurrence: None,
        category: (!category.is_empty()).then_some(category),
        note: Some(note),
        source: Some("telegram".into()),
    };
    super::money_entries::entry_add_in(data, &draft, today)
        .map_err(|e| format!("money: not saved ({e})"))?;
    Ok(Applied {
        kind: "money",
        note: None,
    })
}

fn apply_in(data: &Path, env: Envelope, now: DateTime<FixedOffset>) -> Result<Applied, Rejected> {
    if env.v != 1 {
        return rej("unsupported version");
    }
    if env.source != "telegram" {
        return rej("unknown source");
    }
    if env.at.len() > 40 || DateTime::parse_from_rfc3339(&env.at).is_err() {
        return rej("bad timestamp");
    }
    match env.kind.as_str() {
        "checkin" => apply_checkin(data, env.payload, now),
        "habit" => apply_habit(data, env.payload, now),
        "money" => apply_money(data, env.payload, now),
        _ => rej("unknown kind"),
    }
}

// ---- files ----

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// A real directory (not a symlink) that is ours and not writable by group or others.
fn dir_is_safe(p: &Path) -> bool {
    fs::symlink_metadata(p)
        .map(|m| m.is_dir() && m.uid() == euid() && m.mode() & 0o022 == 0)
        .unwrap_or(false)
}

fn ensure_dir(p: &Path) -> bool {
    if !p.exists() && crate::app_paths::create_private_dir_all(p).is_err() {
        return false;
    }
    dir_is_safe(p)
}

/// Moves `from` into `<inbox>/<sub>/`, never overwriting, and writes a `.why.txt` for a reject.
fn file_away(inbox: &Path, from: &Path, sub: &str, why: Option<&str>, now: DateTime<FixedOffset>) {
    let dir = inbox.join(sub);
    if !ensure_dir(&dir) {
        return;
    }
    let name = from
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed.json")
        .to_string();
    let mut target = dir.join(&name);
    let mut n = 1;
    while fs::symlink_metadata(&target).is_ok() {
        target = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    if fs::rename(from, &target).is_err() {
        return;
    }
    if let Some(why) = why {
        let mut side = target.clone().into_os_string();
        side.push(".why.txt");
        let _ = crate::app_paths::write_private(
            &PathBuf::from(side),
            format!("{why}\n{}\n", iso(&now)),
        );
    }
}

/// Reads one file from `processing/` through a no-follow handle, checking the handle itself.
fn read_checked(p: &Path) -> Result<String, Rejected> {
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p)
        .or_else(|_| rej("not a regular file"))?;
    let m = f.metadata().or_else(|_| rej("unreadable"))?;
    if !m.is_file() || m.uid() != euid() {
        return rej("not a regular file of ours");
    }
    if m.len() > MAX_FILE {
        return rej("file is over 8 KB");
    }
    let mut s = String::new();
    f.take(MAX_FILE + 1)
        .read_to_string(&mut s)
        .or_else(|_| rej("not valid text"))?;
    Ok(s)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    last_applied: Option<(String, String)>,
    #[serde(default)]
    last_rejected: Option<(String, String)>,
    /// JST day the per-day counts belong to.
    #[serde(default)]
    day: String,
    /// Replies applied today per kind, for the daily caps.
    #[serde(default)]
    applied_today: std::collections::BTreeMap<String, u32>,
}

/// A hostile or confused sender must not flood the stores: replies applied per kind per day.
fn daily_cap(kind: &str) -> Option<u32> {
    match kind {
        "checkin" => Some(3),
        "habit" => Some(30),
        "money" => Some(20),
        _ => None,
    }
}

fn applied_today(data: &Path, kind: &str, now: DateTime<FixedOffset>) -> u32 {
    let st: StateFile = read_json(&state_path(data), STATE_MAX).unwrap_or_default();
    if st.day == day_key(day_of(&now)) {
        st.applied_today.get(kind).copied().unwrap_or(0)
    } else {
        0
    }
}

fn state_path(data: &Path) -> PathBuf {
    data.join("inbox_state.json")
}

fn bump_state(
    data: &Path,
    applied: Option<&str>,
    rejected: Option<&str>,
    now: DateTime<FixedOffset>,
) {
    let mut st: StateFile = read_json(&state_path(data), STATE_MAX).unwrap_or_default();
    let today = day_key(day_of(&now));
    if st.day != today {
        st.day = today;
        st.applied_today.clear();
    }
    if let Some(k) = applied {
        st.last_applied = Some((k.to_string(), iso(&now)));
        *st.applied_today.entry(k.to_string()).or_insert(0) += 1;
    }
    if let Some(r) = rejected {
        st.last_rejected = Some((r.chars().take(120).collect(), iso(&now)));
    }
    let _ = write_json_atomic(&state_path(data), &st, STATE_MAX);
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct PassReport {
    pub applied: Vec<&'static str>,
    pub rejected: usize,
    pub skipped: usize,
}

/// After a stop in the middle of an apply: whatever is left in `processing/` goes to `rejected/`.
/// It is never applied again (it may already have been saved; the reason says so).
pub(crate) fn recover_processing_in(data: &Path, now: DateTime<FixedOffset>) -> usize {
    let inbox = inbox_dir(data);
    let proc = inbox.join("processing");
    let Ok(rd) = fs::read_dir(&proc) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        file_away(
            &inbox,
            &p,
            "rejected",
            Some("interrupted: the app stopped while applying this; it may or may not have been saved, so check the app. It was not applied again."),
            now,
        );
        n += 1;
    }
    n
}

pub(crate) fn ingest_pass_in(data: &Path, now: DateTime<FixedOffset>) -> PassReport {
    let _g = PASS.lock().unwrap_or_else(|p| p.into_inner());
    let mut report = PassReport::default();
    let inbox = inbox_dir(data);
    if !inbox.exists() || !dir_is_safe(&inbox) {
        return report;
    }
    let proc = inbox.join("processing");
    if !ensure_dir(&proc) {
        return report;
    }
    if count_json(&inbox.join("rejected")) >= REJECTED_PAUSE {
        bump_state(
            data,
            None,
            Some("rejected folder is full: review it and clear it, ingest is paused"),
            now,
        );
        return report;
    }
    let Ok(rd) = fs::read_dir(&inbox) else {
        return report;
    };
    let mut names: Vec<(String, u64)> = Vec::new();
    for e in rd.flatten().take(MAX_LISTED) {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        match fs::symlink_metadata(e.path()) {
            Ok(m) if m.file_type().is_file() && m.uid() == euid() => names.push((name, m.len())),
            _ => report.skipped += 1,
        }
    }
    names.sort();
    for (name, size) in names.into_iter().take(MAX_PER_PASS) {
        let from = inbox.join(&name);
        if size > MAX_FILE {
            file_away(&inbox, &from, "rejected", Some("file is over 8 KB"), now);
            bump_state(data, None, Some("file is over 8 KB"), now);
            report.rejected += 1;
            continue;
        }
        let held = proc.join(&name);
        if fs::rename(&from, &held).is_err() {
            report.skipped += 1;
            continue;
        }
        let result = read_checked(&held).and_then(|text| {
            let env: Envelope =
                serde_json::from_str(&text).or_else(|_| rej("not valid JSON for this format"))?;
            if let Some(cap) = daily_cap(&env.kind) {
                if applied_today(data, &env.kind, now) >= cap {
                    return rej("daily limit reached for this kind of reply");
                }
            }
            apply_in(data, env, now)
        });
        match result {
            Ok(a) => {
                file_away(&inbox, &held, "done", None, now);
                let _ = remove_asks_in(data, a.kind, now);
                bump_state(data, Some(a.kind), None, now);
                println!(
                    "[inbox] applied {}{}",
                    a.kind,
                    a.note.map(|n| format!(" ({n})")).unwrap_or_default()
                );
                report.applied.push(a.kind);
            }
            Err(why) => {
                file_away(&inbox, &held, "rejected", Some(&why), now);
                bump_state(data, None, Some(&why), now);
                println!("[inbox] rejected one file");
                report.rejected += 1;
            }
        }
    }
    report
}

#[derive(Clone, Serialize)]
struct AppliedEvent {
    kinds: Vec<String>,
}

/// Started once from `setup`: crash recovery first, then a pass now and every 30 s.
pub fn start(app: tauri::AppHandle) {
    use tauri::Emitter;
    tauri::async_runtime::spawn(async move {
        let data = crate::app_paths::data_dir();
        let d = data.clone();
        let _ = tokio::task::spawn_blocking(move || {
            recover_processing_in(&d, super::shared::time::now_jst())
        })
        .await;
        loop {
            let d = data.clone();
            if let Ok(rep) = tokio::task::spawn_blocking(move || {
                ingest_pass_in(&d, super::shared::time::now_jst())
            })
            .await
            {
                if !rep.applied.is_empty() {
                    let _ = app.emit(
                        APPLIED_EVENT,
                        AppliedEvent {
                            kinds: rep.applied.iter().map(|k| k.to_string()).collect(),
                        },
                    );
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

// ---- status ----

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxApplied {
    pub kind: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxStatus {
    pub done: u32,
    pub rejected: u32,
    pub last_applied: Option<InboxApplied>,
    pub last_rejected_reason: Option<String>,
    pub sent_today: Vec<AskSent>,
}

fn count_json(dir: &Path) -> u32 {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .take(COUNT_CAP)
                .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
                .count() as u32
        })
        .unwrap_or(0)
}

pub(crate) fn status_in(data: &Path, now: DateTime<FixedOffset>) -> InboxStatus {
    let inbox = inbox_dir(data);
    let st: StateFile = read_json(&state_path(data), STATE_MAX).unwrap_or_default();
    InboxStatus {
        done: count_json(&inbox.join("done")),
        rejected: count_json(&inbox.join("rejected")),
        last_applied: st.last_applied.map(|(kind, at)| InboxApplied { kind, at }),
        last_rejected_reason: st.last_rejected.map(|(r, _)| r),
        sent_today: sent_today_in(data, day_of(&now)),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn inbox_status() -> Result<InboxStatus, String> {
    super::shared::task::blocking(super::shared::task::JoinErr::Plain, || {
        Ok(status_in(
            &crate::app_paths::data_dir(),
            super::shared::time::now_jst(),
        ))
    })
    .await
}

#[cfg(test)]
mod tests;
