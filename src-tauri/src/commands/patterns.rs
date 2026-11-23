//! Health patterns (HANDOFF-1111a §2). Read-only `patterns_read`.
//!
//! Findings are computed from rows that already exist: NOOP daily metrics (sleep, HRV, strain) through the
//! sandboxed `noop_query` path, Muse session summaries (`sessions.rs`) and check-ins (`checkin.rs`).
//! Nothing is invented: every number in a finding comes from its with/without groups, and every finding
//! lists the source rows (`sourceIds`) behind it.
//!
//! Rules from the handoff, kept exactly:
//! - at least 3 days with and 3 days without the factor before a finding is even computed;
//! - confidence 1 to 5 from n and p; only confidence >= 2 is published (the rest is only counted, as `withheld`);
//! - the sentence is a template over the numbers (no LLM).
//!
//! Recompute weekly with no timer: the result is stored in `<data dir>/patterns/patterns.json` (0600, atomic
//! rename) and `patterns_read` recomputes only when the stored result is older than 7 days (or missing or
//! unreadable). A result computed while NOOP was unreachable is returned but not stored, so it never hides
//! the sleep findings for a week.

use super::local_store::{day_of, iso, now_jst, read_json, write_json_atomic};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

pub const WINDOW_DAYS: i64 = 28;
/// A stored result older than this is recomputed on the next read.
pub const MAX_AGE_DAYS: i64 = 7;
pub const MIN_PER_SIDE: usize = 3;
pub const MIN_CONFIDENCE: u8 = 2;
const MAX_BYTES: u64 = 512 * 1024;
pub const MAX_SOURCE_IDS: usize = 64;
const SLEEP_SPLIT_H: f64 = 7.0;
const EFFORT_SPLIT: f64 = 70.0;

// ───────────────────────────── inputs ─────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DayRow {
    pub day: String,
    pub sleep_h: Option<f64>,
    pub hrv: Option<f64>,
    pub strain: Option<f64>,
    pub recovery: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub day: String,
    pub focus: Option<f64>,
    pub calm: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MoodRow {
    pub id: String,
    pub day: String,
    pub mood: f64,
    pub energy: f64,
    pub tags: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub days: Vec<DayRow>,
    pub sessions: Vec<SessionRow>,
    pub moods: Vec<MoodRow>,
    /// True when a source could not be read (NOOP unreachable): the result must not be cached.
    pub partial: bool,
}

// ───────────────────────────── output ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_label: Option<String>,
    /// `[label, with, without]`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<(String, f64, f64)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<[String; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    /// "sleep_focus" | "effort_focus" | "hrv_calm" | "tag_mood"
    pub pair: String,
    pub title: String,
    pub lag_days: u8,
    pub window_days: i64,
    pub n_with: usize,
    pub n_without: usize,
    pub mean_with: f64,
    pub mean_without: f64,
    pub delta: f64,
    pub r: f64,
    pub p: f64,
    pub confidence: u8,
    pub strength: String,
    pub n: String,
    pub sentence: String,
    pub detail: String,
    pub chart: Chart,
    pub computed_at: String,
    /// Every row behind the numbers: `noop:<metric>:<day>`, `session:<id>`, `checkin:<id>`.
    pub source_ids: Vec<String>,
    /// For "tag_mood" only: the tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub sleep_days: usize,
    pub hrv_days: usize,
    pub strain_days: usize,
    pub sessions: usize,
    pub checkins: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PatternsResult {
    /// "ready" (at least one published finding) | "not_enough_data"
    pub state: String,
    pub computed_at: String,
    pub window_days: i64,
    pub findings: Vec<Finding>,
    /// Findings that were computed but sit under confidence 2. Counted, never shown.
    pub withheld: usize,
    pub counts: Counts,
    /// Plain-words reason when `state` is "not_enough_data".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ───────────────────────────── statistics ─────────────────────────────

pub(crate) fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

pub(crate) fn pearson(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() < 3 || xs.len() != ys.len() {
        return None;
    }
    let (mx, my) = (mean(xs), mean(ys));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return None;
    }
    Some((sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0))
}

fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation.
    const G: [f64; 6] = [
        76.18009172947146,
        -86.50532032941677,
        24.01409824083091,
        -1.231739572450155,
        0.1208650973866179e-2,
        -0.5395239384953e-5,
    ];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for g in G {
        y += 1.0;
        ser += g / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

fn betacf(a: f64, b: f64, x: f64) -> f64 {
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < 1e-30 {
        d = 1e-30;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..200 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 3e-12 {
            break;
        }
    }
    h
}

/// Regularised incomplete beta I_x(a, b).
fn inc_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        bt * betacf(a, b, x) / a
    } else {
        1.0 - bt * betacf(b, a, 1.0 - x) / b
    }
}

/// Two-sided p for Pearson r over n pairs (t test, n - 2 degrees of freedom).
pub(crate) fn p_value(r: f64, n: usize) -> f64 {
    if n < 3 {
        return 1.0;
    }
    let df = (n - 2) as f64;
    let r2 = r * r;
    if r2 >= 1.0 {
        return 0.0;
    }
    let t2 = r2 * df / (1.0 - r2);
    inc_beta(df / 2.0, 0.5, df / (df + t2)).clamp(0.0, 1.0)
}

/// 1 to 5 from n and p. Needs p under 0.05 to reach 2; n >= 10 on both sides adds one.
pub(crate) fn confidence(p: f64, n_with: usize, n_without: usize) -> u8 {
    let base: u8 = if p < 0.001 {
        4
    } else if p < 0.01 {
        3
    } else if p < 0.05 {
        2
    } else {
        return 1;
    };
    let bonus = u8::from(n_with.min(n_without) >= 10);
    (base + bonus).min(5)
}

pub(crate) fn strength(r: f64) -> &'static str {
    let a = r.abs();
    if a >= 0.7 {
        "Strong"
    } else if a >= 0.4 {
        "Medium"
    } else {
        "Weak"
    }
}

// ───────────────────────────── engine ─────────────────────────────

struct XPoint {
    x: f64,
    srcs: Vec<String>,
}
struct YPoint {
    y: f64,
    srcs: Vec<String>,
}

struct Stat {
    pairs: Vec<(String, f64, f64)>, // (x day, x, y)
    with: Vec<f64>,
    without: Vec<f64>,
    r: f64,
    p: f64,
    sources: Vec<String>,
}

fn add_days(day: &str, n: i64) -> Option<String> {
    let d = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some((d + Duration::days(n)).format("%Y-%m-%d").to_string())
}

/// Pairs each x day with the y of `day + lag`, splits at `threshold` (`x >= threshold` is "with").
/// None when either side has fewer than `MIN_PER_SIDE` days or the data has no spread.
fn analyse(
    xs: &BTreeMap<String, XPoint>,
    ys: &BTreeMap<String, YPoint>,
    lag: i64,
    threshold: f64,
) -> Option<Stat> {
    let mut pairs = Vec::new();
    let (mut with, mut without) = (Vec::new(), Vec::new());
    let mut sources = BTreeSet::new();
    for (day, xp) in xs {
        let Some(yd) = add_days(day, lag) else {
            continue;
        };
        let Some(yp) = ys.get(&yd) else {
            continue;
        };
        pairs.push((day.clone(), xp.x, yp.y));
        if xp.x >= threshold {
            with.push(yp.y);
        } else {
            without.push(yp.y);
        }
        sources.extend(xp.srcs.iter().cloned());
        sources.extend(yp.srcs.iter().cloned());
    }
    if with.len() < MIN_PER_SIDE || without.len() < MIN_PER_SIDE {
        return None;
    }
    let x: Vec<f64> = pairs.iter().map(|t| t.1).collect();
    let y: Vec<f64> = pairs.iter().map(|t| t.2).collect();
    let r = pearson(&x, &y)?;
    // The sentence describes with-vs-without means; the test must point the same way (sign of the
    // difference equals the sign of r), or the finding is dropped.
    if (mean(&with) - mean(&without)) * r <= 0.0 {
        return None;
    }
    // A finding whose rows cannot all be listed (64 at most) is not published.
    if sources.len() > MAX_SOURCE_IDS {
        return None;
    }
    let p = p_value(r, pairs.len());
    Some(Stat {
        pairs,
        with,
        without,
        r,
        p,
        sources: sources.into_iter().collect(),
    })
}

fn in_window(day: &str, today: NaiveDate, window: i64) -> bool {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d <= today && d > today - Duration::days(window))
        .unwrap_or(false)
}

