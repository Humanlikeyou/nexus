//! The decisions log (design-1111v §4, HANDOFF-1111v §7.1): `<data dir>/decisions/decisions.json`.
//!
//! Separate from the business ledger: this module never opens `business/` and has no money or network
//! code. A record holds the choice, when (JST), who proposed it and a source link; the outcome is filled
//! in later. Append-only: Undo inside 8 s deletes the record, after that a correction is a new record.

use super::local_store::{
    clean_slug, clean_text, iso, new_id, now_jst, read_json, write_json_atomic,
};
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const UNDO_MS: i64 = 8_000;
const MAX_RECORDS: usize = 2000;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct SourceLink {
    pub id: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRecord {
    pub id: String,
    pub item_id: String,
    pub subject: String,
    pub title: String,
    pub choice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub decided_at: String,
    pub proposer: String,
    pub source_link: SourceLink,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionDraft {
    pub item_id: String,
    pub subject: String,
    pub title: String,
    pub choice: String,
    #[serde(default)]
    pub note: Option<String>,
    pub proposer: String,
    pub source_link: SourceLink,
    pub source_label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    records: Vec<DecisionRecord>,
}

pub(crate) fn dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join("decisions")
}

fn file(dir: &Path) -> PathBuf {
    dir.join("decisions.json")
}

fn validate(d: &DecisionDraft) -> Result<DecisionDraft, String> {
    if d.subject != "vote" && d.subject != "tax_flag" {
        return Err("invalid input".into());
    }
    let ok_choice = match d.subject.as_str() {
        "vote" => d.choice == "yes" || d.choice == "no",
        _ => d.choice == "handled",
    };
    if !ok_choice {
        return Err("invalid input".into());
    }
    Ok(DecisionDraft {
        item_id: clean_slug(&d.item_id.to_lowercase(), 64)?,
        subject: d.subject.clone(),
        title: clean_text(&d.title, 1, 200)?,
        choice: d.choice.clone(),
        note: match d.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => Some(clean_text(n, 1, 500)?),
            None => None,
        },
        proposer: clean_text(&d.proposer, 1, 120)?,
        source_link: SourceLink {
            id: clean_slug(&d.source_link.id, 32)?,
            view: clean_slug(&d.source_link.view, 96)?,
        },
        source_label: clean_text(&d.source_label, 1, 120)?,
    })
}

fn load(dir: &Path) -> Result<Store, String> {
    read_json::<Store>(&file(dir), MAX_BYTES)
}

pub(crate) fn list_in(dir: &Path) -> Result<Vec<DecisionRecord>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    Ok(load(dir)?.records)
}

pub(crate) fn append_in(
    dir: &Path,
    draft: &DecisionDraft,
    now: DateTime<FixedOffset>,
) -> Result<DecisionRecord, String> {
    let d = validate(draft)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = load(dir)?;
    if store.records.len() >= MAX_RECORDS {
        return Err("The decisions log is full.".into());
    }
    let r = DecisionRecord {
        id: new_id("d"),
        item_id: d.item_id,
        subject: d.subject,
        title: d.title,
        choice: d.choice,
        note: d.note,
        decided_at: iso(&now),
        proposer: d.proposer,
        source_link: d.source_link,
        source_label: d.source_label,
        outcome: None,
        outcome_at: None,
    };
    store.records.insert(0, r.clone());
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok(r)
}

/// Deletes the record only inside 8 s of `decidedAt`. Returns whether it was removed.
pub(crate) fn undo_in(dir: &Path, id: &str, now: DateTime<FixedOffset>) -> Result<bool, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = load(dir)?;
    let Some(pos) = store.records.iter().position(|r| r.id == id) else {
        return Ok(false);
    };
    let r = &store.records[pos];
    let age = DateTime::parse_from_rfc3339(&r.decided_at)
        .map(|t| now.signed_duration_since(t).num_milliseconds())
        .unwrap_or(i64::MAX);
    if !(0..=UNDO_MS).contains(&age) || r.outcome.is_some() {
        return Ok(false);
    }
    store.records.remove(pos);
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok(true)
}

/// Fills the empty outcome fields. It never rewrites the choice and never overwrites an outcome.
pub(crate) fn outcome_in(
    dir: &Path,
    id: &str,
    outcome: &str,
    now: DateTime<FixedOffset>,
) -> Result<DecisionRecord, String> {
    let text = clean_text(outcome, 1, 500)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = load(dir)?;
    let r = store
        .records
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or("not found")?;
    if r.outcome.is_some() {
        return Err("This decision already has an outcome.".into());
    }
    r.outcome = Some(text);
    r.outcome_at = Some(iso(&now));
    let out = r.clone();
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok(out)
}

fn data_dir() -> PathBuf {
    dir_in(&crate::app_paths::data_dir())
}

