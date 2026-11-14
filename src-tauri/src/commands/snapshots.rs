use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn snapshots_dir_in(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("snapshots")
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BrainSnapshot {
    pub id: String,
    pub timestamp: String,
    pub fatigue: f64,
    pub engagement: f64,
    #[serde(rename = "alphaPeak")]
    pub alpha_peak: f64,
    #[serde(rename = "betaBalance")]
    pub beta_balance: f64,
    #[serde(rename = "focusScore")]
    pub focus_score: f64,
    #[serde(rename = "relaxationScore")]
    pub relaxation_score: f64,
}

#[tauri::command]
#[specta::specta]
pub fn save_snapshot(snapshot: BrainSnapshot) -> Result<String, String> {
    save_snapshot_at(&nexus_data_dir(), snapshot)
}

/// `save_snapshot` with the app data dir passed in (tests use a temp dir, never HOME).
fn save_snapshot_at(data_dir: &std::path::Path, snapshot: BrainSnapshot) -> Result<String, String> {
    let dir = snapshots_dir_in(data_dir);
    crate::app_paths::create_private_dir_all(&dir)
        .map_err(|e| format!("Failed to create snapshots dir: {}", e))?;

    save_snapshot_in(&dir, &snapshot).map(|path| path.to_string_lossy().to_string())
}

/// Unique, filesystem-safe snapshot file name: sanitized timestamp plus a
/// random suffix, so two snapshots in the same second never collide.
fn snapshot_filename(timestamp: &str) -> String {
    let safe: String = timestamp
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('.');
    let safe = if safe.is_empty() { "snapshot" } else { safe };
    format!(
        "{}_{}.json",
        safe,
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

fn save_snapshot_in(dir: &std::path::Path, snapshot: &BrainSnapshot) -> Result<PathBuf, String> {
    use std::io::Write;
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    // create_new guarantees we never overwrite an existing snapshot.
    for _ in 0..4 {
        let path = dir.join(snapshot_filename(&snapshot.timestamp));
        match crate::app_paths::private_open_options()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(json.as_bytes())
                    .map_err(|e| format!("Failed to write snapshot: {}", e))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Failed to write snapshot: {}", e)),
        }
    }
    Err("Failed to allocate a unique snapshot file name".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_second_snapshots_get_distinct_files() {
        let dir =
            std::env::temp_dir().join(format!("nexus-snapfix-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        let snap = BrainSnapshot {
            id: "a".into(),
            timestamp: "2026-10-21T18:00:00Z".into(),
            fatigue: 0.1,
            engagement: 0.2,
            alpha_peak: 10.0,
            beta_balance: 0.5,
            focus_score: 0.6,
            relaxation_score: 0.7,
        };
        let a = save_snapshot_in(&dir, &snap).unwrap();
        let b = save_snapshot_in(&dir, &snap).unwrap();
        assert_ne!(a, b);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        assert!(a.starts_with(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_filename_is_sanitized() {
        let name = snapshot_filename("../../etc/passwd");
        assert!(!name.contains('/') && !name.starts_with('.'));
        assert!(name.ends_with(".json"));
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_snapshots() -> Result<Vec<BrainSnapshot>, String> {
    list_snapshots_in(&nexus_data_dir())
}

/// `list_snapshots` with the app data dir passed in.
fn list_snapshots_in(data_dir: &std::path::Path) -> Result<Vec<BrainSnapshot>, String> {
    let dir = snapshots_dir_in(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        match serde_json::from_str::<BrainSnapshot>(&data) {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(_) => continue,
        }
    }

    // Sort by timestamp descending
    snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(snapshots)
}

/// Failing repro for TEST_PLAN bug #8 (same-second snapshot overwrite).
#[cfg(test)]
mod repro_tests {
    use super::*;

    fn snap(id: &str, ts: &str) -> BrainSnapshot {
        BrainSnapshot {
            id: id.into(),
            timestamp: ts.into(),
            fatigue: 0.1,
            engagement: 0.2,
            alpha_peak: 10.0,
            beta_balance: 0.5,
            focus_score: 0.6,
            relaxation_score: 0.7,
        }
    }

    /// The data dir is passed in, so no process-wide env (HOME) is touched and the test runs
    /// in parallel with everything else.
    #[test]
    fn repro_same_second_snapshots_are_both_kept() {
        let base = std::env::temp_dir().join(format!("nexus-snap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let a = save_snapshot_at(&base, snap("a", "2026-10-21T18:00:00Z"));
        let b = save_snapshot_at(&base, snap("b", "2026-10-21T18:00:00Z"));
        let listed = list_snapshots_in(&base);
        let _ = std::fs::remove_dir_all(&base);
        assert!(
            a.unwrap().starts_with(base.to_str().unwrap()),
            "snapshot escaped the temp data dir"
        );
        b.unwrap();
        assert_eq!(
            listed.unwrap().len(),
            2,
            "second snapshot overwrote the first"
        );
    }
}
