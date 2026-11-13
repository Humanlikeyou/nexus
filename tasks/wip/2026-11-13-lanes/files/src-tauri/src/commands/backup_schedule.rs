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
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

// ---- nightly scheduler ----

/// Japan Standard Time: UTC+9, no daylight saving.
const JST_SECS: i32 = 9 * 3600;
/// How long after launch before any run (so a catch-up never competes with startup).
pub(crate) const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

fn jst() -> FixedOffset {
    FixedOffset::east_opt(JST_SECS).expect("valid offset")
}

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
            // `already_running` (Back Up Now holds the flag) is simply tried again next tick.
            let _ = super::backups::run_and_record(app).await;
        }
    }
}

/// Start the nightly scheduler. Called once from the app's `setup`.
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
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

#[derive(Serialize, Debug, PartialEq, Default)]
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

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreGroup {
    pub name: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reenter {
    pub name: String,
    pub label: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
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
) -> Result<RestorePreview, String> {
    let Some(naive) = parse_backup_name(file) else {
        return Err("invalid_file".into());
    };
    if BUSY.load(std::sync::atomic::Ordering::Acquire) {
        return Err("backup_running".into());
    }
    let _guard = BusyGuard::acquire(&VERIFY_BUSY).ok_or_else(|| "busy".to_string())?;
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
pub async fn restore_preview(file: String) -> Result<RestorePreview, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    tokio::task::spawn_blocking(move || {
        let s = load_settings(&data)?;
        restore_preview_in(&home, &data, &s.dir, &file)
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

    #[test]
    fn nothing_in_this_module_can_delete_and_auto_prune_defaults_off() {
        assert!(!BackupSettings::default().auto_prune);
        let src = include_str!("backup_schedule.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for bad in ["remove_file", "remove_dir", "fs::rename", "unlink"] {
            assert!(!code.contains(bad), "{bad} in non-test code");
        }
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

        let p = restore_preview_in(&home, &data, "~/Amara Backups", name).unwrap();
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
                restore_preview_in(&home, &data, "~/Amara Backups", f).unwrap_err(),
                "invalid_file",
                "{f}"
            );
        }
        assert_eq!(
            restore_preview_in(
                &home,
                &data,
                "~/Amara Backups",
                "amara-backup-20261112-030000.tar.gz"
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
                "amara-backup-20261112-030000.tar.gz"
            )
            .unwrap_err(),
            "symlink_refused"
        );
        let _ = fs::remove_dir_all(&home);
    }
}
