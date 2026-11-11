use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Shared app-data dir (honours `NEXUS_DATA_DIR`).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

#[derive(Serialize)]
pub struct PackageInfo {
    pub path: String,
    pub size_mb: f64,
    pub includes: Vec<String>,
    /// Files left out of the backup (too large or symlinks), never silent.
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn pack_it_up() -> Result<PackageInfo, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    pack_at(&home, &nexus_data_dir()).await
}

/// `pack_it_up` with the home and data dirs injected (tests use temp dirs).
pub(crate) async fn pack_at(home: &Path, data_dir: &Path) -> Result<PackageInfo, String> {
    let desktop = home.join("Desktop");
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive_name = format!("nexus-backup-{}.tar.gz", timestamp);
    let archive_path = desktop.join(&archive_name);

    // Fresh, unguessable staging dir (create_dir fails if it already exists, so a
    // pre-planted dir or symlink in the temp dir is never reused). The archive's
    // top-level name stays `nexus-pack-*`, which unpack_backup looks for.
    let staging = std::env::temp_dir().join(format!(
        "nexus-pack-{}-{}",
        timestamp,
        uuid::Uuid::new_v4().simple()
    ));
    create_private_dir(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;
    // Removed on every exit path, including early `?` returns while copying.
    let _staging_guard = StagingGuard(staging.clone());

    let mut includes = Vec::new();
    let mut skipped = Vec::new();

    // 1. Copy Nexus config
    let config_src = data_dir.to_path_buf();
    if config_src.exists() {
        let dest = staging.join("nexus-config");
        copy_dir_recursive(&config_src, &dest, Some(MAX_PACK_FILE_BYTES), &mut skipped)?;
        includes.push("Amara OS settings & config".to_string());
    }

    // 3. Copy local session data
    let sessions_dir = data_dir.join("sessions");
    if sessions_dir.exists() {
        let dest = staging.join("sessions");
        copy_dir_recursive(
            &sessions_dir,
            &dest,
            Some(MAX_PACK_FILE_BYTES),
            &mut skipped,
        )?;
        includes.push("BLE session recordings".to_string());
    }

    // 4. Copy goals
    let goals_file = data_dir.join("goals.json");
    if goals_file.exists() {
        fs::copy(&goals_file, staging.join("goals.json"))
            .map_err(|e| format!("Failed to copy goals: {}", e))?;
        includes.push("Goals & trajectory data".to_string());
    }

    // 5. Copy events database
    let events_db = data_dir.join("events.db");
    if events_db.exists() {
        fs::copy(&events_db, staging.join("events.db"))
            .map_err(|e| format!("Failed to copy events.db: {}", e))?;
        // Also copy WAL/SHM files if they exist
        let wal = data_dir.join("events.db-wal");
        let shm = data_dir.join("events.db-shm");
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
        "skipped": &skipped,
        "restore_instructions": "To restore: Open Amara OS on the new machine, go to Settings, click 'Unpack', and select this file."
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
    // Pre-create the archive 0600 (tar truncates it but keeps the mode), so the
    // backup is never readable by other users, even briefly.
    create_private_file(&archive_path)
        .map_err(|e| format!("Failed to create archive file: {}", e))?;
    let output = Command::new("tar")
        .args(["-czf", &archive_arg, "-C", &parent_arg, &name_arg])
        .output()
        .await
        .map_err(|e| format!("Failed to create archive: {}", e))?;

    if !output.status.success() {
        let _ = fs::remove_file(&archive_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tar failed: {}", stderr));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to restrict archive permissions: {}", e))?;
    }

    // Staging cleanup happens when `_staging_guard` drops.

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
        skipped,
    })
}

/// `~/x` → `$HOME/x` (the Settings path field accepts it); anything else unchanged.
pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => dirs::home_dir().unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}

/// How long the restore dialog may stay open before Allow no longer counts.
const RESTORE_APPROVAL_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const RESTORE_LOCK_KEY: &str = "restore:backup";
const RESTORE_BACKUP_DIR: &str = "restore-backups";

