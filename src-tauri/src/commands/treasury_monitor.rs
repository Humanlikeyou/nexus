//! Treasury, phase 5 (part 1): the exception monitor (plan `tasks/plans/treasury.md`, answers
//! `docs/decisions/0032-treasury-answers-2026-11-15.md` #7 and #9).
//!
//! A nightly in-app job (same catch-up pattern as `backup_schedule.rs`) and a manual "run now" read
//! the Treasury database and keep a small table of flags. Only these six flags exist:
//! `stale_data`, `failed_import`, `income_down`, `no_income`, `results_spike`, `no_conversions`.
//!
//! Rules this file keeps (each has a test):
//! - Stale data is its own flag and is shown first. It fires when an active stream has had no
//!   successful import for N days (default 7, a per-stream override), counted from the last good import
//!   or, for a stream never imported, from the day it was added. While a stream is stale its income
//!   flags are not raised (the numbers are not trustworthy), and the status is never "ok".
//! - With no flags and every active stream imported, the status is `ok` with the date of the OLDEST
//!   last import ("ok as of <date>"). There is no bare all-clear: no import means `no_data`.
//! - Thresholds are strict where the plan says "more than" (income down over 30%) and inclusive where
//!   it says "N days" (stale at N days of silence, no income at 14 days, no conversions at 30 days).
//! - A missing figure is never 0: a stream, month or metric without data raises no flag of its own.
//! - De-duplication: a flag row is keyed by kind and subject, keeps its `since` while the condition
//!   holds, and is deleted when the condition clears. A flag is relayed at most once (`alerted`); an
//!   acknowledged flag is never relayed. Flags live in `treasury.db`, so they survive a restart.
//! - No flags (or none new) means no notification. A new flag becomes a Needs-you pointer (read by
//!   `needs_you.rs` from this table) and, only when outbound is allowed and the relay has approved the
//!   `treasury` kind, ONE fixed plain line (`notify_relay::TREASURY_LINE`): no amounts, no names, no
//!   digits. Nothing here formats a value into relay text.
//! - The scheduler is started from `lib.rs` `setup` only; no test starts it.
//!
//! Outside calls from this file: none. The relay call goes through `notify_relay`, which keeps its own
//! kill switch, approvals and the single Telegram host.

use super::backup_schedule::{
    busy_now, prev_slot, read_battery, slot_missed, Battery, Decision, STARTUP_DELAY,
};
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst};
use super::treasury::{db_path, is_date, mean_minor, monthly_totals, now_iso, open_db, store_err};
use chrono::{DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The nightly slot, JST. Backups run at 03:00 by default; this runs after them.
pub(crate) const NIGHTLY_AT: &str = "04:00";
const RETRY_AFTER: Duration = Duration::hours(1);
const MAX_ATTEMPTS: u32 = 3;
const TICK: std::time::Duration = std::time::Duration::from_secs(60);
const SKIP_ON_BATTERY_BELOW: u32 = 20;
/// A bounce/unsubscribe rate needs at least this many sends in the window before it means anything.
const MIN_SENDS: i64 = 50;
/// The spike window is the 7 result days ending at the newest imported day, if that day is this recent.
const SPIKE_WINDOW_DAYS: i64 = 7;
const RESULTS_RECENT_DAYS: i64 = 30;
const MAX_DAYS: u32 = 365;
const MAX_OVERRIDES: usize = 100;

pub(crate) const KIND_ORDER: [&str; 6] = [
    "stale_data",
    "failed_import",
    "income_down",
    "no_income",
    "results_spike",
    "no_conversions",
];

// ───────────────────────────── types ─────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryFlag", rename_all = "camelCase")]
pub struct Flag {
    /// Stable while the condition holds: `kind:subject`.
    pub id: String,
    /// One of `stale_data | failed_import | income_down | no_income | results_spike | no_conversions`.
    pub kind: String,
    pub stream_id: Option<String>,
    pub sequence_id: Option<String>,
    /// When the flag was first raised (ISO, JST).
    pub since: String,
    /// A short fixed sentence with day counts or percentages only. Never an amount or a name.
    pub detail: String,
    pub acked: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryMonitorStatus", rename_all = "camelCase")]
pub struct MonitorStatus {
    /// `stale | flags | ok | no_data`. Stale wins over everything; `ok` only ever comes with `as_of`.
    pub state: String,
    /// The oldest last-import date (YYYY-MM-DD) across active streams. Set only for `ok`.
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryRunResult", rename_all = "camelCase")]
pub struct RunResult {
    pub ran_at: String,
    pub flags: i64,
    pub new_flags: i64,
    pub cleared: i64,
    pub status: MonitorStatus,
    /// `none` (nothing new to say) | `sent` | `skipped` (relay off, kind off or already sent today) |
    /// `off` (outbound disabled) | `failed`.
    pub relay: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryMonitorSettings", rename_all = "camelCase", default)]
pub struct MonitorSettings {
    pub nightly: bool,
    /// Days without a successful import before a stream is stale (decision 0032 #7: 7).
    pub stale_days: u32,
    /// Per-stream override of `stale_days`, by stream id.
    pub stale_days_by_stream: BTreeMap<String, u32>,
    /// Income down by MORE than this percent against the trailing average.
    pub income_drop_pct: u32,
    pub no_income_days: u32,
    pub no_conversion_days: u32,
    /// Bounces plus unsubscribes above this percent of sends.
    pub spike_pct: u32,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            nightly: true,
            stale_days: 7,
            stale_days_by_stream: BTreeMap::new(),
            income_drop_pct: 30,
            no_income_days: 14,
            no_conversion_days: 30,
            spike_pct: 5,
        }
    }
}

#[derive(Debug, Deserialize, Default, specta::Type)]
#[serde(
    rename = "TreasuryMonitorSettingsPatch",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct MonitorSettingsPatch {
    pub nightly: Option<bool>,
    pub stale_days: Option<u32>,
    /// Replaces the whole override map.
    pub stale_days_by_stream: Option<BTreeMap<String, u32>>,
    pub income_drop_pct: Option<u32>,
    pub no_income_days: Option<u32>,
    pub no_conversion_days: Option<u32>,
    pub spike_pct: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryMonitorView", rename_all = "camelCase")]
pub struct MonitorView {
    pub settings: MonitorSettings,
    pub status: MonitorStatus,
    pub last_run_at: Option<String>,
    /// `ok`, `skipped_busy`, `skipped_battery` or an error code.
    pub last_run_result: Option<String>,
}

// ───────────────────────────── state and settings ─────────────────────────────

fn state_get(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM monitor_state WHERE key = ?1",
        [key],
        |r| r.get(0),
    )
    .optional()
    .map_err(store_err)
}

fn state_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO monitor_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(store_err)?;
    Ok(())
}

fn state_del(conn: &Connection, key: &str) -> Result<(), String> {
    conn.execute("DELETE FROM monitor_state WHERE key = ?1", [key])
        .map_err(store_err)?;
    Ok(())
}

/// Missing or unreadable settings are the defaults (they only hold thresholds).
fn settings_in(conn: &Connection) -> Result<MonitorSettings, String> {
    Ok(state_get(conn, "settings")?
        .and_then(|j| serde_json::from_str::<MonitorSettings>(&j).ok())
        .unwrap_or_default())
}

fn in_range(v: u32, lo: u32, hi: u32) -> Result<u32, String> {
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err("invalid_input".into())
    }
}

fn settings_set_in(data: &Path, patch: MonitorSettingsPatch) -> Result<MonitorView, String> {
    let conn = open_db(data)?;
    let mut s = settings_in(&conn)?;
    if let Some(v) = patch.nightly {
        s.nightly = v;
    }
    if let Some(v) = patch.stale_days {
        s.stale_days = in_range(v, 1, MAX_DAYS)?;
    }
    if let Some(map) = patch.stale_days_by_stream {
        if map.len() > MAX_OVERRIDES {
            return Err("invalid_input".into());
        }
        for (id, days) in &map {
            in_range(*days, 1, MAX_DAYS)?;
            let known: Option<i64> = conn
                .query_row("SELECT 1 FROM streams WHERE id = ?1", [id], |r| r.get(0))
                .optional()
                .map_err(store_err)?;
            if known.is_none() {
                return Err("unknown_stream".into());
            }
        }
        s.stale_days_by_stream = map;
    }
    if let Some(v) = patch.income_drop_pct {
        s.income_drop_pct = in_range(v, 1, 99)?;
    }
    if let Some(v) = patch.no_income_days {
        s.no_income_days = in_range(v, 1, MAX_DAYS)?;
    }
    if let Some(v) = patch.no_conversion_days {
        s.no_conversion_days = in_range(v, 1, MAX_DAYS)?;
    }
    if let Some(v) = patch.spike_pct {
        s.spike_pct = in_range(v, 1, 100)?;
    }
    let json = serde_json::to_string(&s).map_err(store_err)?;
    state_set(&conn, "settings", &json)?;
    view_in(&conn, today_of(&now_jst()))
}

