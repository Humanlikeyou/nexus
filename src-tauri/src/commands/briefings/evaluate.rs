//! Measuring due evaluations (decision 0066). When a cross-domain recommendation's `measure_after`
//! date has passed, its metric is read again from the records and a `measured` outcome is appended:
//! `met`, `not_met`, or `unknown` when the records can't say. A measurement says whether the target
//! was reached, not whether the action caused it; Taisun's own "did it help" stays a separate
//! `evaluated` outcome. Each recommendation is measured once.

use super::super::amara_store::commands::RecommendationRow;
use super::super::amara_store::{store_for, Scope};
use super::inputs::{day, Personal};
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::params;
use serde_json::Value;
use std::path::Path;

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The metric's value over `[end - window + 1, end]`, or `None` when the records can't say.
pub(crate) fn personal_metric(
    data: &Path,
    p: &Personal,
    plan: &Value,
    end: NaiveDate,
) -> Option<f64> {
    let window = plan["window_days"].as_i64().unwrap_or(7).clamp(1, 56);
    let w = (end - Duration::days(window - 1), end);
    let inw = |d: NaiveDate| d >= w.0 && d <= w.1;
    // History covers the last 56 days only; older windows are unknown.
    if w.0 < p.today - Duration::days(super::inputs::HISTORY_DAYS - 1) {
        return None;
    }
    let params = &plan["params"];
    match plan["metric"].as_str()? {
        "workout.done_sets_7d" => Some(
            p.workouts
                .iter()
                .filter(|x| inw(x.day))
                .map(|x| x.done_sets as f64)
                .sum(),
        ),
        "workout.sessions_7d" => Some(p.workouts.iter().filter(|x| inw(x.day)).count() as f64),
        "sleep.avg_h_7d" => {
            let v: Vec<f64> = p
                .days
                .as_ref()?
                .iter()
                .filter(|d| day(&d.day).is_some_and(inw))
                .filter_map(|d| d.sleep_total_min.map(|m| m / 60.0))
                .collect();
            (v.len() >= 3).then(|| r1(v.iter().sum::<f64>() / v.len() as f64))
        }
        "checkin.tagged_count_7d" => Some(
            p.checkins
                .iter()
                .filter(|c| day(&c.day).is_some_and(inw) && !c.tags.is_empty())
                .count() as f64,
        ),
        "checkin.low_with_tag_28d" => {
            let tag = params["tag"].as_str()?;
            Some(
                p.checkins
                    .iter()
                    .filter(|c| day(&c.day).is_some_and(inw))
                    .filter(|c| c.energy <= 2 || c.mood <= 2)
                    .filter(|c| c.tags.iter().any(|t| t.trim().to_lowercase() == tag))
                    .count() as f64,
            )
        }
        "habit.done_on_weekday" => {
            let habit = params["habit_id"].as_str()?;
            let wd = params["weekday"].as_u64()? as u32;
            let rows = p.habit_days.as_ref()?;
            let on: Vec<_> = rows
                .iter()
                .filter(|r| r.habit_id == habit)
                .filter(|r| {
                    day(&r.day).is_some_and(|d| inw(d) && d.weekday().num_days_from_monday() == wd)
                })
                .collect();
            // No marks on those days: unknown, not zero.
            (!on.is_empty()).then(|| on.iter().filter(|r| r.done).count() as f64)
        }
        "plan.priority_actions_done" => {
            let ws = params["week_start"].as_str()?;
            let plan = super::super::life_plan::get_in(data, ws).ok()?;
            plan.updated_at.as_ref()?;
            let g = params["goal_id"].as_str().map(str::to_string);
            Some(
                plan.actions
                    .iter()
                    .filter(|a| g.is_none() || a.goal_id == g)
                    .filter(|a| a.done)
                    .count() as f64,
            )
        }
        "goal.progress_pct" => {
            // The current recorded progress: only meaningful when measured close to the date.
            if (p.today - end).num_days() > 7 {
                return None;
            }
            let g = params["goal_id"].as_str()?;
            super::summaries::trajectories(p)
                .into_iter()
                .find(|t| t.goal_id == g)?
                .progress_pct
        }
        _ => None,
    }
}

