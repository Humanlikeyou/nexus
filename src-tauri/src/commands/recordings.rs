//! Read-only access to Muse recordings (`replays/*.ndjson`) for the Neuro screens.
//!
//! Muse recordings are sacred (.claude/rules/muse-recordings.md): every reader here opens a
//! replay with `File::open` (read-only) and nothing in this module can remove, move or modify
//! one. The only thing written is a derived cache beside the replay, `<file>.bands.json`, via a
//! temp file and a rename, and never for the file that is being recorded. There is deliberately
//! no delete command in this module.
//!
//! Replay formats: see `replay.rs`. Format 1 (no `format` field: four channels, no trailer) and
//! format 2 (lazy header, AUX, PPG, motion, trailer) both parse. A recording that crashed has no
//! trailer and may end in a half line; both are tolerated.
//!
//! Gaps: a jump of more than 0.5 s between packets of one stream leaves NaN (JSON `null`) for the
//! missing time instead of joining the two sides.

use super::shared::task::{blocking, JoinErr};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::path_safety::{safe_join, validate_file_name, validate_name};
use super::recordings_dsp as dsp;
use super::replay::AuxInfo;
use super::sessions::{self, StoredSession};

pub const RETENTION_DAYS: i64 = 90;
const DAY_MS: f64 = 86_400_000.0;
const STEP_SECS: usize = 5;
const MAX_WINDOW_SECS: f64 = 60.0;
const GAP_SECS: f64 = 0.5;
const EEG_RATE: f64 = 256.0;
const PPG_RATE: f64 = 64.0;
const MOTION_RATE: f64 = 52.0;
/// 2 (decision 0060): focus/relaxation from the one band-power pipeline. Older sidecars are caches and are
/// rebuilt from their replay the next time the recording is opened; the replay itself is never touched.
const SIDECAR_VERSION: u32 = 2;
const SENSORS: [&str; 4] = ["TP9", "AF7", "AF8", "TP10"];
const KNOWN_CHANNELS: [&str; 5] = ["TP9", "AF7", "AF8", "TP10", "AUX"];
const INDEX_CACHE_SIZE: usize = 4;

// ── IPC types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingListItem {
    /// Replay file stem (what the window readers take), or the session id when there is no replay.
    pub id: String,
    pub session_id: Option<String>,
    pub replay_file: Option<String>,
    pub started_at: String,
    pub duration_secs: u64,
    pub protocol: Option<String>,
    pub channels: Vec<String>,
    pub aux: Option<AuxInfo>,
    pub has_heart: bool,
    pub has_motion: bool,
    pub avg_focus: Option<f64>,
    pub avg_calm: Option<f64>,
    pub time_in_target: Option<f64>,
    pub calm_pct: Option<f64>,
    /// `available`, `recording`, `summary-only` or `missing`.
    pub raw: String,
    pub size_bytes: Option<u64>,
    /// `kept` or `expiring` (older than 90 days). Computed only; nothing acts on it.
    pub retention: String,
}

type Series = Vec<Option<f64>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct Bands {
    pub delta: Series,
    pub theta: Series,
    pub alpha: Series,
    pub beta: Series,
    pub gamma: Series,
}

