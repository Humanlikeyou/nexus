//! Weekly review observations for Life (decision 0056): fixed rules over real records, written as
//! recommendations with evidence (`produced_by = rules:life_week`), at most 3 a week.
//!
//! Rules (no model, no invented numbers):
//! - **Stalled goal:** active, not a habit, no `goal.progressed` activity in the 14 days before the
//!   week's end, and a deadline within 90 days of it. Evidence: the goal (`self_report`).
//! - **Habit under target:** a habit with a weekly target whose recorded done-days in the week are
//!   below it, with at least one day recorded (a week with no records is unknown, never "zero").
//!   Evidence: the goal and each recorded habit day (`self_report`).
//!
//! Idempotent: each observation has a fixed id per week, kind and target, so re-reading the review
//! never duplicates one. Nothing is created for a kind and target dismissed in the last 30 days.

use super::amara_store::commands::RecommendationRow;
use super::amara_store::{store_for, NewEvidence, NewRecommendation, Scope};
use super::goals::{status_of, Goal};
use super::local_store::read_json;
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::params;
use std::path::Path;

pub(crate) const MAX_A_WEEK: usize = 3;
const STALL_DAYS: i64 = 14;
const DEADLINE_DAYS: i64 = 90;

fn day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

fn weekly_target(g: &Goal) -> Option<u32> {
    let f = g.life.frequency.as_ref()?;
    match f.per.as_str() {
        "week" => Some(f.times.min(7)),
        "day" => Some(7),
        _ => None,
    }
}

