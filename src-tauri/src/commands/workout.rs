//! Workout engine (decision 0062, Phase 9). A training system distinct from the Fitness page, which shows
//! the workouts NOOP detects from the strap. Nothing here reads or writes NOOP.
//!
//! - **Library:** a built-in calisthenics library (`workout/library.json`, compiled in) plus your own
//!   exercises (`<data>/workout/exercises.json`).
//! - **Templates:** ordered items with sets, reps or a timed hold, optional load in kg, rest between sets
//!   and between exercises (`<data>/workout/templates.json`). Amara-recommended and user-built templates
//!   are the same type, told apart only by `source` and `source_ref` (provenance).
//! - **Sessions:** planned → active ⇄ paused → completed, partially_completed or abandoned
//!   (`<data>/workout/sessions.json`). Every transition is checked here, and an ended session never
//!   changes again. Rust stamps every time; the screen never supplies one.
//!
//! Every number is deterministic code: the duration estimate, set tracking, the completion state and
//! training volume. All three files are 0600 and written atomically under one lock. A full store refuses
//! new data rather than dropping old data.

use super::local_store::{iso, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use super::shared::time::now_jst;
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(crate) const ENGINE_VERSION: &str = "workout-engine-v1-0062";
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SESSIONS: usize = 5000;
const MAX_TEMPLATES: usize = 500;
const MAX_CUSTOM: usize = 500;
const MAX_ITEMS: usize = 30;
/// Seconds per rep when an exercise does not say (a controlled tempo).
const DEFAULT_SECS_PER_REP: f64 = 3.0;
const DEFAULT_REST_BETWEEN_S: u32 = 90;

static LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Exercise {
    pub id: String,
    pub name: String,
    /// Progression family: `push`, `pull`, `dip`, `legs`, `core`, `skill`, or `custom`.
    pub family: String,
    /// Place in the family's progression, easiest first.
    pub level: u32,
    /// `reps` or `hold`.
    pub measure: String,
    /// `beginner`, `intermediate` or `advanced`.
    pub difficulty: String,
    pub description: String,
    pub instructions: Vec<String>,
    pub muscles: Vec<String>,
    #[serde(default)]
    pub equipment: Vec<String>,
    /// Recommended sets, and reps or hold seconds.
    pub sets: u32,
    #[serde(default)]
    pub reps: Option<u32>,
    #[serde(default)]
    pub hold_s: Option<u32>,
    #[serde(default)]
    pub secs_per_rep: Option<f64>,
    #[serde(default)]
    pub form: Vec<String>,
    #[serde(default)]
    pub safety: Vec<String>,
    /// Demonstration asset reference (Phase 10 fills it in; no asset exists yet).
    #[serde(default)]
    pub demo_asset: Option<String>,
    /// True for exercises you added.
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateItem {
    pub exercise_id: String,
    pub sets: u32,
    #[serde(default)]
    pub reps: Option<u32>,
    #[serde(default)]
    pub hold_s: Option<u32>,
    /// Added load in kg (weighted calisthenics), when used.
    #[serde(default)]
    pub load_kg: Option<f64>,
    /// Rest between this exercise's sets.
    pub rest_s: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Template {
    /// Set by the store on first save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// `user` or `amara`.
    pub source: String,
    /// For `amara`: what recommended it, for example `take:pushup_ladder_1`.
    #[serde(default)]
    pub source_ref: Option<String>,
    pub items: Vec<TemplateItem>,
    /// Rest between exercises.
    #[serde(default = "default_rest_between")]
    pub rest_between_s: u32,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_rest_between() -> u32 {
    DEFAULT_REST_BETWEEN_S
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SetLog {
    pub item: u32,
    pub set: u32,
    pub exercise_id: String,
    /// `done` or `skipped`.
    pub outcome: String,
    pub reps: Option<u32>,
    pub hold_s: Option<u32>,
    pub load_kg: Option<f64>,
    pub finished_at: String,
    /// Rest taken before this set: time since the previous set finished, minus any paused time.
    pub rest_before_s: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Pause {
    pub from: String,
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub template_id: String,
    pub template_name: String,
    pub source: String,
    pub source_ref: Option<String>,
    /// The plan as it was when the session was planned (later template edits never change it).
    pub plan: Vec<TemplateItem>,
    pub rest_between_s: u32,
    /// `planned`, `active`, `paused`, `completed`, `partially_completed` or `abandoned`.
    pub state: String,
    pub planned_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub pauses: Vec<Pause>,
    pub sets: Vec<SetLog>,
    /// The next set to do: (item, set). Past the end when every set is logged or skipped.
    pub next_item: u32,
    pub next_set: u32,
    pub engine: String,
    /// The Amara take offer that launched it (its suggestion id), so finishing can mark that take done.
    #[serde(default)]
    pub offer_sid: Option<String>,
}

/// What the screen asks the engine to do with a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum Action {
    Start,
    Pause,
    Resume,
    /// Log the next set. Reps for a reps item, hold seconds for a hold item; load is optional.
    LogSet {
        #[serde(default)]
        reps: Option<u32>,
        #[serde(default, rename = "holdS")]
        hold_s: Option<u32>,
        #[serde(default, rename = "loadKg")]
        load_kg: Option<f64>,
    },
    /// Skip the next set, or the rest of the current exercise.
    Skip {
        scope: String,
    },
    /// End now. `abandon` ends it as abandoned when no set was done.
    End {
        #[serde(default)]
        abandon: bool,
    },
}

/// Deterministic totals for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub planned_sets: u32,
    pub done_sets: u32,
    pub skipped_sets: u32,
    pub completion_pct: f64,
    pub total_reps: u32,
    pub total_hold_s: u32,
    /// Sum of reps × load over weighted sets (kg).
    pub load_volume_kg: f64,
    /// Done sets per muscle group.
    pub sets_by_muscle: BTreeMap<String, u32>,
    /// Start to end (or now), minus paused time.
    pub active_s: Option<u32>,
    pub paused_s: u32,
    pub rest_s: u32,
    pub estimated_s: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: Session,
    pub summary: Summary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Library {
    #[serde(default)]
    exercises: Vec<Exercise>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Templates {
    #[serde(default)]
    templates: Vec<Template>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Sessions {
    #[serde(default)]
    sessions: Vec<Session>,
}

#[derive(Deserialize)]
struct BuiltIn {
    version: String,
    exercises: Vec<Exercise>,
}

fn built_in() -> &'static (String, Vec<Exercise>) {
    static B: OnceLock<(String, Vec<Exercise>)> = OnceLock::new();
    B.get_or_init(|| {
        let b: BuiltIn = serde_json::from_str(include_str!("workout/library.json"))
            .expect("the built-in workout library is valid JSON (tested)");
        let ex = b
            .exercises
            .into_iter()
            .map(|mut e| {
                e.demo_asset.get_or_insert_with(|| format!("demo:{}", e.id));
                e
            })
            .collect();
        (b.version, ex)
    })
}

pub(crate) fn library_version() -> &'static str {
    &built_in().0
}

// ---------------------------------------------------------------- validation

fn slug_ok(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty()
        && s.chars().count() <= max
        && !s.chars().any(|c| c.is_control() && c != '\n')
}

fn list_ok(v: &[String], max_items: usize, max_len: usize, allow_empty: bool) -> bool {
    (allow_empty || !v.is_empty()) && v.len() <= max_items && v.iter().all(|s| text_ok(s, max_len))
}

fn load_ok(v: Option<f64>) -> bool {
    v.is_none_or(|kg| kg.is_finite() && (0.0..=300.0).contains(&kg))
}

pub(crate) fn validate_exercise(e: &Exercise) -> Result<(), String> {
    if !slug_ok(&e.id, 60) {
        return Err("An exercise id is lowercase letters, digits and dashes.".into());
    }
    if !text_ok(&e.name, 80) || !text_ok(&e.description, 400) {
        return Err("An exercise needs a short name and description.".into());
    }
    if !matches!(e.measure.as_str(), "reps" | "hold") {
        return Err("Measure must be reps or hold.".into());
    }
    if !matches!(
        e.difficulty.as_str(),
        "beginner" | "intermediate" | "advanced"
    ) {
        return Err("Difficulty must be beginner, intermediate or advanced.".into());
    }
    if !list_ok(&e.instructions, 12, 300, false) || !list_ok(&e.muscles, 10, 40, false) {
        return Err("An exercise needs instructions and target muscles.".into());
    }
    if !list_ok(&e.equipment, 8, 60, true)
        || !list_ok(&e.form, 10, 300, true)
        || !list_ok(&e.safety, 10, 300, true)
    {
        return Err("Equipment, form and safety notes must be short.".into());
    }
    if !(1..=20).contains(&e.sets) || !(1..=10).contains(&e.level) {
        return Err("Sets must be 1–20 and level 1–10.".into());
    }
    let (r, h) = (e.reps, e.hold_s);
    let ok = match e.measure.as_str() {
        "reps" => r.is_some_and(|r| (1..=200).contains(&r)) && h.is_none(),
        _ => h.is_some_and(|h| (1..=600).contains(&h)) && r.is_none(),
    };
    if !ok {
        return Err(
            "A reps exercise recommends reps (1–200); a hold recommends seconds (1–600).".into(),
        );
    }
    if e.secs_per_rep
        .is_some_and(|s| !s.is_finite() || !(0.5..=20.0).contains(&s))
    {
        return Err("Seconds per rep must be 0.5–20.".into());
    }
    if e.demo_asset.as_deref().is_some_and(|a| !text_ok(a, 120)) {
        return Err("The demonstration reference is too long.".into());
    }
    Ok(())
}

fn find<'a>(lib: &'a [Exercise], id: &str) -> Option<&'a Exercise> {
    lib.iter().find(|e| e.id == id)
}

pub(crate) fn validate_template(t: &Template, lib: &[Exercise]) -> Result<(), String> {
    if !text_ok(&t.name, 80) {
        return Err("A workout needs a short name.".into());
    }
    if !matches!(t.source.as_str(), "user" | "amara") {
        return Err("Source must be user or amara.".into());
    }
    if t.source_ref.as_deref().is_some_and(|r| !text_ok(r, 120)) {
        return Err("The source reference is too long.".into());
    }
    if t.items.is_empty() || t.items.len() > MAX_ITEMS {
        return Err(format!("A workout has 1–{MAX_ITEMS} exercises."));
    }
    if t.rest_between_s > 900 {
        return Err("Rest between exercises is at most 15 minutes.".into());
    }
    for it in &t.items {
        let Some(e) = find(lib, &it.exercise_id) else {
            return Err("A workout names an exercise that isn't in the library.".into());
        };
        if !(1..=20).contains(&it.sets) || it.rest_s > 600 || !load_ok(it.load_kg) {
            return Err(
                "Each exercise needs 1–20 sets, rest up to 10 minutes and load up to 300 kg."
                    .into(),
            );
        }
        let ok = match e.measure.as_str() {
            "reps" => it.reps.is_some_and(|r| (1..=200).contains(&r)) && it.hold_s.is_none(),
            _ => it.hold_s.is_some_and(|h| (1..=600).contains(&h)) && it.reps.is_none(),
        };
        if !ok {
            return Err(format!(
                "{} is measured in {}.",
                e.name,
                if e.measure == "reps" {
                    "reps"
                } else {
                    "hold seconds"
                }
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- deterministic maths

/// Estimated seconds: each set's work (reps × seconds per rep, or the hold), rest between sets, and rest
/// between exercises. No rest after the last set.
pub(crate) fn estimate_s(items: &[TemplateItem], rest_between_s: u32, lib: &[Exercise]) -> u32 {
    let mut total = 0.0;
    for (i, it) in items.iter().enumerate() {
        let spr = find(lib, &it.exercise_id)
            .and_then(|e| e.secs_per_rep)
            .unwrap_or(DEFAULT_SECS_PER_REP);
        let work = match (it.reps, it.hold_s) {
            (Some(r), _) => f64::from(r) * spr,
            (None, Some(h)) => f64::from(h),
            _ => 0.0,
        };
        total +=
            f64::from(it.sets) * work + f64::from(it.sets.saturating_sub(1)) * f64::from(it.rest_s);
        if i + 1 < items.len() {
            total += f64::from(rest_between_s);
        }
    }
    total.round() as u32
}

fn parse(t: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(t).ok()
}

fn secs_between(a: &str, b: &str) -> i64 {
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => (b - a).num_seconds().max(0),
        _ => 0,
    }
}

/// Paused seconds overlapping [from, to]; an open pause runs to `to`.
fn paused_within(pauses: &[Pause], from: &str, to: &str) -> i64 {
    let (Some(f), Some(t)) = (parse(from), parse(to)) else {
        return 0;
    };
    pauses
        .iter()
        .filter_map(|p| {
            let a = parse(&p.from)?.max(f);
            let b = p.to.as_deref().and_then(parse).unwrap_or(t).min(t);
            (b > a).then(|| (b - a).num_seconds())
        })
        .sum()
}

pub(crate) fn planned_sets(plan: &[TemplateItem]) -> u32 {
    plan.iter().map(|i| i.sets).sum()
}

/// The end state from what was done: every set done → completed; some → partially completed; none →
/// abandoned. A skipped set is never "done".
pub(crate) fn final_state(s: &Session) -> &'static str {
    let done = s.sets.iter().filter(|x| x.outcome == "done").count() as u32;
    if done == 0 {
        "abandoned"
    } else if done == planned_sets(&s.plan) {
        "completed"
    } else {
        "partially_completed"
    }
}

pub(crate) fn summary(s: &Session, lib: &[Exercise], now: &str) -> Summary {
    let done: Vec<&SetLog> = s.sets.iter().filter(|x| x.outcome == "done").collect();
    let planned = planned_sets(&s.plan);
    let mut by_muscle: BTreeMap<String, u32> = BTreeMap::new();
    for x in &done {
        if let Some(e) = find(lib, &x.exercise_id) {
            for m in &e.muscles {
                *by_muscle.entry(m.clone()).or_default() += 1;
            }
        }
    }
    let end = s.ended_at.as_deref().unwrap_or(now);
    let paused = s
        .started_at
        .as_deref()
        .map_or(0, |st| paused_within(&s.pauses, st, end));
    let active = s
        .started_at
        .as_deref()
        .map(|st| (secs_between(st, end) - paused).max(0) as u32);
    let load: f64 = done
        .iter()
        .map(|x| f64::from(x.reps.unwrap_or(0)) * x.load_kg.unwrap_or(0.0))
        .sum();
    Summary {
        planned_sets: planned,
        done_sets: done.len() as u32,
        skipped_sets: s.sets.iter().filter(|x| x.outcome == "skipped").count() as u32,
        completion_pct: if planned == 0 {
            0.0
        } else {
            (f64::from(done.len() as u32) / f64::from(planned) * 1000.0).round() / 10.0
        },
        total_reps: done.iter().filter_map(|x| x.reps).sum(),
        total_hold_s: done.iter().filter_map(|x| x.hold_s).sum(),
        load_volume_kg: (load * 10.0).round() / 10.0,
        sets_by_muscle: by_muscle,
        active_s: active,
        paused_s: paused.max(0) as u32,
        rest_s: done.iter().filter_map(|x| x.rest_before_s).sum(),
        estimated_s: estimate_s(&s.plan, s.rest_between_s, lib),
    }
}

// ---------------------------------------------------------------- state machine

fn ended(s: &Session) -> bool {
    matches!(
        s.state.as_str(),
        "completed" | "partially_completed" | "abandoned"
    )
}

fn advance(s: &mut Session) {
    let item = s.next_item as usize;
    if item >= s.plan.len() {
        return;
    }
    if s.next_set + 1 < s.plan[item].sets {
        s.next_set += 1;
    } else {
        s.next_item += 1;
        s.next_set = 0;
    }
}

fn past_end(s: &Session) -> bool {
    s.next_item as usize >= s.plan.len()
}

fn finish(s: &mut Session, now: &str) {
    if let Some(p) = s.pauses.last_mut().filter(|p| p.to.is_none()) {
        p.to = Some(now.to_string());
    }
    s.ended_at = Some(now.to_string());
    s.state = final_state(s).to_string();
}

/// Applies one action at `now`. Anything not allowed from the current state is refused and changes nothing.
pub(crate) fn apply(
    s: &mut Session,
    action: &Action,
    lib: &[Exercise],
    now: &str,
) -> Result<(), String> {
    if ended(s) {
        return Err("This workout has ended; it can't change.".into());
    }
    match action {
        Action::Start => {
            if s.state != "planned" {
                return Err("Only a planned workout can start.".into());
            }
            s.state = "active".into();
            s.started_at = Some(now.into());
        }
        Action::Pause => {
            if s.state != "active" {
                return Err("Only an active workout can pause.".into());
            }
            s.state = "paused".into();
            s.pauses.push(Pause {
                from: now.into(),
                to: None,
            });
        }
        Action::Resume => {
            if s.state != "paused" {
                return Err("Only a paused workout can resume.".into());
            }
            if let Some(p) = s.pauses.last_mut() {
                p.to = Some(now.into());
            }
            s.state = "active".into();
        }
        Action::LogSet {
            reps,
            hold_s,
            load_kg,
        } => {
            if s.state != "active" {
                return Err("Resume the workout to log a set.".into());
            }
            if past_end(s) {
                return Err("Every set is already logged.".into());
            }
            let it = s.plan[s.next_item as usize].clone();
            let measure = find(lib, &it.exercise_id).map_or("reps", |e| e.measure.as_str());
            let ok = match measure {
                "hold" => hold_s.is_some_and(|h| (1..=3600).contains(&h)) && reps.is_none(),
                _ => reps.is_some_and(|r| (0..=500).contains(&r)) && hold_s.is_none(),
            };
            if !ok || !load_ok(*load_kg) {
                return Err(if measure == "hold" {
                    "Log the hold in seconds (1–3600).".into()
                } else {
                    "Log the reps (0–500).".into()
                });
            }
            let prev = s.sets.last().map(|x| x.finished_at.clone());
            let rest = prev
                .map(|p| (secs_between(&p, now) - paused_within(&s.pauses, &p, now)).max(0) as u32);
            s.sets.push(SetLog {
                item: s.next_item,
                set: s.next_set,
                exercise_id: it.exercise_id.clone(),
                outcome: "done".into(),
                reps: *reps,
                hold_s: *hold_s,
                load_kg: load_kg.or(it.load_kg),
                finished_at: now.into(),
                rest_before_s: rest,
            });
            advance(s);
            if past_end(s) {
                finish(s, now);
            }
        }
        Action::Skip { scope } => {
            if s.state != "active" {
                return Err("Resume the workout to skip.".into());
            }
            if past_end(s) {
                return Err("There is nothing left to skip.".into());
            }
            let item = s.next_item;
            let sets = match scope.as_str() {
                "set" => vec![s.next_set],
                "exercise" => (s.next_set..s.plan[item as usize].sets).collect(),
                _ => return Err("Skip a set or an exercise.".into()),
            };
            for set in sets {
                s.sets.push(SetLog {
                    item,
                    set,
                    exercise_id: s.plan[item as usize].exercise_id.clone(),
                    outcome: "skipped".into(),
                    reps: None,
                    hold_s: None,
                    load_kg: None,
                    finished_at: now.into(),
                    rest_before_s: None,
                });
                advance(s);
            }
            if past_end(s) {
                finish(s, now);
            }
        }
        Action::End { abandon } => {
            finish(s, now);
            if *abandon && s.state != "completed" && !s.sets.iter().any(|x| x.outcome == "done") {
                s.state = "abandoned".into();
            }
        }
    }
    Ok(())
}

pub(crate) fn plan_from(t: &Template, id: String, now: &str) -> Session {
    Session {
        id,
        template_id: t.id.clone(),
        template_name: t.name.clone(),
        source: t.source.clone(),
        source_ref: t.source_ref.clone(),
        plan: t.items.clone(),
        rest_between_s: t.rest_between_s,
        state: "planned".into(),
        planned_at: now.into(),
        started_at: None,
        ended_at: None,
        pauses: Vec::new(),
        sets: Vec::new(),
        next_item: 0,
        next_set: 0,
        engine: ENGINE_VERSION.into(),
        offer_sid: None,
    }
}

// ---------------------------------------------------------------- store

fn dir(data: &Path) -> PathBuf {
    data.join("workout")
}

pub(crate) fn library_in(data: &Path) -> Result<Vec<Exercise>, String> {
    let custom: Library = read_json(&dir(data).join("exercises.json"), MAX_BYTES)?;
    let mut all = built_in().1.clone();
    all.extend(
        custom
            .exercises
            .into_iter()
            .filter(|e| validate_exercise(e).is_ok()),
    );
    Ok(all)
}

pub(crate) fn save_exercise_in(data: &Path, mut e: Exercise) -> Result<Exercise, String> {
    e.custom = true;
    if !e.id.starts_with("custom-") {
        e.id = format!("custom-{}", e.id);
    }
    validate_exercise(&e)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if built_in().1.iter().any(|b| b.id == e.id) {
        return Err("That id belongs to a built-in exercise.".into());
    }
    let path = dir(data).join("exercises.json");
    let mut lib: Library = read_json(&path, MAX_BYTES)?;
    match lib.exercises.iter().position(|x| x.id == e.id) {
        Some(i) => lib.exercises[i] = e.clone(),
        None if lib.exercises.len() >= MAX_CUSTOM => {
            return Err("Too many custom exercises are saved.".into())
        }
        None => lib.exercises.push(e.clone()),
    }
    write_json_atomic(&path, &lib, MAX_BYTES)?;
    Ok(e)
}

pub(crate) fn templates_in(data: &Path) -> Result<Vec<Template>, String> {
    let t: Templates = read_json(&dir(data).join("templates.json"), MAX_BYTES)?;
    Ok(t.templates)
}

pub(crate) fn save_template_in(
    data: &Path,
    mut t: Template,
    now: &str,
) -> Result<Template, String> {
    let lib = library_in(data)?;
    validate_template(&t, &lib)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = dir(data).join("templates.json");
    let mut all: Templates = read_json(&path, MAX_BYTES)?;
    t.updated_at = now.into();
    if t.id.is_empty() {
        if all.templates.len() >= MAX_TEMPLATES {
            return Err("Too many workouts are saved.".into());
        }
        t.id = format!("wt-{}", uuid::Uuid::new_v4().simple());
        t.created_at = now.into();
        all.templates.push(t.clone());
    } else {
        let Some(x) = all.templates.iter_mut().find(|x| x.id == t.id) else {
            return Err("That workout doesn't exist.".into());
        };
        t.created_at = x.created_at.clone();
        *x = t.clone();
    }
    write_json_atomic(&path, &all, MAX_BYTES)?;
    Ok(t)
}

pub(crate) fn sessions_in(data: &Path) -> Result<Vec<Session>, String> {
    let s: Sessions = read_json(&dir(data).join("sessions.json"), MAX_BYTES)?;
    Ok(s.sessions)
}

fn view(s: Session, lib: &[Exercise], now: &str) -> SessionView {
    let summary = summary(&s, lib, now);
    SessionView {
        session: s,
        summary,
    }
}

/// Plans a session from a template. Only one session may be open (planned, active or paused) at a time.
/// `offer_sid` names the take offer that launched it (Amara templates only).
pub(crate) fn plan_in(
    data: &Path,
    template_id: &str,
    offer_sid: Option<&str>,
    now: &str,
) -> Result<SessionView, String> {
    if offer_sid.is_some_and(|o| !text_ok(o, 160) || o.contains(char::is_whitespace)) {
        return Err("That offer id is not valid.".into());
    }
    let lib = library_in(data)?;
    let t = templates_in(data)?
        .into_iter()
        .find(|t| t.id == template_id && !t.archived)
        .ok_or("That workout doesn't exist.")?;
    validate_template(&t, &lib)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = dir(data).join("sessions.json");
    let mut all: Sessions = read_json(&path, MAX_BYTES)?;
    if all.sessions.iter().any(|s| !ended(s)) {
        return Err("Another workout is still open. Finish or end it first.".into());
    }
    if all.sessions.len() >= MAX_SESSIONS {
        return Err("The workout history is full; nothing was dropped to make room.".into());
    }
    let mut s = plan_from(&t, format!("ws-{}", uuid::Uuid::new_v4().simple()), now);
    if t.source == "amara" {
        s.offer_sid = offer_sid.map(str::to_string);
    }
    all.sessions.push(s.clone());
    write_json_atomic(&path, &all, MAX_BYTES)?;
    Ok(view(s, &lib, now))
}

/// Applies an action to a session and saves it, then reads it back. Returns the view and whether it just ended.
pub(crate) fn act_in(
    data: &Path,
    id: &str,
    action: &Action,
    now: &str,
) -> Result<(SessionView, bool), String> {
    let lib = library_in(data)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = dir(data).join("sessions.json");
    let mut all: Sessions = read_json(&path, MAX_BYTES)?;
    let s = all
        .sessions
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or("That workout doesn't exist.")?;
    let was_open = !ended(s);
    apply(s, action, &lib, now)?;
    let just_ended = was_open && ended(s);
    let out = s.clone();
    write_json_atomic(&path, &all, MAX_BYTES)?;
    // A successful write is not proof: read it back.
    let back: Sessions = read_json(&path, MAX_BYTES)?;
    if back.sessions.iter().find(|x| x.id == id) != Some(&out) {
        return Err("The workout couldn't be saved. Try again.".into());
    }
    Ok((view(out, &lib, now), just_ended))
}

pub(crate) fn history_in(data: &Path, limit: usize, now: &str) -> Result<Vec<SessionView>, String> {
    let lib = library_in(data)?;
    let mut all = sessions_in(data)?;
    all.sort_by(|a, b| b.planned_at.cmp(&a.planned_at));
    Ok(all
        .into_iter()
        .take(limit.min(500))
        .map(|s| view(s, &lib, now))
        .collect())
}

// ---------------------------------------------------------------- Amara takes (9b)

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeTemplate {
    take_id: String,
    name: String,
    rest_between_s: u32,
    items: Vec<TemplateItem>,
}

#[derive(Deserialize)]
struct TakeTemplates {
    takes: Vec<TakeTemplate>,
}

fn take_templates() -> &'static [TakeTemplate] {
    static T: OnceLock<Vec<TakeTemplate>> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str::<TakeTemplates>(include_str!("workout/amara_takes.json"))
            .expect("the take mapping is valid JSON (tested)")
            .takes
    })
}

/// The take ids that can run as workouts (tests; the command returns names and estimates too).
#[cfg(test)]
pub(crate) fn mapped_take_ids() -> Vec<String> {
    take_templates().iter().map(|t| t.take_id.clone()).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeWorkout {
    pub take_id: String,
    pub name: String,
    pub estimated_s: u32,
}

/// An Amara take as a template in the same engine, reused when one already exists for that take so history
/// groups by it. Provenance: `source: amara`, `sourceRef: take:<id>`.
pub(crate) fn template_from_take_in(
    data: &Path,
    take_id: &str,
    now: &str,
) -> Result<Template, String> {
    let m = take_templates()
        .iter()
        .find(|t| t.take_id == take_id)
        .ok_or("That suggestion doesn't run as a workout.")?;
    let source_ref = format!("take:{}", m.take_id);
    if let Some(t) = templates_in(data)?
        .into_iter()
        .find(|t| t.source == "amara" && t.source_ref.as_deref() == Some(source_ref.as_str()))
    {
        return Ok(t);
    }
    save_template_in(
        data,
        Template {
            id: String::new(),
            name: m.name.clone(),
            source: "amara".into(),
            source_ref: Some(source_ref),
            items: m.items.clone(),
            rest_between_s: m.rest_between_s,
            archived: false,
            created_at: String::new(),
            updated_at: String::new(),
        },
        now,
    )
}

// ---------------------------------------------------------------- commands

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

fn now() -> String {
    iso(&now_jst())
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub version: String,
    pub exercises: Vec<Exercise>,
}

#[tauri::command]
#[specta::specta]
pub async fn workout_library() -> Result<LibraryView, String> {
    blocking(JoinErr::Plain, || {
        Ok(LibraryView {
            version: library_version().into(),
            exercises: library_in(&data())?,
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_exercise_save(exercise: Exercise) -> Result<Exercise, String> {
    blocking(JoinErr::Plain, move || save_exercise_in(&data(), exercise)).await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_templates() -> Result<Vec<Template>, String> {
    blocking(JoinErr::Plain, || templates_in(&data())).await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_template_save(template: Template) -> Result<Template, String> {
    blocking(JoinErr::Plain, move || {
        save_template_in(&data(), template, &now())
    })
    .await
}

/// The deterministic estimate for a draft, so the builder shows the same number the engine uses.
#[tauri::command]
#[specta::specta]
pub async fn workout_estimate(
    items: Vec<TemplateItem>,
    rest_between_s: u32,
) -> Result<u32, String> {
    blocking(JoinErr::Plain, move || {
        if items.len() > MAX_ITEMS {
            return Err(format!("A workout has 1–{MAX_ITEMS} exercises."));
        }
        Ok(estimate_s(
            &items,
            rest_between_s.min(900),
            &library_in(&data())?,
        ))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_session_plan(
    template_id: String,
    offer_sid: Option<String>,
) -> Result<SessionView, String> {
    blocking(JoinErr::Plain, move || {
        plan_in(&data(), &template_id, offer_sid.as_deref(), &now())
    })
    .await
}

/// Decision 0062 (9b): an Amara take as a workout template, through the same engine.
#[tauri::command]
#[specta::specta]
pub async fn workout_template_from_take(take_id: String) -> Result<Template, String> {
    blocking(JoinErr::Plain, move || {
        template_from_take_in(&data(), &take_id, &now())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_take_ids() -> Result<Vec<TakeWorkout>, String> {
    blocking(JoinErr::Plain, || {
        let lib = library_in(&data())?;
        Ok(take_templates()
            .iter()
            .map(|t| TakeWorkout {
                take_id: t.take_id.clone(),
                name: t.name.clone(),
                estimated_s: estimate_s(&t.items, t.rest_between_s, &lib),
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_session_act(
    session_id: String,
    action: Action,
) -> Result<SessionView, String> {
    blocking(JoinErr::Plain, move || {
        let (v, _just_ended) = act_in(&data(), &session_id, &action, &now())?;
        Ok(v)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn workout_history(limit: Option<u32>) -> Result<Vec<SessionView>, String> {
    blocking(JoinErr::Plain, move || {
        history_in(&data(), limit.unwrap_or(50) as usize, &now())
    })
    .await
}

#[cfg(test)]
mod tests;
