//! NEUROSENSE training records (decision 0059, step 8b).
//!
//! Reads the session progress record that guided training saves inside each session file
//! (`data.neurofeedback.progress_record`, from 0059 on). Session files are opened read-only through
//! `sessions::read_stored_sessions_in`; nothing here writes, moves or deletes a session or recording.
//! Only counts, percentages and thresholds leave this module, never raw EEG.
//!
//! A record is **eligible** for longitudinal use only when it is live, reached training, kept enough
//! valid data, and trained long enough. An ineligible record is still listed, with the reason.

use super::shared::task::{blocking, JoinErr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const MIN_VALID_PCT: f64 = 70.0;
pub(crate) const MIN_TRAINED_SECS: f64 = 180.0;
const MAX_RECORDS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingRecord {
    pub session_id: String,
    pub start: String,
    pub duration_secs: u64,
    pub protocol_id: String,
    pub protocol_set: String,
    pub record_version: String,
    /// Training method version: records are only compared with the same method.
    pub method: String,
    /// `live`, `replay`, `demo`, or null when the session did not say.
    pub source: Option<String>,
    pub valid_pct: Option<f64>,
    pub time_in_target_pct: Option<f64>,
    pub trained_seconds: f64,
    pub target_reward_rate: Option<f64>,
    /// Number of 30 s threshold checks (the baseline entry is not counted).
    pub threshold_checks: u32,
    /// The reward threshold scale at the end of the session (1 = the baseline threshold).
    pub final_threshold_scale: Option<f64>,
    /// Each reward feature's resting level before training: the baseline reward threshold / 1.1
    /// (relative band power, %, from this session's own protocol baseline).
    pub baseline_levels: BTreeMap<String, f64>,
    pub held_seconds: BTreeMap<String, f64>,
    pub eligible: bool,
    pub not_eligible_reason: Option<String>,
    /// Decision 0061: session HRV from the WHOOP strap's RR intervals in NOOP, filled in only by the
    /// records command (null elsewhere). A separate measurement from the EEG (biofeedback, not neurofeedback).
    pub hrv: Option<super::noop_rr::SessionHrv>,
}

/// The session's end, from its start and length.
/// Never panics on a corrupt length (security review, 0061): an unrepresentable end reads as empty.
fn end_of(r: &TrainingRecord) -> String {
    chrono::DateTime::parse_from_rfc3339(&r.start)
        .ok()
        .and_then(|s| {
            i64::try_from(r.duration_secs)
                .ok()
                .and_then(chrono::TimeDelta::try_seconds)
                .and_then(|d| s.checked_add_signed(d))
        })
        .map(|e| e.to_rfc3339())
        .unwrap_or_default()
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

/// Session files are on disk and could be hand-edited or forged, and these strings reach Amara's prompt
/// (security review, 0059). Only short, plain identifiers pass; anything else reads as missing.
fn ident(v: &Value, max: usize, extra: &[char]) -> Option<String> {
    v.as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= max
                && s.chars().all(|c| {
                    c.is_ascii_alphanumeric() || c == '-' || c == '_' || extra.contains(&c)
                })
        })
        .map(str::to_string)
}

/// A protocol id: the same slug rule as the calibration store (lowercase, digits, `-`, `_`, at most 40).
fn slug(v: &Value) -> Option<String> {
    ident(v, 40, &[]).filter(|s| s.chars().all(|c| !c.is_ascii_uppercase()))
}

/// A method or record version, such as `training-v2-0057`.
fn version_of(v: &Value) -> Option<String> {
    ident(v, 60, &['.', ':'])
}

/// A band label or hold reason: letters, digits, space, `_`, `-`, at most 24 (`High Beta`, `not_live`).
fn key_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 24
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '-')
}

fn num_map(v: &Value) -> BTreeMap<String, f64> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| key_ok(k))
                .take(16)
                .filter_map(|(k, x)| num(x).map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default()
}