fn f1(v: f64) -> String {
    format!("{v:.1}")
}
fn f2(v: f64) -> String {
    format!("{v:.2}")
}
fn signed1(v: f64) -> String {
    format!("{v:+.1}")
}

#[allow(clippy::too_many_arguments)]
fn build(
    id: String,
    pair: &str,
    title: String,
    lag: u8,
    window: i64,
    s: &Stat,
    sentence: String,
    detail_unit: &str,
    chart: Chart,
    tag: Option<String>,
    computed_at: &str,
) -> Finding {
    let (mw, mo) = (mean(&s.with), mean(&s.without));
    let delta = mw - mo;
    let conf = confidence(s.p, s.with.len(), s.without.len());
    let detail = format!(
        "That's **{} {}** ({} days vs {}). The link is {} for you (**r = {}**), but it's a pattern, not proof.",
        signed1(delta),
        detail_unit,
        s.with.len(),
        s.without.len(),
        strength(s.r).to_lowercase(),
        f2(s.r),
    );
    Finding {
        id,
        pair: pair.to_string(),
        title,
        lag_days: lag,
        window_days: window,
        n_with: s.with.len(),
        n_without: s.without.len(),
        mean_with: mw,
        mean_without: mo,
        delta,
        r: s.r,
        p: s.p,
        confidence: conf,
        strength: strength(s.r).to_string(),
        n: format!("{} days", s.pairs.len()),
        sentence,
        detail,
        chart,
        computed_at: computed_at.to_string(),
        source_ids: s.sources.clone(),
        tag,
    }
}

fn bars(label: &str, s: &Stat, legend: [&str; 2]) -> Chart {
    Chart {
        kind: "bars".into(),
        points: None,
        split: None,
        x: None,
        y: None,
        x_label: None,
        y_label: None,
        rows: Some(vec![(label.to_string(), mean(&s.with), mean(&s.without))]),
        legend: Some([legend[0].to_string(), legend[1].to_string()]),
    }
}

/// Everything `compute` needs, per day. Built either from per-row `Inputs` ([`prepare`]) or from daily-table
/// rows ([`prepare_daily`]); the engine after this point is the same code for both.
struct Prepared {
    days: Vec<DayRow>,
    focus: BTreeMap<String, YPoint>,
    calm: BTreeMap<String, YPoint>,
    /// Mean mood per day that has a check-in.
    mood_y: BTreeMap<String, YPoint>,
    /// Lower-cased tags per day that has a check-in.
    tag_days: BTreeMap<String, BTreeSet<String>>,
    counts: Counts,
}

fn prepare(inputs: &Inputs, today: NaiveDate) -> Prepared {
    let days: Vec<DayRow> = inputs
        .days
        .iter()
        .filter(|d| in_window(&d.day, today, WINDOW_DAYS))
        .cloned()
        .collect();
    let sessions: Vec<&SessionRow> = inputs
        .sessions
        .iter()
        .filter(|s| in_window(&s.day, today, WINDOW_DAYS))
        .collect();
    let moods: Vec<&MoodRow> = inputs
        .moods
        .iter()
        .filter(|m| in_window(&m.day, today, WINDOW_DAYS))
        .collect();

    let counts = Counts {
        sleep_days: days.iter().filter(|d| d.sleep_h.is_some()).count(),
        hrv_days: days.iter().filter(|d| d.hrv.is_some()).count(),
        strain_days: days.iter().filter(|d| d.strain.is_some()).count(),
        sessions: sessions.len(),
        checkins: moods.len(),
    };

    // Per-day y series (a day's sessions are averaged: one data point per day).
    let session_series = |pick: fn(&SessionRow) -> Option<f64>| {
        let mut acc: BTreeMap<String, (Vec<f64>, Vec<String>)> = BTreeMap::new();
        for s in &sessions {
            if let Some(v) = pick(s) {
                let e = acc.entry(s.day.clone()).or_default();
                e.0.push(v);
                e.1.push(format!("session:{}", s.id));
            }
        }
        acc.into_iter()
            .map(|(d, (v, srcs))| (d, YPoint { y: mean(&v), srcs }))
            .collect::<BTreeMap<_, _>>()
    };
    let focus = session_series(|s| s.focus);
    let calm = session_series(|s| s.calm);

    let mut mood_y: BTreeMap<String, YPoint> = BTreeMap::new();
    let mut tag_days: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    {
        let mut acc: BTreeMap<String, (Vec<f64>, Vec<String>)> = BTreeMap::new();
        for m in &moods {
            let e = acc.entry(m.day.clone()).or_default();
            e.0.push(m.mood);
            e.1.push(format!("checkin:{}", m.id));
            tag_days
                .entry(m.day.clone())
                .or_default()
                .extend(m.tags.iter().map(|t| t.to_lowercase()));
        }
        for (d, (v, srcs)) in acc {
            mood_y.insert(d, YPoint { y: mean(&v), srcs });
        }
    }
    Prepared {
        days,
        focus,
        calm,
        mood_y,
        tag_days,
        counts,
    }
}

