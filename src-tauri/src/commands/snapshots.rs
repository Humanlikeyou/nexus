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
        .join("com.taisun.electron")
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