/// Per-step series of a recording (5 s steps). Missing steps are `null`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSamples {
    pub step_secs: u32,
    pub bands: Bands,
    pub focus: Series,
    pub calm: Series,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensors: Option<BTreeMap<String, BTreeMap<String, Series>>>,
    /// Contact quality per sensor over the recording, 0-100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<BTreeMap<String, f64>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSeries {
    pub step_secs: u32,
    pub bands: Bands,
    pub focus: Series,
    pub calm: Series,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub channel: String,
    /// `good`, `fair`, `poor` or `none` (too little data).
    pub contact: String,
    pub values: Series,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignalWindow {
    pub start_secs: f64,
    pub end_secs: f64,
    pub sample_rate: u32,
    pub lanes: Vec<Lane>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HeartWindow {
    pub ppg_rate: u32,
    pub values: Series,
    pub bpm: Option<f64>,
    pub estimate: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MotionEvent {
    pub at_secs: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MotionWindow {
    pub rate: u32,
    pub values: Series,
    pub events: Vec<MotionEvent>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BodyWindow {
    pub start_secs: f64,
    pub end_secs: f64,
    pub heart: Option<HeartWindow>,
    pub motion: Option<MotionWindow>,
}

// ── Locating a replay ───────────────────────────────────────────

/// `id` is the replay's file stem (`eeg_20260217_100521`). Goes through validate_name,
/// validate_file_name and safe_join, so `..`, separators and symlinks out of `replays/` fail.
/// Never creates the directory.
pub fn resolve_replay(dir: &Path, id: &str) -> Result<PathBuf, String> {
    validate_name(id)?;
    let file = format!("{id}.ndjson");
    validate_file_name(&file, &["ndjson"])?;
    if !dir.is_dir() {
        return Err("Recording not found".to_string());
    }
    let path = safe_join(dir, &file)?;
    if !path.is_file() {
        return Err("Recording not found".to_string());
    }
    Ok(path)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

// ── Parsing ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderInfo {
    pub format: u8,
    pub channels: Vec<String>,
    pub ppg: Vec<String>,
    pub motion: Vec<String>,
    pub start_ts: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrailerInfo {
    pub end_ts: Option<f64>,
    pub aux: Option<AuxInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayIndex {
    pub header: Option<HeaderInfo>,
    pub trailer: Option<TrailerInfo>,
    pub first_ts: Option<f64>,
    pub last_ts: Option<f64>,
    /// `offsets[s]`: byte offset of the first data line at or after second `s` of the recording.
    pub offsets: Vec<u64>,
    pub has_ppg: bool,
    pub has_motion: bool,
    /// EEG channels that have sample lines, in first-seen order.
    pub channels: Vec<String>,
    /// A last line that did not parse (cut off by a crash) was dropped.
    pub cut_off: bool,
}

impl ReplayIndex {
    pub fn duration_secs(&self) -> Option<f64> {
        Some(((self.last_ts? - self.first_ts?) / 1000.0).max(0.0))
    }

    fn eeg_channels(&self) -> Vec<String> {
        let listed: Vec<String> = match &self.header {
            Some(h) => h.channels.clone(),
            None => SENSORS.iter().map(|c| c.to_string()).collect(),
        };
        let mut out: Vec<String> = listed
            .into_iter()
            .filter(|c| KNOWN_CHANNELS.contains(&c.as_str()))
            .collect();
        for c in &self.channels {
            if KNOWN_CHANNELS.contains(&c.as_str()) && !out.contains(c) {
                out.push(c.clone());
            }
        }
        out
    }
}

#[derive(Deserialize)]
struct Probe {
    kind: Option<String>,
    timestamp: Option<f64>,
    channel: Option<String>,
}

#[derive(Deserialize)]
struct SampleLine {
    samples: Vec<f64>,
}

fn strings(v: &serde_json::Value, key: &str, default: &[&str]) -> Vec<String> {
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| default.iter().map(|s| s.to_string()).collect())
}

fn header_from(v: &serde_json::Value) -> HeaderInfo {
    HeaderInfo {
        format: v
            .get("format")
            .and_then(|f| f.as_u64())
            .map(|f| f as u8)
            .unwrap_or(1),
        channels: strings(v, "channels", &["TP9", "AF7", "AF8", "TP10"]),
        ppg: strings(v, "ppg", &[]),
        motion: strings(v, "motion", &[]),
        start_ts: v.get("start_ts").and_then(|t| t.as_f64()),
    }
}

fn trailer_from(v: &serde_json::Value) -> TrailerInfo {
    TrailerInfo {
        end_ts: v.get("end_ts").and_then(|t| t.as_f64()),
        aux: v
            .get("aux")
            .filter(|a| !a.is_null())
            .and_then(|a| serde_json::from_value(a.clone()).ok()),
    }
}

/// One streaming pass over a replay: header, trailer, time span and 1 s byte offsets. Opens the
/// file read-only; malformed lines are skipped.
pub fn parse_replay(path: &Path) -> std::io::Result<ReplayIndex> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut idx = ReplayIndex::default();
    let mut buf = Vec::new();
    let mut offset = 0u64;
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let line_offset = offset;
        offset += n as u64;
        let text = buf.trim_ascii();
        if text.is_empty() {
            continue;
        }
        let Ok(probe) = serde_json::from_slice::<Probe>(text) else {
            idx.cut_off = !buf.ends_with(b"\n");
            continue;
        };
        match probe.kind.as_deref() {
            Some("header") => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(text) {
                    idx.header = Some(header_from(&v));
                }
            }
            Some("trailer") => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(text) {
                    idx.trailer = Some(trailer_from(&v));
                }
            }
            Some(kind @ ("sample" | "ppg" | "motion")) => {
                let Some(ts) = probe.timestamp else { continue };
                match kind {
                    "ppg" => idx.has_ppg = true,
                    "motion" => idx.has_motion = true,
                    _ => {
                        if let Some(c) = probe.channel {
                            if !idx.channels.contains(&c) {
                                idx.channels.push(c);
                            }
                        }
                    }
                }
                let first = *idx.first_ts.get_or_insert(ts);
                let secs = ((ts - first).max(0.0) / 1000.0).floor() as usize;
                // A corrupt timestamp far past the start would otherwise allocate one offset per
                // second up to it; such a line is left out of the time index.
                if secs <= MAX_INDEX_SECS {
                    idx.last_ts = Some(idx.last_ts.map_or(ts, |l| l.max(ts)));
                    while idx.offsets.len() <= secs {
                        idx.offsets.push(line_offset);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(idx)
}

type CacheEntry = (PathBuf, u64, Option<SystemTime>, Arc<ReplayIndex>);

static INDEX_CACHE: Mutex<Vec<CacheEntry>> = Mutex::new(Vec::new());

/// `parse_replay` behind a 4-entry LRU keyed on path, length and mtime (a growing file re-parses).
pub fn get_index(path: &Path) -> Result<Arc<ReplayIndex>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Failed to read recording: {e}"))?;
    let (len, mtime) = (meta.len(), meta.modified().ok());
    if let Ok(mut cache) = INDEX_CACHE.lock() {
        if let Some(pos) = cache
            .iter()
            .position(|(p, l, m, _)| p == path && *l == len && *m == mtime)
        {
            let entry = cache.remove(pos);
            let idx = entry.3.clone();
            cache.insert(0, entry);
            return Ok(idx);
        }
    }
    let idx = Arc::new(parse_replay(path).map_err(|e| format!("Failed to read recording: {e}"))?);
    if let Ok(mut cache) = INDEX_CACHE.lock() {
        cache.retain(|(p, ..)| p != path);
        cache.insert(0, (path.to_path_buf(), len, mtime, idx.clone()));
        cache.truncate(INDEX_CACHE_SIZE);
    }
    Ok(idx)
}

/// Header (first line) and trailer (last 8 KiB) without reading the body. Read-only.
fn peek_replay(path: &Path) -> (Option<HeaderInfo>, Option<TrailerInfo>) {
    let Ok(file) = File::open(path) else {
        return (None, None);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut reader = BufReader::new(file);
    let mut first = String::new();
    let header = reader
        .read_line(&mut first)
        .ok()
        .and_then(|_| serde_json::from_str::<serde_json::Value>(first.trim()).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("header"))
        .map(|v| header_from(&v));
    let tail_len = len.min(8192);
    let mut tail = Vec::new();
    let trailer = reader
        .seek(SeekFrom::Start(len - tail_len))
        .and_then(|_| reader.read_to_end(&mut tail))
        .ok()
        .and_then(|_| {
            tail.split(|b| *b == b'\n')
                .rev()
                .filter_map(|l| serde_json::from_slice::<serde_json::Value>(l.trim_ascii()).ok())
                .find(|v| v.get("kind").and_then(|k| k.as_str()) == Some("trailer"))
        })
        .map(|v| trailer_from(&v));
    (header, trailer)
}

// ── Placing packets on a time axis ──────────────────────────────

/// Lays the packets of one stream end to end. A jump of more than 0.5 s between packets restarts
/// the position from the clock, which leaves the missing time empty.
struct Cursor {
    rate: f64,
    next: Option<i64>,
    prev_rel: f64,
}

impl Cursor {
    fn new(rate: f64) -> Self {
        Cursor {
            rate,
            next: None,
            prev_rel: 0.0,
        }
    }

    /// Absolute index (from the recording's first timestamp) of the packet's first sample.
    fn place(&mut self, rel_secs: f64, n: usize) -> i64 {
        let jump = rel_secs - self.prev_rel > GAP_SECS;
        if self.next.is_none() || jump {
            self.next = Some((rel_secs * self.rate).round() as i64 - n as i64);
        }
        self.prev_rel = rel_secs;
        let at = self.next.unwrap_or(0);
        self.next = Some(at + n as i64);
        at
    }
}

/// Calls `f(rel_secs, value)` for each data line from `start - 0.1` to `end + 0.1` seconds.
fn for_each_line(
    path: &Path,
    idx: &ReplayIndex,
    start: f64,
    end: f64,
    mut f: impl FnMut(f64, &serde_json::Value),
) -> Result<(), String> {
    let Some(first) = idx.first_ts else {
        return Ok(());
    };
    let sec = start.floor().max(0.0) as usize;
    let Some(&offset) = idx.offsets.get(sec) else {
        return Ok(());
    };
    let mut file = File::open(path).map_err(|e| format!("Failed to read recording: {e}"))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Failed to read recording: {e}"))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if !matches!(
            v.get("kind").and_then(|k| k.as_str()),
            Some("sample" | "ppg" | "motion")
        ) {
            continue;
        }
        let Some(ts) = v.get("timestamp").and_then(|t| t.as_f64()) else {
            continue;
        };
        let rel = (ts - first) / 1000.0;
        if rel > end + 0.1 {
            break;
        }
        if rel >= start - 0.1 {
            f(rel, &v);
        }
    }
    Ok(())
}

fn floats(v: &serde_json::Value) -> Vec<f64> {
    v.get("samples")
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

fn check_window(start: f64, span: f64) -> Result<(), String> {
    if !start.is_finite() || !span.is_finite() || start < 0.0 || span <= 0.0 {
        return Err("Invalid window".to_string());
    }
    if span > MAX_WINDOW_SECS {
        return Err(format!("Window is limited to {MAX_WINDOW_SECS} s"));
    }
    Ok(())
}

fn opt(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn to_series(v: &[f64]) -> Series {
    v.iter().map(|x| opt(*x)).collect()
}

fn contact_of(raw: &[f64]) -> &'static str {
    let finite: Vec<f64> = raw.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < 64 {
        return "none";
    }
    dsp::assess_channel(&finite[finite.len().saturating_sub(256)..]).as_str()
}

// ── Windows ─────────────────────────────────────────────────────

/// Raw EEG for up to 60 s: each channel high-passed at 0.5 Hz and bucketed to `points` values.
pub fn read_recording_window_in(
    dir: &Path,
    id: &str,
    start: f64,
    span: f64,
    points: Option<usize>,
) -> Result<SignalWindow, String> {
    check_window(start, span)?;
    let path = resolve_replay(dir, id)?;
    let idx = get_index(&path)?;
    let n = (span * EEG_RATE).round() as usize;
    let origin = (start * EEG_RATE).round() as i64;
    let channels = idx.eeg_channels();
    let mut raw: HashMap<String, Vec<f64>> = channels
        .iter()
        .map(|c| (c.clone(), vec![f64::NAN; n]))
        .collect();
    let mut cursors: HashMap<String, Cursor> = HashMap::new();
    for_each_line(&path, &idx, start, start + span, |rel, v| {
        if v.get("kind").and_then(|k| k.as_str()) != Some("sample") {
            return;
        }
        let Some(ch) = v.get("channel").and_then(|c| c.as_str()) else {
            return;
        };
        let Some(buf) = raw.get_mut(ch) else { return };
        let samples = floats(v);
        let at = cursors
            .entry(ch.to_string())
            .or_insert_with(|| Cursor::new(EEG_RATE))
            .place(rel, samples.len());
        for (i, s) in samples.into_iter().enumerate() {
            let pos = at + i as i64 - origin;
            if (0..n as i64).contains(&pos) {
                buf[pos as usize] = s;
            }
        }
    })?;
    let points = points.unwrap_or(900).clamp(16, 4096);
    let lanes = channels
        .iter()
        .map(|c| {
            let samples = &raw[c];
            Lane {
                channel: c.clone(),
                contact: contact_of(samples).to_string(),
                values: to_series(&dsp::bucket_mean(&dsp::high_pass_05hz(samples), points)),
            }
        })
        .collect();
    Ok(SignalWindow {
        start_secs: start,
        end_secs: start + span,
        sample_rate: 256,
        lanes,
    })
}

fn triples_of(v: &serde_json::Value) -> Vec<[f64; 3]> {
    v.get("samples")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let t = t.as_array()?;
                    Some([
                        t.first()?.as_f64()?,
                        t.get(1)?.as_f64()?,
                        t.get(2)?.as_f64()?,
                    ])
                })
                .collect()
        })
        .unwrap_or_default()
}

/// PPG (heart) and gyroscope (head movement) for up to 60 s. A part is `None` when the
/// recording has none of it in the window. The heart rate is an estimate.
pub fn read_body_window_in(
    dir: &Path,
    id: &str,
    start: f64,
    span: f64,
) -> Result<BodyWindow, String> {
    check_window(start, span)?;
    let path = resolve_replay(dir, id)?;
    let idx = get_index(&path)?;
    let n_ppg = (span * PPG_RATE).round() as usize;
    let n_mot = (span * MOTION_RATE).round() as usize;
    let o_ppg = (start * PPG_RATE).round() as i64;
    let o_mot = (start * MOTION_RATE).round() as i64;
    let mut ppg: HashMap<String, Vec<f64>> = HashMap::new();
    let mut motion: HashMap<String, Vec<[f64; 3]>> = HashMap::new();
    let mut cursors: HashMap<String, Cursor> = HashMap::new();
    for_each_line(&path, &idx, start, start + span, |rel, v| {
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("ppg") => {
                let Some(src) = v.get("source").and_then(|s| s.as_str()) else {
                    return;
                };
                let samples = floats(v);
                let buf = ppg
                    .entry(src.to_string())
                    .or_insert_with(|| vec![f64::NAN; n_ppg]);
                let at = cursors
                    .entry(format!("ppg:{src}"))
                    .or_insert_with(|| Cursor::new(PPG_RATE))
                    .place(rel, samples.len());
                for (i, s) in samples.into_iter().enumerate() {
                    let pos = at + i as i64 - o_ppg;
                    if (0..n_ppg as i64).contains(&pos) {
                        buf[pos as usize] = s;
                    }
                }
            }
            Some("motion") => {
                let Some(sensor) = v.get("sensor").and_then(|s| s.as_str()) else {
                    return;
                };
                let triples = triples_of(v);
                let buf = motion
                    .entry(sensor.to_string())
                    .or_insert_with(|| vec![[f64::NAN; 3]; n_mot]);
                let at = cursors
                    .entry(format!("motion:{sensor}"))
                    .or_insert_with(|| Cursor::new(MOTION_RATE))
                    .place(rel, triples.len());
                for (i, s) in triples.into_iter().enumerate() {
                    let pos = at + i as i64 - o_mot;
                    if (0..n_mot as i64).contains(&pos) {
                        buf[pos as usize] = s;
                    }
                }
            }
            _ => {}
        }
    })?;

    let heart = ["infrared", "red", "ambient"]
        .iter()
        .find_map(|s| ppg.get(*s))
        .filter(|v| v.iter().any(|x| x.is_finite()))
        .map(|values| HeartWindow {
            ppg_rate: 64,
            values: to_series(&dsp::bucket_mean(
                &dsp::high_pass(values, PPG_RATE, 0.5),
                600,
            )),
            bpm: dsp::estimate_bpm(values, PPG_RATE),
            estimate: true,
        });

    let chosen = match motion.get("gyroscope") {
        Some(g) => Some((g, true)),
        None => motion.get("accelerometer").map(|a| (a, false)),
    };
    let motion_part = chosen
        .filter(|(v, _)| v.iter().any(|s| s[0].is_finite()))
        .map(|(samples, is_gyro)| {
            let finite: Vec<(usize, [f64; 3])> = samples
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, s)| s.iter().all(|x| x.is_finite()))
                .collect();
            let (dev, events) =
                dsp::head_movement(&finite.iter().map(|(_, s)| *s).collect::<Vec<_>>());
            let mut values = vec![f64::NAN; samples.len()];
            for ((i, _), d) in finite.iter().zip(&dev) {
                values[*i] = *d;
            }
            MotionWindow {
                rate: 52,
                values: to_series(&dsp::bucket_mean(&values, 600)),
                // Only the gyroscope has a movement threshold.
                events: if is_gyro {
                    events
                        .into_iter()
                        .map(|e| MotionEvent {
                            at_secs: start + finite[e].0 as f64 / MOTION_RATE,
                            label: "Head movement".to_string(),
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            }
        });

    Ok(BodyWindow {
        start_secs: start,
        end_secs: start + span,
        heart,
        motion: motion_part,
    })
}

// ── Series over a whole recording ───────────────────────────────

/// One entry of `data.neurofeedback.timeline` in a saved session.
#[derive(Debug, Clone, Deserialize)]
pub struct TimelineEntry {
    pub timestamp: f64,
    #[serde(default)]
    pub focus: f64,
    #[serde(default)]
    pub relaxation: f64,
    #[serde(default)]
    pub gated: bool,
}

#[derive(Default)]
struct ChannelStats {
    good: u32,
    fair: u32,
    total: u32,
}

struct Chan {
    cursor: Cursor,
    buf: Vec<f64>,
}

fn mean_or_nan(sum: f64, n: u32) -> f64 {
    if n == 0 {
        f64::NAN
    } else {
        sum / n as f64
    }
}

#[derive(Default)]
struct SeriesAcc {
    bands: [Vec<f64>; 5],
    sensors: HashMap<&'static str, [Vec<f64>; 5]>,
    focus: Vec<f64>,
    calm: Vec<f64>,
    stats: HashMap<&'static str, ChannelStats>,
}

impl SeriesAcc {
    /// Reduces one 5 s step (the first 1280 samples of each channel buffer) and drops it.
    fn process(&mut self, chans: &mut HashMap<&'static str, Chan>) {
        let win = EEG_RATE as usize;
        let step_len = STEP_SECS * win;
        let mut band_sum = [0.0f64; 5];
        let mut band_n = 0u32;
        let mut sensor_sum: HashMap<&str, ([f64; 5], u32)> =
            SENSORS.iter().map(|c| (*c, ([0.0; 5], 0))).collect();
        let (mut focus_sum, mut calm_sum, mut fc_n) = (0.0, 0.0, 0u32);
        for w in 0..STEP_SECS {
            let mut usable: Vec<Vec<f64>> = Vec::new();
            let mut quality: HashMap<&str, dsp::Quality> = HashMap::new();
            let mut slices: HashMap<&str, Vec<f64>> = HashMap::new();
            for c in SENSORS {
                let from = w * win;
                let slice: Vec<f64> = (from..from + win)
                    .map(|i| chans[c].buf.get(i).copied().unwrap_or(f64::NAN))
                    .collect();
                if slice.iter().any(|v| !v.is_finite()) {
                    continue;
                }
                let q = dsp::assess_channel(&slice);
                let st = self.stats.entry(c).or_default();
                st.total += 1;
                match q {
                    dsp::Quality::Good => st.good += 1,
                    dsp::Quality::Fair => st.fair += 1,
                    dsp::Quality::Poor => {}
                }
                quality.insert(c, q);
                if q != dsp::Quality::Poor {
                    if let Some(b) = dsp::relative_channel_band_powers(&[&slice]) {
                        if let Some(e) = sensor_sum.get_mut(c) {
                            for (a, v) in e.0.iter_mut().zip(b) {
                                *a += v;
                            }
                            e.1 += 1;
                        }
                    }
                    usable.push(slice.clone());
                }
                slices.insert(c, slice);
            }
            let refs: Vec<&[f64]> = usable.iter().map(|v| v.as_slice()).collect();
            if let Some(b) = dsp::relative_channel_band_powers(&refs) {
                for (a, v) in band_sum.iter_mut().zip(b) {
                    *a += v;
                }
                band_n += 1;
            }
            if quality.get("AF7") == Some(&dsp::Quality::Good)
                && quality.get("AF8") == Some(&dsp::Quality::Good)
            {
                // Decision 0060: power averaged across AF7 and AF8, then the scores (as training does).
                if let Some((f, r)) =
                    dsp::focus_relaxation_channels(&[&slices["AF7"], &slices["AF8"]])
                {
                    focus_sum += f;
                    calm_sum += r;
                    fc_n += 1;
                }
            }
        }
        for (out, sum) in self.bands.iter_mut().zip(band_sum) {
            out.push(mean_or_nan(sum, band_n));
        }
        for c in SENSORS {
            let (sum, n) = sensor_sum[c];
            let out = self.sensors.entry(c).or_default();
            for (o, s) in out.iter_mut().zip(sum) {
                o.push(mean_or_nan(s, n));
            }
        }
        self.focus.push(mean_or_nan(focus_sum, fc_n));
        self.calm.push(mean_or_nan(calm_sum, fc_n));
        for ch in chans.values_mut() {
            let n = ch.buf.len().min(step_len);
            ch.buf.drain(..n);
        }
    }

    fn finish(self, n_steps: usize, timeline: Option<(&[TimelineEntry], f64)>) -> SessionSamples {
        let [delta, theta, alpha, beta, gamma] = self.bands.map(|v| to_series(&v));
        let mut focus = to_series(&self.focus);
        let mut calm = to_series(&self.calm);
        if let Some((entries, origin_ms)) = timeline {
            let mut sums = vec![(0.0, 0.0, 0u32); n_steps];
            for e in entries.iter().filter(|e| !e.gated) {
                let rel = (e.timestamp - origin_ms) / 1000.0;
                if rel < 0.0 || !e.focus.is_finite() || !e.relaxation.is_finite() {
                    continue;
                }
                if let Some(s) = sums.get_mut((rel / STEP_SECS as f64) as usize) {
                    *s = (s.0 + e.focus, s.1 + e.relaxation, s.2 + 1);
                }
            }
            focus = sums.iter().map(|s| opt(mean_or_nan(s.0, s.2))).collect();
            calm = sums.iter().map(|s| opt(mean_or_nan(s.1, s.2))).collect();
        }
        let mut sensors = BTreeMap::new();
        let mut contact = BTreeMap::new();
        for c in SENSORS {
            let series = self.sensors.get(c).cloned().unwrap_or_default();
            let map: BTreeMap<String, Series> = dsp::BAND_NAMES
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    (
                        name.to_string(),
                        to_series(series.get(i).map(Vec::as_slice).unwrap_or(&[])),
                    )
                })
                .collect();
            sensors.insert(c.to_string(), map);
            if let Some(st) = self.stats.get(c).filter(|s| s.total > 0) {
                let pct = (st.good as f64 + 0.5 * st.fair as f64) / st.total as f64 * 100.0;
                contact.insert(c.to_string(), pct.round());
            }
        }
        SessionSamples {
            step_secs: STEP_SECS as u32,
            bands: Bands {
                delta,
                theta,
                alpha,
                beta,
                gamma,
            },
            focus,
            calm,
            sensors: Some(sensors),
            contact: (!contact.is_empty()).then_some(contact),
        }
    }
}