/// The same inputs from daily-table rows. A day's focus/calm/mood is the row's own mean (the table averages
/// the same per-session / per-check-in values `prepare` averages) and its refs are the row's `sources`.
/// A cell with no refs is skipped: a number that cannot be traced is not used.
fn prepare_daily(rows: &[super::daily_table::DailyRow], today: NaiveDate) -> Prepared {
    let rows: Vec<&super::daily_table::DailyRow> = rows
        .iter()
        .filter(|r| in_window(&r.day, today, WINDOW_DAYS))
        .collect();
    let days = day_rows(rows.iter().copied());
    let refs = |r: &super::daily_table::DailyRow, col: &str| -> Vec<String> {
        r.sources.get(col).cloned().unwrap_or_default()
    };
    let series = |col: &str, pick: fn(&super::daily_table::DailyRow) -> Option<f64>| {
        rows.iter()
            .filter_map(|r| {
                let y = pick(r)?;
                let srcs = refs(r, col);
                (!srcs.is_empty()).then(|| (r.day.clone(), YPoint { y, srcs }))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let focus = series("focus_mean", |r| r.focus_mean);
    let calm = series("calm_mean", |r| r.calm_mean);
    let mood_y = series("mood_mean", |r| r.mood_mean);
    let tag_days: BTreeMap<String, BTreeSet<String>> = rows
        .iter()
        .filter(|r| mood_y.contains_key(&r.day))
        .map(|r| {
            (
                r.day.clone(),
                r.checkin_tags
                    .iter()
                    .flatten()
                    .map(|t| t.to_lowercase())
                    .collect(),
            )
        })
        .collect();
    let counts = Counts {
        sleep_days: days.iter().filter(|d| d.sleep_h.is_some()).count(),
        hrv_days: days.iter().filter(|d| d.hrv.is_some()).count(),
        strain_days: days.iter().filter(|d| d.strain.is_some()).count(),
        sessions: rows.iter().map(|r| refs(r, "muse_min").len()).sum(),
        checkins: rows.iter().map(|r| refs(r, "mood_mean").len()).sum(),
    };
    Prepared {
        days,
        focus,
        calm,
        mood_y,
        tag_days,
        counts,
    }
}

/// Daily-table rows to the NOOP-day shape (minutes to hours for sleep). Lossless for these four columns.
pub(crate) fn day_rows<'a>(
    rows: impl Iterator<Item = &'a super::daily_table::DailyRow>,
) -> Vec<DayRow> {
    rows.filter(|r| {
        r.sleep_total_min.is_some()
            || r.hrv_ms.is_some()
            || r.strain.is_some()
            || r.recovery.is_some()
    })
    .map(|r| DayRow {
        day: r.day.clone(),
        sleep_h: r.sleep_total_min.map(|m| m / 60.0),
        hrv: r.hrv_ms,
        strain: r.strain,
        recovery: r.recovery,
    })
    .collect()
}

/// Computes every finding the data supports. Pure: no clock, no disk.
pub fn compute(inputs: &Inputs, now: DateTime<FixedOffset>) -> PatternsResult {
    compute_prepared(prepare(inputs, day_of(&now)), now)
}

/// The same, from daily-table rows (see [`prepare_daily`]).
pub(crate) fn compute_daily(
    rows: &[super::daily_table::DailyRow],
    now: DateTime<FixedOffset>,
) -> PatternsResult {
    compute_prepared(prepare_daily(rows, day_of(&now)), now)
}

fn compute_prepared(p: Prepared, now: DateTime<FixedOffset>) -> PatternsResult {
    let computed_at = iso(&now);
    let Prepared {
        days,
        focus,
        calm,
        mood_y,
        tag_days,
        counts,
    } = p;
    let day_x = |pick: fn(&DayRow) -> Option<f64>, metric: &str| {
        days.iter()
            .filter_map(|d| {
                pick(d).map(|v| {
                    (
                        d.day.clone(),
                        XPoint {
                            x: v,
                            srcs: vec![format!("noop:{metric}:{}", d.day)],
                        },
                    )
                })
            })
            .collect::<BTreeMap<_, _>>()
    };

    let mut found: Vec<Finding> = Vec::new();

    // 1. Sleep on the night before -> that day's session focus (lag 0 from the wake day).
    let sleep = day_x(|d| d.sleep_h, "sleep_total_min");
    if let Some(s) = analyse(&sleep, &focus, 0, SLEEP_SPLIT_H) {
        let pts: Vec<[f64; 2]> = s.pairs.iter().map(|t| [t.1, t.2]).collect();
        let (xmin, xmax) = bounds(pts.iter().map(|p| p[0]));
        let (ymin, ymax) = bounds(pts.iter().map(|p| p[1]));
        let chart = Chart {
            kind: "scatter".into(),
            points: Some(pts),
            split: Some(SLEEP_SPLIT_H),
            x: Some([xmin.floor(), xmax.ceil()]),
            y: Some([(ymin / 5.0).floor() * 5.0, (ymax / 5.0).ceil() * 5.0]),
            x_label: Some("Sleep (h)".into()),
            y_label: Some("Next-session focus".into()),
            rows: None,
            legend: None,
        };
        let sentence = format!(
            "After 7 h+ of sleep, your next session's focus averages **{}**, vs **{}** after shorter nights.",
            f1(mean(&s.with)),
            f1(mean(&s.without))
        );
        found.push(build(
            "sleep_focus".into(),
            "sleep_focus",
            "Sleep → next Neuro session".into(),
            0,
            WINDOW_DAYS,
            &s,
            sentence,
            "points",
            chart,
            None,
            &computed_at,
        ));
    }

    // 2. Effort >= 70 -> next-day focus (lag 1).
    let strain = day_x(|d| d.strain, "strain");
    if let Some(s) = analyse(&strain, &focus, 1, EFFORT_SPLIT) {
        let sentence = format!(
            "The day after a hard workout (effort 70 or more), focus averages **{}**, vs **{}** on other days.",
            f1(mean(&s.with)),
            f1(mean(&s.without))
        );
        let chart = bars("Focus", &s, ["day after hard workout", "other days"]);
        found.push(build(
            "effort_focus".into(),
            "effort_focus",
            "Training load → focus".into(),
            1,
            WINDOW_DAYS,
            &s,
            sentence,
            "points",
            chart,
            None,
            &computed_at,
        ));
    }

    // 3. Morning HRV vs the calm of that day's sessions (split at your own window average).
    let hrv = day_x(|d| d.hrv, "hrv");
    if hrv.len() >= 2 * MIN_PER_SIDE {
        let usual = mean(&hrv.values().map(|p| p.x).collect::<Vec<_>>());
        if let Some(s) = analyse(&hrv, &calm, 0, usual) {
            let sentence = format!(
                "On days your morning HRV is above your usual **{}** ms, calm averages **{}**, vs **{}** on other days.",
                f1(usual),
                f1(mean(&s.with)),
                f1(mean(&s.without))
            );
            let chart = bars("Calm", &s, ["HRV above usual", "HRV at or below usual"]);
            found.push(build(
                "hrv_calm".into(),
                "hrv_calm",
                "HRV → calm".into(),
                0,
                WINDOW_DAYS,
                &s,
                sentence,
                "points",
                chart,
                None,
                &computed_at,
            ));
        }
    }

    // 4. Check-in tags -> mood that day. At most the two strongest tags.
    let all_tags: BTreeSet<String> = tag_days.values().flatten().cloned().collect();
    let mut tag_found: Vec<Finding> = Vec::new();
    let mut tag_stats: Vec<(String, Stat)> = Vec::new();
    for tag in all_tags {
        let mut xs: BTreeMap<String, XPoint> = BTreeMap::new();
        for d in mood_y.keys() {
            let has = tag_days.get(d).is_some_and(|t| t.contains(&tag));
            let srcs = mood_y.get(d).map(|y| y.srcs.clone()).unwrap_or_default();
            xs.insert(
                d.clone(),
                XPoint {
                    x: if has { 1.0 } else { 0.0 },
                    srcs,
                },
            );
        }
        if let Some(st) = analyse(&xs, &mood_y, 0, 0.5) {
            tag_stats.push((tag, st));
        }
    }
    // Bonferroni: every tag with enough days was a test, so p is multiplied by the number of tests.
    let tests = tag_stats.len() as f64;
    for (tag, mut s) in tag_stats {
        s.p = (s.p * tests).min(1.0);
        let sentence = format!(
            "On days you tag \"{tag}\", your mood averages **{}** of 5, vs **{}** on other days.",
            f1(mean(&s.with)),
            f1(mean(&s.without))
        );
        let chart = bars("Mood", &s, ["days tagged", "other days"]);
        tag_found.push(build(
            format!("tag_mood:{tag}"),
            "tag_mood",
            format!("\"{tag}\" → mood"),
            0,
            WINDOW_DAYS,
            &s,
            sentence,
            "mood points",
            chart,
            Some(tag.clone()),
            &computed_at,
        ));
    }
    tag_found.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then(b.r.abs().total_cmp(&a.r.abs()))
    });
    found.extend(tag_found);

    let total = found.len();
    let mut findings: Vec<Finding> = found
        .into_iter()
        .filter(|f| f.confidence >= MIN_CONFIDENCE)
        .collect();
    let withheld = total - findings.len();
    findings.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then(b.r.abs().total_cmp(&a.r.abs()))
    });
    // Keep at most two tag findings, after sorting.
    let mut tags_seen = 0;
    findings.retain(|f| {
        if f.pair == "tag_mood" {
            tags_seen += 1;
            tags_seen <= 2
        } else {
            true
        }
    });

    let (state, reason) = if findings.is_empty() {
        (
            "not_enough_data".to_string(),
            Some(format!(
                "Not enough data yet. Amara needs at least {MIN_PER_SIDE} days with and {MIN_PER_SIDE} days without a factor, and a link that holds up, before it says anything. Last {WINDOW_DAYS} days: {} nights of sleep, {} Neuro sessions, {} check-ins.",
                counts.sleep_days, counts.sessions, counts.checkins
            )),
        )
    } else {
        ("ready".to_string(), None)
    };
    PatternsResult {
        state,
        computed_at,
        window_days: WINDOW_DAYS,
        findings,
        withheld,
        counts,
        reason,
    }
}

fn bounds(it: impl Iterator<Item = f64>) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for v in it {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if hi - lo < 1.0 {
        hi = lo + 1.0;
    }
    (lo, hi)
}

// ───────────────────────────── store ─────────────────────────────

fn file(dir: &Path) -> PathBuf {
    dir.join("patterns").join("patterns.json")
}

/// A stored result is trusted only if it still passes the same checks a fresh one does; anything else is
/// treated as stale and recomputed (a hand-edited or damaged file never reaches the UI).
pub(crate) fn load_stored(dir: &Path) -> Option<PatternsResult> {
    read_json::<StoredWrap>(&file(dir), MAX_BYTES)
        .ok()
        .and_then(|w| w.0)
        .filter(valid_result)
}

const PAIRS: [&str; 4] = ["sleep_focus", "effort_focus", "hrv_calm", "tag_mood"];
const MAX_STR: usize = 600;

/// `^(noop:[a-z_]+:\d{4}-\d{2}-\d{2}|session:[\w-]+|checkin:[\w-]+)$`, ASCII only.
pub(crate) fn valid_source_id(s: &str) -> bool {
    let word = |t: &str| {
        !t.is_empty()
            && t.len() <= 64
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if let Some(rest) = s.strip_prefix("noop:") {
        let Some((metric, day)) = rest.split_once(':') else {
            return false;
        };
        let b = day.as_bytes();
        return !metric.is_empty()
            && metric.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            && b.len() == 10
            && b.iter().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    *c == b'-'
                } else {
                    c.is_ascii_digit()
                }
            });
    }
    if let Some(rest) = s.strip_prefix("session:") {
        return word(rest);
    }
    s.strip_prefix("checkin:").is_some_and(word)
}

