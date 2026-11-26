//! NEUROSENSE protocol settings (decision 0059, step 8a).
//!
//! Each neurofeedback protocol can be configured: planned duration, target reward rate (the reinforcement
//! schedule), sound, optional visual feedback and volume. The settings live in
//! `<data>/neuro/protocol_settings.json` (0600, atomic), apart from recordings and the calibration history.
//! Every value is validated and clamped, an unknown field is refused, and a missing protocol means "use the
//! protocol's defaults". Defaults reproduce the training behaviour from before 0059 exactly.

use super::local_store::{read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 64 * 1024;
const MAX_PROTOCOLS: usize = 64;
pub(crate) const MIN_DURATION_MIN: u32 = 5;
pub(crate) const MAX_DURATION_MIN: u32 = 60;
pub(crate) const MIN_REWARD_RATE: f64 = 0.4;
pub(crate) const MAX_REWARD_RATE: f64 = 0.8;
const SOUNDS: [&str; 3] = ["soundscape", "tone", "silent"];

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolSettings {
    /// Planned session length in minutes (5–60).
    pub duration_min: u32,
    /// Share of clean training time the adaptive thresholds aim to reward (0.4–0.8).
    pub target_reward_rate: f64,
    /// `soundscape`, `tone` or `silent`.
    pub sound: String,
    /// The optional visual feedback (off by default).
    pub visual_feedback: bool,
    /// Feedback volume, 0–1.
    pub volume: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    protocols: BTreeMap<String, ProtocolSettings>,
}

fn file(data: &Path) -> PathBuf {
    data.join("neuro").join("protocol_settings.json")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(crate) fn validate(id: &str, s: &ProtocolSettings) -> Result<(), String> {
    if !valid_id(id) {
        return Err("That protocol id is not valid.".into());
    }
    if !(MIN_DURATION_MIN..=MAX_DURATION_MIN).contains(&s.duration_min) {
        return Err(format!(
            "Duration must be {MIN_DURATION_MIN}–{MAX_DURATION_MIN} minutes."
        ));
    }
    if !s.target_reward_rate.is_finite()
        || !(MIN_REWARD_RATE..=MAX_REWARD_RATE).contains(&s.target_reward_rate)
    {
        return Err("The target reward rate must be between 40% and 80%.".into());
    }
    if !SOUNDS.contains(&s.sound.as_str()) {
        return Err("Sound must be soundscape, tone or silent.".into());
    }
    if !s.volume.is_finite() || !(0.0..=1.0).contains(&s.volume) {
        return Err("Volume must be between 0 and 1.".into());
    }
    Ok(())
}

pub(crate) fn get_in(data: &Path) -> Result<BTreeMap<String, ProtocolSettings>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(data), MAX_BYTES)?;
    // A hand-edited file never feeds an invalid value to training: invalid entries read as defaults.
    Ok(store
        .protocols
        .into_iter()
        .filter(|(id, s)| validate(id, s).is_ok())
        .collect())
}

pub(crate) fn set_in(
    data: &Path,
    id: &str,
    settings: Option<ProtocolSettings>,
) -> Result<BTreeMap<String, ProtocolSettings>, String> {
    if let Some(s) = &settings {
        validate(id, s)?;
    } else if !valid_id(id) {
        return Err("That protocol id is not valid.".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(data), MAX_BYTES)?;
    match settings {
        Some(s) => {
            if !store.protocols.contains_key(id) && store.protocols.len() >= MAX_PROTOCOLS {
                return Err("Too many protocol settings are saved.".into());
            }
            store.protocols.insert(id.to_string(), s);
        }
        // None = back to the protocol's defaults.
        None => {
            store.protocols.remove(id);
        }
    }
    write_json_atomic(&file(data), &store, MAX_BYTES)?;
    Ok(store
        .protocols
        .into_iter()
        .filter(|(id, s)| validate(id, s).is_ok())
        .collect())
}

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn neuro_protocol_settings_get() -> Result<BTreeMap<String, ProtocolSettings>, String> {
    blocking(JoinErr::Plain, || get_in(&data())).await
}

/// Saves one protocol's settings, or with `settings: null` returns it to its defaults.
#[tauri::command]
#[specta::specta]
pub async fn neuro_protocol_settings_set(
    protocol_id: String,
    settings: Option<ProtocolSettings>,
) -> Result<BTreeMap<String, ProtocolSettings>, String> {
    blocking(JoinErr::Plain, move || {
        set_in(&data(), &protocol_id, settings)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    fn ok() -> ProtocolSettings {
        ProtocolSettings {
            duration_min: 15,
            target_reward_rate: 0.6,
            sound: "tone".into(),
            visual_feedback: false,
            volume: 0.7,
        }
    }

    #[test]
    fn missing_file_reads_as_no_settings() {
        let dir = temp_dir("neuro-protocols");
        assert!(get_in(&dir).unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip_and_reset() {
        let dir = temp_dir("neuro-protocols");
        set_in(&dir, "calm", Some(ok())).unwrap();
        assert_eq!(get_in(&dir).unwrap()["calm"], ok());
        let after = set_in(&dir, "calm", None).unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn out_of_range_values_are_refused() {
        let dir = temp_dir("neuro-protocols");
        for bad in [
            ProtocolSettings {
                duration_min: 2,
                ..ok()
            },
            ProtocolSettings {
                target_reward_rate: 0.95,
                ..ok()
            },
            ProtocolSettings {
                target_reward_rate: f64::NAN,
                ..ok()
            },
            ProtocolSettings {
                sound: "loud".into(),
                ..ok()
            },
            ProtocolSettings {
                volume: 1.5,
                ..ok()
            },
        ] {
            assert!(set_in(&dir, "calm", Some(bad)).is_err());
        }
        assert!(set_in(&dir, "../x", Some(ok())).is_err());
        assert!(get_in(&dir).unwrap().is_empty());
    }

    #[test]
    fn unknown_fields_are_refused() {
        let raw = serde_json::json!({ "durationMin": 15, "targetRewardRate": 0.6, "sound": "tone", "visualFeedback": false, "volume": 0.5, "stimulate": true });
        assert!(serde_json::from_value::<ProtocolSettings>(raw).is_err());
    }

    #[test]
    fn a_hand_edited_invalid_entry_reads_as_defaults() {
        let dir = temp_dir("neuro-protocols");
        std::fs::create_dir_all(dir.join("neuro")).unwrap();
        std::fs::write(
            file(&dir),
            r#"{"protocols":{"calm":{"durationMin":15,"targetRewardRate":5.0,"sound":"tone","visualFeedback":false,"volume":0.5}}}"#,
        )
        .unwrap();
        assert!(get_in(&dir).unwrap().is_empty());
    }
}