/// Band power, focus and calm per 5 s step, streamed from the replay (never held whole).
/// Each step is the mean of its five 1 s windows; a window counts for a channel only if it has no
/// gap, and bands skip channels with poor contact. Focus and calm count only windows where both
/// frontal channels (AF7, AF8) have good contact, as the live summary does; `timeline`
/// (entries, origin ms) replaces them with the recorded training timeline.
fn compute_samples(
    path: &Path,
    idx: &ReplayIndex,
    timeline: Option<(&[TimelineEntry], f64)>,
) -> Result<SessionSamples, String> {
    let duration = idx.duration_secs().unwrap_or(0.0);
    let n_steps = if duration >= 1.0 {
        (duration / STEP_SECS as f64).ceil() as usize
    } else {
        0
    };
    let step_len = STEP_SECS * EEG_RATE as usize;
    let mut chans: HashMap<&'static str, Chan> = SENSORS
        .iter()
        .map(|c| {
            (
                *c,
                Chan {
                    cursor: Cursor::new(EEG_RATE),
                    buf: Vec::new(),
                },
            )
        })
        .collect();
    let mut acc = SeriesAcc::default();
    let (Some(first_ts), true) = (idx.first_ts, n_steps > 0) else {
        return Ok(acc.finish(0, None));
    };
    let mut reader =
        BufReader::new(File::open(path).map_err(|e| format!("Failed to read recording: {e}"))?);
    let mut buf = Vec::new();
    let mut done = 0usize;
    loop {
        buf.clear();
        if reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        let text = buf.trim_ascii();
        let Ok(probe) = serde_json::from_slice::<Probe>(text) else {
            continue;
        };
        if probe.kind.as_deref() != Some("sample") {
            continue;
        }
        let (Some(ts), Some(ch)) = (probe.timestamp, probe.channel.as_deref()) else {
            continue;
        };
        if !chans.contains_key(ch) {
            continue;
        }
        let rel = (ts - first_ts) / 1000.0;
        while done < n_steps && rel >= ((done + 1) * STEP_SECS) as f64 + 0.5 {
            acc.process(&mut chans);
            done += 1;
        }
        let Ok(line) = serde_json::from_slice::<SampleLine>(text) else {
            continue;
        };
        let Some(chan) = chans.get_mut(ch) else {
            continue;
        };
        let at = chan.cursor.place(rel, line.samples.len());
        let base = (done * step_len) as i64;
        for (i, s) in line.samples.into_iter().enumerate() {
            let off = at + i as i64 - base;
            if off >= 0 {
                let off = off as usize;
                if chan.buf.len() <= off {
                    chan.buf.resize(off + 1, f64::NAN);
                }
                chan.buf[off] = s;
            }
        }
    }
    while done < n_steps {
        acc.process(&mut chans);
        done += 1;
    }
    Ok(acc.finish(n_steps, timeline))
}

