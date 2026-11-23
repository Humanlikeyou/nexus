//! Generic patterns over the daily table (A3). Pure engine plus one `pub(crate)` entry.
//!
//! The curated findings in `patterns.rs` test four hand-picked pairs. This module runs every pair on an
//! explicit allowlist ([`PAIRS`]) at lags 0 to 3 days over the whole stored history, applies ONE
//! Benjamini-Hochberg correction across every test it ran, and publishes only what survives and still
//! clears the confidence floor.
//!
//! Test statistic: the same as `patterns.rs` (`analyse`): pair each cause day with the effect on
//! `day + lag`, split the cause into "with" and "without", need at least `MIN_PER_SIDE` days on each side,
//! Pearson r over the pairs, two-sided t-test p (`patterns::p_value`), and the with-minus-without
//! difference must have the sign of r or the test is not publishable (it still counts as a test run).
//!
//! Splits: a presence cause (workout, Muse, Hemi-Sync minutes) is "with" when it is above 0; a missing
//! value on a day that has a table row counts as 0 from the first day the column has ever had a value
//! (before that the source did not exist, so nothing is filled). Every other cause is "with" when it is at
//! or above its own full-history mean, as `hrv_calm` does with "usual".
//!
//! Multiple testing: m tests are run (every pair x lag that met the per-side minimum and had spread).
//! BH adjusted p-values are compared with `Q` = 0.05, the level of the existing floor (confidence >= 2
//! needs p < 0.05). The published `confidence` is computed from the adjusted p, so a finding never reads
//! stronger than the correction allows. Nothing is published below `MIN_CONFIDENCE`.
//!
//! Refs: a finding lists every daily row it used as `daily:<day>` (cause day and effect day), sorted.
//! Per-cell source refs stay on the rows themselves. Each finding also carries the cause-high days
//! (the "with" days that had an effect value) for callers that want to quote or act on them.
//!
//! Recent window: the last `RECENT_DAYS` cause days are summarised for the same pair and lag, descriptively
//! (not tested, not part of the correction), and only when both sides meet the minimum.

use super::daily_table::DailyRow;
use super::patterns::{confidence, mean, p_value, pearson, strength, MIN_CONFIDENCE, MIN_PER_SIDE};
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// BH false-discovery level. Equal to the p bound of the existing confidence floor.
pub(crate) const Q: f64 = 0.05;
pub(crate) const MAX_LAG: u8 = 3;
pub(crate) const RECENT_DAYS: i64 = 28;

/// (cause, effect, earliest lag). Every column is a `DailyRow` column. Rules behind the list:
/// - no self pairs; lags run 0 to 3 and never negative, so an effect is never read before its cause;
/// - a morning measure (hrv_ms, recovery) can only be an effect of something the day before or earlier
///   (lag 1+), except sleep, which ends before the morning reading;
/// - same-source pairs are left out (Muse minutes vs the focus/calm of the same sessions need lag 1+);
/// - check-in energy as a cause starts at lag 1 (it is simultaneous with that day's mood).
pub(crate) const PAIRS: &[(&str, &str, u8)] = &[
    ("sleep_total_min", "focus_mean", 0),
    ("sleep_total_min", "calm_mean", 0),
    ("sleep_total_min", "mood_mean", 0),
    ("sleep_total_min", "energy_mean", 0),
    ("sleep_total_min", "hrv_ms", 0),
    ("sleep_total_min", "recovery", 0),
    ("hrv_ms", "focus_mean", 0),
    ("hrv_ms", "calm_mean", 0),
    ("hrv_ms", "mood_mean", 0),
    ("hrv_ms", "energy_mean", 0),
    ("recovery", "focus_mean", 0),
    ("recovery", "calm_mean", 0),
    ("recovery", "mood_mean", 0),
    ("recovery", "energy_mean", 0),
    ("strain", "focus_mean", 0),
    ("strain", "calm_mean", 0),
    ("strain", "mood_mean", 0),
    ("strain", "energy_mean", 0),
    ("strain", "hrv_ms", 1),
    ("strain", "recovery", 1),
    ("workout_min", "focus_mean", 0),
    ("workout_min", "calm_mean", 0),
    ("workout_min", "mood_mean", 0),
    ("workout_min", "energy_mean", 0),
    ("workout_min", "hrv_ms", 1),
    ("workout_min", "recovery", 1),
    ("muse_min", "focus_mean", 1),
    ("muse_min", "calm_mean", 1),
    ("muse_min", "mood_mean", 0),
    ("muse_min", "energy_mean", 0),
    ("muse_min", "hrv_ms", 1),
    ("muse_min", "recovery", 1),
    ("hemisync_min", "focus_mean", 0),
    ("hemisync_min", "calm_mean", 0),
    ("hemisync_min", "mood_mean", 0),
    ("hemisync_min", "energy_mean", 0),
    ("hemisync_min", "hrv_ms", 1),
    ("hemisync_min", "recovery", 1),
    ("energy_mean", "focus_mean", 1),
    ("energy_mean", "calm_mean", 1),
    ("energy_mean", "mood_mean", 1),
    ("energy_mean", "hrv_ms", 1),
    ("energy_mean", "recovery", 1),
];