#[tauri::command]
pub async fn unpack_backup(
    app: tauri::AppHandle,
    archive_path: String,
) -> Result<Vec<String>, String> {
    let archive = expand_home(archive_path.trim());
    if !archive.is_file() {
        return Err("Archive file not found".to_string());
    }

    // Unique, freshly created staging dir (create_dir fails if it already exists).
    let staging =
        std::env::temp_dir().join(format!("nexus-unpack-{}", uuid::Uuid::new_v4().simple()));
    create_private_dir(&staging).map_err(|e| format!("Failed to create staging dir: {}", e))?;
    // Removed on every exit path, including early `?` returns during restore.
    let _staging_guard = StagingGuard(staging.clone());

    // Validate and extract in one pass with the tar crate, reading sizes and
    // types from the headers themselves (no CLI listing to parse), so what is
    // checked is exactly what is written. Nothing outside staging is touched yet.
    let (archive_for_task, staging_for_task) = (archive.clone(), staging.clone());
    tokio::task::spawn_blocking(move || {
        unpack_archive_checked(
            &archive_for_task,
            &staging_for_task,
            MAX_UNPACK_TOTAL_BYTES,
            MAX_UNPACK_ENTRIES,
        )
    })
    .await
    .map_err(|e| format!("Extract task failed: {}", e))??;

    let extracted = find_pack_root(&staging)?;
    let roots = RestoreRoots {
        data: nexus_data_dir(),
    };
    let plan = plan_restore(&extracted)?;
    if plan.items.is_empty() {
        return Ok(vec!["Archive was empty — nothing to restore".to_string()]);
    }
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let replaced = replaced_files(&plan, &roots);
    let archive_name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let asked_at = std::time::Instant::now();
    let _approval = super::approval_gate::require_approval(
        &app,
        &[RESTORE_LOCK_KEY.to_string()],
        super::approval_gate::approval_text(
            &restore_operation(&archive_name, &plan, &replaced, &roots, &stamp),
            "Only files Amara OS backups contain are restored; everything else in the archive is skipped. Your current data is copied to restore-backups/<stamp>-full first, and replaced files to restore-backups/<stamp>.",
        )?,
    )
    .await?;
    if asked_at.elapsed() > RESTORE_APPROVAL_TTL {
        return Err(format!(
            "{} The restore dialog was open too long; nothing was restored. Try again.",
            super::restart_services::EXPIRED_PREFIX
        ));
    }
    // What gets replaced must still be what the dialog showed.
    if replaced_files(&plan, &roots) != replaced {
        return Err("changed: files in the Amara OS data folder changed while the dialog was open; nothing was restored".to_string());
    }

    // Full safety copy of the current data before anything is overwritten.
    let (snapshot, snapshot_skipped) = snapshot_before_restore(&roots, &stamp)?;
    let mut restored = vec![format!(
        "Safety backup of current data saved to {}",
        snapshot.display()
    )];
    restored.extend(
        snapshot_skipped
            .iter()
            .map(|s| format!("Not in safety backup: {s}")),
    );
    restored.extend(apply_restore(&plan, &roots, &stamp)?);
    restored.extend(plan.skipped.iter().map(|s| format!("Skipped: {s}")));
    Ok(restored)
}

/// Copy the current data dir (minus earlier restore backups) into
/// `<data>/restore-backups/<stamp>-full/`
/// (0700), so a restore can be undone by hand. Symlinks are listed, not followed.
fn snapshot_before_restore(
    roots: &RestoreRoots,
    stamp: &str,
) -> Result<(PathBuf, Vec<String>), String> {
    fs::create_dir_all(&roots.data).map_err(|e| e.to_string())?;
    let backups = roots.data.join(RESTORE_BACKUP_DIR);
    match backups.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err(format!("Not a real folder: {}", backups.display()))
        }
        Ok(_) => {}
        Err(_) => create_private_dir(&backups).map_err(|e| e.to_string())?,
    }
    let target = backups.join(format!("{stamp}-full"));
    create_private_dir(&target)
        .map_err(|e| format!("Failed to create safety backup folder: {e}"))?;
    let mut skipped = Vec::new();
    if roots.data.is_dir() {
        let mut names: Vec<String> = fs::read_dir(&roots.data)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != RESTORE_BACKUP_DIR && !is_os_junk(n))
            .collect();
        names.sort();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        copy_dir_selective(&roots.data, &target.join("data"), &refs, None, &mut skipped)?;
    }
    Ok((target, skipped))
}

