//! Proactive Life suggestions (decision 0056): which recommendation, if any, Amara may bring up now,
//! in the app (Life › Overview) and on Telegram (through `proactive.rs`'s existing send path).
//!
//! The rules live here, in Rust, so the app and Telegram can never disagree:
//! - **Settings:** `ProactiveSettings.nudges` (on/off, Telegram on/off, a daily limit, quiet hours).
//! - **Quiet hours:** no new suggestion between `quiet_from` and `quiet_to` (JST).
//! - **Frequency:** at most `max_per_day` new suggestions a JST day, app and Telegram together.
//! - **Evidence quality:** only recommendations with evidence, all of it `measured` or `self_report`
//!   (never `estimate`, `partial` or `missing`).
//! - **No repeats:** only a `proposed` recommendation is brought up, and bringing it up records
//!   `shown` once; the status machine refuses a second `shown`. A dismissed recommendation never comes
//!   back, and nothing with the same kind and target comes back for 30 days after a dismissal.
//!   A past `expires_at` is recorded as `expired` and never shown again.
//! - Takes have their own surface and are not brought up here.
//!
//! State `<data>/life/nudges.json` (0600) records what was brought up, by day and channel.

use super::amara_store::commands::{outcome_in, respond_in, RecommendationRow};
use super::amara_store::{store_for, Scope};
use super::local_store::{day_of, read_json, write_json_atomic};
use super::proactive::ProactiveNudges;
use chrono::{DateTime, Duration, FixedOffset, Timelike, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 128 * 1024;
const KEEP: usize = 300;
const DISMISS_BLOCK_DAYS: i64 = 30;
/// A suggestion already brought up stays in the Overview slot this long, then it is left alone.
const SHOWN_STAYS_DAYS: i64 = 7;
const TELEGRAM_RETRY_MIN: i64 = 30;
pub(crate) const GOOD_QUALITY: [&str; 2] = ["measured", "self_report"];
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Nudged {
    pub recommendation_id: String,
    pub day: String,
    /// `app` | `telegram`
    pub channel: String,
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    #[serde(default)]
    nudged: Vec<Nudged>,
    #[serde(default)]
    telegram_failed_at: Option<String>,
}

fn file(data: &Path) -> PathBuf {
    data.join("life").join("nudges.json")
}

fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

/// Inside quiet hours. Equal ends mean no quiet hours; a range may wrap past midnight.
pub(crate) fn in_quiet(now: DateTime<FixedOffset>, from: &str, to: &str) -> bool {
    let (Some(a), Some(b)) = (minutes(from), minutes(to)) else {
        return false;
    };
    let m = now.hour() * 60 + now.minute();
    match a.cmp(&b) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Less => m >= a && m < b,
        std::cmp::Ordering::Greater => m >= a || m < b,
    }
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub rec: RecommendationRow,
    /// When it was brought up (its `shown` outcome), if it was.
    pub shown_at: Option<String>,
}