pub(crate) fn valid_finding(f: &Finding) -> bool {
    let strs_ok = [
        &f.id,
        &f.pair,
        &f.title,
        &f.strength,
        &f.n,
        &f.sentence,
        &f.detail,
    ]
    .iter()
    .all(|t| t.len() <= MAX_STR)
        && f.tag.as_ref().is_none_or(|t| t.len() <= MAX_STR);
    strs_ok
        && PAIRS.contains(&f.pair.as_str())
        && (MIN_CONFIDENCE..=5).contains(&f.confidence)
        && !f.source_ids.is_empty()
        && f.source_ids.len() <= MAX_SOURCE_IDS
        && f.source_ids.iter().all(|s| valid_source_id(s))
        && f.r.is_finite()
        && f.r.abs() <= 1.0
        && f.p.is_finite()
        && (0.0..=1.0).contains(&f.p)
        && f.n_with >= MIN_PER_SIDE
        && f.n_without >= MIN_PER_SIDE
        && f.n_with + f.n_without >= 2 * MIN_PER_SIDE
        && f.mean_with.is_finite()
        && f.mean_without.is_finite()
        && (f.delta - (f.mean_with - f.mean_without)).abs() <= 1e-6
        && f.delta * f.r > 0.0
}

pub(crate) fn valid_result(r: &PatternsResult) -> bool {
    (r.state == "ready" && !r.findings.is_empty()
        || r.state == "not_enough_data" && r.findings.is_empty())
        && r.findings.len() <= 6
        && r.findings.iter().all(valid_finding)
        && r.reason.as_ref().is_none_or(|t| t.len() <= MAX_STR)
}

/// `read_json` needs `Default`; this wraps the stored result.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct StoredWrap(Option<PatternsResult>);

fn is_fresh(r: &PatternsResult, now: DateTime<FixedOffset>) -> bool {
    match DateTime::parse_from_rfc3339(&r.computed_at) {
        Ok(t) => {
            let age = now.signed_duration_since(t);
            age >= Duration::zero() && age < Duration::days(MAX_AGE_DAYS)
        }
        Err(_) => false,
    }
}

static READ_LOCK: Mutex<()> = Mutex::new(());

/// What a recompute reads: per-row inputs (live gatherers) or daily-table rows.
pub(crate) enum Source {
    Live(Inputs),
    Daily(Vec<super::daily_table::DailyRow>),
}

/// Stored result when fresh; otherwise gather, compute and (unless a source was missing) store.
pub(crate) fn read_in(
    dir: &Path,
    now: DateTime<FixedOffset>,
    gather: impl FnOnce() -> Inputs,
) -> Result<PatternsResult, String> {
    read_in_src(dir, now, || Source::Live(gather()))
}

pub(crate) fn read_in_src(
    dir: &Path,
    now: DateTime<FixedOffset>,
    gather: impl FnOnce() -> Source,
) -> Result<PatternsResult, String> {
    // Single flight: concurrent reads wait here, so one stale file triggers one gather, not several.
    let _g = READ_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(r) = load_stored(dir) {
        if is_fresh(&r, now) {
            return Ok(r);
        }
    }
    let (result, partial) = match gather() {
        Source::Live(inputs) => (compute(&inputs, now), inputs.partial),
        // A table is only used when it is complete (see `table_usable`), so it is never partial.
        Source::Daily(rows) => (compute_daily(&rows, now), false),
    };
    if !partial {
        write_json_atomic(&file(dir), &StoredWrap(Some(result.clone())), MAX_BYTES)?;
    }
    Ok(result)
}

// ───────────────────────────── gathering ─────────────────────────────

/// Parses a `metric_series` payload into (day, value) pairs.
pub(crate) fn series_points(v: &Value) -> Vec<(String, f64)> {
    v.get("points")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let day = p.get("day")?.as_str()?;
                    let val = p.get("value")?.as_f64()?;
                    (val.is_finite() && day.len() == 10).then(|| (day.to_string(), val))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Merges per-metric series into day rows.
pub(crate) fn rows_from_series(series: &[(&str, Vec<(String, f64)>)]) -> Vec<DayRow> {
    let mut map: BTreeMap<String, DayRow> = BTreeMap::new();
    for (key, pts) in series {
        for (day, v) in pts {
            let row = map.entry(day.clone()).or_insert_with(|| DayRow {
                day: day.clone(),
                ..Default::default()
            });
            match *key {
                "sleep_total_min" => row.sleep_h = Some(v / 60.0),
                "hrv" => row.hrv = Some(*v),
                "strain" => row.strain = Some(*v),
                "recovery" => row.recovery = Some(*v),
                _ => {}
            }
        }
    }
    map.into_values().collect()
}

/// Four `metric_series` queries through the sandboxed, no-network NOOP path. `Err` when NOOP is unreachable.
pub(crate) fn fetch_noop_days(days: i64) -> Result<Vec<DayRow>, String> {
    fetch_noop_series(days).map(|series| rows_from_series(&series))
}

/// The raw per-metric series (minutes for sleep, no unit change), same sandboxed path and backoff.
pub(crate) fn fetch_noop_series(days: i64) -> Result<NoopSeries, String> {
    let now = Instant::now();
    if NOOP_BACKOFF.blocked(now, NOOP_BACKOFF_TTL) {
        return Err("noop unavailable".to_string());
    }
    let r = fetch_noop_series_uncached(days);
    match &r {
        Ok(_) => NOOP_BACKOFF.clear(),
        Err(_) => NOOP_BACKOFF.mark(now),
    }
    r
}

pub(crate) type NoopSeries = Vec<(&'static str, Vec<(String, f64)>)>;

/// Remembers when NOOP last failed, so the pages don't spawn the CLI again for 5 minutes.
pub(crate) struct Backoff(Mutex<Option<Instant>>);
pub(crate) const NOOP_BACKOFF_TTL: StdDuration = StdDuration::from_secs(300);
pub(crate) static NOOP_BACKOFF: Backoff = Backoff(Mutex::new(None));
impl Backoff {
    pub(crate) fn blocked(&self, now: Instant, ttl: StdDuration) -> bool {
        let g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        g.is_some_and(|t| now.saturating_duration_since(t) < ttl)
    }
    pub(crate) fn mark(&self, now: Instant) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(now);
    }
    pub(crate) fn clear(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

fn fetch_noop_series_uncached(days: i64) -> Result<NoopSeries, String> {
    use super::noop_query::{load_settings, run_query, with_resolved_db, Roots, DEFAULT_TIMEOUT};
    let settings = with_resolved_db(load_settings());
    let roots = Roots::current()?;
    let mut series = Vec::new();
    for key in ["sleep_total_min", "hrv", "strain", "recovery"] {
        let v = run_query(
            &settings,
            &roots,
            "metric_series",
            &json!({ "key": key, "days": days }),
            DEFAULT_TIMEOUT,
        )
        .map_err(|_| "noop unavailable".to_string())?;
        series.push((key, series_points(&v)));
    }
    Ok(series)
}

pub(crate) fn session_rows(list: &[super::sessions::RecordedSession]) -> Vec<SessionRow> {
    let jst = super::local_store::jst();
    list.iter()
        .filter_map(|s| {
            let t = DateTime::parse_from_rfc3339(&s.start).ok()?;
            Some(SessionRow {
                id: s.id.clone(),
                day: t
                    .with_timezone(&jst)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
                focus: s.summary.avg_focus,
                // calm_pct only: sessions without it are excluded, never mixed with avg_relaxation.
                calm: s.summary.calm_pct,
            })
        })
        .collect()
}

pub(crate) fn mood_rows(list: &[super::checkin::CheckInRecord]) -> Vec<MoodRow> {
    list.iter()
        .map(|c| MoodRow {
            id: c.id.clone(),
            day: c.day.clone(),
            mood: f64::from(c.mood),
            energy: f64::from(c.energy),
            tags: c.tags.clone(),
            text: c.text.clone(),
        })
        .collect()
}

pub(crate) const MAX_SESSION_FILES: usize = 400;
pub(crate) const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;

/// The newest `max_files` session files (by modified time), each at most `max_bytes`, regular files only.
/// Read-only. Anything bigger or unreadable is skipped, never followed (no symlinks).
pub(crate) fn read_sessions_capped(
    dir: &Path,
    max_files: usize,
    max_bytes: u64,
) -> Vec<super::sessions::RecordedSession> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let m = std::fs::symlink_metadata(&path).ok()?;
            (m.file_type().is_file() && m.len() <= max_bytes)
                .then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), path))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(max_files);
    files
        .into_iter()
        .filter_map(|(_, path)| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            let mut s: super::sessions::RecordedSession =
                serde_json::from_value(v.get("session")?.clone()).ok()?;
            s.summary.normalize_legacy_scores();
            Some(s)
        })
        .collect()
}

