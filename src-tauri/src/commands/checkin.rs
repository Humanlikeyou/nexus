//! Check-ins (design-1111v, HANDOFF-1111v §9.4.4): `<data dir>/checkins/checkins.json`. Text only: a
//! voice check-in arrives as a transcript and no audio is ever kept. "Done today" is any entry whose JST
//! day is today, so the quiet Needs-you row leaves when a check-in is saved on another device.

use super::local_store::{clean_text, day_of, iso, new_id, now_jst, read_json, write_json_atomic};
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_ENTRIES: usize = 3000;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckInRecord {
    pub id: String,
    pub at: String,
    /// JST `YYYY-MM-DD`.
    pub day: String,
    pub mood: u8,
    pub energy: u8,
    pub tags: Vec<String>,
    pub text: String,
    #[serde(default)]
    pub transcribed: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckInDraft {
    pub mood: u8,
    pub energy: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub transcribed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    entries: Vec<CheckInRecord>,
}

pub(crate) fn dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join("checkins")
}

fn file(dir: &Path) -> PathBuf {
    dir.join("checkins.json")
}

pub(crate) fn save_in(
    dir: &Path,
    d: &CheckInDraft,
    now: DateTime<FixedOffset>,
) -> Result<CheckInRecord, String> {
    if !(1..=5).contains(&d.mood) || !(1..=5).contains(&d.energy) || d.tags.len() > 8 {
        return Err("invalid input".into());
    }
    let tags = d
        .tags
        .iter()
        .map(|t| clean_text(t, 1, 40))
        .collect::<Result<Vec<_>, _>>()?;
    let text = if d.text.trim().is_empty() {
        String::new()
    } else {
        clean_text(&d.text, 1, 2000)?
    };
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(dir), MAX_BYTES)?;
    if store.entries.len() >= MAX_ENTRIES {
        return Err("The check-in log is full.".into());
    }
    let r = CheckInRecord {
        id: new_id("c"),
        at: iso(&now),
        day: day_of(&now).format("%Y-%m-%d").to_string(),
        mood: d.mood,
        energy: d.energy,
        tags,
        text,
        transcribed: d.transcribed,
    };
    store.entries.insert(0, r.clone());
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    if let Some(data) = super::amara_store::data_above(dir, "checkins") {
        super::amara_store::note(
            data,
            super::amara_store::NewActivity {
                scope: super::amara_store::Scope::Personal,
                kind: "checkin.logged",
                subject_ref: &format!("checkin:{}", r.id),
                occurred_at: &r.at,
                source: "user",
                source_ref: None,
                actor: "taisun",
                summary: None,
                data_json: None,
            },
        );
    }
    Ok(r)
}

/// Newest first.
pub(crate) fn list_in(dir: &Path, limit: usize) -> Result<Vec<CheckInRecord>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(dir), MAX_BYTES)?;
    Ok(store.entries.into_iter().take(limit).collect())
}

pub(crate) fn done_on(dir: &Path, day: NaiveDate) -> Result<bool, String> {
    let want = day.format("%Y-%m-%d").to_string();
    Ok(list_in(dir, 1)?.first().is_some_and(|e| e.day == want))
}

fn dir() -> PathBuf {
    dir_in(&crate::app_paths::data_dir())
}

#[tauri::command]
#[specta::specta]
pub async fn checkin_save(draft: CheckInDraft) -> Result<CheckInRecord, String> {
    tauri::async_runtime::spawn_blocking(move || save_in(&dir(), &draft, now_jst()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn checkin_list(limit: Option<usize>) -> Result<Vec<CheckInRecord>, String> {
    let n = limit.unwrap_or(30).clamp(1, 365);
    tauri::async_runtime::spawn_blocking(move || list_in(&dir(), n))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;

    fn draft(text: &str) -> CheckInDraft {
        CheckInDraft {
            mood: 4,
            energy: 3,
            tags: vec!["training".into()],
            text: text.into(),
            transcribed: false,
        }
    }

    #[test]
    fn saves_text_only_lists_newest_first_and_knows_done_today() {
        let data = temp_dir("ci");
        let dir = dir_in(&data);
        let t = now_jst();
        let a = save_in(&dir, &draft("one"), t).unwrap();
        let b = save_in(&dir, &draft("two"), t).unwrap();
        let l = list_in(&dir, 10).unwrap();
        assert_eq!(l.iter().map(|x| &x.id).collect::<Vec<_>>(), [&b.id, &a.id]);
        assert_eq!(list_in(&dir, 1).unwrap().len(), 1);
        assert!(done_on(&dir, day_of(&t)).unwrap());
        assert!(!done_on(&dir, day_of(&t) + chrono::Duration::days(1)).unwrap());
        // No audio field exists anywhere in the stored record.
        let raw = std::fs::read_to_string(file(&dir)).unwrap();
        assert!(!raw.to_lowercase().contains("audio"));
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn rejects_out_of_range_scores_and_oversize_text() {
        let data = temp_dir("civ");
        let dir = dir_in(&data);
        let mut d = draft("x");
        d.mood = 0;
        assert!(save_in(&dir, &d, now_jst()).is_err());
        let mut d = draft("x");
        d.energy = 6;
        assert!(save_in(&dir, &d, now_jst()).is_err());
        assert!(save_in(&dir, &draft(&"x".repeat(2001)), now_jst()).is_err());
        let mut d = draft("x");
        d.tags = vec!["t".into(); 9];
        assert!(save_in(&dir, &d, now_jst()).is_err());
        assert!(list_in(&dir, 5).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(data);
    }
    /// A voice check-in is its transcript only: the store holds text, and nothing audio is ever written.
    #[test]
    fn a_transcribed_check_in_writes_text_only() {
        let d = super::super::local_store::tests::temp_dir("cin-audio");
        let draft = CheckInDraft {
            mood: 4,
            energy: 3,
            tags: vec![],
            text: "slept ok".into(),
            transcribed: true,
        };
        let r = save_in(&d, &draft, now_jst()).unwrap();
        assert!(r.transcribed);
        let mut names: Vec<String> = Vec::new();
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            names.push(e.file_name().to_string_lossy().into());
        }
        assert_eq!(names, vec!["checkins.json".to_string()]);
        let src = include_str!("checkin.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for needle in ["cpal", "hound", ".wav", "ffmpeg", "AudioContext"] {
            assert!(!prod.contains(needle), "{needle}");
        }
    }
}
