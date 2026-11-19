//! Hemi-Sync in Neuro (docs/decisions/0010).
//!
//! * `hemisync_import` copies audio files from a folder inside home (default
//!   `~/Desktop/Hemisync Collection`) into `<data>/hemisync/<wave-folder>/<file>`. The source is
//!   only read. Fixed error codes, never a path in an error.
//! * `hemisync_library` scans `<data>/hemisync/`. Audio is served by the custom URI scheme
//!   `hemisync://localhost/<track id>` (see `serve`): id-only, read-only, ranged, no path ever
//!   reaches the renderer. No asset protocol, no command returns an absolute path.
//! * Session links live in their own store `<data>/hemisync/sessions.json`. Muse recordings are
//!   sacred (docs/decisions/0007): this module only READS the Muse session summaries for metrics
//!   and never writes under `sessions/`, `replays/` or `muse/` (a test scans this source).
//! * Amara gets a small local `<nexus_hemisync>` context section. Nothing leaves this Mac.

use super::shared::task::{blocking, JoinErr};
use super::{backups, local_store, path_safety, sessions};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::Emitter;

const IMPORT_EVENT: &str = "hemisync://import";
const DEFAULT_FOLDER: &str = "~/Desktop/Hemisync Collection";
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "wav", "flac", "aiff", "aac"];
const MAX_FILE_BYTES: u64 = 400 * 1024 * 1024;
/// Directories are read down to this depth below the source folder (the folder itself is 0).
const MAX_DEPTH: usize = 3;
/// An import needs this much free space on top of the bytes it copies.
const FREE_SPACE_MARGIN: u64 = 5 * 1024 * 1024 * 1024;
const MAX_SESSIONS: usize = 2000;
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_META_BYTES: u64 = 16 * 1024;
const MAX_NOTE_CHARS: usize = 500;
const MAX_LISTENED_MS: u64 = 24 * 60 * 60 * 1000;
const AMARA_SESSIONS: usize = 5;
const AMARA_NOTE_CHARS: usize = 120;
const AMARA_TITLE_CHARS: usize = 80;
const STORE_FILE: &str = "sessions.json";
const META_FILE: &str = "import.json";

static IMPORT_BUSY: AtomicBool = AtomicBool::new(false);
static STORE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------- types

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibTrack {
    pub id: String,
    pub wave: u32,
    pub wave_name: String,
    pub order: u32,
    pub title: String,
    pub duration_ms: Option<u64>,
    /// Never sent to the renderer (no path leaves the backend).
    #[serde(skip)]
    pub rel_path: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub tracks: Vec<LibTrack>,
    pub imported_at: Option<String>,
    pub source_folder: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ImportMeta {
    #[serde(default)]
    imported_at: Option<String>,
    #[serde(default)]
    source_folder: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
pub struct ImportResult {
    pub added: u64,
    pub skipped: u64,
}

#[derive(Serialize, Clone)]
struct ProgressEvent<'a> {
    copied: u64,
    total: u64,
    phase: &'a str,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct LinkRow {
    id: String,
    track_id: String,
    started_at: String,
    #[serde(default)]
    listened_ms: u64,
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    rating: Option<u8>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    sessions: Vec<LinkRow>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub focus_avg: Option<f64>,
    pub calm_avg: Option<f64>,
    /// The session summary holds no band powers, so this is always null (never computed here).
    pub bands: Option<serde_json::Value>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub track_id: String,
    pub started_at: String,
    pub listened_ms: u64,
    pub completed: bool,
    pub rating: Option<u8>,
    pub note: Option<String>,
    pub metrics: Metrics,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackStats {
    pub track_id: String,
    pub sessions: u64,
    pub last_at: Option<String>,
    pub avg_rating: Option<f64>,
    pub focus_avg: Option<f64>,
    pub calm_avg: Option<f64>,
}

// ---------------------------------------------------------------- name parsing

fn take_digits(s: &str) -> Option<(&str, &str)> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        None
    } else {
        Some(s.split_at(n))
    }
}

/// `CD1 - 2 - Title` -> (2, "Title").
fn strip_cd_prefix(s: &str) -> Option<(u32, &str)> {
    let b = s.as_bytes();
    if b.len() < 2 || !b[..2].eq_ignore_ascii_case(b"cd") {
        return None;
    }
    let (_, rest) = take_digits(&s[2..])?;
    let rest = rest.strip_prefix(" - ")?;
    let (track, rest) = take_digits(rest)?;
    let rest = rest.strip_prefix(" - ")?;
    Some((track.parse().ok()?, rest))
}

/// `GE073DL_GE7_` -> the rest.
fn strip_ge_prefix(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.len() < 2 || !b[..2].eq_ignore_ascii_case(b"ge") {
        return None;
    }
    let (_, rest) = take_digits(&s[2..])?;
    let rest = rest.strip_prefix("DL_GE")?;
    let (_, rest) = take_digits(rest)?;
    rest.strip_prefix('_')
}

/// "Title-123" -> "Title" (a trailing `-digits` catalogue number).
fn strip_numeric_suffix(s: &str) -> &str {
    if let Some((head, tail)) = s.rsplit_once('-') {
        if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head;
        }
    }
    s
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// (order, title) from a file name; (0, cleaned stem) when the shape is unknown.
pub(crate) fn parse_track_name(file_name: &str) -> (u32, String) {
    let stem = match file_name.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => file_name,
    };
    let (order, title) = if let Some((order, rest)) = strip_cd_prefix(stem) {
        (order, tidy(strip_numeric_suffix(rest)))
    } else if let Some(rest) = strip_ge_prefix(stem) {
        let order = rest
            .split('_')
            .find(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);
        (order, tidy(&rest.replace('_', " ")))
    } else {
        (0, tidy(&stem.replace('_', " ")))
    };
    if title.is_empty() {
        (order, tidy(&stem.replace('_', " ")))
    } else {
        (order, title)
    }
}