/// A stored daily table is used only when it was built under this long ago (the nightly slot rebuilds it
/// every day, so a healthy table is under 26 h old; 36 h allows one missed night).
const TABLE_MAX_AGE_MS: i64 = 36 * 3600 * 1000;

/// Pure: whether a stored build may stand in for the live gatherers. Needs a build, a recent one, and no
/// failed source that patterns reads (NOOP, check-ins); otherwise the live gatherers run, as before.
pub(crate) fn table_usable(built_at_ms: Option<i64>, partial: &[String], now_ms: i64) -> bool {
    built_at_ms.is_some_and(|t| t > 0 && now_ms - t >= 0 && now_ms - t < TABLE_MAX_AGE_MS)
        && !partial
            .iter()
            .any(|p| p == "noop" || p == "checkins" || p == "hemisync")
}

/// Daily-table rows for the window ending today, or None when the table cannot stand in for the live read.
pub(crate) fn daily_rows_in(
    dir: &Path,
    now: DateTime<FixedOffset>,
    window_days: i64,
) -> Option<Vec<super::daily_table::DailyRow>> {
    let built = super::daily_table::built_at_ms_in(dir);
    let partial = super::daily_table::partial_in(dir)?;
    if !table_usable(built, &partial, now.timestamp_millis()) {
        return None;
    }
    let to = day_of(&now);
    let from = to - Duration::days(window_days + 1);
    let fmt = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let r = super::daily_table::read_range_in(dir, &fmt(from), &fmt(to)).ok()?;
    // The table is built from the day the build ran; a build older than the window's newest day can
    // still be right (36 h rule above), so no extra freshness test here.
    Some(r.rows)
}

/// Reads sessions and check-ins from app data and NOOP from the sandboxed path. Never fails: a missing
/// source marks the inputs partial.
///
/// NOOP days come from the daily table when it is usable ([`daily_rows_in`]); sessions and check-ins are
/// always read directly because the weekly review needs per-session counts, per-check-in means and the
/// check-in text, none of which a per-day row keeps.
pub(crate) fn gather_live(window_days: i64) -> Inputs {
    let data = crate::app_paths::data_dir();
    let mut inputs = Inputs::default();
    match daily_rows_in(&data, now_jst(), window_days) {
        Some(rows) => inputs.days = day_rows(rows.iter()),
        None => match fetch_noop_days(window_days + 2) {
            Ok(d) => inputs.days = d,
            Err(_) => inputs.partial = true,
        },
    }
    inputs.sessions = session_rows(&read_sessions_capped(
        &super::sessions::sessions_dir(),
        MAX_SESSION_FILES,
        MAX_SESSION_BYTES,
    ));
    match super::checkin::list_in(&super::checkin::dir_in(&data), 365) {
        Ok(l) => inputs.moods = mood_rows(&l),
        Err(_) => inputs.partial = true,
    }
    inputs
}

