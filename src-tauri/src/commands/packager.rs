use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tokio::process::Command;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap()
                .join("Library/Application Support")
        })
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
    let archive_name = format!("nexus-backup-{}.tar.gz", timestamp);
    let archive_path = desktop.join(&archive_name);

    // Create a temp staging directory
    let staging = std::env::temp_dir().join(format!("nexus-pack-{}", timestamp));
    fs::create_dir_all(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;

    let mut includes = Vec::new();

    // 1. Copy Nexus config
    let config_src = nexus_data_dir();
    if config_src.exists() {
        let dest = staging.join("nexus-config");
        copy_dir_recursive(&config_src, &dest)?;
        includes.push("Nexus settings & config".to_string());
    }

    // 2. Copy OpenClaw data
    let openclaw = openclaw_dir();
    if openclaw.exists() {
        let dest = staging.join("openclaw");
        copy_dir_selective(
            &openclaw,
            &dest,
            &["openclaw.json", "cron", "skills", "memory", "workspace"],
        )?;
        includes.push("OpenClaw workspace & memory".to_string());
    }

    // 3. Copy local session data
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
        "restore_instructions": "To restore: Open Nexus on the new machine, go to Settings, click 'Unpack', and select this file."
    });
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(staging.join("manifest.json"), &manifest_json)
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

    // 6. Create tar.gz archive
    let utf8 = |p: &std::path::Path| {
        p.to_str()
            .map(str::to_string)
            .ok_or_else(|| format!("Path is not valid UTF-8: {}", p.display()))
    };
    let archive_arg = utf8(&archive_path)?;
    let parent_arg = utf8(
        staging
            .parent()
            .ok_or_else(|| "Staging dir has no parent".to_string())?,
    )?;
    let name_arg = utf8(std::path::Path::new(
        staging
            .file_name()
            .ok_or_else(|| "Staging dir has no name".to_string())?,
    ))?;
    let output = Command::new("tar")
        .args(["-czf", &archive_arg, "-C", &parent_arg, &name_arg])
        .output()
        .await
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

    let archive_str = archive
        .to_str()
        .ok_or_else(|| "Archive path is not valid UTF-8".to_string())?
        .to_string();

    // Inspect the archive before extracting anything: reject absolute paths,
    // `..` components and link entries that could write outside the staging dir.
    let listing = Command::new("tar")
        .args(["-tvzf", &archive_str])
        .output()
        .await
        .map_err(|e| format!("Failed to read archive: {}", e))?;
    if !listing.status.success() {
        return Err(format!(
            "Archive could not be read: {}",
            String::from_utf8_lossy(&listing.stderr)
        ));
    }
    let names = Command::new("tar")
        .args(["-tzf", &archive_str])
        .output()
        .await
        .map_err(|e| format!("Failed to read archive: {}", e))?;
    if !names.status.success() {
        return Err("Archive could not be read".to_string());
    }
    validate_archive_listing(
        &String::from_utf8_lossy(&names.stdout),
        &String::from_utf8_lossy(&listing.stdout),
    )?;

    // Unique, freshly created staging dir (create_dir fails if it already exists).
    let staging =
        std::env::temp_dir().join(format!("nexus-unpack-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;
    let staging_str = staging
        .to_str()
        .ok_or_else(|| "Staging path is not valid UTF-8".to_string())?
        .to_string();

    // Extract archive
    let output = Command::new("tar")
        .args([
            "-xzf",
            &archive_str,
            "--no-same-owner",
            "--no-same-permissions",
            "-C",
            &staging_str,
        ])
        .output()
        .await
        .map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!("Failed to extract: {}", e)
        })?;

    if !output.status.success() {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!(
            "Extract failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Find the extracted directory (should be nexus-pack-*)
    let extracted = fs::read_dir(&staging)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("nexus-pack") || n.starts_with("electron-pack") // legacy backups
        })
        .map(|e| e.path())
        .unwrap_or(staging.clone());

    let mut restored = Vec::new();

    // Restore config
    let config_src = {
        let new_cfg = extracted.join("nexus-config");
        if new_cfg.exists() { new_cfg } else { extracted.join("electron-config") } // legacy backups
    };
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

/// Unpack limits, checked from the listing before anything is extracted.
const MAX_UNPACK_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
const MAX_UNPACK_ENTRIES: usize = 50_000;

/// Size column of a `tar -tv` line. bsdtar (macOS): `perm links user group size ...`;
/// GNU tar: `perm user/group size ...`.
fn listed_entry_size(line: &str) -> Option<u64> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let idx = if fields.get(1).map_or(false, |f| f.contains('/')) { 2 } else { 4 };
    fields.get(idx)?.parse().ok()
}

