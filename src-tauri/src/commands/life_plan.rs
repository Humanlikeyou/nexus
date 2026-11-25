//! Weekly planning for Life (decision 0056): one plan per week, keyed by its Monday (JST).
//!
//! `weekly_review.rs` reviews a week that has ended and stores no plan, so this is the only place a
//! week's primary priority and planned actions live. `<data>/life/week_plan.json` (0600, atomic).
//! Each save is recorded as `week.planned` activity (no text stored in the activity row).

use super::local_store::{clean_text, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 256 * 1024;
/// About two years of weekly plans; the oldest go first.
const MAX_WEEKS: usize = 110;
pub(crate) const MAX_ACTIONS: usize = 5;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlannedAction {
    pub text: String,
    /// The goal it moves, if any (must exist in `goals.json`).
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeekPlan {
    /// The Monday (JST) of the planned week, `YYYY-MM-DD`.
    pub week_start: String,
    /// The one thing that matters most this week, in Taisun's words. `None` = not set.
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub priority_goal_id: Option<String>,
    #[serde(default)]
    pub actions: Vec<PlannedAction>,
    #[serde(default)]
    pub note: Option<String>,
    /// When it was last saved (RFC 3339); `None` for a week never planned.
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    weeks: Vec<WeekPlan>,
}

fn file(data: &Path) -> PathBuf {
    data.join("life").join("week_plan.json")
}

/// A `YYYY-MM-DD` that is a Monday.
pub(crate) fn monday(s: &str) -> Result<NaiveDate, String> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "week must be YYYY-MM-DD")?;
    (d.weekday() == Weekday::Mon)
        .then_some(d)
        .ok_or_else(|| "a week starts on a Monday".to_string())
}

fn opt_text(v: &Option<String>, max: usize) -> Result<Option<String>, String> {
    match v.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(t) => clean_text(t, 1, max).map(Some),
    }
}

fn goal_exists(ids: &[String], id: &Option<String>) -> Result<Option<String>, String> {
    match id.as_deref().filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(g) if ids.iter().any(|x| x == g) => Ok(Some(g.to_string())),
        Some(_) => Err("that goal no longer exists".into()),
    }
}

/// Checks and normalises a plan. Invalid input is refused, never trimmed silently.
pub(crate) fn validate(p: &WeekPlan, goal_ids: &[String]) -> Result<WeekPlan, String> {
    monday(&p.week_start)?;
    if p.actions.len() > MAX_ACTIONS {
        return Err(format!("at most {MAX_ACTIONS} planned actions a week"));
    }
    let mut actions = Vec::new();
    for a in &p.actions {
        actions.push(PlannedAction {
            text: clean_text(&a.text, 1, 200).map_err(|_| "each action needs 1-200 characters")?,
            goal_id: goal_exists(goal_ids, &a.goal_id)?,
            done: a.done,
        });
    }
    Ok(WeekPlan {
        week_start: p.week_start.clone(),
        priority: opt_text(&p.priority, 200).map_err(|_| "the priority is 1-200 characters")?,
        priority_goal_id: goal_exists(goal_ids, &p.priority_goal_id)?,
        actions,
        note: opt_text(&p.note, 1000).map_err(|_| "the note is up to 1000 characters")?,
        updated_at: None,
    })
}

fn goal_ids(data: &Path) -> Result<Vec<String>, String> {
    let goals: Vec<super::goals::Goal> = read_json(&data.join("goals.json"), 1024 * 1024)?;
    Ok(goals.into_iter().map(|g| g.id).collect())
}

/// The plan for a week, or an empty one (no `updated_at`) when it was never planned.
pub(crate) fn get_in(data: &Path, week_start: &str) -> Result<WeekPlan, String> {
    monday(week_start)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(data), MAX_BYTES)?;
    Ok(store
        .weeks
        .into_iter()
        .find(|w| w.week_start == week_start)
        .unwrap_or(WeekPlan {
            week_start: week_start.into(),
            ..Default::default()
        }))
}

/// Saves (replaces) one week's plan and returns it as stored.
pub(crate) fn set_in(data: &Path, plan: &WeekPlan, now: &str) -> Result<WeekPlan, String> {
    let mut v = validate(plan, &goal_ids(data)?)?;
    v.updated_at = Some(now.to_string());
    {
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut store: Store = read_json(&file(data), MAX_BYTES)?;
        store.weeks.retain(|w| w.week_start != v.week_start);
        store.weeks.push(v.clone());
        store.weeks.sort_by(|a, b| a.week_start.cmp(&b.week_start));
        let over = store.weeks.len().saturating_sub(MAX_WEEKS);
        store.weeks.drain(..over);
        write_json_atomic(&file(data), &store, MAX_BYTES)?;
    }
    let subject = format!("review:week-{}", v.week_start);
    let summary = format!(
        "{} planned action{}{}",
        v.actions.len(),
        if v.actions.len() == 1 { "" } else { "s" },
        if v.priority.is_some() {
            ", priority set"
        } else {
            ""
        }
    );
    super::amara_store::note(
        data,
        super::amara_store::NewActivity {
            certainty: "confirmed",
            scope: super::amara_store::Scope::Personal,
            kind: "week.planned",
            subject_ref: &subject,
            occurred_at: now,
            source: "user",
            source_ref: None,
            actor: "taisun",
            summary: Some(&summary),
            data_json: None,
        },
    );
    Ok(v)
}

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