pub(crate) fn business_metric(
    b: &super::enterprise::Business,
    plan: &Value,
    end: NaiveDate,
) -> Option<f64> {
    let window = plan["window_days"].as_i64().unwrap_or(7).clamp(1, 56);
    let w = (end - Duration::days(window - 1), end);
    match plan["metric"].as_str()? {
        "business.execution_7d" => Some(
            b.activity
                .iter()
                .filter(|a| super::enterprise::EXECUTION_KINDS.contains(&a.kind.as_str()))
                .filter(|a| {
                    super::inputs::day_jst(&a.occurred_at).is_some_and(|d| d >= w.0 && d <= w.1)
                })
                .count() as f64,
        ),
        // Search data is deferred (0064): unknown until a search source exists.
        _ => None,
    }
}

/// `met` | `not_met` against the plan's target and direction.
pub(crate) fn verdict(plan: &Value, value: f64) -> &'static str {
    let target = plan["target"].as_f64().unwrap_or(f64::NAN);
    let met = match plan["direction"].as_str() {
        Some("at_most") => value <= target,
        _ => value >= target,
    };
    if met {
        "met"
    } else {
        "not_met"
    }
}

/// Cross-domain recommendations in `scope` whose evaluation is due and not yet measured.
pub(crate) fn due(data: &Path, scope: Scope, today: NaiveDate) -> Vec<RecommendationRow> {
    let Ok(conn) = store_for(data, scope).open() else {
        return vec![];
    };
    let ids: Vec<String> = conn
        .prepare(
            "SELECT r.recommendation_id FROM recommendation r
             WHERE r.scope = ?1 AND r.produced_by = ?2 AND r.evaluation_json IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM recommendation_outcome o
                               WHERE o.recommendation_id = r.recommendation_id AND o.outcome = 'measured')
             ORDER BY r.created_at LIMIT 200",
        )
        .and_then(|mut st| {
            st.query_map(params![scope.as_str(), super::rules::PRODUCER], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()
        })
        .unwrap_or_default();
    let store = store_for(data, scope);
    ids.iter()
        .filter_map(|id| store.get_recommendation(id).ok().flatten())
        .map(|d| d.recommendation)
        .filter(|r| {
            r.evaluation_json
                .as_deref()
                .and_then(|e| serde_json::from_str::<Value>(e).ok())
                .and_then(|v| v["measure_after"].as_str().and_then(day))
                .is_some_and(|d| d <= today)
        })
        .collect()
}

/// Measures and records. Returns how many were recorded.
pub(crate) fn measure(
    data: &Path,
    scope: Scope,
    today: NaiveDate,
    metric: impl Fn(&Value, NaiveDate) -> Option<f64>,
) -> usize {
    let store = store_for(data, scope);
    let mut n = 0;
    for r in due(data, scope, today) {
        let Some(plan) = r
            .evaluation_json
            .as_deref()
            .and_then(|e| serde_json::from_str::<Value>(e).ok())
        else {
            continue;
        };
        let Some(end) = plan["measure_after"].as_str().and_then(day) else {
            continue;
        };
        let unit = plan["unit"].as_str().unwrap_or("");
        let dir = if plan["direction"] == "at_most" {
            "at most"
        } else {
            "at least"
        };
        let target = plan["target"].as_f64().unwrap_or(0.0);
        let (eval, note) = match metric(&plan, end) {
            Some(v) => (
                verdict(&plan, v),
                format!("Measured {} {unit} for the period ending {end} (target {dir} {target}). Whether the suggestion caused it isn't measured.", r1(v)),
            ),
            None => (
                "unknown",
                format!("The records for the period ending {end} can't say (missing or too old)."),
            ),
        };
        if store
            .add_outcome(
                &r.recommendation_id,
                "measured",
                "system",
                None,
                None,
                Some(eval),
                Some(&note),
            )
            .is_ok()
        {
            n += 1;
        }
    }
    n
}