/// The `nexus-pack-*` (or legacy `electron-pack-*`) folder inside staging, else staging.
fn find_pack_root(staging: &Path) -> Result<PathBuf, String> {
    Ok(fs::read_dir(staging)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            (n.starts_with("nexus-pack") || n.starts_with("electron-pack"))
                && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
        })
        .map(|e| e.path())
        .unwrap_or_else(|| staging.to_path_buf()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RestoreRoot {
    Data,
}

struct RestoreRoots {
    data: PathBuf,
}

impl RestoreRoots {
    fn get(&self, root: RestoreRoot) -> &Path {
        match root {
            RestoreRoot::Data => &self.data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestoreItem {
    src: PathBuf,
    root: RestoreRoot,
    pub(crate) rel: PathBuf,
}

#[derive(Debug, Default)]
pub(crate) struct RestorePlan {
    pub(crate) items: Vec<RestoreItem>,
    pub(crate) skipped: Vec<String>,
}

/// Map an archive file (path relative to the pack root) to where it may be restored.
pub(crate) fn restore_target(rel: &[String]) -> Option<(RestoreRoot, PathBuf)> {
    let join = |parts: &[String]| parts.iter().collect::<PathBuf>();
    match rel {
        [top, rest @ ..]
            if (top == "nexus-config" || top == "electron-config")
                && !rest.is_empty()
                && rest[0] != RESTORE_BACKUP_DIR =>
        {
            Some((RestoreRoot::Data, join(rest)))
        }
        [top, rest @ ..] if top == "sessions" && !rest.is_empty() => {
            Some((RestoreRoot::Data, join(rel)))
        }
        [only]
            if matches!(
                only.as_str(),
                "goals.json" | "events.db" | "events.db-wal" | "events.db-shm"
            ) =>
        {
            Some((RestoreRoot::Data, PathBuf::from(only)))
        }
        _ => None,
    }
}

/// Walk the extracted pack (regular files only; staging holds no links) and
/// split it into allowed restore items and skipped entries.
pub(crate) fn plan_restore(pack_root: &Path) -> Result<RestorePlan, String> {
    let mut plan = RestorePlan::default();
    let mut stack = vec![(pack_root.to_path_buf(), Vec::<String>::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            let mut rel = prefix.clone();
            rel.push(name.clone());
            let ft = entry.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                stack.push((entry.path(), rel));
            } else if ft.is_file() {
                if rel == ["manifest.json"] || is_os_junk(&name) {
                    continue;
                }
                match restore_target(&rel) {
                    Some((root, target)) if check_entry_path(&target).is_ok() => {
                        plan.items.push(RestoreItem {
                            src: entry.path(),
                            root,
                            rel: target,
                        })
                    }
                    _ => plan
                        .skipped
                        .push(format!("{} (not part of a Amara OS backup)", rel.join("/"))),
                }
            } else {
                plan.skipped
                    .push(format!("{} (not a regular file)", rel.join("/")));
            }
        }
    }
    // nexus-config/ is a full copy of the data dir, so goals.json, sessions/… can
    // appear twice. Restore each destination once, preferring the dedicated copy.
    let from_config = |i: &RestoreItem| {
        i.src
            .strip_prefix(pack_root)
            .ok()
            .and_then(|r| r.components().next())
            .is_some_and(|c| c.as_os_str() == "nexus-config" || c.as_os_str() == "electron-config")
    };
    plan.items
        .sort_by(|a, b| (a.root, &a.rel, from_config(a)).cmp(&(b.root, &b.rel, from_config(b))));
    plan.items
        .dedup_by(|later, first| later.root == first.root && later.rel == first.rel);
    plan.skipped.sort();
    Ok(plan)
}

/// Destination files that already exist and would be replaced.
fn replaced_files(plan: &RestorePlan, roots: &RestoreRoots) -> Vec<(RestoreRoot, PathBuf)> {
    plan.items
        .iter()
        .filter(|i| roots.get(i.root).join(&i.rel).symlink_metadata().is_ok())
        .map(|i| (i.root, i.rel.clone()))
        .collect()
}

fn restore_operation(
    archive_name: &str,
    plan: &RestorePlan,
    replaced: &[(RestoreRoot, PathBuf)],
    roots: &RestoreRoots,
    stamp: &str,
) -> String {
    let count = |r: RestoreRoot| plan.items.iter().filter(|i| i.root == r).count();
    let rcount = |r: RestoreRoot| replaced.iter().filter(|(x, _)| *x == r).count();
    let mut out = format!(
        "Restore backup {archive_name}:\n- {} files into {} ({} replaced)\nReplaced files are first copied to {}/{stamp} in each folder.",
        count(RestoreRoot::Data),
        roots.data.display(),
        rcount(RestoreRoot::Data),
        RESTORE_BACKUP_DIR,
    );
    if !replaced.is_empty() {
        out.push_str("\nReplaces:");
        for (root, rel) in replaced.iter().take(12) {
            let label = match root {
                RestoreRoot::Data => "data",
            };
            out.push_str(&format!("\n  {label}/{}", rel.display()));
        }
        if replaced.len() > 12 {
            out.push_str(&format!("\n  … and {} more", replaced.len() - 12));
        }
    }
    if !plan.skipped.is_empty() {
        out.push_str(&format!(
            "\nSkipped (not restorable): {}",
            plan.skipped.len()
        ));
    }
    out
}

/// Resolve `root/rel` for writing: every existing component must be a real
/// directory (no symlinks) and the result must stay inside `root`.
fn safe_dest(root: &Path, rel: &Path) -> Result<PathBuf, String> {
    check_entry_path(rel)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().map_err(|e| e.to_string())?;
    let mut cur = root_canon.clone();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        cur.push(c.as_os_str());
        let last = i + 1 == comps.len();
        match cur.symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!(
                    "Refusing to restore through a symlink: {}",
                    cur.display()
                ))
            }
            Ok(m) if !last && !m.is_dir() => {
                return Err(format!("Not a directory: {}", cur.display()))
            }
            Ok(m) if last && m.is_dir() => {
                return Err(format!("A folder is in the way: {}", cur.display()))
            }
            Ok(_) => {}
            Err(_) if !last => fs::create_dir(&cur).map_err(|e| e.to_string())?,
            Err(_) => {}
        }
    }
    if !cur.starts_with(&root_canon) {
        return Err("Path escapes its restore folder".to_string());
    }
    Ok(cur)
}