/// For `patterns_read`: the daily table when usable, else the live gatherers.
pub(crate) fn gather_source(window_days: i64) -> Source {
    let data = crate::app_paths::data_dir();
    match daily_rows_in(&data, now_jst(), window_days) {
        Some(rows) => Source::Daily(rows),
        None => Source::Live(gather_live(window_days)),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn patterns_read() -> Result<PatternsResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        read_in_src(&crate::app_paths::data_dir(), now_jst(), || {
            gather_source(WINDOW_DAYS)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use chrono::TimeZone;

    pub(crate) fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<FixedOffset> {
        super::super::local_store::jst()
            .with_ymd_and_hms(y, m, d, h, 0, 0)
            .unwrap()
    }

    fn day(n: i64) -> String {
        (NaiveDate::from_ymd_opt(2026, 11, 12).unwrap() - Duration::days(n))
            .format("%Y-%m-%d")
            .to_string()
    }

    /// 16 days. Long sleep (>= 7 h) goes with high focus (+ small deterministic jitter).
    pub(crate) fn enough() -> Inputs {
        let mut i = Inputs::default();
        for n in 0..16i64 {
            let long = n % 2 == 0;
            let sleep = if long {
                7.4 + (n % 3) as f64 * 0.2
            } else {
                6.0 + (n % 3) as f64 * 0.2
            };
            i.days.push(DayRow {
                day: day(n),
                sleep_h: Some(sleep),
                hrv: Some(55.0 + (n % 4) as f64),
                strain: Some(40.0),
                recovery: Some(60.0),
            });
            i.sessions.push(SessionRow {
                id: format!("s{n}"),
                day: day(n),
                focus: Some(if long { 64.0 } else { 55.0 } + (n % 3) as f64),
                calm: Some(60.0 + (n % 5) as f64),
            });
        }
        i
    }

    #[test]
    fn enough_data_publishes_a_traceable_sleep_finding() {
        let r = compute(&enough(), at(2026, 11, 12, 10));
        assert_eq!(r.state, "ready");
        let f = r.findings.iter().find(|f| f.pair == "sleep_focus").unwrap();
        assert!(f.confidence >= MIN_CONFIDENCE);
        assert_eq!(f.n_with + f.n_without, 16);
        assert!(f.delta > 5.0 && f.r > 0.8);
        // Every number in the sentence is the row's own number.
        assert!(f.sentence.contains(&format!("**{}**", f1(f.mean_with))));
        assert!(f.sentence.contains(&format!("**{}**", f1(f.mean_without))));
        assert!(f.detail.contains(&signed1(f.delta)));
        assert!(f.detail.contains(&f2(f.r)));
        // Every source id is a real input row, and every scatter point has one.
        let session_ids: BTreeSet<String> = enough()
            .sessions
            .iter()
            .map(|s| format!("session:{}", s.id))
            .collect();
        let sleep_ids: BTreeSet<String> = enough()
            .days
            .iter()
            .map(|d| format!("noop:sleep_total_min:{}", d.day))
            .collect();
        for id in &f.source_ids {
            assert!(session_ids.contains(id) || sleep_ids.contains(id), "{id}");
        }
        assert_eq!(f.source_ids.len(), 32);
        assert_eq!(f.chart.points.as_ref().unwrap().len(), 16);
    }

    #[test]
    fn every_number_in_every_sentence_traces_to_the_row() {
        let r = compute(&enough(), at(2026, 11, 12, 10));
        for f in &r.findings {
            let allowed = [
                f1(f.mean_with),
                f1(f.mean_without),
                signed1(f.delta),
                f2(f.r),
                f.n_with.to_string(),
                f.n_without.to_string(),
            ];
            let text = format!("{} {}", f.sentence, f.detail);
            let mut cur = String::new();
            let mut nums = Vec::new();
            for c in text.chars().chain(std::iter::once(' ')) {
                if c.is_ascii_digit()
                    || (c == '.' && !cur.is_empty())
                    || ((c == '+' || c == '-') && cur.is_empty())
                {
                    cur.push(c);
                } else {
                    if cur.chars().any(|d| d.is_ascii_digit()) {
                        nums.push(cur.trim_end_matches('.').to_string());
                    }
                    cur.clear();
                }
            }
            for n in nums {
                // "7 h+", "70" (the fixed split rules), and the HRV "usual" are template constants or the row's own HRV mean.
                let ok = allowed.contains(&n)
                    || n == "7"
                    || n == "70"
                    || n == "5"
                    || (f.pair == "hrv_calm");
                assert!(ok, "number {n} in {text:?} is not in the row");
            }
        }
    }

    #[test]
    fn no_data_is_an_honest_empty_state() {
        let r = compute(&Inputs::default(), at(2026, 11, 12, 10));
        assert_eq!(r.state, "not_enough_data");
        assert!(r.findings.is_empty());
        assert!(r
            .reason
            .as_deref()
            .unwrap()
            .starts_with("Not enough data yet"));
    }

    #[test]
    fn fewer_than_three_days_each_side_makes_no_finding() {
        let mut i = enough();
        i.days.truncate(5);
        i.sessions.truncate(5);
        // 5 days: at most 3 long + 2 short => the short side has 2.
        let r = compute(&i, at(2026, 11, 12, 10));
        assert!(r.findings.iter().all(|f| f.pair != "sleep_focus"));
        assert_eq!(r.state, "not_enough_data");
    }

    #[test]
    fn confidence_under_two_is_hidden_but_counted() {
        // Same sleep split, focus unrelated to sleep: p is large, confidence 1.
        let mut i = enough();
        for (n, s) in i.sessions.iter_mut().enumerate() {
            s.focus = Some(if (n / 2) % 2 == 0 { 60.0 } else { 58.0 } + (n % 5) as f64);
        }
        let r = compute(&i, at(2026, 11, 12, 10));
        assert!(r.findings.iter().all(|f| f.pair != "sleep_focus"));
        assert!(r.withheld >= 1);
        assert!(r.findings.iter().all(|f| f.confidence >= 2));
        assert_eq!(confidence(0.2, 20, 20), 1);
        assert_eq!(confidence(0.04, 5, 5), 2);
        assert_eq!(confidence(0.04, 12, 12), 3);
        assert_eq!(confidence(0.0001, 12, 12), 5);
    }

    #[test]
    fn data_older_than_the_window_is_ignored() {
        let mut i = enough();
        for d in &mut i.days {
            d.day = add_days(&d.day, -60).unwrap();
        }
        for s in &mut i.sessions {
            s.day = add_days(&s.day, -60).unwrap();
        }
        assert_eq!(compute(&i, at(2026, 11, 12, 10)).state, "not_enough_data");
    }

    #[test]
    fn p_value_matches_known_values() {
        // r = 0.5, n = 10 -> p about 0.141; r = 0.8, n = 12 -> p about 0.0017.
        assert!((p_value(0.5, 10) - 0.141).abs() < 0.005);
        assert!((p_value(0.8, 12) - 0.0017).abs() < 0.0005);
        assert_eq!(p_value(1.0, 10), 0.0);
    }

    #[test]
    fn tag_mood_needs_three_days_each_side() {
        let mut i = Inputs::default();
        for n in 0..10i64 {
            let tagged = n % 2 == 0;
            i.moods.push(MoodRow {
                id: format!("c{n}"),
                day: day(n),
                mood: if tagged { 4.0 } else { 2.0 },
                energy: 3.0,
                tags: if tagged {
                    vec!["Training".into()]
                } else {
                    vec![]
                },
                text: String::new(),
            });
        }
        // Perfect separation has no spread inside a group but r is finite: expect a finding.
        let r = compute(&i, at(2026, 11, 12, 10));
        let f = r.findings.iter().find(|f| f.pair == "tag_mood").unwrap();
        assert_eq!(f.tag.as_deref(), Some("training"));
        assert_eq!((f.n_with, f.n_without), (5, 5));
        assert!(f.source_ids.iter().all(|s| s.starts_with("checkin:")));
    }

    #[test]
    fn stale_result_is_recomputed_fresh_one_is_not() {
        let dir = temp_dir("pat");
        let t0 = at(2026, 11, 5, 10);
        let calls = std::cell::Cell::new(0);
        let gather = |inp: Inputs| {
            calls.set(calls.get() + 1);
            inp
        };
        let first = read_in(&dir, t0, || gather(enough())).unwrap();
        assert_eq!(calls.get(), 1);
        assert!(file(&dir).exists());
        // 6 days later: still fresh, gather not called, identical result.
        let again = read_in(&dir, at(2026, 11, 11, 10), || gather(Inputs::default())).unwrap();
        assert_eq!(calls.get(), 1);
        // What was stored is what comes back (JSON may differ from the in-memory first result by 1 ulp).
        assert_eq!(Some(again.clone()), load_stored(&dir));
        assert_eq!(again.findings.len(), first.findings.len());
        assert_eq!(again.computed_at, first.computed_at);
        // 8 days later: stale, recomputed from the new (empty) inputs.
        let stale = read_in(&dir, at(2026, 11, 13, 10), || gather(Inputs::default())).unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(stale.state, "not_enough_data");
        assert_eq!(stale.computed_at, iso(&at(2026, 11, 13, 10)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_inputs_are_not_cached() {
        let dir = temp_dir("patp");
        let i = Inputs {
            partial: true,
            ..Default::default()
        };
        let r = read_in(&dir, at(2026, 11, 12, 10), || i).unwrap();
        assert_eq!(r.state, "not_enough_data");
        assert!(!file(&dir).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[cfg(unix)]
    fn stored_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("pat6");
        read_in(&dir, at(2026, 11, 12, 10), enough).unwrap();
        let mode = std::fs::metadata(file(&dir)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_store_recomputes_instead_of_failing() {
        let dir = temp_dir("patc");
        std::fs::create_dir_all(dir.join("patterns")).unwrap();
        std::fs::write(file(&dir), "not json").unwrap();
        let r = read_in(&dir, at(2026, 11, 12, 10), enough).unwrap();
        assert_eq!(r.state, "ready");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn series_parsing_and_session_day_in_jst() {
        let v = json!({"points": [{"day":"2026-11-01","value":420.0},{"day":"2026-11-02","value":null},{"day":"bad","value":1.0}]});
        assert_eq!(series_points(&v), vec![("2026-11-01".to_string(), 420.0)]);
        let rows = rows_from_series(&[("sleep_total_min", series_points(&v))]);
        assert_eq!(rows[0].sleep_h, Some(7.0));
    }

    fn mood(id: &str, n: i64, m: f64, tags: &[&str]) -> MoodRow {
        MoodRow {
            id: id.into(),
            day: day(n),
            mood: m,
            energy: 3.0,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            text: String::new(),
        }
    }

    fn tag_inputs(tags: &[&str]) -> Inputs {
        let tagged = [4.0, 3.0, 4.0, 3.0, 4.0, 4.0, 3.0, 4.0];
        let other = [3.0, 3.0, 2.0, 4.0, 3.0, 2.0, 3.0, 3.0];
        let mut i = Inputs::default();
        for n in 0..16i64 {
            let t = n % 2 == 0;
            let v = if t {
                tagged[(n / 2) as usize]
            } else {
                other[(n / 2) as usize]
            };
            i.moods
                .push(mood(&format!("c{n}"), n, v, if t { tags } else { &[] }));
        }
        i
    }

    #[test]
    fn tag_findings_are_bonferroni_corrected_over_the_tags_tested() {
        let now = at(2026, 11, 12, 10);
        let one = compute(&tag_inputs(&["a"]), now);
        let f1 = one
            .findings
            .iter()
            .find(|f| f.pair == "tag_mood")
            .expect("single tag");
        assert!(f1.p < 0.05, "precondition: p {} alone is significant", f1.p);
        // Same days under four tags: four tests, p is multiplied by 4 and must no longer pass.
        let four = compute(&tag_inputs(&["a", "b", "c", "d"]), now);
        assert!(f1.p * 4.0 >= 0.05, "precondition: p*4 = {}", f1.p * 4.0);
        assert!(four.findings.iter().all(|f| f.pair != "tag_mood"));
        assert!(four.withheld >= 4);
    }

    #[test]
    fn a_finding_whose_means_point_against_r_is_dropped() {
        let (xs, ys): (BTreeMap<_, _>, BTreeMap<_, _>) = {
            let xv = [1.0, 2.0, 3.0, 7.0, 8.0, 9.0];
            let yv = [10.0, 20.0, 30.0, 6.0, 16.0, 26.0];
            let mut xs = BTreeMap::new();
            let mut ys = BTreeMap::new();
            for (i, (x, y)) in xv.iter().zip(yv).enumerate() {
                let d = day(i as i64);
                xs.insert(
                    d.clone(),
                    XPoint {
                        x: *x,
                        srcs: vec![format!("noop:hrv:{d}")],
                    },
                );
                ys.insert(
                    d.clone(),
                    YPoint {
                        y,
                        srcs: vec![format!("session:s{i}")],
                    },
                );
            }
            (xs, ys)
        };
        let x: Vec<f64> = xs.values().map(|p| p.x).collect();
        let y: Vec<f64> = ys.values().map(|p| p.y).collect();
        // Days are keyed in date order, so rebuild the series in that order for the precondition.
        let r = pearson(&x, &y).unwrap();
        assert!(r > 0.0, "precondition: r points up");
        let (w, o): (Vec<f64>, Vec<f64>) = (vec![6.0, 16.0, 26.0], vec![10.0, 20.0, 30.0]);
        assert!(
            mean(&w) < mean(&o),
            "precondition: the with-group mean is lower"
        );
        assert!(analyse(&xs, &ys, 0, 5.0).is_none());
    }

    #[test]
    fn all_of_a_days_checkin_ids_are_sources() {
        let mut i = Inputs::default();
        for n in 0..16i64 {
            let t = n % 2 == 0;
            let v = if t { 4.0 } else { 2.0 } + (n % 3) as f64 * 0.1;
            i.moods
                .push(mood(&format!("a{n}"), n, v, if t { &["x"] } else { &[] }));
            i.moods.push(mood(&format!("b{n}"), n, v, &[]));
        }
        let r = compute(&i, at(2026, 11, 12, 10));
        let f = r.findings.iter().find(|f| f.pair == "tag_mood").unwrap();
        for n in 0..16i64 {
            assert!(f.source_ids.contains(&format!("checkin:a{n}")));
            assert!(
                f.source_ids.contains(&format!("checkin:b{n}")),
                "second check-in of day {n}"
            );
        }
    }

    #[test]
    fn calm_uses_calm_pct_only() {
        let s: super::super::sessions::RecordedSession = serde_json::from_value(json!({
            "id": "x", "device_type": "muse", "start": "2026-11-11T10:00:00Z", "end": "2026-11-11T10:10:00Z",
            "duration_secs": 600, "data_points": 1, "file_path": "a.ndjson",
            "summary": { "avg_focus": 60.0, "avg_relaxation": 80.0, "avg_hr": null, "max_hr": null, "avg_hrv": null, "score_scale": 100 }
        })).unwrap();
        let rows = session_rows(std::slice::from_ref(&s));
        assert_eq!(rows[0].calm, None);
        assert_eq!(rows[0].focus, Some(60.0));
        let mut with_calm = s;
        with_calm.summary.calm_pct = Some(70.0);
        assert_eq!(session_rows(&[with_calm])[0].calm, Some(70.0));
    }

    #[test]
    fn a_finding_that_cannot_list_all_its_rows_is_not_published() {
        let mut xs = BTreeMap::new();
        let mut ys = BTreeMap::new();
        for i in 0..28i64 {
            let d = day(i);
            let long = i % 2 == 0;
            xs.insert(
                d.clone(),
                XPoint {
                    x: if long { 8.0 } else { 6.0 } + (i % 3) as f64 * 0.1,
                    srcs: vec![format!("noop:sleep_total_min:{d}")],
                },
            );
            ys.insert(
                d,
                YPoint {
                    y: if long { 64.0 } else { 55.0 } + (i % 3) as f64,
                    srcs: (0..3).map(|k| format!("session:s{i}-{k}")).collect(),
                },
            );
        }
        assert!(analyse(&xs, &ys, 0, 7.0).is_none());
    }

    #[test]
    fn a_tampered_or_invalid_cache_is_recomputed() {
        let dir = temp_dir("patt");
        let now = at(2026, 11, 12, 10);
        let good = read_in(&dir, now, enough).unwrap();
        assert!(valid_result(&good));
        let tamper = |edit: &dyn Fn(&mut PatternsResult)| {
            let mut r = good.clone();
            edit(&mut r);
            write_json_atomic(&file(&dir), &StoredWrap(Some(r)), MAX_BYTES).unwrap();
            let calls = std::cell::Cell::new(0);
            let out = read_in(&dir, now, || {
                calls.set(calls.get() + 1);
                enough()
            })
            .unwrap();
            assert_eq!(calls.get(), 1, "an invalid cache must be recomputed");
            assert!(valid_result(&out));
        };
        tamper(&|r| r.findings[0].confidence = 1);
        tamper(&|r| r.findings[0].source_ids = vec![]);
        tamper(&|r| r.findings[0].source_ids[0] = "../etc/passwd".into());
        tamper(&|r| r.findings[0].r = 1.5);
        tamper(&|r| r.findings[0].p = -0.1);
        tamper(&|r| {
            r.findings[0].n_with = 1;
            r.findings[0].n_without = 1;
        });
        tamper(&|r| r.findings[0].pair = "made_up".into());
        tamper(&|r| r.findings[0].delta += 1.0);
        tamper(&|r| r.findings[0].sentence = "x".repeat(601));
        tamper(&|r| {
            let f = r.findings[0].clone();
            r.findings = vec![f; 7];
        });
        tamper(&|r| r.state = "not_enough_data".into());
        // The untouched cache is reused.
        write_json_atomic(&file(&dir), &StoredWrap(Some(good.clone())), MAX_BYTES).unwrap();
        let reused = read_in(&dir, now, || {
            panic!("fresh valid cache must not be recomputed")
        })
        .unwrap();
        assert_eq!(reused.findings.len(), good.findings.len());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_id_shapes() {
        for ok in [
            "noop:sleep_total_min:2026-11-01",
            "session:ab-12_x",
            "checkin:c-0a1",
        ] {
            assert!(valid_source_id(ok), "{ok}");
        }
        for bad in [
            "",
            "noop:Sleep:2026-11-01",
            "noop:hrv:2026-1-01",
            "noop:hrv",
            "session:",
            "session:a/b",
            "checkin:a b",
            "file:x",
            "session:x\n",
        ] {
            assert!(!valid_source_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn noop_failure_is_remembered_for_five_minutes() {
        let b = Backoff(Mutex::new(None));
        let t = Instant::now();
        assert!(!b.blocked(t, NOOP_BACKOFF_TTL));
        b.mark(t);
        assert!(b.blocked(t + StdDuration::from_secs(299), NOOP_BACKOFF_TTL));
        assert!(!b.blocked(t + StdDuration::from_secs(301), NOOP_BACKOFF_TTL));
        b.mark(t);
        b.clear();
        assert!(!b.blocked(t, NOOP_BACKOFF_TTL));
    }

    #[test]
    fn session_reading_is_capped_by_count_and_size_and_refuses_symlinks() {
        let dir = temp_dir("patsess");
        let mk = |id: &str, pad: usize| {
            let v = json!({"session": {"id": id, "device_type": "muse", "start": "2026-11-11T10:00:00Z", "end": "2026-11-11T10:10:00Z",
                "duration_secs": 600, "data_points": 1, "file_path": "a.ndjson",
                "summary": {"avg_focus": 60.0, "avg_relaxation": 50.0, "avg_hr": null, "max_hr": null, "avg_hrv": null, "score_scale": 100}},
                "data": {"pad": "x".repeat(pad)}});
            std::fs::write(dir.join(format!("{id}.json")), v.to_string()).unwrap();
        };
        for i in 0..5 {
            mk(&format!("s{i}"), 10);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        mk("big", 5000);
        assert_eq!(
            read_sessions_capped(&dir, 100, 4000).len(),
            5,
            "oversize file skipped"
        );
        assert_eq!(read_sessions_capped(&dir, 3, 4000).len(), 3, "count capped");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("s0.json"), dir.join("link.json")).unwrap();
            assert_eq!(
                read_sessions_capped(&dir, 100, 4000).len(),
                5,
                "symlink not followed"
            );
        }
        assert!(read_sessions_capped(&dir.join("missing"), 3, 4000).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    // ───────── A2: daily-table path equals the per-row path ─────────

    use super::super::daily_table as dt;

    /// The same synthetic world as both source rows (`dt::Inputs`, which `dt::build` turns into table rows)
    /// and patterns `Inputs` (what the old gatherers produced). 21 days, 1 to 3 Muse sessions and 1 to 2
    /// check-ins a day, some sessions with no focus or no calm, tags in mixed case.
    pub(crate) fn world() -> (dt::Inputs, Inputs) {
        let mut src = dt::Inputs::default();
        let mut old = Inputs::default();
        for n in 0..21i64 {
            let d = day(n);
            let long = n % 2 == 0;
            let sleep_min = if long { 440.0 } else { 365.0 } + (n % 3) as f64 * 12.0;
            let hrv = 52.0 + (n % 5) as f64 * 1.5;
            let strain = if n % 4 == 0 { 78.0 } else { 45.0 } + (n % 3) as f64;
            let recovery = 55.0 + (n % 7) as f64;
            src.noop.push(dt::NoopDay {
                day: d.clone(),
                sleep_min: Some(sleep_min),
                hrv: Some(hrv),
                recovery: Some(recovery),
                strain: Some(strain),
            });
            old.days.push(DayRow {
                day: d.clone(),
                sleep_h: Some(sleep_min / 60.0),
                hrv: Some(hrv),
                strain: Some(strain),
                recovery: Some(recovery),
            });
            for k in 0..(1 + n % 3) {
                let focus = (k != 1 || n % 5 != 0)
                    .then_some(if long { 63.0 } else { 54.0 } + (n % 3) as f64 + k as f64 * 0.7);
                let calm = (k != 2).then(|| 58.0 + ((n + k) % 6) as f64 * 1.3);
                let id = format!("s{n}k{k}");
                src.muse.push(dt::MuseIn {
                    id: id.clone(),
                    start: format!("{d}T{:02}:30:00+09:00", 8 + k),
                    duration_secs: 600,
                    avg_focus: focus,
                    calm_pct: calm,
                });
                old.sessions.push(SessionRow {
                    id,
                    day: d.clone(),
                    focus,
                    calm,
                });
            }
            for k in 0..(1 + n % 2) {
                let walk = n % 2 == 0 && k == 0;
                let tags: Vec<String> = if walk {
                    vec![if n % 4 == 0 { "Walk" } else { "walk" }.to_string()]
                } else {
                    vec!["desk".to_string()]
                };
                let mood = if walk { 4.0 } else { 2.0 } + (n % 3) as f64 * 0.5 + k as f64;
                let id = format!("c{n}k{k}");
                src.checkins.push(dt::CheckinIn {
                    id: id.clone(),
                    day: d.clone(),
                    mood,
                    energy: 3.0,
                    tags: tags.clone(),
                });
                old.moods.push(MoodRow {
                    id,
                    day: d.clone(),
                    mood,
                    energy: 3.0,
                    tags,
                    text: String::new(),
                });
            }
        }
        (src, old)
    }

    pub(crate) fn stored_rows(
        src: &dt::Inputs,
        now: DateTime<FixedOffset>,
        tag: &str,
    ) -> (PathBuf, Vec<dt::DailyRow>) {
        let to = now.date_naive();
        let from = to - Duration::days(60);
        let (rows, goals) = dt::build(src, from, to);
        // Through the real store, so the proof covers what a reader sees (REAL columns, JSON sources).
        let dir = temp_dir(tag);
        dt::write_in(&dir, &rows, &goals, &[], now.timestamp_millis(), (from, to)).unwrap();
        let back = dt::read_range_in(
            &dir,
            &from.format("%Y-%m-%d").to_string(),
            &to.format("%Y-%m-%d").to_string(),
        )
        .unwrap();
        assert_eq!(back.rows, rows);
        (dir, back.rows)
    }

    /// PROOF for A2. Same sources, two paths, `compute` outputs identical in full (findings, numbers,
    /// p-values, charts, counts, withheld, source ids). No ref normalisation is needed: `compute` writes
    /// its own `noop:<metric>:<day>` refs from the day rows, and the table's `session:<id>` and
    /// `checkin:<id>` refs are the same ids.
    #[test]
    fn daily_table_path_is_identical_to_the_per_row_path() {
        let (src, old) = world();
        let now = at(2026, 11, 12, 10);
        let (dir, rows) = stored_rows(&src, now, "pateq");
        let via_inputs = compute(&old, now);
        let via_table = compute_daily(&rows, now);
        assert_eq!(via_inputs, via_table);
        // The fixture must exercise the engine, not compare two empties.
        assert_eq!(via_inputs.state, "ready");
        for pair in ["sleep_focus", "tag_mood"] {
            assert!(via_inputs.findings.iter().any(|f| f.pair == pair), "{pair}");
        }
        assert!(via_inputs.counts.sessions > 21 && via_inputs.counts.checkins > 21);
        // The adapted day rows equal the old ones exactly.
        let adapted = day_rows(rows.iter());
        assert_eq!(adapted.len(), old.days.len());
        for o in &old.days {
            let a = adapted.iter().find(|a| a.day == o.day).unwrap();
            assert_eq!(
                (a.sleep_h, a.hrv, a.strain, a.recovery),
                (o.sleep_h, o.hrv, o.strain, o.recovery)
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Also when the window cuts the history: older rows and a future-dated one are ignored the same way.
    #[test]
    fn equivalence_holds_with_rows_outside_the_window_and_gaps() {
        let (mut src, mut old) = world();
        // A NOOP day with sleep only, another with nothing but HRV; a day 40 days back; a missing night.
        src.noop.retain(|n| n.day != day(3));
        old.days.retain(|d| d.day != day(3));
        for (n, hrv_only) in [(40i64, false), (6, true)] {
            let d = day(n);
            src.noop.retain(|x| x.day != d);
            old.days.retain(|x| x.day != d);
            let (sleep_min, hrv) = if hrv_only {
                (None, Some(60.0))
            } else {
                (Some(450.0), Some(50.0))
            };
            src.noop.push(dt::NoopDay {
                day: d.clone(),
                sleep_min,
                hrv,
                ..Default::default()
            });
            old.days.push(DayRow {
                day: d,
                sleep_h: sleep_min.map(|m| m / 60.0),
                hrv,
                ..Default::default()
            });
        }
        let now = at(2026, 11, 12, 10);
        let (dir, rows) = stored_rows(&src, now, "pateq2");
        assert_eq!(compute(&old, now), compute_daily(&rows, now));
        // And a different "now" (the window moves; the 40-day-old row enters then leaves).
        let later = at(2026, 11, 30, 10);
        let (dir2, rows2) = stored_rows(&src, later, "pateq3");
        assert_eq!(compute(&old, later), compute_daily(&rows2, later));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn tag_findings_survive_the_daily_table_because_tags_are_kept_per_day() {
        // The per-day union of tags is enough for "any check-in that day has the tag" (what compute tests);
        // mood is the day's mean in both paths. Pin the tag finding's numbers, not just equality.
        let (src, old) = world();
        let now = at(2026, 11, 12, 10);
        let (dir, rows) = stored_rows(&src, now, "pateq4");
        let a = compute(&old, now);
        let b = compute_daily(&rows, now);
        let ta = a.findings.iter().find(|f| f.pair == "tag_mood").unwrap();
        let tb = b.findings.iter().find(|f| f.pair == "tag_mood").unwrap();
        assert_eq!(
            (ta.id.as_str(), ta.n_with, ta.n_without, ta.mean_with, ta.p),
            (tb.id.as_str(), tb.n_with, tb.n_without, tb.mean_with, tb.p)
        );
        assert_eq!(ta.source_ids, tb.source_ids);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn table_usable_needs_a_recent_complete_build() {
        let now = 1_800_000_000_000_i64;
        let h = 3600 * 1000;
        assert!(table_usable(Some(now - h), &[], now));
        assert!(table_usable(Some(now - 35 * h), &["roadmaps".into()], now));
        assert!(!table_usable(None, &[], now));
        assert!(!table_usable(Some(0), &[], now));
        assert!(!table_usable(Some(now - 37 * h), &[], now));
        assert!(!table_usable(Some(now + h), &[], now));
        for p in ["noop", "checkins", "hemisync"] {
            assert!(!table_usable(Some(now - h), &[p.to_string()], now), "{p}");
        }
    }

    #[test]
    fn daily_rows_in_falls_back_when_there_is_no_usable_table() {
        let (src, _) = world();
        let now = at(2026, 11, 12, 10);
        // No table at all.
        let empty = temp_dir("patnone");
        assert!(daily_rows_in(&empty, now, WINDOW_DAYS).is_none());
        // A usable one.
        let (dir, _) = stored_rows(&src, now, "patuse");
        let rows = daily_rows_in(&dir, now, WINDOW_DAYS).unwrap();
        assert!(!rows.is_empty());
        // Two days later the 36 h build age has passed: live gatherers take over.
        assert!(daily_rows_in(&dir, at(2026, 11, 14, 10), WINDOW_DAYS).is_none());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn daily_source_is_cached_and_a_read_through_the_table_matches_a_live_read() {
        let (src, old) = world();
        let now = at(2026, 11, 12, 10);
        let (dir, rows) = stored_rows(&src, now, "patsrc");
        let cache = temp_dir("patcache");
        let via_table = read_in_src(&cache, now, || Source::Daily(rows)).unwrap();
        // Stored (a JSON round trip may differ from memory in the last float digit, as for any cache).
        assert_eq!(
            load_stored(&cache).map(|r| r.findings.len()),
            Some(via_table.findings.len())
        );
        let cache2 = temp_dir("patcache2");
        let live = read_in(&cache2, now, || old).unwrap();
        assert_eq!(live, via_table);
        for d in [dir, cache, cache2] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
