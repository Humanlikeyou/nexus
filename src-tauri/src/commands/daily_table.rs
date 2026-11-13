//! The daily table: one row per JST day, one column per measure, in `daily.db` (app data dir).
//!
//! Derived data. Every source store is read-only here (hard rule 9: Muse recordings are sacred; the
//! follow-ups list belongs to Hermes). A rebuild replaces all rows in ONE transaction. Rules are
//! decision 0020: a column is NULL when the day has no source rows for it (counts too, never 0),
//! NOOP values are raw and keep NOOP's own `day`, and the only day-key code is
//! `shared::time::day_key`, reached through [`day_of_instant`].
//!
//! Shape: live gatherers fill the pure [`Inputs`], [`build`] is pure, [`write_in`] stores. A source
//! that fails to load leaves its columns NULL and is named in meta `partial`; nothing is guessed.
//! Nothing here logs values.

use super::shared::sqlite::open_private_wal;
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst, now_ms};
use chrono::{DateTime, Duration, NaiveDate, Timelike};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Days kept, ending today (JST).
pub(crate) const WINDOW_DAYS: i64 = 400;
const DB_FILE: &str = "daily.db";
/// `noop::recent_workouts_in` reads at most this many days (its own cap).
const WORKOUT_READ_DAYS: u32 = 90;
/// What a failed open of `daily.db` reports (no OS text or paths reach the renderer).
const STORE_UNAVAILABLE: &str = "daily table: store unavailable";
/// `daily_rebuild` refuses when the stored build is younger than this.
const MIN_REBUILD_GAP_MS: i64 = 60_000;
const NO_GOAL_NOTE: &str = "follow-ups carry no goal id";

// ---------------------------------------------------------------- inputs

#[derive(Debug, Clone, Default)]
pub(crate) struct NoopDay {
    pub day: String,
    pub sleep_min: Option<f64>,
    pub hrv: Option<f64>,
    pub recovery: Option<f64>,
    pub strain: Option<f64>,
}

