//! 0037: link a goal to a business-ledger venture, so Ventures › tracking can list that venture's goals.
//!
//! - `goal_venture_set {goalId, ventureId|null, via?}` sets or clears the optional `venture_id` on a goal in
//!   `goals.json`, through `adds::logged_write` (History + 8 s Undo, `add_undo("goal", id)`).
//! - The venture must exist in `business/ledger.json`. No ledger, a ledger that won't load, or an unknown id is
//!   refused with a fixed code and nothing is written. Clearing needs no ledger.
//! - Undo puts back the previous link and `updated_at`, and refuses (`goal: changed since`) unless goals.json
//!   still hashes to what the write produced.
//! - `goals_by_venture` feeds `venture_tracking_list`: each goal's route status and percent come from
//!   `goal_route::route_of` (server-computed), `N of M steps` from the same steps (ghosts excluded). A goal with
//!   no ready route has `None` there, never a made-up number.
//! - Never logs titles. No network, no process.

use super::adds::{self, Dirs, UndoRef};
use super::business_ledger::read_ledger_in;
use super::content_pipeline::hash_hex;
use super::goal_roadmap as rm;
use super::goal_route::{route_of, StepState};
use super::goals::Goal;
use super::local_store::{clean_slug, iso, now_jst, read_json, write_text_atomic};
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "GoalVentureSet", rename_all = "camelCase")]
pub struct GoalVentureSet {
    /// The History id: what `add_undo("goal", id)` takes.
    pub id: String,
    pub goal_id: String,
    pub venture_id: Option<String>,
    pub toast: String,
}

/// One goal under a venture in `venture_tracking_list`.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "VentureGoal", rename_all = "camelCase")]
pub struct VentureGoal {
    pub id: String,
    pub title: String,
    /// `planning | ready` (as `goal_route_get`). A goal with no roadmap is `planning`.
    pub route_status: String,
    /// Server percent from the route; `None` until the route is ready.
    pub percent: Option<u32>,
    /// Steps done and steps on the path (ghosts excluded); `None` until the route is ready.
    pub steps_done: Option<u32>,
    pub steps_total: Option<u32>,
}

fn read_goals_in(dirs_data: &Path) -> Result<Vec<Goal>, String> {
    read_json(&dirs_data.join("goals.json"), adds::MAX_MD_BYTES)
}

pub(crate) fn set_in(
    dirs: &Dirs,
    goal_id: &str,
    venture_id: Option<&str>,
    via: Option<&str>,
    now: DateTime<FixedOffset>,
) -> Result<GoalVentureSet, String> {
    let goal_id = super::local_store::clean_text(goal_id, 1, 80)
        .map_err(|_| "goal: invalid input".to_string())?;
    let venture = match venture_id {
        None => None,
        Some(v) => Some(clean_slug(v, 64).map_err(|_| "venture: invalid input".to_string())?),
    };
    if let Some(v) = &venture {
        match read_ledger_in(&dirs.data) {
            Ok(Some(l)) if l.ventures.iter().any(|x| &x.id == v) => {}
            Ok(Some(_)) => return Err("venture: unknown".into()),
            Ok(None) => return Err("venture: ledger missing".into()),
            Err(_) => return Err("venture: ledger invalid".into()),
        }
    }
    let path = dirs.goals();
    let (id, ()) = adds::logged_write(
        "goal",
        match &venture {
            Some(_) => "Linked a goal to a venture".to_string(),
            None => "Unlinked a goal from its venture".to_string(),
        },
        via,
        dirs,
        now,
        || {
            let mut goals = read_goals_in(&dirs.data)?;
            let g = goals
                .iter_mut()
                .find(|g| g.id == goal_id)
                .ok_or("goal: not found")?;
            if g.venture_id == venture {
                return Err("goal: already set".into());
            }
            let prev = g.venture_id.take();
            let prev_updated_at = std::mem::replace(&mut g.updated_at, iso(&now));
            g.venture_id = venture.clone();
            let text = serde_json::to_string_pretty(&goals)
                .map_err(|_| "store: can't encode".to_string())?;
            write_text_atomic(&path, &text, adds::MAX_MD_BYTES)?;
            Ok((
                (),
                UndoRef::GoalVenture {
                    goal_id: goal_id.clone(),
                    prev,
                    prev_updated_at,
                    after_hash: hash_hex(&text),
                },
            ))
        },
    )?;
    Ok(GoalVentureSet {
        id,
        goal_id,
        venture_id: venture,
        toast: "Saved. Undo is open for a few seconds.".into(),
    })
}

