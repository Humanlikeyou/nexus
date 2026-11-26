//! Summaries and the daily personal briefing (decision 0066). Pure: built from [`Personal`], no
//! clock and no disk. Every figure says where it came from; a part with no records says so.

use super::super::amara_store::commands::RecommendationRow;
use super::inputs::{day, monday_of, Personal};
use super::rules::{ymd, BIOMETRIC_LIMIT};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefingLine {
    pub text: String,
    /// measured | calculated | self_report | unavailable
    pub kind: String,
    /// Record references behind the line.
    pub sources: Vec<String>,
}

fn line(text: impl Into<String>, kind: &str, sources: Vec<String>) -> BriefingLine {
    BriefingLine {
        text: text.into(),
        kind: kind.into(),
        sources,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefingSection {
    pub id: String,
    pub title: String,
    /// data | none | unavailable
    pub state: String,
    pub lines: Vec<BriefingLine>,
    pub note: Option<String>,
}

fn section(id: &str, title: &str, lines: Vec<BriefingLine>, empty: &str) -> BriefingSection {
    BriefingSection {
        id: id.into(),
        title: title.into(),
        state: if lines.is_empty() { "none" } else { "data" }.into(),
        note: lines.is_empty().then(|| empty.to_string()),
        lines,
    }
}

fn unavailable(id: &str, title: &str, why: &str) -> BriefingSection {
    BriefingSection {
        id: id.into(),
        title: title.into(),
        state: "unavailable".into(),
        lines: vec![],
        note: Some(why.into()),
    }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

// ───────────────────────────── goal trajectory ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoalTrajectory {
    pub goal_id: String,
    pub title: String,
    /// How progress is measured: "3 of 8 steps" or "2 of 5 offers".
    pub measure: String,
    pub progress_pct: Option<f64>,
    /// Share of the time from the goal's start to its deadline that has passed.
    pub expected_pct: Option<f64>,
    pub deadline: Option<String>,
    pub days_left: Option<i64>,
    /// Progress recorded in the last 14 days (`goal.progressed` activity).
    pub moves_14d: u32,
    /// ahead | on_pace | behind | no_deadline | not_started | done | unknown
    pub state: String,
    pub next_step: Option<String>,
    pub sources: Vec<String>,
}

pub(crate) fn trajectories(p: &Personal) -> Vec<GoalTrajectory> {
    let mut out = Vec::new();
    for g in p.goals.iter().filter(|g| {
        g.category != "habit"
            && super::super::goals::status_of(g) == "active"
            && !super::super::weekly_review::is_business_goal(g)
    }) {
        let rm = p
            .roadmaps
            .iter()
            .find(|r| r.goal_id == g.id)
            .filter(|r| r.steps.iter().any(|s| !s.deferred));
        let (pct, measure) = match rm {
            Some(r) => {
                let live: Vec<_> = r.steps.iter().filter(|s| !s.deferred).collect();
                let done = live.iter().filter(|s| s.done).count();
                (
                    Some(100.0 * done as f64 / live.len() as f64),
                    format!("{done} of {} steps", live.len()),
                )
            }
            None if g.target > 0.0 => (
                Some((100.0 * g.current / g.target).clamp(0.0, 100.0)),
                format!("{} of {} {}", g.current, g.target, g.unit),
            ),
            None => (None, "no target recorded".into()),
        };
        let next_step = rm
            .and_then(|r| {
                r.steps
                    .iter()
                    .filter(|s| !s.done && !s.deferred)
                    .min_by_key(|s| s.order)
            })
            .map(|s| s.title.clone())
            .or_else(|| g.life.next_action.clone());
        let deadline = g.deadline.as_deref().and_then(day);
        let start = day(&g.created_at);
        let days_left = deadline.map(|d| (d - p.today).num_days());
        let expected = match (start, deadline) {
            (Some(s), Some(d)) if d > s => Some(
                (100.0 * (p.today - s).num_days() as f64 / (d - s).num_days() as f64)
                    .clamp(0.0, 100.0),
            ),
            _ => None,
        };
        let since = p.today - Duration::days(13);
        let moves = p
            .progressed
            .iter()
            .filter(|(id, d)| id == &g.id && *d >= since)
            .count() as u32;
        let state = match (pct, expected, deadline) {
            (Some(x), _, _) if x >= 100.0 => "done",
            (None, _, _) => "unknown",
            (_, _, None) => "no_deadline",
            (Some(_), None, Some(_)) => "unknown",
            (Some(x), Some(e), Some(_)) if x == 0.0 && e < 10.0 => "not_started",
            (Some(x), Some(e), Some(_)) if x >= e + 10.0 => "ahead",
            (Some(x), Some(e), Some(_)) if x + 10.0 >= e => "on_pace",
            _ => "behind",
        };
        let mut sources = vec![format!("goals:{}", g.id)];
        if rm.is_some() {
            sources.push(format!("roadmap:{}", g.id));
        }
        out.push(GoalTrajectory {
            goal_id: g.id.clone(),
            title: g.title.clone(),
            measure,
            progress_pct: pct.map(r1),
            expected_pct: expected.map(r1),
            deadline: g.deadline.clone(),
            days_left,
            moves_14d: moves,
            state: state.into(),
            next_step,
            sources,
        });
    }
    out
}

// ───────────────────────────── training ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingSummary {
    pub period_start: String,
    pub period_end: String,
    pub sessions: u32,
    pub done_sets: u32,
    pub total_reps: u32,
    pub active_min: f64,
    /// Average weekly sets over the 3 earlier weeks that had logged sessions; `None` below 2 such weeks.
    pub baseline_sets_per_week: Option<f64>,
    pub sets_by_muscle: BTreeMap<String, u32>,
    /// WHOOP (via NOOP) detected workout minutes and mean strain over the same days; `None` when the
    /// daily table isn't usable or has no value.
    pub detected_workout_min: Option<f64>,
    pub mean_strain: Option<f64>,
    pub source_ids: Vec<String>,
    pub note: String,
}

pub(crate) fn training(p: &Personal, end: NaiveDate) -> TrainingSummary {
    let w = (end - Duration::days(6), end);
    let inw = |d: NaiveDate| d >= w.0 && d <= w.1;
    let mut s = TrainingSummary {
        period_start: ymd(w.0),
        period_end: ymd(w.1),
        sessions: 0,
        done_sets: 0,
        total_reps: 0,
        active_min: 0.0,
        baseline_sets_per_week: None,
        sets_by_muscle: BTreeMap::new(),
        detected_workout_min: None,
        mean_strain: None,
        source_ids: vec![],
        note: format!("Sessions are the ones you logged in the workout engine. WHOOP-detected workouts are a separate measurement. {BIOMETRIC_LIMIT}"),
    };
    for x in p.workouts.iter().filter(|x| inw(x.day)) {
        s.sessions += 1;
        s.done_sets += x.done_sets;
        s.total_reps += x.total_reps;
        s.active_min = r1(s.active_min + x.active_min);
        for (m, n) in &x.sets_by_muscle {
            *s.sets_by_muscle.entry(m.clone()).or_default() += n;
        }
        s.source_ids.push(format!("workout:{}", x.id));
    }
    let weeks: Vec<f64> = (1..=3)
        .filter_map(|k| {
            let we = (w.0 - Duration::days(7 * k), w.1 - Duration::days(7 * k));
            let v: Vec<_> = p
                .workouts
                .iter()
                .filter(|x| x.day >= we.0 && x.day <= we.1)
                .collect();
            (!v.is_empty()).then(|| v.iter().map(|x| x.done_sets as f64).sum())
        })
        .collect();
    if weeks.len() >= 2 {
        s.baseline_sets_per_week = Some(r1(weeks.iter().sum::<f64>() / weeks.len() as f64));
    }
    if let Some(days) = &p.days {
        let rows: Vec<_> = days
            .iter()
            .filter(|d| day(&d.day).is_some_and(inw))
            .collect();
        let mins: Vec<f64> = rows.iter().filter_map(|d| d.workout_min).collect();
        let strain: Vec<f64> = rows.iter().filter_map(|d| d.strain).collect();
        if !mins.is_empty() {
            s.detected_workout_min = Some(mins.iter().sum::<f64>().round());
        }
        if !strain.is_empty() {
            s.mean_strain = Some(r1(strain.iter().sum::<f64>() / strain.len() as f64));
        }
        for d in rows
            .iter()
            .filter(|d| d.workout_min.is_some() || d.strain.is_some())
        {
            s.source_ids.push(format!("daily:{}", d.day));
        }
    }
    s
}

// ───────────────────────────── neurotechnology ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NeuroSummary {
    pub period_start: String,
    pub period_end: String,
    pub sessions: u32,
    pub minutes: f64,
    /// Training records in the period that met the eligibility rules (0059).
    pub eligible_training: u32,
    pub median_time_in_target_pct: Option<f64>,
    pub median_valid_pct: Option<f64>,
    /// Longitudinal trend statements from `neuro_training::trends_from` (with confidence), all sessions.
    pub trends: Vec<String>,
    pub source_ids: Vec<String>,
    pub note: String,
}

