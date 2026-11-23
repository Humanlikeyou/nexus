//! The TakeLog: Amara's append-only memory of her takes (docs/design/HANDOFF-1111t.md §2, §11 step 1).
//!
//! Storage decision: a JSONL file `{app data}/takes/take_log.jsonl`, owned by Rust, not a kv record.
//! kv is a whole-file overwrite (`save_kv`), so two writers (the TS daily take and the Rust Home
//! heart-rate take) doing read-modify-write would lose entries. Here every write holds one process
//! mutex, an append is one `write` of one line, and `forget` rewrites through a temp file + rename.
//!
//! The log holds the user's own take text, not health samples: no bpm series, no raw data. Never log
//! entry text. Retention (365 days) is computed by readers; nothing here deletes by age.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const PAGES: [&str; 8] = [
    "fitness",
    "neuro",
    "mindBody",
    "business",
    "home",
    "life",
    "agents",
    "smarthome",
];
const MAX_TEXT_CHARS: usize = 2000;
const MAX_ID_CHARS: usize = 120;

/// One mutex for every writer in this process (append, forget, migrate).
static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Mirrors `TakeLogEntry` in HANDOFF-1111t §2. `hr` is an optional extra for Home heart-rate takes
/// (window summary, memory line and question); History ignores it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TakeLogEntry {
    pub id: String,
    pub page: String,
    pub day: String,
    pub generated_at: i64,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub cited: Vec<Value>,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub trial_id: Option<String>,
    #[serde(default)]
    pub memory_line: Option<String>,
    #[serde(default)]
    pub forgotten_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hr: Option<Value>,
}

pub(crate) fn log_dir() -> PathBuf {
    crate::app_paths::data_dir().join("takes")
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("take_log.jsonl")
}

fn valid(entry: &TakeLogEntry) -> Result<(), String> {
    if !PAGES.contains(&entry.page.as_str()) {
        return Err("unknown page".into());
    }
    if entry.id.is_empty() || entry.id.chars().count() > MAX_ID_CHARS {
        return Err("bad id".into());
    }
    if entry.kind != "daily" && entry.kind != "followup" {
        return Err("bad kind".into());
    }
    if entry.text.chars().count() > MAX_TEXT_CHARS {
        return Err("text too long".into());
    }
    Ok(())
}

fn read_all_in(dir: &Path) -> Vec<TakeLogEntry> {
    let Ok(text) = fs::read_to_string(log_path(dir)) else {
        return Vec::new();
    };
    // A damaged line is skipped, never fatal: the rest of the memory still loads.
    text.lines()
        .filter_map(|line| serde_json::from_str::<TakeLogEntry>(line).ok())
        .collect()
}

fn append_locked(dir: &Path, entry: &TakeLogEntry) -> Result<bool, String> {
    valid(entry)?;
    if !dir.exists() {
        crate::app_paths::create_private_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if read_all_in(dir).iter().any(|e| e.id == entry.id) {
        return Ok(false);
    }
    let mut line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = crate::app_paths::private_open_options()
        .append(true)
        .create(true)
        .open(log_path(dir))
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Appends one entry. Returns false (and writes nothing) when an entry with this id exists.
pub(crate) fn append_in(dir: &Path, entry: &TakeLogEntry) -> Result<bool, String> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    append_locked(dir, entry)
}

pub(crate) fn list_in(dir: &Path) -> Vec<TakeLogEntry> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    read_all_in(dir)
}

