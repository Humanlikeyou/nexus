//! Life domain repository (Phase 6 data layer).
//!
//! One source of truth per record, reusing what exists:
//! - goals and habits: `goals.json` (habits are `category: "habit"`), with the Life fields in each
//!   goal's `life` object (`goals::LifeFields`);
//! - habit completions: `habits/days.json` (`habits.rs`);
//! - projects, their milestones and next action: `terrence_projects` in `terrence_core.db`;
//! - goal milestones: the roadmap steps in `goals/roadmaps.json`;
//! - tasks: `TASKS.md` (History/Undo `task`); dates and deadlines: `life/dates.json` (its own
//!   `goal_id` link);
//! - new here: life areas (`life/areas.json`) and goal-to-project and goal-to-habit links
//!   (`life/links.json`), each relationship stored once.
//!
//! Every change goes through the owning store's write path and lands in the activity history.
//! No percentage is computed without a baseline, and a day without a habit record is
//! `no_record`, never "not done" and never zero.

use super::goals::{save_goals_at, status_of, Goal, LifeFields};
use super::local_store::{read_json, write_json_atomic};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub(crate) const GOAL_TYPES: [&str; 3] = ["outcome", "performance", "learning"];
pub(crate) const STATUSES: [&str; 5] = ["active", "paused", "blocked", "completed", "archived"];

fn now() -> String {
    super::shared::time::now_jst().to_rfc3339()
}

fn life_dir(data: &Path) -> PathBuf {
    data.join("life")
}

fn goals_path(data: &Path) -> PathBuf {
    data.join("goals.json")
}

fn read_goals(data: &Path) -> Result<Vec<Goal>, String> {
    read_json(&goals_path(data), MAX_BYTES)
}

fn hhmm(t: &str) -> bool {
    super::goals::valid_hhmm(t).is_some()
}

/// Checks the Life fields. Invalid values are refused, never silently dropped.
pub(crate) fn validate(f: &LifeFields, is_habit: bool) -> Result<(), String> {
    if let Some(t) = &f.goal_type {
        if !GOAL_TYPES.contains(&t.as_str()) {
            return Err("goal type must be outcome, performance or learning".into());
        }
    }
    if let Some(s) = &f.status {
        if !STATUSES.contains(&s.as_str()) {
            return Err("status must be active, paused, blocked, completed or archived".into());
        }
        if s == "blocked"
            && f.status_reason
                .as_deref()
                .is_none_or(|r| r.trim().is_empty())
        {
            return Err("a blocked goal needs what blocks it".into());
        }
    }
    if let Some(fr) = &f.frequency {
        if !is_habit {
            return Err("only a habit has a frequency".into());
        }
        if fr.times == 0 || fr.times > 50 || !matches!(fr.per.as_str(), "day" | "week") {
            return Err("frequency is 1-50 times per day or week".into());
        }
    }
    if f.preferred_time.as_deref().is_some_and(|t| !hhmm(t)) {
        return Err("preferred time must be HH:MM".into());
    }
    if f.baseline.is_some_and(|b| !b.is_finite()) {
        return Err("baseline must be a number".into());
    }
    for t in [
        &f.motivation,
        &f.desired_result,
        &f.measurement,
        &f.completion_criteria,
        &f.next_action,
        &f.cue,
        &f.minimum_version,
        &f.full_version,
        &f.status_reason,
    ]
    .into_iter()
    .flatten()
    {
        if t.chars().count() > 500 {
            return Err("a Life field is longer than 500 characters".into());
        }
    }
    Ok(())
}

