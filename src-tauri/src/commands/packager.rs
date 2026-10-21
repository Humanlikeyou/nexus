use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Shared app-data dir (honours `NEXUS_DATA_DIR`).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
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

    // Unique, freshly created staging dir (create_dir fails if it already exists).
    let staging =
        std::env::temp_dir().join(format!("nexus-unpack-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;
    // Removed on every exit path, including early `?` returns during restore.
    let _staging_guard = StagingGuard(staging.clone());

    // Validate and extract in one pass with the tar crate, reading sizes and
    // types from the headers themselves (no CLI listing to parse), so what is
    // checked is exactly what is written.
    let (archive_for_task, staging_for_task) = (archive.clone(), staging.clone());
    let unpacked = tokio::task::spawn_blocking(move || {
        unpack_archive_checked(
            &archive_for_task,
            &staging_for_task,
            MAX_UNPACK_TOTAL_BYTES,
            MAX_UNPACK_ENTRIES,
        )
    })
    .await
    .map_err(|e| format!("Extract task failed: {}", e))
    .and_then(|r| r);
    unpacked?;

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
        if new_cfg.exists() {
            new_cfg
        } else {
            extracted.join("electron-config")
        } // legacy backups
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

    // Staging cleanup happens when `_staging_guard` drops.

    if restored.is_empty() {
        restored.push("Archive was empty — nothing to restore".to_string());
    }

    Ok(restored)
}

/// Deletes the staging directory when dropped.
struct StagingGuard(PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Unpack limits, enforced from tar headers before each entry is written.
const MAX_UNPACK_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
const MAX_UNPACK_ENTRIES: usize = 50_000;

/// Check one entry's path: relative, no `..`, no root/prefix components.
fn check_entry_path(path: &std::path::Path) -> Result<(), String> {
    use std::path::Component;
    if path.as_os_str().is_empty() {
        return Err("Archive rejected: empty entry name".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!(
                    "Archive rejected: '..' in entry '{}'",
                    path.display()
                ))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "Archive rejected: absolute path entry '{}'",
                    path.display()
                ))
            }
        }
    }
    Ok(())
}

/// Gzip'd tar -> `dest`. Every header is checked before its entry is written:
/// only regular files and directories, safe relative paths, entry count and
/// cumulative declared size within limits. Ownership, permissions, mtimes and
/// xattrs from the archive are not applied. Returns the number of entries.
fn unpack_archive_checked(
    archive: &std::path::Path,
    dest: &std::path::Path,
    max_total_bytes: u64,
    max_entries: usize,
) -> Result<usize, String> {
    let file = fs::File::open(archive).map_err(|e| format!("Failed to open archive: {}", e))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(false);
    tar.set_preserve_ownerships(false);
    tar.set_preserve_mtime(false);
    tar.set_unpack_xattrs(false);
    tar.set_overwrite(false);

    let mut count = 0usize;
    let mut total: u64 = 0;
    for entry in tar
        .entries()
        .map_err(|e| format!("Archive could not be read: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Archive could not be read: {}", e))?;
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err("Archive rejected: contains link or special entries".to_string());
        }
        count += 1;
        if count > max_entries {
            return Err(format!(
                "Archive rejected: more than {} entries",
                max_entries
            ));
        }
        let size = entry
            .header()
            .size()
            .map_err(|e| format!("Archive rejected: bad entry size: {}", e))?;
        total = total.saturating_add(size);
        if total > max_total_bytes {
            return Err(format!(
                "Archive rejected: unpacked size exceeds the limit of {} MB",
                max_total_bytes / (1024 * 1024)
            ));
        }
        let path = entry
            .path()
            .map_err(|e| format!("Archive rejected: bad entry name: {}", e))?
            .into_owned();
        check_entry_path(&path)?;
        // unpack_in also refuses anything that would land outside `dest`.
        let inside = entry
            .unpack_in(dest)
            .map_err(|e| format!("Extract failed for '{}': {}", path.display(), e))?;
        if !inside {
            return Err(format!(
                "Archive rejected: entry '{}' escapes the staging dir",
                path.display()
            ));
        }
    }
    Ok(count)
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

