//! Experiment loop (A5): "Try it this week" -> one stored experiment -> a before/after result.
//!
//! Follow-ups live in Hermes's `open_items.json` and take no new fields (hard rule 5), so an
//! experiment is its own sidecar: `<data>/experiments/experiments.json` (0600, atomic write,
//! capped at [`MAX_EXPERIMENTS`]). It holds the follow-up id, the JST start day, the target measure
//! (a daily_table column from [`MEASURES`]) and the finding pair it came from.
//!
//! The measure is mapped from the weekly review's finding pair ([`measure_for_pair`]); an unknown
//! or missing pair stores `None` and the result says "no measure" - nothing is guessed.
//!
//! Windows (decision: 7 + 7): before = the 7 JST days before the start day, after = the 7 days from
//! the start day. Seven keeps both sides the same length and the same weekday mix, and avoids drift
//! a 14-day baseline would add. The result needs `MIN_PER_SIDE` days with a value per side and
//! p < 0.05 (the point-biserial r test patterns.rs uses, confidence >= 2), else "no clear
//! difference" - with `reason` telling "not_enough_data" from "not_significant".
//! Every measure in the allowlist is higher-is-better; strain and workout minutes are left out
//! because neither direction is "better" without a goal.

use super::daily_table::{read_range_in, DailyRow};
use super::patterns::{mean, p_value, pearson, MIN_PER_SIDE};
use super::shared::fsx::atomic_write_with_mode;
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const MEASURES: [&str; 7] = [
    "sleep_total_min",
    "hrv_ms",
    "recovery",
    "focus_mean",
    "calm_mean",
    "mood_mean",
    "energy_mean",
];
pub(crate) const BASELINE_DAYS: i64 = 7;
pub(crate) const AFTER_DAYS: i64 = 7;
pub(crate) const MAX_EXPERIMENTS: usize = 200;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const SIGNIFICANT_P: f64 = 0.05;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    pub followup_id: i64,
    /// JST day the follow-up was created.
    pub start_day: String,
    /// A daily_table column from [`MEASURES`], or None when the source finding had no clear measure.
    pub measure: Option<String>,
    /// The weekly-review finding pair that produced the suggestion (for traceability).
    pub pair: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    experiments: Vec<Experiment>,
    /// Which stored weekly review (its `weekEnd`) already listed an experiment's result. Not part of
    /// [`Experiment`], so the IPC shape is unchanged.
    #[serde(default)]
    listed: Vec<Listed>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Listed {
    followup_id: i64,
    week_end: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    pub from: String,
    pub to: String,
    /// Days that had a value for the measure.
    pub n: usize,
    /// None when n is 0.
    pub mean: Option<f64>,
    /// The days behind `mean` (daily_table day keys).
    pub days: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentResult {
    pub followup_id: i64,
    pub start_day: String,
    pub measure: Option<String>,
    /// "better" | "worse" | "no_clear_difference".
    pub outcome: String,
    /// User-facing: "better" | "worse" | "no clear difference".
    pub label: String,
    /// For no_clear_difference only: "no_measure" | "not_enough_data" | "not_significant".
    pub reason: Option<String>,
    pub p: Option<f64>,
    pub before: Side,
    pub after: Side,
}

pub(crate) fn measure_for_pair(pair: &str) -> Option<&'static str> {
    match pair {
        "sleep_focus" | "effort_focus" => Some("focus_mean"),
        "hrv_calm" => Some("calm_mean"),
        "tag_mood" => Some("mood_mean"),
        _ => None,
    }
}

fn higher_is_better(_measure: &str) -> bool {
    true
}

fn value(row: &DailyRow, measure: &str) -> Option<f64> {
    let v = match measure {
        "sleep_total_min" => row.sleep_total_min,
        "hrv_ms" => row.hrv_ms,
        "recovery" => row.recovery,
        "focus_mean" => row.focus_mean,
        "calm_mean" => row.calm_mean,
        "mood_mean" => row.mood_mean,
        "energy_mean" => row.energy_mean,
        _ => None,
    };
    v.filter(|x| x.is_finite())
}