/// Undo: only if goals.json is exactly what the write produced.
pub(crate) fn revert(
    dirs: &Dirs,
    goal_id: &str,
    prev: &Option<String>,
    prev_updated_at: &str,
    after_hash: &str,
) -> Result<(), String> {
    let path = dirs.goals();
    let text = adds::read_md(&path)?;
    if hash_hex(&text) != after_hash {
        return Err("goal: changed since".into());
    }
    let mut goals: Vec<Goal> =
        serde_json::from_str(&text).map_err(|_| "store: the file is not valid".to_string())?;
    let g = goals
        .iter_mut()
        .find(|g| g.id == goal_id)
        .ok_or("goal: not found")?;
    g.venture_id = prev.clone();
    g.updated_at = prev_updated_at.to_string();
    let next =
        serde_json::to_string_pretty(&goals).map_err(|_| "store: can't encode".to_string())?;
    write_text_atomic(&path, &next, adds::MAX_MD_BYTES)
}

/// Goals grouped by venture id, in goals.json order. An unreadable goals file or roadmap store gives no goals
/// (or no route figures) rather than an error: the tracking view still loads.
pub(crate) fn goals_by_venture(data: &Path) -> HashMap<String, Vec<VentureGoal>> {
    let mut out: HashMap<String, Vec<VentureGoal>> = HashMap::new();
    let Ok(goals) = read_goals_in(data) else {
        return out;
    };
    let store = rm::read_store_at(data).ok();
    for g in goals {
        let Some(v) = g.venture_id.clone() else {
            continue;
        };
        let route = store
            .as_ref()
            .and_then(|s| s.roadmaps.iter().find(|r| r.goal_id == g.id))
            .map(route_of);
        let entry = match route {
            Some(r) if r.status == "ready" => {
                let on_path = r
                    .steps
                    .iter()
                    .filter(|s| s.state != StepState::Deferred)
                    .count() as u32;
                let done = r
                    .steps
                    .iter()
                    .filter(|s| s.state == StepState::Done)
                    .count() as u32;
                VentureGoal {
                    id: g.id,
                    title: g.title,
                    route_status: r.status,
                    percent: r.percent,
                    steps_done: Some(done),
                    steps_total: Some(on_path),
                }
            }
            _ => VentureGoal {
                id: g.id,
                title: g.title,
                route_status: "planning".into(),
                percent: None,
                steps_done: None,
                steps_total: None,
            },
        };
        out.entry(v).or_default().push(entry);
    }
    out
}