/// Validate `tar -tzf` (names) and `tar -tvzf` (verbose) output for an archive.
/// Rejects absolute paths, any `..` component, symlink/hardlink entries, and
/// archives over the entry-count or total-size limits.
fn validate_archive_listing(names: &str, verbose: &str) -> Result<(), String> {
    validate_archive_listing_with_limits(names, verbose, MAX_UNPACK_TOTAL_BYTES, MAX_UNPACK_ENTRIES)
}

fn validate_archive_listing_with_limits(
    names: &str,
    verbose: &str,
    max_total_bytes: u64,
    max_entries: usize,
) -> Result<(), String> {
    let entry_count = names.lines().filter(|l| !l.is_empty()).count();
    if entry_count > max_entries {
        return Err(format!(
            "Archive rejected: {} entries exceeds the limit of {}",
            entry_count, max_entries
        ));
    }
    let mut total: u64 = 0;
    for line in verbose.lines().filter(|l| !l.is_empty()) {
        let size = listed_entry_size(line)
            .ok_or_else(|| "Archive rejected: entry size could not be read".to_string())?;
        total = total.saturating_add(size);
        if total > max_total_bytes {
            return Err(format!(
                "Archive rejected: unpacked size exceeds the limit of {} MB",
                max_total_bytes / (1024 * 1024)
            ));
        }
    }
    for name in names.lines().filter(|l| !l.is_empty()) {
        let path = std::path::Path::new(name);
        if name.starts_with('/') || name.starts_with('\\') || path.is_absolute() {
            return Err(format!("Archive rejected: absolute path entry '{}'", name));
        }
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_)))
        {
            return Err(format!("Archive rejected: '..' in entry '{}'", name));
        }
    }
    for line in verbose.lines().filter(|l| !l.is_empty()) {
        match line.chars().next() {
            Some('-') | Some('d') => {}
            _ => {
                return Err(
                    "Archive rejected: contains link or special entries".to_string(),
                )
            }
        }
    }
    Ok(())
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

        // Never follow symlinks while copying restored data.
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
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

#[cfg(test)]
mod unpack_tests {
    use super::{listed_entry_size, validate_archive_listing, validate_archive_listing_with_limits};

    #[test]
    fn reads_sizes_from_bsd_and_gnu_listings() {
        assert_eq!(listed_entry_size("-rw-r--r--  0 u g 1234 Oct 21 18:00 a/b"), Some(1234));
        assert_eq!(listed_entry_size("-rw-r--r-- u/g 99 2026-10-21 18:00 a/b"), Some(99));
        assert_eq!(listed_entry_size("garbage"), None);
    }

    #[test]
    fn rejects_oversized_or_too_many_entries() {
        let names = "a/1\na/2\n";
        let verbose = "-rw-r--r--  0 u g 600 Oct 21 18:00 a/1\n-rw-r--r--  0 u g 600 Oct 21 18:00 a/2\n";
        assert!(validate_archive_listing_with_limits(names, verbose, 1_200, 10).is_ok());
        assert!(validate_archive_listing_with_limits(names, verbose, 1_000, 10).is_err());
        assert!(validate_archive_listing_with_limits(names, verbose, 10_000, 1).is_err());
        assert!(validate_archive_listing_with_limits("a\n", "-rw-r--r-- nonsense\n", 10_000, 10).is_err());
    }

    #[test]
    fn accepts_normal_pack_listing() {
        let names = "nexus-pack-1/\nnexus-pack-1/manifest.json\nnexus-pack-1/sessions/a.json\n";
        let verbose = "drwxr-xr-x  0 u g 0 Oct 21 18:00 nexus-pack-1/\n-rw-r--r--  0 u g 10 Oct 21 18:00 nexus-pack-1/manifest.json\n";
        assert!(validate_archive_listing(names, verbose).is_ok());
    }

    #[test]
    fn rejects_traversal_absolute_and_links() {
        assert!(validate_archive_listing("../evil\n", "").is_err());
        assert!(validate_archive_listing("a/../../evil\n", "").is_err());
        assert!(validate_archive_listing("/etc/passwd\n", "").is_err());
        assert!(validate_archive_listing(
            "a/link\n",
            "lrwxr-xr-x  0 u g 0 Oct 21 18:00 a/link -> /etc\n"
        )
        .is_err());
        assert!(validate_archive_listing(
            "a/hard\n",
            "hrw-r--r--  0 u g 0 Oct 21 18:00 a/hard link to /etc/passwd\n"
        )
        .is_err());
    }
}
