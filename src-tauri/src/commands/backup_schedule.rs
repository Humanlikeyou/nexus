//! Backups, the rest of design-1111s step 3: the in-app nightly scheduler, the prune PLAN and the
//! read-only restore preview.
//!
//! - Nightly: a tokio task started with the app. No LaunchAgent, no cron, nothing while the app is
//!   closed. The slot is `settings.at` in Japan time (JST, UTC+9, no DST). A slot that passed while
//!   the app was closed or busy runs once on the next launch, after `STARTUP_DELAY`, and never while
//!   a Hermes chat turn, a crew run or a Muse recording is active. It is skipped on battery below
//!   `skip_on_battery_below` (`pmset -g batt`; unreadable means AC). One run at a time, shared with
//!   Back Up Now through `backups::BUSY`.
//! - Prune: `prune_plan` only LISTS what would go. Nothing here or anywhere deletes a backup (hard
//!   rule 7: backup prune comes last, with Taisun's OK).
//! - Restore preview: read-only. `restore()` is not built (Taisun's gate).

use super::backups::{
    clean_name, home_dir, list_in, load_settings, parse_backup_name, record_run, resolve_dir,
    verify_archive, BackupSettings, BusyGuard, VerifyReport, BUSY, VERIFY_BUSY, VERIFY_CAPS,
    VERIFY_MAX_ENTRIES, VERIFY_MAX_MANIFEST_BYTES,
};
use super::shared::time::jst;
use chrono::{DateTime, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

// ---- nightly scheduler ----

/// Japan Standard Time: UTC+9, no daylight saving.
/// How long after launch before any run (so a catch-up never competes with startup).
pub(crate) const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// After a failed run: wait this long, and give up on the slot after this many failures.
const RETRY_AFTER: Duration = Duration::hours(1);
const MAX_ATTEMPTS: u32 = 3;
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

fn parse_at(at: &str) -> Option<NaiveTime> {
    let b = at.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return None;
    }
    NaiveTime::from_hms_opt(at[0..2].parse().ok()?, at[3..5].parse().ok()?, 0)
}

/// The latest slot (`at` o'clock JST) at or before `now`.
pub(crate) fn prev_slot(now: DateTime<Utc>, at: &str) -> Option<DateTime<Utc>> {
    let t = parse_at(at)?;
    let nowj = now.with_timezone(&jst());
    let today = jst()
        .from_local_datetime(&nowj.date_naive().and_time(t))
        .single()?;
    let slot = if today > nowj {
        today - Duration::days(1)
    } else {
        today
    };
    Some(slot.with_timezone(&Utc))
}

/// The first slot strictly after `now`.
pub(crate) fn next_run_after(now: DateTime<Utc>, at: &str) -> Option<DateTime<Utc>> {
    prev_slot(now, at).map(|p| p + Duration::days(1))
}