// ---------------------------------------------------------------- store

fn file(dir: &Path) -> PathBuf {
    dir.join("experiments").join("experiments.json")
}

fn load(dir: &Path) -> Result<Store, String> {
    let p = file(dir);
    match fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(_) => return Err("storage_error".into()),
        Ok(m) if !m.is_file() || m.len() > MAX_FILE_BYTES => return Err("unsafe_storage".into()),
        Ok(_) => {}
    }
    let text = fs::read_to_string(&p).map_err(|_| "storage_error".to_string())?;
    serde_json::from_str(&text).map_err(|_| "malformed_file".to_string())
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn save(dir: &Path, store: &Store) -> Result<(), String> {
    let p = file(dir);
    crate::app_paths::create_private_dir_all(p.parent().ok_or("storage_error")?)
        .map_err(|_| "storage_error".to_string())?;
    let bytes = serde_json::to_vec_pretty(store).map_err(|_| "storage_error".to_string())?;
    atomic_write_with_mode(&p, &bytes, Some(0o600)).map_err(|_| "storage_error".to_string())
}

/// Stores one experiment for a follow-up (idempotent per follow-up id). `pair` is the weekly-review
/// finding pair; anything not in the mapping stores `measure: None`. Keeps the newest
/// [`MAX_EXPERIMENTS`].
pub(crate) fn record_start_in(
    dir: &Path,
    followup_id: i64,
    pair: Option<&str>,
    start_day: NaiveDate,
) -> Result<Experiment, String> {
    if followup_id <= 0 {
        return Err("bad_followup_id".to_string());
    }
    let _l = LOCK.lock().map_err(|_| "storage_error".to_string())?;
    let mut store = load(dir)?;
    if let Some(e) = store
        .experiments
        .iter()
        .find(|e| e.followup_id == followup_id)
    {
        return Ok(e.clone());
    }
    let pair =
        pair.filter(|p| p.len() <= 40 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    let exp = Experiment {
        followup_id,
        start_day: start_day.to_string(),
        measure: pair.and_then(measure_for_pair).map(str::to_string),
        pair: pair.map(str::to_string),
    };
    store.experiments.push(exp.clone());
    if store.experiments.len() > MAX_EXPERIMENTS {
        let drop = store.experiments.len() - MAX_EXPERIMENTS;
        store.experiments.drain(..drop);
    }
    save(dir, &store)?;
    Ok(exp)
}

// ---------------------------------------------------------------- result

/// The "after" window starts on the start day itself, which is only partly after the change.
fn side(from: NaiveDate, to: NaiveDate, rows: &[DailyRow], measure: &str) -> (Side, Vec<f64>) {
    let (f, t) = (from.to_string(), to.to_string());
    let mut days = Vec::new();
    let mut vals = Vec::new();
    for r in rows.iter().filter(|r| r.day >= f && r.day <= t) {
        if let Some(v) = value(r, measure) {
            days.push(r.day.clone());
            vals.push(v);
        }
    }
    let m = (!vals.is_empty()).then(|| mean(&vals));
    (
        Side {
            from: f,
            to: t,
            n: vals.len(),
            mean: m,
            days,
        },
        vals,
    )
}

/// Pure: the before/after result for one experiment from daily rows.
pub(crate) fn evaluate(exp: &Experiment, rows: &[DailyRow]) -> Option<ExperimentResult> {
    let start = parse_day(&exp.start_day)?;
    let (b_from, b_to) = (
        start - Duration::days(BASELINE_DAYS),
        start - Duration::days(1),
    );
    let (a_from, a_to) = (start, start + Duration::days(AFTER_DAYS - 1));
    let measure = exp.measure.as_deref().filter(|m| MEASURES.contains(m));
    let m = measure.unwrap_or("");
    let (before, bv) = side(b_from, b_to, rows, m);
    let (after, av) = side(a_from, a_to, rows, m);
    let mut p = None;
    let (outcome, reason) = if measure.is_none() {
        ("no_clear_difference", Some("no_measure"))
    } else if bv.len() < MIN_PER_SIDE || av.len() < MIN_PER_SIDE {
        ("no_clear_difference", Some("not_enough_data"))
    } else {
        let x: Vec<f64> = bv
            .iter()
            .map(|_| 0.0)
            .chain(av.iter().map(|_| 1.0))
            .collect();
        let y: Vec<f64> = bv.iter().chain(av.iter()).copied().collect();
        match pearson(&x, &y) {
            None => ("no_clear_difference", Some("not_significant")),
            Some(r) => {
                let pv = p_value(r, y.len());
                p = Some(pv);
                let diff = (mean(&av) - mean(&bv)) * if higher_is_better(m) { 1.0 } else { -1.0 };
                if pv >= SIGNIFICANT_P || diff == 0.0 {
                    ("no_clear_difference", Some("not_significant"))
                } else if diff > 0.0 {
                    ("better", None)
                } else {
                    ("worse", None)
                }
            }
        }
    };
    let label = match outcome {
        "better" => "better",
        "worse" => "worse",
        _ => "no clear difference",
    };
    Some(ExperimentResult {
        followup_id: exp.followup_id,
        start_day: exp.start_day.clone(),
        measure: measure.map(str::to_string),
        outcome: outcome.into(),
        label: label.into(),
        reason: reason.map(str::to_string),
        p,
        before,
        after,
    })
}

/// A result is dropped once its review week ends more than this many days after its due day.
pub(crate) const CARRY_OVER_DAYS: i64 = 28;

/// Results to list in the stored weekly review that ends on `review_end_day` (decision 0022, item 2).
/// An experiment is due on `start + 7`. It is listed in the first stored review whose week end is on
/// or after its due day, unless an earlier stored review already listed it (the `listed` ledger,
/// written by [`mark_listed`] only when a review was stored), and is dropped when that week end is
/// more than [`CARRY_OVER_DAYS`] past due. A review recomputed for the same week keeps its own
/// results. A daily-table read failure counts as no rows ("not_enough_data"), never a guess.
pub(crate) fn results_for_review(dir: &Path, review_end_day: &str) -> Vec<ExperimentResult> {
    let Some(end) = parse_day(review_end_day) else {
        return Vec::new();
    };
    let Ok(store) = load(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for exp in &store.experiments {
        let Some(start) = parse_day(&exp.start_day) else {
            continue;
        };
        let due = start + Duration::days(AFTER_DAYS);
        if due > end || end - due > Duration::days(CARRY_OVER_DAYS) {
            continue;
        }
        if store
            .listed
            .iter()
            .any(|l| l.followup_id == exp.followup_id && l.week_end != review_end_day)
        {
            continue;
        }
        let from = (start - Duration::days(BASELINE_DAYS)).to_string();
        let to = (start + Duration::days(AFTER_DAYS - 1)).to_string();
        let rows = read_range_in(dir, &from, &to)
            .map(|r| r.rows)
            .unwrap_or_default();
        if let Some(r) = evaluate(exp, &rows) {
            out.push(r);
        }
    }
    out
}

/// Records that the stored review for `week_end` listed these follow-ups (first listing wins).
pub(crate) fn mark_listed(dir: &Path, week_end: &str, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let _l = LOCK.lock().map_err(|_| "storage_error".to_string())?;
    let mut store = load(dir)?;
    for id in ids {
        if !store.listed.iter().any(|l| l.followup_id == *id) {
            store.listed.push(Listed {
                followup_id: *id,
                week_end: week_end.to_string(),
            });
        }
    }
    let known: Vec<i64> = store.experiments.iter().map(|e| e.followup_id).collect();
    store.listed.retain(|l| known.contains(&l.followup_id));
    save(dir, &store)
}

// ---------------------------------------------------------------- commands

/// Called by "Try it this week" right after `followups_add_item` succeeded. `pair` is the
/// weekly-review finding pair behind the suggestion (None when unknown).
#[tauri::command]
pub async fn experiments_record_start(
    followup_id: i64,
    pair: Option<String>,
) -> Result<Experiment, String> {
    blocking(JoinErr::Code("io_error"), move || {
        record_start_in(
            &crate::app_paths::data_dir(),
            followup_id,
            pair.as_deref(),
            day_key(&now_jst()),
        )
    })
    .await
}

/// Stored experiments (read-only).
#[tauri::command]
pub async fn experiments_list() -> Result<Vec<Experiment>, String> {
    blocking(JoinErr::Code("io_error"), move || {
        load(&crate::app_paths::data_dir()).map(|s| s.experiments)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-exp-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn day(s: &str) -> NaiveDate {
        parse_day(s).unwrap()
    }
    fn rows(start: &str, before: &[f64], after: &[f64]) -> Vec<DailyRow> {
        let s = day(start);
        let mut out = Vec::new();
        for (i, v) in before.iter().enumerate() {
            out.push(DailyRow {
                day: (s - Duration::days(BASELINE_DAYS - i as i64)).to_string(),
                focus_mean: Some(*v),
                ..Default::default()
            });
        }
        for (i, v) in after.iter().enumerate() {
            out.push(DailyRow {
                day: (s + Duration::days(i as i64)).to_string(),
                focus_mean: Some(*v),
                ..Default::default()
            });
        }
        out
    }
    fn exp(measure: Option<&str>) -> Experiment {
        Experiment {
            followup_id: 7,
            start_day: "2026-11-02".into(),
            measure: measure.map(str::to_string),
            pair: None,
        }
    }

    #[test]
    fn record_maps_pair_to_measure_and_unknown_is_none_never_guessed() {
        let d = tmp();
        let a = record_start_in(&d, 1, Some("sleep_focus"), day("2026-11-02")).unwrap();
        assert_eq!(a.measure.as_deref(), Some("focus_mean"));
        let b = record_start_in(&d, 2, Some("hrv_calm"), day("2026-11-02")).unwrap();
        assert_eq!(b.measure.as_deref(), Some("calm_mean"));
        assert_eq!(
            record_start_in(&d, 3, None, day("2026-11-02"))
                .unwrap()
                .measure,
            None
        );
        assert_eq!(
            record_start_in(&d, 4, Some("evil; drop"), day("2026-11-02"))
                .unwrap()
                .pair,
            None
        );
        // idempotent per follow-up id, 0600, no temp files left
        record_start_in(&d, 1, Some("tag_mood"), day("2026-11-09")).unwrap();
        let s = load(&d).unwrap();
        assert_eq!(s.experiments.len(), 4);
        assert_eq!(s.experiments[0].start_day, "2026-11-02");
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file(&d)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_dir(d.join("experiments")).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_followup_ids_are_rejected_and_the_dir_is_private() {
        let d = tmp();
        for id in [0, -1] {
            assert_eq!(
                record_start_in(&d, id, None, day("2026-11-02")).unwrap_err(),
                "bad_followup_id"
            );
        }
        assert!(!file(&d).exists());
        record_start_in(&d, 1, None, day("2026-11-02")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(d.join("experiments"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn store_is_capped() {
        let d = tmp();
        // ids start at 1: 0 and below are rejected
        for i in 1..=(MAX_EXPERIMENTS as i64 + 5) {
            record_start_in(&d, i, None, day("2026-11-02")).unwrap();
        }
        let s = load(&d).unwrap();
        assert_eq!(s.experiments.len(), MAX_EXPERIMENTS);
        assert_eq!(s.experiments[0].followup_id, 6);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn clear_rise_is_better_and_clear_fall_is_worse() {
        let low = [40.0, 42.0, 41.0, 43.0, 40.0, 42.0, 41.0];
        let high = [60.0, 62.0, 61.0, 63.0, 60.0, 62.0, 61.0];
        let r = evaluate(&exp(Some("focus_mean")), &rows("2026-11-02", &low, &high)).unwrap();
        assert_eq!((r.outcome.as_str(), r.label.as_str()), ("better", "better"));
        assert_eq!((r.before.n, r.after.n), (7, 7));
        assert_eq!(r.before.days.len(), 7);
        assert!((r.after.mean.unwrap() - 61.2857).abs() < 0.01);
        assert!(r.p.unwrap() < 0.05 && r.reason.is_none());
        let r = evaluate(&exp(Some("focus_mean")), &rows("2026-11-02", &high, &low)).unwrap();
        assert_eq!(r.outcome, "worse");
    }

    #[test]
    fn noise_is_no_clear_difference_not_significant() {
        let a = [50.0, 60.0, 40.0, 55.0, 45.0, 52.0, 48.0];
        let b = [51.0, 41.0, 59.0, 47.0, 55.0, 49.0, 53.0];
        let r = evaluate(&exp(Some("focus_mean")), &rows("2026-11-02", &a, &b)).unwrap();
        assert_eq!(r.outcome, "no_clear_difference");
        assert_eq!(r.label, "no clear difference");
        assert_eq!(r.reason.as_deref(), Some("not_significant"));
    }

    #[test]
    fn too_few_days_is_not_enough_data_and_no_measure_is_named() {
        let r = evaluate(
            &exp(Some("focus_mean")),
            &rows("2026-11-02", &[40.0, 41.0], &[60.0, 61.0, 62.0, 63.0]),
        )
        .unwrap();
        assert_eq!(r.reason.as_deref(), Some("not_enough_data"));
        assert_eq!((r.before.n, r.after.n), (2, 4));
        let none = evaluate(&exp(None), &rows("2026-11-02", &[1.0; 7], &[9.0; 7])).unwrap();
        assert_eq!(none.reason.as_deref(), Some("no_measure"));
        assert_eq!(none.label, "no clear difference");
        // a measure outside the allowlist is treated as no measure
        let bad = evaluate(
            &exp(Some("strain")),
            &rows("2026-11-02", &[1.0; 7], &[9.0; 7]),
        )
        .unwrap();
        assert_eq!(bad.reason.as_deref(), Some("no_measure"));
    }

    #[test]
    fn due_from_its_due_day_until_28_days_after_and_unreadable_table_is_not_enough_data() {
        let d = tmp();
        record_start_in(&d, 1, Some("sleep_focus"), day("2026-11-02")).unwrap();
        // start+7 = 2026-11-09
        assert_eq!(results_for_review(&d, "2026-11-08").len(), 0);
        let r = results_for_review(&d, "2026-11-09");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].reason.as_deref(), Some("not_enough_data")); // no daily.db at all
        assert_eq!(results_for_review(&d, "2026-11-15").len(), 1);
        // Changed by decision 0022 item 2: a result is carried into later reviews (was 0 here),
        // until its week end is more than 28 days past due (start+7 = 11-09).
        assert_eq!(results_for_review(&d, "2026-11-16").len(), 1);
        assert_eq!(results_for_review(&d, "2026-12-07").len(), 1);
        assert_eq!(results_for_review(&d, "2026-12-08").len(), 0);
        assert_eq!(results_for_review(&d, "garbage").len(), 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn malformed_store_is_an_error_and_never_overwritten() {
        let d = tmp();
        fs::create_dir_all(d.join("experiments")).unwrap();
        fs::write(file(&d), "{nope").unwrap();
        assert_eq!(
            record_start_in(&d, 1, None, day("2026-11-02")).unwrap_err(),
            "malformed_file"
        );
        assert_eq!(fs::read_to_string(file(&d)).unwrap(), "{nope");
        let _ = fs::remove_dir_all(&d);
    }
}