fn dismissed_recently(data: &Path, kind: &str, target: &str) -> bool {
    let Ok(conn) = store_for(data, Scope::Personal).open() else {
        return true; // can't check: create nothing
    };
    let since = (Utc::now() - Duration::days(30)).to_rfc3339();
    conn.query_row(
        "SELECT COUNT(*) FROM recommendation_outcome o JOIN recommendation r
           ON r.recommendation_id = o.recommendation_id
         WHERE o.outcome = 'dismissed' AND r.kind = ?1 AND r.target_ref = ?2 AND o.at >= ?3",
        params![kind, target, since],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(true)
}

struct Draft {
    kind: &'static str,
    goal_id: String,
    title: String,
    observation: String,
    action: String,
    evidence: Vec<String>,
}

fn drafts(data: &Path, start: NaiveDate) -> Result<Vec<Draft>, String> {
    let end = start + Duration::days(6);
    let goals: Vec<Goal> = read_json(&data.join("goals.json"), 1024 * 1024)?;
    let store = store_for(data, Scope::Personal);
    let since = format!("{}T00:00:00+09:00", end - Duration::days(STALL_DAYS - 1));
    let activity = store.list_activity(Scope::Personal, None, Some(&since), 1000)?;
    let roadmaps = super::goal_roadmap::read_store_at(data).ok();
    let mut out = Vec::new();
    for g in goals
        .iter()
        .filter(|g| g.category != "habit" && status_of(g) == "active")
    {
        let subject = format!("goals:{}", g.id);
        let moved = activity.iter().any(|a| {
            a.subject_ref == subject
                && a.kind == "goal.progressed"
                && day(&a.occurred_at).is_some_and(|d| d <= end)
        });
        let near = g
            .deadline
            .as_deref()
            .and_then(day)
            .is_some_and(|d| d >= end && (d - end).num_days() <= DEADLINE_DAYS);
        if moved || !near {
            continue;
        }
        let next_step = roadmaps.as_ref().and_then(|s| {
            s.roadmaps
                .iter()
                .find(|r| r.goal_id == g.id)
                .and_then(|r| {
                    r.steps
                        .iter()
                        .filter(|x| !x.done && !x.deferred)
                        .min_by_key(|x| x.order)
                })
                .map(|x| x.title.clone())
        });
        let action = next_step
            .or_else(|| g.life.next_action.clone())
            .unwrap_or_else(|| "Pick one small next step and do it this week".into());
        out.push(Draft {
            kind: "goal_stalled",
            goal_id: g.id.clone(),
            title: format!("“{}” hasn't moved in {STALL_DAYS} days", g.title),
            observation: format!(
                "No progress recorded since {} and the deadline is {}.",
                end - Duration::days(STALL_DAYS - 1),
                g.deadline.as_deref().unwrap_or("")
            ),
            action,
            evidence: vec![subject],
        });
    }
    let rows = super::habits::rows_in(&super::habits::dir_in(data))?;
    for g in goals
        .iter()
        .filter(|g| g.category == "habit" && !g.paused && status_of(g) == "active")
    {
        let Some(target) = weekly_target(g) else {
            continue;
        };
        let week: Vec<_> = rows
            .iter()
            .filter(|r| r.habit_id == g.id && day(&r.day).is_some_and(|d| d >= start && d <= end))
            .collect();
        if week.is_empty() {
            continue; // unknown, not zero
        }
        let done = week.iter().filter(|r| r.done).count() as u32;
        if done >= target {
            continue;
        }
        let unknown = 7 - week.len();
        let action = g
            .life
            .minimum_version
            .as_ref()
            .map(|m| format!("Try the minimum version on hard days: {m}"))
            .unwrap_or_else(|| {
                format!("Set {done} as next week's target, or pick a cue that comes before it")
            });
        let mut evidence = vec![format!("goals:{}", g.id)];
        evidence.extend(
            week.iter()
                .map(|r| format!("habit_day:{}:{}", r.habit_id, r.day)),
        );
        out.push(Draft {
            kind: "habit_below_target",
            goal_id: g.id.clone(),
            title: format!("{} was done {done} of {target} days", g.title),
            observation: format!(
                "{done} done and {} not done in the week{}.",
                week.len() as u32 - done,
                if unknown > 0 {
                    format!(", {unknown} days with no record")
                } else {
                    String::new()
                }
            ),
            action,
            evidence,
        });
    }
    Ok(out)
}

fn id_for(start: &str, kind: &str, goal: &str) -> String {
    format!("lw-{start}-{kind}-{goal}")
}

/// Creates this week's observations (up to 3, once each) and returns the week's observations.
pub(crate) fn observe_in(data: &Path, week_start: &str) -> Result<Vec<RecommendationRow>, String> {
    let start = super::life_plan::monday(week_start)?;
    let store = store_for(data, Scope::Personal);
    let prefix = format!("lw-{week_start}-");
    let existing = |store: &super::amara_store::Store| -> Result<Vec<RecommendationRow>, String> {
        Ok(store
            .list_recommendations(Scope::Personal, None, 500)?
            .into_iter()
            .filter(|r| r.recommendation_id.starts_with(&prefix))
            .collect())
    };
    let have = existing(&store)?;
    let mut room = MAX_A_WEEK.saturating_sub(have.len());
    let window_start = format!("{start}T00:00:00+09:00");
    let window_end = format!("{}T23:59:59+09:00", start + Duration::days(6));
    for d in drafts(data, start)? {
        if room == 0 {
            break;
        }
        let id = id_for(week_start, d.kind, &d.goal_id);
        let target = format!("goals:{}", d.goal_id);
        if have.iter().any(|r| r.recommendation_id == id)
            || dismissed_recently(data, d.kind, &target)
        {
            continue;
        }
        let evidence: Vec<NewEvidence> = d
            .evidence
            .iter()
            .map(|r| NewEvidence {
                record_ref: r,
                window_start: Some(&window_start),
                window_end: Some(&window_end),
                role: "observation",
                quality: "self_report",
                note: None,
            })
            .collect();
        let rec = NewRecommendation {
            id: Some(&id),
            scope: Scope::Personal,
            domain: "life",
            kind: d.kind,
            title: &d.title,
            body: &d.observation,
            expires_at: None,
            produced_by: "rules:life_week",
            model_tier: None,
            confidence: None,
            confidence_basis: None,
            target_ref: Some(&target),
            supersedes_id: None,
            observation: Some(&d.observation),
            rationale: Some(if d.kind == "goal_stalled" {
                "A goal with a near deadline and no recent progress usually needs a smaller next step."
            } else {
                "Seeing the gap against your own target is the first step to closing it."
            }),
            suggested_action: Some(&d.action),
            uncertainty: Some(
                "Only what was recorded in the app is counted; days without a record are unknown.",
            ),
            impact: None,
            effort: Some("low"),
            evaluation_json: None,
            period_start: None,
            period_end: None,
            fingerprint: None,
        };
        if store.create_recommendation(&rec, &evidence).is_ok() {
            room -= 1;
        }
    }
    existing(&store)
}

#[tauri::command]
#[specta::specta]
pub async fn life_week_observations(week_start: String) -> Result<Vec<RecommendationRow>, String> {
    super::shared::task::blocking(super::shared::task::JoinErr::Plain, move || {
        observe_in(&crate::app_paths::data_dir(), &week_start)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    fn seed(d: &Path) {
        std::fs::write(
            d.join("goals.json"),
            r#"[
 {"id":"g1","title":"Akiya offer","category":"personal","target":1,"current":0,"unit":"offer","deadline":"2026-12-20"},
 {"id":"g2","title":"Far goal","category":"personal","target":1,"current":0,"unit":"x","deadline":"2028-01-01"},
 {"id":"h1","title":"Meditate","category":"habit","target":7,"current":0,"unit":"days","deadline":null,"life":{"frequency":{"times":5,"per":"week"},"minimumVersion":"one breath"}},
 {"id":"h2","title":"Read","category":"habit","target":7,"current":0,"unit":"days","deadline":null,"life":{"frequency":{"times":3,"per":"week"}}}
]"#,
        )
        .unwrap();
        std::fs::create_dir_all(d.join("habits")).unwrap();
        std::fs::write(
            d.join("habits/days.json"),
            r#"{"days":[
 {"habitId":"h1","day":"2026-11-16","done":true,"source":"manual","at":"2026-11-16T07:00:00+09:00"},
 {"habitId":"h1","day":"2026-11-17","done":false,"source":"manual","at":"2026-11-17T07:00:00+09:00"}
]}"#,
        )
        .unwrap();
    }

    #[test]
    fn rules_find_the_stalled_goal_and_the_habit_gap_once_and_skip_unknown_weeks() {
        let d = temp_dir("lw");
        seed(&d);
        let first = observe_in(&d, "2026-11-16").unwrap();
        let mut kinds: Vec<_> = first
            .iter()
            .map(|r| (r.kind.clone(), r.target_ref.clone().unwrap()))
            .collect();
        kinds.sort();
        // g2's deadline is too far; h2 has no record this week (unknown, so nothing is said).
        assert_eq!(
            kinds,
            [
                ("goal_stalled".to_string(), "goals:g1".to_string()),
                ("habit_below_target".to_string(), "goals:h1".to_string())
            ]
        );
        let h = first
            .iter()
            .find(|r| r.kind == "habit_below_target")
            .unwrap();
        assert_eq!(h.title, "Meditate was done 1 of 5 days");
        assert!(h.body.contains("5 days with no record"));
        assert_eq!(
            h.suggested_action.as_deref(),
            Some("Try the minimum version on hard days: one breath")
        );
        assert_eq!(h.produced_by, "rules:life_week");
        let again = observe_in(&d, "2026-11-16").unwrap();
        assert_eq!(again.len(), 2, "re-reading never duplicates");
        let detail = store_for(&d, Scope::Personal)
            .get_recommendation(&h.recommendation_id)
            .unwrap()
            .unwrap();
        assert!(detail.evidence.iter().all(|e| e.quality == "self_report"));
        assert_eq!(detail.evidence.len(), 3);
    }

    #[test]
    fn a_dismissed_kind_and_target_is_not_raised_again_and_the_week_caps_at_three() {
        let d = temp_dir("lw-dismiss");
        seed(&d);
        let first = observe_in(&d, "2026-11-16").unwrap();
        let g = first.iter().find(|r| r.kind == "goal_stalled").unwrap();
        super::super::amara_store::commands::respond_in(
            &d,
            "personal",
            &g.recommendation_id,
            "dismissed",
            None,
        )
        .unwrap();
        let next = observe_in(&d, "2026-11-23").unwrap();
        assert!(
            next.iter().all(|r| r.kind != "goal_stalled"),
            "dismissed within 30 days"
        );
        assert!(observe_in(&d, "2026-11-24").is_err(), "not a Monday");
    }
}