/// `day` is NOOP's own (decision 7). `started_at` only names the row in sources.
#[derive(Debug, Clone)]
pub(crate) struct WorkoutIn {
    pub day: String,
    pub started_at: String,
    pub duration_min: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct MuseIn {
    pub id: String,
    /// RFC3339.
    pub start: String,
    pub duration_secs: u64,
    pub avg_focus: Option<f64>,
    pub calm_pct: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct HemiIn {
    pub id: String,
    /// RFC3339.
    pub started_at: String,
    pub listened_ms: u64,
    pub rating: Option<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckinIn {
    pub id: String,
    /// The check-in's own JST day label.
    pub day: String,
    pub mood: f64,
    pub energy: f64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TakeEventIn {
    pub id: String,
    /// `take_shown` or `take_rating`; anything else is ignored.
    pub kind: String,
    pub at: i64,
    /// The event's own day label.
    pub day: Option<String>,
    pub take_id: Option<String>,
    pub helpful: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct FollowupIn {
    pub id: i64,
    /// `done` date as stored (`YYYY-MM-DD`).
    pub done: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StepIn {
    pub goal_id: String,
    pub step_id: String,
    /// RFC3339 `doneAt`.
    pub done_at: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Inputs {
    pub noop: Vec<NoopDay>,
    pub workouts: Vec<WorkoutIn>,
    pub muse: Vec<MuseIn>,
    pub hemi: Vec<HemiIn>,
    pub checkins: Vec<CheckinIn>,
    pub take_events: Vec<TakeEventIn>,
    pub followups: Vec<FollowupIn>,
    pub steps: Vec<StepIn>,
    /// Sources that failed to load. Their columns are NULL.
    pub partial: Vec<String>,
}

// ---------------------------------------------------------------- output rows

type Sources = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRow {
    pub day: String,
    pub sleep_total_min: Option<f64>,
    pub hrv_ms: Option<f64>,
    pub recovery: Option<f64>,
    pub strain: Option<f64>,
    pub workout_min: Option<f64>,
    pub muse_min: Option<f64>,
    pub focus_mean: Option<f64>,
    pub calm_mean: Option<f64>,
    pub hemisync_min: Option<f64>,
    pub hemisync_rating_mean: Option<f64>,
    pub mood_mean: Option<f64>,
    pub energy_mean: Option<f64>,
    pub checkin_tags: Option<Vec<String>>,
    pub takes_shown: Option<i64>,
    pub takes_rated: Option<i64>,
    pub takes_helpful: Option<i64>,
    pub followups_closed: Option<i64>,
    /// Column name (snake_case, as in the table) to the source refs behind its cell.
    pub sources: Sources,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyGoalRow {
    pub day: String,
    pub goal_id: String,
    pub roadmap_steps_done: Option<i64>,
    /// Always NULL: follow-ups carry no goal id.
    pub followups_closed: Option<i64>,
    pub sources: Sources,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyBuild {
    pub days: u32,
    pub built_at_ms: i64,
    pub partial: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyRange {
    pub rows: Vec<DailyRow>,
    pub goals: Vec<DailyGoalRow>,
    pub built_at_ms: i64,
}

// ---------------------------------------------------------------- day keys

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// The ONE way an instant becomes a day label: parse, then `shared::time::day_key`.
fn day_of_instant(rfc3339: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|t| fmt(day_key(&t)))
}

/// A source-labelled day (NOOP, check-in, take event, follow-up): kept as its label when it is a
/// strict `YYYY-MM-DD`.
fn labelled_day(s: &str) -> Option<String> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    (s.len() == 10).then(|| fmt(d))
}

// ---------------------------------------------------------------- pure build

#[derive(Default)]
struct Acc {
    row: DailyRow,
}

fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

fn sum(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>())
}

fn put(src: &mut Sources, col: &str, refs: Vec<String>) {
    if !refs.is_empty() {
        let e = src.entry(col.to_string()).or_default();
        e.extend(refs);
        e.sort();
        e.dedup();
    }
}

/// Values and refs gathered for one day and one column.
#[derive(Default)]
struct Col {
    vals: Vec<f64>,
    refs: Vec<String>,
}

impl Col {
    fn add(&mut self, v: f64, r: String) {
        self.vals.push(v);
        self.refs.push(r);
    }
}

#[derive(Default)]
struct DayBag {
    sleep: Col,
    hrv: Col,
    recovery: Col,
    strain: Col,
    workout: Col,
    muse: Col,
    focus: Col,
    calm: Col,
    hemi: Col,
    hemi_rating: Col,
    mood: Col,
    energy: Col,
    tags: BTreeSet<String>,
    checkin_refs: Vec<String>,
    shown: BTreeMap<String, ()>,
    shown_refs: Vec<String>,
    /// takeId -> (at, order, helpful, event id): the last rating wins.
    ratings: BTreeMap<String, (i64, usize, bool, String)>,
    followups: Vec<String>,
}

fn bag(
    days: &mut BTreeMap<String, DayBag>,
    d: String,
    from: NaiveDate,
    to: NaiveDate,
) -> Option<&mut DayBag> {
    let x = NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()?;
    (x >= from && x <= to).then(|| days.entry(d).or_default())
}

/// Pure: the table from the gathered inputs, for days in `from..=to`.
pub(crate) fn build(
    inputs: &Inputs,
    from: NaiveDate,
    to: NaiveDate,
) -> (Vec<DailyRow>, Vec<DailyGoalRow>) {
    let in_window = |d: &str| {
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(|x| x >= from && x <= to)
            .unwrap_or(false)
    };
    let mut days: BTreeMap<String, DayBag> = BTreeMap::new();
    for n in &inputs.noop {
        let Some(day) = labelled_day(&n.day) else {
            continue;
        };
        let Some(b) = bag(&mut days, day.clone(), from, to) else {
            continue;
        };
        let r = |f: &str| format!("noop:dailyMetric.{f}:{day}");
        if let Some(v) = n.sleep_min {
            b.sleep.add(v, r("totalSleepMin"));
        }
        if let Some(v) = n.hrv {
            b.hrv.add(v, r("avgHrv"));
        }
        if let Some(v) = n.recovery {
            b.recovery.add(v, r("recovery"));
        }
        if let Some(v) = n.strain {
            b.strain.add(v, r("strain"));
        }
    }
    for w in &inputs.workouts {
        let (Some(day), Some(min)) = (labelled_day(&w.day), w.duration_min) else {
            continue;
        };
        if let Some(b) = bag(&mut days, day, from, to) {
            b.workout.add(min, format!("noop:workout:{}", w.started_at));
        }
    }
    for s in &inputs.muse {
        let Some(day) = day_of_instant(&s.start) else {
            continue;
        };
        let Some(b) = bag(&mut days, day, from, to) else {
            continue;
        };
        let r = format!("session:{}", s.id);
        b.muse.add(s.duration_secs as f64 / 60.0, r.clone());
        if let Some(v) = s.avg_focus {
            b.focus.add(v, r.clone());
        }
        // calm_pct only (decision 4); avg_relaxation is never mixed in.
        if let Some(v) = s.calm_pct {
            b.calm.add(v, r);
        }
    }
    for h in &inputs.hemi {
        let Some(day) = day_of_instant(&h.started_at) else {
            continue;
        };
        let Some(b) = bag(&mut days, day, from, to) else {
            continue;
        };
        let r = format!("hemisync:{}", h.id);
        b.hemi.add(h.listened_ms as f64 / 60_000.0, r.clone());
        if let Some(v) = h.rating {
            b.hemi_rating.add(f64::from(v), r);
        }
    }
    for c in &inputs.checkins {
        let Some(day) = labelled_day(&c.day) else {
            continue;
        };
        let Some(b) = bag(&mut days, day, from, to) else {
            continue;
        };
        let r = format!("checkin:{}", c.id);
        b.mood.add(c.mood, r.clone());
        b.energy.add(c.energy, r.clone());
        b.tags.extend(c.tags.iter().cloned());
        b.checkin_refs.push(r);
    }
    for (order, e) in inputs.take_events.iter().enumerate() {
        let (Some(day), Some(tid)) = (e.day.as_deref().and_then(labelled_day), &e.take_id) else {
            continue;
        };
        let Some(b) = bag(&mut days, day, from, to) else {
            continue;
        };
        match e.kind.as_str() {
            "take_shown" => {
                b.shown.insert(tid.clone(), ());
                b.shown_refs.push(format!("take_event:{}", e.id));
            }
            "take_rating" => {
                let Some(h) = e.helpful else { continue };
                let newer = b
                    .ratings
                    .get(tid)
                    .is_none_or(|(at, o, _, _)| (e.at, order) >= (*at, *o));
                if newer {
                    b.ratings
                        .insert(tid.clone(), (e.at, order, h, e.id.clone()));
                }
            }
            _ => {}
        }
    }
    for f in &inputs.followups {
        let Some(day) = labelled_day(&f.done) else {
            continue;
        };
        if let Some(b) = bag(&mut days, day, from, to) {
            b.followups.push(format!("followup:{}", f.id));
        }
    }

    // Roadmap: per goal, steps whose doneAt falls on the JST day.
    let mut goals: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for s in &inputs.steps {
        let Some(day) = day_of_instant(&s.done_at) else {
            continue;
        };
        if in_window(&day) {
            goals
                .entry((day, s.goal_id.clone()))
                .or_default()
                .push(format!("roadmap:{}/{}", s.goal_id, s.step_id));
        }
    }

    let rows = days
        .into_iter()
        .map(|(day, b)| finish_row(day, b))
        // A day whose only inputs were unusable has no source rows: no row.
        .filter(|r| !r.sources.is_empty())
        .collect::<Vec<_>>();
    let goal_rows = goals
        .into_iter()
        .map(|((day, goal_id), mut refs)| {
            refs.sort();
            refs.dedup();
            let mut sources = Sources::new();
            let n = refs.len() as i64;
            put(&mut sources, "roadmap_steps_done", refs);
            sources.insert("followups_closed".into(), vec![NO_GOAL_NOTE.to_string()]);
            DailyGoalRow {
                day,
                goal_id,
                roadmap_steps_done: Some(n),
                followups_closed: None,
                sources,
            }
        })
        .collect();
    (rows, goal_rows)
}

fn finish_row(day: String, b: DayBag) -> DailyRow {
    let mut a = Acc::default();
    let r = &mut a.row;
    r.day = day;
    let mut s = Sources::new();
    r.sleep_total_min = sum(&b.sleep.vals);
    put(&mut s, "sleep_total_min", b.sleep.refs);
    r.hrv_ms = sum(&b.hrv.vals);
    put(&mut s, "hrv_ms", b.hrv.refs);
    r.recovery = sum(&b.recovery.vals);
    put(&mut s, "recovery", b.recovery.refs);
    r.strain = sum(&b.strain.vals);
    put(&mut s, "strain", b.strain.refs);
    r.workout_min = sum(&b.workout.vals);
    put(&mut s, "workout_min", b.workout.refs);
    r.muse_min = sum(&b.muse.vals);
    put(&mut s, "muse_min", b.muse.refs);
    r.focus_mean = mean(&b.focus.vals);
    put(&mut s, "focus_mean", b.focus.refs);
    r.calm_mean = mean(&b.calm.vals);
    put(&mut s, "calm_mean", b.calm.refs);
    r.hemisync_min = sum(&b.hemi.vals);
    put(&mut s, "hemisync_min", b.hemi.refs);
    r.hemisync_rating_mean = mean(&b.hemi_rating.vals);
    put(&mut s, "hemisync_rating_mean", b.hemi_rating.refs);
    r.mood_mean = mean(&b.mood.vals);
    put(&mut s, "mood_mean", b.mood.refs);
    r.energy_mean = mean(&b.energy.vals);
    put(&mut s, "energy_mean", b.energy.refs);
    if !b.checkin_refs.is_empty() {
        r.checkin_tags = Some(b.tags.into_iter().collect());
        put(&mut s, "checkin_tags", b.checkin_refs);
    }
    if !b.shown.is_empty() {
        r.takes_shown = Some(b.shown.len() as i64);
        put(&mut s, "takes_shown", b.shown_refs);
    }
    if !b.ratings.is_empty() {
        r.takes_rated = Some(b.ratings.len() as i64);
        r.takes_helpful = Some(b.ratings.values().filter(|v| v.2).count() as i64);
        let refs: Vec<String> = b
            .ratings
            .values()
            .map(|v| format!("take_event:{}", v.3))
            .collect();
        put(&mut s, "takes_rated", refs.clone());
        put(&mut s, "takes_helpful", refs);
    }
    if !b.followups.is_empty() {
        r.followups_closed = Some(b.followups.len() as i64);
        put(&mut s, "followups_closed", b.followups);
    }
    r.sources = s;
    a.row
}

// ---------------------------------------------------------------- store

fn db_path(dir: &Path) -> PathBuf {
    dir.join(DB_FILE)
}

fn sql(e: impl std::fmt::Display) -> String {
    format!("daily table: {e}")
}

fn ensure_schema(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS daily(
            day TEXT PRIMARY KEY, sleep_total_min REAL, hrv_ms REAL, recovery REAL, strain REAL,
            workout_min REAL, muse_min REAL, focus_mean REAL, calm_mean REAL, hemisync_min REAL,
            hemisync_rating_mean REAL, mood_mean REAL, energy_mean REAL, checkin_tags TEXT,
            takes_shown INTEGER, takes_rated INTEGER, takes_helpful INTEGER,
            followups_closed INTEGER, sources TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS daily_goal(
            day TEXT NOT NULL, goal_id TEXT NOT NULL, roadmap_steps_done INTEGER,
            followups_closed INTEGER, sources TEXT NOT NULL, PRIMARY KEY(day, goal_id));
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);",
    )
    .map_err(sql)
}

/// Replaces every row in one transaction (a failure leaves the old table as it was).
pub(crate) fn write_in(
    dir: &Path,
    rows: &[DailyRow],
    goals: &[DailyGoalRow],
    partial: &[String],
    built_at_ms: i64,
    window: (NaiveDate, NaiveDate),
) -> Result<(), String> {
    let mut c = open_private_wal(&db_path(dir)).map_err(|_| STORE_UNAVAILABLE.to_string())?;
    ensure_schema(&c)?;
    let tx = c.transaction().map_err(sql)?;
    tx.execute_batch("DELETE FROM daily; DELETE FROM daily_goal; DELETE FROM meta;")
        .map_err(sql)?;
    for r in rows {
        let tags = r
            .checkin_tags
            .as_ref()
            .map(|t| serde_json::to_string(t).map_err(sql))
            .transpose()?;
        tx.execute(
            "INSERT INTO daily VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                r.day,
                r.sleep_total_min,
                r.hrv_ms,
                r.recovery,
                r.strain,
                r.workout_min,
                r.muse_min,
                r.focus_mean,
                r.calm_mean,
                r.hemisync_min,
                r.hemisync_rating_mean,
                r.mood_mean,
                r.energy_mean,
                tags,
                r.takes_shown,
                r.takes_rated,
                r.takes_helpful,
                r.followups_closed,
                serde_json::to_string(&r.sources).map_err(sql)?
            ],
        )
        .map_err(sql)?;
    }
    for g in goals {
        tx.execute(
            "INSERT INTO daily_goal VALUES(?1,?2,?3,?4,?5)",
            params![
                g.day,
                g.goal_id,
                g.roadmap_steps_done,
                g.followups_closed,
                serde_json::to_string(&g.sources).map_err(sql)?
            ],
        )
        .map_err(sql)?;
    }
    for (k, v) in [
        ("built_at_ms", built_at_ms.to_string()),
        ("window", format!("{}..{}", fmt(window.0), fmt(window.1))),
        ("partial", serde_json::to_string(partial).map_err(sql)?),
        ("workout_window_days", WORKOUT_READ_DAYS.to_string()),
    ] {
        tx.execute("INSERT INTO meta VALUES(?1,?2)", params![k, v])
            .map_err(sql)?;
    }
    tx.commit().map_err(sql)
}

fn parse_day_arg(s: &str) -> Result<NaiveDate, String> {
    if s.len() != 10 {
        return Err("invalid date".into());
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "invalid date".to_string())
}

/// Validates a range: both `YYYY-MM-DD`, `from <= to`, at most `WINDOW_DAYS` days inclusive.
pub(crate) fn validate_range(from: &str, to: &str) -> Result<(NaiveDate, NaiveDate), String> {
    let (f, t) = (parse_day_arg(from)?, parse_day_arg(to)?);
    if f > t {
        return Err("from is after to".into());
    }
    if (t - f).num_days() + 1 > WINDOW_DAYS {
        return Err("range too long".into());
    }
    Ok((f, t))
}

fn parse_sources(s: String) -> Sources {
    serde_json::from_str(&s).unwrap_or_default()
}

pub(crate) fn built_at_ms_in(dir: &Path) -> Option<i64> {
    if !db_path(dir).exists() {
        return None;
    }
    let c = open_private_wal(&db_path(dir)).ok()?;
    c.query_row("SELECT value FROM meta WHERE key='built_at_ms'", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()?
    .parse()
    .ok()
}

pub(crate) fn read_range_in(dir: &Path, from: &str, to: &str) -> Result<DailyRange, String> {
    validate_range(from, to)?;
    if !db_path(dir).exists() {
        return Ok(DailyRange {
            rows: Vec::new(),
            goals: Vec::new(),
            built_at_ms: 0,
        });
    }
    let c = open_private_wal(&db_path(dir)).map_err(|_| STORE_UNAVAILABLE.to_string())?;
    ensure_schema(&c)?;
    let mut st = c
        .prepare(
            "SELECT day, sleep_total_min, hrv_ms, recovery, strain, workout_min, muse_min,
                    focus_mean, calm_mean, hemisync_min, hemisync_rating_mean, mood_mean,
                    energy_mean, checkin_tags, takes_shown, takes_rated, takes_helpful,
                    followups_closed, sources
             FROM daily WHERE day >= ?1 AND day <= ?2 ORDER BY day",
        )
        .map_err(sql)?;
    let rows = st
        .query_map(params![from, to], |r| {
            let tags: Option<String> = r.get(13)?;
            Ok(DailyRow {
                day: r.get(0)?,
                sleep_total_min: r.get(1)?,
                hrv_ms: r.get(2)?,
                recovery: r.get(3)?,
                strain: r.get(4)?,
                workout_min: r.get(5)?,
                muse_min: r.get(6)?,
                focus_mean: r.get(7)?,
                calm_mean: r.get(8)?,
                hemisync_min: r.get(9)?,
                hemisync_rating_mean: r.get(10)?,
                mood_mean: r.get(11)?,
                energy_mean: r.get(12)?,
                checkin_tags: tags.and_then(|t| serde_json::from_str(&t).ok()),
                takes_shown: r.get(14)?,
                takes_rated: r.get(15)?,
                takes_helpful: r.get(16)?,
                followups_closed: r.get(17)?,
                sources: parse_sources(r.get(18)?),
            })
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let mut gt = c
        .prepare(
            "SELECT day, goal_id, roadmap_steps_done, followups_closed, sources
             FROM daily_goal WHERE day >= ?1 AND day <= ?2 ORDER BY day, goal_id",
        )
        .map_err(sql)?;
    let goals = gt
        .query_map(params![from, to], |r| {
            Ok(DailyGoalRow {
                day: r.get(0)?,
                goal_id: r.get(1)?,
                roadmap_steps_done: r.get(2)?,
                followups_closed: r.get(3)?,
                sources: parse_sources(r.get(4)?),
            })
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let built_at_ms = built_at_ms_in(dir).unwrap_or(0);
    Ok(DailyRange {
        rows,
        goals,
        built_at_ms,
    })
}

// ---------------------------------------------------------------- live gatherers (read-only)

fn gather_noop(days: i64, inputs: &mut Inputs) {
    match super::patterns::fetch_noop_series(days) {
        Ok(series) => {
            let mut map: BTreeMap<String, NoopDay> = BTreeMap::new();
            for (key, pts) in series {
                for (day, v) in pts {
                    let d = map.entry(day.clone()).or_insert_with(|| NoopDay {
                        day,
                        ..Default::default()
                    });
                    match key {
                        "sleep_total_min" => d.sleep_min = Some(v),
                        "hrv" => d.hrv = Some(v),
                        "recovery" => d.recovery = Some(v),
                        "strain" => d.strain = Some(v),
                        _ => {}
                    }
                }
            }
            inputs.noop = map.into_values().collect();
        }
        Err(_) => inputs.partial.push("noop".into()),
    }
    match super::noop::recent_workouts_live(WORKOUT_READ_DAYS) {
        Ok(list) => {
            inputs.workouts = list
                .into_iter()
                .map(|w| WorkoutIn {
                    day: w.day,
                    started_at: w.started_at,
                    duration_min: w.duration_min,
                })
                .collect()
        }
        Err(_) => inputs.partial.push("noop_workouts".into()),
    }
}

pub(crate) fn gather_live() -> Inputs {
    gather_in(&crate::app_paths::data_dir())
}

pub(crate) fn gather_in(data: &Path) -> Inputs {
    let data = data.to_path_buf();
    let mut i = Inputs::default();
    gather_noop(WINDOW_DAYS + 2, &mut i);

    i.muse = super::patterns::read_sessions_capped(
        &super::sessions::sessions_dir(),
        super::patterns::MAX_SESSION_FILES,
        super::patterns::MAX_SESSION_BYTES,
    )
    .into_iter()
    .map(|s| MuseIn {
        id: s.id,
        start: s.start,
        duration_secs: s.duration_secs,
        avg_focus: s.summary.avg_focus,
        calm_pct: s.summary.calm_pct,
    })
    .collect();

    match super::hemisync::link_rows_in(&data) {
        Ok(rows) => {
            i.hemi = rows
                .into_iter()
                .map(|l| HemiIn {
                    id: l.id,
                    started_at: l.started_at,
                    listened_ms: l.listened_ms,
                    rating: l.rating,
                })
                .collect()
        }
        Err(_) => i.partial.push("hemisync".into()),
    }

    match super::checkin::list_in(&super::checkin::dir_in(&data), 3000) {
        Ok(list) => {
            i.checkins = list
                .into_iter()
                .map(|c| CheckinIn {
                    id: c.id,
                    day: c.day,
                    mood: f64::from(c.mood),
                    energy: f64::from(c.energy),
                    tags: c.tags,
                })
                .collect()
        }
        Err(_) => i.partial.push("checkins".into()),
    }

    i.take_events = super::take_events::list_in(&data.join("takes"))
        .into_iter()
        .filter(|e| e.kind == "take_shown" || e.kind == "take_rating")
        .map(|e| TakeEventIn {
            take_id: e
                .data
                .get("takeId")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            helpful: e.data.get("helpful").and_then(|v| v.as_bool()),
            id: e.id,
            kind: e.kind,
            at: e.at,
            day: e.day,
        })
        .collect();

    match super::followups::closed_days_live() {
        Ok(list) => {
            i.followups = list
                .into_iter()
                .map(|(id, done)| FollowupIn { id, done })
                .collect()
        }
        Err(_) => i.partial.push("followups".into()),
    }

    match super::goal_roadmap::load(&data) {
        Ok(store) => {
            i.steps = store
                .roadmaps
                .into_iter()
                .flat_map(|r| {
                    let goal_id = r.goal_id;
                    r.steps.into_iter().filter_map(move |s| {
                        Some(StepIn {
                            goal_id: goal_id.clone(),
                            step_id: s.id,
                            done_at: s.done_at.filter(|_| s.done)?,
                        })
                    })
                })
                .collect()
        }
        Err(_) => i.partial.push("roadmaps".into()),
    }
    i.partial.sort();
    i
}

// ---------------------------------------------------------------- build runner

static RUNNING: AtomicBool = AtomicBool::new(false);

struct RunGuard;

impl RunGuard {
    fn take() -> Result<Self, String> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| RunGuard)
            .map_err(|_| "daily build already running".to_string())
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

/// Gathers (read-only), builds and stores. Single-flight.
fn rebuild_live(dir: &Path) -> Result<DailyBuild, String> {
    let _g = RunGuard::take()?;
    let inputs = gather_live();
    let to = day_key(&now_jst());
    let from = to - Duration::days(WINDOW_DAYS - 1);
    let (rows, goals) = build(&inputs, from, to);
    let built_at_ms = now_ms() as i64;
    write_in(dir, &rows, &goals, &inputs.partial, built_at_ms, (from, to))?;
    Ok(DailyBuild {
        days: rows.len() as u32,
        built_at_ms,
        partial: inputs.partial,
    })
}

/// The on-demand command's throttle (the nightly loop builds at most once a day, so it skips this).
fn check_rebuild_gap(stored_ms: Option<i64>, now: i64) -> Result<(), String> {
    match stored_ms {
        Some(t) if now - t < MIN_REBUILD_GAP_MS => {
            Err("daily table: built under a minute ago".to_string())
        }
        _ => Ok(()),
    }
}

#[tauri::command]
pub async fn daily_rebuild() -> Result<DailyBuild, String> {
    blocking(JoinErr::Plain, || {
        let dir = crate::app_paths::data_dir();
        check_rebuild_gap(built_at_ms_in(&dir), now_ms() as i64)?;
        rebuild_live(&dir)
    })
    .await
}

#[tauri::command]
pub async fn daily_range(from: String, to: String) -> Result<DailyRange, String> {
    validate_range(&from, &to)?;
    blocking(JoinErr::Plain, move || {
        read_range_in(&crate::app_paths::data_dir(), &from, &to)
    })
    .await
}

// ---------------------------------------------------------------- nightly

const TICK: std::time::Duration = std::time::Duration::from_secs(60);
const FIRST_BUILD_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Nightly slot: once per JST day after 05:30.
const SLOT_MINUTE_OF_DAY: u32 = 5 * 60 + 30;

/// Whether to build now. `stored_day` is the JST day of the stored build, `attempted_day` the day of
/// the last attempt this launch (so a failing build is not retried every minute), `launch_pending`
/// true until the first post-launch decision has been made.
pub(crate) fn should_build(
    now: DateTime<chrono::FixedOffset>,
    since_launch: std::time::Duration,
    stored_day: Option<NaiveDate>,
    attempted_day: Option<NaiveDate>,
    launch_pending: bool,
) -> bool {
    let today = day_key(&now);
    if since_launch < FIRST_BUILD_AFTER || stored_day == Some(today) || attempted_day == Some(today)
    {
        return false;
    }
    launch_pending || now.hour() * 60 + now.minute() >= SLOT_MINUTE_OF_DAY
}

pub fn start(_app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let mut attempted: Option<NaiveDate> = None;
        let mut launch_pending = true;
        loop {
            tokio::time::sleep(TICK).await;
            if RUNNING.load(Ordering::Acquire) {
                continue;
            }
            let dir = crate::app_paths::data_dir();
            let stored = blocking(JoinErr::Plain, {
                let dir = dir.clone();
                move || Ok(built_at_ms_in(&dir))
            })
            .await
            .ok()
            .flatten()
            .and_then(DateTime::from_timestamp_millis)
            .map(|t| day_key(&t));
            let now = now_jst();
            if !should_build(now, started.elapsed(), stored, attempted, launch_pending) {
                continue;
            }
            if started.elapsed() >= FIRST_BUILD_AFTER {
                launch_pending = false;
            }
            attempted = Some(day_key(&now));
            let _ = blocking(JoinErr::Plain, move || rebuild_live(&dir)).await;
        }
    });
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("daily-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn te(
        id: &str,
        kind: &str,
        at: i64,
        day: &str,
        tid: &str,
        helpful: Option<bool>,
    ) -> TakeEventIn {
        TakeEventIn {
            id: id.into(),
            kind: kind.into(),
            at,
            day: Some(day.into()),
            take_id: Some(tid.into()),
            helpful,
        }
    }

    /// Known rows for EVERY column. Day 2026-11-12 is the full day; 2026-11-13 is sparse.
    fn fixture() -> Inputs {
        Inputs {
            noop: vec![
                NoopDay {
                    day: "2026-11-12".into(),
                    sleep_min: Some(410.0),
                    hrv: Some(54.5),
                    recovery: Some(71.0),
                    strain: Some(9.5),
                },
                // Only sleep on the 13th: the other NOOP columns stay NULL.
                NoopDay {
                    day: "2026-11-13".into(),
                    sleep_min: Some(300.0),
                    ..Default::default()
                },
                // Outside the window: dropped.
                NoopDay {
                    day: "2020-01-01".into(),
                    sleep_min: Some(1.0),
                    ..Default::default()
                },
            ],
            workouts: vec![
                WorkoutIn {
                    day: "2026-11-12".into(),
                    started_at: "2026-11-12T06:00:00+09:00".into(),
                    duration_min: Some(30.0),
                },
                WorkoutIn {
                    day: "2026-11-12".into(),
                    started_at: "2026-11-12T18:00:00+09:00".into(),
                    duration_min: Some(15.5),
                },
            ],
            muse: vec![
                MuseIn {
                    id: "s1".into(),
                    start: "2026-11-12T10:00:00+09:00".into(),
                    duration_secs: 600,
                    avg_focus: Some(60.0),
                    calm_pct: Some(40.0),
                },
                // No focus, no calm: excluded from both means, still counted in minutes.
                MuseIn {
                    id: "s2".into(),
                    start: "2026-11-12T20:00:00+09:00".into(),
                    duration_secs: 300,
                    avg_focus: None,
                    calm_pct: None,
                },
                // 2026-11-12T15:00:00Z is 00:00 JST on the 13th, not the 12th.
                MuseIn {
                    id: "edge".into(),
                    start: "2026-11-12T15:00:00Z".into(),
                    duration_secs: 120,
                    avg_focus: Some(80.0),
                    calm_pct: Some(20.0),
                },
            ],
            hemi: vec![
                // Overlaps Muse s1 (same instant): its own column, never summed with Muse.
                HemiIn {
                    id: "s1".into(),
                    started_at: "2026-11-12T10:00:00+09:00".into(),
                    listened_ms: 300_000,
                    rating: Some(4),
                },
                HemiIn {
                    id: "h2".into(),
                    started_at: "2026-11-12T21:00:00+09:00".into(),
                    listened_ms: 150_000,
                    rating: None,
                },
                HemiIn {
                    id: "h3".into(),
                    started_at: "2026-11-12T22:00:00+09:00".into(),
                    listened_ms: 60_000,
                    rating: Some(5),
                },
            ],
            checkins: vec![
                CheckinIn {
                    id: "c1".into(),
                    day: "2026-11-12".into(),
                    mood: 2.0,
                    energy: 3.0,
                    tags: vec!["work".into(), "calm".into()],
                },
                CheckinIn {
                    id: "c2".into(),
                    day: "2026-11-12".into(),
                    mood: 5.0,
                    energy: 4.0,
                    tags: vec!["calm".into(), "run".into()],
                },
            ],
            take_events: vec![
                te("e1", "take_shown", 1, "2026-11-12", "t1", None),
                te("e2", "take_shown", 2, "2026-11-12", "t1", None),
                te("e3", "take_shown", 3, "2026-11-12", "t2", None),
                te("e4", "take_rating", 10, "2026-11-12", "t1", Some(false)),
                te("e5", "take_rating", 20, "2026-11-12", "t1", Some(true)),
                te("e6", "take_rating", 30, "2026-11-12", "t2", Some(false)),
                // Not a take kind: ignored.
                te("e7", "answer", 40, "2026-11-12", "t9", None),
            ],
            followups: vec![
                FollowupIn {
                    id: 7,
                    done: "2026-11-12".into(),
                },
                FollowupIn {
                    id: 8,
                    done: "2026-11-12".into(),
                },
                FollowupIn {
                    id: 9,
                    done: "not a date".into(),
                },
            ],
            steps: vec![
                StepIn {
                    goal_id: "g1".into(),
                    step_id: "a".into(),
                    done_at: "2026-11-12T23:59:59+09:00".into(),
                },
                StepIn {
                    goal_id: "g1".into(),
                    step_id: "b".into(),
                    done_at: "2026-11-12T14:59:59Z".into(),
                },
                // JST midnight edge: 15:00:00Z is the 13th in JST.
                StepIn {
                    goal_id: "g1".into(),
                    step_id: "c".into(),
                    done_at: "2026-11-12T15:00:00Z".into(),
                },
                StepIn {
                    goal_id: "g2".into(),
                    step_id: "x".into(),
                    done_at: "2026-11-12T08:00:00+09:00".into(),
                },
            ],
            partial: vec![],
        }
    }

    fn src(r: &DailyRow, col: &str) -> Vec<String> {
        r.sources.get(col).cloned().unwrap_or_default()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_column_matches_its_source_rows_and_refs() {
        let (rows, goals) = build(&fixture(), d("2026-10-01"), d("2026-11-14"));
        let days: Vec<_> = rows.iter().map(|r| r.day.as_str()).collect();
        assert_eq!(days, ["2026-11-12", "2026-11-13"]);
        let r = &rows[0];

        assert_eq!(r.sleep_total_min, Some(410.0));
        assert_eq!(
            src(r, "sleep_total_min"),
            strs(&["noop:dailyMetric.totalSleepMin:2026-11-12"])
        );
        assert_eq!(r.hrv_ms, Some(54.5));
        assert_eq!(
            src(r, "hrv_ms"),
            strs(&["noop:dailyMetric.avgHrv:2026-11-12"])
        );
        assert_eq!(r.recovery, Some(71.0));
        assert_eq!(
            src(r, "recovery"),
            strs(&["noop:dailyMetric.recovery:2026-11-12"])
        );
        // Strain is stored raw.
        assert_eq!(r.strain, Some(9.5));
        assert_eq!(
            src(r, "strain"),
            strs(&["noop:dailyMetric.strain:2026-11-12"])
        );

        assert_eq!(r.workout_min, Some(45.5));
        assert_eq!(
            src(r, "workout_min"),
            strs(&[
                "noop:workout:2026-11-12T06:00:00+09:00",
                "noop:workout:2026-11-12T18:00:00+09:00"
            ])
        );

        // Muse: s1 10 min + s2 5 min on the 12th; "edge" starts at JST midnight so it is the 13th.
        assert_eq!(r.muse_min, Some(15.0));
        assert_eq!(src(r, "muse_min"), strs(&["session:s1", "session:s2"]));
        assert_eq!(r.focus_mean, Some(60.0));
        assert_eq!(src(r, "focus_mean"), strs(&["session:s1"]));
        assert_eq!(r.calm_mean, Some(40.0));
        assert_eq!(src(r, "calm_mean"), strs(&["session:s1"]));

        // Hemi-Sync is its own column: 5 + 2.5 + 1 min, never added to Muse minutes.
        assert_eq!(r.hemisync_min, Some(8.5));
        assert_eq!(
            src(r, "hemisync_min"),
            strs(&["hemisync:h2", "hemisync:h3", "hemisync:s1"])
        );
        assert_eq!(r.hemisync_rating_mean, Some(4.5));
        assert_eq!(
            src(r, "hemisync_rating_mean"),
            strs(&["hemisync:h3", "hemisync:s1"])
        );

        assert_eq!(r.mood_mean, Some(3.5));
        assert_eq!(src(r, "mood_mean"), strs(&["checkin:c1", "checkin:c2"]));
        assert_eq!(r.energy_mean, Some(3.5));
        assert_eq!(src(r, "energy_mean"), strs(&["checkin:c1", "checkin:c2"]));
        assert_eq!(r.checkin_tags, Some(strs(&["calm", "run", "work"])));
        assert_eq!(src(r, "checkin_tags"), strs(&["checkin:c1", "checkin:c2"]));

        // t1 shown twice counts once; two distinct takes shown.
        assert_eq!(r.takes_shown, Some(2));
        assert_eq!(
            src(r, "takes_shown"),
            strs(&["take_event:e1", "take_event:e2", "take_event:e3"])
        );
        // t1 changed false -> true (last wins), t2 false: 2 rated, 1 helpful.
        assert_eq!(r.takes_rated, Some(2));
        assert_eq!(r.takes_helpful, Some(1));
        assert_eq!(
            src(r, "takes_rated"),
            strs(&["take_event:e5", "take_event:e6"])
        );
        assert_eq!(
            src(r, "takes_helpful"),
            strs(&["take_event:e5", "take_event:e6"])
        );

        assert_eq!(r.followups_closed, Some(2));
        assert_eq!(
            src(r, "followups_closed"),
            strs(&["followup:7", "followup:8"])
        );

        // The 13th: sleep from NOOP, the JST-midnight Muse session; everything else NULL.
        let n = &rows[1];
        assert_eq!(n.sleep_total_min, Some(300.0));
        assert_eq!(n.muse_min, Some(2.0));
        assert_eq!(n.focus_mean, Some(80.0));
        assert_eq!(n.calm_mean, Some(20.0));
        assert!(n.hrv_ms.is_none() && n.recovery.is_none() && n.strain.is_none());
        assert!(n.workout_min.is_none() && n.hemisync_min.is_none());
        assert!(
            n.hemisync_rating_mean.is_none() && n.mood_mean.is_none() && n.energy_mean.is_none()
        );
        assert!(n.checkin_tags.is_none());
        assert!(n.takes_shown.is_none() && n.takes_rated.is_none() && n.takes_helpful.is_none());
        assert!(n.followups_closed.is_none());
        assert!(!n.sources.contains_key("hrv_ms"));

        // Roadmap per goal: JST-day of doneAt. a, b on the 12th (23:59:59+09, 14:59:59Z), c on the 13th.
        let g = |day: &str, id: &str| {
            goals
                .iter()
                .find(|g| g.day == day && g.goal_id == id)
                .unwrap_or_else(|| panic!("no goal row {day} {id}"))
        };
        assert_eq!(g("2026-11-12", "g1").roadmap_steps_done, Some(2));
        assert_eq!(
            g("2026-11-12", "g1").sources["roadmap_steps_done"],
            strs(&["roadmap:g1/a", "roadmap:g1/b"])
        );
        assert_eq!(g("2026-11-13", "g1").roadmap_steps_done, Some(1));
        assert_eq!(g("2026-11-12", "g2").roadmap_steps_done, Some(1));
        assert_eq!(goals.len(), 3);
        for row in &goals {
            assert!(row.followups_closed.is_none());
            assert_eq!(row.sources["followups_closed"], strs(&[NO_GOAL_NOTE]));
        }
    }

    #[test]
    fn missing_source_gives_null_not_zero_and_partial_is_stored() {
        let mut i = fixture();
        i.hemi.clear();
        i.partial = strs(&["hemisync"]);
        let (rows, _) = build(&i, d("2026-10-01"), d("2026-11-14"));
        assert_eq!(rows[0].hemisync_min, None);
        assert_eq!(rows[0].hemisync_rating_mean, None);
        assert!(!rows[0].sources.contains_key("hemisync_min"));

        let dir = tmp("partial");
        let (rows, goals) = build(&i, d("2026-10-01"), d("2026-11-14"));
        write_in(
            &dir,
            &rows,
            &goals,
            &i.partial,
            5,
            (d("2026-10-01"), d("2026-11-14")),
        )
        .unwrap();
        let c = Connection::open(db_path(&dir)).unwrap();
        let p: String = c
            .query_row("SELECT value FROM meta WHERE key='partial'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(p, r#"["hemisync"]"#);
        let n: Option<f64> = c
            .query_row(
                "SELECT hemisync_min FROM daily WHERE day='2026-11-12'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n.is_none());
    }

    #[test]
    fn empty_inputs_build_no_rows() {
        let (rows, goals) = build(&Inputs::default(), d("2026-10-01"), d("2026-11-14"));
        assert!(rows.is_empty() && goals.is_empty());
    }

    #[test]
    fn rating_without_a_helpful_flag_is_not_a_rating() {
        let i = Inputs {
            take_events: vec![te("a", "take_rating", 1, "2026-11-12", "t", None)],
            ..Default::default()
        };
        let (rows, _) = build(&i, d("2026-11-01"), d("2026-11-14"));
        assert!(rows.is_empty());
    }

    #[test]
    fn write_read_round_trip_keeps_every_cell() {
        let dir = tmp("rt");
        let (rows, goals) = build(&fixture(), d("2026-10-01"), d("2026-11-14"));
        write_in(
            &dir,
            &rows,
            &goals,
            &[],
            1234,
            (d("2026-10-01"), d("2026-11-14")),
        )
        .unwrap();
        let got = read_range_in(&dir, "2026-11-12", "2026-11-13").unwrap();
        assert_eq!(got.rows, rows);
        assert_eq!(got.goals, goals);
        assert_eq!(got.built_at_ms, 1234);
        assert_eq!(built_at_ms_in(&dir), Some(1234));
        let one = read_range_in(&dir, "2026-11-13", "2026-11-13").unwrap();
        assert_eq!(one.rows.len(), 1);
        assert_eq!(one.goals.len(), 1);
    }

    #[test]
    fn rebuild_replaces_old_rows() {
        let dir = tmp("replace");
        let w = (d("2026-10-01"), d("2026-11-14"));
        let (rows, goals) = build(&fixture(), w.0, w.1);
        write_in(&dir, &rows, &goals, &[], 1, w).unwrap();
        let small = Inputs {
            noop: vec![NoopDay {
                day: "2026-11-01".into(),
                sleep_min: Some(1.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (r2, g2) = build(&small, w.0, w.1);
        write_in(&dir, &r2, &g2, &[], 2, w).unwrap();
        let got = read_range_in(&dir, "2026-10-01", "2026-11-14").unwrap();
        assert_eq!(got.rows.len(), 1);
        assert_eq!(got.rows[0].day, "2026-11-01");
        assert!(got.goals.is_empty());
        assert_eq!(got.built_at_ms, 2);
    }

    #[test]
    fn missing_db_reads_as_empty_without_creating_one() {
        let dir = tmp("none");
        let got = read_range_in(&dir, "2026-11-01", "2026-11-02").unwrap();
        assert!(got.rows.is_empty() && got.goals.is_empty() && got.built_at_ms == 0);
        assert!(!db_path(&dir).exists());
        assert_eq!(built_at_ms_in(&dir), None);
    }

    #[test]
    fn range_validation() {
        assert!(validate_range("2026-11-01", "2026-11-01").is_ok());
        assert!(validate_range("2026-11-02", "2026-11-01").is_err());
        assert!(validate_range("2026-1-01", "2026-11-01").is_err());
        assert!(validate_range("2026-02-30", "2026-03-01").is_err());
        assert!(validate_range("nope", "2026-11-01").is_err());
        assert!(validate_range("2026-11-01", "2026-11-01 ").is_err());
        // 400 days inclusive is the most allowed.
        let from = d("2026-11-14") - Duration::days(399);
        assert!(validate_range(&fmt(from), "2026-11-14").is_ok());
        assert!(validate_range(&fmt(from - Duration::days(1)), "2026-11-14").is_err());
        let dir = tmp("badrange");
        assert!(read_range_in(&dir, "2026-11-02", "2026-11-01").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn db_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("mode");
        let w = (d("2026-10-01"), d("2026-11-14"));
        write_in(&dir, &[], &[], &[], 1, w).unwrap();
        let mode = std::fs::metadata(db_path(&dir))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn day_key_is_jst_for_instants_and_label_for_labelled_days() {
        assert_eq!(
            day_of_instant("2026-11-12T14:59:59Z").as_deref(),
            Some("2026-11-12")
        );
        assert_eq!(
            day_of_instant("2026-11-12T15:00:00Z").as_deref(),
            Some("2026-11-13")
        );
        assert_eq!(
            day_of_instant("2026-11-12T23:00:00-05:00").as_deref(),
            Some("2026-11-13")
        );
        assert_eq!(day_of_instant("2026-11-12"), None);
        assert_eq!(labelled_day("2026-11-12").as_deref(), Some("2026-11-12"));
        assert_eq!(labelled_day("2026-11-12T01:00:00+09:00"), None);
    }

    #[test]
    fn schedule_decisions() {
        use chrono::TimeZone;
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap();
        let jst_now = |h: u32, m: u32| {
            super::super::shared::time::jst()
                .with_ymd_and_hms(2026, 11, 14, h, m, 0)
                .unwrap()
        };
        let long = std::time::Duration::from_secs(3600);
        let today = Some(d("2026-11-14"));
        let yday = Some(d("2026-11-13"));
        // Not before 10 minutes after launch.
        assert!(!should_build(
            jst_now(9, 0),
            std::time::Duration::from_secs(599),
            yday,
            None,
            true
        ));
        // First build after launch: stored day is not today -> build, even before 05:30.
        assert!(should_build(
            jst_now(3, 0),
            std::time::Duration::from_secs(600),
            yday,
            None,
            true
        ));
        // Stored today: no.
        assert!(!should_build(jst_now(9, 0), long, today, None, true));
        // Later ticks wait for 05:30.
        assert!(!should_build(jst_now(5, 29), long, yday, None, false));
        assert!(should_build(jst_now(5, 30), long, yday, None, false));
        // One attempt per day.
        assert!(!should_build(jst_now(6, 0), long, yday, today, false));
        let _ = at("2026-11-14T00:00:00Z");
    }

    #[test]
    fn rebuild_throttle_uses_the_injected_clock() {
        assert!(check_rebuild_gap(None, 5).is_ok());
        assert!(check_rebuild_gap(Some(1_000), 1_000).is_err());
        assert_eq!(
            check_rebuild_gap(Some(1_000), 60_999).unwrap_err(),
            "daily table: built under a minute ago"
        );
        assert!(check_rebuild_gap(Some(1_000), 61_000).is_ok());
    }

    #[test]
    fn gatherer_reads_hemisync_links_without_a_muse_sessions_dir() {
        let data = tmp("hemi");
        let dir = data.join("hemisync");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("sessions.json"),
            r#"{"sessions":[{"id":"s1","trackId":"t","startedAt":"2026-11-12T10:00:00+09:00","listenedMs":300000,"completed":true,"rating":4}]}"#,
        )
        .unwrap();
        // No sessions dir exists under `data`; the link rows still load.
        assert!(!data.join("sessions").exists());
        let rows = super::super::hemisync::link_rows_in(&data).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s1");
        assert_eq!(rows[0].listened_ms, 300_000);
        assert_eq!(rows[0].rating, Some(4));
    }

    #[test]
    fn single_flight_guard() {
        let g = RunGuard::take().unwrap();
        assert!(RunGuard::take().is_err());
        drop(g);
        assert!(RunGuard::take().is_ok());
    }

    /// Source pin: outside the tests this module never writes, removes, renames or creates a file
    /// itself. The only store it writes is `daily.db`, through the shared sqlite opener.
    #[test]
    fn source_pin_no_writes_outside_daily_db() {
        let all = include_str!("daily_table.rs");
        let code = all.split("#[cfg(test)]").next().unwrap();
        for bad in [
            "fs::write",
            "remove_file",
            "remove_dir",
            "fs::rename",
            "create_dir",
            "File::create",
            "OpenOptions",
            "write_private",
            "atomic_write",
            "fs::copy",
            "set_permissions",
            "write_text_atomic",
            "write_json_atomic",
            "hard_link",
            "symlink",
            "set_len",
            "std::process",
            "Command::new",
        ] {
            assert!(!code.contains(bad), "daily_table.rs must not use {bad}");
        }
        // The one sqlite file it writes.
        assert_eq!(code.matches("open_private_wal(").count(), 3);
        assert!(code.contains("const DB_FILE: &str = \"daily.db\""));
    }
}