/// The newest slot has no successful backup at or after it. With no backup at all it is missed, so
/// a first backup is made soon after the first launch instead of waiting up to a day.
pub(crate) fn slot_missed(last_done: Option<DateTime<Utc>>, now: DateTime<Utc>, at: &str) -> bool {
    match prev_slot(now, at) {
        Some(slot) => last_done.is_none_or(|l| l < slot),
        None => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Battery {
    pub on_battery: bool,
    pub percent: u32,
}

/// Parse `pmset -g batt`. `None` when it says nothing usable (the caller then assumes AC).
pub(crate) fn parse_pmset(out: &str) -> Option<Battery> {
    let first = out.lines().next()?;
    let on_battery = if first.contains("Battery Power") {
        true
    } else if first.contains("AC Power") {
        false
    } else {
        return None;
    };
    let percent = out
        .split(|c: char| !(c.is_ascii_digit() || c == '%'))
        .find_map(|tok| tok.strip_suffix('%')?.parse::<u32>().ok())
        .unwrap_or(100)
        .min(100);
    Some(Battery {
        on_battery,
        percent,
    })
}

/// Fixed argument list, absolute path, no shell. Unreadable (not macOS, pmset missing) is `None`.
fn read_battery() -> Option<Battery> {
    let out = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_pmset(&String::from_utf8_lossy(&out.stdout))
}

#[derive(Debug, PartialEq)]
pub(crate) enum Decision {
    Wait,
    Run,
    /// Skipped this tick; retried on the next one. The code is what lands in `last_run_result`.
    Skip(&'static str),
}

pub(crate) struct Inputs<'a> {
    pub now: DateTime<Utc>,
    pub since_launch: std::time::Duration,
    pub settings: &'a BackupSettings,
    /// The later of the last successful backup and the newest archive on disk.
    pub last_done: Option<DateTime<Utc>>,
    /// Hermes chat turn, crew run or Muse recording active.
    pub busy: bool,
    pub battery: Option<Battery>,
}

pub(crate) fn decide(i: &Inputs) -> Decision {
    if !i.settings.nightly
        || i.since_launch < STARTUP_DELAY
        || !slot_missed(i.last_done, i.now, &i.settings.at)
    {
        return Decision::Wait;
    }
    // After an error: at most MAX_ATTEMPTS runs per slot, RETRY_AFTER apart.
    if let Some(slot) = prev_slot(i.now, &i.settings.at) {
        let failed_here = i.settings.failed_slot.as_deref() == Some(slot.to_rfc3339().as_str());
        if failed_here {
            if i.settings.failed_attempts >= MAX_ATTEMPTS {
                return Decision::Wait;
            }
            if parse_rfc(&i.settings.last_run_at).is_some_and(|t| i.now - t < RETRY_AFTER) {
                return Decision::Wait;
            }
        }
    }
    if i.busy {
        return Decision::Skip("skipped_busy");
    }
    if let Some(b) = i.battery {
        if b.on_battery && b.percent < i.settings.skip_on_battery_below {
            return Decision::Skip("skipped_battery");
        }
    }
    Decision::Run
}

fn parse_rfc(s: &Option<String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.as_deref()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// A Hermes turn, a crew run, or a Muse recording that is still open.
async fn busy_now() -> bool {
    super::goal_roadmap::system_busy().await
        || super::replay::active_recording_name().await.is_some()
}

async fn tick(app: &tauri::AppHandle, started: std::time::Instant) {
    let (Ok(home), data) = (home_dir(), crate::app_paths::data_dir()) else {
        return;
    };
    let d2 = data.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        let s = load_settings(&d2)?;
        let newest = list_in(&home, &d2, &s.dir, Local::now())?
            .first()
            .and_then(|e| DateTime::parse_from_rfc3339(&e.made_at).ok())
            .map(|d| d.with_timezone(&Utc));
        Ok::<_, String>((s, newest))
    })
    .await;
    let Ok(Ok((settings, newest))) = loaded else {
        return;
    };
    let last_done = parse_rfc(&settings.last_ok_at).max(newest);
    // Cheap checks first: pmset and the crew/Hermes probes only run when a run is due.
    let now = Utc::now();
    let due = Inputs {
        now,
        since_launch: started.elapsed(),
        settings: &settings,
        last_done,
        busy: false,
        battery: None,
    };
    if decide(&due) == Decision::Wait {
        return;
    }
    let busy = busy_now().await;
    let battery = tokio::task::spawn_blocking(read_battery)
        .await
        .ok()
        .flatten();
    let inputs = Inputs {
        busy,
        battery,
        ..due
    };
    match decide(&inputs) {
        Decision::Wait => {}
        Decision::Skip(code) => {
            if settings.last_run_result.as_deref() != Some(code) {
                let at = Local::now().to_rfc3339();
                let _ =
                    tokio::task::spawn_blocking(move || record_run(&data, &at, code, false)).await;
            }
        }
        Decision::Run => {
            // Re-check right before packing: a chat turn, crew run or Muse recording may have
            // started since the checks above.
            if busy_now().await {
                return;
            }
            // `already_running` (Back Up Now holds the flag) is simply tried again next tick.
            let res = super::backups::run_and_record(app).await;
            let ok = res.is_ok();
            // `already_running` is not a failure of this slot: it is tried again next tick.
            if res.as_ref().err().is_none_or(|e| e != "already_running") {
                if let Some(slot) = prev_slot(Utc::now(), &settings.at) {
                    let (d, s, failed) = (data.clone(), slot.to_rfc3339(), !ok);
                    let _ = tokio::task::spawn_blocking(move || {
                        super::backups::record_attempt(&d, &s, failed)
                    })
                    .await;
                }
            }
            if ok && settings.git_bundle {
                run_git_bundle(&settings, &data).await;
            }
        }
    }
}

// ---- optional git bundle (decision 0016; default off, no upload) ----

/// Repo bundled: `~/projects/nexus-amara`.
const BUNDLE_REPO: &str = "projects/nexus-amara";
const BUNDLE_PREFIX: &str = "amara-repo-";

/// `amara-repo-YYYYMMDD-HHMMSS.bundle`: never matches an Amara archive name, so a prune plan and
/// the backup list ignore it.
pub(crate) fn bundle_name(now: DateTime<Local>) -> String {
    format!("{BUNDLE_PREFIX}{}.bundle", now.format("%Y%m%d-%H%M%S"))
}

const GIT: &str = "/usr/bin/git";
const BUNDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// The exact command: `/usr/bin/git` with a cleared environment (so no inherited GIT_*, hooks path
/// or config), then only the variables below. `bundle create - --branches --tags` writes to stdout
/// (committed history of branches and tags only, not stashes, remotes or the index).
pub(crate) fn bundle_command(repo: &Path) -> std::process::Command {
    let mut c = std::process::Command::new(GIT);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C")
        .arg("-C")
        .arg(repo)
        .args(["bundle", "create", "-", "--branches", "--tags"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c
}

/// Create the bundle in `dir`. The output goes to `<name>.part`, opened O_CREAT|O_EXCL|O_NOFOLLOW at
/// 0600, and is renamed to `<name>` only when git exits 0 inside `timeout`. On any failure only that
/// `.part` file is removed. Never overwrites. Returns the file name.
pub(crate) fn create_git_bundle_with(
    repo: &Path,
    dir: &Path,
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let name = bundle_name(Local::now());
    let out = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    if out.exists() {
        return Err("exists".into());
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&part)
        .map_err(|_| "io_error".to_string())?;
    let result = (|| {
        let mut child = bundle_command(repo)
            .stdout(std::process::Stdio::from(file))
            .spawn()
            .map_err(|_| "git_missing".to_string())?;
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) if started.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timeout".to_string());
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => return Err("git_failed".to_string()),
            }
        };
        if !status.success() {
            return Err("git_failed".to_string());
        }
        std::fs::rename(&part, &out).map_err(|_| "io_error".to_string())?;
        Ok(name.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part); // our own .part, nothing else
    }
    result
}

pub(crate) fn create_git_bundle(repo: &Path, dir: &Path) -> Result<String, String> {
    create_git_bundle_with(repo, dir, BUNDLE_TIMEOUT)
}

async fn run_git_bundle(settings: &BackupSettings, data: &Path) {
    let (settings, data) = (settings.clone(), data.to_path_buf());
    let _ = tokio::task::spawn_blocking(move || {
        let result = (|| {
            let home = home_dir()?;
            let dir = resolve_dir(&settings.dir, &home, &data)?;
            create_git_bundle(&home.join(BUNDLE_REPO), &dir).map(|_| ())
        })();
        let code = match result {
            Ok(()) => "ok".to_string(),
            Err(e) => e,
        };
        let _ = super::backups::record_bundle(&data, &code);
    })
    .await;
}

/// Start the nightly scheduler. Called once from the app's `setup`.
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let at = crate::commands::backups::load_settings(&crate::app_paths::data_dir())
            .map(|s| s.at)
            .unwrap_or_else(|_| "03:00".into());
        if let Some(next) = next_run_after(Utc::now(), &at) {
            println!(
                "[backup] nightly scheduler on; next slot {}",
                next.with_timezone(&jst())
            );
        }
        loop {
            tokio::time::sleep(TICK).await;
            tick(&app, started).await;
        }
    });
}

// ---- prune plan (a list, never a delete) ----

