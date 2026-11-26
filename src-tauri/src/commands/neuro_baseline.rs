//! NEUROSENSE calibration history and personal baseline (decision 0058).
//!
//! Two kinds of calibration report, kept apart:
//! - `pre_meditation`: resting eyes-open and eyes-closed recordings with artifact rejection and features
//!   (relative band power, alpha peak frequency, EEG variability, channel quality). Only these feed the
//!   personal baseline, and only when recorded live at high quality.
//! - `protocol`: the thresholds a neurofeedback protocol calibrated for one session, with its data
//!   quality and method. Never part of the baseline.
//! - `post_session` (decision 0059): the same resting recording, made after a session. Never part of the
//!   baseline (it is a different recording condition). On save it is compared with the same day's
//!   pre-meditation calibration and with the baseline, as a measured change and nothing more.
//!
//! Reports hold features and counts only, never raw EEG. `<data>/neuro/calibrations.json` (0600, atomic).
//! The store refuses new reports when full rather than dropping old ones. The baseline is recomputed on
//! read with a versioned method (robust statistics: median and MAD), so it is reproducible from the history.
//! Band power is a signal measurement, never a measure of emotion, attention, stress or relaxation.

use super::local_store::{read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 6 * 1024 * 1024;
const MAX_REPORTS: usize = 1000;
pub(crate) const BASELINE_VERSION: &str = "baseline-v1-median-mad";
/// High-quality requirements for a condition to count toward the baseline.
const MIN_ACCEPTED_EPOCHS: u32 = 20;
const MIN_ACCEPTED_FRACTION: f64 = 0.6;
const MIN_CHANNEL_QUALITY: f64 = 0.5;
/// The baseline uses at most this many of the newest qualifying recordings per condition.
const BASELINE_WINDOW: usize = 20;
const MIN_BASELINE_N: usize = 3;
pub(crate) const BANDS: [&str; 5] = ["delta", "theta", "alpha", "beta", "gamma"];
pub(crate) const CONDITIONS: [&str; 2] = ["eyes_open", "eyes_closed"];
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationMethod {
    /// The frontend feature code's version (e.g. `calibration-v1`).
    pub version: String,
    pub epoch_secs: f64,
    /// FFT, window and band edges, in words (so a later reader can recompute or compare).
    pub fft: String,
    /// What rejects an epoch, in words.
    pub rejection: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionFeatures {
    /// Median relative power per band over accepted epochs, percent of 1–45 Hz.
    pub rel_bands: BTreeMap<String, f64>,
    /// Peak of the 7–13 Hz spectrum, only where a clear peak was found (else none).
    #[serde(default)]
    pub alpha_peak_hz: Option<f64>,
    /// Accepted epochs that had a clear alpha peak.
    #[serde(default)]
    pub alpha_peak_epochs: u32,
    /// Epoch-to-epoch spread of log total power (MAD), a measure of signal variability.
    #[serde(default)]
    pub variability: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionResult {
    /// `eyes_open` or `eyes_closed`.
    pub condition: String,
    pub planned_secs: f64,
    pub recorded_secs: f64,
    pub epochs_total: u32,
    pub epochs_accepted: u32,
    /// Rejected epochs by reason (`contact`, `blink`, `movement`, `jaw`, `amplitude`, `flat`, `frozen`, `dropout`).
    #[serde(default)]
    pub rejected: BTreeMap<String, u32>,
    /// Per channel, the fraction of epochs with usable contact (0–1).
    pub channel_quality: BTreeMap<String, f64>,
    /// None when no epoch was accepted.
    #[serde(default)]
    pub features: Option<ConditionFeatures>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationReport {
    /// Set by the store.
    #[serde(default)]
    pub id: String,
    /// Set by the store (RFC 3339).
    #[serde(default)]
    pub created_at: String,
    /// `pre_meditation`, `protocol` or `post_session`.
    pub kind: String,
    /// `live`, `replay` or `demo` (only live reports can enter the baseline).
    pub source: String,
    pub method: CalibrationMethod,
    #[serde(default)]
    pub conditions: Vec<ConditionResult>,
    /// Protocol calibration only: protocol id, thresholds, baseline ticks used and rejected, method.
    #[serde(default)]
    pub protocol: Option<serde_json::Value>,
    /// Links a protocol calibration to its training session.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    reports: Vec<CalibrationReport>,
}

fn file(data: &Path) -> PathBuf {
    data.join("neuro").join("calibrations.json")
}

fn finite01(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

/// Refuses anything malformed; never stores raw samples (the shape has no room for them).
pub(crate) fn validate(r: &CalibrationReport) -> Result<(), String> {
    if !matches!(
        r.kind.as_str(),
        "pre_meditation" | "protocol" | "post_session"
    ) {
        return Err("kind must be pre_meditation, protocol or post_session".into());
    }
    if !matches!(r.source.as_str(), "live" | "replay" | "demo") {
        return Err("source must be live, replay or demo".into());
    }
    if r.method.version.trim().is_empty()
        || r.method.version.len() > 60
        || r.method.fft.len() > 400
        || r.method.rejection.len() > 400
    {
        return Err("the method needs a version and short descriptions".into());
    }
    if matches!(r.kind.as_str(), "pre_meditation" | "post_session") && r.conditions.is_empty() {
        return Err("a resting calibration needs at least one condition".into());
    }
    if r.kind == "protocol" && r.protocol.is_none() {
        return Err("a protocol calibration needs its protocol block".into());
    }
    if let Some(p) = &r.protocol {
        if serde_json::to_string(p).map_or(true, |s| s.len() > 3072) {
            return Err("the protocol block is too large".into());
        }
        // The protocol id reaches Amara's context: a short slug only.
        let id = p.get("protocolId").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty()
            || id.len() > 40
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err("the protocol block needs a short protocolId".into());
        }
    }
    if r.session_id.as_ref().is_some_and(|s| s.len() > 80) {
        return Err("the session id is too long".into());
    }
    // Every report fits the store's budget: MAX_REPORTS of them stay under MAX_BYTES.
    if serde_json::to_string(r).map_or(true, |s| s.len() as u64 > MAX_BYTES / MAX_REPORTS as u64) {
        return Err("the report is too large".into());
    }
    if r.conditions.len() > 2 {
        return Err("at most two conditions".into());
    }
    for c in &r.conditions {
        if !CONDITIONS.contains(&c.condition.as_str()) {
            return Err("condition must be eyes_open or eyes_closed".into());
        }
        if !(c.planned_secs.is_finite()
            && c.recorded_secs.is_finite()
            && (0.0..=3600.0).contains(&c.planned_secs)
            && (0.0..=3600.0).contains(&c.recorded_secs))
        {
            return Err("durations are 0 to 3600 seconds".into());
        }
        if c.epochs_accepted > c.epochs_total
            || c.channel_quality.len() > 8
            || c.rejected.len() > 12
        {
            return Err("epoch counts or channel list are inconsistent".into());
        }
        if c.channel_quality.values().any(|v| !finite01(*v)) {
            return Err("channel quality is a fraction from 0 to 1".into());
        }
        if let Some(f) = &c.features {
            if f.rel_bands.keys().any(|k| !BANDS.contains(&k.as_str()))
                || f.rel_bands
                    .values()
                    .any(|v| !(v.is_finite() && (0.0..=100.0).contains(v)))
            {
                return Err("relative bands are percentages of the five named bands".into());
            }
            if f.alpha_peak_hz
                .is_some_and(|h| !(h.is_finite() && (7.0..=13.0).contains(&h)))
            {
                return Err("an alpha peak is between 7 and 13 Hz".into());
            }
            if f.variability.is_some_and(|v| !(v.is_finite() && v >= 0.0)) {
                return Err("variability is a non-negative number".into());
            }
        }
    }
    Ok(())
}

/// Whether one condition of a report meets the baseline's high-quality requirements.
pub(crate) fn high_quality(r: &CalibrationReport, c: &ConditionResult) -> bool {
    r.kind == "pre_meditation"
        && r.source == "live"
        && c.features.is_some()
        && c.epochs_accepted >= MIN_ACCEPTED_EPOCHS
        && c.epochs_total > 0
        && f64::from(c.epochs_accepted) / f64::from(c.epochs_total) >= MIN_ACCEPTED_FRACTION
        && !c.channel_quality.is_empty()
        && c.channel_quality
            .values()
            .all(|q| *q >= MIN_CHANNEL_QUALITY)
}

fn median(xs: &mut [f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    let n = xs.len();
    Some(if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    })
}

/// Median and robust SD (MAD × 1.4826) of a feature across baseline recordings.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RobustStat {
    pub median: f64,
    pub robust_sd: f64,
    pub n: u32,
}

pub(crate) fn robust(values: &[f64]) -> Option<RobustStat> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    let n = v.len();
    let m = median(&mut v)?;
    let mut dev: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    let mad = median(&mut dev).unwrap_or(0.0);
    Some(RobustStat {
        median: m,
        robust_sd: mad * 1.4826,
        n: n as u32,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConditionBaseline {
    pub condition: String,
    /// Qualifying recordings used (newest first, at most 20).
    pub recordings: u32,
    pub report_ids: Vec<String>,
    pub rel_bands: BTreeMap<String, RobustStat>,
    pub alpha_peak_hz: Option<RobustStat>,
    pub variability: Option<RobustStat>,
    /// `none` (fewer than 3), `low` (3–5), `medium` (6–11) or `high` (12+).
    pub confidence: String,
    pub uncertainty: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PersonalBaseline {
    pub version: String,
    pub conditions: Vec<ConditionBaseline>,
    /// Eyes-closed over eyes-open alpha (median of per-session ratios), where both were recorded well.
    pub alpha_reactivity: Option<RobustStat>,
    pub note: String,
}

fn confidence(n: usize) -> &'static str {
    match n {
        0..=2 => "none",
        3..=5 => "low",
        6..=11 => "medium",
        _ => "high",
    }
}

/// The baseline from a report history (newest last), excluding `exclude_id` (the report being compared).
pub(crate) fn baseline_from(
    reports: &[CalibrationReport],
    exclude_id: Option<&str>,
) -> PersonalBaseline {
    let usable = |c: &str| -> Vec<(&CalibrationReport, &ConditionResult)> {
        reports
            .iter()
            .rev()
            .filter(|r| Some(r.id.as_str()) != exclude_id)
            .filter_map(|r| {
                r.conditions
                    .iter()
                    .find(|x| x.condition == c && high_quality(r, x))
                    .map(|x| (r, x))
            })
            .take(BASELINE_WINDOW)
            .collect()
    };
    let conditions = CONDITIONS
        .iter()
        .map(|c| {
            let rows = usable(c);
            let n = rows.len();
            let feats = |f: &dyn Fn(&ConditionFeatures) -> Option<f64>| -> Vec<f64> { rows.iter().filter_map(|(_, x)| x.features.as_ref().and_then(f)).collect() };
            let rel_bands = if n >= MIN_BASELINE_N {
                BANDS
                    .iter()
                    .filter_map(|b| robust(&feats(&|f| f.rel_bands.get(*b).copied())).map(|s| (b.to_string(), s)))
                    .collect()
            } else {
                BTreeMap::new()
            };
            let peaks = feats(&|f| f.alpha_peak_hz);
            let vars = feats(&|f| f.variability);
            ConditionBaseline {
                condition: c.to_string(),
                recordings: n as u32,
                report_ids: rows.iter().map(|(r, _)| r.id.clone()).collect(),
                rel_bands,
                alpha_peak_hz: (peaks.len() >= MIN_BASELINE_N).then(|| robust(&peaks)).flatten(),
                variability: (vars.len() >= MIN_BASELINE_N).then(|| robust(&vars)).flatten(),
                confidence: confidence(n).into(),
                uncertainty: match n {
                    0..=2 => format!("{n} high-quality recording{} so far; at least 3 are needed before there is a baseline.", if n == 1 { "" } else { "s" }),
                    _ => format!("From {n} high-quality recordings. Day-to-day EEG varies; a value outside the usual range on one day is common and not a finding on its own."),
                },
            }
        })
        .collect();
    let ratios: Vec<f64> = reports
        .iter()
        .rev()
        .filter(|r| Some(r.id.as_str()) != exclude_id)
        .filter_map(|r| {
            let get = |c: &str| {
                r.conditions
                    .iter()
                    .find(|x| x.condition == c && high_quality(r, x))
                    .and_then(|x| x.features.as_ref())
                    .and_then(|f| f.rel_bands.get("alpha").copied())
            };
            match (get("eyes_open"), get("eyes_closed")) {
                (Some(o), Some(cl)) if o > 0.0 => Some(cl / o),
                _ => None,
            }
        })
        .take(BASELINE_WINDOW)
        .collect();
    PersonalBaseline {
        version: BASELINE_VERSION.into(),
        conditions,
        alpha_reactivity: (ratios.len() >= MIN_BASELINE_N).then(|| robust(&ratios)).flatten(),
        note: "Relative EEG band power and alpha peak frequency are signal measurements. They are not measures of emotion, attention, stress or relaxation.".into(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FeatureComparison {
    pub condition: String,
    /// `alpha` (relative band), `alpha_peak_hz` or `variability`.
    pub feature: String,
    pub value: f64,
    pub usual_median: f64,
    pub usual_robust_sd: f64,
    /// (value - median) / robust SD; none when the spread is zero.
    pub robust_z: Option<f64>,
    /// `within`, `above` or `below` the usual range (|z| < 1.5), in signal terms only.
    pub position: String,
}

/// Each feature of `r` against the baseline built from the history before it.
pub(crate) fn compare(r: &CalibrationReport, base: &PersonalBaseline) -> Vec<FeatureComparison> {
    let mut out = Vec::new();
    for c in &r.conditions {
        let (Some(f), Some(b)) = (
            &c.features,
            base.conditions.iter().find(|x| x.condition == c.condition),
        ) else {
            continue;
        };
        let mut push = |name: &str, v: Option<f64>, s: Option<&RobustStat>| {
            if let (Some(v), Some(s)) = (v, s) {
                let z = (s.robust_sd > 0.0).then(|| (v - s.median) / s.robust_sd);
                out.push(FeatureComparison {
                    condition: c.condition.clone(),
                    feature: name.into(),
                    value: v,
                    usual_median: s.median,
                    usual_robust_sd: s.robust_sd,
                    robust_z: z.map(|z| (z * 100.0).round() / 100.0),
                    position: match z {
                        Some(z) if z >= 1.5 => "above",
                        Some(z) if z <= -1.5 => "below",
                        _ => "within",
                    }
                    .into(),
                });
            }
        };
        for band in BANDS {
            push(band, f.rel_bands.get(band).copied(), b.rel_bands.get(band));
        }
        push("alpha_peak_hz", f.alpha_peak_hz, b.alpha_peak_hz.as_ref());
        push("variability", f.variability, b.variability.as_ref());
    }
    out
}

/// The most a pre-meditation calibration can come before a post-session one and still pair with it.
pub(crate) const PRE_POST_MAX_MINUTES: i64 = 6 * 60;
/// A change bigger than this many usual spreads (robust SD) is outside your usual variation.
const PRE_POST_SPREADS: f64 = 1.5;

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrePostChange {
    pub condition: String,
    pub feature: String,
    pub pre: f64,
    pub post: f64,
    /// post - pre, in the feature's own unit.
    pub change: f64,
    /// The baseline's robust SD for this feature, when there is a baseline.
    pub usual_robust_sd: Option<f64>,
    /// change / usual robust SD.
    pub change_in_usual_spreads: Option<f64>,
    /// |change| is more than 1.5 usual spreads. False when there is no baseline to judge by.
    pub beyond_usual_variation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrePost {
    pub pre_report_id: String,
    pub minutes_between: i64,
    pub changes: Vec<PrePostChange>,
    pub note: String,
}

fn features_of(c: &ConditionResult) -> Vec<(String, f64)> {
    let Some(f) = &c.features else {
        return Vec::new();
    };
    let mut out: Vec<(String, f64)> = BANDS
        .iter()
        .filter_map(|b| f.rel_bands.get(*b).map(|v| (b.to_string(), *v)))
        .collect();
    if let Some(v) = f.alpha_peak_hz {
        out.push(("alpha_peak_hz".into(), v));
    }
    if let Some(v) = f.variability {
        out.push(("variability".into(), v));
    }
    out
}

fn usual_sd(base: &PersonalBaseline, condition: &str, feature: &str) -> Option<f64> {
    let b = base.conditions.iter().find(|x| x.condition == condition)?;
    let s = match feature {
        "alpha_peak_hz" => b.alpha_peak_hz.as_ref(),
        "variability" => b.variability.as_ref(),
        band => b.rel_bands.get(band),
    }?;
    (s.robust_sd > 0.0).then_some(s.robust_sd)
}

/// A post-session recording against the newest pre-meditation one from the same source within six hours
/// before it, feature by feature, each change also measured in the baseline's usual spread.
pub(crate) fn pre_post(
    history: &[CalibrationReport],
    post: &CalibrationReport,
    base: &PersonalBaseline,
) -> Option<PrePost> {
    if post.kind != "post_session" {
        return None;
    }
    let at = chrono::DateTime::parse_from_rfc3339(&post.created_at).ok()?;
    let (pre, minutes) = history.iter().rev().find_map(|r| {
        if r.kind != "pre_meditation" || r.source != post.source {
            return None;
        }
        let t = chrono::DateTime::parse_from_rfc3339(&r.created_at).ok()?;
        let m = (at - t).num_minutes();
        (0..=PRE_POST_MAX_MINUTES).contains(&m).then_some((r, m))
    })?;
    let mut changes = Vec::new();
    for c in &post.conditions {
        let Some(pc) = pre.conditions.iter().find(|x| x.condition == c.condition) else {
            continue;
        };
        let before: BTreeMap<String, f64> = features_of(pc).into_iter().collect();
        for (feature, post_v) in features_of(c) {
            let Some(pre_v) = before.get(&feature).copied() else {
                continue;
            };
            let change = ((post_v - pre_v) * 1000.0).round() / 1000.0;
            let sd = usual_sd(base, &c.condition, &feature);
            let spreads = sd.map(|s| ((change / s) * 100.0).round() / 100.0);
            changes.push(PrePostChange {
                condition: c.condition.clone(),
                feature,
                pre: pre_v,
                post: post_v,
                change,
                usual_robust_sd: sd,
                change_in_usual_spreads: spreads,
                beyond_usual_variation: spreads.is_some_and(|z| z.abs() > PRE_POST_SPREADS),
            });
        }
    }
    Some(PrePost {
        pre_report_id: pre.id.clone(),
        minutes_between: minutes,
        changes,
        note: "A measured change in the EEG signal between two resting recordings. It does not show a lasting change, an improvement, or a change in emotion, attention, stress or relaxation; resting EEG also varies from one recording to the next.".into(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedCalibration {
    pub report: CalibrationReport,
    /// Per condition: whether this recording met the baseline's quality bar.
    pub baseline_eligible: BTreeMap<String, bool>,
    /// Against the baseline as it stood before this report.
    pub comparison: Vec<FeatureComparison>,
    pub baseline_before: PersonalBaseline,
    /// Post-session recordings only: the comparison with the same day's pre-meditation recording.
    pub pre_post: Option<PrePost>,
}

pub(crate) fn save_in(
    data: &Path,
    mut r: CalibrationReport,
    now: &str,
) -> Result<SavedCalibration, String> {
    validate(&r)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(data), MAX_BYTES)?;
    if store.reports.len() >= MAX_REPORTS {
        return Err("The calibration history is full; nothing was dropped to make room.".into());
    }
    r.id = format!("cal-{}", uuid::Uuid::new_v4().simple());
    r.created_at = now.to_string();
    let before = baseline_from(&store.reports, None);
    let comparison = compare(&r, &before);
    let eligible = r
        .conditions
        .iter()
        .map(|c| (c.condition.clone(), high_quality(&r, c)))
        .collect();
    let pre_post = pre_post(&store.reports, &r, &before);
    store.reports.push(r.clone());
    write_json_atomic(&file(data), &store, MAX_BYTES)?;
    Ok(SavedCalibration {
        report: r,
        baseline_eligible: eligible,
        comparison,
        baseline_before: before,
        pre_post,
    })
}

pub(crate) fn list_in(
    data: &Path,
    kind: Option<&str>,
    limit: usize,
) -> Result<Vec<CalibrationReport>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(data), MAX_BYTES)?;
    Ok(store
        .reports
        .into_iter()
        .rev()
        .filter(|r| kind.is_none_or(|k| r.kind == k))
        .take(limit.min(500))
        .collect())
}

pub(crate) fn baseline_in(data: &Path) -> Result<PersonalBaseline, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(data), MAX_BYTES)?;
    Ok(baseline_from(&store.reports, None))
}

fn fmt_stat(s: &RobustStat, unit: &str) -> String {
    format!(
        "{:.1}{unit} (usual {:.1}–{:.1}{unit}, n={})",
        s.median,
        s.median - s.robust_sd,
        s.median + s.robust_sd,
        s.n
    )
}

/// Amara's structured measurement report: the newest pre-meditation calibration and the baseline, as
/// numbers with their quality and uncertainty. No raw EEG, no interpretation as emotion or state.
pub(crate) fn context_section_in(data: &Path) -> Option<String> {
    let reports = list_in(data, None, 500).ok()?;
    let latest_pre = reports.iter().find(|r| r.kind == "pre_meditation");
    let latest_proto = reports.iter().find(|r| r.kind == "protocol");
    if latest_pre.is_none() && latest_proto.is_none() {
        return Some("No NEUROSENSE calibration recorded yet. Calibration is optional.".into());
    }
    let base = baseline_in(data).ok()?;
    let mut lines = vec!["NEUROSENSE measurement report (relative EEG band power, alpha peak, variability). These are signal measurements, not measures of emotion, attention, stress or relaxation; do not describe them as such.".to_string()];
    if let Some(r) = latest_pre {
        lines.push(format!(
            "Latest pre-meditation calibration {} (source {}, method {}):",
            r.created_at.get(..10).unwrap_or(""),
            r.source,
            r.method.version
        ));
        for c in &r.conditions {
            let q = if high_quality(r, c) {
                "high quality"
            } else {
                "below the baseline quality bar"
            };
            let f = c.features.as_ref();
            lines.push(format!(
                "- {}: {} of {} epochs accepted ({q}); alpha {}; alpha peak {}.",
                c.condition.replace('_', " "),
                c.epochs_accepted,
                c.epochs_total,
                f.and_then(|f| f.rel_bands.get("alpha"))
                    .map_or("not measured".into(), |v| format!("{v:.1}%")),
                f.and_then(|f| f.alpha_peak_hz)
                    .map_or("not estimable".into(), |v| format!("{v:.1} Hz")),
            ));
        }
    }
    for c in &base.conditions {
        match c.rel_bands.get("alpha") {
            Some(s) => lines.push(format!(
                "Baseline {}: alpha {} ; confidence {}.",
                c.condition.replace('_', " "),
                fmt_stat(s, "%"),
                c.confidence
            )),
            None => lines.push(format!(
                "Baseline {}: none yet ({}).",
                c.condition.replace('_', " "),
                c.uncertainty
            )),
        }
    }
    if let Some(p) = latest_proto.and_then(|r| r.protocol.as_ref()) {
        if let Some(id) = p.get("protocolId").and_then(|v| v.as_str()) {
            lines.push(format!("Latest protocol calibration: {id} (thresholds set from that session's own baseline)."));
        }
    }
    Some(lines.join("\n"))
}

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_calibration_save(report: CalibrationReport) -> Result<SavedCalibration, String> {
    blocking(JoinErr::Plain, move || {
        save_in(
            &data(),
            report,
            &super::shared::time::now_jst().to_rfc3339(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_calibration_list(
    kind: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<CalibrationReport>, String> {
    blocking(JoinErr::Plain, move || {
        list_in(&data(), kind.as_deref(), limit.unwrap_or(50) as usize)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_baseline_get() -> Result<PersonalBaseline, String> {
    blocking(JoinErr::Plain, || baseline_in(&data())).await
}

#[cfg(test)]
mod tests;
