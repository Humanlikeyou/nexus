use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap()
                .join("Library/Application Support")
        })
        .join(crate::app_paths::DATA_DIR_NAME)
}

fn snapshots_dir() -> PathBuf {
    nexus_data_dir().join("snapshots")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub fn save_snapshot(snapshot: BrainSnapshot) -> Result<String, String> {
    let dir = snapshots_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create snapshots dir: {}", e))?;

    let filename = format!("{}.json", snapshot.timestamp.replace(':', "-"));
    let path = dir.join(&filename);

    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write snapshot: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn list_snapshots() -> Result<Vec<BrainSnapshot>, String> {
    let dir = snapshots_dir();
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
            id: id.into(), timestamp: ts.into(), fatigue: 0.1, engagement: 0.2, alpha_peak: 10.0,
            beta_balance: 0.5, focus_score: 0.6, relaxation_score: 0.7,
        }
    }

    /// Ignored by default because it sets the process-wide HOME, which would race other
    /// tests. Run with: cargo test repro_ -- --ignored --test-threads=1
    #[test]
    #[ignore = "mutates HOME; run with --ignored --test-threads=1"]
    fn repro_same_second_snapshots_are_both_kept() {
        let home = std::env::temp_dir().join(format!("nexus-snap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let a = save_snapshot(snap("a", "2026-10-21T18:00:00Z"));
        let b = save_snapshot(snap("b", "2026-10-21T18:00:00Z"));
        let listed = list_snapshots();
        match old { Some(v) => std::env::set_var("HOME", v), None => std::env::remove_var("HOME") }
        let _ = std::fs::remove_dir_all(&home);
        assert!(a.unwrap().starts_with(home.to_str().unwrap()), "snapshot escaped temp HOME");
        b.unwrap();
        assert_eq!(listed.unwrap().len(), 2, "second snapshot overwrote the first");
    }
}