/// Sets a goal's or habit's Life fields (all of them: `None` clears one). Ids, titles and values
/// are untouched. Goes through `save_goals_at` (locked, atomic, read back, activity recorded).
pub(crate) fn update_in(data: &Path, goal_id: &str, fields: LifeFields) -> Result<Goal, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut goals = read_goals(data)?;
    let g = goals
        .iter_mut()
        .find(|g| g.id == goal_id)
        .ok_or("goal not found")?;
    validate(&fields, g.category == "habit")?;
    if let Some(a) = &fields.area_id {
        if !areas_in(data)?.iter().any(|x| &x.id == a) {
            return Err("unknown life area".into());
        }
    }
    let mut fields = fields;
    // An all-empty `life` would read as "keep what's on disk" on the next save, so clearing
    // everything stores the current status explicitly instead.
    if fields.is_empty() {
        fields.status = Some(status_of(g).to_string());
    }
    // Keep the old flag in step for code that reads it.
    if let Some(s) = &fields.status {
        g.paused = s == "paused";
    }
    g.life = fields;
    g.updated_at = String::new();
    save_goals_at(&goals_path(data), goals)?;
    read_goals(data)?
        .into_iter()
        .find(|g| g.id == goal_id)
        .ok_or_else(|| "goal vanished after save".to_string())
}

// ─── Areas ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeArea {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `active` | `archived`
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Areas {
    #[serde(default)]
    areas: Vec<LifeArea>,
}

pub(crate) fn areas_in(data: &Path) -> Result<Vec<LifeArea>, String> {
    Ok(read_json::<Areas>(&life_dir(data).join("areas.json"), MAX_BYTES)?.areas)
}

/// Creates (no id) or renames (id) an area.
pub(crate) fn area_upsert_in(
    data: &Path,
    id: Option<&str>,
    name: &str,
    description: Option<&str>,
) -> Result<LifeArea, String> {
    let name = super::local_store::clean_text(name, 1, 80)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = life_dir(data).join("areas.json");
    let mut store: Areas = read_json(&path, MAX_BYTES)?;
    let t = now();
    let area = match id {
        Some(id) => {
            let a = store
                .areas
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or("area not found")?;
            a.name = name;
            a.description = description.map(str::to_string);
            a.updated_at = t;
            a.clone()
        }
        None => {
            if store
                .areas
                .iter()
                .any(|a| a.name.eq_ignore_ascii_case(&name) && a.status == "active")
            {
                return Err("an area with that name exists".into());
            }
            let a = LifeArea {
                id: super::local_store::new_id("area"),
                name,
                description: description.map(str::to_string),
                status: "active".into(),
                created_at: t.clone(),
                updated_at: t,
            };
            store.areas.push(a.clone());
            a
        }
    };
    write_json_atomic(&path, &store, MAX_BYTES)?;
    note(data, "area.updated", &format!("area:{}", area.id), None);
    Ok(area)
}

/// Archives an area; its goals keep their `area_id` (nothing is deleted).
pub(crate) fn area_archive_in(data: &Path, id: &str) -> Result<LifeArea, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = life_dir(data).join("areas.json");
    let mut store: Areas = read_json(&path, MAX_BYTES)?;
    let a = store
        .areas
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or("area not found")?;
    a.status = "archived".into();
    a.updated_at = now();
    let out = a.clone();
    write_json_atomic(&path, &store, MAX_BYTES)?;
    note(
        data,
        "area.updated",
        &format!("area:{id}"),
        Some(r#"{"status":"archived"}"#),
    );
    Ok(out)
}

// ─── Links ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeLink {
    pub id: String,
    /// `goal_project` | `goal_habit`
    pub kind: String,
    pub goal_id: String,
    /// `project:<id>` or `goals:<habit id>`.
    pub other_ref: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Links {
    #[serde(default)]
    links: Vec<LifeLink>,
}

fn project_exists(data: &Path, id: &str) -> bool {
    let db = data.join("terrence_core.db");
    if !db.is_file() {
        return false;
    }
    rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM terrence_projects WHERE id = ?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
        })
        .is_ok_and(|n| n > 0)
}

