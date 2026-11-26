use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Shared app-data dir (honours `NEXUS_DATA_DIR`).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

#[derive(Serialize, specta::Type)]
pub struct PackageInfo {
    pub path: String,
    pub size_mb: f64,
    pub includes: Vec<String>,
    /// Files left out of the backup (too large or symlinks), never silent.
    pub skipped: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn pack_it_up() -> Result<PackageInfo, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    pack_at(&home, &nexus_data_dir()).await
}

/// `pack_it_up` with the home and data dirs injected (tests use temp dirs).
pub(crate) async fn pack_at(home: &Path, data_dir: &Path) -> Result<PackageInfo, String> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive_name = format!("nexus-backup-{}.tar.gz", timestamp);
    // Taisun, 2026-11-12: the Desktop archive leaves secrets out too (key file, tokens, .env, and
    // secret fields of every JSON store), like Back Up Now. A restore of it asks for those again.
    pack_to(
        data_dir,
        &home.join("Desktop"),
        &archive_name,
        &PackOpts {
            exclude_secrets: true,
            progress: None,
            ..Default::default()
        },
    )
    .await
}

/// Progress callback: `(phase label, percent 0..=100)`.
pub(crate) type Progress<'a> = &'a (dyn Fn(&str, u8) + Send + Sync);

/// How `pack_to` differs between the one-off Desktop pack and a scheduled-style backup.
#[derive(Default)]
pub(crate) struct PackOpts<'a> {
    /// Leave every secret file out and strip secret fields from `config.json`
    /// (see `backups::EXCLUDED_*`). Both pack_it_up and Back Up Now set it (2026-11-12).
    pub exclude_secrets: bool,
    pub progress: Option<Progress<'a>>,
    /// Tests only: pretend this recording file is open. `None` asks the live recorder.
    pub active_recording: Option<String>,
}