fn today_of(now: &DateTime<FixedOffset>) -> NaiveDate {
    day_key(now)
}

fn view_in(conn: &Connection, today: NaiveDate) -> Result<MonitorView, String> {
    Ok(MonitorView {
        settings: settings_in(conn)?,
        status: status_in(conn, today)?,
        last_run_at: state_get(conn, "last_run_at")?,
        last_run_result: state_get(conn, "last_run_result")?,
    })
}

// ───────────────────────────── checks ─────────────────────────────

/// A condition that currently holds. `subject` makes the flag id: the stream (plus currency for income
/// flags) or the sequence.
#[derive(Debug, Clone, PartialEq)]
struct Cand {
    kind: &'static str,
    subject: String,
    stream_id: Option<String>,
    sequence_id: Option<String>,
    detail: String,
}

fn day_part(iso: &str) -> Option<NaiveDate> {
    let d = iso.get(..10)?;
    if is_date(d) {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
    } else {
        None
    }
}

struct ActiveStream {
    id: String,
    created: Option<NaiveDate>,
    last_ok_import: Option<NaiveDate>,
}

fn active_streams(conn: &Connection) -> Result<Vec<ActiveStream>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.created_at,
                    (SELECT MAX(at) FROM imports i WHERE i.stream_id = s.id AND i.status IN ('ok','partial'))
             FROM streams s WHERE s.status = 'active' ORDER BY s.created_at, s.id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, created, last) = row.map_err(store_err)?;
        out.push(ActiveStream {
            id,
            created: day_part(&created),
            last_ok_import: last.as_deref().and_then(day_part),
        });
    }
    Ok(out)
}

fn stale_limit(s: &MonitorSettings, stream: &str) -> i64 {
    i64::from(
        s.stale_days_by_stream
            .get(stream)
            .copied()
            .unwrap_or(s.stale_days),
    )
}

/// Days of silence for a stream, `None` when neither an import nor a creation date can be read.
fn silent_days(st: &ActiveStream, today: NaiveDate) -> Option<(i64, bool)> {
    match (st.last_ok_import, st.created) {
        (Some(d), _) => Some(((today - d).num_days(), true)),
        (None, Some(c)) => Some(((today - c).num_days(), false)),
        _ => None,
    }
}

fn check_stale(
    streams: &[ActiveStream],
    s: &MonitorSettings,
    today: NaiveDate,
) -> (Vec<Cand>, BTreeSet<String>) {
    let mut out = Vec::new();
    let mut stale = BTreeSet::new();
    for st in streams {
        let Some((days, imported)) = silent_days(st, today) else {
            continue;
        };
        let limit = stale_limit(s, &st.id);
        if days >= limit {
            stale.insert(st.id.clone());
            out.push(Cand {
                kind: "stale_data",
                subject: st.id.clone(),
                stream_id: Some(st.id.clone()),
                sequence_id: None,
                detail: if imported {
                    format!("No import for {days} days (limit {limit}).")
                } else {
                    format!(
                        "No import yet, {days} days after the stream was added (limit {limit})."
                    )
                },
            });
        }
    }
    (out, stale)
}