pub(crate) fn neuro(p: &Personal, end: NaiveDate, days: i64) -> NeuroSummary {
    let w = (end - Duration::days(days - 1), end);
    let inw = |d: NaiveDate| d >= w.0 && d <= w.1;
    let sessions: Vec<_> = p.neuro_sessions.iter().filter(|s| inw(s.day)).collect();
    let records: Vec<_> = p
        .training
        .iter()
        .filter(|r| super::inputs::day_jst(&r.start).is_some_and(inw))
        .collect();
    let eligible: Vec<_> = records.iter().filter(|r| r.eligible).collect();
    let trends = super::super::neuro_training::trends_from(&p.training)
        .protocols
        .iter()
        .flat_map(|pt| {
            pt.features
                .iter()
                .filter(|f| f.confidence != "none")
                .map(move |f| {
                    format!(
                        "{}: {} is {} over {} sessions (confidence {}).",
                        pt.protocol_id,
                        f.feature.replace('_', " "),
                        match f.direction.as_str() {
                            "higher" => "higher in later sessions",
                            "lower" => "lower in later sessions",
                            _ => "without a clear change",
                        },
                        f.n,
                        f.confidence
                    )
                })
        })
        .collect();
    let mut source_ids: Vec<String> = sessions
        .iter()
        .map(|s| format!("session:{}", s.id))
        .collect();
    source_ids.sort();
    source_ids.dedup();
    NeuroSummary {
        period_start: ymd(w.0),
        period_end: ymd(w.1),
        sessions: sessions.len() as u32,
        minutes: r1(sessions.iter().map(|s| s.minutes).sum()),
        eligible_training: eligible.len() as u32,
        median_time_in_target_pct: median(eligible.iter().filter_map(|r| r.time_in_target_pct).collect()).map(r1),
        median_valid_pct: median(records.iter().filter_map(|r| r.valid_pct).collect()).map(r1),
        trends,
        source_ids,
        note: "Muse EEG figures describe the recorded signal and the training protocol's targets. They are not a measure of mood, ability or mental health. Trends compare sessions with the same method only.".into(),
    }
}