/// Pack the app data into `<out_dir>/<file_name>` (a `.tar.gz` whose top folder is
/// `nexus-pack-*`, which restore looks for). `out_dir` must already exist; the archive is
/// created 0600 and never overwrites an existing file.
pub(crate) async fn pack_to(
    data_dir: &Path,
    out_dir: &Path,
    file_name: &str,
    opts: &PackOpts<'_>,
) -> Result<PackageInfo, String> {
    let report = |phase: &str, pct: u8| {
        if let Some(p) = opts.progress {
            p(phase, pct);
        }
    };
    // An open Muse recording is still being written: its file and its `.bands.json` sidecar are left
    // out (and listed as skipped) rather than copied half-written.
    let active = match &opts.active_recording {
        Some(n) => Some(n.clone()),
        None => super::replay::active_recording_name().await,
    };
    let active_names: Vec<String> = active
        .into_iter()
        .flat_map(|n| [format!("{n}.bands.json"), n])
        .collect();
    let exclude_secrets = opts.exclude_secrets;
    let skip_secret = move |name: &str, depth: usize| {
        active_names.iter().any(|a| a == name)
            || (exclude_secrets && super::backups::is_excluded_name(name, depth))
    };
    let skip_secret: &dyn Fn(&str, usize) -> bool = &skip_secret;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let archive_path = out_dir.join(file_name);
    report("Preparing", 3);

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
        report("Copying settings", 10);
        copy_dir_filtered(
            &config_src,
            &dest,
            Some(MAX_PACK_FILE_BYTES),
            &mut skipped,
            0,
            skip_secret,
            if opts.exclude_secrets {
                JsonMode::Redact
            } else {
                JsonMode::Copy
            },
        )?;
        if opts.exclude_secrets {
            super::backups::write_redacted_config(&config_src, &dest, &mut skipped)?;
        }
        includes.push("Amara OS settings & config".to_string());
    }

    // 3. Copy local session data
    let sessions_dir = data_dir.join("sessions");
    if sessions_dir.exists() {
        let dest = staging.join("sessions");
        report("Copying recordings", 40);
        // Muse recordings are copied byte-for-byte: no redaction or rewrite pass (a secret-looking
        // NAME still keeps a file out).
        copy_dir_filtered(
            &sessions_dir,
            &dest,
            Some(MAX_PACK_FILE_BYTES),
            &mut skipped,
            1,
            skip_secret,
            JsonMode::Muse,
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
    report("Copying databases", 60);
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

    // 0052: the execution ledger, as one consistent snapshot (VACUUM INTO copies a live WAL
    // database safely; a raw file copy could miss the WAL).
    let execution_db = data_dir.join("execution.db");
    if execution_db.exists() {
        let dest = staging.join("execution.db");
        rusqlite::Connection::open(&execution_db)
            .and_then(|c| c.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()]))
            .map_err(|e| format!("Failed to copy execution.db: {e}"))?;
        includes.push("Execution ledger".to_string());
    }

    // 5. Write a manifest with metadata
    report("Writing manifest", 70);
    // Hash every staged file (the manifest itself excluded) so a backup can be verified later.
    let sha256 = hash_tree(&staging)?;
    let manifest = serde_json::json!({
        "version": "0.1.0",
        "sha256": &sha256,
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
    report("Compressing", 75);
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

/// sha256 (hex) of every regular file under `root`, keyed by its `/`-joined path relative to `root`.
/// Symlinks and special files are not followed or listed (the copy step never stages them).
fn hash_tree(root: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for e in fs::read_dir(&dir).map_err(|e| format!("Failed to hash backup: {e}"))? {
            let e = e.map_err(|e| format!("Failed to hash backup: {e}"))?;
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let ft = e
                .file_type()
                .map_err(|e| format!("Failed to hash backup: {e}"))?;
            if ft.is_dir() {
                stack.push((e.path(), rel));
            } else if ft.is_file() {
                let mut f =
                    fs::File::open(e.path()).map_err(|e| format!("Failed to hash backup: {e}"))?;
                let mut h = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                loop {
                    let n = f
                        .read(&mut buf)
                        .map_err(|e| format!("Failed to hash backup: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                out.insert(rel, hex::encode(h.finalize()));
            }
        }
    }
    Ok(out)
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

/// What the approval dialog names: the full canonical path, so it cannot be mistaken for another file.
fn restore_archive_label(canonical: &Path) -> String {
    canonical.display().to_string()
}

/// Restore from a file picked by hand (the legacy Settings flow). The file must be a plain file with
/// a backup name, no backup may be running, and the archive must verify, all before the approval
/// dialog, which shows the full canonical path.
#[tauri::command]
#[specta::specta]
pub async fn unpack_backup(
    app: tauri::AppHandle,
    archive_path: String,
) -> Result<Vec<String>, String> {
    let archive = expand_home(archive_path.trim());
    if !archive.is_file() {
        return Err("Archive file not found".to_string());
    }
    let to_check = archive.clone();
    let canonical = tokio::task::spawn_blocking(move || {
        super::backup_schedule::restore_check_archive(
            &to_check,
            &super::backups::BUSY,
            &super::backups::VERIFY_BUSY,
        )
    })
    .await
    .map_err(|_| "io_error".to_string())??;
    unpack_verified(&app, canonical).await
}

/// Restore a backup chosen by id from the backup folder. `file` is a name from `backup_list`, never
/// a path. Refused while a backup runs or when the archive does not verify, before any approval.
#[tauri::command]
#[specta::specta]
pub async fn restore_backup(app: tauri::AppHandle, file: String) -> Result<Vec<String>, String> {
    let (home, data) = (super::backups::home_dir()?, crate::app_paths::data_dir());
    let canonical = tokio::task::spawn_blocking(move || {
        let s = super::backups::load_settings(&data)?;
        super::backup_schedule::restore_precheck_in(
            &home,
            &data,
            &s.dir,
            &file,
            &super::backups::BUSY,
            &super::backups::VERIFY_BUSY,
        )
    })
    .await
    .map_err(|_| "io_error".to_string())??;
    unpack_verified(&app, canonical).await
}

/// Shared restore: extract to staging, plan, native approval (TTL, recheck), safety copy, apply.
async fn unpack_verified(app: &tauri::AppHandle, archive: PathBuf) -> Result<Vec<String>, String> {
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
    let archive_name = restore_archive_label(&archive);

    let asked_at = std::time::Instant::now();
    let _approval = super::approval_gate::require_approval(
        app,
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
pub(crate) fn find_pack_root(staging: &Path) -> Result<PathBuf, String> {
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

/// Live approval state a restore never overwrites: a crafted backup must not be able to switch the
/// Telegram relay on without its approval dialog (`notify_relay.json`) or reset its once-a-day
/// ledger (`notify_relay_sent.json`), or turn outbound on (`outbound_setting.json`).
pub(crate) const RESTORE_NEVER: &[&str] = &[
    "notify_relay.json",
    "notify_relay_sent.json",
    super::outbound::FILE_NAME,
    super::supervisor::act::FILE_NAME,
];

fn is_live_only(rel: &[String]) -> bool {
    rel.last()
        .is_some_and(|n| RESTORE_NEVER.contains(&n.to_ascii_lowercase().as_str()))
}

/// Map an archive file (path relative to the pack root) to where it may be restored.
pub(crate) fn restore_target(rel: &[String]) -> Option<(RestoreRoot, PathBuf)> {
    let join = |parts: &[String]| parts.iter().collect::<PathBuf>();
    if is_live_only(rel) {
        return None;
    }
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
                "goals.json" | "events.db" | "events.db-wal" | "events.db-shm" | "execution.db"
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
                if is_live_only(&rel) {
                    plan.skipped.push(format!(
                        "{} (live approval state, never restored)",
                        rel.join("/")
                    ));
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
    let mut config_skipped: Vec<String> = Vec::new();
    for item in &plan.items {
        let root = roots.get(item.root);
        let dest = safe_dest(root, &item.rel)?;
        // A JSON file restored over a live one keeps the live secret fields the backup left out
        // (backups are written without secrets), so a restore never wipes this Mac's tokens.
        let merged = (item.rel.extension().is_some_and(|e| e == "json") && dest.is_file())
            .then(|| {
                let (r, c) = (fs::read(&item.src).ok()?, fs::read(&dest).ok()?);
                super::backups::merge_kept_secrets(&r, &c)
            })
            .flatten();
        // `config.json` is ALWAYS rewritten, never raw-copied: `allow_claude` (Settings > Assistant
        // > Allow Claude) is a consent that sends data off the Mac, so a backup never turns it on.
        // Only a valid live file with an exact true/false keeps its value; otherwise it is dropped.
        // A backup config that isn't a JSON object is skipped and listed.
        let config_bytes = if item.rel == Path::new("config.json") {
            match sanitised_config(merged.clone(), &item.src, &dest) {
                Some(b) => Some(b),
                None => {
                    config_skipped.push(
                        "config.json (not a valid settings file, left as it was)".to_string(),
                    );
                    continue;
                }
            }
        } else {
            None
        };
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
        match config_bytes.or(merged) {
            Some(bytes) => crate::app_paths::write_private(&tmp, bytes)
                .map_err(|e| format!("restore {}: {}", item.rel.display(), e))?,
            None => {
                fs::copy(&item.src, &tmp)
                    .map_err(|e| format!("restore {}: {}", item.rel.display(), e))?;
            }
        }
        fs::rename(&tmp, &dest).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("restore {}: {}", item.rel.display(), e)
        })?;
        *counts.entry(item.root).or_insert(0usize) += 1;
    }
    let mut out = Vec::new();
    out.extend(config_skipped.into_iter().map(|s| format!("Skipped: {s}")));
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

/// The `config.json` bytes a restore writes: the backup's settings (already merged with the live
/// secrets when `restored` is `Some`) with `allow_claude` taken only from a valid live file and only
/// when it is exactly `true` or `false`; otherwise the field is removed. `None` when the backup's
/// config isn't a JSON object.
fn sanitised_config(restored: Option<Vec<u8>>, src: &Path, live: &Path) -> Option<Vec<u8>> {
    use serde_json::Value;
    let bytes = match restored {
        Some(b) => b,
        None => fs::read(src).ok()?,
    };
    let mut r: Value = serde_json::from_slice(&bytes).ok()?;
    let live_consent = fs::read(live)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.get("allow_claude").and_then(Value::as_bool));
    let map = r.as_object_mut()?;
    match live_consent {
        Some(v) => map.insert("allow_claude".into(), Value::Bool(v)),
        None => map.remove("allow_claude"),
    };
    serde_json::to_vec_pretty(&r).ok()
}

/// New directory readable only by the user (0700 on Unix); fails if it exists.
pub(crate) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut b = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path)
}

/// New empty file readable only by the user (0600 on Unix); fails if it exists.
pub(crate) fn create_private_file(path: &Path) -> std::io::Result<()> {
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
#[specta::specta]
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
    copy_dir_filtered(
        src,
        dst,
        max_file_bytes,
        skipped,
        0,
        &|_, _| false,
        JsonMode::Copy,
    )
}

/// What `copy_dir_filtered` does with `.json` files.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonMode {
    /// Copy as is.
    Copy,
    /// Parse and write without secret fields (a backup that excludes secrets).
    Redact,
    /// A Muse recording folder: byte-for-byte copies only; a JSON with a secret-looking field is
    /// left out and listed.
    Muse,
}

/// The data-root folders that hold Muse recordings.
fn is_muse_dir(name: &str) -> bool {
    matches!(name, "replays" | "sessions" | "muse")
}

/// `copy_dir_recursive` with an exclusion hook: `exclude(name, depth)` true leaves the
/// entry (file or folder) out and records its name (never its contents) in `skipped`.
/// `redact_json`: every `*.json` file is parsed and written without its secret fields
/// (`backups::redact_json_bytes`); one that can't be parsed is left out and noted. Used only
/// when a backup excludes secrets.
#[allow(clippy::too_many_arguments)]
fn copy_dir_filtered(
    src: &Path,
    dst: &Path,
    max_file_bytes: Option<u64>,
    skipped: &mut Vec<String>,
    depth: usize,
    exclude: &dyn Fn(&str, usize) -> bool,
    mode: JsonMode,
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
        if exclude(&name, depth) {
            skipped.push(format!("{name} (left out on purpose)"));
            continue;
        }

        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            skipped.push(format!("{} (symlink)", src_path.display()));
            continue;
        }
        if file_type.is_dir() {
            // Muse recordings (`replays`, `sessions`, `muse` at the data root) are sacred: never
            // redacted or rewritten (see `JsonMode::Muse`).
            let sub_mode = if mode == JsonMode::Redact && depth == 0 && is_muse_dir(&name) {
                JsonMode::Muse
            } else {
                mode
            };
            copy_dir_filtered(
                &src_path,
                &dst_path,
                max_file_bytes,
                skipped,
                depth + 1,
                exclude,
                sub_mode,
            )?;
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
        if mode == JsonMode::Muse && name.to_ascii_lowercase().ends_with(".json") {
            // `.bands.json` sidecars and every non-JSON recording file fall through to the raw copy
            // below. A session/muse JSON is copied byte-for-byte only when it holds no secret-looking
            // field anywhere; one that does is left out (never redacted, never copied raw). JSON that
            // doesn't parse is EEG data, not config: copied raw and noted.
            if !name.to_ascii_lowercase().ends_with(".bands.json") {
                let bytes = fs::read(&src_path).map_err(|e| format!("read {name}: {e}"))?;
                match super::backups::redact_json_bytes(&bytes) {
                    Some((_, removed)) if !removed.is_empty() => {
                        skipped.push(format!(
                            "{} (Muse JSON with a secret-looking field, left out of the backup; the original is untouched)",
                            src_path.display()
                        ));
                        continue;
                    }
                    Some(_) => {}
                    None => skipped.push(format!(
                        "{} (Muse JSON that is not valid JSON, copied as is)",
                        src_path.display()
                    )),
                }
            }
        }
        if mode == JsonMode::Redact && name.to_ascii_lowercase().ends_with(".json") {
            let bytes = fs::read(&src_path).map_err(|e| format!("read {name}: {e}"))?;
            match super::backups::redact_json_bytes(&bytes) {
                Some((clean, removed)) => {
                    for k in removed {
                        skipped.push(format!("{name} field {k} (secret, removed)"));
                    }
                    crate::app_paths::write_private(&dst_path, clean)
                        .map_err(|e| format!("write {name}: {e}"))?;
                }
                None => skipped.push(format!("{name} (not valid JSON, left out)")),
            }
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

    fn restore_config(src: Option<&str>, live: Option<&str>) -> (Option<String>, Vec<String>) {
        let pack = tmp("cfg-pack");
        let data = tmp("cfg-data");
        if let Some(c) = src {
            put(&pack, "nexus-config/config.json", c);
        }
        put(&pack, "goals.json", "[]");
        if let Some(l) = live {
            put(&data, "config.json", l);
        }
        let plan = plan_restore(&pack).unwrap();
        let out = apply_restore(&plan, &RestoreRoots { data: data.clone() }, "st").unwrap();
        let got = fs::read_to_string(data.join("config.json")).ok();
        let _ = (fs::remove_dir_all(&pack), fs::remove_dir_all(&data));
        (got, out)
    }

    #[test]
    fn config_json_is_always_sanitised_never_raw_copied() {
        let on = r#"{"allow_claude":true,"theme":"light"}"#;
        let allow = |g: &Option<String>| {
            serde_json::from_str::<serde_json::Value>(g.as_deref().unwrap()).unwrap()
                ["allow_claude"]
                .clone()
        };
        // no live file: the consent is dropped, the rest restored
        let (g, _) = restore_config(Some(on), None);
        assert!(allow(&g).is_null());
        assert!(g.unwrap().contains("light"));
        // corrupt live file
        let (g, _) = restore_config(Some(on), Some("{not json"));
        assert!(allow(&g).is_null());
        // live file is valid but its value isn't exactly true/false
        let (g, _) = restore_config(Some(on), Some(r#"{"allow_claude":"true"}"#));
        assert!(allow(&g).is_null());
        let (g, _) = restore_config(Some(on), Some(r#"{"allow_claude":1}"#));
        assert!(allow(&g).is_null());
        // live file is an array
        let (g, _) = restore_config(Some(on), Some("[true]"));
        assert!(allow(&g).is_null());
        // valid live file: its exact value wins, both ways
        let (g, _) = restore_config(Some(on), Some(r#"{"allow_claude":false}"#));
        assert_eq!(allow(&g), serde_json::json!(false));
        let (g, _) = restore_config(
            Some(r#"{"allow_claude":false}"#),
            Some(r#"{"allow_claude":true}"#),
        );
        assert_eq!(allow(&g), serde_json::json!(true));
        // non-object backup config: skipped and listed, the live file untouched
        for bad in ["[1]", "\"x\"", "42", "{broken", "null"] {
            let (g, out) = restore_config(Some(bad), Some(r#"{"allow_claude":true}"#));
            assert_eq!(g.as_deref(), Some(r#"{"allow_claude":true}"#), "{bad}");
            assert!(
                out.iter().any(|l| l.starts_with("Skipped: config.json")),
                "{bad}: {out:?}"
            );
        }
        // ...and with no live file nothing is created from a bad config
        let (g, out) = restore_config(Some("[1]"), None);
        assert!(g.is_none() && out.iter().any(|l| l.contains("config.json")));
    }

    #[test]
    fn restore_never_writes_the_relay_files_or_turns_on_allow_claude() {
        let pack = tmp("never-pack");
        let data = tmp("never-data");
        for n in ["notify_relay.json", "notify_relay_sent.json"] {
            put(
                &pack,
                &format!("nexus-config/{n}"),
                "{\"enabled\":true,\"CRAFTED\":1}",
            );
            put(&data, n, &format!("live-{n}"));
        }
        put(
            &pack,
            "nexus-config/NOTIFY_RELAY.json",
            "{\"enabled\":true}",
        );
        put(&pack, "nexus-config/other.json", "{\"v\":2}");
        put(
            &pack,
            "nexus-config/config.json",
            "{\"allow_claude\":true,\"theme\":\"light\"}",
        );
        put(&pack, "goals.json", "[2]");
        put(&data, "other.json", "{\"v\":1}");
        put(&data, "config.json", "{\"theme\":\"dark\"}");
        put(&data, "goals.json", "[1]");
        let plan = plan_restore(&pack).unwrap();
        let skipped = plan.skipped.join("|");
        assert!(skipped.contains("notify_relay.json (live approval state"));
        assert!(skipped.contains("notify_relay_sent.json (live approval state"));
        assert!(plan.items.iter().all(|i| !i
            .rel
            .to_string_lossy()
            .to_lowercase()
            .contains("notify_relay")));
        let roots = RestoreRoots { data: data.clone() };
        apply_restore(&plan, &roots, "stamp").unwrap();
        for n in ["notify_relay.json", "notify_relay_sent.json"] {
            assert_eq!(
                fs::read_to_string(data.join(n)).unwrap(),
                format!("live-{n}")
            );
        }
        // everything else came back
        assert_eq!(
            fs::read_to_string(data.join("goals.json"))
                .unwrap()
                .split_whitespace()
                .collect::<String>(),
            "[2]"
        );
        assert!(fs::read_to_string(data.join("other.json"))
            .unwrap()
            .contains("2"));
        let cfg: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join("config.json")).unwrap()).unwrap();
        assert_eq!(cfg["theme"], "light");
        assert!(
            cfg.get("allow_claude").is_none(),
            "a backup never turns Allow Claude on"
        );
        // a live "on" stays on
        put(&data, "config.json", "{\"allow_claude\":true}");
        let plan = plan_restore(&pack).unwrap();
        apply_restore(&plan, &roots, "stamp2").unwrap();
        let cfg: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join("config.json")).unwrap()).unwrap();
        assert_eq!(cfg["allow_claude"], true);
        assert_eq!(cfg["theme"], "light");
        let _ = (fs::remove_dir_all(&pack), fs::remove_dir_all(&data));
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
    fn approval_text_shows_the_full_canonical_path() {
        let pack = tmp("label");
        let canonical = pack
            .canonicalize()
            .unwrap()
            .join("nexus-backup-20261029_101500.tar.gz");
        let label = restore_archive_label(&canonical);
        let plan = RestorePlan {
            items: vec![],
            skipped: vec![],
        };
        let roots = RestoreRoots {
            data: pack.join("data"),
        };
        let op = restore_operation(&label, &plan, &[], &roots, "20261029_101600");
        assert!(op.contains(&canonical.display().to_string()), "{op}");
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

/// Phase 16: migration probe on SYNTHETIC data. Builds a fake app-data dir in a temp folder, backs it
/// up with the app's own backup path (`backups::run_backup`), restores that archive into a SECOND temp
/// dir with the app's own restore steps (`unpack_archive_checked`, `plan_restore`, `apply_restore`),
/// and checks parity. Never reads the real app data, the Keychain, a real backup or the network.
/// `scripts/migration-probe.sh` runs it with `--nocapture` and prints the `PROBE` lines.
#[cfg(test)]
mod migration_probe_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-migration-probe-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    /// rel path (`/`-joined) -> bytes, skipping the restore-backups folder.
    fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let t = e.file_type().unwrap();
                if t.is_dir() {
                    stack.push(e.path());
                } else if t.is_file() {
                    let rel = e
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !rel.starts_with(RESTORE_BACKUP_DIR) {
                        out.insert(rel, fs::read(e.path()).unwrap());
                    }
                }
            }
        }
        out
    }

    fn make_db(path: &Path) -> usize {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(
            "PRAGMA journal_mode=DELETE;
             CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT, at TEXT);
             CREATE INDEX events_kind ON events(kind);",
        )
        .unwrap();
        for i in 0..250 {
            c.execute(
                "INSERT INTO events (kind, at) VALUES (?1, ?2)",
                rusqlite::params![
                    format!("fixture-{}", i % 7),
                    format!("2026-01-01T00:{:02}:00", i % 60)
                ],
            )
            .unwrap();
        }
        250
    }

    fn db_probe(path: &Path) -> (String, i64) {
        let c =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let check: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        (check, n)
    }

    /// Fixture bytes that look like data but are fake: counters and a repeating pattern.
    fn blob(seed: u8, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| seed.wrapping_add((i % 251) as u8))
            .collect()
    }

    #[test]
    fn migration_probe_backup_restore_parity_on_synthetic_data() {
        let home = scratch("home");
        let data = home.join("app-data-original");
        let restored = home.join("app-data-restored");
        fs::create_dir_all(&data).unwrap();
        for d in ["replays", "sessions", "muse", "takes", "workspace/notes"] {
            fs::create_dir_all(data.join(d)).unwrap();
        }
        // ---- fixture data (invented; nothing from a real install) ----
        fs::write(
            data.join("config.json"),
            r#"{"theme":"dark","llm_token_threshold":4000,"telegram_bot_token":"FIXTURE-NOT-A-SECRET","allow_claude":true}"#,
        )
        .unwrap();
        fs::write(
            data.join("goals.json"),
            r#"[{"id":"g1","title":"fixture goal"}]"#,
        )
        .unwrap();
        fs::write(
            data.join("takes/take-1.json"),
            r#"{"text":"fixture take","day":"2026-01-01"}"#,
        )
        .unwrap();
        fs::write(data.join("workspace/notes/n1.md"), "# fixture note\n").unwrap();
        fs::write(data.join("replays/eeg_fixture_1.ndjson"), blob(1, 40_000)).unwrap();
        fs::write(
            data.join("replays/eeg_fixture_1.ndjson.bands.json"),
            r#"{"a":[0.1,0.25]}"#,
        )
        .unwrap();
        fs::write(
            data.join("sessions/session_fixture.json"),
            r#"{"id":"s1","hr":[60,61,62]}"#,
        )
        .unwrap();
        fs::write(data.join("muse/fixture.bin"), blob(9, 5_000)).unwrap();
        let db_rows = make_db(&data.join("events.db"));
        // Secrets that must never travel (all fake).
        fs::write(data.join("crew_service_token"), "FIXTURE").unwrap();
        fs::write(data.join("amara-n8n.key"), "FIXTURE").unwrap();
        fs::write(data.join(".env"), "HA_TOKEN=FIXTURE\n").unwrap();
        // Live-only state a restore must never bring back.
        fs::write(data.join("notify_relay.json"), r#"{"enabled":true}"#).unwrap();

        let src_before = tree(&data);

        // ---- 1. backup with the app's own code path ----
        let settings = super::super::backups::BackupSettings {
            dir: "~/Amara Backups".into(),
            ..Default::default()
        };
        let busy = AtomicBool::new(false);
        let quiet = |_: &str, _: u8| {};
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let run = rt
            .block_on(super::super::backups::run_backup(
                &home,
                &data,
                &settings,
                &|_| Ok(u64::MAX / 2),
                &quiet,
                &busy,
            ))
            .unwrap();
        let archive = home.join("Amara Backups").join(&run.entry.id);
        use std::os::unix::fs::PermissionsExt;
        let amode = fs::metadata(&archive).unwrap().permissions().mode() & 0o777;
        let dmode = fs::metadata(home.join("Amara Backups"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(amode, 0o600, "archive mode");
        assert_eq!(dmode, 0o700, "archive dir mode");
        println!("PROBE archive_modes archive={amode:o} dir={dmode:o} ok");

        // ---- 2. restore preview: names only ----
        let nb = AtomicBool::new(false);
        let nb2 = AtomicBool::new(false);
        let preview = super::super::backup_schedule::restore_preview_in(
            &home,
            &data,
            "~/Amara Backups",
            &run.entry.id,
            &nb,
            &nb2,
        )
        .unwrap();
        assert!(preview.verify.ok, "{:?}", preview.verify);
        assert!(!preview.reenter.is_empty());
        let names: Vec<&str> = preview.reenter.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"telegram_bot_token") && names.contains(&"n8n_encryption_key"));
        for r in &preview.reenter {
            // names and labels only: no value-looking text
            assert!(!r.name.contains("FIXTURE") && !r.label.contains("FIXTURE"));
        }
        println!(
            "PROBE keys_to_reenter count={} names={} ok",
            names.len(),
            names.join(",")
        );

        // ---- 3. restore into a SECOND dir with the app's own steps ----
        let staging = scratch("staging");
        unpack_archive_checked(&archive, &staging, 1 << 30, 100_000).unwrap();
        let pack = find_pack_root(&staging).unwrap();
        // manifest hashes vs the unpacked pack
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(pack.join("manifest.json")).unwrap()).unwrap();
        let listed = manifest["sha256"].as_object().unwrap();
        let packed = tree(&pack);
        let mut manifest_bad = 0usize;
        for (k, v) in listed {
            match packed.get(k) {
                Some(b) if Some(sha(b).as_str()) == v.as_str() => {}
                _ => manifest_bad += 1,
            }
        }
        let packed_files = packed.keys().filter(|k| *k != "manifest.json").count();
        assert_eq!(manifest_bad, 0);
        assert_eq!(
            listed.len(),
            packed_files,
            "manifest lists every packed file"
        );
        println!(
            "PROBE manifest_hashes listed={} matching={} ok",
            listed.len(),
            listed.len() - manifest_bad
        );

        let plan = plan_restore(&pack).unwrap();
        let roots = RestoreRoots {
            data: restored.clone(),
        };
        let msgs = apply_restore(&plan, &roots, "probe").unwrap();
        assert!(
            msgs.iter().any(|m| m.contains("files restored")),
            "{msgs:?}"
        );

        // ---- 4. parity probes ----
        let dst = tree(&restored);
        // 4a. what must be absent in the restore
        let never_travel = [
            "crew_service_token",
            "amara-n8n.key",
            ".env",
            "notify_relay.json",
        ];
        for n in never_travel {
            assert!(!dst.contains_key(n), "{n} must not be restored");
        }
        // 4b. everything else: present, byte-identical, except config.json (sanitised)
        let (mut same, mut checked, mut json_semantic) = (0usize, 0usize, 0usize);
        let mut counts_src: BTreeMap<String, usize> = BTreeMap::new();
        let mut counts_dst: BTreeMap<String, usize> = BTreeMap::new();
        for (k, v) in &src_before {
            if never_travel.contains(&k.as_str()) || k == "config.json" {
                continue;
            }
            checked += 1;
            let top = k.split('/').next().unwrap().to_string();
            *counts_src.entry(top.clone()).or_default() += 1;
            let got = dst
                .get(k)
                .unwrap_or_else(|| panic!("missing after restore: {k}"));
            // Muse folders and every non-JSON file: byte-identical. Other JSON goes through the
            // secret-field redaction pass, which may re-serialise it: compare the parsed value.
            let muse = ["replays/", "sessions/", "muse/"]
                .iter()
                .any(|p| k.starts_with(p));
            if k.ends_with(".json") && !muse {
                let (a, b): (serde_json::Value, serde_json::Value) = (
                    serde_json::from_slice(got).unwrap(),
                    serde_json::from_slice(v).unwrap(),
                );
                assert_eq!(a, b, "json differs: {k}");
                json_semantic += 1;
            } else {
                assert_eq!(sha(got), sha(v), "checksum differs: {k}");
            }
            *counts_dst.entry(top).or_default() += 1;
            same += 1;
        }
        let extra: Vec<_> = dst
            .keys()
            .filter(|k| *k != "config.json" && !src_before.contains_key(*k))
            .collect();
        assert!(extra.is_empty(), "unexpected extra files: {extra:?}");
        assert_eq!(counts_src, counts_dst);
        let groups = counts_src
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        println!("PROBE file_parity files={checked} matching={same} (json compared as parsed value: {json_semantic}) extra=0 groups={groups} ok");
        // 4c. config.json: no secret field, allow_claude never turned on, other fields kept
        let cfg: serde_json::Value =
            serde_json::from_slice(dst.get("config.json").expect("config.json restored")).unwrap();
        assert!(cfg.get("telegram_bot_token").is_none());
        assert!(cfg
            .get("allow_claude")
            .is_none_or(|v| v != &serde_json::Value::Bool(true)));
        assert_eq!(cfg["theme"], "dark");
        assert_eq!(cfg["llm_token_threshold"], 4000);
        println!("PROBE config_json secret_fields=0 allow_claude_on=false kept_fields=ok ok");
        // 4d. database integrity + row count
        let (check_src, n_src) = db_probe(&data.join("events.db"));
        let (check_dst, n_dst) = db_probe(&restored.join("events.db"));
        assert_eq!(check_src, "ok");
        assert_eq!(check_dst, "ok", "integrity_check on the restored db");
        assert_eq!(n_src, db_rows as i64);
        assert_eq!(n_dst, n_src);
        println!("PROBE sqlite events.db integrity_check={check_dst} rows={n_dst} (source rows={n_src}) ok");
        // 4e. no path of the source Mac is baked into the restored data
        let src_str = data.to_string_lossy().into_owned();
        let home_str = home.to_string_lossy().into_owned();
        let hits = dst
            .values()
            .filter(|b| {
                let s = String::from_utf8_lossy(b);
                s.contains(&src_str) || s.contains(&home_str)
            })
            .count();
        assert_eq!(hits, 0, "restored files contain an absolute source path");
        println!("PROBE no_absolute_paths hits=0 ok");
        // 4f. file modes under the restored dir
        let leaky = fs::read_dir(&restored)
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().unwrap().is_file())
            .filter(|e| {
                e.metadata().unwrap().permissions().mode() & 0o077 != 0
                    && e.file_name() == "config.json"
            })
            .count();
        assert_eq!(leaky, 0, "config.json restored with group/world bits");
        println!("PROBE config_mode group_world_bits=0 ok");
        // ---- 5. the source was not modified by the whole exercise ----
        assert_eq!(tree(&data), src_before, "source dir changed");
        println!("PROBE source_untouched ok");

        for d in [&home, &staging] {
            let _ = fs::remove_dir_all(d);
        }
    }
}