/// One record from a stored session, or None when the session has no 0059 progress record.
pub(crate) fn record_from(
    session: &super::sessions::RecordedSession,
    data: &Value,
) -> Option<TrainingRecord> {
    let r = data.get("neurofeedback")?.get("progress_record")?;
    if !r.is_object() {
        return None;
    }
    let source = data
        .get("data_source")
        .and_then(Value::as_str)
        .filter(|s| matches!(*s, "live" | "replay" | "demo"))
        .map(str::to_string);
    let empty = Vec::new();
    let log = r
        .get("threshold_log")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let baseline_levels = log
        .iter()
        .find(|e| e.get("event").and_then(Value::as_str) == Some("baseline"))
        .map(|e| {
            num_map(e.get("reward_min").unwrap_or(&Value::Null))
                .into_iter()
                .map(|(k, v)| (k, (v / 1.1 * 100.0).round() / 100.0))
                .collect()
        })
        .unwrap_or_default();
    let final_threshold_scale = log
        .iter()
        .rev()
        .find_map(|e| e.get("scale_after").and_then(num));
    let valid_pct = r.get("valid_pct").and_then(num);
    let trained_seconds = r.get("trained_seconds").and_then(num).unwrap_or(0.0);
    let method = r.get("method").and_then(version_of).unwrap_or_default();
    let reason = if source.as_deref() != Some("live") {
        Some("not recorded from the live headset")
    } else if method.is_empty() {
        Some("no method recorded")
    } else if valid_pct.is_none_or(|v| v < MIN_VALID_PCT) {
        Some("less than 70% of the training time was usable")
    } else if trained_seconds < MIN_TRAINED_SECS {
        Some("under 3 minutes of usable training")
    } else {
        None
    };
    Some(TrainingRecord {
        session_id: session.id.clone(),
        start: session.start.clone(),
        duration_secs: session.duration_secs,
        protocol_id: r.get("protocol_id").and_then(slug).unwrap_or_default(),
        protocol_set: r
            .get("protocol_set")
            .and_then(version_of)
            .unwrap_or_default(),
        record_version: r.get("version").and_then(version_of).unwrap_or_default(),
        method,
        source,
        valid_pct,
        time_in_target_pct: r.get("time_in_target_pct").and_then(num),
        trained_seconds,
        target_reward_rate: r.get("target_reward_rate").and_then(num),
        threshold_checks: log
            .iter()
            .filter(|e| e.get("event").and_then(Value::as_str) == Some("adapt"))
            .count() as u32,
        final_threshold_scale,
        baseline_levels,
        held_seconds: num_map(r.get("held_seconds").unwrap_or(&Value::Null)),
        eligible: reason.is_none(),
        not_eligible_reason: reason.map(str::to_string),
        hrv: None,
    })
}

/// Every session with a progress record, newest first.
pub(crate) fn records_in(sessions_dir: &Path) -> Vec<TrainingRecord> {
    let mut out: Vec<TrainingRecord> = super::sessions::read_stored_sessions_in(sessions_dir)
        .iter()
        .filter_map(|s| record_from(&s.session, &s.data))
        .collect();
    out.sort_by(|a, b| b.start.cmp(&a.start));
    out.truncate(MAX_RECORDS);
    out
}