/// Links a goal to a project or a habit. Both ends must exist; the same link twice is one link.
pub(crate) fn link_add_in(
    data: &Path,
    kind: &str,
    goal_id: &str,
    other_id: &str,
) -> Result<LifeLink, String> {
    let goals = read_goals(data)?;
    let goal = goals
        .iter()
        .find(|g| g.id == goal_id)
        .ok_or("goal not found")?;
    if goal.category == "habit" {
        return Err("link from a goal, not from a habit".into());
    }
    let other_ref = match kind {
        "goal_habit" => {
            let h = goals
                .iter()
                .find(|g| g.id == other_id)
                .ok_or("habit not found")?;
            if h.category != "habit" {
                return Err("that is not a habit".into());
            }
            format!("goals:{other_id}")
        }
        "goal_project" => {
            if !project_exists(data, other_id) {
                return Err("project not found".into());
            }
            format!("project:{other_id}")
        }
        _ => return Err("link kind must be goal_project or goal_habit".into()),
    };
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = life_dir(data).join("links.json");
    let mut store: Links = read_json(&path, MAX_BYTES)?;
    if let Some(l) = store
        .links
        .iter()
        .find(|l| l.kind == kind && l.goal_id == goal_id && l.other_ref == other_ref)
    {
        return Ok(l.clone());
    }
    let link = LifeLink {
        id: super::local_store::new_id("lnk"),
        kind: kind.into(),
        goal_id: goal_id.into(),
        other_ref: other_ref.clone(),
        created_at: now(),
    };
    store.links.push(link.clone());
    write_json_atomic(&path, &store, MAX_BYTES)?;
    note(
        data,
        "link.added",
        &format!("goals:{goal_id}"),
        Some(&format!(r#"{{"to":"{other_ref}"}}"#)),
    );
    Ok(link)
}

pub(crate) fn link_remove_in(data: &Path, link_id: &str) -> Result<bool, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = life_dir(data).join("links.json");
    let mut store: Links = read_json(&path, MAX_BYTES)?;
    let Some(i) = store.links.iter().position(|l| l.id == link_id) else {
        return Ok(false);
    };
    let l = store.links.remove(i);
    write_json_atomic(&path, &store, MAX_BYTES)?;
    note(
        data,
        "link.removed",
        &format!("goals:{}", l.goal_id),
        Some(&format!(r#"{{"from":"{}"}}"#, l.other_ref)),
    );
    Ok(true)
}

/// Links touching a goal or habit (either end).
pub(crate) fn links_for_in(data: &Path, goal_id: &str) -> Result<Vec<LifeLink>, String> {
    let as_habit = format!("goals:{goal_id}");
    Ok(
        read_json::<Links>(&life_dir(data).join("links.json"), MAX_BYTES)?
            .links
            .into_iter()
            .filter(|l| l.goal_id == goal_id || l.other_ref == as_habit)
            .collect(),
    )
}

// ─── Progress and history (read-only, never invented) ────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeProgress {
    pub goal_id: String,
    pub status: String,
    pub current: f64,
    pub target: f64,
    pub unit: String,
    pub baseline: Option<f64>,
    /// Only when a baseline is set and differs from the target: (current - baseline) / (target - baseline).
    /// Not clamped, so overshooting or going backwards shows as it is.
    pub fraction: Option<f64>,
    /// Recorded progress changes, oldest first (from the activity history).
    pub history: Vec<super::amara_store::commands::ActivityRow>,
}

pub(crate) fn progress_in(data: &Path, goal_id: &str) -> Result<LifeProgress, String> {
    let g = read_goals(data)?
        .into_iter()
        .find(|g| g.id == goal_id)
        .ok_or("goal not found")?;
    let fraction = g
        .life
        .baseline
        .filter(|b| (g.target - b).abs() > f64::EPSILON)
        .map(|b| (g.current - b) / (g.target - b));
    let store = super::amara_store::store_for(data, super::amara_store::Scope::Personal);
    let mut history: Vec<_> = store
        .list_activity(
            super::amara_store::Scope::Personal,
            Some(&format!("goals:{goal_id}")),
            None,
            1000,
        )?
        .into_iter()
        .filter(|a| {
            matches!(
                a.kind.as_str(),
                "goal.progressed" | "goal.completed" | "goal.status_changed"
            )
        })
        .collect();
    history.reverse();
    Ok(LifeProgress {
        goal_id: g.id.clone(),
        status: status_of(&g).to_string(),
        current: g.current,
        target: g.target,
        unit: g.unit.clone(),
        baseline: g.life.baseline,
        fraction,
        history,
    })
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HabitDay {
    pub day: String,
    /// `done` | `not_done` (marked) | `no_record` (nothing was marked: unknown, not zero)
    pub state: String,
}

/// Every day from `from` to `to` (inclusive, at most 400), with what was actually recorded.
pub(crate) fn habit_history_in(
    data: &Path,
    habit_id: &str,
    from: &str,
    to: &str,
) -> Result<Vec<HabitDay>, String> {
    let parse = |d: &str| {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| "dates are YYYY-MM-DD".to_string())
    };
    let (a, b) = (parse(from)?, parse(to)?);
    if b < a || (b - a).num_days() > 400 {
        return Err("range is 0-400 days".into());
    }
    let rows = super::habits::rows_in(&super::habits::dir_in(data))?;
    Ok(a.iter_days()
        .take_while(|d| *d <= b)
        .map(|d| {
            let day = d.format("%Y-%m-%d").to_string();
            let state = match rows.iter().find(|r| r.habit_id == habit_id && r.day == day) {
                Some(r) if r.done => "done",
                Some(_) => "not_done",
                None => "no_record",
            };
            HabitDay {
                day,
                state: state.into(),
            }
        })
        .collect())
}

fn note(data: &Path, kind: &str, subject: &str, data_json: Option<&str>) {
    super::amara_store::note(
        data,
        super::amara_store::NewActivity {
            certainty: "confirmed",
            scope: super::amara_store::Scope::Personal,
            kind,
            subject_ref: subject,
            occurred_at: &now(),
            source: "user",
            source_ref: None,
            actor: "taisun",
            summary: None,
            data_json,
        },
    );
}

#[cfg(test)]
mod tests;

// ─── Tauri commands (the Life interface's backend; decision 0055) ─────────────

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub fn life_update(goal_id: String, fields: LifeFields) -> Result<Goal, String> {
    update_in(&data(), &goal_id, fields)
}

#[tauri::command]
#[specta::specta]
pub fn life_progress(goal_id: String) -> Result<LifeProgress, String> {
    progress_in(&data(), &goal_id)
}

#[tauri::command]
#[specta::specta]
pub fn life_habit_history(
    habit_id: String,
    from: String,
    to: String,
) -> Result<Vec<HabitDay>, String> {
    habit_history_in(&data(), &habit_id, &from, &to)
}

#[tauri::command]
#[specta::specta]
pub fn life_areas() -> Result<Vec<LifeArea>, String> {
    areas_in(&data())
}

#[tauri::command]
#[specta::specta]
pub fn life_area_upsert(
    id: Option<String>,
    name: String,
    description: Option<String>,
) -> Result<LifeArea, String> {
    area_upsert_in(&data(), id.as_deref(), &name, description.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn life_area_archive(id: String) -> Result<LifeArea, String> {
    area_archive_in(&data(), &id)
}

#[tauri::command]
#[specta::specta]
pub fn life_link_add(kind: String, goal_id: String, other_id: String) -> Result<LifeLink, String> {
    link_add_in(&data(), &kind, &goal_id, &other_id)
}

#[tauri::command]
#[specta::specta]
pub fn life_link_remove(link_id: String) -> Result<bool, String> {
    link_remove_in(&data(), &link_id)
}

#[tauri::command]
#[specta::specta]
pub fn life_links(goal_id: String) -> Result<Vec<LifeLink>, String> {
    links_for_in(&data(), &goal_id)
}