// ── Sidecar cache ───────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Sidecar {
    version: u32,
    source_len: u64,
    /// `timeline` (recorded training timeline) or `computed`.
    focus_source: String,
    samples: SessionSamples,
}

fn sidecar_path(replay: &Path) -> PathBuf {
    let mut name = replay.as_os_str().to_owned();
    name.push(".bands.json");
    PathBuf::from(name)
}

fn read_sidecar(replay: &Path, len: u64) -> Option<Sidecar> {
    let bytes = std::fs::read(sidecar_path(replay)).ok()?;
    let sc: Sidecar = serde_json::from_slice(&bytes).ok()?;
    (sc.version == SIDECAR_VERSION && sc.source_len == len).then_some(sc)
}

/// Temp file in the same directory, then rename over the sidecar. Never touches the replay. A
/// failed rename leaves the temp file; the next attempt overwrites it.
/// One day: no Muse recording is longer.
const MAX_INDEX_SECS: usize = 24 * 60 * 60;

fn write_sidecar(replay: &Path, sc: &Sidecar) -> std::io::Result<()> {
    let target = sidecar_path(replay);
    let mut tmp_name = target.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let json = serde_json::to_vec(sc).map_err(std::io::Error::other)?;
    // Don't follow a planted symlink (or write into anything but a plain file) at the temp path.
    if let Ok(m) = std::fs::symlink_metadata(&tmp) {
        if !m.file_type().is_file() {
            return Err(std::io::Error::other(
                "sidecar temp path is not a regular file",
            ));
        }
    }
    crate::app_paths::write_private(&tmp, json)?;
    std::fs::rename(&tmp, &target)
}