const ARCHIVE_PREFIX: &str = "amara-backup-";
const ARCHIVE_SUFFIX: &str = ".tar.gz";
const PRE_UPDATE_TAG: &str = "-pre-update";

/// Amara's own archive names only: `amara-backup-YYYYMMDD-HHMMSS.tar.gz`, or the same with
/// `-pre-update` before the extension. Returns the stamp and whether it is a pre-update one.
/// Legacy `nexus-backup-*` and every other name are not Amara archives for this purpose.
pub(crate) fn amara_archive(name: &str) -> Option<(NaiveDateTime, bool)> {
    let rest = name
        .strip_prefix(ARCHIVE_PREFIX)?
        .strip_suffix(ARCHIVE_SUFFIX)?;
    let (stamp, pre) = match rest.strip_suffix(PRE_UPDATE_TAG) {
        Some(s) => (s, true),
        None => (rest, false),
    };
    let (d, t) = stamp.split_once('-')?;
    if d.len() != 8 || t.len() != 6 || !d.bytes().chain(t.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let at = NaiveDateTime::parse_from_str(&format!("{d}{t}"), "%Y%m%d%H%M%S").ok()?;
    Some((at, pre))
}

#[derive(Serialize, Debug, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrunePlan {
    /// WOULD be deleted (oldest first). Nothing deletes them.
    pub would_delete: Vec<String>,
    /// The newest `keep` ordinary archives.
    pub keep: Vec<String>,
    /// Pre-update archives: always kept, and they don't count toward `keep`.
    pub pre_update_kept: Vec<String>,
}

/// Pure: which of `names` would be pruned with `keep` ordinary archives retained. Names that are
/// not Amara archives are ignored (they appear nowhere in the plan).
pub(crate) fn prune_plan_names<'a>(
    names: impl IntoIterator<Item = &'a str>,
    keep: usize,
) -> PrunePlan {
    let mut ordinary: Vec<(NaiveDateTime, &str)> = Vec::new();
    let mut plan = PrunePlan::default();
    for n in names {
        match amara_archive(n) {
            Some((_, true)) => plan.pre_update_kept.push(n.to_string()),
            Some((t, false)) => ordinary.push((t, n)),
            None => {}
        }
    }
    ordinary.sort();
    plan.pre_update_kept.sort();
    let cut = ordinary.len().saturating_sub(keep);
    for (i, (_, n)) in ordinary.into_iter().enumerate() {
        if i < cut {
            plan.would_delete.push(n.to_string());
        } else {
            plan.keep.push(n.to_string());
        }
    }
    plan
}

/// Read-only: lists `dir` (regular files only; a symlink is never listed) and plans. `keep` of 0
/// is refused so a bad setting can never plan to remove everything.
pub(crate) fn prune_plan(dir: &Path, keep: u32) -> Result<PrunePlan, String> {
    if keep == 0 {
        return Err("invalid_keep".into());
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PrunePlan::default()),
        Err(_) => return Err("io_error".into()),
    };
    let names: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    Ok(prune_plan_names(
        names.iter().map(String::as_str),
        keep as usize,
    ))
}

/// What a prune WOULD delete under the saved `keep`. Deletes nothing, and nothing calls it to.
#[tauri::command]
#[specta::specta]
pub async fn backup_prune_plan() -> Result<PrunePlan, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || {
        let s = load_settings(&data)?;
        let dir = resolve_dir(&s.dir, &home, &data)?;
        prune_plan(&dir, s.keep)
    })
    .await
    .map_err(|_| "io_error".to_string())?
}

// ---- restore preview (read-only) ----

#[derive(Serialize, Debug, Clone, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreGroup {
    pub name: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Reenter {
    pub name: String,
    pub label: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub file: String,
    pub verify: VerifyReport,
    /// Regular files in the archive (the manifest included), and their unpacked size.
    pub files: usize,
    pub bytes: u64,
    pub groups: Vec<RestoreGroup>,
    /// Names the backup left out on purpose or couldn't take (from its manifest; names only).
    pub left_out: Vec<String>,
    /// Keychain items a restore never brings back: type these in again.
    pub reenter: Vec<Reenter>,
    /// The n8n key file is never inside a backup either: restore it from its own passphrase file.
    pub key_file_note: String,
}

/// `nexus-config/<x>/...` counts under `nexus-config/<x>`; anything else under its first folder.
fn group_of(rel: &[&str]) -> String {
    match rel {
        [] => String::new(),
        [only] => clean_name(only),
        [first, second, _, ..] if *first == "nexus-config" => {
            format!("{}/{}", clean_name(first), clean_name(second))
        }
        [first, ..] => clean_name(first),
    }
}

/// Lists an archive without extracting or writing anything.
fn list_archive(path: &Path) -> Result<(usize, u64, Vec<RestoreGroup>, Vec<String>), String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "symlink_refused_or_unreadable".to_string())?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err("not_a_file".into());
    }
    let started = std::time::Instant::now();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut groups: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let (mut files, mut bytes, mut seen) = (0usize, 0u64, 0usize);
    let mut left_out = Vec::new();
    for entry in tar
        .entries()
        .map_err(|_| "archive_unreadable".to_string())?
    {
        let mut entry = entry.map_err(|_| "archive_unreadable".to_string())?;
        seen += 1;
        if seen > VERIFY_MAX_ENTRIES || started.elapsed() > VERIFY_CAPS_DEADLINE {
            return Err("too_many_entries".into());
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rel: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).skip(1).collect();
        if rel.last().is_some_and(|l| l.starts_with("._")) {
            continue; // AppleDouble metadata is never restored
        }
        let size = entry.size();
        files += 1;
        bytes = bytes.saturating_add(size);
        let g = groups.entry(group_of(&rel)).or_default();
        g.0 += 1;
        g.1 = g.1.saturating_add(size);
        if rel == ["manifest.json"] && size <= VERIFY_MAX_MANIFEST_BYTES {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                if let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(&buf) {
                    if let Some(Value::Array(a)) = o.get("skipped") {
                        left_out = a
                            .iter()
                            .filter_map(Value::as_str)
                            .take(50)
                            .map(clean_name)
                            .collect();
                    }
                }
            }
        }
    }
    let groups = groups
        .into_iter()
        .take(200)
        .map(|(name, (files, bytes))| RestoreGroup { name, files, bytes })
        .collect();
    Ok((files, bytes, groups, left_out))
}