/// Recommendations that pass the evidence and repeat rules, newest first. Expired ones are recorded
/// as `expired` on the way.
pub(crate) fn candidates(
    data: &Path,
    now: DateTime<FixedOffset>,
) -> Result<Vec<Candidate>, String> {
    let store = store_for(data, Scope::Personal);
    let rows = store.list_recommendations(Scope::Personal, None, 500)?;
    let conn = store.open()?;
    let now_utc = now.with_timezone(&Utc);
    let block_since = now_utc - Duration::days(DISMISS_BLOCK_DAYS);
    // (kind, target) pairs dismissed in the last 30 days.
    let mut dismissed: Vec<(String, Option<String>)> = Vec::new();
    {
        let mut st = conn
            .prepare(
                "SELECT r.kind, r.target_ref, o.at FROM recommendation_outcome o
                 JOIN recommendation r ON r.recommendation_id = o.recommendation_id
                 WHERE o.outcome = 'dismissed' AND r.scope = 'personal'",
            )
            .map_err(|e| format!("amara store: {e}"))?;
        let it = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("amara store: {e}"))?;
        for row in it.flatten() {
            if parse_time(&row.2).is_some_and(|t| t >= block_since) {
                dismissed.push((row.0, row.1));
            }
        }
    }
    let mut out = Vec::new();
    for rec in rows {
        if rec.kind == "take" || !matches!(rec.status.as_str(), "proposed" | "shown") {
            continue;
        }
        if rec
            .expires_at
            .as_deref()
            .and_then(parse_time)
            .is_some_and(|t| t <= now_utc)
        {
            let _ = outcome_in(
                data,
                "personal",
                &rec.recommendation_id,
                "expired",
                None,
                None,
                None,
            );
            continue;
        }
        let qualities: Vec<String> = conn
            .prepare("SELECT quality FROM recommendation_evidence WHERE recommendation_id = ?1")
            .and_then(|mut st| {
                st.query_map(params![rec.recommendation_id], |r| r.get::<_, String>(0))?
                    .collect()
            })
            .map_err(|e| format!("amara store: {e}"))?;
        if qualities.is_empty() || !qualities.iter().all(|q| GOOD_QUALITY.contains(&q.as_str())) {
            continue;
        }
        if dismissed
            .iter()
            .any(|(k, t)| *k == rec.kind && *t == rec.target_ref)
        {
            continue;
        }
        let shown_at: Option<String> = conn
            .query_row(
                "SELECT at FROM recommendation_outcome WHERE recommendation_id = ?1 AND outcome = 'shown'
                 ORDER BY at DESC LIMIT 1",
                params![rec.recommendation_id],
                |r| r.get(0),
            )
            .ok();
        out.push(Candidate { rec, shown_at });
    }
    Ok(out)
}

fn load(data: &Path) -> State {
    read_json(&file(data), MAX_BYTES).unwrap_or_default()
}

fn save(data: &Path, st: &mut State) -> Result<(), String> {
    let over = st.nudged.len().saturating_sub(KEEP);
    st.nudged.drain(..over);
    write_json_atomic(&file(data), st, MAX_BYTES)
}

fn count_today(st: &State, today: &str) -> u32 {
    st.nudged.iter().filter(|n| n.day == today).count() as u32
}

/// Why nothing new may be brought up now, or `None` when one may.
pub(crate) fn blocked(
    cfg: &ProactiveNudges,
    now: DateTime<FixedOffset>,
    st_count_today: u32,
) -> Option<&'static str> {
    if !cfg.enabled {
        return Some("off");
    }
    if in_quiet(now, &cfg.quiet_from, &cfg.quiet_to) {
        return Some("quiet_hours");
    }
    if st_count_today >= cfg.max_per_day {
        return Some("daily_limit");
    }
    None
}