fn copy_dir_selective(src: &Path, dst: &Path, names: &[&str]) -> Result<(), String> {
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
    use super::unpack_archive_checked;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-unpack-test-{}-{}",
            tag,
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn gz(dir: &Path, raw_tar: &[u8]) -> PathBuf {
        let path = dir.join("a.tar.gz");
        let mut enc = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        enc.write_all(raw_tar).unwrap();
        enc.finish().unwrap();
        path
    }

    /// Raw header with arbitrary name/type/size, bypassing Builder's path checks.
    fn raw_header(name: &str, kind: tar::EntryType, size: u64, gname: &str) -> tar::Header {
        let mut h = tar::Header::new_gnu();
        {
            let bytes = h.as_old_mut();
            bytes.name = [0; 100];
            bytes.name[..name.len()].copy_from_slice(name.as_bytes());
        }
        h.set_entry_type(kind);
        h.set_size(size);
        h.set_mode(0o644);
        h.set_groupname(gname).unwrap();
        h.set_cksum();
        h
    }

    fn one_entry_tar(
        name: &str,
        kind: tar::EntryType,
        data: &[u8],
        declared: u64,
        gname: &str,
    ) -> Vec<u8> {
        let mut out = raw_header(name, kind, declared, gname).as_bytes().to_vec();
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512 + 1024, 0);
        out
    }

    #[test]
    fn staging_guard_removes_dir_on_early_return() {
        fn fails_midway(dir: &Path) -> Result<(), String> {
            let _guard = super::StagingGuard(dir.to_path_buf());
            std::fs::write(dir.join("partial"), b"x").map_err(|e| e.to_string())?;
            Err("restore failed".to_string())?;
            Ok(())
        }
        let dir = tmp("guard");
        assert!(fails_midway(&dir).is_err());
        assert!(!dir.exists(), "staging dir must be removed on error paths");
    }

    #[test]
    fn unpacks_a_normal_pack() {
        let dir = tmp("ok");
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(5);
        h.set_mode(0o600);
        h.set_cksum();
        b.append_data(&mut h, "nexus-pack-1/manifest.json", &b"{\"a\"}"[..])
            .unwrap();
        let archive = gz(&dir, &b.into_inner().unwrap());
        let dest = dir.join("out");
        std::fs::create_dir(&dest).unwrap();
        assert_eq!(
            unpack_archive_checked(&archive, &dest, 1_000, 10).unwrap(),
            1
        );
        assert_eq!(
            std::fs::read(dest.join("nexus-pack-1/manifest.json")).unwrap(),
            b"{\"a\"}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Archives made by the system `tar` CLI (what pack_it_up uses, incl. macOS
    /// bsdtar pax/xattr headers) must unpack.
    #[test]
    fn unpacks_an_archive_made_by_system_tar() {
        let dir = tmp("cli");
        let src = dir.join("nexus-pack-x");
        std::fs::create_dir_all(src.join("sessions")).unwrap();
        std::fs::write(src.join("manifest.json"), b"{}").unwrap();
        std::fs::write(src.join("sessions").join("s.json"), b"[]").unwrap();
        let archive = dir.join("cli.tar.gz");
        let ok = std::process::Command::new("tar")
            .args([
                "-czf",
                archive.to_str().unwrap(),
                "-C",
                dir.to_str().unwrap(),
                "nexus-pack-x",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("system tar unavailable; skipping");
            return;
        }
        let dest = dir.join("out");
        std::fs::create_dir(&dest).unwrap();
        unpack_archive_checked(&archive, &dest, 1_000_000, 100).unwrap();
        assert!(dest.join("nexus-pack-x/sessions/s.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malicious_gname_and_huge_declared_size_hit_the_cap_before_writing() {
        // A group name with spaces used to shift the `tar -tv` columns; the
        // header-based size must still be read and capped.
        let dir = tmp("cap");
        let raw = one_entry_tar(
            "nexus-pack-1/big.bin",
            tar::EntryType::Regular,
            b"",
            3 * 1024 * 1024 * 1024,
            "x 0",
        );
        let archive = gz(&dir, &raw);
        let dest = dir.join("out");
        std::fs::create_dir(&dest).unwrap();
        let err = unpack_archive_checked(&archive, &dest, 2 * 1024 * 1024 * 1024, 10).unwrap_err();
        assert!(err.contains("size exceeds"), "{err}");
        assert!(
            !dest.join("nexus-pack-1").exists(),
            "nothing may be written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_traversal_absolute_links_and_too_many_entries() {
        for (name, kind) in [
            ("../evil", tar::EntryType::Regular),
            ("a/../../evil", tar::EntryType::Regular),
            ("/etc/evil", tar::EntryType::Regular),
            ("a/link", tar::EntryType::Symlink),
            ("a/hard", tar::EntryType::Link),
        ] {
            let dir = tmp("bad");
            let archive = gz(&dir, &one_entry_tar(name, kind, b"x", 1, "g"));
            let dest = dir.join("out");
            std::fs::create_dir(&dest).unwrap();
            assert!(
                unpack_archive_checked(&archive, &dest, 1_000, 10).is_err(),
                "{name} must be rejected"
            );
            assert!(!dir.join("evil").exists());
            let _ = std::fs::remove_dir_all(&dir);
        }
        let dir = tmp("count");
        let mut raw = Vec::new();
        for i in 0..3 {
            let mut e = raw_header(&format!("p/f{i}"), tar::EntryType::Regular, 1, "g")
                .as_bytes()
                .to_vec();
            e.extend_from_slice(b"x");
            e.resize(1024, 0);
            raw.extend(e);
        }
        raw.extend([0u8; 1024]);
        let archive = gz(&dir, &raw);
        let dest = dir.join("out");
        std::fs::create_dir(&dest).unwrap();
        assert!(unpack_archive_checked(&archive, &dest, 1_000, 2).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