fn find_timeline(stored: &[StoredSession], replay_file: &str) -> Option<Vec<TimelineEntry>> {
    let s = stored.iter().find(|s| s.session.file_path == replay_file)?;
    let entries: Vec<TimelineEntry> =
        serde_json::from_value(s.data.get("neurofeedback")?.get("timeline")?.clone()).ok()?;
    (!entries.is_empty()).then_some(entries)
}

/// The full per-step samples of a recording: from the sidecar when it is current, else computed
/// and cached. `active` is the file being recorded now, which is never cached.
pub(crate) fn samples_for(
    replays: &Path,
    stored: &[StoredSession],
    id: &str,
    active: Option<&str>,
) -> Result<SessionSamples, String> {
    let path = resolve_replay(replays, id)?;
    let file = file_name_of(&path);
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    let timeline = find_timeline(stored, &file);
    let source = if timeline.is_some() {
        "timeline"
    } else {
        "computed"
    };
    if let Some(sc) = read_sidecar(&path, len) {
        if sc.focus_source == source {
            return Ok(sc.samples);
        }
    }
    let idx = get_index(&path)?;
    let origin = idx
        .header
        .as_ref()
        .and_then(|h| h.start_ts)
        .or(idx.first_ts)
        .unwrap_or(0.0);
    let samples = compute_samples(&path, &idx, timeline.as_deref().map(|t| (t, origin)))?;
    if active != Some(file.as_str()) {
        let _ = write_sidecar(
            &path,
            &Sidecar {
                version: SIDECAR_VERSION,
                source_len: len,
                focus_source: source.to_string(),
                samples: samples.clone(),
            },
        );
    }
    Ok(samples)
}

/// For `list_sessions`: the cached samples of the replay a session names, only if a current
/// sidecar already exists. Never computes and never writes.
pub fn cached_session_samples(replays: &Path, replay_file: &str) -> Option<SessionSamples> {
    validate_file_name(replay_file, &["ndjson"]).ok()?;
    let stem = replay_file.strip_suffix(".ndjson")?;
    let path = resolve_replay(replays, stem).ok()?;
    let len = std::fs::metadata(&path).ok()?.len();
    read_sidecar(&path, len).map(|s| s.samples)
}

// ── Listing ─────────────────────────────────────────────────────

/// `expiring` when older than 90 days (strictly). Computed only; nothing acts on it.
pub fn retention_for(started_ms: f64, now_ms: f64) -> &'static str {
    if now_ms - started_ms > RETENTION_DAYS as f64 * DAY_MS {
        "expiring"
    } else {
        "kept"
    }
}

fn iso(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

fn parse_start(s: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis() as f64)
}

/// `eeg_YYYYMMDD_HHMMSS` as UTC ms (the recorder names files in UTC).
fn stem_start(stem: &str) -> Option<f64> {
    let t = stem.strip_prefix("eeg_")?;
    chrono::NaiveDateTime::parse_from_str(t, "%Y%m%d_%H%M%S")
        .ok()
        .map(|d| d.and_utc().timestamp_millis() as f64)
}

