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
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

fn str_of(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .map(str::to_string)
}

fn num_map(v: &Value) -> BTreeMap<String, f64> {
    v.as_object()
        .map(|o| {
            o.iter()
                .take(32)
                .filter_map(|(k, x)| num(x).map(|n| (k.chars().take(64).collect(), n)))
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
    let source = data.get("data_source").and_then(str_of);
    let log = r
        .get("threshold_log")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
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
    let method = r.get("method").and_then(str_of).unwrap_or_default();
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
        protocol_id: r.get("protocol_id").and_then(str_of).unwrap_or_default(),
        protocol_set: r.get("protocol_set").and_then(str_of).unwrap_or_default(),
        record_version: r.get("version").and_then(str_of).unwrap_or_default(),
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
        Ok(r)
    })
    .await
}

#[cfg(test)]
mod tests;
