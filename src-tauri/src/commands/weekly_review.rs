//! Weekly review (HANDOFF-1111a §7). Read-only `weekly_review_read`.
//!
//! The review for the most recent Sunday 19:00 JST that has passed. There is no timer: when the app is
//! opened and the stored review is not for that Sunday, `weekly_review_read` builds it from the rows
//! that exist (NOOP daily metrics, Muse sessions, check-ins, goals, published patterns, and the personal
//! money store) and stores it in `<data dir>/weekly_review/review.json` (0600, atomic).
//!
//! Honesty rules: a section with no rows says so (`state: "none"`) and carries no numbers; every number
//! lists its source ids/dates; the suggestion is one published pattern with confidence >= 3, quoted
//! from the finding, or none. Personal money reads only `scope = personal` accounts and only USD
//! accounts get a number (no yen conversion rule exists yet). Business ventures are never read here:
//! they stay in Enterprise. The written summary (LLM prose) is not wired: `summary` is `null`.

use super::local_store::{day_of, iso, jst, now_jst, read_json, write_json_atomic};
use super::patterns::{self, DayRow, Inputs, MoodRow, PatternsResult, SessionRow};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 256 * 1024;
const MIN_NIGHTS_FOR_DELTA: usize = 3;
const SUGGESTION_MIN_CONFIDENCE: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
#[serde(rename = "WeeklyReviewHealth")]
pub struct Health {
    /// "data" | "none"
    pub state: String,
    pub nights: usize,
    pub avg_sleep_h: Option<f64>,
    pub prev_avg_sleep_h: Option<f64>,
    /// This week minus last week, hours. Only with >= 3 nights in both weeks.
    pub sleep_delta_h: Option<f64>,
    pub avg_hrv: Option<f64>,
    pub avg_recovery: Option<f64>,
    pub source_ids: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub state: String,
    pub focus_sessions: usize,
    pub avg_focus: Option<f64>,
    pub source_ids: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub text: String,
    pub day: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Checkins {
    pub state: String,
    pub count: usize,
    pub avg_mood: Option<f64>,
    pub avg_energy: Option<f64>,
    pub notes: Vec<Note>,
    pub source_ids: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
#[serde(rename = "WeeklyReviewMilestone")]
pub struct Milestone {
    pub id: String,
    pub title: String,
    pub current: f64,
    pub target: f64,
    pub unit: String,
    pub pct: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HabitWeek {
    pub id: String,
    pub title: String,
    /// Days marked done in the review week (Mon to Sun, up to the review day).
    pub done: u32,
    /// Days a week the habit is meant for.
    pub target: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoalsSection {
    pub state: String,
    pub milestones: Vec<Milestone>,
    pub habits: usize,
    /// Per-habit done/target from the habit-day store. Filled on every read (never stored), so a mark made
    /// after the review was built still counts. Old stored reviews load with none.
    #[serde(default)]
    pub habit_weeks: Vec<HabitWeek>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MoneyAccountWeek {
    pub account_id: String,
    pub currency: String,
    /// True for a non-USD account: no number is shown until Taisun gives a conversion rule.
    pub needs_conversion: bool,
    pub balance_usd: Option<f64>,
    pub as_of: Option<String>,
    /// Change over the week from two real balance points (USD only).
    pub change_usd: Option<f64>,
    pub change_from: Option<String>,
    pub change_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MoneySection {
    pub state: String,
    pub accounts: Vec<MoneyAccountWeek>,
    /// Sum of `change_usd` over USD accounts that have a real change; None when none do.
    pub personal_change_usd: Option<f64>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub title: String,
    pub reason: String,
    pub evidence_finding_id: String,
    pub confidence: u8,
    pub source_ids: Vec<String>,
    /// The whole published finding the suggestion quotes, so a stored review can be re-checked on load.
    pub finding: patterns::Finding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    /// "ready" (at least one section has data) | "not_enough_data"
    pub state: String,
    /// Monday and Sunday (JST) of the reviewed week.
    pub week_start: String,
    pub week_end: String,
    /// The Sunday 19:00 JST this review is for.
    pub due_at: String,
    pub computed_at: String,
    pub health: Health,
    pub work: Work,
    pub checkins: Checkins,
    pub goals: GoalsSection,
    pub money: MoneySection,
    pub suggestion: Option<Suggestion>,
    pub suggestion_note: String,
    /// LLM-written prose is not wired. Always null.
    pub summary: Option<String>,
    /// Results of "try it this week" experiments that fell due on or before this week's end
    /// (`experiments::results_for_review`: also carries results from earlier weeks that no stored review listed yet); empty when none. Filled by `read_in`, not `build`.
    #[serde(default)]
    pub experiments: Vec<super::experiments::ExperimentResult>,
}

// ───────────────────────────── schedule ─────────────────────────────

/// The most recent Sunday 19:00 JST at or before `now`.
pub fn last_due(now: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    let mut d = day_of(&now);
    while d.weekday() != Weekday::Sun {
        d -= Duration::days(1);
    }
    let at = |d: NaiveDate| {
        jst()
            .from_local_datetime(&d.and_hms_opt(19, 0, 0).expect("valid time"))
            .single()
            .expect("fixed offset")
    };
    if at(d) > now {
        d -= Duration::days(7);
    }
    at(d)
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn in_range(day: &str, from: NaiveDate, to: NaiveDate) -> bool {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d >= from && d <= to)
        .unwrap_or(false)
}

fn avg(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

// ───────────────────────────── build ─────────────────────────────

/// Pure: builds the review for `due` from the given rows. No clock, no disk.
pub fn build(
    inputs: &Inputs,
    patterns: &PatternsResult,
    goals: &[super::goals::Goal],
    money: MoneySection,
    due: DateTime<FixedOffset>,
    now: DateTime<FixedOffset>,
) -> Review {
    let end = day_of(&due);
    let start = end - Duration::days(6);
    let prev_start = start - Duration::days(7);
    let prev_end = start - Duration::days(1);

    let week_days: Vec<&DayRow> = inputs
        .days
        .iter()
        .filter(|d| in_range(&d.day, start, end))
        .collect();
    let prev_days: Vec<&DayRow> = inputs
        .days
        .iter()
        .filter(|d| in_range(&d.day, prev_start, prev_end))
        .collect();

    // Health
    let sleep: Vec<(&str, f64)> = week_days
        .iter()
        .filter_map(|d| d.sleep_h.map(|s| (d.day.as_str(), s)))
        .collect();
    let prev_sleep: Vec<f64> = prev_days.iter().filter_map(|d| d.sleep_h).collect();
    let hrv: Vec<f64> = week_days.iter().filter_map(|d| d.hrv).collect();
    let recovery: Vec<f64> = week_days.iter().filter_map(|d| d.recovery).collect();
    let sleep_vals: Vec<f64> = sleep.iter().map(|s| s.1).collect();
    let avg_sleep = avg(&sleep_vals);
    let delta = match (avg_sleep, avg(&prev_sleep)) {
        (Some(a), Some(b))
            if sleep_vals.len() >= MIN_NIGHTS_FOR_DELTA
                && prev_sleep.len() >= MIN_NIGHTS_FOR_DELTA =>
        {
            Some(a - b)
        }
        _ => None,
    };
    let mut health_ids: Vec<String> = Vec::new();
    for d in &week_days {
        if d.sleep_h.is_some() {
            health_ids.push(format!("noop:sleep_total_min:{}", d.day));
        }
        if d.hrv.is_some() {
            health_ids.push(format!("noop:hrv:{}", d.day));
        }
        if d.recovery.is_some() {
            health_ids.push(format!("noop:recovery:{}", d.day));
        }
    }
    let health_has = !(sleep.is_empty() && hrv.is_empty() && recovery.is_empty());
    let health = Health {
        state: if health_has { "data" } else { "none" }.into(),
        nights: sleep.len(),
        avg_sleep_h: avg_sleep,
        prev_avg_sleep_h: if delta.is_some() {
            avg(&prev_sleep)
        } else {
            None
        },
        sleep_delta_h: delta,
        avg_hrv: avg(&hrv),
        avg_recovery: avg(&recovery),
        source_ids: if health_has { health_ids } else { Vec::new() },
        note: if health_has {
            "From NOOP daily rows. Workouts are not included yet.".into()
        } else {
            "No sleep, HRV or recovery rows for this week.".into()
        },
    };

    // Work
    let sessions: Vec<&SessionRow> = inputs
        .sessions
        .iter()
        .filter(|s| in_range(&s.day, start, end))
        .collect();
    let focus: Vec<f64> = sessions.iter().filter_map(|s| s.focus).collect();
    let work = Work {
        state: if sessions.is_empty() { "none" } else { "data" }.into(),
        focus_sessions: sessions.len(),
        avg_focus: avg(&focus),
        source_ids: sessions
            .iter()
            .map(|s| format!("session:{}", s.id))
            .collect(),
        note: if sessions.is_empty() {
            "No Muse sessions this week.".into()
        } else {
            "Muse sessions only. Launch tasks and crew runs are not included yet.".into()
        },
    };

    // Check-ins
    let moods: Vec<&MoodRow> = inputs
        .moods
        .iter()
        .filter(|m| in_range(&m.day, start, end))
        .collect();
    let mood_vals: Vec<f64> = moods.iter().map(|m| m.mood).collect();
    let energy_vals: Vec<f64> = moods.iter().map(|m| m.energy).collect();
    let mut notes: Vec<Note> = moods
        .iter()
        .filter(|m| !m.text.trim().is_empty())
        .map(|m| Note {
            text: m.text.clone(),
            day: m.day.clone(),
            id: format!("checkin:{}", m.id),
        })
        .collect();
    notes.sort_by(|a, b| a.day.cmp(&b.day));
    notes.truncate(3);
    let checkins = Checkins {
        state: if moods.is_empty() { "none" } else { "data" }.into(),
        count: moods.len(),
        avg_mood: avg(&mood_vals),
        avg_energy: avg(&energy_vals),
        notes,
        source_ids: moods.iter().map(|m| format!("checkin:{}", m.id)).collect(),
        note: if moods.is_empty() {
            "No check-ins this week.".into()
        } else {
            String::new()
        },
    };

    // Goals (milestones from goals.json; habit day marks are not computed anywhere yet). Goals carry only a
    // free-text category, no venture id: those marked business/venture/enterprise belong to Enterprise and are
    // dropped here. Revenue goals filed under another category (e.g. "money") can't be told apart: open question.
    let goals: Vec<&super::goals::Goal> = goals.iter().filter(|g| !is_business_goal(g)).collect();
    let milestones: Vec<Milestone> = goals
        .iter()
        .filter(|g| g.category != "habit" && g.target > 0.0 && g.current.is_finite())
        .map(|g| Milestone {
            id: g.id.clone(),
            title: g.title.clone(),
            current: g.current,
            target: g.target,
            unit: g.unit.clone(),
            pct: ((g.current / g.target) * 100.0).clamp(0.0, 100.0).round() as u32,
        })
        .collect();
    let habits = goals.iter().filter(|g| g.category == "habit").count();
    let goals_section = GoalsSection {
        state: if milestones.is_empty() && habits == 0 {
            "none"
        } else {
            "data"
        }
        .into(),
        habits,
        habit_weeks: vec![],
        note: if milestones.is_empty() && habits == 0 {
            "No goals saved yet.".into()
        } else {
            String::new()
        },
        milestones,
    };

    // Suggestion: one published finding, confidence >= 3, quoted. Otherwise none.
    let top = patterns
        .findings
        .iter()
        .filter(|f| f.confidence >= SUGGESTION_MIN_CONFIDENCE)
        .max_by(|a, b| {
            a.confidence
                .cmp(&b.confidence)
                .then(a.r.abs().total_cmp(&b.r.abs()))
        });
    let suggestion = top.map(|f| Suggestion {
        title: match f.pair.as_str() {
            "sleep_focus" => "Protect 7 hours of sleep before your focus sessions.".to_string(),
            "effort_focus" => {
                "Keep hard workouts off the day before your focus sessions.".to_string()
            }
            "hrv_calm" => "Check your morning HRV before planning a calm session.".to_string(),
            _ => format!(
                "Notice how \"{}\" days go this week.",
                f.tag.clone().unwrap_or_default()
            ),
        },
        reason: format!("{} {}", f.sentence, f.detail),
        evidence_finding_id: f.id.clone(),
        confidence: f.confidence,
        source_ids: f.source_ids.clone(),
        finding: f.clone(),
    });
    let suggestion_note = if suggestion.is_some() {
        String::new()
    } else {
        "Amara suggests a change only from a pattern with confidence 3 or higher. None has reached that yet.".into()
    };

    let any = health_has
        || !sessions.is_empty()
        || !moods.is_empty()
        || goals_section.state == "data"
        || money.state == "data";
    Review {
        state: if any { "ready" } else { "not_enough_data" }.into(),
        week_start: ymd(start),
        week_end: ymd(end),
        due_at: iso(&due),
        computed_at: iso(&now),
        health,
        work,
        checkins,
        goals: goals_section,
        money,
        suggestion,
        suggestion_note,
        summary: None,
        experiments: Vec::new(),
    }
}

// ───────────────────────────── money (personal only) ─────────────────────────────

pub(crate) fn is_business_goal(g: &super::goals::Goal) -> bool {
    matches!(
        g.category.trim().to_lowercase().as_str(),
        "business" | "venture" | "ventures" | "enterprise"
    )
}

pub(crate) fn money_section(
    data: &Path,
    week_start: NaiveDate,
    week_end: NaiveDate,
) -> MoneySection {
    let none = |note: &str| MoneySection {
        state: "none".into(),
        accounts: Vec::new(),
        personal_change_usd: None,
        note: note.into(),
    };
    let Ok(accounts) = super::money::accounts_list_in(data) else {
        return none("The money store couldn't be read.");
    };
    let personal: Vec<_> = accounts
        .into_iter()
        .filter(|a| a.scope == "personal")
        .collect();
    if personal.is_empty() {
        return none("No personal accounts yet.");
    }
    let Ok(bal) = super::money::balances_list_scoped_in(data, week_end, Some("personal")) else {
        return none("The money store couldn't be read.");
    };
    let from = week_start - Duration::days(1);
    let mut rows = Vec::new();
    for a in &personal {
        let Some(b) = bal.balances.iter().find(|b| b.account_id == a.id) else {
            continue;
        };
        if a.currency != "USD" {
            rows.push(MoneyAccountWeek {
                account_id: a.id.clone(),
                currency: a.currency.clone(),
                needs_conversion: true,
                balance_usd: None,
                as_of: Some(b.as_of.clone()),
                change_usd: None,
                change_from: None,
                change_to: None,
            });
            continue;
        }
        let pts: Vec<_> = b
            .history
            .iter()
            .filter(|p| in_range(&p.date, from, week_end))
            .collect();
        let change = match (pts.first(), pts.last()) {
            (Some(f), Some(l)) if f.date != l.date => {
                Some((l.amount - f.amount, f.date.clone(), l.date.clone()))
            }
            _ => None,
        };
        rows.push(MoneyAccountWeek {
            account_id: a.id.clone(),
            currency: a.currency.clone(),
            needs_conversion: false,
            balance_usd: Some(b.amount),
            as_of: Some(b.as_of.clone()),
            change_usd: change.as_ref().map(|c| c.0),
            change_from: change.as_ref().map(|c| c.1.clone()),
            change_to: change.as_ref().map(|c| c.2.clone()),
        });
    }
    if rows.is_empty() {
        return none("Personal accounts have no balance yet.");
    }
    let changes: Vec<f64> = rows.iter().filter_map(|r| r.change_usd).collect();
    MoneySection {
        state: "data".into(),
        personal_change_usd: (!changes.is_empty()).then(|| changes.iter().sum()),
        note: if rows.iter().any(|r| r.needs_conversion) {
            "Yen accounts need a conversion rule before they count. Business money is in Enterprise.".into()
        } else {
            "Personal accounts only. Business money is in Enterprise.".into()
        },
        accounts: rows,
    }
}

// ───────────────────────────── store ─────────────────────────────

const MAX_STR: usize = 600;
const MAX_IDS: usize = 500;

/// A stored review is trusted only if it still passes the checks a fresh one does; otherwise it is stale.
pub(crate) fn valid_review(r: &Review) -> bool {
    let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let (Some(start), Some(end)) = (day(&r.week_start), day(&r.week_end)) else {
        return false;
    };
    let sect = |s: &str| s == "data" || s == "none";
    let ids = |v: &[String]| v.len() <= MAX_IDS && v.iter().all(|s| patterns::valid_source_id(s));
    let fin = |o: Option<f64>| o.is_none_or(f64::is_finite);
    let notes_ok = r.checkins.notes.len() <= 3
        && r.checkins.notes.iter().all(|n| {
            n.text.len() <= 2000 && day(&n.day).is_some() && patterns::valid_source_id(&n.id)
        });
    let suggestion_ok = match &r.suggestion {
        None => true,
        Some(s) => {
            s.confidence >= SUGGESTION_MIN_CONFIDENCE
                && s.confidence == s.finding.confidence
                && s.evidence_finding_id == s.finding.id
                && patterns::valid_finding(&s.finding)
                && s.source_ids == s.finding.source_ids
                && s.title.len() <= MAX_STR
                && s.reason.len() <= 2 * MAX_STR + 1
        }
    };
    let any = r.health.state == "data"
        || r.work.state == "data"
        || r.checkins.state == "data"
        || r.goals.state == "data"
        || r.money.state == "data";
    (r.state == "ready" && any || r.state == "not_enough_data" && !any)
        && end.weekday() == Weekday::Sun
        && end - start == Duration::days(6)
        && sect(&r.health.state)
        && sect(&r.work.state)
        && sect(&r.checkins.state)
        && sect(&r.goals.state)
        && sect(&r.money.state)
        && r.health.nights <= 7
        && fin(r.health.avg_sleep_h)
        && fin(r.health.sleep_delta_h)
        && fin(r.health.avg_hrv)
        && fin(r.health.avg_recovery)
        && fin(r.work.avg_focus)
        && fin(r.checkins.avg_mood)
        && fin(r.checkins.avg_energy)
        && fin(r.money.personal_change_usd)
        && ids(&r.health.source_ids)
        && ids(&r.work.source_ids)
        && ids(&r.checkins.source_ids)
        && r.work.focus_sessions == r.work.source_ids.len()
        && r.checkins.count == r.checkins.source_ids.len()
        && notes_ok
        && r.money.accounts.len() <= 50
        && r.goals.milestones.len() <= 200
        && r.experiments.len() <= super::experiments::MAX_EXPERIMENTS
        && suggestion_ok
        && r.summary.is_none()
        && [
            &r.health.note,
            &r.work.note,
            &r.checkins.note,
            &r.goals.note,
            &r.money.note,
            &r.suggestion_note,
        ]
        .iter()
        .all(|t| t.len() <= MAX_STR)
}

fn file(dir: &Path) -> PathBuf {
    dir.join("weekly_review").join("review.json")
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
struct Stored(Option<Review>);

static READ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct Gathered {
    pub inputs: Inputs,
    pub patterns: PatternsResult,
    pub goals: Vec<super::goals::Goal>,
    pub money: MoneySection,
}

/// Stored review when it is for the latest due Sunday; otherwise gather, build and (unless a source was
/// missing) store.
pub(crate) fn read_in(
    dir: &Path,
    now: DateTime<FixedOffset>,
    gather: impl FnOnce(NaiveDate, NaiveDate) -> Gathered,
) -> Result<Review, String> {
    let due = last_due(now);
    let end = day_of(&due);
    let start = end - Duration::days(6);
    let _g = READ_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(r) = read_json::<Stored>(&file(dir), MAX_BYTES)
        .ok()
        .and_then(|s| s.0)
        .filter(valid_review)
    {
        if r.week_end == ymd(end) {
            return Ok(r);
        }
        // About to replace an older stored review: its experiment results count as listed (covers
        // reviews stored before the ledger existed). A failed ledger write is not worth losing the review.
        let ids: Vec<i64> = r.experiments.iter().map(|e| e.followup_id).collect();
        let _ = super::experiments::mark_listed(dir, &r.week_end, &ids);
    }
    let g = gather(start, end);
    let mut review = build(&g.inputs, &g.patterns, &g.goals, g.money, due, now);
    review.experiments = super::experiments::results_for_review(dir, &ymd(end));
    if !g.inputs.partial && review.state == "ready" {
        write_json_atomic(&file(dir), &Stored(Some(review.clone())), MAX_BYTES)?;
        // Only a stored review counts as having listed a result.
        let ids: Vec<i64> = review.experiments.iter().map(|e| e.followup_id).collect();
        let _ = super::experiments::mark_listed(dir, &ymd(end), &ids);
    }
    Ok(review)
}

fn gather_live(
    dir: &Path,
    now: DateTime<FixedOffset>,
) -> impl FnOnce(NaiveDate, NaiveDate) -> Gathered + '_ {
    move |start, end| {
        // One gather feeds both the review and (if its file is stale) the patterns recompute.
        let inputs = patterns::gather_live(patterns::WINDOW_DAYS);
        let p = patterns::read_in(dir, now, || inputs.clone())
            .unwrap_or_else(|_| patterns::compute(&Inputs::default(), now));
        let goals = read_json::<Vec<super::goals::Goal>>(&dir.join("goals.json"), MAX_BYTES * 8)
            .unwrap_or_default();
        Gathered {
            inputs,
            patterns: p,
            goals,
            money: money_section(dir, start, end),
        }
    }
}

/// A goal with a roadmap measures progress by its steps, not by the stale `current / target` in goals.json
/// (a step marked Yes moves the roadmap percent only). Applied on every read, never stored, so a step done
/// after the Sunday review was built still shows. Milestones become "<done> of <total> steps"; a goal that has
/// a roadmap but no target gets a milestone too. Roadmaps still being drafted (no live steps) change nothing.
pub(crate) fn overlay_roadmaps(
    review: &mut Review,
    goals: &[super::goals::Goal],
    store: &super::goal_roadmap::Store,
) {
    for g in goals
        .iter()
        .filter(|g| g.category != "habit" && !is_business_goal(g))
    {
        let Some(rm) = store.roadmaps.iter().find(|r| r.goal_id == g.id) else {
            continue;
        };
        let live: Vec<_> = rm.steps.iter().filter(|s| !s.deferred).collect();
        if live.is_empty() {
            continue;
        }
        let m = Milestone {
            id: g.id.clone(),
            title: g.title.clone(),
            current: live.iter().filter(|s| s.done).count() as f64,
            target: live.len() as f64,
            unit: "steps".into(),
            pct: rm.pct.min(100),
        };
        match review.goals.milestones.iter_mut().find(|x| x.id == g.id) {
            Some(slot) => *slot = m,
            None => review.goals.milestones.push(m),
        }
    }
    if !review.goals.milestones.is_empty() && review.goals.state == "none" {
        review.goals.state = "data".into();
    }
}

/// Per-habit done/target for the review week from the habit-day store. Applied on every read, never stored.
/// Business-category and paused habits are left out. An unreadable habit log leaves a plain note instead of
/// invented numbers.
pub(crate) fn overlay_habits(
    review: &mut Review,
    goals: &[super::goals::Goal],
    rows: Result<Vec<super::habits::HabitDayRow>, String>,
) {
    review.goals.habit_weeks.clear();
    let habits: Vec<&super::goals::Goal> = goals
        .iter()
        .filter(|g| g.category == "habit" && !is_business_goal(g) && !g.paused)
        .collect();
    if habits.is_empty() {
        return;
    }
    let (Ok(start), Ok(end)) = (
        NaiveDate::parse_from_str(&review.week_start, "%Y-%m-%d"),
        NaiveDate::parse_from_str(&review.week_end, "%Y-%m-%d"),
    ) else {
        return;
    };
    let Ok(rows) = rows else {
        review.goals.note = "Habit marks couldn't be read, so habits show no counts.".into();
        return;
    };
    review.goals.habit_weeks = habits
        .iter()
        .map(|g| HabitWeek {
            id: g.id.clone(),
            title: g.title.clone(),
            done: rows
                .iter()
                .filter(|r| r.habit_id == g.id && r.done)
                .filter(|r| in_range(&r.day, start, end))
                .count() as u32,
            target: super::habits::target_of(g),
        })
        .collect();
}

#[tauri::command]
#[specta::specta]
pub async fn weekly_review_read() -> Result<Review, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let dir = crate::app_paths::data_dir();
        let now = now_jst();
        let mut review = read_in(&dir, now, gather_live(&dir, now))?;
        let goals = read_json::<Vec<super::goals::Goal>>(&dir.join("goals.json"), MAX_BYTES * 8)
            .unwrap_or_default();
        if let Ok(store) = super::goal_roadmap::read_store_at(&dir) {
            overlay_roadmaps(&mut review, &goals, &store);
        }
        overlay_habits(
            &mut review,
            &goals,
            super::habits::rows_in(&super::habits::dir_in(&dir)),
        );
        Ok(review)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::patterns::tests::{at, enough};
    use super::*;

    fn no_money() -> MoneySection {
        MoneySection {
            state: "none".into(),
            accounts: vec![],
            personal_change_usd: None,
            note: "No personal accounts yet.".into(),
        }
    }

    fn goal(id: &str, cat: &str, target: f64, current: f64) -> super::super::goals::Goal {
        super::super::goals::Goal {
            id: id.into(),
            title: id.into(),
            category: cat.into(),
            target,
            current,
            unit: "USD".into(),
            deadline: None,
            created_at: String::new(),
            updated_at: String::new(),
            venture_id: None,
            why: None,
            remind_at: None,
            paused: false,
            life: Default::default(),
        }
    }

    fn gathered(inputs: Inputs, now: DateTime<FixedOffset>) -> Gathered {
        Gathered {
            patterns: patterns::compute(&inputs, now),
            inputs,
            goals: vec![],
            money: no_money(),
        }
    }

    /// A2: the review's health section reads days from the daily table; everything else stays per-row
    /// (per-session count, per-check-in means, check-in text are not in a per-day row).
    #[test]
    fn roadmap_progress_replaces_stale_goal_numbers_in_the_review() {
        use super::super::goal_roadmap::{Roadmap, Step, StepSize, Store};
        let step = |id: &str, size: StepSize, done: bool, deferred: bool| Step {
            id: id.into(),
            title: id.into(),
            why: None,
            size,
            order: 1,
            done,
            done_at: None,
            not_yet_count: 0,
            smaller_suggestion: None,
            last_asked: None,
            deferred,
        };
        let store = Store {
            roadmaps: vec![
                Roadmap {
                    goal_id: "g1".into(),
                    status: "accepted".into(),
                    drafted_at: None,
                    steps: vec![
                        step("a", StepSize::Small, true, false),
                        step("b", StepSize::Small, false, false),
                        step("ghost", StepSize::Big, false, true),
                    ],
                    pct: 50,
                    version: 2,
                    moved_at: None,
                    plan_note: None,
                },
                Roadmap {
                    goal_id: "g2".into(),
                    status: "drafting".into(),
                    drafted_at: None,
                    steps: vec![],
                    pct: 0,
                    version: 0,
                    moved_at: None,
                    plan_note: None,
                },
            ],
            ..Store::default()
        };
        let now = at(2026, 11, 12, 10);
        let g = vec![
            goal("g1", "personal", 25.0, 17.0),
            goal("g2", "personal", 10.0, 2.0),
            goal("g3", "personal", 0.0, 0.0),
        ];
        let mut r = build(
            &Inputs::default(),
            &patterns::compute(&Inputs::default(), now),
            &g,
            no_money(),
            last_due(now),
            now,
        );
        assert_eq!(
            r.goals
                .milestones
                .iter()
                .find(|m| m.id == "g1")
                .unwrap()
                .pct,
            68
        );
        overlay_roadmaps(&mut r, &g, &store);
        let m1 = r.goals.milestones.iter().find(|m| m.id == "g1").unwrap();
        assert_eq!(
            (m1.pct, m1.current, m1.target, m1.unit.as_str()),
            (50, 1.0, 2.0, "steps")
        );
        // still drafting: the old numbers stay; a goal with no target and no roadmap gets nothing
        let m2 = r.goals.milestones.iter().find(|m| m.id == "g2").unwrap();
        assert_eq!((m2.pct, m2.unit.as_str()), (20, "USD"));
        assert!(r.goals.milestones.iter().all(|m| m.id != "g3"));
    }

    #[test]
    fn review_is_identical_with_days_taken_from_the_daily_table() {
        use super::super::patterns::tests::{stored_rows, world};
        let now = at(2026, 11, 12, 10);
        let (src, mut old) = world();
        // The live NOOP gatherer yields days ascending (a BTreeMap); the table reads ascending too. The
        // review's id order and sum order follow input order, so compare like with like.
        old.days.sort_by(|a, b| a.day.cmp(&b.day));
        let (dir, rows) = stored_rows(&src, now, "wrdaily");
        let mut via_table = old.clone();
        via_table.days = patterns::day_rows(rows.iter());
        let due = last_due(now);
        let a = build(
            &old,
            &patterns::compute(&old, now),
            &[],
            no_money(),
            due,
            now,
        );
        let b = build(
            &via_table,
            &patterns::compute_daily(&rows, now),
            &[],
            no_money(),
            due,
            now,
        );
        assert_eq!(a, b);
        assert_eq!(a.health.state, "data");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn due_is_the_latest_sunday_1900_jst() {
        // 2026-11-12 is a Thursday: the due Sunday is 2026-11-08 19:00.
        assert_eq!(
            iso(&last_due(at(2026, 11, 12, 10))),
            "2026-11-08T19:00:00.000+09:00"
        );
        // Sunday 18:59 is still the previous Sunday; 19:00 is that day.
        assert_eq!(
            day_of(&last_due(at(2026, 11, 8, 18))).to_string(),
            "2026-11-01"
        );
        assert_eq!(
            day_of(&last_due(at(2026, 11, 8, 19))).to_string(),
            "2026-11-08"
        );
    }

    #[test]
    fn empty_week_says_so_in_every_section() {
        let now = at(2026, 11, 12, 10);
        let r = build(
            &Inputs::default(),
            &patterns::compute(&Inputs::default(), now),
            &[],
            no_money(),
            last_due(now),
            now,
        );
        assert_eq!(r.state, "not_enough_data");
        for s in [
            &r.health.state,
            &r.work.state,
            &r.checkins.state,
            &r.goals.state,
            &r.money.state,
        ] {
            assert_eq!(s, "none");
        }
        assert!(
            r.health.avg_sleep_h.is_none()
                && r.work.avg_focus.is_none()
                && r.checkins.avg_mood.is_none()
        );
        assert!(r.suggestion.is_none() && !r.suggestion_note.is_empty());
        assert!(r.summary.is_none());
    }

    #[test]
    fn numbers_trace_to_rows_inside_the_week() {
        let now = at(2026, 11, 12, 10);
        let mut inputs = enough();
        inputs.moods.push(MoodRow {
            id: "c1".into(),
            day: "2026-11-05".into(),
            mood: 4.0,
            energy: 3.0,
            tags: vec![],
            text: "Slept well.".into(),
        });
        let p = patterns::compute(&inputs, now);
        let r = build(
            &inputs,
            &p,
            &[goal("g1", "money", 5000.0, 1250.0)],
            no_money(),
            last_due(now),
            now,
        );
        // Week is Mon 2026-11-02 .. Sun 2026-11-08.
        assert_eq!(
            (r.week_start.as_str(), r.week_end.as_str()),
            ("2026-11-02", "2026-11-08")
        );
        let week_sleep: Vec<f64> = inputs
            .days
            .iter()
            .filter(|d| {
                in_range(
                    &d.day,
                    NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
                    NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
                )
            })
            .filter_map(|d| d.sleep_h)
            .collect();
        assert_eq!(r.health.nights, week_sleep.len());
        assert_eq!(r.health.avg_sleep_h, avg(&week_sleep));
        assert!(r.health.sleep_delta_h.is_some());
        assert!(r.health.source_ids.iter().all(|s| s.starts_with("noop:")
            && [
                "2026-11-02",
                "2026-11-03",
                "2026-11-04",
                "2026-11-05",
                "2026-11-06",
                "2026-11-07",
                "2026-11-08"
            ]
            .iter()
            .any(|d| s.ends_with(d))));
        assert_eq!(r.work.focus_sessions, 7);
        assert_eq!(r.work.source_ids.len(), 7);
        assert_eq!(r.checkins.count, 1);
        assert_eq!(r.checkins.notes[0].text, "Slept well.");
        assert_eq!(r.goals.milestones[0].pct, 25);
        assert_eq!(r.state, "ready");
    }

    #[test]
    fn suggestion_needs_confidence_three_and_quotes_the_finding() {
        let now = at(2026, 11, 12, 10);
        let inputs = enough();
        let mut p = patterns::compute(&inputs, now);
        let top = p.findings[0].clone();
        let r = build(&inputs, &p, &[], no_money(), last_due(now), now);
        if top.confidence >= 3 {
            let s = r.suggestion.unwrap();
            assert_eq!(s.evidence_finding_id, top.id);
            assert!(s.reason.contains(&top.sentence));
        }
        // A finding at confidence 2 never becomes a suggestion.
        for f in &mut p.findings {
            f.confidence = 2;
        }
        assert!(build(&inputs, &p, &[], no_money(), last_due(now), now)
            .suggestion
            .is_none());
    }

    #[test]
    fn computes_on_read_when_due_and_reuses_within_the_week() {
        let dir = temp_dir("wr");
        let calls = std::cell::Cell::new(0);
        let g = |now| {
            move |_s: NaiveDate, _e: NaiveDate| {
                let _ = &now;
                gathered(enough(), at(2026, 11, 12, 10))
            }
        };
        let t1 = at(2026, 11, 12, 10);
        let first = read_in(&dir, t1, |s, e| {
            calls.set(calls.get() + 1);
            g(t1)(s, e)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(file(&dir).exists());
        // Same due Sunday, later in the week: stored review, gather not called.
        let again = read_in(&dir, at(2026, 11, 14, 9), |s, e| {
            calls.set(calls.get() + 1);
            g(t1)(s, e)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        // (compare fields: a JSON round-trip can move a float by 1 ulp)
        assert_eq!(
            (
                again.week_end.as_str(),
                again.computed_at.as_str(),
                again.state.as_str()
            ),
            (
                first.week_end.as_str(),
                first.computed_at.as_str(),
                first.state.as_str()
            )
        );
        // After the next Sunday 19:00: a new review for the new week (stale one is replaced).
        let t2 = at(2026, 11, 15, 19);
        let next = read_in(&dir, t2, |s, e| {
            calls.set(calls.get() + 1);
            g(t2)(s, e)
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(next.week_end, "2026-11-15");
        // Just before 19:00 Sunday it is still last week's.
        let before = read_in(&dir, at(2026, 11, 22, 18), |s, e| {
            calls.set(calls.get() + 1);
            g(t2)(s, e)
        })
        .unwrap();
        assert_eq!(before.week_end, "2026-11-15");
        assert_eq!(calls.get(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_or_partial_reviews_are_not_stored() {
        let dir = temp_dir("wre");
        let now = at(2026, 11, 12, 10);
        let r = read_in(&dir, now, |_, _| gathered(Inputs::default(), now)).unwrap();
        assert_eq!(r.state, "not_enough_data");
        assert!(!file(&dir).exists());
        let mut partial = enough();
        partial.partial = true;
        read_in(&dir, now, |_, _| gathered(partial, now)).unwrap();
        assert!(!file(&dir).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[cfg(unix)]
    fn stored_review_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("wr6");
        let now = at(2026, 11, 12, 10);
        read_in(&dir, now, |_, _| gathered(enough(), now)).unwrap();
        assert_eq!(
            std::fs::metadata(file(&dir)).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_money_fixture(dir: &Path) {
        use super::super::money::MoneyAccount;
        let acct = |id: &str, scope: &str, cur: &str| MoneyAccount {
            id: id.into(),
            nickname: id.into(),
            institution: "bank".into(),
            kind: "checking".into(),
            route: "csv".into(),
            currency: cur.into(),
            scope: scope.into(),
            created_at: "2026-11-01T00:00:00.000+09:00".into(),
        };
        let accounts = vec![
            acct("pers-usd", "personal", "USD"),
            acct("pers-jpy", "personal", "JPY"),
            acct("biz-usd", "venture", "USD"),
        ];
        let m = dir.join("money");
        std::fs::create_dir_all(m.join("tx")).unwrap();
        std::fs::write(
            m.join("accounts.json"),
            serde_json::to_string(&accounts).unwrap(),
        )
        .unwrap();
        let tx = |id: &str, rows: &[(&str, i64)]| {
            let text: String = rows
                .iter()
                .enumerate()
                .map(|(i, (d, b))| format!("{{\"key\":\"k{i}\",\"date\":\"{d}\",\"amount\":-1,\"balance\":{b},\"description\":\"x\"}}\n"))
                .collect();
            std::fs::write(m.join("tx").join(format!("{id}.jsonl")), text).unwrap();
        };
        tx("pers-usd", &[("2026-11-02", 1000), ("2026-11-07", 1250)]);
        tx(
            "pers-jpy",
            &[("2026-11-02", 500_000), ("2026-11-07", 480_000)],
        );
        tx(
            "biz-usd",
            &[("2026-11-02", 777_777), ("2026-11-07", 888_888)],
        );
    }

    #[test]
    fn money_is_personal_only_usd_only_and_never_includes_business() {
        let dir = temp_dir("wrm");
        write_money_fixture(&dir);
        let s = money_section(
            &dir,
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        );
        assert_eq!(s.state, "data");
        let usd = s
            .accounts
            .iter()
            .find(|a| a.account_id == "pers-usd")
            .unwrap();
        assert_eq!(
            (usd.change_usd, usd.balance_usd),
            (Some(250.0), Some(1250.0))
        );
        assert_eq!(
            (usd.change_from.as_deref(), usd.change_to.as_deref()),
            (Some("2026-11-02"), Some("2026-11-07"))
        );
        let jpy = s
            .accounts
            .iter()
            .find(|a| a.account_id == "pers-jpy")
            .unwrap();
        assert!(jpy.needs_conversion && jpy.balance_usd.is_none() && jpy.change_usd.is_none());
        assert_eq!(s.personal_change_usd, Some(250.0));
        let json = serde_json::to_string(&s).unwrap();
        for leak in ["biz-usd", "777777", "888888", "500000", "480000", "¥"] {
            assert!(!json.contains(leak), "{leak} leaked");
        }
        // Business numbers do not move the personal result, and vice versa.
        let before = serde_json::to_string(&s).unwrap();
        std::fs::write(dir.join("money").join("tx").join("biz-usd.jsonl"), "").unwrap();
        let after = serde_json::to_string(&money_section(
            &dir,
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        ))
        .unwrap();
        assert_eq!(before, after);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn money_section_is_none_without_personal_data() {
        let dir = temp_dir("wrn");
        let (a, b) = (
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        );
        assert_eq!(money_section(&dir, a, b).state, "none");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_poisoned_or_unreadable_venture_file_is_never_opened_by_the_personal_read() {
        let dir = temp_dir("wrp");
        write_money_fixture(&dir);
        let biz = dir.join("money").join("tx").join("biz-usd.jsonl");
        let (a, b) = (
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
        );
        let clean = serde_json::to_string(&money_section(&dir, a, b)).unwrap();
        // Garbage content: the unscoped read fails on it (so the file really is opened there)...
        std::fs::write(&biz, "{not json\n").unwrap();
        assert!(super::super::money::balances_list_in(&dir, b).is_err());
        // ...the personal read never opens it and is unchanged.
        assert_eq!(
            serde_json::to_string(&money_section(&dir, a, b)).unwrap(),
            clean
        );
        // Unreadable (mode 000) and a directory in place of the file: same.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&biz, "x").unwrap();
            std::fs::set_permissions(&biz, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(
                serde_json::to_string(&money_section(&dir, a, b)).unwrap(),
                clean
            );
            std::fs::set_permissions(&biz, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::remove_file(&biz).unwrap();
        std::fs::create_dir(&biz).unwrap();
        assert_eq!(
            serde_json::to_string(&money_section(&dir, a, b)).unwrap(),
            clean
        );
        // The scoped read never reads fx.
        let scoped =
            super::super::money::balances_list_scoped_in(&dir, b, Some("personal")).unwrap();
        assert!(scoped.fx.is_none());
        assert!(scoped
            .balances
            .iter()
            .all(|x| x.account_id.starts_with("pers-")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn business_category_goals_stay_out_of_the_personal_review() {
        let now = at(2026, 11, 12, 10);
        let p = patterns::compute(&Inputs::default(), now);
        let goals = [
            goal("mine", "health", 10.0, 5.0),
            goal("biz", "Business", 5000.0, 1820.0),
            goal("ven", "venture", 10.0, 1.0),
            goal("hab", "habit", 0.0, 0.0),
        ];
        let r = build(
            &Inputs::default(),
            &p,
            &goals,
            no_money(),
            last_due(now),
            now,
        );
        assert_eq!(
            r.goals
                .milestones
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["mine"]
        );
        assert_eq!(r.goals.habits, 1);
        assert!(!serde_json::to_string(&r).unwrap().contains("1820"));
    }

    #[test]
    fn a_tampered_cached_review_is_recomputed() {
        let dir = temp_dir("wrt");
        let now = at(2026, 11, 12, 10);
        let good = read_in(&dir, now, |_, _| gathered(enough(), now)).unwrap();
        assert!(valid_review(&good));
        assert!(
            good.suggestion.is_some(),
            "fixture has a confidence >= 3 finding"
        );
        let tamper = |edit: &dyn Fn(&mut Review)| {
            let mut r = good.clone();
            edit(&mut r);
            write_json_atomic(&file(&dir), &Stored(Some(r)), MAX_BYTES).unwrap();
            let calls = std::cell::Cell::new(0);
            let out = read_in(&dir, now, |_, _| {
                calls.set(calls.get() + 1);
                gathered(enough(), now)
            })
            .unwrap();
            assert_eq!(
                calls.get(),
                1,
                "an invalid cached review must be recomputed"
            );
            assert!(valid_review(&out));
        };
        tamper(&|r| r.suggestion.as_mut().unwrap().confidence = 2);
        tamper(&|r| r.suggestion.as_mut().unwrap().finding.confidence = 2);
        tamper(&|r| r.suggestion.as_mut().unwrap().evidence_finding_id = "nope".into());
        tamper(&|r| r.suggestion.as_mut().unwrap().finding.source_ids.clear());
        tamper(&|r| r.work.source_ids.push("session:../x".into()));
        tamper(&|r| r.work.focus_sessions += 1);
        tamper(&|r| r.summary = Some("made up prose".into()));
        tamper(&|r| r.week_end = "2026-11-09".into());
        tamper(&|r| r.money.note = "x".repeat(601));
        // Untouched cache is reused.
        write_json_atomic(&file(&dir), &Stored(Some(good.clone())), MAX_BYTES).unwrap();
        let again = read_in(&dir, now, |_, _| panic!("valid cache must be reused")).unwrap();
        assert_eq!(again.week_end, good.week_end);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A5 wiring: experiments whose window ended inside the review week ride on the review.
    #[test]
    fn review_carries_experiments_due_this_week_and_is_empty_otherwise() {
        use super::super::daily_table::{write_in, DailyRow};
        use super::super::experiments::record_start_in;
        let dir = temp_dir("wrexp");
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let now = at(2026, 11, 12, 10); // review week 2026-11-02..2026-11-08
        let g = |_s: NaiveDate, _e: NaiveDate| gathered(enough(), now);
        // None recorded: empty list.
        let none = read_in(&dir, now, g).unwrap();
        assert!(none.experiments.is_empty());
        let _ = std::fs::remove_dir_all(&dir);

        let dir = temp_dir("wrexp2");
        // Experiment A starts 2026-11-01 (due 11-08, this week): clear rise in focus.
        // Experiment B starts 2026-11-01 with no measure: "no clear difference".
        // Experiment C starts 2026-10-25 (due 11-01, last week): not in this review.
        record_start_in(&dir, 1, Some("sleep_focus"), d("2026-11-01")).unwrap();
        record_start_in(&dir, 2, None, d("2026-11-01")).unwrap();
        record_start_in(&dir, 3, Some("sleep_focus"), d("2026-10-25")).unwrap();
        let rows: Vec<DailyRow> = (0..14)
            .map(|i| {
                let day = d("2026-10-25") + Duration::days(i);
                let wobble = (i % 3) as f64;
                DailyRow {
                    day: day.format("%Y-%m-%d").to_string(),
                    focus_mean: Some(if day < d("2026-11-01") { 50.0 } else { 80.0 } + wobble),
                    ..Default::default()
                }
            })
            .collect();
        write_in(&dir, &rows, &[], &[], 0, (d("2026-10-25"), d("2026-11-07"))).unwrap();
        let r = read_in(&dir, now, g).unwrap();
        assert_eq!(r.week_end, "2026-11-08");
        // Changed by decision 0022 item 2: C (due 11-01, the week before) was never listed by an
        // earlier stored review, so this first stored review carries it (was: only A and B).
        assert_eq!(r.experiments.len(), 3);
        let a = r.experiments.iter().find(|e| e.followup_id == 1).unwrap();
        assert_eq!((a.outcome.as_str(), a.label.as_str()), ("better", "better"));
        assert!(a.after.mean > a.before.mean);
        let b = r.experiments.iter().find(|e| e.followup_id == 2).unwrap();
        assert_eq!(b.label, "no clear difference");
        assert!(r.experiments.iter().any(|e| e.followup_id == 3));
        // Stored with the review: a re-read in the same week returns the same list.
        let again = read_in(&dir, at(2026, 11, 13, 9), |_, _| panic!("stored")).unwrap();
        assert_eq!(again.experiments.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Decision 0022 item 2: carry-over across stored reviews.
    #[test]
    fn experiment_results_carry_over_once_and_expire_after_28_days() {
        use super::super::daily_table::{write_in, DailyRow};
        use super::super::experiments::record_start_in;
        let dir = temp_dir("wrcarry");
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let ids = |r: &Review| {
            let mut v: Vec<i64> = r.experiments.iter().map(|e| e.followup_id).collect();
            v.sort();
            v
        };
        let rows: Vec<DailyRow> = (0..80)
            .map(|i| DailyRow {
                day: (d("2026-09-20") + Duration::days(i)).to_string(),
                focus_mean: Some(60.0 + (i % 3) as f64),
                ..Default::default()
            })
            .collect();
        write_in(&dir, &rows, &[], &[], 0, (d("2026-09-20"), d("2026-12-08"))).unwrap();
        // Weeks end Sundays: 11-01, 11-08, 11-15, 11-22, 11-29, 12-06.
        // 1: start 10-25, due 11-01 (this week of the first review).
        // 2: start 11-04, due 11-11: falls in week ending 11-15, but 11-08 review is skipped (never opened).
        // 3: start 10-01, due 10-08: more than 28 days before the first review week end 11-01.
        record_start_in(&dir, 1, Some("sleep_focus"), d("2026-10-25")).unwrap();
        record_start_in(&dir, 2, Some("sleep_focus"), d("2026-11-04")).unwrap();
        record_start_in(&dir, 3, Some("sleep_focus"), d("2026-10-01")).unwrap();
        let read = |now: DateTime<FixedOffset>| {
            read_in(&dir, now, |_, _| gathered(enough(), at(2026, 11, 12, 10))).unwrap()
        };
        // Review for week ending 11-01: 1 is due this week; 3 is 24 days past due -> still listed.
        let r1 = read(at(2026, 11, 2, 10));
        assert_eq!((r1.week_end.as_str(), ids(&r1)), ("2026-11-01", vec![1, 3]));
        // Week ending 11-08 never opened. Next stored review (11-15) lists 2 (due in the skipped
        // week) and not 1 or 3 again.
        let r3 = read(at(2026, 11, 16, 10));
        assert_eq!((r3.week_end.as_str(), ids(&r3)), ("2026-11-15", vec![2]));
        // Same week re-read comes from the stored review, unchanged.
        let again = read_in(&dir, at(2026, 11, 17, 9), |_, _| panic!("stored")).unwrap();
        assert_eq!(ids(&again), vec![2]);
        // Listed once: the following stored review does not list 1, 2 or 3 again.
        let r4 = read(at(2026, 11, 23, 10));
        assert!(r4.experiments.is_empty(), "{:?}", ids(&r4));
        let _ = std::fs::remove_dir_all(dir);

        // More than 28 days past due: dropped.
        let dir = temp_dir("wrcarry2");
        write_in(&dir, &rows, &[], &[], 0, (d("2026-09-20"), d("2026-12-08"))).unwrap();
        record_start_in(&dir, 9, Some("sleep_focus"), d("2026-10-01")).unwrap(); // due 10-08
        let now = at(2026, 11, 16, 10); // week end 11-15: 38 days past due
        let r = read_in(&dir, now, |_, _| gathered(enough(), at(2026, 11, 12, 10))).unwrap();
        assert!(r.experiments.is_empty());
        // A review that was not stored (partial) does not use up a result.
        let dir2 = temp_dir("wrcarry3");
        write_in(
            &dir2,
            &rows,
            &[],
            &[],
            0,
            (d("2026-09-20"), d("2026-12-08")),
        )
        .unwrap();
        record_start_in(&dir2, 5, Some("sleep_focus"), d("2026-11-01")).unwrap(); // due 11-08
        let now = at(2026, 11, 9, 10);
        let mut partial = enough();
        partial.partial = true;
        let p = read_in(&dir2, now, |_, _| {
            gathered(partial.clone(), at(2026, 11, 12, 10))
        })
        .unwrap();
        assert_eq!(ids(&p), vec![5]);
        assert!(!file(&dir2).exists());
        let q = read_in(&dir2, now, |_, _| gathered(enough(), at(2026, 11, 12, 10))).unwrap();
        assert_eq!(ids(&q), vec![5]);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn habit_weeks_count_real_marks_in_the_review_week() {
        use super::super::habits::HabitDayRow;
        let now = at(2026, 11, 12, 10);
        let p = patterns::compute(&Inputs::default(), now);
        let mut h = goal("h1", "habit", 3.0, 0.0);
        h.title = "Walk".into();
        let mut paused = goal("h2", "habit", 3.0, 0.0);
        paused.paused = true;
        let goals = [h, paused, goal("g", "health", 10.0, 5.0)];
        let mut r = build(
            &Inputs::default(),
            &p,
            &goals,
            no_money(),
            last_due(now),
            now,
        );
        assert!(r.goals.habit_weeks.is_empty());
        assert!(!r.goals.note.contains("not computed"));
        let row = |id: &str, day: &str, done: bool| HabitDayRow {
            habit_id: id.into(),
            day: day.into(),
            done,
            source: "manual".into(),
            at: String::new(),
        };
        let (s, e) = (r.week_start.clone(), r.week_end.clone());
        let before = (chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").unwrap()
            - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
        let rows = [
            row("h1", &s, true),
            row("h1", &e, true),
            row("h1", &before, true),
            row("h1", &s, true),
            row("h2", &s, true),
        ];
        // a duplicate day row can't happen in the store, but only done rows in range count
        overlay_habits(&mut r, &goals, Ok(rows[..3].to_vec()));
        assert_eq!(
            r.goals.habit_weeks,
            vec![HabitWeek {
                id: "h1".into(),
                title: "Walk".into(),
                done: 2,
                target: 3
            }]
        );
        overlay_habits(&mut r, &goals, Ok(vec![row("h1", &e, false)]));
        assert_eq!(r.goals.habit_weeks[0].done, 0);
        overlay_habits(&mut r, &goals, Err("x".into()));
        assert!(r.goals.habit_weeks.is_empty());
        assert!(r.goals.note.contains("couldn't be read"));
    }
}
