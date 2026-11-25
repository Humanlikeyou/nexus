//! Life repository tests: real files in a temp data folder.

use super::*;
use crate::commands::goals::Frequency;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-life-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}

/// A goals.json as written before Phase 6 (no `life`), with a venture link and a paused habit.
const OLD: &str = r#"[
 {"id":"g-run","title":"Run a half marathon","category":"fitness","target":21.1,"current":8.5,"unit":"km","deadline":"2027-04-01","created_at":"2026-01-02T09:00:00+09:00","updated_at":"2026-11-01T09:00:00+09:00","venture_id":"v1"},
 {"id":"h-med","title":"Meditate","category":"habit","target":5,"current":0,"unit":"daily","deadline":null,"created_at":"2026-02-01T09:00:00+09:00","updated_at":"2026-02-01T09:00:00+09:00","paused":true}
]"#;

fn seed(t: &Tmp) {
    std::fs::write(t.0.join("goals.json"), OLD).unwrap();
}

fn goal(t: &Tmp, id: &str) -> Goal {
    read_goals(&t.0)
        .unwrap()
        .into_iter()
        .find(|g| g.id == id)
        .unwrap()
}

fn activity_kinds(t: &Tmp) -> Vec<String> {
    crate::commands::amara_store::store_for(&t.0, crate::commands::amara_store::Scope::Personal)
        .list_activity(
            crate::commands::amara_store::Scope::Personal,
            None,
            None,
            100,
        )
        .unwrap()
        .into_iter()
        .map(|a| a.kind)
        .collect()
}

#[test]
fn existing_goals_load_unchanged_and_keep_ids_and_values() {
    let t = tmp("preserve");
    seed(&t);
    let before = goal(&t, "g-run");
    assert!(before.life.is_empty());
    assert_eq!(status_of(&goal(&t, "h-med")), "paused");
    let g = update_in(
        &t.0,
        "g-run",
        LifeFields {
            goal_type: Some("performance".into()),
            motivation: Some("Feel strong at 40".into()),
            baseline: Some(5.0),
            desired_result: Some("Finish 21.1 km".into()),
            measurement: Some("Longest run, km".into()),
            completion_criteria: Some("A timed half marathon".into()),
            next_action: Some("Book the March race".into()),
            ..Default::default()
        },
    )
    .unwrap();
    for (a, b) in [
        (&before.id, &g.id),
        (&before.title, &g.title),
        (&before.unit, &g.unit),
        (&before.created_at, &g.created_at),
    ] {
        assert_eq!(a, b);
    }
    assert_eq!((g.current, g.target), (8.5, 21.1));
    assert_eq!(g.deadline.as_deref(), Some("2027-04-01"));
    assert_eq!(g.venture_id.as_deref(), Some("v1"));
    assert_eq!(g.life.goal_type.as_deref(), Some("performance"));
    // A console save that doesn't know `life` keeps it (like venture_id).
    let mut list = read_goals(&t.0).unwrap();
    for x in list.iter_mut() {
        x.life = LifeFields::default();
    }
    save_goals_at(&goals_path(&t.0), list).unwrap();
    assert_eq!(
        goal(&t, "g-run").life.motivation.as_deref(),
        Some("Feel strong at 40")
    );
    // The untouched habit is written without a `life` key (older builds read the file as before).
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(t.0.join("goals.json")).unwrap()).unwrap();
    assert!(raw[1].get("life").is_none());
}

#[test]
fn invalid_life_values_are_refused_not_dropped() {
    let t = tmp("invalid");
    seed(&t);
    let cases = [
        LifeFields {
            goal_type: Some("dream".into()),
            ..Default::default()
        },
        LifeFields {
            status: Some("done".into()),
            ..Default::default()
        },
        LifeFields {
            status: Some("blocked".into()),
            ..Default::default()
        },
        LifeFields {
            frequency: Some(Frequency {
                times: 3,
                per: "week".into(),
            }),
            ..Default::default()
        },
        LifeFields {
            preferred_time: Some("7am".into()),
            ..Default::default()
        },
        LifeFields {
            area_id: Some("area-nope".into()),
            ..Default::default()
        },
    ];
    for f in cases {
        assert!(update_in(&t.0, "g-run", f).is_err());
    }
    assert!(goal(&t, "g-run").life.is_empty(), "nothing was written");
    assert!(update_in(&t.0, "missing", LifeFields::default()).is_err());
}