/// Link a goal to a ledger venture, or clear the link with `ventureId: null`. History + 8 s Undo.
#[tauri::command]
#[specta::specta]
pub async fn goal_venture_set(
    goal_id: String,
    venture_id: Option<String>,
    via: Option<String>,
) -> Result<GoalVentureSet, String> {
    blocking(JoinErr::Plain, move || {
        set_in(
            &Dirs::real(),
            &goal_id,
            venture_id.as_deref(),
            via.as_deref(),
            now_jst(),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    fn dirs() -> Dirs {
        let root = temp_dir("goal-venture");
        let d = Dirs {
            data: root.join("data"),
            workspace: root.join("ws"),
            legacy: root.join("legacy"),
        };
        std::fs::create_dir_all(&d.data).unwrap();
        std::fs::create_dir_all(&d.workspace).unwrap();
        d
    }

    const GOALS: &str = r#"[
      {"id":"g-1","title":"Launch the shop","category":"business","target":1.0,"current":0.0,"unit":"x","deadline":null,"created_at":"2026-11-01","updated_at":"2026-11-02"},
      {"id":"g-2","title":"Run 5k","category":"habit","target":3.0,"current":0.0,"unit":"x","deadline":null,"created_at":"2026-11-01","updated_at":"2026-11-02"}]"#;
    const LEDGER: &str = r#"{"version":1,"fx":null,"funds":[],
        "ventures":[{"id":"taiyo-skin","name":"Taiyo Skin","stage":"","health":"on_track","progress":null,"nextStep":null,"targetJpy":null,"detail":null}]}"#;

    fn seed(d: &Dirs, ledger: bool) {
        std::fs::write(d.goals(), GOALS).unwrap();
        if ledger {
            std::fs::create_dir_all(d.data.join("business")).unwrap();
            std::fs::write(d.data.join("business").join("ledger.json"), LEDGER).unwrap();
        }
    }

    fn venture_of(d: &Dirs, id: &str) -> Option<String> {
        read_goals_in(&d.data)
            .unwrap()
            .into_iter()
            .find(|g| g.id == id)
            .unwrap()
            .venture_id
    }

    #[test]
    fn old_goal_files_without_the_field_load_and_stay_clean() {
        let d = dirs();
        seed(&d, false);
        assert_eq!(venture_of(&d, "g-1"), None);
        let out = serde_json::to_string(&read_goals_in(&d.data).unwrap()).unwrap();
        assert!(!out.contains("venture_id"), "an unset link is not written");
    }

    #[test]
    fn a_missing_ledger_or_unknown_venture_is_refused_and_writes_nothing() {
        let d = dirs();
        seed(&d, false);
        let now = now_jst();
        assert_eq!(
            set_in(&d, "g-1", Some("taiyo-skin"), None, now).unwrap_err(),
            "venture: ledger missing"
        );
        assert_eq!(std::fs::read_to_string(d.goals()).unwrap(), GOALS);
        assert!(adds::history_in("goal", &d).unwrap().is_empty());
        seed(&d, true);
        assert_eq!(
            set_in(&d, "g-1", Some("nope"), None, now).unwrap_err(),
            "venture: unknown"
        );
        assert!(set_in(&d, "g-1", Some("../x"), None, now).is_err());
        assert_eq!(
            set_in(&d, "ghost", Some("taiyo-skin"), None, now).unwrap_err(),
            "goal: not found"
        );
        assert_eq!(std::fs::read_to_string(d.goals()).unwrap(), GOALS);
        // A ledger that won't parse is refused too.
        std::fs::write(d.data.join("business").join("ledger.json"), "{ nope").unwrap();
        assert_eq!(
            set_in(&d, "g-1", Some("taiyo-skin"), None, now).unwrap_err(),
            "venture: ledger invalid"
        );
        // Clearing needs no ledger (nothing set, so it is "already set", not a ledger error).
        assert_eq!(
            set_in(&d, "g-1", None, None, now).unwrap_err(),
            "goal: already set"
        );
    }

    #[test]
    fn link_is_logged_and_undo_restores_within_8_seconds() {
        let d = dirs();
        seed(&d, true);
        let now = now_jst();
        let r = set_in(&d, "g-1", Some("taiyo-skin"), Some("chat"), now).unwrap();
        assert_eq!(r.venture_id.as_deref(), Some("taiyo-skin"));
        assert_eq!(venture_of(&d, "g-1").as_deref(), Some("taiyo-skin"));
        assert_eq!(venture_of(&d, "g-2"), None, "other goals are untouched");
        let h = adds::history_in("goal", &d).unwrap();
        assert_eq!(
            (h.len(), h[0].id.as_str(), h[0].via.as_str()),
            (1, r.id.as_str(), "chat")
        );
        assert!(adds::undo_in("goal", &r.id, &d, now + chrono::Duration::seconds(3)).unwrap());
        assert_eq!(venture_of(&d, "g-1"), None);
        let g = &read_goals_in(&d.data).unwrap()[0];
        assert_eq!(g.updated_at, "2026-11-02", "updated_at goes back too");
        assert!(adds::history_in("goal", &d).unwrap().is_empty());
    }

    #[test]
    fn undo_after_8_seconds_or_after_a_change_is_refused() {
        let d = dirs();
        seed(&d, true);
        let now = now_jst();
        let r = set_in(&d, "g-1", Some("taiyo-skin"), None, now).unwrap();
        assert!(!adds::undo_in("goal", &r.id, &d, now + chrono::Duration::seconds(9)).unwrap());
        assert_eq!(venture_of(&d, "g-1").as_deref(), Some("taiyo-skin"));
        // Clear, then edit the file by hand: Undo refuses and leaves it alone.
        let c = set_in(&d, "g-1", None, None, now).unwrap();
        let edited = format!("{} ", std::fs::read_to_string(d.goals()).unwrap());
        std::fs::write(d.goals(), &edited).unwrap();
        let e = adds::undo_in("goal", &c.id, &d, now + chrono::Duration::seconds(2)).unwrap_err();
        assert_eq!(e, "goal: changed since");
        assert_eq!(std::fs::read_to_string(d.goals()).unwrap(), edited);
    }

    #[test]
    fn tracking_lists_goals_with_server_percent_and_never_invents_one() {
        let d = dirs();
        seed(&d, true);
        let now = now_jst();
        set_in(&d, "g-1", Some("taiyo-skin"), None, now).unwrap();
        set_in(&d, "g-2", Some("taiyo-skin"), None, now).unwrap();
        // No roadmap yet: planning, no figures.
        let by = goals_by_venture(&d.data);
        let mine = &by["taiyo-skin"];
        assert_eq!(mine.len(), 2);
        assert_eq!(
            (
                mine[0].route_status.as_str(),
                mine[0].percent,
                mine[0].steps_done,
                mine[0].steps_total
            ),
            ("planning", None, None, None)
        );
        // Give g-1 a ready route (2 steps, 1 done) through the roadmap store.
        crate::commands::goal_roadmap::with_store_at(&d.data, |s| {
            s.roadmaps.push(
                serde_json::from_value(serde_json::json!({
                    "goalId": "g-1", "status": "accepted", "draftedAt": null, "pct": 0, "version": 1,
                    "steps": [
                      {"id":"s1","title":"One","why":null,"size":"medium","order":0,"done":true,"doneAt":null,"notYetCount":0,"smallerSuggestion":null,"deferred":false},
                      {"id":"s2","title":"Two","why":null,"size":"medium","order":1,"done":false,"doneAt":null,"notYetCount":0,"smallerSuggestion":null,"deferred":false},
                      {"id":"s3","title":"Ghost","why":null,"size":"medium","order":2,"done":false,"doneAt":null,"notYetCount":0,"smallerSuggestion":null,"deferred":true}
                    ]
                }))
                .unwrap(),
            );
            Ok(())
        })
        .unwrap();
        let by = goals_by_venture(&d.data);
        let g1 = &by["taiyo-skin"][0];
        let route = crate::commands::goal_route::route_get_in(&d.data, "g-1").unwrap();
        assert_eq!(g1.route_status, "ready");
        assert_eq!(
            g1.percent, route.percent,
            "the server's number, not a recount"
        );
        assert_eq!(g1.percent, Some(50));
        assert_eq!((g1.steps_done, g1.steps_total), (Some(1), Some(2)));
        assert_eq!(by["taiyo-skin"][1].percent, None);
        // The view carries them under the venture.
        let t = crate::commands::enterprise_views::tracking_in(
            &d.data,
            chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
        );
        assert_eq!(t.ventures[0].goals.len(), 2);
        assert_eq!(t.ventures[0].goals[0].percent, Some(50));
        // A goal that points at a venture that is not in the ledger shows nowhere.
        let mut goals = read_goals_in(&d.data).unwrap();
        goals[1].venture_id = Some("gone".into());
        std::fs::write(d.goals(), serde_json::to_string(&goals).unwrap()).unwrap();
        let t = crate::commands::enterprise_views::tracking_in(
            &d.data,
            chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
        );
        assert_eq!(t.ventures[0].goals.len(), 1);
    }

    #[test]
    fn save_goals_never_takes_the_link_from_the_caller() {
        let d = dirs();
        seed(&d, true);
        let now = now_jst();
        set_in(&d, "g-1", Some("taiyo-skin"), None, now).unwrap();
        let mut goals = read_goals_in(&d.data).unwrap();
        // Caller tries to move g-1's link, set one on g-2, and sneak in an unknown goal with one.
        goals[0].venture_id = Some("x".into());
        goals[1].venture_id = Some("x".into());
        goals.push(Goal {
            id: "g-3".into(),
            venture_id: Some("x".into()),
            ..goals[1].clone()
        });
        crate::commands::goals::save_goals_at(&d.goals(), goals).unwrap();
        let after = read_goals_in(&d.data).unwrap();
        assert_eq!(after[0].venture_id.as_deref(), Some("taiyo-skin"));
        assert_eq!(after[1].venture_id, None);
        assert_eq!(after[2].venture_id, None);
        // A caller that clears it doesn't clear it either; goal_venture_set(null) does.
        let mut goals = after.clone();
        goals[0].venture_id = None;
        crate::commands::goals::save_goals_at(&d.goals(), goals).unwrap();
        assert_eq!(venture_of(&d, "g-1").as_deref(), Some("taiyo-skin"));
        set_in(&d, "g-1", None, None, now).unwrap();
        assert_eq!(venture_of(&d, "g-1"), None);
        // An unreadable file means no links at all.
        std::fs::write(d.goals(), "{ nope").unwrap();
        let mut g = after;
        g[0].venture_id = Some("x".into());
        crate::commands::goals::save_goals_at(&d.goals(), g).unwrap();
        assert!(read_goals_in(&d.data)
            .unwrap()
            .iter()
            .all(|g| g.venture_id.is_none()));
    }

    #[test]
    fn it_has_no_outside_call_and_the_command_name_is_plain() {
        let src = include_str!("goal_venture.rs");
        let src = &src[..src.find("#[cfg(test)]").unwrap()];
        for needle in [
            "reqwest",
            "TcpStream",
            "Command::new",
            "http://",
            "https://",
        ] {
            assert!(!src.contains(needle), "production source contains {needle}");
        }
        let name = "goal_venture_set";
        assert!(src.contains(&format!("pub async fn {name}")));
        for w in ["post", "publish", "send", "share"] {
            assert!(!name.contains(w));
        }
    }
}
