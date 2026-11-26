//! What the personal briefing, summaries and rules read (decision 0066). Every source is read through
//! its owner's own reader, read-only. A source that can't be read is named in `missing` and its part
//! of the briefing says so; nothing is guessed or shown as zero.

use super::super::checkin::CheckInRecord;
use super::super::daily_table::DailyRow;
use super::super::goal_roadmap::Roadmap;
use super::super::goals::Goal;
use super::super::habits::HabitDayRow;
use super::super::life_dates::LifeDateRecord;
use super::super::life_plan::WeekPlan;
use super::super::local_store::read_json;
use super::super::neuro_training::TrainingRecord;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use std::collections::BTreeMap;
use std::path::Path;

/// Days of history the rules and summaries look at.
pub(crate) const HISTORY_DAYS: i64 = 56;

#[derive(Debug, Clone, Default)]
pub(crate) struct Workout {
    pub id: String,
    pub day: NaiveDate,
    pub done_sets: u32,
    pub total_reps: u32,
    pub active_min: f64,
    pub sets_by_muscle: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NeuroSession {
    pub id: String,
    pub day: NaiveDate,
    pub minutes: f64,
}

/// The personal inputs, read once per build.
#[derive(Debug, Clone, Default)]
pub(crate) struct Personal {
    pub today: NaiveDate,
    pub goals: Vec<Goal>,
    pub roadmaps: Vec<Roadmap>,
    /// `None`: the habit log couldn't be read.
    pub habit_days: Option<Vec<HabitDayRow>>,
    pub dates: Vec<LifeDateRecord>,
    /// This week's plan, when one was saved.
    pub plan: Option<WeekPlan>,
    pub checkins: Vec<CheckInRecord>,
    /// Ended, non-abandoned workout-engine sessions in the history window.
    pub workouts: Vec<Workout>,
    /// The derived daily table (NOOP sleep, HRV, recovery, strain, detected workout minutes);
    /// `None` when it isn't usable.
    pub days: Option<Vec<DailyRow>>,
    pub training: Vec<TrainingRecord>,
    pub neuro_sessions: Vec<NeuroSession>,
    /// `goal.progressed` activity: (goal id, day).
    pub progressed: Vec<(String, NaiveDate)>,
    /// The money store's own one-line summary (entries Taisun logged himself), if any.
    pub money_line: Option<String>,
    /// Sources that couldn't be read, in plain words.
    pub missing: Vec<String>,
}

pub(crate) fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub(crate) fn day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

pub(crate) fn gather(data: &Path, now: DateTime<FixedOffset>) -> Personal {
    let today = now.date_naive();
    let from = today - Duration::days(HISTORY_DAYS - 1);
    let mut p = Personal {
        today,
        ..Default::default()
    };
    match read_json::<Vec<Goal>>(&data.join("goals.json"), 4 * 1024 * 1024) {
        Ok(g) => p.goals = g,
        Err(_) => p.missing.push("goals".into()),
    }
    if let Ok(s) = super::super::goal_roadmap::read_store_at(data) {
        p.roadmaps = s.roadmaps;
    }
    p.habit_days = super::super::habits::rows_in(&super::super::habits::dir_in(data)).ok();
    if p.habit_days.is_none() {
        p.missing.push("habit days".into());
    }
    match super::super::life_dates::list_in(&super::super::life_dates::dir_in(data), today) {
        Ok(v) => p.dates = v.into_iter().map(|d| d.record).collect(),
        Err(_) => p.missing.push("Life dates".into()),
    }
    let week = monday_of(today).format("%Y-%m-%d").to_string();
    p.plan = super::super::life_plan::get_in(data, &week)
        .ok()
        .filter(|w| w.updated_at.is_some());
    match super::super::checkin::list_in(&super::super::checkin::dir_in(data), 400) {
        Ok(l) => {
            p.checkins = l
                .into_iter()
                .filter(|c| day(&c.day).is_some_and(|d| d >= from && d <= today))
                .collect()
        }
        Err(_) => p.missing.push("check-ins".into()),
    }
    let now_s = super::super::local_store::iso(&now);
    match (
        super::super::workout::sessions_in(data),
        super::super::workout::library_in(data),
    ) {
        (Ok(sessions), Ok(lib)) => {
            for s in sessions {
                if s.ended_at.is_none() || s.state == "abandoned" {
                    continue;
                }
                let Some(d) = s.started_at.as_deref().and_then(day) else {
                    continue;
                };
                if d < from || d > today {
                    continue;
                }
                let m = super::super::workout::summary(&s, &lib, &now_s);
                p.workouts.push(Workout {
                    id: s.id.clone(),
                    day: d,
                    done_sets: m.done_sets,
                    total_reps: m.total_reps,
                    active_min: m.active_s.unwrap_or(0) as f64 / 60.0,
                    sets_by_muscle: m.sets_by_muscle,
                });
            }
            p.workouts.sort_by_key(|w| w.day);
        }
        _ => p.missing.push("workout sessions".into()),
    }
    p.days = super::super::patterns::daily_rows_in(data, now, HISTORY_DAYS);
    if p.days.is_none() {
        p.missing.push("WHOOP daily table (NOOP)".into());
    }
    let sessions_dir = data.join("sessions");
    p.training = super::super::neuro_training::records_in(&sessions_dir);
    for s in super::super::patterns::read_sessions_capped(
        &sessions_dir,
        super::super::patterns::MAX_SESSION_FILES,
        super::super::patterns::MAX_SESSION_BYTES,
    ) {
        let Some(d) = day_jst(&s.start) else { continue };
        if d < from || d > today {
            continue;
        }
        p.neuro_sessions.push(NeuroSession {
            id: s.id.clone(),
            day: d,
            minutes: s.duration_secs as f64 / 60.0,
        });
    }
    p.neuro_sessions.sort_by_key(|s| s.day);
    let store =
        super::super::amara_store::store_for(data, super::super::amara_store::Scope::Personal);
    match store.list_activity(
        super::super::amara_store::Scope::Personal,
        None,
        Some(&format!("{from}T00:00:00+09:00")),
        2000,
    ) {
        Ok(rows) => {
            for a in rows.into_iter().filter(|a| a.kind == "goal.progressed") {
                if let (Some(id), Some(d)) = (
                    a.subject_ref.strip_prefix("goals:"),
                    day_jst(&a.occurred_at),
                ) {
                    p.progressed.push((id.to_string(), d));
                }
            }
        }
        Err(_) => p.missing.push("activity history".into()),
    }
    p.money_line = super::super::money_entries::money_context_section_on(data, today);
    p
}

/// The JST day of an RFC 3339 time (falls back to its date part).
pub(crate) fn day_jst(s: &str) -> Option<NaiveDate> {
    match DateTime::parse_from_rfc3339(s) {
        Ok(t) => Some(
            t.with_timezone(&super::super::local_store::jst())
                .date_naive(),
        ),
        Err(_) => day(s),
    }
}