/// Causes tested as present/absent (above 0) rather than against their own mean.
const PRESENCE: &[&str] = &["workout_min", "muse_min", "hemisync_min"];

fn col(name: &str) -> Option<fn(&DailyRow) -> Option<f64>> {
    Some(match name {
        "sleep_total_min" => |r| r.sleep_total_min,
        "hrv_ms" => |r| r.hrv_ms,
        "recovery" => |r| r.recovery,
        "strain" => |r| r.strain,
        "workout_min" => |r| r.workout_min,
        "muse_min" => |r| r.muse_min,
        "hemisync_min" => |r| r.hemisync_min,
        "focus_mean" => |r| r.focus_mean,
        "calm_mean" => |r| r.calm_mean,
        "mood_mean" => |r| r.mood_mean,
        "energy_mean" => |r| r.energy_mean,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Recent {
    pub window_days: i64,
    pub n_with: usize,
    pub n_without: usize,
    pub mean_with: f64,
    pub mean_without: f64,
    pub delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenericFinding {
    pub cause: String,
    pub effect: String,
    pub lag_days: u8,
    /// "above_zero" (presence causes) or "at_or_above_mean".
    pub split: String,
    pub threshold: f64,
    pub n_with: usize,
    pub n_without: usize,
    pub mean_with: f64,
    pub mean_without: f64,
    pub delta: f64,
    pub r: f64,
    pub p_raw: f64,
    /// Benjamini-Hochberg adjusted p over every test run.
    pub p_adj: f64,
    /// 1 to 5 from `p_adj` (published only at `MIN_CONFIDENCE` or more).
    pub confidence: u8,
    pub strength: String,
    /// Cause days on the "with" side that had an effect value, ascending.
    pub cause_high_days: Vec<String>,
    /// Every cause day tested for this pair and lag (the "with" and "without" sides together, i.e. the
    /// days that had an effect value), ascending. `cause_high_days` is the subset on the "with" side.
    pub cause_days: Vec<String>,
    /// `daily:<day>` for every row used (cause day and effect day), ascending.
    pub refs: Vec<String>,
    /// Last `RECENT_DAYS` cause days, same pair and lag; None when a side is under the minimum.
    pub recent: Option<Recent>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenericResult {
    /// Tests run: the BH family size m.
    pub tests_run: usize,
    pub findings: Vec<GenericFinding>,
    /// Tests that survived BH but fell under the confidence floor, or whose direction disagreed.
    pub withheld: usize,
    /// Daily rows read.
    pub rows: usize,
    /// Presence causes left out of the run entirely because the source behind them failed to load
    /// (daily_table meta `partial`): their absent days would otherwise be zero-filled as "did not
    /// happen". Cause column names, ascending.
    pub skipped_causes: Vec<String>,
}

/// Benjamini-Hochberg adjusted p-values, in input order: sort ascending, p_(i) * m / i, then a running
/// minimum from the largest rank down so the adjusted values never decrease with rank, capped at 1.
pub(crate) fn bh_adjust(p: &[f64]) -> Vec<f64> {
    let m = p.len();
    let mut idx: Vec<usize> = (0..m).collect();
    idx.sort_by(|&a, &b| p[a].total_cmp(&p[b]).then(a.cmp(&b)));
    let mut adj = vec![0.0; m];
    let mut running = 1.0_f64;
    for (rank0, &i) in idx.iter().enumerate().rev() {
        running = running.min(p[i] * m as f64 / (rank0 + 1) as f64).min(1.0);
        adj[i] = running;
    }
    adj
}

fn parse(day: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

struct Raw {
    cause: &'static str,
    effect: &'static str,
    lag: u8,
    presence: bool,
    threshold: f64,
    /// (cause day, x, y)
    pairs: Vec<(NaiveDate, f64, f64)>,
    n_with: usize,
    n_without: usize,
    mean_with: f64,
    mean_without: f64,
    r: f64,
    p: f64,
}

/// A cause value per row day, or None when the row has no value (and no zero-fill applies).
fn cause_series(
    rows: &[(NaiveDate, &DailyRow)],
    name: &str,
    get: fn(&DailyRow) -> Option<f64>,
) -> BTreeMap<NaiveDate, f64> {
    let presence = PRESENCE.contains(&name);
    let first = rows.iter().find(|(_, r)| get(r).is_some()).map(|(d, _)| *d);
    rows.iter()
        .filter_map(|(d, r)| {
            let v = get(r).filter(|v| v.is_finite());
            match (v, first) {
                (Some(v), _) => Some((*d, v)),
                (None, Some(f)) if presence && *d >= f => Some((*d, 0.0)),
                _ => None,
            }
        })
        .collect()
}

/// Pure. All tests on the allowlist over `rows` (any order). `today` anchors the recent window.
#[cfg(test)]
pub(crate) fn compute(rows: &[DailyRow], today: NaiveDate) -> GenericResult {
    compute_skipping(rows, today, &[])
}

/// The presence cause column behind a daily_table source named in meta `partial`. Muse sessions
/// cannot fail per daily_table (no `partial` name), so `muse_min` has no entry.
pub(crate) fn cause_of_partial_source(source: &str) -> Option<&'static str> {
    match source {
        "noop_workouts" => Some("workout_min"),
        "hemisync" => Some("hemisync_min"),
        _ => None,
    }
}

/// [`compute`] with some causes excluded: not tested, not zero-filled, listed in `skipped_causes`.
pub(crate) fn compute_skipping(
    rows: &[DailyRow],
    today: NaiveDate,
    skip: &[&str],
) -> GenericResult {
    let mut dated: Vec<(NaiveDate, &DailyRow)> = rows
        .iter()
        .filter_map(|r| parse(&r.day).filter(|d| *d <= today).map(|d| (d, r)))
        .collect();
    dated.sort_by_key(|(d, _)| *d);
    dated.dedup_by_key(|(d, _)| *d);

    let mut raws: Vec<Raw> = Vec::new();
    for &(cause, effect, min_lag) in PAIRS {
        debug_assert!(cause != effect && min_lag <= MAX_LAG);
        let (Some(cget), Some(eget)) = (col(cause), col(effect)) else {
            continue;
        };
        if skip.contains(&cause) {
            continue;
        }
        let xs = cause_series(&dated, cause, cget);
        let ys = cause_series(&dated, effect, eget);
        if xs.is_empty() {
            continue;
        }
        let presence = PRESENCE.contains(&cause);
        let threshold = if presence {
            0.0
        } else {
            mean(&xs.values().copied().collect::<Vec<_>>())
        };
        for lag in min_lag..=MAX_LAG {
            let mut pairs = Vec::new();
            for (d, x) in &xs {
                let Some(y) = ys.get(&(*d + Duration::days(i64::from(lag)))) else {
                    continue;
                };
                pairs.push((*d, *x, *y));
            }
            let is_with = |x: f64| if presence { x > 0.0 } else { x >= threshold };
            let with: Vec<f64> = pairs.iter().filter(|t| is_with(t.1)).map(|t| t.2).collect();
            let without: Vec<f64> = pairs
                .iter()
                .filter(|t| !is_with(t.1))
                .map(|t| t.2)
                .collect();
            if with.len() < MIN_PER_SIDE || without.len() < MIN_PER_SIDE {
                continue;
            }
            let x: Vec<f64> = pairs.iter().map(|t| t.1).collect();
            let y: Vec<f64> = pairs.iter().map(|t| t.2).collect();
            let Some(r) = pearson(&x, &y) else {
                continue;
            };
            raws.push(Raw {
                cause,
                effect,
                lag,
                presence,
                threshold,
                n_with: with.len(),
                n_without: without.len(),
                mean_with: mean(&with),
                mean_without: mean(&without),
                r,
                p: p_value(r, pairs.len()),
                pairs,
            });
        }
    }

    let adj = bh_adjust(&raws.iter().map(|r| r.p).collect::<Vec<_>>());
    let tests_run = raws.len();
    let mut withheld = 0;
    let mut findings = Vec::new();
    for (raw, p_adj) in raws.into_iter().zip(adj) {
        let direction_ok = (raw.mean_with - raw.mean_without) * raw.r > 0.0;
        let conf = confidence(p_adj, raw.n_with, raw.n_without);
        if p_adj > Q || conf < MIN_CONFIDENCE || !direction_ok {
            if p_adj <= Q {
                withheld += 1;
            }
            continue;
        }
        findings.push(finding(raw, p_adj, conf, today));
    }
    findings.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then(b.r.abs().total_cmp(&a.r.abs()))
            .then(a.cause.cmp(&b.cause))
            .then(a.effect.cmp(&b.effect))
            .then(a.lag_days.cmp(&b.lag_days))
    });
    GenericResult {
        tests_run,
        findings,
        withheld,
        rows: dated.len(),
        skipped_causes: {
            let mut v: Vec<String> = PRESENCE
                .iter()
                .filter(|c| skip.contains(c))
                .map(|c| c.to_string())
                .collect();
            v.sort();
            v
        },
    }
}

fn finding(raw: Raw, p_adj: f64, conf: u8, today: NaiveDate) -> GenericFinding {
    let is_with = |x: f64| {
        if raw.presence {
            x > 0.0
        } else {
            x >= raw.threshold
        }
    };
    let lag = Duration::days(i64::from(raw.lag));
    let mut refs = BTreeSet::new();
    for (d, _, _) in &raw.pairs {
        refs.insert(format!("daily:{}", fmt(*d)));
        refs.insert(format!("daily:{}", fmt(*d + lag)));
    }
    let cause_high_days: Vec<String> = raw
        .pairs
        .iter()
        .filter(|t| is_with(t.1))
        .map(|t| fmt(t.0))
        .collect();
    let cause_days: Vec<String> = raw.pairs.iter().map(|t| fmt(t.0)).collect();
    let recent_from = today - Duration::days(RECENT_DAYS - 1);
    let rw: Vec<f64> = raw
        .pairs
        .iter()
        .filter(|t| t.0 >= recent_from && is_with(t.1))
        .map(|t| t.2)
        .collect();
    let ro: Vec<f64> = raw
        .pairs
        .iter()
        .filter(|t| t.0 >= recent_from && !is_with(t.1))
        .map(|t| t.2)
        .collect();
    let recent = (rw.len() >= MIN_PER_SIDE && ro.len() >= MIN_PER_SIDE).then(|| Recent {
        window_days: RECENT_DAYS,
        n_with: rw.len(),
        n_without: ro.len(),
        mean_with: mean(&rw),
        mean_without: mean(&ro),
        delta: mean(&rw) - mean(&ro),
    });
    GenericFinding {
        cause: raw.cause.to_string(),
        effect: raw.effect.to_string(),
        lag_days: raw.lag,
        split: if raw.presence {
            "above_zero"
        } else {
            "at_or_above_mean"
        }
        .to_string(),
        threshold: raw.threshold,
        n_with: raw.n_with,
        n_without: raw.n_without,
        mean_with: raw.mean_with,
        mean_without: raw.mean_without,
        delta: raw.mean_with - raw.mean_without,
        r: raw.r,
        p_raw: raw.p,
        p_adj,
        confidence: conf,
        strength: strength(raw.r).to_string(),
        cause_high_days,
        cause_days,
        refs: refs.into_iter().collect(),
        recent,
    }
}

/// Reads the whole stored daily table under `dir` and runs [`compute`]. An absent table is an empty
/// result (nothing to test), a store that cannot be read is an error.
pub(crate) fn generic_patterns_in(dir: &Path, today: NaiveDate) -> Result<GenericResult, String> {
    let from = today - Duration::days(super::daily_table::WINDOW_DAYS - 1);
    let r = super::daily_table::read_range_in(dir, &fmt(from), &fmt(today))?;
    let skip: Vec<&str> = super::daily_table::partial_in(dir)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| cause_of_partial_source(s))
        .collect();
    Ok(compute_skipping(&r.rows, today, &skip))
}

/// Live entry for other modules (A4): the app's own daily table, today in JST.
#[allow(dead_code)]
pub(crate) fn generic_patterns_live() -> Result<GenericResult, String> {
    let today = super::shared::time::day_key(&super::shared::time::now_jst());
    generic_patterns_in(&crate::app_paths::data_dir(), today)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()
    }

    /// Deterministic noise in [0, 1): splitmix64 over a counter (no clock, no rand crate).
    fn noise(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn day(n: i64) -> String {
        fmt(today() - Duration::days(n))
    }

    /// `n` days oldest first. Sleep is iid noise; focus on `d + lag` follows sleep on `d`.
    fn planted(n: i64, lag: i64, signal: f64) -> Vec<DailyRow> {
        let mut seed = 7;
        let sleep: Vec<f64> = (0..n).map(|_| 300.0 + 200.0 * noise(&mut seed)).collect();
        (0..n)
            .map(|i| {
                let from = i - lag;
                let mut seed2 = 1000 + i as u64;
                let jitter = noise(&mut seed2) * 4.0;
                let base = if from >= 0 && (from as usize) < sleep.len() {
                    50.0 + signal * (sleep[from as usize] - 400.0) / 100.0
                } else {
                    50.0
                };
                DailyRow {
                    day: day(n - 1 - i),
                    sleep_total_min: Some(sleep[i as usize]),
                    focus_mean: Some(base + jitter),
                    ..Default::default()
                }
            })
            .collect()
    }

    #[test]
    fn bh_matches_a_hand_worked_set() {
        // sorted .005 .01 .03 .04, m = 4: .02 .02 .0400 .04 (already monotone), back in input order.
        let adj = bh_adjust(&[0.01, 0.04, 0.03, 0.005]);
        let want = [0.02, 0.04, 0.04, 0.02];
        for (a, w) in adj.iter().zip(want) {
            assert!((a - w).abs() < 1e-12, "{adj:?}");
        }
    }

    #[test]
    fn bh_enforces_monotonicity_and_caps_at_one() {
        // sorted .01 .02 .02 .9, m = 4: raw .04 .04 .0267 .9; the running minimum from the top lowers
        // ranks 1 and 2 to .0267 (adjusted values never fall as rank rises).
        let adj = bh_adjust(&[0.9, 0.02, 0.01, 0.02]);
        for i in [1, 2, 3] {
            assert!((adj[i] - 0.08 / 3.0).abs() < 1e-12, "{adj:?}");
        }
        assert!((adj[0] - 0.9).abs() < 1e-12);
        // A rank-wise dip is lifted: [.001, .04, .041] m=3 -> .003 .06 .041 -> min from top: .003 .041 .041.
        let adj = bh_adjust(&[0.001, 0.04, 0.041]);
        assert!((adj[0] - 0.003).abs() < 1e-12);
        assert!((adj[1] - 0.041).abs() < 1e-12);
        assert!((adj[2] - 0.041).abs() < 1e-12);
        assert!(bh_adjust(&[0.9, 0.99]).iter().all(|p| *p <= 1.0));
        assert!(bh_adjust(&[]).is_empty());
    }

    #[test]
    fn allowlist_has_no_self_pairs_no_unknown_columns_and_lags_in_range() {
        for &(c, e, min_lag) in PAIRS {
            assert_ne!(c, e);
            assert!(col(c).is_some() && col(e).is_some(), "{c} {e}");
            assert!(min_lag <= MAX_LAG);
        }
        // Morning measures are only ever effects at lag 1+ unless sleep is the cause.
        for &(c, e, min_lag) in PAIRS {
            if (e == "hrv_ms" || e == "recovery") && c != "sleep_total_min" {
                assert!(min_lag >= 1, "{c} -> {e}");
            }
        }
        let unique: BTreeSet<_> = PAIRS.iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(unique.len(), PAIRS.len());
    }

    #[test]
    fn a_planted_effect_is_found_at_the_right_lag_with_refs_and_cause_days() {
        let rows = planted(120, 1, 6.0);
        let r = compute(&rows, today());
        let f = &r.findings[0];
        assert_eq!(
            (f.cause.as_str(), f.effect.as_str(), f.lag_days),
            ("sleep_total_min", "focus_mean", 1)
        );
        assert!(f.confidence >= MIN_CONFIDENCE && f.p_adj <= Q && f.p_adj >= f.p_raw);
        assert!(f.r > 0.8 && f.delta > 0.0);
        assert_eq!(
            r.tests_run, 4,
            "sleep -> focus at lags 0..=3 is the only pair these rows can test"
        );
        // Refs: every cause day and the effect day after it, all real rows.
        let have: BTreeSet<String> = rows.iter().map(|x| format!("daily:{}", x.day)).collect();
        assert!(!f.refs.is_empty() && f.refs.iter().all(|x| have.contains(x)));
        assert!(f.refs.windows(2).all(|w| w[0] < w[1]));
        // Cause-high days are the with side: sleep at or above the mean, and the day has an effect value.
        assert_eq!(f.cause_high_days.len(), f.n_with);
        let mean_sleep = mean(
            &rows
                .iter()
                .filter_map(|x| x.sleep_total_min)
                .collect::<Vec<_>>(),
        );
        for d in &f.cause_high_days {
            let row = rows.iter().find(|x| &x.day == d).unwrap();
            assert!(row.sleep_total_min.unwrap() >= mean_sleep);
        }
        // Other lags of the same pair are noise here and are not published.
        assert!(r.findings.iter().all(|g| !(g.cause == "sleep_total_min"
            && g.effect == "focus_mean"
            && g.lag_days == 0)));
        // Recent window is reported (28 days is enough for 3 per side).
        let rec = f.recent.as_ref().unwrap();
        assert_eq!(rec.window_days, 28);
        assert!(rec.n_with >= 3 && rec.n_without >= 3);
    }

    #[test]
    fn an_effect_that_precedes_its_cause_is_never_published() {
        // Focus on day d follows sleep on day d + 1 (the future): lag -1 is not tested.
        let rows = planted(120, -1, 6.0);
        let r = compute(&rows, today());
        assert!(
            r.findings
                .iter()
                .all(|f| !(f.cause == "sleep_total_min" && f.effect == "focus_mean")),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn pure_noise_publishes_nothing_even_across_many_tests() {
        for seed0 in [11u64, 12, 13] {
            let mut seed = seed0;
            let mut n = || noise(&mut seed);
            let rows: Vec<DailyRow> = (0..150)
                .map(|i| DailyRow {
                    day: day(149 - i),
                    sleep_total_min: Some(300.0 + 200.0 * n()),
                    hrv_ms: Some(40.0 + 30.0 * n()),
                    recovery: Some(100.0 * n()),
                    strain: Some(100.0 * n()),
                    workout_min: (n() > 0.6).then(|| 30.0 + 30.0 * n()),
                    muse_min: (n() > 0.5).then(|| 10.0 + 20.0 * n()),
                    hemisync_min: (n() > 0.7).then(|| 15.0 + 15.0 * n()),
                    focus_mean: Some(40.0 + 30.0 * n()),
                    calm_mean: Some(40.0 + 30.0 * n()),
                    mood_mean: Some(1.0 + 4.0 * n()),
                    energy_mean: Some(1.0 + 4.0 * n()),
                    ..Default::default()
                })
                .collect();
            let r = compute(&rows, today());
            assert!(r.tests_run > 100, "{}", r.tests_run);
            assert!(r.findings.is_empty(), "seed {seed0}: {:?}", r.findings);
        }
    }

    #[test]
    fn min_days_per_side_is_enforced() {
        // Two long-sleep days among many short ones, effect huge: the "with" side has 2 < 3.
        let mut rows: Vec<DailyRow> = (0..30)
            .map(|i| DailyRow {
                day: day(29 - i),
                sleep_total_min: Some(if i < 2 { 900.0 } else { 300.0 + (i % 3) as f64 }),
                focus_mean: Some(if i < 2 { 90.0 } else { 40.0 + (i % 4) as f64 }),
                ..Default::default()
            })
            .collect();
        let r = compute(&rows, today());
        assert!(r.findings.is_empty());
        assert_eq!(r.tests_run, 0, "no test may run under the per-side minimum");
        // With the third high day it runs and is found.
        rows[2].sleep_total_min = Some(900.0);
        rows[2].focus_mean = Some(90.0);
        let r = compute(&rows, today());
        assert!(r.tests_run > 0);
        assert!(r.findings.iter().any(|f| f.cause == "sleep_total_min"));
    }

    #[test]
    fn hemisync_day_is_a_presence_cause_with_zero_fill_only_after_the_first_session() {
        // 40 days. Hemi-Sync on every 3rd day from day 10; mood next day is higher after it.
        let rows: Vec<DailyRow> = (0..40)
            .map(|i| {
                let hemi = i >= 10 && i % 3 == 0;
                let after = i >= 11 && (i - 1) % 3 == 0;
                DailyRow {
                    day: day(39 - i),
                    hemisync_min: hemi.then_some(20.0),
                    mood_mean: Some(if after { 4.5 } else { 3.0 } + (i % 4) as f64 * 0.05),
                    ..Default::default()
                }
            })
            .collect();
        let r = compute(&rows, today());
        let f = r
            .findings
            .iter()
            .find(|f| f.cause == "hemisync_min" && f.effect == "mood_mean")
            .expect("found");
        assert_eq!(f.lag_days, 1);
        assert_eq!(f.split, "above_zero");
        // Days before the first session (0..10) are unknown, not zero: never in the without side.
        let first = day(29);
        assert!(f.refs.iter().all(|x| x[6..] >= *first.as_str()));
        assert!(f
            .cause_high_days
            .iter()
            .all(|d| d.as_str() >= first.as_str()));
    }

    #[test]
    fn ordering_is_deterministic_and_input_order_does_not_matter() {
        let rows = planted(120, 2, 5.0);
        let a = compute(&rows, today());
        let mut rev = rows.clone();
        rev.reverse();
        let b = compute(&rev, today());
        assert_eq!(a, b);
        assert_eq!(a, compute(&rows, today()));
        for w in a.findings.windows(2) {
            assert!(
                (w[0].confidence, w[0].r.abs()) >= (w[1].confidence, w[1].r.abs()),
                "not sorted"
            );
        }
    }

    #[test]
    fn nothing_below_the_floor_is_published() {
        let r = compute(&planted(120, 1, 6.0), today());
        assert!(r
            .findings
            .iter()
            .all(|f| f.confidence >= MIN_CONFIDENCE && f.p_adj < 0.05 + 1e-12));
    }

    #[test]
    fn an_empty_or_missing_table_is_an_empty_result() {
        assert_eq!(compute(&[], today()).tests_run, 0);
        let dir = super::super::local_store::tests::temp_dir("pgen");
        let r = generic_patterns_in(&dir, today()).unwrap();
        assert!(r.findings.is_empty() && r.rows == 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_partial_source_excludes_its_cause_instead_of_zero_filling_it() {
        // Same rows as the hemisync test, stored through the real table. Without a `partial` mark the
        // finding is published; with meta partial = ["hemisync"] the cause is left out and reported.
        let rows: Vec<DailyRow> = (0..40)
            .map(|i| {
                let hemi = i >= 10 && i % 3 == 0;
                let after = i >= 11 && (i - 1) % 3 == 0;
                DailyRow {
                    day: day(39 - i),
                    hemisync_min: hemi.then_some(20.0),
                    mood_mean: Some(if after { 4.5 } else { 3.0 } + (i % 4) as f64 * 0.05),
                    ..Default::default()
                }
            })
            .collect();
        let window = (today() - Duration::days(39), today());
        let run = |tag: &str, partial: &[String]| {
            let dir = super::super::local_store::tests::temp_dir(tag);
            super::super::daily_table::write_in(&dir, &rows, &[], partial, 0, window).unwrap();
            let r = generic_patterns_in(&dir, today()).unwrap();
            let _ = std::fs::remove_dir_all(dir);
            r
        };
        let ok = run("pgp1", &[]);
        assert!(ok.findings.iter().any(|f| f.cause == "hemisync_min"));
        assert!(ok.skipped_causes.is_empty());
        let part = run("pgp2", &["hemisync".to_string()]);
        assert!(part.findings.iter().all(|f| f.cause != "hemisync_min"));
        assert_eq!(part.skipped_causes, vec!["hemisync_min".to_string()]);
        assert!(
            part.tests_run < ok.tests_run,
            "the cause is not even tested"
        );
        // A partial source with no presence cause behind it skips nothing.
        let other = run("pgp3", &["checkins".to_string()]);
        assert!(other.skipped_causes.is_empty());
        assert_eq!(
            cause_of_partial_source("noop_workouts"),
            Some("workout_min")
        );
    }
}