/// Copy every planned file into place. Existing files are first copied to
/// `<root>/restore-backups/<stamp>/<rel>`; new files are written via a temp
/// sibling + rename so a crash never leaves a half-written file.
fn apply_restore(
    plan: &RestorePlan,
    roots: &RestoreRoots,
    stamp: &str,
) -> Result<Vec<String>, String> {
    let mut counts = std::collections::BTreeMap::new();
    let mut backed_up = 0usize;
    for item in &plan.items {
        let root = roots.get(item.root);
        let dest = safe_dest(root, &item.rel)?;
        if dest.is_file() {
            let backup_rel = Path::new(RESTORE_BACKUP_DIR).join(stamp).join(&item.rel);
            let backup = safe_dest(root, &backup_rel)?;
            fs::copy(&dest, &backup).map_err(|e| format!("backup {}: {}", dest.display(), e))?;
            backed_up += 1;
        }
        let tmp = dest.with_file_name(format!(
            ".{}.restore-{}",
            dest.file_name().unwrap_or_default().to_string_lossy(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::copy(&item.src, &tmp).map_err(|e| format!("restore {}: {}", item.rel.display(), e))?;
        fs::rename(&tmp, &dest).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("restore {}: {}", item.rel.display(), e)
        })?;
        *counts.entry(item.root).or_insert(0usize) += 1;
    }
    let mut out = Vec::new();
    if let Some(n) = counts.get(&RestoreRoot::Data) {
        out.push(format!("Amara OS data: {n} files restored"));
    }
    if backed_up > 0 {
        out.push(format!(
            "{backed_up} replaced files saved to {RESTORE_BACKUP_DIR}/{stamp}"
        ));
    }
    Ok(out)
}

/// New directory readable only by the user (0700 on Unix); fails if it exists.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut b = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path)
}