const VERIFY_CAPS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(600);

/// Read-only preview of restoring `file` (an id from `backup_list`). Opens the archive read-only,
/// extracts nothing, writes nothing. `file` must be an Amara or legacy backup name, never a path.
pub(crate) fn restore_preview_in(
    home: &Path,
    data_dir: &Path,
    dir_setting: &str,
    file: &str,
    backup_busy: &std::sync::atomic::AtomicBool,
    verify_busy: &std::sync::atomic::AtomicBool,
) -> Result<RestorePreview, String> {
    // An ordinary backup name, or a `-pre-update` one (the name this lane gives them; nothing writes
    // those yet, the updater is unbuilt).
    let Some(naive) = parse_backup_name(file).or_else(|| amara_archive(file).map(|(t, _)| t))
    else {
        return Err("invalid_file".into());
    };
    if backup_busy.load(std::sync::atomic::Ordering::Acquire) {
        return Err("backup_running".into());
    }
    let _guard = BusyGuard::acquire(verify_busy).ok_or_else(|| "busy".to_string())?;
    let dir = resolve_dir(dir_setting, home, data_dir)?;
    let path = dir.join(file);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "not_found".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("symlink_refused".into());
    }
    if !meta.is_file() {
        return Err("not_a_file".into());
    }
    let at = Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.to_rfc3339())
        .unwrap_or_default();
    let verify = verify_archive(&path, file, at, VERIFY_CAPS)?;
    let (files, bytes, groups, left_out) = list_archive(&path)?;
    let reenter = super::keychain::SECRETS
        .iter()
        .map(|(name, label)| Reenter {
            name: (*name).to_string(),
            label: (*label).to_string(),
        })
        .collect();
    Ok(RestorePreview {
        file: clean_name(file),
        verify,
        files,
        bytes,
        groups,
        left_out,
        reenter,
        key_file_note:
            "amara-n8n.key is not in a backup; unlock it from its own file and passphrase.".into(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn restore_preview(file: String) -> Result<RestorePreview, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || {
        let s = load_settings(&data)?;
        restore_preview_in(&home, &data, &s.dir, &file, &BUSY, &VERIFY_BUSY)
    })
    .await
    .map_err(|_| "io_error".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    // ---- next 03:00 JST ----

    #[test]
    fn next_slot_is_03_00_jst_whatever_the_machine_zone() {
        // 02:59 JST Nov 13 = 17:59Z Nov 12 -> 03:00 JST Nov 13 = 18:00Z Nov 12.
        assert_eq!(
            next_run_after(utc("2026-11-12T17:59:00Z"), "03:00"),
            Some(utc("2026-11-12T18:00:00Z"))
        );
        // Exactly on the slot: the NEXT one is a day later.
        assert_eq!(
            next_run_after(utc("2026-11-12T18:00:00Z"), "03:00"),
            Some(utc("2026-11-13T18:00:00Z"))
        );
        // 12:00 JST (03:00Z): tomorrow's slot, across a month end too.
        assert_eq!(
            next_run_after(utc("2026-11-30T03:00:00Z"), "03:00"),
            Some(utc("2026-11-30T18:00:00Z"))
        );
        // A custom time is still JST: 23:30 JST = 14:30Z.
        assert_eq!(
            next_run_after(utc("2026-11-12T00:00:00Z"), "23:30"),
            Some(utc("2026-11-12T14:30:00Z"))
        );
        assert_eq!(next_run_after(utc("2026-11-12T00:00:00Z"), "24:00"), None);
        assert_eq!(prev_slot(utc("2026-11-12T00:00:00Z"), "3:00"), None);
    }

    // ---- catch-up ----

    fn settings() -> BackupSettings {
        BackupSettings::default()
    }
    const WAITED: std::time::Duration = std::time::Duration::from_secs(10 * 60);

    fn inputs<'a>(s: &'a BackupSettings, now: &str, last: Option<&str>) -> Inputs<'a> {
        Inputs {
            now: utc(now),
            since_launch: WAITED,
            settings: s,
            last_done: last.map(utc),
            busy: false,
            battery: None,
        }
    }

    #[test]
    fn a_missed_nightly_runs_once_and_a_done_one_waits() {
        let s = settings();
        // Slot 03:00 JST Nov 13 = 18:00Z Nov 12. Last good backup the night before: missed.
        let i = inputs(&s, "2026-11-13T02:00:00Z", Some("2026-11-11T18:01:00Z"));
        assert_eq!(decide(&i), Decision::Run);
        // Ran at 03:01 JST: nothing is due until tomorrow's slot.
        let i = inputs(&s, "2026-11-13T02:00:00Z", Some("2026-11-12T18:01:00Z"));
        assert_eq!(decide(&i), Decision::Wait);
        // Before today's slot passes, last night's backup is current.
        let i = inputs(&s, "2026-11-12T17:00:00Z", Some("2026-11-11T18:01:00Z"));
        assert_eq!(decide(&i), Decision::Wait);
        // No backup at all: one is made.
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T02:00:00Z", None)),
            Decision::Run
        );
    }

    #[test]
    fn after_an_error_it_backs_off_an_hour_and_stops_after_three_attempts() {
        let slot = prev_slot(utc("2026-11-13T02:00:00Z"), "03:00").unwrap();
        let failed = |n: u32, last: &str| BackupSettings {
            failed_attempts: n,
            failed_slot: Some(slot.to_rfc3339()),
            last_run_at: Some(last.into()),
            last_run_result: Some("io_error".into()),
            ..settings()
        };
        // Failed 20 minutes ago: wait. An hour and a minute ago: run again.
        let s = failed(1, "2026-11-13T01:40:00Z");
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T02:00:00Z", None)),
            Decision::Wait
        );
        let s = failed(1, "2026-11-13T00:59:00Z");
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T02:00:00Z", None)),
            Decision::Run
        );
        let s = failed(2, "2026-11-13T00:59:00Z");
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T02:00:00Z", None)),
            Decision::Run
        );
        // Three failures: the slot is given up, however long ago.
        let s = failed(3, "2026-11-12T20:00:00Z");
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T02:00:00Z", None)),
            Decision::Wait
        );
        // A failure recorded for an OLDER slot does not hold back this one.
        let old = BackupSettings {
            failed_slot: Some("2026-11-11T18:00:00+00:00".into()),
            ..failed(3, "2026-11-13T01:59:00Z")
        };
        assert_eq!(
            decide(&inputs(&old, "2026-11-13T02:00:00Z", None)),
            Decision::Run
        );
        // The next slot (03:00 JST the next day) is a fresh start.
        let s = failed(3, "2026-11-13T01:00:00Z");
        assert_eq!(
            decide(&inputs(&s, "2026-11-13T18:30:00Z", None)),
            Decision::Run
        );
    }

    #[test]
    fn the_retry_counter_counts_per_slot_and_clears_on_success() {
        let d = tmp("attempts");
        crate::commands::backups::record_attempt(&d, "slot-a", true).unwrap();
        crate::commands::backups::record_attempt(&d, "slot-a", true).unwrap();
        let s = load_settings(&d).unwrap();
        assert_eq!(
            (s.failed_attempts, s.failed_slot.as_deref()),
            (2, Some("slot-a"))
        );
        crate::commands::backups::record_attempt(&d, "slot-b", true).unwrap();
        assert_eq!(load_settings(&d).unwrap().failed_attempts, 1);
        crate::commands::backups::record_attempt(&d, "slot-b", false).unwrap();
        let s = load_settings(&d).unwrap();
        assert_eq!((s.failed_attempts, s.failed_slot), (0, None));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_runs_before_the_startup_delay_or_when_nightly_is_off() {
        let s = settings();
        let mut i = inputs(&s, "2026-11-13T02:00:00Z", None);
        i.since_launch = std::time::Duration::from_secs(30);
        assert_eq!(decide(&i), Decision::Wait);
        let off = BackupSettings {
            nightly: false,
            ..settings()
        };
        assert_eq!(
            decide(&inputs(&off, "2026-11-13T02:00:00Z", None)),
            Decision::Wait
        );
    }

    // ---- busy and battery ----

    #[test]
    fn busy_chat_crew_or_muse_defers_the_run() {
        let s = settings();
        let mut i = inputs(&s, "2026-11-13T02:00:00Z", None);
        i.busy = true;
        assert_eq!(decide(&i), Decision::Skip("skipped_busy"));
        i.busy = false;
        assert_eq!(decide(&i), Decision::Run);
    }

    #[test]
    fn low_battery_skips_but_ac_or_unreadable_does_not() {
        let s = settings(); // skip below 20
        let mut i = inputs(&s, "2026-11-13T02:00:00Z", None);
        i.battery = Some(Battery {
            on_battery: true,
            percent: 19,
        });
        assert_eq!(decide(&i), Decision::Skip("skipped_battery"));
        i.battery = Some(Battery {
            on_battery: true,
            percent: 20,
        });
        assert_eq!(decide(&i), Decision::Run, "20% is not below 20%");
        i.battery = Some(Battery {
            on_battery: false,
            percent: 5,
        });
        assert_eq!(decide(&i), Decision::Run, "on AC at 5% still runs");
        i.battery = None; // pmset unreadable: assume AC
        assert_eq!(decide(&i), Decision::Run);
        // Busy is checked before battery.
        i.busy = true;
        assert_eq!(decide(&i), Decision::Skip("skipped_busy"));
    }

    #[test]
    fn pmset_output_parses() {
        let on_batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t18%; discharging; 1:02 remaining present: true\n";
        assert_eq!(
            parse_pmset(on_batt),
            Some(Battery {
                on_battery: true,
                percent: 18
            })
        );
        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(
            parse_pmset(ac),
            Some(Battery {
                on_battery: false,
                percent: 100
            })
        );
        // A desktop Mac: AC and no battery line.
        assert_eq!(
            parse_pmset("Now drawing from 'AC Power'\n"),
            Some(Battery {
                on_battery: false,
                percent: 100
            })
        );
        assert_eq!(parse_pmset(""), None);
        assert_eq!(parse_pmset("pmset: command not understood"), None);
    }

    // ---- prune plan ----

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-prune-test-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn listing(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    fs::read(e.path()).unwrap_or_default(),
                )
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn prune_plan_lists_only_amara_archives_and_deletes_nothing() {
        let d = tmp("plan");
        let mut ordinary = Vec::new();
        for day in 1..=20 {
            let n = format!("amara-backup-202611{day:02}-030000.tar.gz");
            touch(&d, &n);
            ordinary.push(n);
        }
        touch(&d, "amara-backup-20261001-030000-pre-update.tar.gz");
        // Look-alikes and other people's files: never matched, never listed.
        for n in [
            "nexus-backup-20260101_030000.tar.gz",
            "amara-backup-20261001-030000.tar.gz.bak",
            "amara-backup-2026-030000.tar.gz",
            "amara-backup-20261301-030000.tar.gz",
            "amara-backup-20261001-030000-extra.tar.gz",
            "my-amara-backup-20261001-030000.tar.gz",
            "notes.txt",
            "amara-n8n.key",
            "amara-backup-20261001-030000.tar.gz.sha256",
        ] {
            touch(&d, n);
        }
        fs::create_dir(d.join("amara-backup-20260901-030000.tar.gz")).unwrap();
        std::os::unix::fs::symlink(
            d.join("notes.txt"),
            d.join("amara-backup-20260902-030000.tar.gz"),
        )
        .unwrap();
        let before = listing(&d);

        let plan = prune_plan(&d, 14).unwrap();
        assert_eq!(
            plan.would_delete,
            ordinary[..6].to_vec(),
            "oldest six, in order"
        );
        assert_eq!(plan.keep, ordinary[6..].to_vec());
        assert_eq!(
            plan.pre_update_kept,
            vec!["amara-backup-20261001-030000-pre-update.tar.gz"]
        );
        for n in plan.would_delete.iter().chain(&plan.keep) {
            assert!(amara_archive(n).is_some(), "{n}");
        }
        assert_eq!(listing(&d), before, "planning changed nothing on disk");
        assert_eq!(prune_plan(&d, 0).unwrap_err(), "invalid_keep");
        assert_eq!(
            prune_plan(&d.join("missing"), 14).unwrap(),
            PrunePlan::default()
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn pre_update_archives_never_count_or_go() {
        let names = [
            "amara-backup-20261101-030000-pre-update.tar.gz",
            "amara-backup-20261102-030000.tar.gz",
            "amara-backup-20261103-030000-pre-update.tar.gz",
            "amara-backup-20261104-030000.tar.gz",
        ];
        let p = prune_plan_names(names, 1);
        assert_eq!(p.would_delete, vec!["amara-backup-20261102-030000.tar.gz"]);
        assert_eq!(p.keep, vec!["amara-backup-20261104-030000.tar.gz"]);
        assert_eq!(p.pre_update_kept.len(), 2);
        assert!(prune_plan_names(names, 5).would_delete.is_empty());
    }

    fn git_in(repo: &Path, args: &[&str]) {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false"
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }

    fn temp_repo() -> std::path::PathBuf {
        let repo = tmp("bundle-repo");
        git_in(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("a.txt"), "hi").unwrap();
        git_in(&repo, &["add", "a.txt"]);
        git_in(&repo, &["commit", "-q", "-m", "x"]);
        git_in(&repo, &["tag", "v1"]);
        repo
    }

    #[test]
    fn git_bundle_command_is_fixed_and_clean() {
        let c = bundle_command(Path::new("/r"));
        assert_eq!(c.get_program(), "/usr/bin/git");
        let args: Vec<_> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            ["-C", "/r", "bundle", "create", "-", "--branches", "--tags"]
        );
        assert!(!args.iter().any(|a| a == "--all"));
        let envs: BTreeMap<_, _> = c
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let want = [
            ("PATH", "/usr/bin:/bin"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("LC_ALL", "C"),
        ];
        assert_eq!(envs.len(), want.len(), "{envs:?}");
        for (k, v) in want {
            assert_eq!(envs[k].as_deref(), Some(v), "{k}");
        }
    }

    #[test]
    fn git_bundle_is_off_by_default_named_apart_and_really_bundles() {
        assert!(!BackupSettings::default().git_bundle);
        let n = bundle_name(Local::now());
        assert!(amara_archive(&n).is_none() && parse_backup_name(&n).is_none());
        let repo = temp_repo();
        let dir = tmp("bundle-out");
        let name = create_git_bundle(&repo, &dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let m = fs::metadata(dir.join(&name)).unwrap();
        assert!(m.len() > 0 && m.permissions().mode() & 0o777 == 0o600);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no .part left");
        // a real bundle that verifies and carries the branch and the tag
        let v = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["bundle", "verify"])
            .arg(dir.join(&name))
            .output()
            .unwrap();
        assert!(v.status.success());
        let heads = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["bundle", "list-heads"])
            .arg(dir.join(&name))
            .output()
            .unwrap();
        let heads = String::from_utf8_lossy(&heads.stdout).into_owned();
        assert!(
            heads.contains("refs/heads/main") && heads.contains("refs/tags/v1"),
            "{heads}"
        );
        let _ = (fs::remove_dir_all(&repo), fs::remove_dir_all(&dir));
    }

    #[test]
    fn git_bundle_failure_removes_only_its_own_part_file() {
        let dir = tmp("bundle-fail");
        let keep = dir.join("amara-backup-20261112-030000.tar.gz");
        fs::write(&keep, b"precious").unwrap();
        fs::write(dir.join("notes.part"), b"mine").unwrap();
        // not a repo
        assert_eq!(
            create_git_bundle(&dir.join("nope"), &dir).unwrap_err(),
            "git_failed"
        );
        // timeout kills git and cleans up
        let repo = temp_repo();
        let r = create_git_bundle_with(&repo, &dir, std::time::Duration::ZERO);
        assert_eq!(r.unwrap_err(), "timeout");
        let mut left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with("amara-repo-") || n.ends_with(".part"))
            .collect();
        left.sort();
        assert_eq!(left, ["amara-backup-20261112-030000.tar.gz", "notes.part"]);
        assert_eq!(fs::read(&keep).unwrap(), b"precious");
        // O_EXCL: an existing .part of the same name is refused, not overwritten
        let name = bundle_name(Local::now());
        fs::write(dir.join(format!("{name}.part")), b"theirs").unwrap();
        assert_eq!(create_git_bundle(&repo, &dir).unwrap_err(), "io_error");
        assert_eq!(
            fs::read(dir.join(format!("{name}.part"))).unwrap(),
            b"theirs"
        );
        // a symlink at the .part name is refused (O_NOFOLLOW + O_EXCL)
        let _ = fs::remove_file(dir.join(format!("{name}.part")));
        std::os::unix::fs::symlink(&keep, dir.join(format!("{name}.part"))).unwrap();
        assert_eq!(create_git_bundle(&repo, &dir).unwrap_err(), "io_error");
        assert_eq!(fs::read(&keep).unwrap(), b"precious");
        let _ = (fs::remove_dir_all(&repo), fs::remove_dir_all(&dir));
    }

    #[test]
    fn nothing_in_this_module_can_delete_a_backup_and_auto_prune_defaults_off() {
        assert!(!BackupSettings::default().auto_prune);
        let src = include_str!("backup_schedule.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        // The only removal is the git bundle's own `.part` file.
        let (a, b) = (
            code.find("pub(crate) fn create_git_bundle_with").unwrap(),
            code.find("pub(crate) fn create_git_bundle(").unwrap(),
        );
        let outside = format!("{}{}", &code[..a], &code[b..]);
        for bad in ["remove_file", "remove_dir", "fs::rename", "unlink"] {
            assert!(!outside.contains(bad), "{bad} outside the bundle writer");
        }
        assert_eq!(code[a..b].matches("remove_file").count(), 1);
        assert!(code[a..b].contains("remove_file(&part)"));
    }

    // ---- restore preview ----

    fn make_archive(path: &Path, files: &[(&str, &[u8])]) {
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (n, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o600);
            h.set_cksum();
            b.append_data(&mut h, format!("nexus-pack-1/{n}"), *data)
                .unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }

    fn nb() -> std::sync::atomic::AtomicBool {
        std::sync::atomic::AtomicBool::new(false)
    }

    fn sha(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(b))
    }

    #[test]
    fn restore_preview_is_read_only_and_lists_what_would_come_back() {
        let home = tmp("preview");
        let data = home.join("Library-data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("goals.json"), "[1]").unwrap();
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let (a, b) = (b"{\"a\":1}".as_slice(), b"EEG".as_slice());
        let manifest = serde_json::json!({
            "version": "0.1.0",
            "sha256": {"nexus-config/replays/r1.bin": sha(b), "goals.json": sha(a)},
            "skipped": ["crew_service_token (left out on purpose)"],
        })
        .to_string();
        let name = "amara-backup-20261112-030000.tar.gz";
        make_archive(
            &dir.join(name),
            &[
                ("nexus-config/replays/r1.bin", b),
                ("goals.json", a),
                ("manifest.json", manifest.as_bytes()),
            ],
        );
        let before = listing(&dir);
        let meta_before: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().modified().unwrap())
            .collect();
        let data_before = listing(&data);

        let p = restore_preview_in(&home, &data, "~/Amara Backups", name, &nb(), &nb()).unwrap();
        assert!(p.verify.ok, "{:?}", p.verify);
        assert_eq!(p.verify.mode, "sha256");
        assert_eq!(p.files, 3);
        assert_eq!(p.bytes, (a.len() + b.len() + manifest.len()) as u64);
        let g: Vec<_> = p
            .groups
            .iter()
            .map(|g| (g.name.as_str(), g.files))
            .collect();
        assert_eq!(
            g,
            vec![
                ("goals.json", 1),
                ("manifest.json", 1),
                ("nexus-config/replays", 1)
            ]
        );
        assert_eq!(p.left_out, vec!["crew_service_token (left out on purpose)"]);
        assert!(p.reenter.iter().any(|r| r.name == "telegram_bot_token"));
        assert!(p.reenter.iter().any(|r| r.name == "n8n_encryption_key"));
        assert!(!p.key_file_note.is_empty());

        assert_eq!(
            listing(&dir),
            before,
            "archive dir unchanged (names and bytes)"
        );
        let meta_after: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().modified().unwrap())
            .collect();
        assert_eq!(meta_after, meta_before, "mtimes unchanged");
        assert_eq!(listing(&data), data_before, "app data unchanged");
        assert_eq!(
            fs::read_dir(&home).unwrap().count(),
            2,
            "no extraction anywhere"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn restore_preview_accepts_a_pre_update_archive() {
        let home = tmp("preview-pre");
        let data = home.join("Library-data");
        fs::create_dir_all(&data).unwrap();
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        let name = "amara-backup-20261112-030000-pre-update.tar.gz";
        let manifest = br#"{"version":"0.1.0","skipped":[]}"#;
        make_archive(
            &dir.join(name),
            &[("goals.json", b"[]"), ("manifest.json", manifest)],
        );
        let p = restore_preview_in(&home, &data, "~/Amara Backups", name, &nb(), &nb()).unwrap();
        assert!(p.verify.ok && p.files == 2 && p.file == name, "{p:?}");
        // near-misses are still refused
        for bad in [
            "amara-backup-20261112-030000-pre-update-x.tar.gz",
            "amara-backup-20261112-030000-pre-update.tar.gz.bak",
            "amara-backup-2026-030000-pre-update.tar.gz",
        ] {
            assert_eq!(
                restore_preview_in(&home, &data, "~/Amara Backups", bad, &nb(), &nb()).unwrap_err(),
                "invalid_file",
                "{bad}"
            );
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn restore_preview_refuses_paths_and_non_backup_names() {
        let home = tmp("preview-bad");
        let data = home.join("Library-data");
        fs::create_dir_all(&data).unwrap();
        let dir = home.join("Amara Backups");
        fs::create_dir_all(&dir).unwrap();
        fs::write(home.join("amara-backup-20261112-030000.tar.gz"), b"x").unwrap();
        for f in [
            "../amara-backup-20261112-030000.tar.gz",
            "/etc/passwd",
            "sub/amara-backup-20261112-030000.tar.gz",
            "notes.txt",
            "",
        ] {
            assert_eq!(
                restore_preview_in(&home, &data, "~/Amara Backups", f, &nb(), &nb()).unwrap_err(),
                "invalid_file",
                "{f}"
            );
        }
        assert_eq!(
            restore_preview_in(
                &home,
                &data,
                "~/Amara Backups",
                "amara-backup-20261112-030000.tar.gz",
                &nb(),
                &nb()
            )
            .unwrap_err(),
            "not_found"
        );
        std::os::unix::fs::symlink(
            home.join("amara-backup-20261112-030000.tar.gz"),
            dir.join("amara-backup-20261112-030000.tar.gz"),
        )
        .unwrap();
        assert_eq!(
            restore_preview_in(
                &home,
                &data,
                "~/Amara Backups",
                "amara-backup-20261112-030000.tar.gz",
                &nb(),
                &nb()
            )
            .unwrap_err(),
            "symlink_refused"
        );
        let _ = fs::remove_dir_all(&home);
    }

    /// Restore drill on a COPY. Ignored by default; run by hand with DRILL_DIR (a new, empty dir) and
    /// DRILL_SRC (the app data dir, only ever read). Writes nothing outside DRILL_DIR.
    #[test]
    #[ignore]
    fn restore_drill_on_a_copy() {
        use sha2::{Digest, Sha256};
        use std::time::Instant;
        let drill = std::path::PathBuf::from(std::env::var("DRILL_DIR").expect("DRILL_DIR"));
        let src = std::path::PathBuf::from(std::env::var("DRILL_SRC").expect("DRILL_SRC"));
        let home = dirs::home_dir().unwrap();
        assert!(drill.starts_with(&home) && !drill.starts_with(&src) && !src.starts_with(&drill));
        let free = super::super::backups::free_bytes(&home).unwrap();
        let mut out = serde_json::Map::new();
        out.insert("free_bytes_before".into(), free.into());
        if free < 15 * 1024 * 1024 * 1024 {
            out.insert("skipped".into(), "under 15 GiB free".into());
            fs::write(drill.join("result.json"), Value::Object(out).to_string()).unwrap();
            return;
        }
        let archives = drill.join("archives");
        let restore = drill.join("restore");
        fs::create_dir_all(&archives).unwrap();
        fs::create_dir_all(&restore).unwrap();
        let settings = BackupSettings {
            dir: archives.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let busy = std::sync::atomic::AtomicBool::new(false);
        let t0 = Instant::now();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let quiet = |_: &str, _: u8| {};
        let run = rt
            .block_on(super::super::backups::run_backup(
                &home,
                &src,
                &settings,
                &super::super::backups::free_bytes,
                &quiet,
                &busy,
            ))
            .unwrap();
        let t_pack = t0.elapsed();
        let archive = archives.join(&run.entry.id);
        let t1 = Instant::now();
        let entries = super::super::packager::unpack_archive_checked(
            &archive,
            &restore,
            8 * 1024 * 1024 * 1024,
            500_000,
        )
        .unwrap();
        let t_unpack = t1.elapsed();
        let pack = fs::read_dir(&restore)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("nexus-pack-"))
            .unwrap()
            .path();
        // Manifest vs restored tree.
        let manifest: Value =
            serde_json::from_slice(&fs::read(pack.join("manifest.json")).unwrap()).unwrap();
        let listed = manifest["sha256"].as_object().unwrap();
        let mut files = BTreeMap::new();
        let mut stack = vec![pack.clone()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let t = e.file_type().unwrap();
                if t.is_dir() {
                    stack.push(e.path());
                } else if t.is_file() {
                    let rel = e
                        .path()
                        .strip_prefix(&pack)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if rel != "manifest.json" {
                        files.insert(rel, e.path());
                    }
                }
            }
        }
        let (mut hash_ok, mut hash_bad, mut missing) = (0usize, Vec::new(), Vec::new());
        for (k, v) in listed {
            match files.get(k) {
                None => missing.push(k.clone()),
                Some(p) => {
                    let got = hex::encode(Sha256::digest(fs::read(p).unwrap()));
                    if Some(got.as_str()) == v.as_str() {
                        hash_ok += 1;
                    } else {
                        hash_bad.push(k.clone());
                    }
                }
            }
        }
        let extra: Vec<_> = files
            .keys()
            .filter(|k| !listed.contains_key(*k))
            .cloned()
            .collect();
        // Muse folders: restored vs original, read-only.
        let mut muse = serde_json::Map::new();
        let mut muse_bad = Vec::new();
        let mut muse_skipped: Vec<String> = Vec::new();
        for d in ["replays", "sessions", "muse"] {
            let (mut same, mut total) = (0usize, 0usize);
            let mut st = vec![src.join(d)];
            while let Some(x) = st.pop() {
                let Ok(rd) = fs::read_dir(&x) else { continue };
                for e in rd.flatten() {
                    let t = e.file_type().unwrap();
                    if t.is_dir() {
                        st.push(e.path());
                    } else if t.is_file() {
                        total += 1;
                        let rel = e.path().strip_prefix(&src).unwrap().to_path_buf();
                        let got = fs::read(pack.join("nexus-config").join(&rel));
                        let relname = rel.to_string_lossy().into_owned();
                        match got {
                            Ok(g) if g == fs::read(e.path()).unwrap() => same += 1,
                            // Left out on purpose and listed in the manifest: counted, not a failure.
                            Err(_)
                                if run
                                    .skipped
                                    .iter()
                                    .any(|s| s.contains(&relname) && s.contains("left out")) =>
                            {
                                muse_skipped.push(relname)
                            }
                            _ => muse_bad.push(relname),
                        }
                    }
                }
            }
            muse.insert(
                d.into(),
                serde_json::json!({"original_files": total, "byte_identical": same}),
            );
        }
        // No secret-named file or Keychain/n8n key file inside.
        let secretish: Vec<_> = files
            .keys()
            .filter(|k| {
                let last = k.rsplit('/').next().unwrap();
                super::super::backups::is_excluded_name(last, 1)
            })
            .cloned()
            .collect();
        let archive_size = fs::metadata(&archive).unwrap().len();
        out.insert("archive".into(), run.entry.id.clone().into());
        out.insert("archive_bytes".into(), archive_size.into());
        out.insert("archive_entries".into(), entries.into());
        out.insert("restored_files_excl_manifest".into(), files.len().into());
        out.insert("manifest_hashes".into(), listed.len().into());
        out.insert("hash_ok".into(), hash_ok.into());
        out.insert("hash_bad".into(), hash_bad.clone().into());
        out.insert("missing".into(), missing.clone().into());
        out.insert("extra".into(), extra.clone().into());
        out.insert("muse".into(), Value::Object(muse));
        out.insert("muse_not_identical".into(), muse_bad.clone().into());
        out.insert("muse_skipped_listed".into(), muse_skipped.clone().into());
        out.insert("secretish_names_inside".into(), secretish.clone().into());
        out.insert("skipped_at_backup_count".into(), run.skipped.len().into());
        out.insert(
            "skipped_at_backup".into(),
            run.skipped
                .iter()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .into(),
        );
        out.insert("pack_secs".into(), t_pack.as_secs_f64().into());
        out.insert("unpack_secs".into(), t_unpack.as_secs_f64().into());
        out.insert("total_secs".into(), t0.elapsed().as_secs_f64().into());
        fs::write(drill.join("result.json"), Value::Object(out).to_string()).unwrap();
        assert!(hash_bad.is_empty() && missing.is_empty() && extra.is_empty());
        assert!(secretish.is_empty());
        assert!(muse_bad.is_empty(), "{muse_bad:?}");
    }
}
