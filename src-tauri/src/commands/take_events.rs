//! Append-only event store for suggestions, answers and settings (HANDOFF-1111t §11 steps 2-5).
//!
//! Why a second JSONL file and not the take log or kv: the take log holds Amara's words and `forget`
//! blanks them; answers are the user's own history and must survive a forgotten take. kv is a whole-file
//! overwrite, and answers have several writers (a tap in the webview, a detector run), so a
//! read-modify-write would lose events. Here one process mutex guards every write, an append is one
//! line, and the only rewrite (cap pruning) goes through a temp file and rename.
//!
//! Events are small JSON facts: no health samples, no text of takes. The reducer lives in TypeScript
//! (`takes/events.ts`); this file only validates, appends, lists and keeps the file bounded.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KINDS: [&str; 8] = [
    "today",
    "answer",
    "undo",
    "setting",
    "type_action",
    "ledger_seen",
    "take_shown",
    "take_rating",
];
const MAX_ID_CHARS: usize = 160;
const MAX_EVENT_BYTES: usize = 4096;
/// Hard cap on stored events. At the cap, events older than `PRUNE_AFTER_DAYS` are dropped atomically;
/// if the file is still full the append fails (it never silently discards recent history).
const MAX_EVENTS: usize = 50_000;
const PRUNE_AFTER_DAYS: i64 = 400;
const DAY_MS: i64 = 86_400_000;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TakeEvent {
    pub id: String,
    pub kind: String,
    /// ms since epoch.
    pub at: i64,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub day: Option<String>,
    #[serde(default)]
    pub data: Value,
}

fn dir() -> PathBuf {
    crate::app_paths::data_dir().join("takes")
}

fn path(dir: &Path) -> PathBuf {
    dir.join("take_events.jsonl")
}

fn valid(e: &TakeEvent) -> Result<(), String> {
    if !KINDS.contains(&e.kind.as_str()) {
        return Err("unknown event kind".into());
    }
    if e.id.is_empty() || e.id.chars().count() > MAX_ID_CHARS {
        return Err("bad id".into());
    }
    if let Some(p) = &e.page {
        if !super::take_log::PAGES.contains(&p.as_str()) {
            return Err("unknown page".into());
        }
    }
    if e.kind == "take_shown" || e.kind == "take_rating" {
        valid_take_fact(e)?;
    }
    if serde_json::to_string(e).map_err(|x| x.to_string())?.len() > MAX_EVENT_BYTES {
        return Err("event too large".into());
    }
    Ok(())
}

/// `take_shown` data is exactly `{takeId}`; `take_rating` is exactly `{takeId, helpful}`. Both carry a page and a
/// `YYYY-MM-DD` day (the daily table counts them per day). Nothing else may ride along.
fn valid_take_fact(e: &TakeEvent) -> Result<(), String> {
    if e.page.is_none() {
        return Err("take event needs a page".into());
    }
    let day_ok = e.day.as_deref().is_some_and(|d| {
        let b = d.as_bytes();
        b.len() == 10
            && b.iter().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    *c == b'-'
                } else {
                    c.is_ascii_digit()
                }
            })
    });
    if !day_ok {
        return Err("take event needs a YYYY-MM-DD day".into());
    }
    let obj = e
        .data
        .as_object()
        .ok_or("take event data must be an object")?;
    let want = if e.kind == "take_rating" { 2 } else { 1 };
    if obj.len() != want {
        return Err("unexpected take event data keys".into());
    }
    match obj.get("takeId").and_then(Value::as_str) {
        Some(t)
            if (1..=MAX_ID_CHARS).contains(&t.chars().count())
                && !t.chars().any(char::is_control) => {}
        _ => return Err("bad takeId".into()),
    }
    if e.kind == "take_rating" && !obj.get("helpful").is_some_and(Value::is_boolean) {
        return Err("helpful must be a boolean".into());
    }
    Ok(())
}

fn read_all(dir: &Path) -> Vec<TakeEvent> {
    let Ok(text) = fs::read_to_string(path(dir)) else {
        return Vec::new();
    };
    // A damaged line is skipped, never fatal.
    text.lines()
        .filter_map(|l| serde_json::from_str::<TakeEvent>(l).ok())
        .collect()
}

fn rewrite(dir: &Path, events: &[TakeEvent]) -> Result<(), String> {
    let mut out = String::new();
    for e in events {
        out.push_str(&serde_json::to_string(e).map_err(|x| x.to_string())?);
        out.push('\n');
    }
    let tmp = dir.join("take_events.jsonl.tmp");
    crate::app_paths::write_private(&tmp, out).map_err(|x| x.to_string())?;
    fs::rename(&tmp, path(dir)).map_err(|x| x.to_string())
}

/// Appends one event. Returns false (writes nothing) when the id already exists.
pub(crate) fn append_in(dir: &Path, e: &TakeEvent, now_ms: i64) -> Result<bool, String> {
    valid(e)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !dir.exists() {
        crate::app_paths::create_private_dir_all(dir).map_err(|x| x.to_string())?;
    }
    let mut all = read_all(dir);
    if all.iter().any(|x| x.id == e.id) {
        return Ok(false);
    }
    if all.len() >= MAX_EVENTS {
        let cutoff = now_ms - PRUNE_AFTER_DAYS * DAY_MS;
        all.retain(|x| x.at >= cutoff);
        if all.len() >= MAX_EVENTS {
            return Err("event store full".into());
        }
        rewrite(dir, &all)?;
    }
    let mut line = serde_json::to_string(e).map_err(|x| x.to_string())?;
    line.push('\n');
    let mut f = crate::app_paths::private_open_options()
        .append(true)
        .create(true)
        .open(path(dir))
        .map_err(|x| x.to_string())?;
    f.write_all(line.as_bytes()).map_err(|x| x.to_string())?;
    Ok(true)
}