/// (wave, name) from a folder name such as "Wave 5 Exploring"; (0, readable name) otherwise.
pub(crate) fn parse_wave_folder(name: &str) -> (u32, String) {
    let tokens: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("wave") {
        if let Ok(n) = tokens[1].parse::<u32>() {
            return (n, tokens[2..].join(" "));
        }
    }
    (0, tokens.join(" "))
}

/// Folder name -> a safe directory name: ASCII letters and digits, anything else becomes "-".
pub(crate) fn wave_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let out: String = out.trim_matches('-').chars().take(64).collect();
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "unsorted".to_string()
    } else {
        out
    }
}

fn is_audio_name(name: &str) -> bool {
    if name.starts_with('.') || name.chars().any(char::is_control) {
        return false;
    }
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && AUDIO_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
    })
}

fn track_id(rel_path: &str) -> String {
    let d = Sha256::digest(rel_path.as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn valid_track_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// ---------------------------------------------------------------- library

pub(crate) fn hemisync_dir(data: &Path) -> PathBuf {
    data.join("hemisync")
}

fn is_real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

fn is_real_file(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

/// Scan `<data>/hemisync/`: one folder level, audio files only, symlinks ignored.
pub(crate) fn scan_library(data: &Path) -> Library {
    let root = hemisync_dir(data);
    if !is_real_dir(&root) {
        return Library::default(); // missing, or a symlink: refused
    }
    let meta: ImportMeta =
        local_store::read_json(&root.join(META_FILE), MAX_META_BYTES).unwrap_or_default();
    let mut tracks = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for dir in rd.flatten() {
            let Some(dir_name) = dir.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if dir_name.starts_with('.') || !is_real_dir(&dir.path()) {
                continue;
            }
            let (wave, wave_name) = parse_wave_folder(&dir_name);
            let Ok(files) = fs::read_dir(dir.path()) else {
                continue;
            };
            for f in files.flatten() {
                let Some(name) = f.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if !is_audio_name(&name) || !is_real_file(&f.path()) {
                    continue;
                }
                let rel_path = format!("{dir_name}/{name}");
                let (order, title) = parse_track_name(&name);
                tracks.push(LibTrack {
                    id: track_id(&rel_path),
                    wave,
                    wave_name: wave_name.clone(),
                    order,
                    title,
                    duration_ms: None,
                    rel_path,
                });
            }
        }
    }
    tracks.sort_by(|a, b| {
        (a.wave, a.order, &a.title, &a.rel_path).cmp(&(b.wave, b.order, &b.title, &b.rel_path))
    });
    Library {
        tracks,
        imported_at: meta.imported_at,
        source_folder: meta.source_folder,
    }
}

/// Canonical path of `rel` under the hemisync dir, only if it stays inside it.
fn resolve_rel(data: &Path, root_canon: &Path, rel: &str) -> Option<PathBuf> {
    let path = hemisync_dir(data).join(rel).canonicalize().ok()?;
    path.starts_with(root_canon).then_some(path)
}

/// id -> canonical path for every library track (one fresh scan).
fn library_paths(data: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let Ok(root) = hemisync_dir(data).canonicalize() else {
        return out;
    };
    for t in scan_library(data).tracks {
        if let Some(p) = resolve_rel(data, &root, &t.rel_path) {
            out.insert(t.id, p);
        }
    }
    out
}

/// Absolute path of a library track, looked up by id from a fresh scan (tests; the scheme
/// handler uses `library_paths` through the cache).
#[cfg(test)]
pub(crate) fn track_path_in(data: &Path, id: &str) -> Result<String, String> {
    if !valid_track_id(id) {
        return Err("invalid_track_id".into());
    }
    let lib = scan_library(data);
    let track = lib
        .tracks
        .iter()
        .find(|t| t.id == id)
        .ok_or("track_not_found")?;
    let root = hemisync_dir(data)
        .canonicalize()
        .map_err(|_| "track_not_found".to_string())?;
    resolve_rel(data, &root, &track.rel_path)
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "track_not_found".to_string())
}

// ---------------------------------------------------------------- import

struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Candidate {
    src: PathBuf,
    size: u64,
    slug: String,
    name: String,
}

/// Audio files under `root`, at most `MAX_DEPTH` folders down. Symlinks, hidden files and
/// non-audio files are ignored. The second value counts audio files left out for size.
fn collect_candidates(root: &Path) -> (Vec<Candidate>, u64) {
    let mut out = Vec::new();
    let mut oversize = 0u64;
    let mut stack: Vec<(PathBuf, usize, Option<String>)> = vec![(root.to_path_buf(), 0, None)];
    while let Some((dir, depth, slug)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            let ft = meta.file_type();
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if ft.is_dir() {
                if depth < MAX_DEPTH {
                    let child = slug.clone().or_else(|| Some(wave_slug(&name)));
                    stack.push((e.path(), depth + 1, child));
                }
            } else if ft.is_file() && is_audio_name(&name) && meta.len() > 0 {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() != 1 {
                        oversize += 1; // counted as skipped
                        continue;
                    }
                }
                if meta.len() > MAX_FILE_BYTES {
                    oversize += 1;
                    continue;
                }
                out.push(Candidate {
                    src: e.path(),
                    size: meta.len(),
                    slug: slug.clone().unwrap_or_else(|| "unsorted".to_string()),
                    name,
                });
            }
        }
    }
    out.sort_by(|a, b| a.src.cmp(&b.src));
    (out, oversize)
}