/// Brings one proposed recommendation up: records `shown` once (refused if it moved meanwhile) and the
/// nudge itself. Under the state lock, so two callers can't both spend the last slot.
fn bring_up(
    data: &Path,
    cfg: &ProactiveNudges,
    now: DateTime<FixedOffset>,
    channel: &str,
    pick: &RecommendationRow,
    enforce: bool,
) -> Result<bool, String> {
    let today = day_of(&now).format("%Y-%m-%d").to_string();
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut st = load(data);
    if enforce && blocked(cfg, now, count_today(&st, &today)).is_some() {
        return Ok(false);
    }
    respond_in(data, "personal", &pick.recommendation_id, "shown", None)?;
    st.nudged.push(Nudged {
        recommendation_id: pick.recommendation_id.clone(),
        day: today,
        channel: channel.into(),
        at: now.to_rfc3339(),
    });
    save(data, &mut st)?;
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeInsight {
    pub recommendation: Option<RecommendationRow>,
    /// True when this read brought it up (and recorded `shown`).
    pub nudged: bool,
    /// When nothing new was brought up: `off`, `quiet_hours`, `daily_limit` or `none` (nothing qualifies).
    pub held: Option<String>,
}

/// The one suggestion for Life › Overview: the newest one already brought up in the last 7 days, else
/// a new one when the rules allow, else none.
pub(crate) fn insight_in(
    data: &Path,
    cfg: &ProactiveNudges,
    now: DateTime<FixedOffset>,
) -> Result<LifeInsight, String> {
    let c = candidates(data, now)?;
    let recent = now.with_timezone(&Utc) - Duration::days(SHOWN_STAYS_DAYS);
    if let Some(s) = c.iter().find(|x| {
        x.rec.status == "shown"
            && x.shown_at
                .as_deref()
                .and_then(parse_time)
                .is_some_and(|t| t >= recent)
    }) {
        return Ok(LifeInsight {
            recommendation: Some(s.rec.clone()),
            nudged: false,
            held: None,
        });
    }
    let Some(p) = c.iter().find(|x| x.rec.status == "proposed") else {
        return Ok(LifeInsight {
            recommendation: None,
            nudged: false,
            held: Some("none".into()),
        });
    };
    let today = day_of(&now).format("%Y-%m-%d").to_string();
    if let Some(why) = blocked(cfg, now, count_today(&load(data), &today)) {
        return Ok(LifeInsight {
            recommendation: None,
            nudged: false,
            held: Some(why.into()),
        });
    }
    if bring_up(data, cfg, now, "app", &p.rec, true)? {
        let mut rec = p.rec.clone();
        rec.status = "shown".into();
        return Ok(LifeInsight {
            recommendation: Some(rec),
            nudged: true,
            held: None,
        });
    }
    Ok(LifeInsight {
        recommendation: None,
        nudged: false,
        held: Some("daily_limit".into()),
    })
}

/// The Telegram text: the recommendation's own title and suggested action, then where to answer it.
pub(crate) fn telegram_text(r: &RecommendationRow) -> String {
    let action = r
        .suggested_action
        .as_deref()
        .map(|a| format!(" Suggested: {a}"))
        .unwrap_or_default();
    format!(
        "Amara suggestion: {}.{action} Open Life in Amara OS to accept, change or dismiss it.",
        r.title.trim_end_matches('.')
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TgOutcome {
    Sent,
    Failed,
}

/// One Telegram pass (called each proactive tick). Sends nothing while outbound is disabled, in quiet
/// hours, over the limit, or within 30 minutes of a failed send. Only a sent message records `shown`.
pub(crate) async fn telegram_in<F, Fut>(
    data: &Path,
    cfg: &ProactiveNudges,
    now: DateTime<FixedOffset>,
    disabled: bool,
    send: F,
) -> Option<TgOutcome>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = TgOutcome>,
{
    if disabled || !cfg.telegram {
        return None;
    }
    let today = day_of(&now).format("%Y-%m-%d").to_string();
    let st = load(data);
    if blocked(cfg, now, count_today(&st, &today)).is_some() {
        return None;
    }
    if st
        .telegram_failed_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| (now - t).num_minutes() < TELEGRAM_RETRY_MIN)
    {
        return None;
    }
    let pick = candidates(data, now)
        .ok()?
        .into_iter()
        .find(|c| c.rec.status == "proposed")?
        .rec;
    let outcome = send(telegram_text(&pick)).await;
    match outcome {
        TgOutcome::Sent => {
            // Already sent: record it whatever the limit says now, so it is never sent twice.
            if let Err(e) = bring_up(data, cfg, now, "telegram", &pick, false) {
                println!("[nudges] sent but not recorded: {e}");
            }
        }
        TgOutcome::Failed => {
            let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let mut st = load(data);
            st.telegram_failed_at = Some(now.to_rfc3339());
            let _ = save(data, &mut st);
        }
    }
    Some(outcome)
}

#[tauri::command]
#[specta::specta]
pub async fn life_insight() -> Result<LifeInsight, String> {
    super::shared::task::blocking(super::shared::task::JoinErr::Plain, || {
        let data = crate::app_paths::data_dir();
        let cfg = super::proactive::load_settings_in(&data).nudges;
        insight_in(&data, &cfg, super::shared::time::now_jst())
    })
    .await
}

#[cfg(test)]
mod tests;
