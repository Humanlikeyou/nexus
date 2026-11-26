//! Resolves a [`RecordRef`] against the store that owns it (0053, foundations phase).
//!
//! A reference is only evidence if the record really exists. The resolver reads the owning store
//! (read-only, through each store's own reader where one exists) and returns a SHA-256 of the
//! record as read, so a later change to the source is detectable without copying its values.
//! Model-written recommendations may cite only references that resolve to `Found`: a language
//! model cannot invent evidence.

use super::RecordRef;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exists; SHA-256 of the record as read.
    Found { sha256: String },
    /// The store was read and the record isn't there.
    Missing,
    /// This store can't be checked from here (external, or no reader yet).
    Unverifiable,
}

fn sha(v: &Value) -> String {
    hex::encode(Sha256::digest(v.to_string().as_bytes()))
}

fn found_or_missing(v: Option<Value>) -> Resolution {
    match v {
        Some(v) => Resolution::Found { sha256: sha(&v) },
        None => Resolution::Missing,
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The element of `list` whose `"id"` is `id`.
fn by_id(list: Option<&Value>, id: &str) -> Option<Value> {
    list?
        .as_array()?
        .iter()
        .find(|x| x["id"].as_str() == Some(id))
        .cloned()
}

fn jsonl_by_id(path: &Path, id: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["id"].as_str() == Some(id))
}

/// Names that may become a file name under a store folder (no path tricks).
fn plain(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.starts_with('.')
}

/// Looks the record up in its owning store under `data`.
pub(crate) fn resolve(data: &Path, r: &RecordRef) -> Resolution {
    let id = r.id.as_str();
    match r.store.as_str() {
        "goals" => found_or_missing(by_id(read_json(&data.join("goals.json")).as_ref(), id)),
        "habit_day" => {
            // habit_day:<habit id>:<YYYY-MM-DD>
            let Some((habit, day)) = id.rsplit_once(':') else {
                return Resolution::Missing;
            };
            let dir = crate::commands::habits::dir_in(data);
            let rows = crate::commands::habits::rows_in(&dir).unwrap_or_default();
            found_or_missing(
                rows.into_iter()
                    .find(|x| x.habit_id == habit && x.day == day)
                    .and_then(|x| serde_json::to_value(x).ok()),
            )
        }
        "checkin" => found_or_missing(by_id(
            read_json(&crate::commands::checkin::dir_in(data).join("checkins.json"))
                .as_ref()
                .map(|v| &v["entries"]),
            id,
        )),
        "date" => found_or_missing(by_id(
            read_json(&crate::commands::life_dates::dir_in(data).join("dates.json"))
                .as_ref()
                .map(|v| &v["dates"]),
            id,
        )),
        "money" => found_or_missing(jsonl_by_id(&data.join("money").join("entries.jsonl"), id)),
        "take" => found_or_missing(jsonl_by_id(&data.join("takes").join("take_log.jsonl"), id)),
        "workout" => found_or_missing(by_id(
            read_json(&data.join("workout").join("sessions.json"))
                .as_ref()
                .map(|v| &v["sessions"]),
            id,
        )),
        "session" => {
            if !plain(id) {
                return Resolution::Missing;
            }
            found_or_missing(
                read_json(&data.join("sessions").join(format!("{id}.json")))
                    .map(|v| v["session"].clone()),
            )
        }
        "replay" => {
            // Recordings are sacred: only their metadata is read, never their samples.
            if !plain(id) {
                return Resolution::Missing;
            }
            let p = data.join("replays").join(format!("{id}.ndjson"));
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_file() => Resolution::Found {
                    sha256: sha(&serde_json::json!({"recording": id, "bytes": m.len()})),
                },
                _ => Resolution::Missing,
            }
        }
        "execution" => {
            let ledger = crate::commands::execution::Ledger::at(data.join("execution.db"));
            if !ledger.path().exists() {
                return Resolution::Missing;
            }
            match ledger.get(id) {
                Ok(Some(rec)) => Resolution::Found {
                    sha256: sha(
                        &serde_json::json!({"id": rec.execution_id, "status": rec.status.as_str()}),
                    ),
                },
                Ok(None) => Resolution::Missing,
                Err(_) => Resolution::Unverifiable,
            }
        }
        "ledger" => {
            // ledger:<section>:<id>, e.g. ledger:venture:v1
            let Some((section, rid)) = id.split_once(':') else {
                return Resolution::Missing;
            };
            let key = match section {
                "venture" => "ventures",
                "fund" => "funds",
                _ => return Resolution::Missing,
            };
            found_or_missing(by_id(
                read_json(&data.join("business").join("ledger.json"))
                    .as_ref()
                    .map(|v| &v[key]),
                rid,
            ))
        }
        // External (NOOP), SQLite stores without a reader here yet, and derived rows.
        _ => Resolution::Unverifiable,
    }
}