#[tauri::command]
#[specta::specta]
pub async fn decisions_list() -> Result<Vec<DecisionRecord>, String> {
    blocking(JoinErr::Plain, || list_in(&data_dir())).await
}

#[tauri::command]
#[specta::specta]
pub async fn decisions_append(draft: DecisionDraft) -> Result<DecisionRecord, String> {
    blocking(JoinErr::Plain, move || {
        append_in(&data_dir(), &draft, now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn decisions_undo(id: String) -> Result<bool, String> {
    blocking(JoinErr::Plain, move || undo_in(&data_dir(), &id, now_jst())).await
}

#[tauri::command]
#[specta::specta]
pub async fn decisions_add_outcome(id: String, outcome: String) -> Result<DecisionRecord, String> {
    blocking(JoinErr::Plain, move || {
        outcome_in(&data_dir(), &id, &outcome, now_jst())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use chrono::Duration;

    fn draft(item: &str, choice: &str) -> DecisionDraft {
        DecisionDraft {
            item_id: item.into(),
            subject: "vote".into(),
            title: "Taiyo Skin · second supplier quote".into(),
            choice: choice.into(),
            note: Some(" why \n ".into()),
            proposer: "Taiyo Skin crew".into(),
            source_link: SourceLink {
                id: "enterprise".into(),
                view: "ventures/taiyo-skin".into(),
            },
            source_label: "Ventures › Taiyo Skin ›".into(),
        }
    }

    #[test]
    fn append_lists_newest_first_and_touches_no_ledger() {
        let data = temp_dir("dec");
        let biz = data.join("business");
        std::fs::create_dir_all(&biz).unwrap();
        let ledger = r#"{"version":1}"#;
        std::fs::write(biz.join("ledger.json"), ledger).unwrap();
        let dir = dir_in(&data);
        let t = now_jst();
        let a = append_in(&dir, &draft("f1", "yes"), t).unwrap();
        let b = append_in(&dir, &draft("t1", "no"), t).unwrap();
        let all = list_in(&dir).unwrap();
        assert_eq!(
            all.iter().map(|r| &r.id).collect::<Vec<_>>(),
            [&b.id, &a.id]
        );
        assert_eq!(a.note.as_deref(), Some("why"));
        assert!(a.decided_at.ends_with("+09:00"));
        // No money moved: the ledger is byte-for-byte what it was, and the only new folder is decisions/.
        assert_eq!(
            std::fs::read_to_string(biz.join("ledger.json")).unwrap(),
            ledger
        );
        let mut names: Vec<_> = std::fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["business", "decisions"]);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn undo_only_inside_eight_seconds() {
        let data = temp_dir("decu");
        let dir = dir_in(&data);
        let t = now_jst();
        let r = append_in(&dir, &draft("f1", "yes"), t).unwrap();
        assert!(!undo_in(&dir, &r.id, t + Duration::milliseconds(8_001)).unwrap());
        assert_eq!(list_in(&dir).unwrap().len(), 1);
        assert!(undo_in(&dir, &r.id, t + Duration::milliseconds(7_900)).unwrap());
        assert!(list_in(&dir).unwrap().is_empty());
        assert!(!undo_in(&dir, "missing", t).unwrap());
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn outcome_fills_once_and_never_rewrites_the_choice() {
        let data = temp_dir("deco");
        let dir = dir_in(&data);
        let t = now_jst();
        let r = append_in(&dir, &draft("f1", "yes"), t).unwrap();
        let o = outcome_in(&dir, &r.id, "Quote accepted", t).unwrap();
        assert_eq!(o.choice, "yes");
        assert_eq!(o.outcome.as_deref(), Some("Quote accepted"));
        assert!(outcome_in(&dir, &r.id, "again", t).is_err());
        assert!(outcome_in(&dir, "nope", "x", t).is_err());
        // Once an outcome exists, Undo no longer deletes it.
        assert!(!undo_in(&dir, &r.id, t).unwrap());
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn rejects_bad_input_and_a_full_log() {
        let data = temp_dir("decv");
        let dir = dir_in(&data);
        let t = now_jst();
        assert!(append_in(&dir, &draft("f1", "maybe"), t).is_err());
        let mut d = draft("f1", "yes");
        d.source_link.view = "../x".into();
        assert!(append_in(&dir, &d, t).is_err());
        let mut d = draft("f1", "yes");
        d.title = "x".repeat(201);
        assert!(append_in(&dir, &d, t).is_err());
        let mut d = draft("t1", "handled");
        d.subject = "tax_flag".into();
        assert!(append_in(&dir, &d, t).is_ok());
        d.choice = "yes".into();
        assert!(append_in(&dir, &d, t).is_err());
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn the_module_has_no_money_or_network_code() {
        let src = include_str!("decisions_log.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        for word in ["business_ledger", "reqwest", "arm::", "TcpStream"] {
            assert!(!body.contains(word), "decisions_log must not use {word}");
        }
    }
}