/// New empty file readable only by the user (0600 on Unix); fails if it exists.
fn create_private_file(path: &Path) -> std::io::Result<()> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).map(|_| ())
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

/// `._foo` resource-fork sidecar files created by macOS tar/Finder.
fn is_apple_double(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("._"))
}

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
pub(crate) fn unpack_archive_checked(
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
        // macOS AppleDouble metadata (`._name`) is never written and does not
        // count toward the entry/size caps.
        if is_apple_double(
            &entry
                .path()
                .map_err(|e| format!("Archive rejected: bad entry name: {}", e))?,
        ) {
            continue;
        }
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
        "home_dir": dirs::home_dir().map(|p| p.to_string_lossy().to_string()),
    }))
}

/// Per-file limit when packing (keeps backups a manageable size). Files over
/// it are listed in `PackageInfo::skipped` and the manifest.
const MAX_PACK_FILE_BYTES: u64 = 50_000_000;

/// macOS metadata that is not user data (never packed or restored).
fn is_os_junk(name: &str) -> bool {
    // The n8n key file (and its temp files) never goes into a backup or a restore.
    name == ".DS_Store"
        || name.starts_with("._")
        || name == super::key_file::KEY_FILE_NAME
        || (name.starts_with(".amara-n8n.key.") && name.ends_with(".tmp"))
}

/// Recursively copy `src` into `dst`. Dot-files are copied. Symlinks are never
/// followed, and files over `max_file_bytes` (if set) are not copied; both are
/// recorded in `skipped` instead of being dropped silently.
fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    max_file_bytes: Option<u64>,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {}", dst.display(), e))?;

    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().to_string();
        if is_os_junk(&name) {
            continue;
        }

        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            skipped.push(format!("{} (symlink)", src_path.display()));
            continue;
        }
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, max_file_bytes, skipped)?;
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if max_file_bytes.is_some_and(|max| size >= max) {
            skipped.push(format!(
                "{} ({:.1} MB, over the {} MB per-file limit)",
                src_path.display(),
                size as f64 / 1_000_000.0,
                max_file_bytes.unwrap_or(0) / 1_000_000
            ));
            continue;
        }
        fs::copy(&src_path, &dst_path)
            .map_err(|e| format!("copy {}: {}", src_path.display(), e))?;
    }
    Ok(())
}