#[test]
fn paused_blocked_completed_and_archived_are_distinct_and_recorded() {
    let t = tmp("status");
    seed(&t);
    let set = |s: &str, reason: Option<&str>| {
        update_in(
            &t.0,
            "g-run",
            LifeFields {
                status: Some(s.into()),
                status_reason: reason.map(str::to_string),
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert!(set("paused", None).paused);
    let b = set("blocked", Some("Knee injury"));
    assert!(!b.paused);
    assert_eq!(status_of(&b), "blocked");
    assert_eq!(status_of(&set("completed", None)), "completed");
    assert_eq!(status_of(&set("archived", None)), "archived");
    let n = activity_kinds(&t)
        .iter()
        .filter(|k| *k == "goal.status_changed")
        .count();
    assert_eq!(n, 4);
    // Habits record status changes too, and keep their old paused flag in step.
    let h = update_in(
        &t.0,
        "h-med",
        LifeFields {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!h.paused);
}

#[test]
fn habit_fields_and_history_never_turn_missing_days_into_zero() {
    let t = tmp("habit");
    seed(&t);
    let h = update_in(
        &t.0,
        "h-med",
        LifeFields {
            frequency: Some(Frequency {
                times: 5,
                per: "week".into(),
            }),
            preferred_time: Some("07:15".into()),
            cue: Some("After coffee".into()),
            minimum_version: Some("1 minute".into()),
            full_version: Some("15 minutes".into()),
            reminders: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        h.life.frequency,
        Some(Frequency {
            times: 5,
            per: "week".into()
        })
    );
    std::fs::create_dir_all(t.0.join("habits")).unwrap();
    std::fs::write(
        t.0.join("habits/days.json"),
        r#"{"days":[{"habitId":"h-med","day":"2026-11-20","done":true,"source":"manual","at":"t"},{"habitId":"h-med","day":"2026-11-21","done":false,"source":"manual","at":"t"}]}"#,
    )
    .unwrap();
    let days = habit_history_in(&t.0, "h-med", "2026-11-20", "2026-11-22").unwrap();
    let states: Vec<&str> = days.iter().map(|d| d.state.as_str()).collect();
    assert_eq!(states, ["done", "not_done", "no_record"]);
    assert!(habit_history_in(&t.0, "h-med", "2026-11-22", "2026-11-20").is_err());
}

#[test]
fn progress_is_never_invented() {
    let t = tmp("progress");
    seed(&t);
    // No baseline: no fraction, whatever current and target say.
    let p = progress_in(&t.0, "g-run").unwrap();
    assert_eq!(p.fraction, None);
    update_in(
        &t.0,
        "g-run",
        LifeFields {
            baseline: Some(5.0),
            ..Default::default()
        },
    )
    .unwrap();
    let p = progress_in(&t.0, "g-run").unwrap();
    let f = p.fraction.unwrap();
    assert!((f - (8.5 - 5.0) / (21.1 - 5.0)).abs() < 1e-9);
    // Recorded progress appears in the history.
    let mut list = read_goals(&t.0).unwrap();
    list[0].current = 12.0;
    save_goals_at(&goals_path(&t.0), list).unwrap();
    let p = progress_in(&t.0, "g-run").unwrap();
    assert!(p.history.iter().any(|a| a.kind == "goal.progressed"));
    // Baseline equal to the target: no fraction (it would divide by zero).
    update_in(
        &t.0,
        "g-run",
        LifeFields {
            baseline: Some(21.1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(progress_in(&t.0, "g-run").unwrap().fraction, None);
}

#[test]
fn links_are_stored_once_and_both_ends_must_exist() {
    let t = tmp("links");
    std::fs::write(
        t.0.join("goals.json"),
        r#"[{"id":"g-a","title":"A","category":"life","target":1,"current":0,"unit":"x","deadline":null},
            {"id":"g-b","title":"B","category":"life","target":1,"current":0,"unit":"x","deadline":null},
            {"id":"h-1","title":"Walk","category":"habit","target":7,"current":0,"unit":"daily","deadline":null}]"#,
    )
    .unwrap();
    let c = rusqlite::Connection::open(t.0.join("terrence_core.db")).unwrap();
    c.execute_batch("CREATE TABLE terrence_projects (id TEXT PRIMARY KEY); INSERT INTO terrence_projects VALUES ('p-1');").unwrap();
    drop(c);
    // One habit supports two goals; one goal has a habit and a project.
    let l1 = link_add_in(&t.0, "goal_habit", "g-a", "h-1").unwrap();
    let again = link_add_in(&t.0, "goal_habit", "g-a", "h-1").unwrap();
    assert_eq!(l1.id, again.id, "the same link twice is one link");
    link_add_in(&t.0, "goal_habit", "g-b", "h-1").unwrap();
    link_add_in(&t.0, "goal_project", "g-a", "p-1").unwrap();
    assert_eq!(links_for_in(&t.0, "g-a").unwrap().len(), 2);
    assert_eq!(links_for_in(&t.0, "h-1").unwrap().len(), 2);
    // Nothing that doesn't exist, and no habit-to-habit or goal-as-habit links.
    assert!(link_add_in(&t.0, "goal_project", "g-a", "p-404").is_err());
    assert!(link_add_in(&t.0, "goal_habit", "g-a", "g-b").is_err());
    assert!(link_add_in(&t.0, "goal_habit", "h-1", "h-1").is_err());
    assert!(link_add_in(&t.0, "goal_habit", "g-404", "h-1").is_err());
    assert!(link_remove_in(&t.0, &l1.id).unwrap());
    assert!(!link_remove_in(&t.0, &l1.id).unwrap());
    assert_eq!(links_for_in(&t.0, "h-1").unwrap().len(), 1);
    // The goals themselves were never copied or changed by linking.
    assert_eq!(read_goals(&t.0).unwrap().len(), 3);
    let kinds = activity_kinds(&t);
    assert_eq!(kinds.iter().filter(|k| *k == "link.added").count(), 3);
    assert_eq!(kinds.iter().filter(|k| *k == "link.removed").count(), 1);
}

#[test]
fn areas_are_created_renamed_and_archived_without_touching_goals() {
    let t = tmp("areas");
    seed(&t);
    let a = area_upsert_in(&t.0, None, "Health", Some("Body and mind")).unwrap();
    assert!(
        area_upsert_in(&t.0, None, "health", None).is_err(),
        "no duplicate names"
    );
    update_in(
        &t.0,
        "g-run",
        LifeFields {
            area_id: Some(a.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let r = area_upsert_in(&t.0, Some(&a.id), "Health & fitness", None).unwrap();
    assert_eq!(r.id, a.id);
    let arch = area_archive_in(&t.0, &a.id).unwrap();
    assert_eq!(arch.status, "archived");
    assert_eq!(
        goal(&t, "g-run").life.area_id.as_deref(),
        Some(a.id.as_str())
    );
    assert_eq!(areas_in(&t.0).unwrap().len(), 1);
    assert!(
        activity_kinds(&t)
            .iter()
            .filter(|k| *k == "area.updated")
            .count()
            >= 3
    );
}