pub(crate) fn list_in(dir: &Path) -> Vec<TakeEvent> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    read_all(dir)
}

#[tauri::command]
pub async fn take_events_append(event: TakeEvent) -> Result<bool, String> {
    let now = chrono::Utc::now().timestamp_millis();
    tauri::async_runtime::spawn_blocking(move || append_in(&dir(), &event, now))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn take_events_list() -> Result<Vec<TakeEvent>, String> {
    tauri::async_runtime::spawn_blocking(|| list_in(&dir()))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("takeev-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn ev(id: &str, kind: &str, at: i64) -> TakeEvent {
        TakeEvent {
            id: id.into(),
            kind: kind.into(),
            at,
            page: Some("fitness".into()),
            day: Some("2026-11-12".into()),
            data: serde_json::json!({"state": "done"}),
        }
    }

    #[test]
    fn append_is_ordered_and_idempotent() {
        let d = tmp("a");
        assert!(append_in(&d, &ev("a", "answer", 1), 10).unwrap());
        assert!(append_in(&d, &ev("b", "undo", 2), 10).unwrap());
        assert!(!append_in(&d, &ev("a", "answer", 3), 10).unwrap());
        let ids: Vec<_> = list_in(&d).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn rejects_bad_kind_page_and_oversize() {
        let d = tmp("b");
        assert!(append_in(&d, &ev("a", "transfer", 1), 10).is_err());
        let mut e = ev("b", "answer", 1);
        e.page = Some("nope".into());
        assert!(append_in(&d, &e, 10).is_err());
        let mut big = ev("c", "answer", 1);
        big.data = serde_json::json!({"x": "y".repeat(5000)});
        assert!(append_in(&d, &big, 10).is_err());
        assert!(list_in(&d).is_empty());
    }

    #[test]
    fn damaged_line_is_skipped() {
        let d = tmp("c");
        append_in(&d, &ev("a", "answer", 1), 10).unwrap();
        let mut f = fs::OpenOptions::new().append(true).open(path(&d)).unwrap();
        f.write_all(b"{not json\n").unwrap();
        append_in(&d, &ev("b", "answer", 2), 10).unwrap();
        assert_eq!(list_in(&d).len(), 2);
    }

    fn fact(id: &str, kind: &str, data: Value) -> TakeEvent {
        TakeEvent {
            id: id.into(),
            kind: kind.into(),
            at: 1,
            page: Some("fitness".into()),
            day: Some("2026-11-14".into()),
            data,
        }
    }

    #[test]
    fn take_shown_and_rating_accept_the_contract_shape() {
        let d = tmp("d");
        let shown = fact(
            "s1",
            "take_shown",
            serde_json::json!({"takeId": "fitness-1"}),
        );
        let rate = fact(
            "r1",
            "take_rating",
            serde_json::json!({"takeId": "fitness-1", "helpful": false}),
        );
        assert!(append_in(&d, &shown, 10).unwrap());
        assert!(append_in(&d, &rate, 10).unwrap());
        assert_eq!(list_in(&d).len(), 2);
    }

    #[test]
    fn take_shown_and_rating_reject_bad_shapes() {
        let d = tmp("e");
        let long = "x".repeat(161);
        let bad = [
            fact("a", "take_shown", serde_json::json!({})),
            fact(
                "b",
                "take_shown",
                serde_json::json!({"takeId": "t", "extra": 1}),
            ),
            fact("c", "take_shown", serde_json::json!({"takeId": 5})),
            fact("d", "take_shown", serde_json::json!({"takeId": ""})),
            fact("e", "take_shown", serde_json::json!({"takeId": long})),
            fact("f", "take_shown", serde_json::json!({"takeId": "a\nb"})),
            fact(
                "g",
                "take_shown",
                serde_json::json!({"takeId": "t", "helpful": true}),
            ),
            fact("h", "take_shown", serde_json::json!("t")),
            fact("i", "take_rating", serde_json::json!({"takeId": "t"})),
            fact(
                "j",
                "take_rating",
                serde_json::json!({"takeId": "t", "helpful": "yes"}),
            ),
            fact(
                "k",
                "take_rating",
                serde_json::json!({"takeId": "t", "helpful": true, "x": 1}),
            ),
        ];
        for e in &bad {
            assert!(append_in(&d, e, 10).is_err(), "{} should be rejected", e.id);
        }
        let mut no_page = fact("l", "take_shown", serde_json::json!({"takeId": "t"}));
        no_page.page = None;
        assert!(append_in(&d, &no_page, 10).is_err());
        let mut bad_day = fact("m", "take_shown", serde_json::json!({"takeId": "t"}));
        bad_day.day = Some("14-11-2026".into());
        assert!(append_in(&d, &bad_day, 10).is_err());
        bad_day.day = None;
        assert!(append_in(&d, &bad_day, 10).is_err());
        assert!(list_in(&d).is_empty());
    }
}