fn copy_dir_selective(
    src: &Path,
    dst: &Path,
    names: &[&str],
    max_file_bytes: Option<u64>,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;

    for name in names {
        let src_path = src.join(name);
        let dst_path = dst.join(name);
        if src_path.is_symlink() {
            skipped.push(format!("{} (symlink)", src_path.display()));
        } else if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, max_file_bytes, skipped)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod copy_tests {
    use super::copy_dir_recursive;

    #[test]
    fn copies_dotfiles_reports_symlinks_and_oversize_skips_junk() {
        let base =
            std::env::temp_dir().join(format!("nexus-copy-{}", uuid::Uuid::new_v4().simple()));
        let src = base.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join(".hidden_config"), b"keep").unwrap();
        std::fs::write(src.join("sub").join("big.bin"), vec![0u8; 2048]).unwrap();
        std::fs::write(src.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(src.join("._x"), b"junk").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", src.join("link")).unwrap();

        // Restore mode: no size limit.
        let mut skipped = Vec::new();
        copy_dir_recursive(&src, &base.join("restore"), None, &mut skipped).unwrap();
        assert_eq!(
            std::fs::read(base.join("restore/.hidden_config")).unwrap(),
            b"keep"
        );
        assert!(base.join("restore/sub/big.bin").is_file());
        assert!(!base.join("restore/.DS_Store").exists());
        assert!(!base.join("restore/._x").exists());
        #[cfg(unix)]
        {
            assert!(!base.join("restore/link").exists());
            assert_eq!(skipped.len(), 1);
            assert!(skipped[0].contains("symlink"));
        }

        // Pack mode: per-file limit is reported, not silent.
        let mut skipped = Vec::new();
        copy_dir_recursive(&src, &base.join("pack"), Some(1024), &mut skipped).unwrap();
        assert!(!base.join("pack/sub/big.bin").exists());
        assert!(skipped
            .iter()
            .any(|s| s.contains("big.bin") && s.contains("limit")));
        let _ = std::fs::remove_dir_all(&base);
    }
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
    fn apple_double_entries_are_skipped_and_not_counted() {
        let dir = tmp("appledouble");
        // AppleDouble entry + one real file; caps of 1 entry / 1 byte only pass
        // if the AppleDouble entry is not counted.
        let mut small = Vec::new();
        let mut ad = raw_header("p/._f", tar::EntryType::Regular, 1, "g")
            .as_bytes()
            .to_vec();
        ad.extend_from_slice(b"m");
        ad.resize(1024, 0);
        small.extend(ad);
        let mut f = raw_header("p/f", tar::EntryType::Regular, 1, "g")
            .as_bytes()
            .to_vec();
        f.extend_from_slice(b"x");
        f.resize(1024, 0);
        small.extend(f);
        small.extend([0u8; 1024]);
        let archive = gz(&dir, &small);
        let dest = dir.join("out");
        std::fs::create_dir(&dest).unwrap();
        assert_eq!(unpack_archive_checked(&archive, &dest, 1, 1).unwrap(), 1);
        assert!(dest.join("p/f").is_file());
        assert!(!dest.join("p/._f").exists());
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

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-restore-test-{}-{}",
            tag,
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn put(base: &Path, rel: &str, body: &str) {
        let p = base.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn s(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn expand_home_only_touches_a_leading_tilde_slash() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_home("~/Desktop/b.tar.gz"),
            home.join("Desktop/b.tar.gz")
        );
        assert_eq!(
            expand_home("/tmp/~/b.tar.gz"),
            PathBuf::from("/tmp/~/b.tar.gz")
        );
        assert_eq!(
            expand_home("~other/b.tar.gz"),
            PathBuf::from("~other/b.tar.gz")
        );
    }

    #[test]
    fn snapshot_before_restore_copies_current_data_but_not_old_backups() {
        let base = tmp("snapshot");
        let roots = RestoreRoots {
            data: base.join("data"),
        };
        put(&roots.data, "goals.json", "[1]");
        put(&roots.data, "sessions/a.ndjson", "x");
        put(&roots.data, "restore-backups/old/goals.json", "old");

        let (dir, skipped) = snapshot_before_restore(&roots, "20261029_100000").unwrap();
        assert_eq!(dir, roots.data.join("restore-backups/20261029_100000-full"));
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(
            fs::read_to_string(dir.join("data/goals.json")).unwrap(),
            "[1]"
        );
        assert!(dir.join("data/sessions/a.ndjson").is_file());
        assert!(!dir.join("data/restore-backups").exists());
        assert!(
            !dir.join("openclaw").exists(),
            "no OpenClaw files in the safety copy"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        // Same stamp twice never overwrites an earlier safety copy.
        assert!(snapshot_before_restore(&roots, "20261029_100000").is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn snapshot_refuses_a_symlinked_backup_folder() {
        let base = tmp("snapshot-link");
        let roots = RestoreRoots {
            data: base.join("data"),
        };
        fs::create_dir_all(&roots.data).unwrap();
        fs::create_dir_all(base.join("elsewhere")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("elsewhere"), roots.data.join("restore-backups"))
                .unwrap();
            assert!(snapshot_before_restore(&roots, "s").is_err());
            assert_eq!(fs::read_dir(base.join("elsewhere")).unwrap().count(), 0);
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn allow_list_matches_what_pack_it_up_writes() {
        let ok = |p: &[&str]| restore_target(&s(p));
        assert_eq!(
            ok(&["sessions", "eeg.ndjson"]),
            Some((RestoreRoot::Data, "sessions/eeg.ndjson".into()))
        );
        assert_eq!(
            ok(&["goals.json"]),
            Some((RestoreRoot::Data, "goals.json".into()))
        );
        assert_eq!(
            ok(&["nexus-config", "settings.json"]),
            Some((RestoreRoot::Data, "settings.json".into()))
        );
        // Old backups may still hold an `openclaw/` folder: nothing in it is ever restored.
        for bad in [
            &["openclaw", "openclaw.json"][..],
            &["openclaw", "skills", "brief.md"],
            &["openclaw", "workspace", "TASKS.md"],
            &["openclaw", "cron", "jobs.json"],
            &["openclaw", "memory", "2026-10-28.md"],
            &["openclaw", "credentials", "telegram.json"],
            &["openclaw", "skills", "evil.sh"],
            &["nexus-config", "restore-backups", "x.json"],
            &["random.txt"],
        ] {
            assert_eq!(ok(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn plan_skips_everything_off_the_list() {
        let pack = tmp("plan");
        put(&pack, "manifest.json", "{}");
        put(&pack, "openclaw/openclaw.json", "{}");
        put(&pack, "openclaw/skills/a.md", "# a");
        put(&pack, "openclaw/credentials/token.json", "{}");
        put(&pack, "extra/x", "x");
        put(&pack, "goals.json", "top");
        put(&pack, "nexus-config/goals.json", "nested");
        let plan = plan_restore(&pack).unwrap();
        let goals: Vec<_> = plan
            .items
            .iter()
            .filter(|i| i.rel == Path::new("goals.json"))
            .collect();
        assert_eq!(goals.len(), 1, "one write per destination");
        assert_eq!(fs::read_to_string(&goals[0].src).unwrap(), "top");
        assert!(
            plan.items.iter().all(|i| !i.rel.starts_with("skills")),
            "nothing under openclaw/ is planned"
        );
        assert_eq!(plan.skipped.len(), 4, "{:?}", plan.skipped);
        assert!(plan
            .skipped
            .iter()
            .any(|s| s.starts_with("openclaw/openclaw.json")));
        let _ = fs::remove_dir_all(&pack);
    }

    #[test]
    fn apply_backs_up_replaced_files_and_stays_inside_roots() {
        let base = tmp("apply");
        let pack = base.join("pack");
        put(&pack, "openclaw/openclaw.json", "NEW");
        put(&pack, "goals.json", "[1]");
        let roots = RestoreRoots {
            data: base.join("data"),
        };
        put(&roots.data, "goals.json", "OLD");
        let plan = plan_restore(&pack).unwrap();
        let replaced = replaced_files(&plan, &roots);
        assert_eq!(
            replaced,
            vec![(RestoreRoot::Data, PathBuf::from("goals.json"))]
        );
        let op = restore_operation("b.tar.gz", &plan, &replaced, &roots, "20261028_140000");
        assert!(
            op.contains("1 files into")
                && op.contains("(1 replaced)")
                && op.contains("data/goals.json"),
            "{op}"
        );
        assert!(!op.to_lowercase().contains("openclaw"), "{op}");

        let out = apply_restore(&plan, &roots, "20261028_140000").unwrap();
        assert_eq!(
            fs::read_to_string(roots.data.join("goals.json")).unwrap(),
            "[1]"
        );
        assert_eq!(
            fs::read_to_string(
                roots
                    .data
                    .join("restore-backups/20261028_140000/goals.json")
            )
            .unwrap(),
            "OLD"
        );
        assert!(
            out.iter().any(|l| l.contains("1 replaced files saved")),
            "{out:?}"
        );
        assert!(!roots.data.join("openclaw").exists());
        // No temp files left next to restored files.
        let leftovers: Vec<_> = fs::read_dir(&roots.data)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".restore-"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn restore_refuses_symlinked_destinations() {
        let base = tmp("link");
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let root = base.join("data");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("skills")).unwrap();
        assert!(safe_dest(&root, Path::new("skills/a.md")).is_err());
        std::os::unix::fs::symlink(outside.join("x.json"), root.join("goals.json")).unwrap();
        assert!(safe_dest(&root, Path::new("goals.json")).is_err());
        assert!(safe_dest(&root, Path::new("../x")).is_err());
        assert!(safe_dest(&root, Path::new("/etc/hosts")).is_err());
        assert!(safe_dest(&root, Path::new("workspace/ok.md"))
            .unwrap()
            .starts_with(&root));
        assert_eq!(
            fs::read_dir(&outside).unwrap().count(),
            0,
            "nothing written outside"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_and_file_modes() {
        use std::os::unix::fs::PermissionsExt;
        let base = tmp("modes");
        let d = base.join("staging");
        create_private_dir(&d).unwrap();
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(
            create_private_dir(&d).is_err(),
            "never reuses an existing dir"
        );
        let f = base.join("a.tar.gz");
        create_private_file(&f).unwrap();
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            create_private_file(&f).is_err(),
            "never reuses an existing file"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn pack_root_prefers_the_pack_folder() {
        let base = tmp("root");
        assert_eq!(find_pack_root(&base).unwrap(), base);
        fs::create_dir_all(base.join("nexus-pack-1")).unwrap();
        assert_eq!(find_pack_root(&base).unwrap(), base.join("nexus-pack-1"));
        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    /// A fake home with Desktop/ and a data dir holding goals.json (never the real ~).
    fn fake_home(tag: &str) -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "nexus-pack-test-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(home.join("Desktop")).unwrap();
        let data = home.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("goals.json"), "[1]").unwrap();
        (home.canonicalize().unwrap(), data.canonicalize().unwrap())
    }

    fn archive_entries(path: &str) -> Vec<String> {
        let f = fs::File::open(path).unwrap();
        let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
        a.entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect()
    }

    #[tokio::test]
    async fn packing_never_expects_or_reads_openclaw() {
        let (home, data) = fake_home("no-openclaw");
        let info = pack_at(&home, &data).await.unwrap();
        assert!(info.skipped.is_empty(), "{:?}", info.skipped);
        assert!(!info.includes.iter().any(|i| i.contains("OpenClaw")));
        let entries = archive_entries(&info.path);
        assert!(entries.iter().any(|e| e.ends_with("/goals.json")));
        assert!(!entries.iter().any(|e| e.contains("/openclaw")));
        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn an_existing_dot_openclaw_is_not_backed_up() {
        let (home, data) = fake_home("with-openclaw");
        let oc = home.join(".openclaw");
        fs::create_dir_all(oc.join("skills")).unwrap();
        fs::write(oc.join("openclaw.json"), "{}").unwrap();
        fs::write(oc.join("skills/s.md"), "s").unwrap();
        let info = pack_at(&home, &data).await.unwrap();
        assert!(info.skipped.is_empty(), "{:?}", info.skipped);
        assert!(!info.includes.iter().any(|i| i.contains("OpenClaw")));
        let entries = archive_entries(&info.path);
        assert!(
            !entries.iter().any(|e| e.contains("openclaw")),
            "{entries:?}"
        );
        assert!(oc.join("openclaw.json").is_file(), "left untouched");
        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn a_backup_round_trips_without_openclaw() {
        let (home, data) = fake_home("roundtrip");
        let info = pack_at(&home, &data).await.unwrap();

        let staging = home.join("staging");
        fs::create_dir(&staging).unwrap();
        unpack_archive_checked(
            Path::new(&info.path),
            &staging,
            MAX_UNPACK_TOTAL_BYTES,
            MAX_UNPACK_ENTRIES,
        )
        .unwrap();
        let plan = plan_restore(&find_pack_root(&staging).unwrap()).unwrap();

        let target = home.join("new-machine");
        let roots = RestoreRoots {
            data: target.join("data"),
        };
        fs::create_dir_all(&roots.data).unwrap();
        snapshot_before_restore(&roots, "20261030_120000").unwrap();
        let out = apply_restore(&plan, &roots, "20261030_120000").unwrap();
        assert!(out.iter().any(|l| l.starts_with("Amara OS data:")));
        assert!(!out.iter().any(|l| l.to_lowercase().contains("openclaw")));
        assert_eq!(
            fs::read_to_string(roots.data.join("goals.json")).unwrap(),
            "[1]"
        );
        assert!(
            !target.join(".openclaw").exists(),
            "no ~/.openclaw is created"
        );
        let _ = fs::remove_dir_all(&home);
    }
}