fn open_nofollow(p: &Path) -> std::io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let f = o.open(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // A hard link can point at a file outside the folder: only single-link files are read.
        if f.metadata()?.nlink() != 1 {
            return Err(std::io::Error::other("multiple links"));
        }
    }
    Ok(f)
}

/// Create our `.part` file: create_new + O_NOFOLLOW, 0600. A leftover `.part` (lstat-checked, not
/// followed) is removed once and the create retried.
fn open_tmp(tmp: &Path) -> Result<fs::File, String> {
    for attempt in 0..2 {
        let mut o = crate::app_paths::private_open_options();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.custom_flags(libc::O_NOFOLLOW);
        }
        match o.open(tmp) {
            Ok(f) => return Ok(f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                match fs::symlink_metadata(tmp) {
                    Ok(m) if !m.file_type().is_dir() => {
                        fs::remove_file(tmp).map_err(|_| "io_error".to_string())?
                    }
                    _ => return Err("io_error".into()),
                }
            }
            Err(_) => return Err("io_error".into()),
        }
    }
    Err("io_error".into())
}

fn sha256_file(p: &Path) -> Result<Vec<u8>, String> {
    let mut f = open_nofollow(p).map_err(|_| "io_error".to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|_| "io_error".to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_vec())
}

/// Insert " (n)" before the extension.
fn numbered_name(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem} ({n}).{ext}"),
        None => format!("{name} ({n})"),
    }
}

enum Placed {
    Copied,
    Duplicate,
}

/// Copy `c` into `dir` (temp file + rename). A same-name file with the same size and SHA-256 is a
/// duplicate; a same-name file with other content gets a numbered name.
fn place(c: &Candidate, dir: &Path) -> Result<Placed, String> {
    let mut src_hash: Option<Vec<u8>> = None;
    let mut target = None;
    for n in 1..=99u32 {
        let name = if n == 1 {
            c.name.clone()
        } else {
            numbered_name(&c.name, n)
        };
        let dest = dir.join(&name);
        match fs::symlink_metadata(&dest) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                target = Some((name, dest));
                break;
            }
            Err(_) => return Err("io_error".into()),
            Ok(m) if m.file_type().is_file() && m.len() == c.size => {
                if src_hash.is_none() {
                    src_hash = Some(sha256_file(&c.src)?);
                }
                if Some(sha256_file(&dest)?) == src_hash {
                    return Ok(Placed::Duplicate);
                }
            }
            Ok(_) => {}
        }
    }
    let (name, dest) = target.ok_or_else(|| "io_error".to_string())?;
    let tmp = dir.join(format!(".{name}.part"));
    let result = (|| -> Result<(), String> {
        let mut src = open_nofollow(&c.src).map_err(|_| "io_error".to_string())?;
        let meta = src.metadata().map_err(|_| "io_error".to_string())?;
        if !meta.is_file() || meta.len() != c.size {
            return Err("io_error".into()); // changed while importing
        }
        let mut out = open_tmp(&tmp)?;
        let mut buf = vec![0u8; 1024 * 1024];
        let mut copied = 0u64;
        loop {
            let n = src.read(&mut buf).map_err(|_| "io_error".to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|_| "io_error".to_string())?;
            copied += n as u64;
        }
        if copied != c.size {
            return Err("io_error".into());
        }
        out.sync_all().map_err(|_| "io_error".to_string())?;
        drop(out);
        fs::rename(&tmp, &dest).map_err(|_| "io_error".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp); // our own temp file inside <data>/hemisync/
    }
    result.map(|_| Placed::Copied)
}

fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.to_string_lossy()),
        Err(_) => "~".to_string(),
    }
}

pub(crate) type ProgressFn<'a> = &'a dyn Fn(u64, u64, &str);

