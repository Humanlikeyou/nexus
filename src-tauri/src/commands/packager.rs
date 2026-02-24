use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join("Library/Application Support"))
        .join("com.taisun.nexus")
}

fn openclaw_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
}

#[derive(Serialize)]
pub struct PackageInfo {
    pub path: String,
    pub size_mb: f64,
    pub includes: Vec<String>,
}

#[tauri::command]
pub async fn pack_it_up() -> Result<PackageInfo, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let desktop = home.join("Desktop");
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive_name = format!("electron-backup-{}.tar.gz", timestamp);
    let archive_path = desktop.join(&archive_name);

    // Create a temp staging directory
    let staging = std::env::temp_dir().join(format!("electron-pack-{}", timestamp));
    fs::create_dir_all(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;

    let mut includes = Vec::new();

    // 1. Copy Nexus config
    let config_src = nexus_data_dir();
    if config_src.exists() {
        let dest = staging.join("electron-config");
        copy_dir_recursive(&config_src, &dest)?;
        includes.push("Electron settings & config".to_string());
    }

    // 2. Copy OpenClaw data
    let openclaw = openclaw_dir();
    if openclaw.exists() {
        let dest = staging.join("openclaw");
        copy_dir_selective(&openclaw, &dest, &[
            "openclaw.json",
            "cron",
            "skills",
            "memory",
            "workspace",
        ])?;
        includes.push("OpenClaw workspace & memory".to_string());
    }

    // 3. Copy local session data (Muse/Whoop recordings)
    let sessions_dir = nexus_data_dir().join("sessions");
    if sessions_dir.exists() {
        let dest = staging.join("sessions");
        copy_dir_recursive(&sessions_dir, &dest)?;
        includes.push("BLE session recordings".to_string());
    }

    // 4. Copy goals
    let goals_file = nexus_data_dir().join("goals.json");
    if goals_file.exists() {
        fs::copy(&goals_file, staging.join("goals.json"))
            .map_err(|e| format!("Failed to copy goals: {}", e))?;
        includes.push("Goals & trajectory data".to_string());
    }

    // 5. Copy events database
    let events_db = nexus_data_dir().join("events.db");
    if events_db.exists() {
        fs::copy(&events_db, staging.join("events.db"))
            .map_err(|e| format!("Failed to copy events.db: {}", e))?;
        // Also copy WAL/SHM files if they exist
        let wal = nexus_data_dir().join("events.db-wal");
        let shm = nexus_data_dir().join("events.db-shm");
        if wal.exists() {
            let _ = fs::copy(&wal, staging.join("events.db-wal"));
        }
        if shm.exists() {
            let _ = fs::copy(&shm, staging.join("events.db-shm"));
        }
        includes.push("Canonical event store".to_string());
    }

    // 5. Write a manifest with metadata
    let manifest = serde_json::json!({
        "version": "0.1.0",
        "packed_at": chrono::Local::now().to_rfc3339(),
        "hostname": hostname::get().map(|h| h.to_string_lossy().to_string()).unwrap_or_default(),
        "includes": &includes,
        "restore_instructions": "To restore: Open Electron on the new machine, go to Settings, click 'Unpack', and select this file."
    });
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(staging.join("manifest.json"), &manifest_json)
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

    // 6. Create tar.gz archive
    let output = Command::new("tar")
        .args([
            "-czf",
            archive_path.to_str().unwrap(),
            "-C",
            staging.parent().unwrap().to_str().unwrap(),
            staging.file_name().unwrap().to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("Failed to create archive: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tar failed: {}", stderr));
    }

    // Cleanup staging
    let _ = fs::remove_dir_all(&staging);

    // Get file size
    let metadata = fs::metadata(&archive_path).map_err(|e| e.to_string())?;
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    if includes.is_empty() {
        includes.push("Base config (no data found to pack yet)".to_string());
    }

    Ok(PackageInfo {
        path: archive_path.to_string_lossy().to_string(),
        size_mb: (size_mb * 100.0).round() / 100.0,
        includes,
    })
}

#[tauri::command]
pub async fn unpack_backup(archive_path: String) -> Result<Vec<String>, String> {
    let archive = PathBuf::from(&archive_path);
    if !archive.exists() {
        return Err("Archive file not found".to_string());
    }

    let staging = std::env::temp_dir().join("electron-unpack");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    // Extract archive
    let output = Command::new("tar")
        .args([
            "-xzf",
            archive.to_str().unwrap(),
            "-C",
            staging.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("Failed to extract: {}", e))?;

    if !output.status.success() {
        return Err(format!("Extract failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Find the extracted directory (should be electron-pack-*)
    let extracted = fs::read_dir(&staging)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("electron-pack"))
        .map(|e| e.path())
        .unwrap_or(staging.clone());

    let mut restored = Vec::new();

    // Restore config
    let config_src = extracted.join("electron-config");
    if config_src.exists() {
        let dest = nexus_data_dir();
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        copy_dir_recursive(&config_src, &dest)?;
        restored.push("Settings & config restored".to_string());
    }

    // Restore OpenClaw
    let openclaw_src = extracted.join("openclaw");
    if openclaw_src.exists() {
        let dest = openclaw_dir();
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        copy_dir_recursive(&openclaw_src, &dest)?;
        restored.push("OpenClaw workspace restored".to_string());
    }

    // Restore sessions
    let sessions_src = extracted.join("sessions");
    if sessions_src.exists() {
        let dest = nexus_data_dir().join("sessions");
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        copy_dir_recursive(&sessions_src, &dest)?;
        restored.push("BLE session data restored".to_string());
    }

    // Restore goals
    let goals_src = extracted.join("goals.json");
    if goals_src.exists() {
        let dest = nexus_data_dir().join("goals.json");
        fs::copy(&goals_src, &dest).map_err(|e| e.to_string())?;
        restored.push("Goals restored".to_string());
    }

    // Restore events database
    let events_src = extracted.join("events.db");
    if events_src.exists() {
        let dest = nexus_data_dir().join("events.db");
        fs::copy(&events_src, &dest).map_err(|e| e.to_string())?;
        // Also restore WAL/SHM if present
        let wal_src = extracted.join("events.db-wal");
        let shm_src = extracted.join("events.db-shm");
        if wal_src.exists() {
            let _ = fs::copy(&wal_src, nexus_data_dir().join("events.db-wal"));
        }
        if shm_src.exists() {
            let _ = fs::copy(&shm_src, nexus_data_dir().join("events.db-shm"));
        }
        restored.push("Event store restored".to_string());
    }

    // Cleanup
    let _ = fs::remove_dir_all(&staging);

    if restored.is_empty() {
        restored.push("Archive was empty — nothing to restore".to_string());
    }

    Ok(restored)
}

#[tauri::command]
pub fn get_app_paths() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "data_dir": nexus_data_dir().to_string_lossy(),
        "openclaw_dir": openclaw_dir().to_string_lossy(),
        "home_dir": dirs::home_dir().map(|p| p.to_string_lossy().to_string()),
    }))
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {}", dst.display(), e))?;

    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            // Skip very large files (>50MB) and hidden OS files
            let size = fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
            let name = entry.file_name().to_string_lossy().to_string();
            if size < 50_000_000 && !name.starts_with('.') {
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| format!("copy {}: {}", src_path.display(), e))?;
            }
        }
    }
    Ok(())
}

fn copy_dir_selective(src: &PathBuf, dst: &PathBuf, names: &[&str]) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;

    for name in names {
        let src_path = src.join(name);
        let dst_path = dst.join(name);
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