// ───────────────────────────── daily briefing ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DailyBriefing {
    pub day: String,
    pub built_at: String,
    /// timer | catch_up | opened | manual
    pub trigger: String,
    pub sections: Vec<BriefingSection>,
    pub trajectories: Vec<GoalTrajectory>,
    pub training: TrainingSummary,
    pub neuro: NeuroSummary,
    /// Open cross-domain and Life recommendation ids at build time.
    pub open_recommendations: Vec<String>,
    /// Sources that couldn't be read.
    pub missing: Vec<String>,
    pub method: String,
}

pub(crate) fn daily(
    p: &Personal,
    open: &[RecommendationRow],
    built_at: &str,
    trigger: &str,
) -> DailyBriefing {
    let today = p.today;
    // Today and the week ahead.
    let mut commitments: Vec<BriefingLine> = p
        .dates
        .iter()
        .filter(|d| !d.done)
        .filter_map(|d| day(&d.date).map(|x| (x, d)))
        .filter(|(x, _)| *x >= today && *x <= today + Duration::days(6))
        .map(|(x, d)| {
            let when = match (x - today).num_days() {
                0 => "today".to_string(),
                1 => "tomorrow".to_string(),
                n => format!("in {n} days ({x})"),
            };
            line(
                format!("{} is due {when}.", d.title),
                "self_report",
                vec![format!("date:{}", d.id)],
            )
        })
        .collect();
    if let Some(plan) = &p.plan {
        if let Some(pr) = plan.priority.as_deref().filter(|s| !s.trim().is_empty()) {
            let done = plan.actions.iter().filter(|a| a.done).count();
            commitments.insert(
                0,
                line(
                    format!(
                        "This week's priority: {pr}. {done} of {} planned actions done.",
                        plan.actions.len()
                    ),
                    "self_report",
                    plan.priority_goal_id
                        .iter()
                        .map(|g| format!("goals:{g}"))
                        .collect(),
                ),
            );
        }
    }
    let mut sections = vec![section(
        "today",
        "Today and the week ahead",
        commitments,
        "No Life dates in the next 7 days and no priority set for this week.",
    )];
    sections[0].note = Some(match &sections[0].note {
        Some(n) => format!("{n} No calendar is connected, so only Life dates are listed."),
        None => "No calendar is connected, so only Life dates are listed.".into(),
    });

    // Body, from WHOOP via NOOP.
    sections.push(match &p.days {
        None => unavailable(
            "body",
            "Sleep and recovery",
            "The WHOOP daily table (NOOP) isn't usable right now, so nothing is shown.",
        ),
        Some(days) => {
            let latest = days
                .iter()
                .filter(|d| {
                    day(&d.day).is_some_and(|x| x <= today && x >= today - Duration::days(2))
                })
                .filter(|d| {
                    d.sleep_total_min.is_some() || d.recovery.is_some() || d.hrv_ms.is_some()
                })
                .max_by(|a, b| a.day.cmp(&b.day));
            let window: Vec<_> = days
                .iter()
                .filter(|d| {
                    day(&d.day).is_some_and(|x| x < today && x >= today - Duration::days(28))
                })
                .collect();
            let med = |f: fn(&super::super::daily_table::DailyRow) -> Option<f64>| {
                median(window.iter().filter_map(|d| f(d)).collect())
            };
            let mut lines = Vec::new();
            if let Some(d) = latest {
                let mut parts = Vec::new();
                if let Some(s) = d.sleep_total_min {
                    parts.push(format!(
                        "{} h sleep{}",
                        r1(s / 60.0),
                        med(|d| d.sleep_total_min)
                            .map(|m| format!(" (your 28-day median {} h)", r1(m / 60.0)))
                            .unwrap_or_default()
                    ));
                }
                if let Some(h) = d.hrv_ms {
                    parts.push(format!(
                        "HRV {} ms{}",
                        h.round(),
                        med(|d| d.hrv_ms)
                            .map(|m| format!(" (median {})", m.round()))
                            .unwrap_or_default()
                    ));
                }
                if let Some(r) = d.recovery {
                    parts.push(format!(
                        "recovery {}{}",
                        r.round(),
                        med(|d| d.recovery)
                            .map(|m| format!(" (median {})", m.round()))
                            .unwrap_or_default()
                    ));
                }
                lines.push(line(
                    format!("{}: {}.", d.day, parts.join(", ")),
                    "measured",
                    vec![format!("daily:{}", d.day)],
                ));
            }
            let mut s = section(
                "body",
                "Sleep and recovery",
                lines,
                "No WHOOP reading in the last 2 days.",
            );
            s.note = Some(match s.note {
                Some(n) => format!("{n} {BIOMETRIC_LIMIT}"),
                None => BIOMETRIC_LIMIT.into(),
            });
            s
        }
    });

    // Habits due today.
    sections.push(match &p.habit_days {
        None => unavailable("habits", "Habits today", "The habit log couldn't be read."),
        Some(rows) => {
            let lines = p
                .goals
                .iter()
                .filter(|g| {
                    g.category == "habit"
                        && !g.paused
                        && super::super::goals::status_of(g) == "active"
                })
                .map(|g| {
                    let t = ymd(today);
                    let mark = rows.iter().find(|r| r.habit_id == g.id && r.day == t);
                    let text = match mark {
                        Some(r) if r.done => format!("{}: done today.", g.title),
                        Some(_) => format!("{}: marked not done today.", g.title),
                        None => format!("{}: not marked yet today.", g.title),
                    };
                    line(text, "self_report", vec![format!("goals:{}", g.id)])
                })
                .collect();
            section("habits", "Habits today", lines, "No active habits.")
        }
    });

    // Goals.
    let traj = trajectories(p);
    let goal_lines: Vec<BriefingLine> = traj
        .iter()
        .filter(|t| {
            matches!(
                t.state.as_str(),
                "behind" | "ahead" | "on_pace" | "not_started"
            )
        })
        .map(|t| {
            let state = match t.state.as_str() {
                "behind" => "behind the pace its deadline needs",
                "ahead" => "ahead of pace",
                "on_pace" => "on pace",
                _ => "not started",
            };
            line(
                format!(
                    "{}: {} ({}), {state}{}.",
                    t.title,
                    t.progress_pct
                        .map(|x| format!("{}%", x.round()))
                        .unwrap_or_default(),
                    t.measure,
                    t.days_left
                        .map(|d| format!(", {d} days left"))
                        .unwrap_or_default()
                ),
                "calculated",
                t.sources.clone(),
            )
        })
        .collect();
    sections.push(section(
        "goals",
        "Goals",
        goal_lines,
        "No active goal has both a target and a deadline.",
    ));

    // Training and neuro (last 7 days).
    let tr = training(p, today);
    let mut tl = Vec::new();
    if tr.sessions > 0 {
        tl.push(line(
            format!(
                "{} session(s), {} sets in the last 7 days{}.",
                tr.sessions,
                tr.done_sets,
                tr.baseline_sets_per_week
                    .map(|b| format!(" (recent average {b} sets a week)"))
                    .unwrap_or_default()
            ),
            "calculated",
            tr.source_ids
                .iter()
                .filter(|s| s.starts_with("workout:"))
                .cloned()
                .collect(),
        ));
    }
    if let Some(m) = tr.detected_workout_min {
        tl.push(line(
            format!("WHOOP detected {m} workout minutes over the same days."),
            "measured",
            tr.source_ids
                .iter()
                .filter(|s| s.starts_with("daily:"))
                .cloned()
                .collect(),
        ));
    }
    sections.push(section(
        "training",
        "Training",
        tl,
        "No workouts logged or detected in the last 7 days.",
    ));
    let nr = neuro(p, today, 7);
    let mut nl = Vec::new();
    if nr.sessions > 0 {
        nl.push(line(
            format!(
                "{} Muse session(s), {} minutes in the last 7 days{}.",
                nr.sessions,
                nr.minutes.round(),
                nr.median_time_in_target_pct
                    .map(|t| format!(
                        "; median time in target {t}% over {} eligible training session(s)",
                        nr.eligible_training
                    ))
                    .unwrap_or_default()
            ),
            "calculated",
            nr.source_ids.clone(),
        ));
    }
    sections.push(section(
        "neuro",
        "Neurotechnology",
        nl,
        "No Muse sessions in the last 7 days.",
    ));

    // Money: the money store's own summary of entries Taisun logged.
    sections.push(match &p.money_line {
        Some(m) => section(
            "money",
            "Personal money",
            vec![line(m.clone(), "self_report", vec![])],
            "",
        ),
        None => unavailable(
            "money",
            "Personal money",
            "Nothing logged in the money store; no bank account is connected.",
        ),
    });

    // Suggestions.
    let rec_lines = open
        .iter()
        .take(5)
        .map(|r| {
            line(
                r.title.clone(),
                "calculated",
                vec![format!("recommendation:{}", r.recommendation_id)],
            )
        })
        .collect();
    sections.push(section(
        "suggestions",
        "Amara's open suggestions",
        rec_lines,
        "No open suggestions.",
    ));

    DailyBriefing {
        day: ymd(today),
        built_at: built_at.into(),
        trigger: trigger.into(),
        sections,
        trajectories: traj,
        training: tr,
        neuro: nr,
        open_recommendations: open.iter().map(|r| r.recommendation_id.clone()).collect(),
        missing: p.missing.clone(),
        method: "Fixed rules and arithmetic over your records. No language model was used.".into(),
    }
}

/// The week from `week_start` (a Monday): training and neuro for the weekly Life review.
pub(crate) fn week_bounds(week_start: NaiveDate) -> (NaiveDate, NaiveDate) {
    let m = monday_of(week_start);
    (m, m + Duration::days(6))
}