/// One import. Everything injectable so tests use temp dirs and a fake free-space reading.
/// `copied` in progress counts files handled so far (copied or skipped as duplicates).
pub(crate) fn import_in(
    home: &Path,
    data: &Path,
    folder: Option<&str>,
    free_space: backups::FreeSpaceFn<'_>,
    progress: ProgressFn<'_>,
    busy: &AtomicBool,
) -> Result<ImportResult, String> {
    let _guard = BusyGuard::acquire(busy).ok_or_else(|| "import_busy".to_string())?;
    let src_root = backups::resolve_dir(folder.unwrap_or(DEFAULT_FOLDER), home, data)?;
    if !is_real_dir(&src_root) {
        return Err("folder_missing".into());
    }
    progress(0, 0, "scanning");
    let (candidates, oversize) = collect_candidates(&src_root);
    if candidates.is_empty() && oversize == 0 {
        return Err("no_audio_files".into());
    }
    let total = candidates.len() as u64;
    let root = hemisync_dir(data);
    if fs::symlink_metadata(&root).is_ok_and(|m| !m.file_type().is_dir()) {
        return Err("io_error".into());
    }
    let need: u64 = candidates
        .iter()
        .filter(|c| {
            fs::symlink_metadata(root.join(&c.slug).join(&c.name))
                .map_or(true, |m| !(m.file_type().is_file() && m.len() == c.size))
        })
        .map(|c| c.size)
        .sum();
    if need > 0 && free_space(data)? < need.saturating_add(FREE_SPACE_MARGIN) {
        return Err("not_enough_space".into());
    }
    crate::app_paths::create_private_dir_all(&root).map_err(|_| "io_error".to_string())?;
    let (mut added, mut skipped, mut done) = (0u64, oversize, 0u64);
    progress(0, total, "copying");
    for c in &candidates {
        let dir = root.join(&c.slug);
        if fs::symlink_metadata(&dir).is_ok_and(|m| !m.file_type().is_dir()) {
            return Err("io_error".into());
        }
        crate::app_paths::create_private_dir_all(&dir).map_err(|_| "io_error".to_string())?;
        match place(c, &dir)? {
            Placed::Copied => added += 1,
            Placed::Duplicate => skipped += 1,
        }
        done += 1;
        progress(done, total, "copying");
    }
    let meta = ImportMeta {
        imported_at: Some(local_store::iso(&local_store::now_jst())),
        source_folder: Some(display_path(
            &src_root,
            &home.canonicalize().unwrap_or(home.into()),
        )),
    };
    local_store::write_json_atomic(&root.join(META_FILE), &meta, MAX_META_BYTES)
        .map_err(|_| "io_error".to_string())?;
    PATH_CACHE.clear();
    progress(total, total, "done");
    Ok(ImportResult { added, skipped })
}

// ---------------------------------------------------------------- session store

fn store_path(data: &Path) -> PathBuf {
    hemisync_dir(data).join(STORE_FILE)
}

/// The hemisync dir may be absent, but never a symlink or a file.
fn check_root(data: &Path) -> Result<(), String> {
    match fs::symlink_metadata(hemisync_dir(data)) {
        Ok(m) if m.file_type().is_dir() => Ok(()),
        Ok(_) => Err("store_error".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("store_error".into()),
    }
}

fn load_store(data: &Path) -> Result<Store, String> {
    check_root(data)?;
    local_store::read_json(&store_path(data), MAX_STORE_BYTES).map_err(|_| "store_error".into())
}

/// The Hemi-Sync link rows (id, startedAt, listenedMs, rating) straight from the store file, with
/// the module's capped, no-follow read. Never touches Muse sessions. Read-only.
pub(crate) fn link_rows_in(data: &Path) -> Result<Vec<LinkSummary>, String> {
    Ok(load_store(data)?
        .sessions
        .into_iter()
        .map(|s| LinkSummary {
            id: s.id,
            started_at: s.started_at,
            listened_ms: s.listened_ms,
            rating: s.rating,
        })
        .collect())
}

/// The few link-row fields other readers need.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkSummary {
    pub id: String,
    pub started_at: String,
    pub listened_ms: u64,
    pub rating: Option<u8>,
}

fn save_store(data: &Path, store: &Store) -> Result<(), String> {
    check_root(data)?;
    local_store::write_json_atomic(&store_path(data), store, MAX_STORE_BYTES)
        .map_err(|_| "store_error".to_string())
}

fn check_session_id(id: &str) -> Result<(), String> {
    path_safety::validate_name(id)
        .map(|_| ())
        .map_err(|_| "invalid_session_id".to_string())
}

pub(crate) fn link_in(
    data: &Path,
    session_id: &str,
    track: &str,
    started_at: &str,
) -> Result<(), String> {
    check_session_id(session_id)?;
    if !valid_track_id(track) || !scan_library(data).tracks.iter().any(|t| t.id == track) {
        return Err("track_not_found".into());
    }
    if started_at.len() > 40 || DateTime::parse_from_rfc3339(started_at).is_err() {
        return Err("invalid_started_at".into());
    }
    let _l = STORE_LOCK.lock().map_err(|_| "store_error".to_string())?;
    let mut store = load_store(data)?;
    if let Some(existing) = store.sessions.iter().find(|s| s.id == session_id) {
        return if existing.track_id == track {
            Ok(())
        } else {
            Err("session_exists".into())
        };
    }
    if store.sessions.len() >= MAX_SESSIONS {
        return Err("store_full".into());
    }
    store.sessions.push(LinkRow {
        id: session_id.to_string(),
        track_id: track.to_string(),
        started_at: started_at.to_string(),
        listened_ms: 0,
        completed: false,
        rating: None,
        note: None,
    });
    save_store(data, &store)
}

fn update_row(data: &Path, session_id: &str, f: impl FnOnce(&mut LinkRow)) -> Result<(), String> {
    check_session_id(session_id)?;
    let _l = STORE_LOCK.lock().map_err(|_| "store_error".to_string())?;
    let mut store = load_store(data)?;
    let row = store
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .ok_or("session_not_found")?;
    f(row);
    save_store(data, &store)
}

pub(crate) fn finish_in(
    data: &Path,
    session_id: &str,
    listened_ms: u64,
    completed: bool,
) -> Result<(), String> {
    update_row(data, session_id, |r| {
        r.listened_ms = r.listened_ms.max(listened_ms.min(MAX_LISTENED_MS));
        r.completed |= completed;
    })
}