/// The user's explicit "forget this take": blanks `text` and `cited`, sets `forgottenAt` and keeps
/// the row (HANDOFF-1111t §2). Returns false when the id is unknown.
pub(crate) fn forget_in(dir: &Path, id: &str, now_ms: i64) -> Result<bool, String> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut entries = read_all_in(dir);
    let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
        return Ok(false);
    };
    if entry.forgotten_at.is_none() {
        entry.text = String::new();
        entry.cited = Vec::new();
        entry.memory_line = None;
        entry.hr = None;
        entry.forgotten_at = Some(now_ms);
    }
    let mut out = String::new();
    for e in &entries {
        out.push_str(&serde_json::to_string(e).map_err(|e| e.to_string())?);
        out.push('\n');
    }
    let tmp = dir.join("take_log.jsonl.tmp");
    crate::app_paths::write_private(&tmp, out).map_err(|e| e.to_string())?;
    fs::rename(&tmp, log_path(dir)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Copies today's kv `amara_take_<page>` records (the W12d daily cache) into the log. Read-only on
/// kv: the old keys stay. The id is `<page>-<generatedAt>`, the same id `generate.ts` uses, so
/// running this again, or appending the same take later, never duplicates. Returns entries added.
pub(crate) fn migrate_in(dir: &Path, kv_dir: &Path) -> Result<usize, String> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut added = 0;
    for page in PAGES {
        let Ok(raw) = fs::read_to_string(kv_dir.join(format!("amara_take_{page}.json"))) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(t) = v.get("take").filter(|t| t.is_object()) else {
            continue;
        };
        let (Some(text), Some(day), Some(at)) = (
            t.get("text").and_then(Value::as_str),
            t.get("day").and_then(Value::as_str),
            t.get("generatedAt").and_then(Value::as_i64),
        ) else {
            continue;
        };
        let entry = TakeLogEntry {
            id: format!("{page}-{at}"),
            page: page.to_string(),
            day: day.to_string(),
            generated_at: at,
            kind: "daily".into(),
            text: text.to_string(),
            cited: t
                .get("cited")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            fingerprint: t
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tier: "migrated".into(),
            trial_id: None,
            memory_line: None,
            forgotten_at: None,
            hr: None,
        };
        if append_locked(dir, &entry).unwrap_or(false) {
            added += 1;
        }
    }
    Ok(added)
}