/// Every replay (joined to its session by `file_path`) and every muse session without a replay.
/// `now_ms` is injected for the retention test; `active` is the file being recorded.
pub(crate) fn list_recordings_in(
    replays: &Path,
    stored: &[StoredSession],
    now_ms: f64,
    active: Option<&str>,
) -> Vec<RecordingListItem> {
    let mut items: Vec<(f64, RecordingListItem)> = Vec::new();
    let mut seen_files: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(replays) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if validate_file_name(&name, &["ndjson"]).is_err() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let stem = name.trim_end_matches(".ndjson").to_string();
            let path = entry.path();
            let (header, trailer) = peek_replay(&path);
            let session = stored
                .iter()
                .find(|s| s.session.file_path == name && s.session.device_type == "muse");
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            let header_start = header.as_ref().and_then(|h| h.start_ts);
            let started = session
                .and_then(|s| parse_start(&s.session.start))
                .or(header_start)
                .or_else(|| stem_start(&stem))
                .unwrap_or(mtime_ms);
            let is_active = active == Some(name.as_str());
            let duration = match (&trailer, header_start) {
                (
                    Some(TrailerInfo {
                        end_ts: Some(end), ..
                    }),
                    Some(start),
                ) => ((end - start) / 1000.0).max(0.0),
                _ => session
                    .map(|s| s.session.duration_secs as f64)
                    .or_else(|| {
                        if is_active {
                            header_start.map(|s| ((now_ms - s) / 1000.0).max(0.0))
                        } else {
                            None
                        }
                    })
                    .or_else(|| get_index(&path).ok().and_then(|i| i.duration_secs()))
                    .unwrap_or(0.0),
            };
            let channels = header
                .as_ref()
                .map(|h| h.channels.clone())
                .unwrap_or_else(|| SENSORS.iter().map(|c| c.to_string()).collect())
                .into_iter()
                .filter(|c| KNOWN_CHANNELS.contains(&c.as_str()))
                .collect();
            seen_files.push(name.clone());
            items.push((
                started,
                RecordingListItem {
                    id: stem,
                    session_id: session.map(|s| s.session.id.clone()),
                    replay_file: Some(name),
                    started_at: iso(started),
                    duration_secs: duration.round() as u64,
                    protocol: session.and_then(protocol_of),
                    channels,
                    aux: trailer.and_then(|t| t.aux),
                    has_heart: header.as_ref().is_some_and(|h| !h.ppg.is_empty()),
                    has_motion: header.as_ref().is_some_and(|h| !h.motion.is_empty()),
                    avg_focus: session.and_then(|s| s.session.summary.avg_focus),
                    avg_calm: session.and_then(|s| s.session.summary.avg_relaxation),
                    time_in_target: session.and_then(|s| s.session.summary.time_in_target_pct),
                    calm_pct: session.and_then(|s| s.session.summary.calm_pct),
                    raw: if is_active { "recording" } else { "available" }.to_string(),
                    size_bytes: Some(meta.len()),
                    retention: retention_for(started, now_ms).to_string(),
                },
            ));
        }
    }
    for s in stored.iter().filter(|s| s.session.device_type == "muse") {
        if seen_files.contains(&s.session.file_path) {
            continue;
        }
        let started = parse_start(&s.session.start).unwrap_or(0.0);
        let named = !s.session.file_path.is_empty();
        items.push((
            started,
            RecordingListItem {
                id: s.session.id.clone(),
                session_id: Some(s.session.id.clone()),
                replay_file: named.then(|| s.session.file_path.clone()),
                started_at: s.session.start.clone(),
                duration_secs: s.session.duration_secs,
                protocol: protocol_of(s),
                channels: SENSORS.iter().map(|c| c.to_string()).collect(),
                aux: None,
                has_heart: false,
                has_motion: false,
                avg_focus: s.session.summary.avg_focus,
                avg_calm: s.session.summary.avg_relaxation,
                time_in_target: s.session.summary.time_in_target_pct,
                calm_pct: s.session.summary.calm_pct,
                raw: if named { "missing" } else { "summary-only" }.to_string(),
                size_bytes: None,
                retention: retention_for(started, now_ms).to_string(),
            },
        ));
    }
    items.sort_by(|a, b| b.0.total_cmp(&a.0));
    items.into_iter().map(|(_, i)| i).collect()
}

fn protocol_of(s: &StoredSession) -> Option<String> {
    s.data
        .get("neurofeedback")?
        .get("protocol_name")?
        .as_str()
        .map(str::to_string)
}

// ── Commands (read-only) ────────────────────────────────────────

