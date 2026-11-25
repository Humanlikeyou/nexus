use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn goals_path() -> PathBuf {
    nexus_data_dir().join("goals.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub category: String,
    pub target: f64,
    pub current: f64,
    pub unit: String,
    pub deadline: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Optional link to a business-ledger venture (decision 0037). Old files without it load as `None`.
    /// Set only through `goal_venture_set`, which checks the ledger and logs History + Undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venture_id: Option<String>,
    /// Habits only (decision 0045 item 3): why this habit matters to Taisun, in his words. Old files load as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Habits only: the `HH:MM` JST time Needs you may start asking whether today is marked. `None` = 20:00.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_at: Option<String>,
    /// Habits only: a paused habit is not asked about and does not count as missed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

#[tauri::command]
#[specta::specta]
pub fn read_goals() -> Result<Vec<Goal>, String> {
    let path = goals_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path).map_err(|e| format!("Failed to read goals: {}", e))?;
    let goals: Vec<Goal> =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse goals: {}", e))?;
    Ok(goals)
}

#[tauri::command]
#[specta::specta]
pub fn save_goals(goals: Vec<Goal>) -> Result<(), String> {
    save_goals_at(&goals_path(), goals)
}

/// The venture link never comes from the caller: each goal takes `venture_id` from the goal with the same id
/// on disk (None when there is no match or the file can't be read). Only `goal_venture_set` changes a link.
///
/// 0053 step 2: under the History store lock; an existing file that can't be read is never
/// overwritten (it is kept aside and the save is refused); the write is atomic (temp + rename, 0600)
/// and read back before it counts as saved.
pub(crate) fn save_goals_at(path: &std::path::Path, goals: Vec<Goal>) -> Result<(), String> {
    let _lock = super::adds::store_lock();
    let old: Vec<Goal> = read_existing(path)?;
    let mut goals = goals;
    let stamp = crate::commands::shared::time::now_jst().to_rfc3339();
    for g in goals.iter_mut() {
        // A row written by the console carries no timestamps; the habit tracker needs the day it was created.
        let old_row = old.iter().find(|o| o.id == g.id);
        if g.created_at.is_empty() {
            g.created_at = old_row.map_or_else(|| stamp.clone(), |o| o.created_at.clone());
        }
        if g.updated_at.is_empty() {
            g.updated_at = stamp.clone();
        }
        g.remind_at = g.remind_at.take().and_then(|t| valid_hhmm(&t));
        g.why = g
            .why
            .take()
            .map(|w| {
                w.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect::<String>()
            })
            .filter(|w| !w.is_empty());
        g.venture_id = old
            .iter()
            .find(|o| o.id == g.id)
            .and_then(|o| o.venture_id.clone());
    }
    super::local_store::write_json_atomic(path, &goals, super::adds::MAX_MD_BYTES)
        .map_err(|e| format!("Failed to write goals: {e}"))?;
    // Read back: the save counts only if the file on disk holds exactly these goals.
    let back: Vec<Goal> = super::local_store::read_json(path, super::adds::MAX_MD_BYTES)
        .map_err(|e| format!("Goals were written but can't be read back: {e}"))?;
    let ids = |g: &[Goal]| g.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
    if ids(&back) != ids(&goals) {
        return Err("Goals were written but the file doesn't match what was saved.".into());
    }
    if let Some(data) = path.parent() {
        note_goal_activity(data, &old, &goals);
    }
    Ok(())
}

/// 0053 step 4: what changed between the old and new goal lists, as activity rows. Habits have
/// their own day marks (habits.rs), so they are skipped here.
fn note_goal_activity(data: &std::path::Path, old: &[Goal], new: &[Goal]) {
    use super::amara_store::{note, NewActivity, Scope};
    for g in new.iter().filter(|g| g.category != "habit") {
        let prev = old.iter().find(|o| o.id == g.id);
        // Created: when the goal was made. Progress: now (the console may keep the old updated_at).
        let at = match prev {
            None if !g.created_at.is_empty() => g.created_at.clone(),
            _ => crate::commands::shared::time::now_jst().to_rfc3339(),
        };
        let subject = format!("goals:{}", g.id);
        let done = |x: &Goal| x.target > 0.0 && x.current >= x.target;
        let kind = match prev {
            None => Some("goal.created"),
            Some(p) if done(g) && !done(p) => Some("goal.completed"),
            Some(p) if (p.current - g.current).abs() > f64::EPSILON => Some("goal.progressed"),
            _ => None,
        };
        if let Some(kind) = kind {
            let data_json = prev
                .filter(|_| kind == "goal.progressed" || kind == "goal.completed")
                .map(|p| format!(r#"{{"from":{},"to":{}}}"#, p.current, g.current));
            note(
                data,
                NewActivity {
                    certainty: "confirmed",
                    scope: Scope::Personal,
                    kind,
                    subject_ref: &subject,
                    occurred_at: &at,
                    source: "user",
                    source_ref: None,
                    actor: "taisun",
                    summary: None,
                    data_json: data_json.as_deref(),
                },
            );
        }
    }
}

/// The goals already on disk. Missing = none. A file that exists but can't be read or parsed is an
/// error, and a copy is kept beside it, so nothing ever replaces real goals with an empty list.
fn read_existing(path: &std::path::Path) -> Result<Vec<Goal>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Goals couldn't be read, so nothing was saved: {e}")),
    };
    match serde_json::from_str(&text) {
        Ok(g) => Ok(g),
        Err(_) => {
            let kept = keep_unreadable(path, &text);
            Err(format!(
                "Goals couldn't be read, so nothing was saved and the file was left as it is.{}",
                kept.map(|n| format!(" A copy is in {n}."))
                    .unwrap_or_default()
            ))
        }
    }
}

/// `goals.json.unreadable-<timestamp>` (0600), once per distinct content. Returns its file name.
fn keep_unreadable(path: &std::path::Path, text: &str) -> Option<String> {
    let dir = path.parent()?;
    use sha2::Digest;
    let hash = &hex::encode(sha2::Sha256::digest(text.as_bytes()))[..12];
    let name = format!("goals.json.unreadable-{hash}");
    let copy = dir.join(&name);
    if !copy.exists() {
        crate::app_paths::write_private(&copy, text).ok()?;
    }
    Some(name)
}

/// `HH:MM` (24 h) or nothing: a reminder time that is not a real clock time is dropped, never stored.
pub(crate) fn valid_hhmm(t: &str) -> Option<String> {
    let t = t.trim();
    let (h, m) = t.split_once(':')?;
    let ok = h.len() == 2
        && m.len() == 2
        && h.parse::<u32>().is_ok_and(|h| h < 24)
        && m.parse::<u32>().is_ok_and(|m| m < 60);
    ok.then(|| t.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_goal(goal_id: String) -> Result<Vec<Goal>, String> {
    let mut goals = read_goals()?;
    goals.retain(|g| g.id != goal_id);
    save_goals(goals.clone())?;
    // 0053: deleting a goal also removes the activity and evidence that point at it.
    super::amara_store::forget_record(&nexus_data_dir(), &format!("goals:{goal_id}"));
    Ok(goals)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("nexus-goals-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn goal(id: &str) -> Goal {
        Goal {
            id: id.into(),
            title: format!("Goal {id}"),
            category: "life".into(),
            target: 10.0,
            current: 1.0,
            unit: "x".into(),
            deadline: None,
            created_at: String::new(),
            updated_at: String::new(),
            venture_id: None,
            why: None,
            remind_at: None,
            paused: false,
        }
    }

    #[test]
    fn an_unreadable_goals_file_is_never_overwritten_and_a_copy_is_kept() {
        let t = tmp("corrupt");
        let path = t.0.join("goals.json");
        let broken = "[{\"id\": \"g1\", \"title\": \"Run a marathon\"  <truncated";
        fs::write(&path, broken).unwrap();
        let e = save_goals_at(&path, vec![]).unwrap_err();
        assert!(e.contains("nothing was saved"), "{e}");
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        let copies: Vec<_> = fs::read_dir(&t.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("goals.json.unreadable-")
            })
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read_to_string(copies[0].path()).unwrap(), broken);
        // A second attempt keeps one copy, not two.
        let _ = save_goals_at(&path, vec![goal("g2")]);
        assert_eq!(
            fs::read_dir(&t.0)
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("goals.json.unreadable-"))
                .count(),
            1
        );
    }

    #[test]
    fn a_save_is_atomic_private_and_read_back() {
        let t = tmp("ok");
        let path = t.0.join("goals.json");
        save_goals_at(&path, vec![goal("g1"), goal("g2")]).unwrap();
        let back: Vec<Goal> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            back.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["g1", "g2"]
        );
        assert!(!back[0].created_at.is_empty());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // No temp file is left behind (amara.db beside it is the activity store, 0053 step 4).
        let leftovers: Vec<String> = fs::read_dir(&t.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("goals") && n != "goals.json")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn concurrent_saves_never_leave_a_torn_file() {
        let t = tmp("race");
        let path = t.0.join("goals.json");
        std::thread::scope(|s| {
            // Light on purpose: the History lock is shared with every other test in the process.
            for i in 0..4 {
                let path = &path;
                s.spawn(move || {
                    for n in 0..5 {
                        let list: Vec<Goal> = (0..=(i + n) % 7)
                            .map(|k| goal(&format!("g{i}-{k}")))
                            .collect();
                        save_goals_at(path, list).unwrap();
                    }
                });
            }
        });
        let back: Vec<Goal> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(!back.is_empty());
    }

    #[test]
    fn a_missing_file_is_an_empty_start_not_an_error() {
        let t = tmp("missing");
        save_goals_at(&t.0.join("goals.json"), vec![goal("g1")]).unwrap();
    }
}