#[tauri::command]
#[specta::specta]
pub async fn take_log_append(entry: TakeLogEntry) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || append_in(&log_dir(), &entry))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn take_log_list() -> Result<Vec<TakeLogEntry>, String> {
    tauri::async_runtime::spawn_blocking(|| list_in(&log_dir()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn take_log_forget(id: String) -> Result<bool, String> {
    let now = chrono::Utc::now().timestamp_millis();
    tauri::async_runtime::spawn_blocking(move || forget_in(&log_dir(), &id, now))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn take_log_migrate() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(|| migrate_in(&log_dir(), &super::kv::kv_root()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("takelog-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    pub(crate) fn entry(id: &str, page: &str, at: i64, text: &str) -> TakeLogEntry {
        TakeLogEntry {
            id: id.into(),
            page: page.into(),
            day: "2026-11-11".into(),
            generated_at: at,
            kind: "daily".into(),
            text: text.into(),
            cited: vec![serde_json::json!({"key": "k"})],
            fingerprint: "fp".into(),
            tier: "light".into(),
            trial_id: None,
            memory_line: None,
            forgotten_at: None,
            hr: None,
        }
    }

    #[test]
    fn append_is_ordered_and_idempotent_by_id() {
        let d = temp_dir("append");
        assert!(append_in(&d, &entry("a", "home", 1, "one")).unwrap());
        assert!(append_in(&d, &entry("b", "home", 2, "two")).unwrap());
        assert!(!append_in(&d, &entry("a", "home", 3, "dupe")).unwrap());
        let all = list_in(&d);
        assert_eq!(
            all.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn rejects_unknown_page_kind_and_oversize_text() {
        let d = temp_dir("valid");
        assert!(append_in(&d, &entry("a", "nope", 1, "x")).is_err());
        let mut e = entry("b", "home", 1, "x");
        e.kind = "other".into();
        assert!(append_in(&d, &e).is_err());
        assert!(append_in(&d, &entry("c", "home", 1, &"x".repeat(2001))).is_err());
        assert!(list_in(&d).is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn concurrent_appends_lose_nothing() {
        let d = temp_dir("conc");
        let handles: Vec<_> = (0..16)
            .map(|i| {
                let d = d.clone();
                std::thread::spawn(move || {
                    append_in(&d, &entry(&format!("e{i}"), "home", i, "t")).unwrap()
                })
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap());
        }
        assert_eq!(list_in(&d).len(), 16);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn forget_blanks_one_entry_and_keeps_the_row() {
        let d = temp_dir("forget");
        append_in(&d, &entry("a", "home", 1, "keep me")).unwrap();
        append_in(&d, &entry("b", "home", 2, "forget me")).unwrap();
        assert!(forget_in(&d, "b", 99).unwrap());
        assert!(!forget_in(&d, "missing", 99).unwrap());
        let all = list_in(&d);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "keep me");
        assert_eq!(all[1].text, "");
        assert!(all[1].cited.is_empty());
        assert_eq!(all[1].forgotten_at, Some(99));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn migrate_copies_kv_records_once_and_never_touches_kv() {
        let d = temp_dir("mig");
        let kv = temp_dir("migkv");
        let rec = r#"{"day":"2026-11-11","attempts":1,"fingerprint":"fp","take":{"page":"fitness","day":"2026-11-11","text":"Sleep ran 7.2 h.","cited":[{"key":"sleep","label":"Sleep","display":"7.2 h","baseline":null}],"fingerprint":"fp","generatedAt":1762800000000}}"#;
        fs::write(kv.join("amara_take_fitness.json"), rec).unwrap();
        fs::write(kv.join("amara_take_neuro.json"), "not json").unwrap();
        assert_eq!(migrate_in(&d, &kv).unwrap(), 1);
        assert_eq!(migrate_in(&d, &kv).unwrap(), 0);
        let all = list_in(&d);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "fitness-1762800000000");
        assert_eq!(all[0].text, "Sleep ran 7.2 h.");
        assert_eq!(all[0].cited.len(), 1);
        assert_eq!(
            fs::read_to_string(kv.join("amara_take_fitness.json")).unwrap(),
            rec
        );
        // the same take appended later by generate.ts (same id) is not duplicated
        assert!(!append_in(
            &d,
            &entry("fitness-1762800000000", "fitness", 1762800000000, "x")
        )
        .unwrap());
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(kv);
    }

    #[test]
    fn agents_and_smarthome_takes_are_logged_and_migrated() {
        let d = temp_dir("newpages");
        let kv = temp_dir("newpageskv");
        assert!(append_in(&d, &entry("agents-1", "agents", 1, "fixture a")).unwrap());
        assert!(append_in(&d, &entry("smarthome-2", "smarthome", 2, "fixture s")).unwrap());
        assert!(append_in(&d, &entry("x-3", "garage", 3, "nope")).is_err());
        let rec = r#"{"day":"2026-11-11","attempts":1,"fingerprint":"fp","take":{"page":"smarthome","day":"2026-11-11","text":"Fixture.","cited":[],"fingerprint":"fp","generatedAt":1762800000001}}"#;
        fs::write(kv.join("amara_take_smarthome.json"), rec).unwrap();
        assert_eq!(migrate_in(&d, &kv).unwrap(), 1);
        assert_eq!(list_in(&d).len(), 3);
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(kv);
    }

    #[test]
    fn a_damaged_line_is_skipped() {
        let d = temp_dir("bad");
        append_in(&d, &entry("a", "home", 1, "ok")).unwrap();
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(log_path(&d))
            .unwrap();
        f.write_all(b"{broken\n").unwrap();
        append_in(&d, &entry("b", "home", 2, "ok2")).unwrap();
        assert_eq!(list_in(&d).len(), 2);
        let _ = fs::remove_dir_all(d);
    }
}