fn now() -> String {
    super::shared::time::now_jst().to_rfc3339()
}

/// This week's plan when `week_start` is omitted.
#[tauri::command]
#[specta::specta]
pub async fn life_week_plan_get(week_start: Option<String>) -> Result<WeekPlan, String> {
    blocking(JoinErr::Plain, move || {
        let w = week_start.unwrap_or_else(|| {
            super::habits::week_start(super::shared::time::now_jst().date_naive())
                .format("%Y-%m-%d")
                .to_string()
        });
        get_in(&data(), &w)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn life_week_plan_set(plan: WeekPlan) -> Result<WeekPlan, String> {
    blocking(JoinErr::Plain, move || set_in(&data(), &plan, &now())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    fn seed_goals(d: &Path) {
        std::fs::write(
            d.join("goals.json"),
            r#"[{"id":"g1","title":"Run","category":"fitness","target":300,"current":10,"unit":"km","deadline":null,"created_at":"x","updated_at":"x"}]"#,
        )
        .unwrap();
    }

    fn plan(week: &str) -> WeekPlan {
        WeekPlan {
            week_start: week.into(),
            priority: Some("  Ship the akiya offer  ".into()),
            priority_goal_id: Some("g1".into()),
            actions: vec![PlannedAction {
                text: "Call the agent".into(),
                goal_id: None,
                done: false,
            }],
            note: None,
            updated_at: None,
        }
    }

    #[test]
    fn a_week_never_planned_reads_empty_not_invented() {
        let d = temp_dir("plan-empty");
        let p = get_in(&d, "2026-11-23").unwrap();
        assert_eq!(p.priority, None);
        assert!(p.actions.is_empty());
        assert_eq!(p.updated_at, None);
    }

    #[test]
    fn save_round_trips_and_replaces_the_same_week() {
        let d = temp_dir("plan-save");
        seed_goals(&d);
        let saved = set_in(&d, &plan("2026-11-23"), "2026-11-25T10:00:00+09:00").unwrap();
        assert_eq!(saved.priority.as_deref(), Some("Ship the akiya offer"));
        assert_eq!(get_in(&d, "2026-11-23").unwrap(), saved);
        let mut p2 = plan("2026-11-23");
        p2.actions.clear();
        set_in(&d, &p2, "2026-11-25T11:00:00+09:00").unwrap();
        assert!(get_in(&d, "2026-11-23").unwrap().actions.is_empty());
        let store: Store = read_json(&file(&d), MAX_BYTES).unwrap();
        assert_eq!(store.weeks.len(), 1);
    }

    #[test]
    fn bad_input_is_refused_and_nothing_written() {
        let d = temp_dir("plan-bad");
        seed_goals(&d);
        assert!(
            set_in(&d, &plan("2026-11-24"), "t").is_err(),
            "not a Monday"
        );
        let mut p = plan("2026-11-23");
        p.priority_goal_id = Some("gone".into());
        assert!(set_in(&d, &p, "t").is_err(), "unknown goal");
        let mut p = plan("2026-11-23");
        p.actions = (0..6)
            .map(|i| PlannedAction {
                text: format!("a{i}"),
                ..Default::default()
            })
            .collect();
        assert!(set_in(&d, &p, "t").is_err(), "too many actions");
        let mut p = plan("2026-11-23");
        p.actions[0].text = "   ".into();
        assert!(set_in(&d, &p, "t").is_err(), "empty action");
        assert!(!file(&d).exists());
    }

    #[test]
    fn old_weeks_are_capped() {
        let d = temp_dir("plan-cap");
        seed_goals(&d);
        let mut day = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(); // a Monday
        for _ in 0..(MAX_WEEKS + 3) {
            let mut p = plan(&day.format("%Y-%m-%d").to_string());
            p.priority_goal_id = None;
            set_in(&d, &p, "2026-11-25T10:00:00+09:00").unwrap();
            day += chrono::Duration::days(7);
        }
        let store: Store = read_json(&file(&d), MAX_BYTES).unwrap();
        assert_eq!(store.weeks.len(), MAX_WEEKS);
        assert_eq!(store.weeks[0].week_start, "2024-01-22");
    }
}