fn to_series_view(s: SessionSamples) -> SessionSeries {
    SessionSeries {
        step_secs: s.step_secs,
        bands: s.bands,
        focus: s.focus,
        calm: s.calm,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_recordings() -> Result<Vec<RecordingListItem>, String> {
    let active = super::replay::active_recording_name().await;
    blocking(JoinErr::Prefix("Task failed: "), move || {
        let stored = sessions::read_stored_sessions_in(&sessions::sessions_dir());
        let now = chrono::Utc::now().timestamp_millis() as f64;
        Ok(list_recordings_in(
            &super::replay::replays_dir(),
            &stored,
            now,
            active.as_deref(),
        ))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn read_recording_window(
    id: String,
    start_secs: f64,
    span_secs: f64,
    points: Option<usize>,
) -> Result<SignalWindow, String> {
    blocking(JoinErr::Prefix("Task failed: "), move || {
        read_recording_window_in(
            &super::replay::replays_dir(),
            &id,
            start_secs,
            span_secs,
            points,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn read_body_window(
    id: String,
    start_secs: f64,
    span_secs: f64,
) -> Result<BodyWindow, String> {
    blocking(JoinErr::Prefix("Task failed: "), move || {
        read_body_window_in(&super::replay::replays_dir(), &id, start_secs, span_secs)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn recording_series(id: String) -> Result<SessionSeries, String> {
    let active = super::replay::active_recording_name().await;
    blocking(JoinErr::Prefix("Task failed: "), move || {
        let stored = sessions::read_stored_sessions_in(&sessions::sessions_dir());
        samples_for(
            &super::replay::replays_dir(),
            &stored,
            &id,
            active.as_deref(),
        )
        .map(to_series_view)
    })
    .await
}

/// The per-step samples of a saved session's recording, or `None` when it has no replay.
#[tauri::command]
#[specta::specta]
pub async fn get_session_samples(session_id: String) -> Result<Option<SessionSamples>, String> {
    validate_name(&session_id)?;
    let active = super::replay::active_recording_name().await;
    blocking(JoinErr::Prefix("Task failed: "), move || {
        let stored = sessions::read_stored_sessions_in(&sessions::sessions_dir());
        let Some(s) = stored.iter().find(|s| s.session.id == session_id) else {
            return Ok(None);
        };
        let file = s.session.file_path.clone();
        if validate_file_name(&file, &["ndjson"]).is_err() {
            return Ok(None);
        }
        let stem = file.trim_end_matches(".ndjson");
        match samples_for(
            &super::replay::replays_dir(),
            &stored,
            stem,
            active.as_deref(),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e == "Recording not found" => Ok(None),
            Err(e) => Err(e),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const DAY: f64 = 86_400_000.0;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test/fixtures/muse")
            .join(name)
    }

    /// A fresh temp folder holding copies of the named fixtures.
    fn workdir(files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nexus-rec-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        for f in files {
            fs::copy(fixture(f), dir.join(f)).unwrap();
        }
        dir
    }

    fn stored_from(dir: &Path) -> Vec<StoredSession> {
        sessions::read_stored_sessions_in(dir)
    }

    #[test]
    fn a_corrupt_far_future_timestamp_does_not_blow_up_the_index() {
        let dir = workdir(&["format1_4ch.ndjson"]);
        let p = dir.join("format1_4ch.ndjson");
        let mut text = fs::read_to_string(&p).unwrap();
        // ~3,000 years after the start: before the cap this pushed ~10^11 offsets.
        text.push_str("{\"kind\":\"sample\",\"dt_ms\":0,\"timestamp\":1e14,\"channel\":\"TP9\",\"samples\":[0,0,0,0,0,0,0,0,0,0,0,0]}\n");
        fs::write(&p, text).unwrap();
        let idx = parse_replay(&p).unwrap();
        assert!(
            idx.offsets.len() <= MAX_INDEX_SECS + 1,
            "{}",
            idx.offsets.len()
        );
        assert!(
            idx.offsets.len() < 3600,
            "the corrupt line is left out of the time index"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_sidecar_temp_path_never_follows_a_planted_symlink() {
        let dir = workdir(&["format1_4ch.ndjson"]);
        let replay = dir.join("format1_4ch.ndjson");
        let victim = dir.join("victim.txt");
        fs::write(&victim, "keep me").unwrap();
        let mut tmp = sidecar_path(&replay).into_os_string();
        tmp.push(".tmp");
        std::os::unix::fs::symlink(&victim, PathBuf::from(tmp)).unwrap();
        // Computing the samples tries to cache them; the planted temp symlink must make that fail.
        samples_for(&dir, &[], "format1_4ch", None).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
        assert!(!sidecar_path(&replay).exists());
    }

    fn nulls(v: &Series) -> usize {
        v.iter().filter(|x| x.is_none()).count()
    }

    #[test]
    fn format1_parses_as_four_channels_without_trailer() {
        let idx = parse_replay(&fixture("format1_4ch.ndjson")).unwrap();
        let h = idx.header.as_ref().unwrap();
        assert_eq!(h.format, 1);
        assert_eq!(h.channels, ["TP9", "AF7", "AF8", "TP10"]);
        assert!(h.ppg.is_empty() && h.motion.is_empty());
        assert!(idx.trailer.is_none() && !idx.has_ppg && !idx.has_motion);
        let d = idx.duration_secs().unwrap();
        assert!((5.9..6.1).contains(&d), "duration {d}");
        // One byte offset per whole second, the first being the first data line.
        assert_eq!(idx.offsets.len(), 6);
        let header_len = fs::read_to_string(fixture("format1_4ch.ndjson"))
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .len() as u64
            + 1;
        assert_eq!(idx.offsets[0], header_len);
        assert!(idx.offsets.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn format2_parses_header_streams_and_trailer() {
        let idx = parse_replay(&fixture("format2_aux_on.ndjson")).unwrap();
        let h = idx.header.as_ref().unwrap();
        assert_eq!(h.format, 2);
        assert_eq!(h.channels.len(), 5);
        assert_eq!(h.ppg, ["infrared"]);
        assert_eq!(h.motion, ["gyroscope"]);
        assert!(idx.has_ppg && idx.has_motion);
        let aux = idx.trailer.unwrap().aux.unwrap();
        assert!(aux.detected);
        assert_eq!(aux.placement, "occipital");
        assert_eq!(aux.detected_by, "signal-quality");
        assert!(idx.channels.contains(&"AUX".to_string()));
    }

    #[test]
    fn truncated_and_header_only_files_parse() {
        let t = parse_replay(&fixture("truncated.ndjson")).unwrap();
        assert!(t.cut_off, "half line at the end is dropped, not fatal");
        assert!(t.trailer.is_none());
        assert!(t.duration_secs().unwrap() > 2.0);
        let h = parse_replay(&fixture("header_only.ndjson")).unwrap();
        assert!(h.header.is_some() && h.trailer.is_some());
        assert_eq!(h.duration_secs(), None);
        assert!(h.offsets.is_empty());
        // A header-only file reads as an empty window, not an error.
        let dir = workdir(&["header_only.ndjson"]);
        let w = read_recording_window_in(&dir, "header_only", 0.0, 2.0, None).unwrap();
        assert!(w.lanes.iter().all(|l| l.values.iter().all(|v| v.is_none())));
    }

    #[test]
    fn legacy_session_with_a_data_array_still_parses() {
        let dir = workdir(&[]);
        fs::copy(
            fixture("legacy_session.json"),
            dir.join("muse_legacy1.json"),
        )
        .unwrap();
        let s = stored_from(&dir);
        assert_eq!(s.len(), 1);
        assert!(s[0].data.is_array());
        // Old 0-1 scores are scaled on read; the file itself is untouched.
        assert!((s[0].session.summary.avg_focus.unwrap() - 50.0).abs() < 1e-9);
        let on_disk: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("muse_legacy1.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk["session"]["summary"]["avg_focus"], 0.5);
        let items = list_recordings_in(&dir.join("replays"), &s, 1.8e12, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].raw, "summary-only");
        assert_eq!(items[0].replay_file, None);
    }

    #[test]
    fn window_reads_all_lanes_with_contact() {
        let dir = workdir(&["format1_4ch.ndjson"]);
        let w = read_recording_window_in(&dir, "format1_4ch", 1.0, 2.0, Some(4096)).unwrap();
        assert_eq!(w.sample_rate, 256);
        assert_eq!(w.lanes.len(), 4);
        for lane in &w.lanes {
            assert_eq!(lane.values.len(), 512);
            assert_eq!(lane.contact, "good", "{}", lane.channel);
            assert!(nulls(&lane.values) <= 12, "{} nulls", lane.channel);
        }
        // Bucketed down: 900 by default only when there are more samples.
        let long = read_recording_window_in(&dir, "format1_4ch", 0.0, 5.0, None).unwrap();
        assert_eq!(long.lanes[0].values.len(), 900);
        // High-passed: the DC offset is gone.
        let mean: f64 = w.lanes[0].values.iter().flatten().sum::<f64>() / 500.0;
        assert!(mean.abs() < 5.0);
    }

    #[test]
    fn a_gap_over_half_a_second_is_null_not_joined() {
        let dir = workdir(&["gap_tp9.ndjson"]);
        let w = read_recording_window_in(&dir, "gap_tp9", 1.0, 3.0, Some(4096)).unwrap();
        let tp9 = &w.lanes[0];
        let n = nulls(&tp9.values);
        assert!((330..=420).contains(&n), "TP9 nulls {n}");
        assert!(
            nulls(&w.lanes[1].values) <= 24,
            "other channels have no gap"
        );
    }

    #[test]
    fn window_limits_and_bad_ids_are_rejected() {
        let dir = workdir(&["format1_4ch.ndjson"]);
        assert!(read_recording_window_in(&dir, "format1_4ch", 0.0, 61.0, None).is_err());
        assert!(read_recording_window_in(&dir, "format1_4ch", -1.0, 5.0, None).is_err());
        assert!(read_recording_window_in(&dir, "format1_4ch", 0.0, 0.0, None).is_err());
        assert!(read_body_window_in(&dir, "format1_4ch", 0.0, 61.0).is_err());
        for bad in ["../format1_4ch", "a/b", "format1_4ch.ndjson", "", "nope"] {
            assert!(
                read_recording_window_in(&dir, bad, 0.0, 1.0, None).is_err(),
                "{bad:?}"
            );
        }
        // The missing directory is not created by a read.
        let missing = dir.join("replays-not-here");
        assert!(resolve_replay(&missing, "x").is_err());
        assert!(!missing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_replays_folder_is_rejected() {
        let dir = workdir(&[]);
        let outside = workdir(&["format1_4ch.ndjson"]);
        std::os::unix::fs::symlink(outside.join("format1_4ch.ndjson"), dir.join("evil.ndjson"))
            .unwrap();
        assert!(resolve_replay(&dir, "evil").is_err());
    }

    #[test]
    fn body_window_has_heart_and_head_movement() {
        let dir = workdir(&["format2_aux_on.ndjson", "format1_4ch.ndjson"]);
        let b = read_body_window_in(&dir, "format2_aux_on", 0.0, 6.0).unwrap();
        let heart = b.heart.unwrap();
        assert!(heart.estimate && heart.ppg_rate == 64);
        let bpm = heart.bpm.expect("a 72 bpm pulse is found");
        assert!((bpm - 72.0).abs() <= 3.0, "bpm {bpm}");
        let motion = b.motion.unwrap();
        assert_eq!(motion.rate, 52);
        assert_eq!(motion.events.len(), 1, "{:?}", motion.events);
        assert!((1.5..4.5).contains(&motion.events[0].at_secs));
        // Format 1 has neither.
        let old = read_body_window_in(&dir, "format1_4ch", 0.0, 6.0).unwrap();
        assert!(old.heart.is_none() && old.motion.is_none());
    }

    #[test]
    fn aux_on_reads_good_and_aux_off_reads_poor() {
        let dir = workdir(&["format2_aux_on.ndjson", "format2_aux_off.ndjson"]);
        let contact = |id: &str| {
            let w = read_recording_window_in(&dir, id, 1.0, 4.0, None).unwrap();
            w.lanes
                .iter()
                .find(|l| l.channel == "AUX")
                .unwrap()
                .contact
                .clone()
        };
        assert_eq!(contact("format2_aux_on"), "good");
        assert_eq!(contact("format2_aux_off"), "poor");
        let items = list_recordings_in(&dir, &[], 1.9e12, None);
        let on = items.iter().find(|i| i.id == "format2_aux_on").unwrap();
        let off = items.iter().find(|i| i.id == "format2_aux_off").unwrap();
        assert!(on.aux.as_ref().unwrap().detected);
        assert!(!off.aux.as_ref().unwrap().detected);
        assert!(on.has_heart && on.has_motion && !off.has_heart);
        assert_eq!(on.channels.len(), 5);
    }

    #[test]
    fn series_is_cached_in_a_sidecar_and_skips_the_open_recording() {
        let dir = workdir(&["format2_aux_on.ndjson", "format1_4ch.ndjson"]);
        let a = samples_for(&dir, &[], "format2_aux_on", None).unwrap();
        assert_eq!(a.step_secs, 5);
        assert_eq!(a.focus.len(), 2);
        let alpha: Vec<f64> = a.bands.alpha.iter().flatten().copied().collect();
        assert!(!alpha.is_empty() && alpha[0] > 40.0, "alpha {alpha:?}");
        let contact = a.contact.as_ref().unwrap();
        assert!(contact["TP9"] > 90.0, "{contact:?}");
        let sidecar = dir.join("format2_aux_on.ndjson.bands.json");
        assert!(sidecar.is_file());
        assert!(!dir.join("format2_aux_on.ndjson.bands.json.tmp").exists());
        // Second read comes from the sidecar and matches.
        let b = samples_for(&dir, &[], "format2_aux_on", None).unwrap();
        // JSON text round-trips floats to within the last digit.
        assert_eq!(a.focus, b.focus);
        assert_eq!(a.contact, b.contact);
        for (x, y) in a.bands.alpha.iter().zip(&b.bands.alpha) {
            assert!((x.unwrap_or(0.0) - y.unwrap_or(0.0)).abs() < 1e-9);
        }
        let cached = cached_session_samples(&dir, "format2_aux_on.ndjson").unwrap();
        assert_eq!(cached, b);
        assert_eq!(cached_session_samples(&dir, "format1_4ch.ndjson"), None);
        // The file being recorded gets no sidecar.
        samples_for(&dir, &[], "format1_4ch", Some("format1_4ch.ndjson")).unwrap();
        assert!(!dir.join("format1_4ch.ndjson.bands.json").exists());
        // A sidecar for a different length is not trusted.
        let mut sc: serde_json::Value =
            serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        sc["source_len"] = serde_json::json!(1);
        fs::write(&sidecar, sc.to_string()).unwrap();
        assert_eq!(cached_session_samples(&dir, "format2_aux_on.ndjson"), None);
    }

    #[test]
    fn focus_and_calm_come_from_the_timeline_when_the_session_has_one() {
        let dir = workdir(&["format2_aux_on.ndjson"]);
        let sessions = workdir(&[]);
        fs::copy(
            fixture("session_with_timeline.json"),
            sessions.join("muse_tl1.json"),
        )
        .unwrap();
        let computed = samples_for(&dir, &[], "format2_aux_on", None).unwrap();
        let stored = stored_from(&sessions);
        assert_eq!(stored.len(), 1);
        // The cached "computed" sidecar is not reused once a timeline exists.
        let from_timeline = samples_for(&dir, &stored, "format2_aux_on", None).unwrap();
        assert_eq!(from_timeline.focus[1], Some(60.0));
        assert_eq!(from_timeline.calm[0], Some(30.0));
        assert!((from_timeline.focus[0].unwrap() - 140.0 / 3.0).abs() < 1e-9);
        assert_ne!(computed.focus, from_timeline.focus);
        // list_sessions attaches the cached samples for that session.
        let listed = sessions::list_sessions_in(&sessions, &dir);
        assert_eq!(listed[0].samples.as_ref().unwrap().calm[0], Some(30.0));
    }

    #[test]
    fn listing_joins_sessions_and_marks_the_open_recording() {
        let dir = workdir(&["format2_aux_on.ndjson", "format1_4ch.ndjson"]);
        let sdir = workdir(&["session_with_timeline.json"]);
        let stored = stored_from(&sdir);
        let now = 1_760_000_000_000.0 + 10.0 * DAY;
        let items = list_recordings_in(&dir, &stored, now, Some("format1_4ch.ndjson"));
        assert_eq!(items.len(), 2);
        let joined = items.iter().find(|i| i.id == "format2_aux_on").unwrap();
        assert_eq!(joined.session_id.as_deref(), Some("tl1"));
        assert_eq!(joined.protocol.as_deref(), Some("Free session"));
        assert_eq!(
            joined.avg_focus,
            Some(50.0),
            "score_scale 100 is not rescaled"
        );
        assert_eq!(joined.raw, "available");
        assert!(joined.size_bytes.unwrap() > 1000);
        assert_eq!(joined.duration_secs, 6);
        let open = items.iter().find(|i| i.id == "format1_4ch").unwrap();
        assert_eq!(open.raw, "recording");
        assert!(open.session_id.is_none() && open.avg_focus.is_none());
    }

    #[test]
    fn retention_is_computed_at_89_90_91_days_with_an_injected_clock() {
        let start = 1_760_000_000_000.0;
        assert_eq!(retention_for(start, start + 89.0 * DAY), "kept");
        assert_eq!(retention_for(start, start + 90.0 * DAY), "kept");
        assert_eq!(retention_for(start, start + 91.0 * DAY), "expiring");
        let dir = workdir(&["format1_4ch.ndjson"]);
        let at = |days: f64| {
            list_recordings_in(&dir, &[], start + days * DAY, None)[0]
                .retention
                .clone()
        };
        assert_eq!((at(89.0).as_str(), at(91.0).as_str()), ("kept", "expiring"));
    }

    #[cfg(unix)]
    #[test]
    fn readers_leave_a_read_only_replay_byte_for_byte_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = workdir(&["format2_aux_on.ndjson"]);
        let replay = dir.join("format2_aux_on.ndjson");
        fs::set_permissions(&replay, fs::Permissions::from_mode(0o444)).unwrap();
        let before_bytes = fs::read(&replay).unwrap();
        let before = fs::metadata(&replay).unwrap();
        let (len, mtime) = (before.len(), before.modified().unwrap());

        let _ = list_recordings_in(&dir, &[], 1.9e12, None);
        read_recording_window_in(&dir, "format2_aux_on", 0.0, 5.0, None).unwrap();
        read_body_window_in(&dir, "format2_aux_on", 0.0, 5.0).unwrap();
        samples_for(&dir, &[], "format2_aux_on", None).unwrap();
        let _ = cached_session_samples(&dir, "format2_aux_on.ndjson");

        let after = fs::metadata(&replay).unwrap();
        assert_eq!(fs::read(&replay).unwrap(), before_bytes);
        assert_eq!((after.len(), after.modified().unwrap()), (len, mtime));
        assert_eq!(after.permissions().mode() & 0o777, 0o444);
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["format2_aux_on.ndjson", "format2_aux_on.ndjson.bands.json"],
            "only the sidecar is ever added"
        );
    }

    #[test]
    fn the_index_cache_holds_at_most_four_files() {
        for i in 0..6 {
            let dir = workdir(&["format1_4ch.ndjson"]);
            let _ = i;
            get_index(&dir.join("format1_4ch.ndjson")).unwrap();
        }
        assert!(INDEX_CACHE.lock().unwrap().len() <= INDEX_CACHE_SIZE);
    }
}