pub(crate) fn rate_in(
    data: &Path,
    session_id: &str,
    rating: u8,
    note: Option<&str>,
) -> Result<(), String> {
    if !(1..=5).contains(&rating) {
        return Err("invalid_rating".into());
    }
    let note = match note {
        None => None,
        Some(n) => {
            if n.chars().count() > MAX_NOTE_CHARS * 2 {
                return Err("invalid_note".into());
            }
            let clean = local_store::clean_text(n, 0, MAX_NOTE_CHARS)
                .map_err(|_| "invalid_note".to_string())?;
            if clean.is_empty() {
                None
            } else {
                Some(clean)
            }
        }
    };
    update_row(data, session_id, |r| {
        r.rating = Some(rating);
        r.note = note;
    })
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}

/// Focus / calm averages from the linked Muse sessions, READ-ONLY. `calm` is the summary's
/// `avg_relaxation` (what the Neuro screen shows as calm). A missing session gives nulls.
fn muse_metrics(
    muse_sessions_dir: &Path,
    wanted: &[&str],
) -> HashMap<String, (Option<f64>, Option<f64>)> {
    let mut out = HashMap::new();
    if wanted.is_empty() {
        return out;
    }
    for s in sessions::read_stored_sessions_in(muse_sessions_dir) {
        if wanted.contains(&s.session.id.as_str()) {
            out.insert(
                s.session.id.clone(),
                (
                    finite(s.session.summary.avg_focus),
                    finite(s.session.summary.avg_relaxation),
                ),
            );
        }
    }
    out
}

pub(crate) fn sessions_in(
    data: &Path,
    muse_sessions_dir: &Path,
) -> Result<Vec<SessionRow>, String> {
    let store = load_store(data)?;
    let ids: Vec<&str> = store.sessions.iter().map(|s| s.id.as_str()).collect();
    let metrics = muse_metrics(muse_sessions_dir, &ids);
    let mut rows: Vec<SessionRow> = store
        .sessions
        .iter()
        .map(|s| {
            let (focus_avg, calm_avg) = metrics.get(&s.id).copied().unwrap_or((None, None));
            SessionRow {
                id: s.id.clone(),
                track_id: s.track_id.clone(),
                started_at: s.started_at.clone(),
                listened_ms: s.listened_ms,
                completed: s.completed,
                rating: s.rating,
                note: s.note.clone(),
                metrics: Metrics {
                    focus_avg,
                    calm_avg,
                    bands: None,
                },
            }
        })
        .collect();
    rows.sort_by_key(|r| {
        std::cmp::Reverse(
            DateTime::parse_from_rfc3339(&r.started_at)
                .map(|d| d.timestamp_millis())
                .unwrap_or(0),
        )
    });
    Ok(rows)
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = xs.collect();
    if v.is_empty() {
        None
    } else {
        Some(v.iter().sum::<f64>() / v.len() as f64)
    }
}

pub(crate) fn stats_from(rows: &[SessionRow]) -> Vec<TrackStats> {
    let mut by: HashMap<&str, Vec<&SessionRow>> = HashMap::new();
    for r in rows {
        by.entry(r.track_id.as_str()).or_default().push(r);
    }
    let mut out: Vec<TrackStats> = by
        .into_iter()
        .map(|(track_id, rs)| {
            let last = rs
                .iter()
                .max_by_key(|r| {
                    DateTime::parse_from_rfc3339(&r.started_at)
                        .map(|d| d.timestamp_millis())
                        .unwrap_or(0)
                })
                .map(|r| r.started_at.clone());
            TrackStats {
                track_id: track_id.to_string(),
                sessions: rs.len() as u64,
                last_at: last,
                avg_rating: mean(rs.iter().filter_map(|r| r.rating).map(f64::from)),
                focus_avg: mean(rs.iter().filter_map(|r| r.metrics.focus_avg)),
                calm_avg: mean(rs.iter().filter_map(|r| r.metrics.calm_avg)),
            }
        })
        .collect();
    out.sort_by(|a, b| a.track_id.cmp(&b.track_id));
    out
}

// ---------------------------------------------------------------- ranking (read-only)