fn safe_code(status: &str) -> Option<&str> {
    let code = status.strip_prefix("failed:")?;
    (!code.is_empty()
        && code.len() <= 32
        && code.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
    .then_some(code)
}

fn check_failed_imports(conn: &Connection) -> Result<Vec<Cand>, String> {
    let mut out = Vec::new();
    // Income imports: the newest failed row is later than the newest good one (or there is no good one).
    let mut stmt = conn
        .prepare(
            "SELECT s.id,
                    (SELECT status FROM imports f WHERE f.stream_id = s.id AND f.status LIKE 'failed:%' ORDER BY at DESC LIMIT 1),
                    (SELECT MAX(at) FROM imports f WHERE f.stream_id = s.id AND f.status LIKE 'failed:%'),
                    (SELECT MAX(at) FROM imports g WHERE g.stream_id = s.id AND g.status IN ('ok','partial'))
             FROM streams s WHERE s.status = 'active' ORDER BY s.created_at, s.id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(store_err)?;
    for row in rows {
        let (id, status, failed_at, ok_at) = row.map_err(store_err)?;
        let (Some(status), Some(failed_at)) = (status, failed_at) else {
            continue;
        };
        if ok_at.is_some_and(|ok| ok >= failed_at) {
            continue;
        }
        out.push(Cand {
            kind: "failed_import",
            subject: id.clone(),
            stream_id: Some(id),
            sequence_id: None,
            detail: match safe_code(&status) {
                Some(code) => {
                    format!("The latest import failed ({code}) and none has worked since.")
                }
                None => "The latest import failed and none has worked since.".into(),
            },
        });
    }
    drop(stmt);
    // Provider-results imports, per sequence, same rule.
    let mut stmt = conn
        .prepare(
            "SELECT d.id,
                    (SELECT status FROM results_imports f WHERE f.sequence_id = d.id AND f.status LIKE 'failed:%' ORDER BY at DESC LIMIT 1),
                    (SELECT MAX(at) FROM results_imports f WHERE f.sequence_id = d.id AND f.status LIKE 'failed:%'),
                    (SELECT MAX(at) FROM results_imports g WHERE g.sequence_id = d.id AND g.status NOT LIKE 'failed:%')
             FROM sequence_drafts d ORDER BY d.created_at, d.id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(store_err)?;
    for row in rows {
        let (id, status, failed_at, ok_at) = row.map_err(store_err)?;
        let (Some(status), Some(failed_at)) = (status, failed_at) else {
            continue;
        };
        if ok_at.is_some_and(|ok| ok >= failed_at) {
            continue;
        }
        out.push(Cand {
            kind: "failed_import",
            subject: format!("seq:{id}"),
            stream_id: None,
            sequence_id: Some(id),
            detail: match safe_code(&status) {
                Some(code) => {
                    format!("The latest results import failed ({code}) and none has worked since.")
                }
                None => "The latest results import failed and none has worked since.".into(),
            },
        });
    }
    Ok(out)
}

fn month_key(first_of_month: NaiveDate, back: u32) -> Result<String, String> {
    Ok(first_of_month
        .checked_sub_months(Months::new(back))
        .ok_or_else(|| "invalid_input".to_string())?
        .format("%Y-%m")
        .to_string())
}

/// The last complete month against the mean of the three months before it. Needs a figure for the last
/// month and at least one earlier month, and a positive average. Strictly more than `pct` percent down.
fn income_down(avg: i64, last: i64, pct: u32) -> bool {
    avg > 0 && i128::from(avg - last) * 100 > i128::from(pct) * i128::from(avg)
}

fn check_income_down(
    conn: &Connection,
    streams: &[ActiveStream],
    stale: &BTreeSet<String>,
    s: &MonitorSettings,
    today: NaiveDate,
) -> Result<Vec<Cand>, String> {
    let first = today
        .with_day(1)
        .ok_or_else(|| "invalid_input".to_string())?;
    let (m4, m1) = (month_key(first, 4)?, month_key(first, 1)?);
    let keys = [
        month_key(first, 4)?,
        month_key(first, 3)?,
        month_key(first, 2)?,
    ];
    let totals = monthly_totals(conn, &m4, &m1, None)?;
    let mut out = Vec::new();
    for st in streams {
        if stale.contains(&st.id) {
            continue;
        }
        for cur in ["JPY", "USD"] {
            let get = |m: &str| totals.get(&(st.id.clone(), cur.to_string(), m.to_string()));
            let Some(&last) = get(&m1) else { continue };
            let prior: Vec<Option<i64>> = keys.iter().map(|k| get(k).copied()).collect();
            let Some(avg) = mean_minor(&prior) else {
                continue;
            };
            if income_down(avg, last, s.income_drop_pct) {
                let drop = i128::from(avg - last) * 100 / i128::from(avg);
                out.push(Cand {
                    kind: "income_down",
                    subject: format!("{}:{cur}", st.id),
                    stream_id: Some(st.id.clone()),
                    sequence_id: None,
                    detail: format!(
                        "{cur} income last month is down {drop}% against the trailing average."
                    ),
                });
            }
        }
    }
    Ok(out)
}

fn check_no_income(
    conn: &Connection,
    streams: &[ActiveStream],
    stale: &BTreeSet<String>,
    s: &MonitorSettings,
    today: NaiveDate,
) -> Result<Vec<Cand>, String> {
    let mut out = Vec::new();
    for st in streams {
        if stale.contains(&st.id) {
            continue;
        }
        let last: Option<String> = conn
            .query_row(
                "SELECT MAX(date) FROM income_events WHERE stream_id = ?1 AND amount_minor > 0",
                [&st.id],
                |r| r.get(0),
            )
            .map_err(store_err)?;
        let Some(day) = last.as_deref().and_then(day_part) else {
            continue;
        };
        let days = (today - day).num_days();
        if days >= i64::from(s.no_income_days) {
            out.push(Cand {
                kind: "no_income",
                subject: st.id.clone(),
                stream_id: Some(st.id.clone()),
                sequence_id: None,
                detail: format!("No income recorded for {days} days."),
            });
        }
    }
    Ok(out)
}

struct ResultDay {
    day: NaiveDate,
    sends: Option<i64>,
    bounces: Option<i64>,
    unsubscribes: Option<i64>,
    purchases: Option<i64>,
}

fn results_for(conn: &Connection, sequence_id: &str) -> Result<Vec<ResultDay>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT day, sends, bounces, unsubscribes, purchases_after FROM sequence_results
             WHERE sequence_id = ?1 ORDER BY day DESC",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([sequence_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for row in rows {
        let (day, sends, bounces, unsubscribes, purchases) = row.map_err(store_err)?;
        if let Some(day) = day_part(&day) {
            out.push(ResultDay {
                day,
                sends,
                bounces,
                unsubscribes,
                purchases,
            });
        }
    }
    Ok(out)
}

/// Bounces plus unsubscribes strictly above `pct` percent of sends, with enough sends to judge.
fn spike(sends: i64, bad: i64, pct: u32) -> bool {
    sends >= MIN_SENDS && i128::from(bad) * 100 > i128::from(pct) * i128::from(sends)
}

fn check_sequences(
    conn: &Connection,
    s: &MonitorSettings,
    today: NaiveDate,
) -> Result<Vec<Cand>, String> {
    let mut stmt = conn
        .prepare("SELECT id, status, approved_at FROM sequence_drafts ORDER BY created_at, id")
        .map_err(store_err)?;
    let drafts: Vec<(String, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(store_err)?
        .collect::<Result<_, _>>()
        .map_err(store_err)?;
    drop(stmt);
    let mut out = Vec::new();
    for (id, status, approved_at) in drafts {
        let results = results_for(conn, &id)?;
        let Some(newest) = results.first().map(|r| r.day) else {
            continue;
        };
        // Spike: the 7 result days ending at the newest one, only days with all three figures.
        if (today - newest).num_days() <= RESULTS_RECENT_DAYS {
            let (mut sends, mut bad) = (0i64, 0i64);
            for r in results
                .iter()
                .filter(|r| (newest - r.day).num_days() < SPIKE_WINDOW_DAYS)
            {
                if let (Some(a), Some(b), Some(u)) = (r.sends, r.bounces, r.unsubscribes) {
                    sends = sends.saturating_add(a);
                    bad = bad.saturating_add(b).saturating_add(u);
                }
            }
            if spike(sends, bad, s.spike_pct) {
                let rate = i128::from(bad) * 100 / i128::from(sends);
                out.push(Cand {
                    kind: "results_spike",
                    subject: id.clone(),
                    stream_id: None,
                    sequence_id: Some(id.clone()),
                    detail: format!(
                        "Bounces and unsubscribes are {rate}% of sends over the latest {SPIKE_WINDOW_DAYS} result days."
                    ),
                });
            }
        }
        // No conversions: an approved sequence, N days after approval, with results that report
        // purchases and all of them zero. A sequence without purchase figures is unknown, not zero.
        if status == "approved" {
            let Some(since) = approved_at.as_deref().and_then(day_part) else {
                continue;
            };
            let days = (today - since).num_days();
            let known: Vec<i64> = results.iter().filter_map(|r| r.purchases).collect();
            if days >= i64::from(s.no_conversion_days)
                && !known.is_empty()
                && known.iter().all(|p| *p == 0)
            {
                out.push(Cand {
                    kind: "no_conversions",
                    subject: id.clone(),
                    stream_id: None,
                    sequence_id: Some(id.clone()),
                    detail: format!("No conversions {days} days after the sequence was approved."),
                });
            }
        }
    }
    Ok(out)
}

fn candidates_in(
    conn: &Connection,
    s: &MonitorSettings,
    today: NaiveDate,
) -> Result<Vec<Cand>, String> {
    let streams = active_streams(conn)?;
    let (mut out, stale) = check_stale(&streams, s, today);
    out.extend(check_failed_imports(conn)?);
    out.extend(check_income_down(conn, &streams, &stale, s, today)?);
    out.extend(check_no_income(conn, &streams, &stale, s, today)?);
    out.extend(check_sequences(conn, s, today)?);
    Ok(out)
}

// ───────────────────────────── flags table ─────────────────────────────

fn key_of(c: &Cand) -> String {
    format!("{}:{}", c.kind, c.subject)
}

fn kind_rank(kind: &str) -> usize {
    KIND_ORDER
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(KIND_ORDER.len())
}

pub(crate) fn flags_in(conn: &Connection) -> Result<Vec<Flag>, String> {
    let mut stmt = conn
        .prepare("SELECT id, kind, stream_id, sequence_id, since, detail, acked FROM flags")
        .map_err(store_err)?;
    let mut out: Vec<Flag> = stmt
        .query_map([], |r| {
            Ok(Flag {
                id: r.get(0)?,
                kind: r.get(1)?,
                stream_id: r.get(2)?,
                sequence_id: r.get(3)?,
                since: r.get(4)?,
                detail: r.get(5)?,
                acked: r.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(store_err)?
        .collect::<Result<_, _>>()
        .map_err(store_err)?;
    // Stale data first, then the plan's order, oldest first, id as the tie-break.
    out.sort_by(|a, b| {
        (kind_rank(&a.kind), &a.since, &a.id).cmp(&(kind_rank(&b.kind), &b.since, &b.id))
    });
    Ok(out)
}

/// Replace the table's contents with `cands`: keep `since`, `acked` and `alerted` of a flag that still
/// holds, insert new ones, delete the ones that cleared. Returns (new, cleared).
fn sync_flags(conn: &mut Connection, cands: &[Cand]) -> Result<(i64, i64), String> {
    let tx = conn.transaction().map_err(store_err)?;
    let existing: BTreeSet<String> = {
        let mut stmt = tx.prepare("SELECT id FROM flags").map_err(store_err)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(store_err)?;
        rows.collect::<Result<_, _>>().map_err(store_err)?
    };
    let now = now_iso();
    let mut keep = BTreeSet::new();
    let mut new = 0i64;
    for c in cands {
        let id = key_of(c);
        if existing.contains(&id) {
            tx.execute(
                "UPDATE flags SET detail = ?1 WHERE id = ?2",
                params![c.detail, id],
            )
            .map_err(store_err)?;
        } else {
            tx.execute(
                "INSERT INTO flags(id, kind, stream_id, sequence_id, since, detail, acked, alerted)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
                params![id, c.kind, c.stream_id, c.sequence_id, now, c.detail],
            )
            .map_err(store_err)?;
            new += 1;
        }
        keep.insert(id);
    }
    let mut cleared = 0i64;
    for id in existing.difference(&keep) {
        tx.execute("DELETE FROM flags WHERE id = ?1", [id])
            .map_err(store_err)?;
        cleared += 1;
    }
    tx.commit().map_err(store_err)?;
    Ok((new, cleared))
}

fn flag_ack_in(data: &Path, id: &str) -> Result<(), String> {
    let conn = open_db(data)?;
    let n = conn
        .execute(
            "UPDATE flags SET acked = 1, alerted = 1 WHERE id = ?1",
            [id],
        )
        .map_err(store_err)?;
    if n == 0 {
        return Err("unknown_flag".into());
    }
    Ok(())
}

fn pending_alerts(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM flags WHERE alerted = 0 AND acked = 0",
        [],
        |r| r.get(0),
    )
    .map_err(store_err)
}

fn mark_alerted(conn: &Connection) -> Result<(), String> {
    conn.execute("UPDATE flags SET alerted = 1 WHERE alerted = 0", [])
        .map_err(store_err)?;
    Ok(())
}

pub(crate) fn status_in(conn: &Connection, today: NaiveDate) -> Result<MonitorStatus, String> {
    let flags = flags_in(conn)?;
    if flags.iter().any(|f| f.kind == "stale_data") {
        return Ok(MonitorStatus {
            state: "stale".into(),
            as_of: None,
        });
    }
    if !flags.is_empty() {
        return Ok(MonitorStatus {
            state: "flags".into(),
            as_of: None,
        });
    }
    let _ = today;
    let streams = active_streams(conn)?;
    let oldest = streams
        .iter()
        .map(|s| s.last_ok_import)
        .collect::<Option<Vec<_>>>()
        .and_then(|v| v.into_iter().min());
    Ok(match oldest {
        Some(d) if !streams.is_empty() => MonitorStatus {
            state: "ok".into(),
            as_of: Some(d.format("%Y-%m-%d").to_string()),
        },
        _ => MonitorStatus {
            state: "no_data".into(),
            as_of: None,
        },
    })
}

/// Evaluate every check, update the table and the run record. No relay here.
fn evaluate_in(data: &Path, now: DateTime<FixedOffset>) -> Result<(RunResult, i64), String> {
    let mut conn = open_db(data)?;
    let today = today_of(&now);
    let s = settings_in(&conn)?;
    let cands = candidates_in(&conn, &s, today)?;
    let (new_flags, cleared) = sync_flags(&mut conn, &cands)?;
    let ran_at = super::local_store::iso(&now);
    state_set(&conn, "last_run_at", &ran_at)?;
    state_set(&conn, "last_run_result", "ok")?;
    state_set(&conn, "last_ok_at", &now.with_timezone(&Utc).to_rfc3339())?;
    state_del(&conn, "failed_slot")?;
    state_del(&conn, "failed_attempts")?;
    let pending = pending_alerts(&conn)?;
    let result = RunResult {
        ran_at,
        flags: cands.len() as i64,
        new_flags,
        cleared,
        status: status_in(&conn, today)?,
        relay: "none".into(),
    };
    Ok((result, pending))
}

// ───────────────────────────── relay port and the run ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelayOutcome {
    Sent,
    Skipped,
    Failed,
}

/// What the monitor needs from the relay: send the fixed line for a day, nothing else.
pub(crate) trait RelayPort {
    async fn send(&self, day: &str) -> RelayOutcome;
}

struct LiveRelay;
impl RelayPort for LiveRelay {
    async fn send(&self, day: &str) -> RelayOutcome {
        match super::notify_relay::dispatch_treasury_live(day).await {
            Ok(super::notify_relay::Dispatch::Sent) => RelayOutcome::Sent,
            Ok(super::notify_relay::Dispatch::Skipped(_)) => RelayOutcome::Skipped,
            Err(_) => RelayOutcome::Failed,
        }
    }
}

/// One run: evaluate, then (only when something new is pending and outbound is allowed) at most one
/// relay line. A flag is marked alerted only after the line was sent.
pub(crate) async fn run_with<R: RelayPort>(
    data: &Path,
    now: DateTime<FixedOffset>,
    outbound_off: bool,
    relay: &R,
) -> Result<RunResult, String> {
    let d = data.to_path_buf();
    let (mut result, pending) =
        blocking(JoinErr::Code("store_error"), move || evaluate_in(&d, now)).await?;
    if pending == 0 {
        return Ok(result);
    }
    if outbound_off {
        result.relay = "off".into();
        return Ok(result);
    }
    let day = today_of(&now).format("%Y-%m-%d").to_string();
    result.relay = match relay.send(&day).await {
        RelayOutcome::Sent => {
            let d = data.to_path_buf();
            blocking(JoinErr::Code("store_error"), move || {
                mark_alerted(&open_db(&d)?)
            })
            .await?;
            "sent"
        }
        RelayOutcome::Skipped => "skipped",
        RelayOutcome::Failed => "failed",
    }
    .into();
    Ok(result)
}

static RUNNING: AtomicBool = AtomicBool::new(false);

struct RunSlot;
impl RunSlot {
    fn take() -> Option<RunSlot> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RunSlot)
    }
}
impl Drop for RunSlot {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

async fn run_live() -> Result<RunResult, String> {
    let _slot = RunSlot::take().ok_or_else(|| "already_running".to_string())?;
    run_with(
        &crate::app_paths::data_dir(),
        now_jst(),
        super::telegram::outbound_disabled(),
        &LiveRelay,
    )
    .await
}

// ───────────────────────────── scheduler ─────────────────────────────

pub(crate) struct Inputs<'a> {
    pub now: DateTime<Utc>,
    pub since_launch: std::time::Duration,
    pub nightly: bool,
    /// The last successful run.
    pub last_done: Option<DateTime<Utc>>,
    pub busy: bool,
    pub battery: Option<Battery>,
    pub failed_slot: Option<&'a str>,
    pub failed_attempts: u32,
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Same shape as `backup_schedule::decide`: a missed slot runs once after launch, never while a chat
/// turn, crew run or Muse recording is active, skipped on a low battery, bounded retries after errors.
pub(crate) fn decide(i: &Inputs) -> Decision {
    if !i.nightly || i.since_launch < STARTUP_DELAY || !slot_missed(i.last_done, i.now, NIGHTLY_AT)
    {
        return Decision::Wait;
    }
    if let Some(slot) = prev_slot(i.now, NIGHTLY_AT) {
        if i.failed_slot == Some(slot.to_rfc3339().as_str()) {
            if i.failed_attempts >= MAX_ATTEMPTS {
                return Decision::Wait;
            }
            if i.last_run_at.is_some_and(|t| i.now - t < RETRY_AFTER) {
                return Decision::Wait;
            }
        }
    }
    if i.busy {
        return Decision::Skip("skipped_busy");
    }
    if let Some(b) = i.battery {
        if b.on_battery && b.percent < SKIP_ON_BATTERY_BELOW {
            return Decision::Skip("skipped_battery");
        }
    }
    Decision::Run
}

fn parse_rfc(s: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

struct SchedState {
    nightly: bool,
    last_done: Option<DateTime<Utc>>,
    failed_slot: Option<String>,
    failed_attempts: u32,
    last_run_at: Option<DateTime<Utc>>,
    last_result: Option<String>,
}

fn sched_state(data: &Path) -> Result<Option<SchedState>, String> {
    // Nothing to watch until Treasury has a database: never create one from the timer.
    if std::fs::symlink_metadata(db_path(data)).is_err() {
        return Ok(None);
    }
    let conn = open_db(data)?;
    Ok(Some(SchedState {
        nightly: settings_in(&conn)?.nightly,
        last_done: parse_rfc(state_get(&conn, "last_ok_at")?.as_deref()),
        failed_slot: state_get(&conn, "failed_slot")?,
        failed_attempts: state_get(&conn, "failed_attempts")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        last_run_at: parse_rfc(state_get(&conn, "last_run_at")?.as_deref()),
        last_result: state_get(&conn, "last_run_result")?,
    }))
}

fn record_skip(data: &Path, code: &str) -> Result<(), String> {
    let conn = open_db(data)?;
    state_set(&conn, "last_run_at", &now_iso())?;
    state_set(&conn, "last_run_result", code)
}

fn record_failure(data: &Path, slot: &str, code: &str) -> Result<(), String> {
    let conn = open_db(data)?;
    let same = state_get(&conn, "failed_slot")?.as_deref() == Some(slot);
    let n: u32 = if same {
        state_get(&conn, "failed_attempts")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    } else {
        0
    };
    state_set(&conn, "failed_slot", slot)?;
    state_set(&conn, "failed_attempts", &(n + 1).to_string())?;
    state_set(&conn, "last_run_at", &now_iso())?;
    state_set(&conn, "last_run_result", code)
}

async fn tick(started: std::time::Instant) {
    let data = crate::app_paths::data_dir();
    let d = data.clone();
    let Ok(Ok(Some(st))) = tokio::task::spawn_blocking(move || sched_state(&d)).await else {
        return;
    };
    let now = Utc::now();
    let base = |busy, battery| Inputs {
        now,
        since_launch: started.elapsed(),
        nightly: st.nightly,
        last_done: st.last_done,
        busy,
        battery,
        failed_slot: st.failed_slot.as_deref(),
        failed_attempts: st.failed_attempts,
        last_run_at: st.last_run_at,
    };
    // Cheap checks first: pmset and the busy probes only run when a run is due.
    if decide(&base(false, None)) == Decision::Wait {
        return;
    }
    let busy = busy_now().await;
    let battery = tokio::task::spawn_blocking(read_battery)
        .await
        .ok()
        .flatten();
    match decide(&base(busy, battery)) {
        Decision::Wait => {}
        Decision::Skip(code) => {
            if st.last_result.as_deref() != Some(code) {
                let d = data.clone();
                let _ = tokio::task::spawn_blocking(move || record_skip(&d, code)).await;
            }
        }
        Decision::Run => {
            // Re-check right before the run: a chat turn or recording may have started.
            if busy_now().await {
                return;
            }
            // `already_running` (Run Now holds the slot) is simply tried again next tick.
            let res = run_live().await;
            if let Err(code) = res {
                if code != "already_running" {
                    if let Some(slot) = prev_slot(Utc::now(), NIGHTLY_AT) {
                        let (d, s) = (data.clone(), slot.to_rfc3339());
                        let _ = tokio::task::spawn_blocking(move || record_failure(&d, &s, &code))
                            .await;
                    }
                }
            }
        }
    }
}

/// Start the nightly job. Called once from the app's `setup`; nothing runs while the app is closed.
pub fn start() {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        println!("[treasury] nightly monitor on (slot {NIGHTLY_AT} JST)");
        loop {
            tokio::time::sleep(TICK).await;
            tick(started).await;
        }
    });
}

// ───────────────────────────── needs-you source ─────────────────────────────

/// The open flags the Needs-you list points at, stale first. `Ok(None)` when Treasury has no database
/// yet (unwired). Acknowledged flags are left out. Read-only; nothing is created.
pub(crate) fn needs_you_rows_in(data: &Path) -> Result<Option<Vec<Flag>>, String> {
    if std::fs::symlink_metadata(db_path(data)).is_err() {
        return Ok(None);
    }
    let conn = open_db(data)?;
    Ok(Some(
        flags_in(&conn)?.into_iter().filter(|f| !f.acked).collect(),
    ))
}

// ───────────────────────────── commands ─────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn treasury_flags_list() -> Result<Vec<Flag>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        flags_in(&open_db(&crate::app_paths::data_dir())?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_flags_ack(id: String) -> Result<(), String> {
    blocking(JoinErr::Code("store_error"), move || {
        flag_ack_in(&crate::app_paths::data_dir(), &id)
    })
    .await
}

/// The same code path as the nightly job, including the relay line for new flags.
#[tauri::command]
#[specta::specta]
pub async fn treasury_monitor_run_now() -> Result<RunResult, String> {
    run_live().await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_monitor_settings_get() -> Result<MonitorView, String> {
    blocking(JoinErr::Code("store_error"), move || {
        view_in(
            &open_db(&crate::app_paths::data_dir())?,
            today_of(&now_jst()),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_monitor_settings_set(
    patch: MonitorSettingsPatch,
) -> Result<MonitorView, String> {
    blocking(JoinErr::Code("store_error"), move || {
        settings_set_in(&crate::app_paths::data_dir(), patch)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::shared::time::jst;
    use crate::commands::treasury::stream_add_in;
    use crate::commands::treasury::test_support::{temp_dir, write_ledger, BANNED};
    use chrono::TimeZone;
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }
    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// A temp data dir with one venture and one active stream.
    fn setup(tag: &str) -> (PathBuf, String) {
        let d = temp_dir(tag);
        write_ledger(&d, &["v1"]);
        let s = stream_add_in(&d, "v1", "Shop", "product_sales", None, None).unwrap();
        (d, s.id)
    }

    fn exec(d: &Path, sql: &str, args: &[&dyn rusqlite::ToSql]) {
        open_db(d).unwrap().execute(sql, args).unwrap();
    }

    fn good_import(d: &Path, sid: &str, day: &str) {
        exec(
            d,
            "INSERT INTO imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)
             VALUES(?1, ?2, 'generic', NULL, 1, 1, 0, 'ok', ?3)",
            &[&uuid::Uuid::new_v4().to_string(), &sid, &format!("{day}T10:00:00.000+09:00")],
        );
    }

    fn failed_import(d: &Path, sid: &str, day: &str, code: &str) {
        exec(
            d,
            "INSERT INTO imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)
             VALUES(?1, ?2, 'generic', NULL, 0, 0, 0, ?3, ?4)",
            &[
                &uuid::Uuid::new_v4().to_string(),
                &sid,
                &format!("failed:{code}"),
                &format!("{day}T11:00:00.000+09:00"),
            ],
        );
    }

    fn income(d: &Path, sid: &str, day: &str, minor: i64) {
        exec(
            d,
            "INSERT INTO income_events(id, stream_id, date, amount_minor, currency, source, raw_ref, dedupe_key, import_id)
             VALUES(?1, ?2, ?3, ?4, 'JPY', 'generic', 'row:1', ?5, NULL)",
            &[
                &uuid::Uuid::new_v4().to_string(),
                &sid,
                &day,
                &minor,
                &uuid::Uuid::new_v4().to_string(),
            ],
        );
    }

    fn cands(d: &Path, today: NaiveDate) -> Vec<Cand> {
        let conn = open_db(d).unwrap();
        candidates_in(&conn, &settings_in(&conn).unwrap(), today).unwrap()
    }
    fn kinds(c: &[Cand]) -> Vec<&'static str> {
        c.iter().map(|c| c.kind).collect()
    }

    fn sequence(d: &Path, id: &str, status: &str, approved: Option<&str>) {
        exec(
            d,
            "INSERT INTO sequence_drafts(id, venture_id, kind, emails_json, checklist_json, status, content_hash, approved_at, created_at, updated_at)
             VALUES(?1, 'v1', 'welcome', '[]', '[]', ?2, 'h', ?3, '2026-01-01T00:00:00.000+09:00', '2026-01-01T00:00:00.000+09:00')",
            &[&id, &status, &approved],
        );
    }
    fn result_day(d: &Path, seq: &str, day: &str, v: [Option<i64>; 4]) {
        exec(
            d,
            "INSERT INTO sequence_results(sequence_id, day, sends, bounces, unsubscribes, purchases_after, import_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            &[&seq, &day, &v[0], &v[1], &v[2], &v[3]],
        );
    }

    // ---- flags at the edge ----

    #[test]
    fn stale_fires_at_n_days_of_csv_silence_and_not_below() {
        let (d, sid) = setup("stale-edge");
        good_import(&d, &sid, "2026-11-10");
        assert!(kinds(&cands(&d, date(2026, 11, 16))).is_empty(), "6 days");
        let c = cands(&d, date(2026, 11, 17));
        assert_eq!(kinds(&c), ["stale_data"], "7 days");
        assert_eq!(c[0].detail, "No import for 7 days (limit 7).");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn stale_uses_the_per_stream_override() {
        let (d, sid) = setup("stale-override");
        good_import(&d, &sid, "2026-11-10");
        settings_set_in(
            &d,
            MonitorSettingsPatch {
                stale_days_by_stream: Some([(sid.clone(), 35)].into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(kinds(&cands(&d, date(2026, 12, 14))).is_empty(), "34 days");
        assert_eq!(kinds(&cands(&d, date(2026, 12, 15))), ["stale_data"]);
        let bad = settings_set_in(
            &d,
            MonitorSettingsPatch {
                stale_days_by_stream: Some([("nope".to_string(), 3)].into()),
                ..Default::default()
            },
        );
        assert_eq!(bad.unwrap_err(), "unknown_stream");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_stream_never_imported_goes_stale_from_the_day_it_was_added() {
        let (d, _) = setup("stale-never");
        let today = now_jst().date_naive();
        assert!(kinds(&cands(&d, today)).is_empty());
        let c = cands(&d, today + Duration::days(7));
        assert_eq!(kinds(&c), ["stale_data"]);
        assert!(c[0].detail.starts_with("No import yet"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn paused_streams_are_not_watched() {
        let (d, sid) = setup("paused");
        exec(
            &d,
            "UPDATE streams SET status = 'paused' WHERE id = ?1",
            &[&sid],
        );
        assert!(kinds(&cands(&d, date(2030, 1, 1))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_failed_import_flags_until_a_later_good_one() {
        let (d, sid) = setup("failed");
        good_import(&d, &sid, "2026-11-10");
        failed_import(&d, &sid, "2026-11-11", "bad_date");
        let c = cands(&d, date(2026, 11, 12));
        assert_eq!(kinds(&c), ["failed_import"]);
        assert!(c[0].detail.contains("(bad_date)"));
        good_import(&d, &sid, "2026-11-12");
        assert!(kinds(&cands(&d, date(2026, 11, 12))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_failed_results_import_flags_the_sequence() {
        let (d, _) = setup("failed-results");
        sequence(&d, "seq1", "draft", None);
        exec(
            &d,
            "INSERT INTO results_imports(id, sequence_id, file_sha256, row_count, status, at) VALUES('r1','seq1',NULL,0,'failed:bad_date','2026-11-11T10:00:00.000+09:00')",
            &[],
        );
        let c = cands(&d, date(2026, 11, 12));
        assert_eq!(kinds(&c), ["failed_import"]);
        assert_eq!(c[0].sequence_id.as_deref(), Some("seq1"));
        exec(
            &d,
            "INSERT INTO results_imports(id, sequence_id, file_sha256, row_count, status, at) VALUES('r2','seq1',NULL,3,'ok','2026-11-12T10:00:00.000+09:00')",
            &[],
        );
        assert!(kinds(&cands(&d, date(2026, 11, 12))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn income_down_is_strictly_more_than_the_threshold() {
        assert!(!income_down(100_000, 70_000, 30), "exactly 30%");
        assert!(income_down(100_000, 69_999, 30), "just over 30%");
        assert!(!income_down(100_000, 150_000, 30));
        assert!(!income_down(0, -5, 30), "no positive baseline");
        assert!(income_down(100_000, -1, 30), "a refund month counts");
    }

    fn income_months(d: &Path, sid: &str, vals: [i64; 4]) {
        // Months 2026-07..2026-10 (the last complete month before 2026-11-18), plus a recent sale so
        // `no_income` stays quiet.
        income(d, sid, "2026-11-16", 1);
        for (day, v) in ["2026-07-15", "2026-08-15", "2026-09-15", "2026-10-15"]
            .iter()
            .zip(vals)
        {
            income(d, sid, day, v);
        }
    }

    #[test]
    fn income_down_flag_fires_over_30_percent_and_not_at_30() {
        let today = date(2026, 11, 18);
        let (d, sid) = setup("income-down-at");
        good_import(&d, &sid, "2026-11-17");
        income_months(&d, &sid, [100_000, 100_000, 100_000, 70_000]);
        assert!(kinds(&cands(&d, today)).is_empty());
        let _ = fs::remove_dir_all(d);
        let (d, sid) = setup("income-down-over");
        good_import(&d, &sid, "2026-11-17");
        income_months(&d, &sid, [100_000, 100_000, 100_000, 69_999]);
        let c = cands(&d, today);
        assert_eq!(kinds(&c), ["income_down"]);
        assert_eq!(
            c[0].detail,
            "JPY income last month is down 30% against the trailing average."
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn income_down_needs_a_figure_for_last_month_and_an_earlier_one() {
        let today = date(2026, 11, 18);
        let (d, sid) = setup("income-down-missing");
        good_import(&d, &sid, "2026-11-17");
        // Only last month: no baseline. Only earlier months: last month unknown (never read as 0).
        income(&d, &sid, "2026-10-15", 1_000);
        income(&d, &sid, "2026-11-16", 1);
        assert!(kinds(&cands(&d, today)).is_empty());
        exec(&d, "DELETE FROM income_events", &[]);
        income(&d, &sid, "2026-08-15", 100_000);
        income(&d, &sid, "2026-09-15", 100_000);
        assert!(!kinds(&cands(&d, today)).contains(&"income_down"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn no_income_fires_at_14_days_and_not_at_13() {
        let (d, sid) = setup("no-income");
        good_import(&d, &sid, "2026-11-17");
        income(&d, &sid, "2026-11-05", 5_000);
        assert!(
            !kinds(&cands(&d, date(2026, 11, 18))).contains(&"no_income"),
            "13 days"
        );
        let c = cands(&d, date(2026, 11, 19));
        // The import is 2 days old: fresh, so the income flag may stand.
        assert_eq!(kinds(&c), ["no_income"], "14 days");
        assert_eq!(c[0].detail, "No income recorded for 14 days.");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn income_flags_wait_while_a_stream_is_stale() {
        let (d, sid) = setup("stale-suppresses");
        good_import(&d, &sid, "2026-10-01");
        income(&d, &sid, "2026-10-01", 5_000);
        let c = cands(&d, date(2026, 11, 18));
        assert_eq!(
            kinds(&c),
            ["stale_data"],
            "no_income is not raised on stale data"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_stream_with_no_income_at_all_has_no_no_income_flag() {
        let (d, sid) = setup("no-baseline");
        good_import(&d, &sid, "2026-11-17");
        assert!(kinds(&cands(&d, date(2026, 11, 18))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn spike_fires_over_the_rate_and_not_at_it() {
        assert!(!spike(100, 5, 5), "exactly 5%");
        assert!(spike(100, 6, 5));
        assert!(!spike(49, 40, 5), "too few sends to judge");
        assert!(spike(50, 6, 5));
    }

    #[test]
    fn results_spike_flag_uses_the_latest_seven_result_days() {
        let (d, _) = setup("spike");
        sequence(&d, "s1", "draft", None);
        // 5 days x 20 sends = 100 sends; 5 bad = exactly 5%.
        for i in 0..5 {
            let day = format!("2026-11-{:02}", 10 + i);
            result_day(&d, "s1", &day, [Some(20), Some(1), Some(0), None]);
        }
        assert!(kinds(&cands(&d, date(2026, 11, 16))).is_empty());
        result_day(&d, "s1", "2026-11-15", [Some(0), Some(0), Some(1), None]);
        // 100 sends, 6 bad.
        let c = cands(&d, date(2026, 11, 16));
        assert_eq!(kinds(&c), ["results_spike"]);
        assert_eq!(
            c[0].detail,
            "Bounces and unsubscribes are 6% of sends over the latest 7 result days."
        );
        // Old results (newest day more than 30 days back) raise nothing.
        assert!(!kinds(&cands(&d, date(2026, 12, 20))).contains(&"results_spike"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_result_day_missing_a_figure_is_skipped_not_counted_as_zero() {
        let (d, _) = setup("spike-null");
        sequence(&d, "s1", "draft", None);
        result_day(&d, "s1", "2026-11-10", [Some(100), None, Some(50), None]);
        assert!(kinds(&cands(&d, date(2026, 11, 11))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn zero_conversions_fire_at_30_days_after_approval_and_not_at_29() {
        let (d, _) = setup("conv");
        sequence(&d, "s1", "approved", Some("2026-10-01T10:00:00.000+09:00"));
        result_day(
            &d,
            "s1",
            "2026-10-20",
            [Some(40), Some(0), Some(0), Some(0)],
        );
        assert!(kinds(&cands(&d, date(2026, 10, 30))).is_empty(), "29 days");
        let c = cands(&d, date(2026, 10, 31));
        assert_eq!(kinds(&c), ["no_conversions"], "30 days");
        assert_eq!(
            c[0].detail,
            "No conversions 30 days after the sequence was approved."
        );
        // One purchase anywhere clears it.
        result_day(
            &d,
            "s1",
            "2026-10-21",
            [Some(40), Some(0), Some(0), Some(1)],
        );
        assert!(kinds(&cands(&d, date(2026, 10, 31))).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn unknown_purchases_or_no_results_are_not_zero_conversions() {
        let (d, _) = setup("conv-unknown");
        sequence(&d, "s1", "approved", Some("2026-09-01T10:00:00.000+09:00"));
        assert!(
            kinds(&cands(&d, date(2026, 11, 1))).is_empty(),
            "no results at all"
        );
        result_day(&d, "s1", "2026-09-20", [Some(40), Some(0), Some(0), None]);
        assert!(
            kinds(&cands(&d, date(2026, 11, 1))).is_empty(),
            "purchases not reported"
        );
        sequence(&d, "s2", "draft", None);
        result_day(
            &d,
            "s2",
            "2026-09-20",
            [Some(40), Some(0), Some(0), Some(0)],
        );
        assert!(
            kinds(&cands(&d, date(2026, 11, 1))).is_empty(),
            "a draft is not measured"
        );
        let _ = fs::remove_dir_all(d);
    }

    // ---- status ----

    #[test]
    fn status_is_ok_as_of_the_last_import_only_with_fresh_data_and_no_flags() {
        let (d, sid) = setup("status");
        let conn = open_db(&d).unwrap();
        let today = date(2026, 11, 18);
        // No import yet: no data, never ok.
        assert_eq!(status_in(&conn, today).unwrap().state, "no_data");
        good_import(&d, &sid, "2026-11-16");
        let st = status_in(&conn, today).unwrap();
        assert_eq!(
            (st.state.as_str(), st.as_of.as_deref()),
            ("ok", Some("2026-11-16"))
        );
        // A second stream never imported keeps it from saying ok.
        stream_add_in(&d, "v1", "Other", "affiliate", None, None).unwrap();
        assert_eq!(status_in(&conn, today).unwrap().state, "no_data");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_oldest_import_is_the_as_of_date() {
        let (d, sid) = setup("status-oldest");
        let other = stream_add_in(&d, "v1", "Other", "affiliate", None, None).unwrap();
        good_import(&d, &sid, "2026-11-16");
        good_import(&d, &other.id, "2026-11-12");
        let conn = open_db(&d).unwrap();
        let st = status_in(&conn, date(2026, 11, 18)).unwrap();
        assert_eq!(st.as_of.as_deref(), Some("2026-11-12"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn stale_wins_the_status_and_sorts_first_even_when_acknowledged() {
        let (d, sid) = setup("status-stale");
        good_import(&d, &sid, "2026-11-01");
        failed_import(&d, &sid, "2026-11-02", "bad_date");
        let mut conn = open_db(&d).unwrap();
        let c = candidates_in(&conn, &settings_in(&conn).unwrap(), date(2026, 11, 18)).unwrap();
        sync_flags(&mut conn, &c).unwrap();
        let flags = flags_in(&conn).unwrap();
        assert_eq!(flags[0].kind, "stale_data");
        assert_eq!(flags[1].kind, "failed_import");
        flag_ack_in(&d, &flags[0].id).unwrap();
        let st = status_in(&conn, date(2026, 11, 18)).unwrap();
        assert_eq!(
            st.state, "stale",
            "an acknowledged stale flag is still stale"
        );
        assert!(st.as_of.is_none());
        let _ = fs::remove_dir_all(d);
    }

    // ---- flags table: de-dup, survive restart, clear ----

    #[test]
    fn a_flag_keeps_its_row_while_it_holds_and_leaves_when_it_clears() {
        let (d, sid) = setup("dedupe");
        good_import(&d, &sid, "2026-11-01");
        let now = at(2026, 11, 18, 4);
        let (r1, p1) = evaluate_in(&d, now).unwrap();
        assert_eq!((r1.flags, r1.new_flags, r1.cleared, p1), (1, 1, 0, 1));
        let since = flags_in(&open_db(&d).unwrap()).unwrap()[0].since.clone();
        let (r2, p2) = evaluate_in(&d, at(2026, 11, 19, 4)).unwrap();
        assert_eq!(
            (r2.flags, r2.new_flags, r2.cleared, p2),
            (1, 0, 0, 1),
            "same flag, not new"
        );
        assert_eq!(flags_in(&open_db(&d).unwrap()).unwrap()[0].since, since);
        good_import(&d, &sid, "2026-11-19");
        let (r3, p3) = evaluate_in(&d, at(2026, 11, 19, 5)).unwrap();
        assert_eq!((r3.flags, r3.cleared, p3), (0, 1, 0));
        assert!(flags_in(&open_db(&d).unwrap()).unwrap().is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_flag_survives_a_restart() {
        let (d, sid) = setup("restart");
        good_import(&d, &sid, "2026-11-01");
        evaluate_in(&d, at(2026, 11, 18, 4)).unwrap();
        // A fresh connection (the app restarted) reads the same flag, status and acknowledgment.
        let id = flags_in(&open_db(&d).unwrap()).unwrap()[0].id.clone();
        flag_ack_in(&d, &id).unwrap();
        let conn = open_db(&d).unwrap();
        let flags = flags_in(&conn).unwrap();
        assert_eq!(flags.len(), 1);
        assert!(flags[0].acked);
        assert_eq!(status_in(&conn, date(2026, 11, 18)).unwrap().state, "stale");
        // And the next run does not make it new again.
        let (r, pending) = evaluate_in(&d, at(2026, 11, 19, 4)).unwrap();
        assert_eq!((r.new_flags, pending), (0, 0));
        assert_eq!(
            flag_ack_in(&d, "stale_data:nope").unwrap_err(),
            "unknown_flag"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn needs_you_rows_skip_acknowledged_flags_and_are_unwired_without_a_db() {
        let empty = temp_dir("ny-empty");
        assert_eq!(needs_you_rows_in(&empty).unwrap(), None);
        assert!(
            !db_path(&empty).exists(),
            "reading never creates the database"
        );
        let (d, sid) = setup("ny");
        good_import(&d, &sid, "2026-11-01");
        evaluate_in(&d, at(2026, 11, 18, 4)).unwrap();
        assert_eq!(needs_you_rows_in(&d).unwrap().unwrap().len(), 1);
        let id = needs_you_rows_in(&d).unwrap().unwrap()[0].id.clone();
        flag_ack_in(&d, &id).unwrap();
        assert!(needs_you_rows_in(&d).unwrap().unwrap().is_empty());
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(empty);
    }

    // ---- the run and the relay ----

    struct FakeRelay {
        calls: Cell<u32>,
        out: RelayOutcome,
        days: std::cell::RefCell<Vec<String>>,
    }
    impl FakeRelay {
        fn new(out: RelayOutcome) -> Self {
            Self {
                calls: Cell::new(0),
                out,
                days: Default::default(),
            }
        }
    }
    impl RelayPort for FakeRelay {
        async fn send(&self, day: &str) -> RelayOutcome {
            self.calls.set(self.calls.get() + 1);
            self.days.borrow_mut().push(day.to_string());
            self.out
        }
    }

    #[tokio::test]
    async fn no_flags_sends_nothing() {
        let (d, sid) = setup("no-flags");
        good_import(&d, &sid, "2026-11-17");
        let relay = FakeRelay::new(RelayOutcome::Sent);
        let r = run_with(&d, at(2026, 11, 18, 4), false, &relay)
            .await
            .unwrap();
        assert_eq!((r.flags, r.relay.as_str()), (0, "none"));
        assert_eq!(r.status.state, "ok");
        assert_eq!(r.status.as_of.as_deref(), Some("2026-11-17"));
        assert_eq!(relay.calls.get(), 0);
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_new_flag_sends_one_line_once_and_a_repeat_run_sends_nothing() {
        let (d, sid) = setup("relay-once");
        good_import(&d, &sid, "2026-11-01");
        let relay = FakeRelay::new(RelayOutcome::Sent);
        let r1 = run_with(&d, at(2026, 11, 18, 4), false, &relay)
            .await
            .unwrap();
        assert_eq!(r1.relay, "sent");
        assert_eq!(relay.days.borrow().as_slice(), ["2026-11-18"]);
        let r2 = run_with(&d, at(2026, 11, 19, 4), false, &relay)
            .await
            .unwrap();
        assert_eq!((r2.flags, r2.relay.as_str()), (1, "none"));
        assert_eq!(relay.calls.get(), 1, "one alert per flag until it clears");
        // It clears, then comes back: a new alert.
        good_import(&d, &sid, "2026-11-19");
        run_with(&d, at(2026, 11, 19, 5), false, &relay)
            .await
            .unwrap();
        let r4 = run_with(&d, at(2026, 12, 20, 4), false, &relay)
            .await
            .unwrap();
        assert_eq!(r4.relay, "sent");
        assert_eq!(relay.calls.get(), 2);
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_skipped_or_failed_relay_leaves_the_flag_unalerted_for_the_next_run() {
        let (d, sid) = setup("relay-retry");
        good_import(&d, &sid, "2026-11-01");
        let skipped = FakeRelay::new(RelayOutcome::Skipped);
        let r = run_with(&d, at(2026, 11, 18, 4), false, &skipped)
            .await
            .unwrap();
        assert_eq!(r.relay, "skipped");
        let failed = FakeRelay::new(RelayOutcome::Failed);
        let r = run_with(&d, at(2026, 11, 19, 4), false, &failed)
            .await
            .unwrap();
        assert_eq!(r.relay, "failed");
        let sent = FakeRelay::new(RelayOutcome::Sent);
        let r = run_with(&d, at(2026, 11, 20, 4), false, &sent)
            .await
            .unwrap();
        assert_eq!(r.relay, "sent");
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn an_acknowledged_flag_is_never_relayed() {
        let (d, sid) = setup("relay-acked");
        good_import(&d, &sid, "2026-11-01");
        let first = FakeRelay::new(RelayOutcome::Skipped);
        run_with(&d, at(2026, 11, 18, 4), false, &first)
            .await
            .unwrap();
        let id = flags_in(&open_db(&d).unwrap()).unwrap()[0].id.clone();
        flag_ack_in(&d, &id).unwrap();
        let relay = FakeRelay::new(RelayOutcome::Sent);
        let r = run_with(&d, at(2026, 11, 19, 4), false, &relay)
            .await
            .unwrap();
        assert_eq!((r.relay.as_str(), relay.calls.get()), ("none", 0));
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn the_job_honours_the_outbound_kill_switch_for_the_relay_part() {
        let (d, sid) = setup("relay-off");
        good_import(&d, &sid, "2026-11-01");
        let relay = FakeRelay::new(RelayOutcome::Sent);
        let r = run_with(&d, at(2026, 11, 18, 4), true, &relay)
            .await
            .unwrap();
        assert_eq!(r.relay, "off");
        assert_eq!(relay.calls.get(), 0, "the relay is not even asked");
        // The flag is still recorded and still points in Needs-you.
        assert_eq!(needs_you_rows_in(&d).unwrap().unwrap().len(), 1);
        // And the live path reads the one shared switch.
        let src = production();
        assert!(src.contains("super::telegram::outbound_disabled()"));
        let _ = fs::remove_dir_all(d);
    }

    // ---- settings ----

    #[test]
    fn settings_default_to_the_decided_values_and_validate() {
        let (d, _) = setup("settings");
        let v = view_in(&open_db(&d).unwrap(), date(2026, 11, 18)).unwrap();
        assert_eq!(
            (
                v.settings.stale_days,
                v.settings.income_drop_pct,
                v.settings.no_income_days,
                v.settings.no_conversion_days
            ),
            (7, 30, 14, 30)
        );
        assert!(v.settings.nightly);
        let ok = settings_set_in(
            &d,
            MonitorSettingsPatch {
                stale_days: Some(10),
                income_drop_pct: Some(40),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (ok.settings.stale_days, ok.settings.income_drop_pct),
            (10, 40)
        );
        for bad in [
            MonitorSettingsPatch {
                stale_days: Some(0),
                ..Default::default()
            },
            MonitorSettingsPatch {
                stale_days: Some(366),
                ..Default::default()
            },
            MonitorSettingsPatch {
                income_drop_pct: Some(100),
                ..Default::default()
            },
            MonitorSettingsPatch {
                spike_pct: Some(0),
                ..Default::default()
            },
        ] {
            assert_eq!(settings_set_in(&d, bad).unwrap_err(), "invalid_input");
        }
        // Survives a restart, and a rejected patch changed nothing.
        let again = view_in(&open_db(&d).unwrap(), date(2026, 11, 18)).unwrap();
        assert_eq!(again.settings.stale_days, 10);
        let _ = fs::remove_dir_all(d);
    }

    // ---- scheduler ----

    fn inputs(now: DateTime<Utc>, last_done: Option<DateTime<Utc>>) -> Inputs<'static> {
        Inputs {
            now,
            since_launch: std::time::Duration::from_secs(3_600),
            nightly: true,
            last_done,
            busy: false,
            battery: None,
            failed_slot: None,
            failed_attempts: 0,
            last_run_at: None,
        }
    }
    fn utc(d: DateTime<FixedOffset>) -> DateTime<Utc> {
        d.with_timezone(&Utc)
    }

    #[test]
    fn catch_up_fires_once_after_a_slept_night() {
        // The Mac slept through 04:00 on the 18th; it wakes at 09:00 JST.
        let last = utc(at(2026, 11, 17, 4));
        let wake = utc(at(2026, 11, 18, 9));
        assert_eq!(decide(&inputs(wake, Some(last))), Decision::Run);
        // The run records its time; the same morning does not fire again.
        let after = utc(at(2026, 11, 18, 9));
        assert_eq!(
            decide(&inputs(after + Duration::minutes(5), Some(after))),
            Decision::Wait
        );
        // And the next slot is the next night.
        assert_eq!(
            decide(&inputs(utc(at(2026, 11, 18, 23)), Some(after))),
            Decision::Wait
        );
        assert_eq!(
            decide(&inputs(utc(at(2026, 11, 19, 4)), Some(after))),
            Decision::Run
        );
    }

    #[test]
    fn the_scheduler_waits_off_busy_and_battery_and_startup() {
        let now = utc(at(2026, 11, 18, 9));
        let last = Some(utc(at(2026, 11, 17, 4)));
        let mut i = inputs(now, last);
        i.nightly = false;
        assert_eq!(decide(&i), Decision::Wait);
        let mut i = inputs(now, last);
        i.since_launch = std::time::Duration::from_secs(30);
        assert_eq!(decide(&i), Decision::Wait);
        let mut i = inputs(now, last);
        i.busy = true;
        assert_eq!(decide(&i), Decision::Skip("skipped_busy"));
        let mut i = inputs(now, last);
        i.battery = Some(Battery {
            on_battery: true,
            percent: 19,
        });
        assert_eq!(decide(&i), Decision::Skip("skipped_battery"));
        i.battery = Some(Battery {
            on_battery: true,
            percent: 20,
        });
        assert_eq!(decide(&i), Decision::Run);
        i.battery = Some(Battery {
            on_battery: false,
            percent: 5,
        });
        assert_eq!(decide(&i), Decision::Run);
        // Never run yet: a first run is due.
        assert_eq!(decide(&inputs(now, None)), Decision::Run);
    }

    #[test]
    fn failed_slots_retry_after_an_hour_at_most_three_times() {
        let now = utc(at(2026, 11, 18, 9));
        let slot = prev_slot(now, NIGHTLY_AT).unwrap().to_rfc3339();
        let last = Some(utc(at(2026, 11, 17, 4)));
        let mut i = inputs(now, last);
        i.failed_slot = Some(&slot);
        i.failed_attempts = 1;
        i.last_run_at = Some(now - Duration::minutes(30));
        assert_eq!(decide(&i), Decision::Wait);
        i.last_run_at = Some(now - Duration::minutes(61));
        assert_eq!(decide(&i), Decision::Run);
        i.failed_attempts = 3;
        assert_eq!(decide(&i), Decision::Wait);
    }

    #[test]
    fn failures_and_skips_are_recorded_for_the_next_decision() {
        let (d, _) = setup("sched-state");
        let slot = "2026-11-18T04:00:00+09:00";
        record_failure(&d, slot, "store_error").unwrap();
        record_failure(&d, slot, "store_error").unwrap();
        let st = sched_state(&d).unwrap().unwrap();
        assert_eq!(
            (st.failed_slot.as_deref(), st.failed_attempts),
            (Some(slot), 2)
        );
        assert_eq!(st.last_result.as_deref(), Some("store_error"));
        record_failure(&d, "2026-11-19T04:00:00+09:00", "x").unwrap();
        assert_eq!(sched_state(&d).unwrap().unwrap().failed_attempts, 1);
        // A good run clears the failure state.
        evaluate_in(&d, at(2026, 11, 19, 5)).unwrap();
        let st = sched_state(&d).unwrap().unwrap();
        assert_eq!((st.failed_slot, st.failed_attempts), (None, 0));
        assert!(st.last_done.is_some());
        let none = temp_dir("sched-none");
        assert!(
            sched_state(&none).unwrap().is_none(),
            "no database, nothing to run"
        );
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(none);
    }

    // ---- source pins ----

    fn production() -> String {
        let src = include_str!("treasury_monitor.rs");
        src.split(concat!("#[cfg(test)]", "\nmod tests {"))
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn the_scheduler_is_started_only_from_setup_and_never_in_tests() {
        let lib = include_str!("../lib.rs");
        assert_eq!(lib.matches("treasury_monitor::start()").count(), 1);
        // Every `start` call site in the commands tree: none inside a test module or test fn.
        let src = production();
        assert!(src.contains("pub fn start()"));
        assert!(!src.contains("start()\n    .await"));
        let tests = include_str!("treasury_monitor.rs");
        let body = tests
            .split(concat!("#[cfg(test)]", "\nmod tests {"))
            .nth(1)
            .unwrap();
        assert!(
            !body.contains(concat!("super::", "start(")),
            "a test starts the scheduler"
        );
        assert!(!body.contains(concat!("tick", "(started")));
    }

    #[test]
    fn the_relay_call_passes_no_value_and_the_line_has_no_digits_or_names() {
        let src = production();
        // The monitor's only relay call is `send(&day)`: a date. No format! feeds relay text, and no
        // amount, name or detail is in scope at that call.
        let call_site = &src[src.find("let day = today_of(&now)").unwrap()..];
        let call_site = &call_site[..call_site.find("Ok(result)").unwrap()];
        assert!(call_site.contains("relay.send(&day)"));
        for banned in [
            "detail",
            "amount",
            "name",
            "format!(\"{",
            "total",
            "stream_id",
        ] {
            assert!(!call_site.contains(banned), "{banned}");
        }
        let line = crate::commands::notify_relay::TREASURY_LINE;
        assert!(
            !line.chars().any(|c| c.is_numeric()),
            "no digit of any script"
        );
        assert!(crate::commands::notify_relay::plain_line_ok(line));
        // The relay side builds nothing from flags either.
        let nr = include_str!("notify_relay.rs");
        let nr = &nr[nr.find("pub(crate) async fn dispatch_treasury<").unwrap()..];
        let nr = &nr[..nr.find("/// The live path for the monitor").unwrap()];
        assert!(nr.contains("TREASURY_LINE") && !nr.contains("format!"));
        assert!(
            !nr.contains("compose("),
            "no model writes the Treasury line"
        );
    }

    #[test]
    fn commands_avoid_the_banned_names_and_the_module_has_no_network_client() {
        let src = production();
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| l.contains("fn ")).unwrap();
                names.push(
                    f.split("fn ")
                        .nth(1)
                        .unwrap()
                        .split('(')
                        .next()
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
        }
        assert_eq!(names.len(), 5, "{names:?}");
        for n in &names {
            assert!(n.starts_with("treasury_"), "{n}");
            for b in BANNED {
                assert!(!n.contains(b), "{n} contains {b}");
            }
        }
        for needle in [
            "reqwest",
            "TcpStream",
            concat!("http", "://"),
            concat!("https", "://"),
            "Command::new",
        ] {
            assert!(!src.contains(needle), "{needle}");
        }
    }
}