fn sessions_dir() -> PathBuf {
    super::sessions::sessions_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_training_records(limit: Option<u32>) -> Result<Vec<TrainingRecord>, String> {
    blocking(JoinErr::Plain, move || {
        let mut r = records_in(&sessions_dir());
        r.truncate(limit.unwrap_or(50).min(MAX_RECORDS as u32) as usize);
        // One read-only open of NOOP's store for the whole list (security review, 0061).
        let windows: Vec<(String, String)> = r
            .iter()
            .map(|rec| (rec.start.clone(), end_of(rec)))
            .collect();
        for (rec, h) in r.iter_mut().zip(super::noop_rr::session_hrv_many(&windows)) {
            rec.hrv = Some(h);
        }
        Ok(r)
    })
    .await
}

// ---------------------------------------------------------------- longitudinal (8f)

pub(crate) const TRENDS_VERSION: &str = "trends-v1-0059";
/// A trend needs at least this many eligible sessions of one protocol and method.
pub(crate) const MIN_TREND_SESSIONS: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FeatureTrend {
    /// `resting_level:<band>`, `final_threshold_scale` or `valid_pct`.
    pub feature: String,
    pub unit: String,
    /// How the per-session number is calculated, in words.
    pub calculation: String,
    pub n: u32,
    pub median: f64,
    pub robust_sd: f64,
    /// Median of the earlier half of the sessions, then of the later half (in time order).
    pub earlier_median: f64,
    pub later_median: f64,
    /// later - earlier, in the feature's unit, and in robust SDs of all sessions.
    pub change: f64,
    pub change_in_spreads: Option<f64>,
    /// Theil-Sen slope per session (median of pairwise slopes): robust to one odd session.
    pub slope_per_session: f64,
    /// `higher`, `lower` or `no_clear_change` (|change| under one robust SD, or no spread).
    pub direction: String,
    pub confidence: String,
    pub uncertainty: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolTrends {
    pub protocol_id: String,
    pub method: String,
    pub sessions: u32,
    pub first: String,
    pub last: String,
    /// Empty below MIN_TREND_SESSIONS.
    pub features: Vec<FeatureTrend>,
    /// Session ids the trends were calculated from, oldest first (traceability).
    pub session_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingTrends {
    pub version: String,
    pub records: u32,
    pub eligible: u32,
    pub protocols: Vec<ProtocolTrends>,
    pub note: String,
}

fn trend_confidence(n: usize) -> &'static str {
    match n {
        0..=4 => "none",
        5..=9 => "low",
        10..=19 => "medium",
        _ => "high",
    }
}

/// Over the newest 100 sessions at most, so the pairwise work stays small (security review, 0059).
const THEIL_SEN_MAX: usize = 100;

fn theil_sen(values: &[f64]) -> f64 {
    let values = &values[values.len().saturating_sub(THEIL_SEN_MAX)..];
    let mut slopes = Vec::with_capacity(values.len() * values.len().saturating_sub(1) / 2);
    for i in 0..values.len() {
        for j in i + 1..values.len() {
            slopes.push((values[j] - values[i]) / (j - i) as f64);
        }
    }
    super::neuro_baseline::robust(&slopes).map_or(0.0, |r| r.median)
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

fn feature_trend(
    feature: &str,
    unit: &str,
    calculation: &str,
    values: &[f64],
) -> Option<FeatureTrend> {
    if values.len() < MIN_TREND_SESSIONS {
        return None;
    }
    let all = super::neuro_baseline::robust(values)?;
    let half = values.len() / 2;
    let earlier = super::neuro_baseline::robust(&values[..half])?.median;
    let later = super::neuro_baseline::robust(&values[values.len() - half..])?.median;
    let change = later - earlier;
    let spreads = (all.robust_sd > 0.0).then(|| change / all.robust_sd);
    let direction = match spreads {
        Some(z) if z >= 1.0 => "higher",
        Some(z) if z <= -1.0 => "lower",
        _ => "no_clear_change",
    };
    let n = values.len();
    Some(FeatureTrend {
        feature: feature.into(),
        unit: unit.into(),
        calculation: calculation.into(),
        n: n as u32,
        median: round(all.median, 3),
        robust_sd: round(all.robust_sd, 3),
        earlier_median: round(earlier, 3),
        later_median: round(later, 3),
        change: round(change, 3),
        change_in_spreads: spreads.map(|z| round(z, 2)),
        slope_per_session: round(theil_sen(values), 4),
        direction: direction.into(),
        confidence: trend_confidence(n).into(),
        uncertainty: format!(
            "From {n} sessions. Session-to-session EEG varies with fit, time of day, sleep and more, and the protocol's thresholds adapt within each session, so a difference between earlier and later sessions is a measured change, not evidence of a lasting or cognitive change."
        ),
    })
}

/// Trends per protocol and training method, from eligible records only.
pub(crate) fn trends_from(records: &[TrainingRecord]) -> TrainingTrends {
    let eligible: Vec<&TrainingRecord> = records.iter().filter(|r| r.eligible).collect();
    let mut groups: BTreeMap<(String, String), Vec<&TrainingRecord>> = BTreeMap::new();
    for r in &eligible {
        groups
            .entry((r.protocol_id.clone(), r.method.clone()))
            .or_default()
            .push(r);
    }
    let protocols = groups
        .into_iter()
        .map(|((protocol_id, method), mut rs)| {
            rs.sort_by(|a, b| a.start.cmp(&b.start));
            let mut features = Vec::new();
            let bands: std::collections::BTreeSet<&String> =
                rs.iter().flat_map(|r| r.baseline_levels.keys()).collect();
            for band in bands {
                let v: Vec<f64> = rs.iter().filter_map(|r| r.baseline_levels.get(band).copied()).collect();
                if let Some(t) = feature_trend(
                    &format!("resting_level:{band}"),
                    "%",
                    &format!("{band} relative band power in the session's own protocol baseline, before training (baseline reward threshold / 1.1)"),
                    &v,
                ) {
                    features.push(t);
                }
            }
            let scale: Vec<f64> = rs.iter().filter_map(|r| r.final_threshold_scale).collect();
            if let Some(t) = feature_trend(
                "final_threshold_scale",
                "x",
                "the reward threshold at the end of the session, as a multiple of that session's baseline threshold (it rises when reward came more often than the target rate)",
                &scale,
            ) {
                features.push(t);
            }
            let valid: Vec<f64> = rs.iter().filter_map(|r| r.valid_pct).collect();
            if let Some(t) = feature_trend(
                "valid_pct",
                "%",
                "share of training time with usable signal (data quality, not performance)",
                &valid,
            ) {
                features.push(t);
            }
            ProtocolTrends {
                protocol_id,
                method,
                sessions: rs.len() as u32,
                first: rs.first().map(|r| r.start.clone()).unwrap_or_default(),
                last: rs.last().map(|r| r.start.clone()).unwrap_or_default(),
                features,
                session_ids: rs.iter().map(|r| r.session_id.clone()).collect(),
            }
        })
        .collect();
    TrainingTrends {
        version: TRENDS_VERSION.into(),
        records: records.len() as u32,
        eligible: eligible.len() as u32,
        protocols,
        note: format!("Only live sessions with at least {MIN_VALID_PCT}% usable data and 3 minutes of training count, and only sessions of the same protocol and training method are compared. A trend needs {MIN_TREND_SESSIONS} such sessions. Time in target is not trended: the adaptive threshold holds it near the target rate by design. These are EEG signal measurements, not measures of emotion, attention, stress or relaxation."),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_training_trends() -> Result<TrainingTrends, String> {
    blocking(JoinErr::Plain, || {
        Ok(trends_from(&records_in(&sessions_dir())))
    })
    .await
}

/// Lines for Amara's local-only `nexus_neuro` section: structured, no raw EEG, with what is uncertain.
pub(crate) fn context_lines(sessions_dir: &Path) -> Vec<String> {
    context_lines_with(sessions_dir, |r| {
        super::noop_rr::session_hrv(&r.start, &end_of(r))
    })
}

/// `hrv_of` reads the latest session's HRV (injected so tests never open the real NOOP store).
pub(crate) fn context_lines_with(
    sessions_dir: &Path,
    hrv_of: impl Fn(&TrainingRecord) -> super::noop_rr::SessionHrv,
) -> Vec<String> {
    let records = records_in(sessions_dir);
    if records.is_empty() {
        return vec!["No neurofeedback training records yet (records start with guided sessions from decision 0059).".into()];
    }
    let t = trends_from(&records);
    // The honesty note goes first, so the section's length limit can never cut it (security review, 0059).
    let mut lines = vec![t.note.clone(), format!(
        "Neurofeedback training records: {} sessions, {} usable for trends. When explaining them: say what changed, how it compares with his own history, which measurements support it, and what is uncertain; optional training may be suggested to test, never prescribed. An in-session change does not prove cognitive improvement.",
        t.records, t.eligible
    )];
    if let Some(r) = records.first() {
        let levels: Vec<String> = r
            .baseline_levels
            .iter()
            .map(|(k, v)| format!("{k} {v:.1}%"))
            .collect();
        lines.push(format!(
            "Latest session {} protocol {} (source {}): usable {} of training, {:.0} s trained, time in target {} (target {}), {} threshold checks ending at x{}; resting level {}{}.",
            r.start.get(..10).unwrap_or(""),
            r.protocol_id,
            r.source.as_deref().unwrap_or("not recorded"),
            r.valid_pct.map_or("not measured".into(), |v| format!("{v:.0}%")),
            r.trained_seconds,
            r.time_in_target_pct.map_or("not measured".into(), |v| format!("{v:.0}%")),
            r.target_reward_rate.map_or("not recorded".into(), |v| format!("{:.0}%", v * 100.0)),
            r.threshold_checks,
            r.final_threshold_scale.map_or("not recorded".into(), |v| format!("{v:.2}")),
            if levels.is_empty() { "not recorded".into() } else { levels.join(", ") },
            r.not_eligible_reason.as_deref().map_or(String::new(), |w| format!("; not used for trends ({w})")),
        ));
        let h = hrv_of(r);
        lines.push(match (h.state.as_str(), h.rmssd_ms) {
            ("available", Some(v)) => format!(
                "Latest session HRV (biofeedback, separate from the EEG): RMSSD {v:.0} ms over {} clean beat pairs, {:.0}% of the session covered; source {}.",
                h.accepted_pairs, h.coverage_pct, h.source
            ),
            _ => format!(
                "Latest session HRV: not available ({}).",
                h.reason.as_deref().unwrap_or("no reading")
            ),
        });
    }
    for p in &t.protocols {
        if p.features.is_empty() {
            lines.push(format!(
                "Protocol {}: {} usable sessions, fewer than {MIN_TREND_SESSIONS}, so no trend yet.",
                p.protocol_id, p.sessions
            ));
            continue;
        }
        for f in &p.features {
            lines.push(format!(
                "Trend {} {} ({} sessions, confidence {}): earlier median {}{u}, later median {}{u}, change {}{u}{}; {}.",
                p.protocol_id,
                f.feature,
                f.n,
                f.confidence,
                f.earlier_median,
                f.later_median,
                f.change,
                f.change_in_spreads.map_or(String::new(), |z| format!(" ({z} usual spreads)")),
                f.direction.replace('_', " "),
                u = f.unit,
            ));
        }
    }
    // Whole lines only, within a budget, so nothing is cut mid-sentence by the section limit.
    let mut used = 0;
    let mut kept = Vec::new();
    let total = lines.len();
    for line in lines {
        if used + line.len() > CONTEXT_BUDGET && kept.len() >= 2 {
            kept.push(format!(
                "{} more training lines not shown.",
                total - kept.len()
            ));
            break;
        }
        used += line.len() + 1;
        kept.push(line);
    }
    kept
}

/// Characters of training lines Amara gets (the whole nexus_neuro section is capped at 4000).
pub(crate) const CONTEXT_BUDGET: usize = 2_400;

#[cfg(test)]
mod tests;