/// A track needs at least this many sessions WITH a value for a measure before it is ranked on
/// that measure (same floor as patterns.rs `MIN_PER_SIDE`). Below it: "not enough sessions".
pub(crate) const MIN_SESSIONS_PER_TRACK: usize = 3;
/// Muse session files larger than this are skipped (the existing screen path reads every file in
/// the sessions folder with no cap; the ranking opens only linked sessions and caps each file).
const MAX_RANK_SESSION_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RankedTrack {
    /// 1-based; equal means share a rank.
    pub rank: usize,
    pub track_id: String,
    /// Sessions that had a value for this measure.
    pub n: usize,
    pub mean: f64,
    /// Hemi-Sync session ids behind `mean` (each is also a Muse session id).
    pub session_ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnrankedTrack {
    pub track_id: String,
    pub n: usize,
    pub session_ids: Vec<String>,
    /// Always "not enough sessions".
    pub reason: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeasureRanking {
    /// Best first.
    pub ranked: Vec<RankedTrack>,
    pub not_enough_sessions: Vec<UnrankedTrack>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackRanking {
    pub min_sessions: usize,
    /// Which Muse summary field "focus" and "calm" use (decision 0020).
    pub focus_field: String,
    pub calm_field: String,
    pub focus: MeasureRanking,
    pub calm: MeasureRanking,
    pub rating: MeasureRanking,
}

fn rank_measure(per_track: HashMap<String, Vec<(String, f64)>>) -> MeasureRanking {
    let mut ranked = Vec::new();
    let mut short = Vec::new();
    for (track_id, mut vals) in per_track {
        vals.sort_by(|a, b| a.0.cmp(&b.0));
        let ids: Vec<String> = vals.iter().map(|v| v.0.clone()).collect();
        if vals.len() < MIN_SESSIONS_PER_TRACK {
            short.push(UnrankedTrack {
                track_id,
                n: vals.len(),
                session_ids: ids,
                reason: "not enough sessions".into(),
            });
        } else {
            let m = vals.iter().map(|v| v.1).sum::<f64>() / vals.len() as f64;
            ranked.push(RankedTrack {
                rank: 0,
                track_id,
                n: vals.len(),
                mean: m,
                session_ids: ids,
            });
        }
    }
    ranked.sort_by(|a, b| {
        b.mean
            .total_cmp(&a.mean)
            .then_with(|| a.track_id.cmp(&b.track_id))
    });
    let mut prev: Option<(f64, usize)> = None;
    for (i, r) in ranked.iter_mut().enumerate() {
        r.rank = match prev {
            Some((m, rank)) if m == r.mean => rank,
            _ => i + 1,
        };
        prev = Some((r.mean, r.rank));
    }
    short.sort_by(|a, b| a.track_id.cmp(&b.track_id));
    MeasureRanking {
        ranked,
        not_enough_sessions: short,
    }
}

/// Focus (`avg_focus`) and calm (`calm_pct`, decision 0020; never `avg_relaxation`) of ONLY the
/// linked sessions: files are matched by the `_<id>.json` name suffix before anything is parsed.
fn rank_metrics(
    muse_sessions_dir: &Path,
    wanted: &[&str],
) -> HashMap<String, (Option<f64>, Option<f64>)> {
    let mut out = HashMap::new();
    let Ok(rd) = fs::read_dir(muse_sessions_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        // The id is exactly what follows the last "_" in the stem: "muse_a1" is a1, never "1".
        let Some(file_id) = stem.rsplit_once('_').map(|(_, id)| id) else {
            continue;
        };
        let Some(id) = wanted.iter().find(|id| **id == file_id).copied() else {
            continue;
        };
        if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > MAX_RANK_SESSION_BYTES {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(sv) = v.get("session").cloned() else {
            continue;
        };
        if let Ok(mut s) = serde_json::from_value::<sessions::RecordedSession>(sv) {
            s.summary.normalize_legacy_scores();
            if s.id == id {
                out.insert(
                    id.to_string(),
                    (finite(s.summary.avg_focus), finite(s.summary.calm_pct)),
                );
            }
        }
    }
    out
}

/// Tracks ranked by measured EEG effect (mean focus, mean calm_pct of their linked Muse sessions)
/// and, separately, by mean rating. These are plain means, not a controlled comparison.
pub(crate) fn ranking_in(data: &Path, muse_sessions_dir: &Path) -> Result<TrackRanking, String> {
    let store = load_store(data)?;
    let ids: Vec<&str> = store.sessions.iter().map(|s| s.id.as_str()).collect();
    let metrics = rank_metrics(muse_sessions_dir, &ids);
    let (mut focus, mut calm, mut rating) = (HashMap::new(), HashMap::new(), HashMap::new());
    for s in &store.sessions {
        // Every track appears in every measure, so one with no values shows up as "not enough".
        let f: &mut Vec<_> = focus.entry(s.track_id.clone()).or_default();
        let c: &mut Vec<_> = calm.entry(s.track_id.clone()).or_default();
        let r: &mut Vec<_> = rating.entry(s.track_id.clone()).or_default();
        let (mf, mc) = metrics.get(&s.id).copied().unwrap_or((None, None));
        if let Some(v) = mf {
            f.push((s.id.clone(), v));
        }
        if let Some(v) = mc {
            c.push((s.id.clone(), v));
        }
        if let Some(v) = s.rating {
            r.push((s.id.clone(), f64::from(v)));
        }
    }
    Ok(TrackRanking {
        min_sessions: MIN_SESSIONS_PER_TRACK,
        focus_field: "avg_focus".into(),
        calm_field: "calm_pct".into(),
        focus: rank_measure(focus),
        calm: rank_measure(calm),
        rating: rank_measure(rating),
    })
}

// ---------------------------------------------------------------- Amara context

/// Unicode format characters (zero-width, bidi marks and overrides, BOM, ...): dropped, so they
/// can't hide or reorder text.
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

fn plain(s: &str) -> String {
    tidy(
        &s.chars()
            .filter(|c| !is_format_char(*c))
            .map(|c| {
                if c.is_control() || c == '<' || c == '>' {
                    ' '
                } else {
                    c
                }
            })
            .collect::<String>(),
    )
}

/// `plain`, then at most `max` characters.
fn plain_capped(s: &str, max: usize) -> String {
    plain(s).chars().take(max).collect()
}

fn num_or_na(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".to_string(), |x| format!("{}", x.round() as i64))
}

/// Body of the `<nexus_hemisync>` section for one Amara turn (the caller runs it through the
/// context guard and adds the tags): the last 5 sessions, no paths, notes cut at 120 chars.
/// None when there is nothing to say. Listening is reported in minutes (track length is not
/// known to the backend, so there is no listened percentage).
pub(crate) fn context_section_in(data: &Path, muse_sessions_dir: &Path) -> Option<String> {
    let rows = sessions_in(data, muse_sessions_dir).ok()?;
    if rows.is_empty() {
        return None;
    }
    let lib = scan_library(data);
    let mut lines = vec![
        "Taisun's latest Hemi-Sync listening sessions recorded with the Muse (local, newest first):"
            .to_string(),
    ];
    for r in rows.iter().take(AMARA_SESSIONS) {
        let date: String = r.started_at.chars().take(10).collect();
        let track = lib.tracks.iter().find(|t| t.id == r.track_id).map_or_else(
            || "unknown track".to_string(),
            |t| {
                if t.wave > 0 {
                    format!(
                        "Wave {} {}, {}",
                        t.wave,
                        plain_capped(&t.wave_name, AMARA_TITLE_CHARS),
                        plain_capped(&t.title, AMARA_TITLE_CHARS)
                    )
                } else {
                    plain_capped(&t.title, AMARA_TITLE_CHARS)
                }
            },
        );
        let mut line = format!(
            "- {date}, {track}: listened {} min, completed {}, rating {}, focus {}, calm {}",
            r.listened_ms / 60_000,
            if r.completed { "yes" } else { "no" },
            r.rating
                .map_or_else(|| "n/a".to_string(), |v| format!("{v}/5")),
            num_or_na(r.metrics.focus_avg),
            num_or_na(r.metrics.calm_avg),
        );
        if let Some(note) = &r.note {
            let n: String = plain(note).chars().take(AMARA_NOTE_CHARS).collect();
            if !n.is_empty() {
                line.push_str(&format!(", note: {n}"));
            }
        }
        lines.push(line);
    }
    Some(lines.join("\n"))
}

/// For `terrence_runtime`: the live section from the real data dir.
pub(crate) fn amara_context_section() -> Option<String> {
    context_section_in(&crate::app_paths::data_dir(), &sessions::sessions_dir())
}

// ---------------------------------------------------------------- audio scheme

/// One response chunk is at most this big; the client re-requests the rest with `Range`.
const MAX_BODY: u64 = 4 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct Served {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Served {
    fn empty(status: u16) -> Self {
        Served {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("aiff") => "audio/aiff",
        Some("aac") => "audio/aac",
        _ => "application/octet-stream",
    }
}

/// `Range: bytes=a-b | a- | -n` against `size`: inclusive (start, end), or None when it can't
/// be satisfied. A header that isn't a single `bytes=` range is `Err(())` (treated as absent).
fn parse_range(header: &str, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let spec = header.trim().strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') {
        return Err(());
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let num = |s: &str| -> Result<Option<u64>, ()> {
        if s.is_empty() {
            Ok(None)
        } else if s.bytes().all(|c| c.is_ascii_digit()) {
            s.parse::<u64>().map(Some).map_err(|_| ())
        } else {
            Err(())
        }
    };
    let (a, b) = (num(a.trim())?, num(b.trim())?);
    if size == 0 {
        return Ok(None);
    }
    Ok(match (a, b) {
        (Some(a), b) if a < size && b.is_none_or(|b| b >= a) => {
            Some((a, b.map_or(size - 1, |b| b.min(size - 1))))
        }
        (None, Some(n)) if n > 0 => Some((size.saturating_sub(n), size - 1)),
        _ => None,
    })
}

/// Request -> response for `hemisync://localhost/<id>`. Pure apart from reading the file that
/// `lookup` returns (opened O_NOFOLLOW, never loaded whole). `target` is the request's path and
/// query; only `/` + exactly 16 lowercase hex characters is served. Nothing in the response
/// echoes the request.
pub(crate) fn serve(
    method: &str,
    target: &str,
    range: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<PathBuf>,
) -> Served {
    let head = match method {
        "GET" => false,
        "HEAD" => true,
        _ => {
            return Served {
                headers: vec![("Allow", "GET, HEAD".to_string())],
                ..Served::empty(405)
            }
        }
    };
    let id = match target.strip_prefix('/') {
        Some(id) if valid_track_id(id) => id,
        _ => return Served::empty(404),
    };
    let Some(path) = lookup(id) else {
        return Served::empty(404);
    };
    let Ok(mut file) = open_nofollow(&path) else {
        return Served::empty(404);
    };
    let Ok(meta) = file.metadata() else {
        return Served::empty(404);
    };
    if !meta.is_file() {
        return Served::empty(404);
    }
    let size = meta.len();
    let (start, end, partial) = match range.map(|r| parse_range(r, size)) {
        Some(Ok(Some((a, b)))) => (a, b, true),
        Some(Ok(None)) => {
            return Served {
                headers: vec![("Content-Range", format!("bytes */{size}"))],
                ..Served::empty(416)
            }
        }
        _ if size == 0 => (0, 0, false),
        _ => (0, size - 1, false),
    };
    let mut headers = vec![
        ("Content-Type", mime_for(&path).to_string()),
        ("Accept-Ranges", "bytes".to_string()),
        ("X-Content-Type-Options", "nosniff".to_string()),
        ("Cache-Control", "no-store".to_string()),
    ];
    if size == 0 {
        headers.push(("Content-Length", "0".to_string()));
        return Served {
            status: 200,
            headers,
            body: Vec::new(),
        };
    }
    let end = end.min(start.saturating_add(MAX_BODY - 1));
    let len = end - start + 1;
    let status = if partial || end < size - 1 { 206 } else { 200 };
    if status == 206 {
        headers.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
    }
    headers.push(("Content-Length", len.to_string()));
    let mut body = Vec::new();
    if !head {
        use std::io::{Seek, SeekFrom};
        if file.seek(SeekFrom::Start(start)).is_err()
            || Read::by_ref(&mut file)
                .take(len)
                .read_to_end(&mut body)
                .is_err()
            || body.len() as u64 != len
        {
            return Served::empty(404);
        }
    }
    Served {
        status,
        headers,
        body,
    }
}

/// Only the app's own hosts: `localhost` (macOS/Linux) and `hemisync.localhost` (Windows).
fn host_ok(host: Option<&str>) -> bool {
    matches!(host, Some("localhost") | Some("hemisync.localhost"))
}

/// At most this many requests are served at once; the rest get 503.
const MAX_IN_FLIGHT: usize = 4;
/// The id -> path map is rebuilt at most this often (and after every import).
const PATH_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct InFlight<'a>(&'a std::sync::atomic::AtomicUsize);
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Run `f` unless `cap` requests are already in flight (then 503). The counter always goes back down.
pub(crate) fn with_cap(
    counter: &std::sync::atomic::AtomicUsize,
    cap: usize,
    f: impl FnOnce() -> Served,
) -> Served {
    if counter.fetch_add(1, Ordering::AcqRel) >= cap {
        counter.fetch_sub(1, Ordering::AcqRel);
        return Served {
            headers: vec![("Retry-After", "1".to_string())],
            ..Served::empty(503)
        };
    }
    let _guard = InFlight(counter);
    f()
}

/// Short-lived id -> path map so a burst of range requests doesn't rescan the library each time.
type CacheSlot = (std::time::Instant, PathBuf, HashMap<String, PathBuf>);

pub(crate) struct PathCache {
    slot: Mutex<Option<CacheSlot>>,
}

impl PathCache {
    pub(crate) const fn new() -> Self {
        PathCache {
            slot: Mutex::new(None),
        }
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut s) = self.slot.lock() {
            *s = None;
        }
    }
    /// `build` runs only when the cache is empty, older than `ttl`, or for another data dir.
    pub(crate) fn get(
        &self,
        data: &Path,
        now: std::time::Instant,
        ttl: std::time::Duration,
        id: &str,
        build: impl FnOnce() -> HashMap<String, PathBuf>,
    ) -> Option<PathBuf> {
        let mut slot = self.slot.lock().ok()?;
        let fresh = slot
            .as_ref()
            .is_some_and(|(at, d, _)| d == data && now.saturating_duration_since(*at) < ttl);
        if !fresh {
            *slot = Some((now, data.to_path_buf(), build()));
        }
        slot.as_ref().and_then(|(_, _, m)| m.get(id).cloned())
    }
}

static PATH_CACHE: PathCache = PathCache::new();

/// The scheme handler: live lookup through the library scan (id -> canonical path inside
/// `<data>/hemisync/`).
pub(crate) fn handle(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri();
    let served = if host_ok(uri.host()) {
        let data = crate::app_paths::data_dir();
        let range = request.headers().get("range").and_then(|v| v.to_str().ok());
        let target = uri.path_and_query().map_or("", |p| p.as_str());
        with_cap(&IN_FLIGHT, MAX_IN_FLIGHT, || {
            serve(request.method().as_str(), target, range, &|id| {
                PATH_CACHE.get(&data, std::time::Instant::now(), PATH_CACHE_TTL, id, || {
                    library_paths(&data)
                })
            })
        })
    } else {
        Served::empty(404)
    };
    let mut b = tauri::http::Response::builder().status(served.status);
    for (k, v) in &served.headers {
        b = b.header(*k, v.as_str());
    }
    b.body(served.body).unwrap_or_else(|_| {
        let mut r = tauri::http::Response::new(Vec::new());
        *r.status_mut() = tauri::http::StatusCode::NOT_FOUND;
        r
    })
}

// ---------------------------------------------------------------- commands

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "dir_invalid".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn hemisync_import(
    app: tauri::AppHandle,
    folder: Option<String>,
) -> Result<ImportResult, String> {
    let (home, data) = (home_dir()?, crate::app_paths::data_dir());
    blocking(JoinErr::Code("io_error"), move || {
        let emit = |copied: u64, total: u64, phase: &str| {
            let _ = app.emit(
                IMPORT_EVENT,
                ProgressEvent {
                    copied,
                    total,
                    phase,
                },
            );
        };
        import_in(
            &home,
            &data,
            folder.as_deref(),
            &backups::free_bytes,
            &emit,
            &IMPORT_BUSY,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn hemisync_library() -> Result<Library, String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || Ok(scan_library(&data))).await
}

#[tauri::command]
pub async fn hemisync_session_link(
    session_id: String,
    track_id: String,
    started_at: String,
) -> Result<(), String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        link_in(&data, &session_id, &track_id, &started_at)
    })
    .await
}

#[tauri::command]
pub async fn hemisync_session_finish(
    session_id: String,
    listened_ms: u64,
    completed: bool,
) -> Result<(), String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        finish_in(&data, &session_id, listened_ms, completed)
    })
    .await
}

#[tauri::command]
pub async fn hemisync_session_rate(
    session_id: String,
    rating: u8,
    note: Option<String>,
) -> Result<(), String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        rate_in(&data, &session_id, rating, note.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn hemisync_sessions() -> Result<Vec<SessionRow>, String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        sessions_in(&data, &sessions::sessions_dir())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn hemisync_stats() -> Result<Vec<TrackStats>, String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        sessions_in(&data, &sessions::sessions_dir()).map(|r| stats_from(&r))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn hemisync_ranking() -> Result<TrackRanking, String> {
    let data = crate::app_paths::data_dir();
    blocking(JoinErr::Code("io_error"), move || {
        ranking_in(&data, &sessions::sessions_dir())
    })
    .await
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests;
