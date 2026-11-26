use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use super::muse::{ChannelContinuity, EegSample, MotionSample, PpgSample, SeqTracker, SeqVerdict};

// ── Data directory ──────────────────────────────────────────────

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

pub(crate) fn replays_dir() -> PathBuf {
    nexus_data_dir().join("replays")
}

// ── Global state ────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref REPLAY_ACTIVE: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref RECORDING_ENABLED: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref RECORDING_FILE: Arc<Mutex<Option<BufWriter<File>>>> = Arc::new(Mutex::new(None));
    /// Monotonic packet index for recording — increments per sample line written
    static ref RECORDING_PREV_TS: Arc<Mutex<Option<f64>>> = Arc::new(Mutex::new(None));
    /// Header-to-be, trailer bookkeeping and the aux verdict for the open recording. Always
    /// locked after RECORDING_FILE.
    static ref RECORDING_META: Arc<Mutex<RecordingMeta>> = Arc::new(Mutex::new(RecordingMeta::default()));
}

// ── NDJSON Schema ───────────────────────────────────────────────

/// Replay format written by this build. Files without a `format` field are format 1: four EEG
/// channels, no PPG, no motion, no trailer.
pub const REPLAY_FORMAT: u8 = 2;

/// The optional streams the headset actually subscribed, which decide what the header lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamSet {
    /// The 5th (back of head) electrode, `AUX`. Never `AUX_L`.
    pub aux: bool,
    /// Subset of `ambient`, `infrared`, `red`.
    pub ppg: Vec<String>,
    /// Subset of `accelerometer`, `gyroscope`.
    pub motion: Vec<String>,
}

#[derive(Serialize)]
struct ReplayHeader {
    kind: &'static str, // "header"
    format: u8,
    sample_rate: u16,
    channels: Vec<String>,
    ppg: Vec<String>,
    motion: Vec<String>,
    start_ts: f64,
    session_id: String,
    // Reproducibility (decision 0057). Additive: readers parse JSON values, so older readers ignore these.
    /// `live`, `replay` or `demo`: where the recorded samples came from.
    data_source: &'static str,
    /// `sample_rate` is the headset's nominal rate, not a measurement; the trailer has the measured rate.
    sample_rate_basis: &'static str,
    /// `host_receive_ms_per_packet`: one host-clock stamp per 12-sample packet, taken when it arrived.
    timestamp_basis: &'static str,
    samples_per_packet: u8,
    /// How raw counts became µV: 12-bit, 0.48828125 µV per count around 2048.
    eeg_decoder: &'static str,
    app_version: &'static str,
}

/// What start_eeg_recording knows; written as the header with the first line (or on stop).
#[derive(Debug, Clone)]
struct PendingHeader {
    start_ts: f64,
    session_id: String,
}

pub(crate) const EEG_DECODER: &str = "muse_eeg_12bit_0.48828125uV_v1";

/// Per channel, what the recording actually holds: the basis for its measured sample rate.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct RecordedChannel {
    pub channel: String,
    pub packets: u64,
    pub samples: u64,
    pub first_ts: f64,
    pub last_ts: f64,
}

/// Trailer statistics for the open recording (decision 0057).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RecordingStats {
    pub data_source: Option<&'static str>,
    pub channels: Vec<RecordedChannel>,
    pub continuity: SeqTracker,
    pub write_errors: u64,
    /// Samples from another source than the recording's first (e.g. demo during a live recording): never written.
    pub other_source_dropped: u64,
}

impl RecordingStats {
    /// Whether a sample may be written: its source must match the recording's, and a repeated packet is
    /// never written twice. Counts what it sees.
    pub(crate) fn admit(&mut self, sample: &EegSample) -> bool {
        match self.data_source {
            None => self.data_source = Some(sample.source),
            Some(s) if s != sample.source => {
                self.other_source_dropped += 1;
                return false;
            }
            _ => {}
        }
        if let Some(seq) = sample.sequence {
            if self.continuity.observe(&sample.channel, seq) == SeqVerdict::Duplicate {
                return false;
            }
        }
        let c = match self
            .channels
            .iter()
            .position(|c| c.channel == sample.channel)
        {
            Some(i) => &mut self.channels[i],
            None => {
                self.channels.push(RecordedChannel {
                    channel: sample.channel.clone(),
                    first_ts: sample.timestamp,
                    ..Default::default()
                });
                self.channels.last_mut().expect("just pushed")
            }
        };
        c.packets += 1;
        c.samples += sample.samples.len() as u64;
        c.last_ts = sample.timestamp;
        true
    }

    /// Samples per second between the first and last packet's arrival, from what was written. The last
    /// packet's samples are not counted (its arrival time marks the end of the span). `None` under 2 s.
    pub(crate) fn measured_rate(c: &RecordedChannel) -> Option<f64> {
        let span = (c.last_ts - c.first_ts) / 1000.0;
        (span >= 2.0 && c.packets >= 2).then(|| ((c.samples - c.samples / c.packets) as f64) / span)
    }
}

#[derive(Serialize)]
struct TrailerChannel<'a> {
    channel: &'a str,
    packets: u64,
    samples: u64,
    /// Measured from the recording; `null` when it is too short to say.
    measured_rate_hz: Option<f64>,
    /// Packets the headset counter says never arrived (lost before this app saw them).
    missed_packets: u64,
    duplicates_dropped: u64,
    counter_resets: u64,
}

#[derive(Serialize)]
struct TrailerStats<'a> {
    channels: Vec<TrailerChannel<'a>>,
    write_errors: u64,
    other_source_dropped: u64,
}

fn trailer_stats(stats: &RecordingStats) -> TrailerStats<'_> {
    let cont: Vec<ChannelContinuity> = stats.continuity.counts();
    TrailerStats {
        channels: stats
            .channels
            .iter()
            .map(|c| {
                let k = cont.iter().find(|x| x.channel == c.channel);
                TrailerChannel {
                    channel: &c.channel,
                    packets: c.packets,
                    samples: c.samples,
                    measured_rate_hz: RecordingStats::measured_rate(c)
                        .map(|r| (r * 100.0).round() / 100.0),
                    missed_packets: k.map_or(0, |k| k.missed_packets),
                    duplicates_dropped: k.map_or(0, |k| k.duplicates_dropped),
                    counter_resets: k.map_or(0, |k| k.counter_resets),
                }
            })
            .collect(),
        write_errors: stats.write_errors,
        other_source_dropped: stats.other_source_dropped,
    }
}

/// The back-of-head electrode verdict that goes in the trailer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AuxInfo {
    pub detected: bool,
    pub placement: String,
    #[serde(rename = "detectedBy")]
    pub detected_by: String,
    #[serde(
        rename = "detectedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub detected_at: Option<String>,
}

#[derive(Default)]
struct RecordingMeta {
    pending_header: Option<PendingHeader>,
    trailer_written: bool,
    aux: Option<AuxInfo>,
    /// File name of the open recording (readers must not write a sidecar for it).
    name: Option<String>,
    stats: RecordingStats,
}

#[derive(Serialize)]
struct ReplayTrailer<'a> {
    kind: &'static str, // "trailer"
    end_ts: f64,
    aux: Option<&'a AuxInfo>,
    stats: TrailerStats<'a>,
}

#[derive(Serialize)]
struct ReplayPpgLine<'a> {
    kind: &'static str, // "ppg"
    source: &'a str,
    timestamp: f64,
    samples: &'a [u32],
}

#[derive(Serialize)]
struct ReplayMotionLine<'a> {
    kind: &'static str, // "motion"
    sensor: &'a str,
    timestamp: f64,
    samples: &'a [[f64; 3]],
}

fn header_json(
    pending: &PendingHeader,
    streams: &StreamSet,
    data_source: &'static str,
) -> serde_json::Result<String> {
    let mut channels: Vec<String> = ["TP9", "AF7", "AF8", "TP10"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    if streams.aux {
        channels.push("AUX".to_string());
    }
    let sorted = |v: &[String]| {
        let mut v = v.to_vec();
        v.sort();
        v.dedup();
        v
    };
    serde_json::to_string(&ReplayHeader {
        kind: "header",
        format: REPLAY_FORMAT,
        sample_rate: 256,
        channels,
        ppg: sorted(&streams.ppg),
        motion: sorted(&streams.motion),
        start_ts: pending.start_ts,
        session_id: pending.session_id.clone(),
        data_source,
        sample_rate_basis: "nominal",
        timestamp_basis: "host_receive_ms_per_packet",
        samples_per_packet: 12,
        eeg_decoder: EEG_DECODER,
        app_version: env!("CARGO_PKG_VERSION"),
    })
}

/// Writes the pending header (once), then `line`. The header is lazy so it lists only the
/// streams that had subscribed by the time data arrived.
fn write_recorded_line<W: Write>(
    w: &mut W,
    meta: &mut RecordingMeta,
    streams: &StreamSet,
    line: &str,
) -> std::io::Result<()> {
    write_pending_header(w, meta, streams)?;
    writeln!(w, "{line}")
}

fn write_pending_header<W: Write>(
    w: &mut W,
    meta: &mut RecordingMeta,
    streams: &StreamSet,
) -> std::io::Result<()> {
    if let Some(pending) = meta.pending_header.as_ref() {
        let json = header_json(pending, streams, meta.stats.data_source.unwrap_or("live"))
            .map_err(std::io::Error::other)?;
        writeln!(w, "{json}")?;
        meta.pending_header = None;
    }
    Ok(())
}

/// Header if still missing, the trailer (once), then flush. Safe to call again after a failure.
fn write_trailer_and_flush<W: Write>(
    w: &mut BufWriter<W>,
    meta: &mut RecordingMeta,
    streams: &StreamSet,
    end_ts: f64,
) -> std::io::Result<()> {
    write_pending_header(w, meta, streams)?;
    if !meta.trailer_written {
        let json = serde_json::to_string(&ReplayTrailer {
            kind: "trailer",
            end_ts,
            aux: meta.aux.as_ref(),
            stats: trailer_stats(&meta.stats),
        })
        .map_err(std::io::Error::other)?;
        writeln!(w, "{json}")?;
        meta.trailer_written = true;
    }
    w.flush()
}

#[derive(Serialize, Deserialize)]
struct ReplaySampleLine {
    kind: String, // "sample"
    dt_ms: f64,   // delta from previous sample (0.0 for first)
    timestamp: f64,
    channel: String,
    samples: Vec<f64>,
    /// The headset's packet counter (decision 0057); absent in older files and for demo data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seq: Option<u16>,
}

#[derive(Serialize, Clone, specta::Type)]
pub struct ReplayFileInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub modified: String,
    pub duration_secs: Option<f64>,
}

// ── BLE state payload with data_source ──────────────────────────

#[derive(Serialize, Clone)]
struct BleStatePayload {
    state: String,
    data_source: String, // "live" | "replay" | "demo"
}

fn emit_streaming(app: &AppHandle, data_source: &str) {
    let _ = app.emit(
        "muse-ble-state",
        BleStatePayload {
            state: "streaming".to_string(),
            data_source: data_source.to_string(),
        },
    );
}

// ── Recording (called from muse.rs emit path) ──────────────────

/// The unused left aux is on the wire but never in a recording.
fn is_recorded_channel(channel: &str) -> bool {
    channel != "AUX_L"
}

/// Called from muse.rs after each muse-eeg emit. Fast path: checks bool, returns immediately if not recording.
/// Records TP9/AF7/AF8/TP10 and the back-of-head `AUX`; the unused left aux (`AUX_L`) is never written.
pub async fn maybe_record_sample(sample: &EegSample) {
    if !is_recorded_channel(&sample.channel) {
        return;
    }
    let enabled = *RECORDING_ENABLED.lock().await;
    if !enabled {
        return;
    }

    let mut file_guard = RECORDING_FILE.lock().await;
    let file = match file_guard.as_mut() {
        Some(f) => f,
        None => return,
    };
    let mut meta = RECORDING_META.lock().await;
    if !meta.stats.admit(sample) {
        return;
    }

    let mut prev_ts = RECORDING_PREV_TS.lock().await;
    let dt_ms = match *prev_ts {
        Some(prev) => (sample.timestamp - prev).max(0.0),
        None => 0.0,
    };
    *prev_ts = Some(sample.timestamp);

    let line = ReplaySampleLine {
        kind: "sample".to_string(),
        dt_ms,
        timestamp: sample.timestamp,
        channel: sample.channel.clone(),
        samples: sample.samples.clone(),
        seq: sample.sequence,
    };

    match serde_json::to_string(&line) {
        Ok(json) => {
            if write_recorded_line(file, &mut meta, &super::muse::subscribed_streams(), &json)
                .is_err()
            {
                meta.stats.write_errors += 1;
            }
        }
        Err(_) => meta.stats.write_errors += 1,
    }
}

/// PPG packet from the optional stream. No-op unless recording.
pub async fn maybe_record_ppg(sample: &PpgSample) {
    let json = serde_json::to_string(&ReplayPpgLine {
        kind: "ppg",
        source: &sample.channel,
        timestamp: sample.timestamp,
        samples: &sample.samples,
    });
    record_extra_line(json).await;
}

/// Accelerometer or gyroscope packet from the optional streams. No-op unless recording.
pub async fn maybe_record_motion(sample: &MotionSample) {
    let json = serde_json::to_string(&ReplayMotionLine {
        kind: "motion",
        sensor: &sample.sensor,
        timestamp: sample.timestamp,
        samples: &sample.samples,
    });
    record_extra_line(json).await;
}

async fn record_extra_line(json: serde_json::Result<String>) {
    if !*RECORDING_ENABLED.lock().await {
        return;
    }
    let mut file_guard = RECORDING_FILE.lock().await;
    let Some(file) = file_guard.as_mut() else {
        return;
    };
    let mut meta = RECORDING_META.lock().await;
    // PPG and motion only come from the headset: never mixed into a replay or demo recording. When they
    // come first they set the recording's source to live, so a later demo sample is refused, not mislabelled.
    if *meta.stats.data_source.get_or_insert("live") != "live" {
        meta.stats.other_source_dropped += 1;
        return;
    }
    match json {
        Ok(json) => {
            if write_recorded_line(file, &mut meta, &super::muse::subscribed_streams(), &json)
                .is_err()
            {
                meta.stats.write_errors += 1;
            }
        }
        Err(_) => meta.stats.write_errors += 1,
    }
}

lazy_static::lazy_static! {
    /// Which source owns `muse-eeg` right now (decision 0057): claimed and released under one lock, so
    /// live and replay/demo can never feed the app at the same time.
    static ref EEG_SOURCE: Mutex<Option<(&'static str, std::time::Instant)>> = Mutex::new(None);
}

/// A live claim this old with no live stream running is left over from a start that failed.
const STALE_LIVE_CLAIM: Duration = Duration::from_secs(30);

/// Pure rule for [`claim_source`]: `Ok(true)` = already this source's, `Ok(false)` = newly claimed.
pub(crate) fn claim_rule(
    current: Option<&'static str>,
    want: &'static str,
    live_stale: bool,
) -> Result<bool, String> {
    match current {
        None => Ok(false),
        Some(c) if c == want => Ok(true),
        Some("live") if live_stale => Ok(false),
        Some("live") => Err(
            "The headset is streaming. Stop it before playing a replay or the demo.".to_string(),
        ),
        // A replay and the demo replace each other (the old one stops at its next packet).
        Some(_) if want != "live" => Ok(false),
        Some(_) => Err(
            "A replay or demo is running. Stop it before streaming from the headset.".to_string(),
        ),
    }
}

/// Claims `muse-eeg` for `want` (`live`, `replay` or `demo`).
pub(crate) async fn claim_source(want: &'static str) -> Result<bool, String> {
    let mut cur = EEG_SOURCE.lock().await;
    let live_stale = match *cur {
        Some(("live", at)) => {
            at.elapsed() > STALE_LIVE_CLAIM && !super::muse::live_streaming().await
        }
        _ => false,
    };
    let already = claim_rule(cur.map(|(s, _)| s), want, live_stale)?;
    if !already {
        *cur = Some((want, std::time::Instant::now()));
    }
    Ok(already)
}

/// Releases the claim if `src` holds it.
pub(crate) async fn release_source(src: &'static str) {
    let mut cur = EEG_SOURCE.lock().await;
    if cur.is_some_and(|(s, _)| s == src) {
        *cur = None;
    }
}

/// File name of the recording being written right now, if any. Readers use it to leave the
/// open file (and its sidecar) alone.
pub async fn active_recording_name() -> Option<String> {
    if !*RECORDING_ENABLED.lock().await {
        return None;
    }
    RECORDING_META.lock().await.name.clone()
}

/// Records the back-of-head electrode verdict for the open recording; it lands in the trailer.
/// Only touches the open file's state: never another file.
#[tauri::command]
#[specta::specta]
pub async fn mark_recording_aux(aux: AuxInfo) -> Result<(), String> {
    if aux.placement != "occipital" {
        return Err("Invalid aux placement".to_string());
    }
    if aux.detected_by != "signal-quality" && aux.detected_by != "contact" {
        return Err("Invalid aux detector".to_string());
    }
    if !*RECORDING_ENABLED.lock().await {
        return Err("No recording is open".to_string());
    }
    let file_guard = RECORDING_FILE.lock().await;
    if file_guard.is_none() {
        return Err("No recording is open".to_string());
    }
    RECORDING_META.lock().await.aux = Some(aux);
    Ok(())
}

// ── Tauri commands ──────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn start_eeg_recording() -> Result<String, String> {
    let dir = replays_dir();
    crate::app_paths::create_private_dir_all(&dir)
        .map_err(|e| format!("Failed to create replays dir: {e}"))?;

    let now = chrono::Utc::now();
    let filename = format!("eeg_{}.ndjson", now.format("%Y%m%d_%H%M%S"));
    let path = dir.join(&filename);

    // create_new: never reopen (and so never truncate) an existing replay, e.g. after the clock
    // stepped back or a restart within the same second. Muse recordings are sacred.
    let file = crate::app_paths::private_open_options()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "A recording with this start time already exists; try again in a second".to_string()
            } else {
                format!("Failed to create file: {e}")
            }
        })?;
    let writer = BufWriter::new(file);

    // The header is written with the first line, so it lists only what subscribed.
    *RECORDING_META.lock().await = RecordingMeta {
        pending_header: Some(PendingHeader {
            start_ts: now.timestamp_millis() as f64,
            session_id: uuid::Uuid::new_v4().to_string(),
        }),
        trailer_written: false,
        aux: None,
        name: Some(filename.clone()),
        stats: RecordingStats::default(),
    };

    *RECORDING_FILE.lock().await = Some(writer);
    *RECORDING_PREV_TS.lock().await = None;
    *RECORDING_ENABLED.lock().await = true;

    println!("[replay] recording started: {filename}");
    Ok(filename)
}

fn finish_recording<W: Write>(slot: &mut Option<BufWriter<W>>) -> std::io::Result<()> {
    // Retain the writer and buffered samples until flushing succeeds.
    if let Some(writer) = slot.as_mut() {
        writer.flush()?;
    }
    slot.take();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_eeg_recording() -> Result<String, String> {
    *RECORDING_ENABLED.lock().await = false;

    let mut file_guard = RECORDING_FILE.lock().await;
    let mut meta = RECORDING_META.lock().await;
    if let Some(writer) = file_guard.as_mut() {
        write_trailer_and_flush(
            writer,
            &mut meta,
            &super::muse::subscribed_streams(),
            chrono::Utc::now().timestamp_millis() as f64,
        )
        .map_err(|e| e.to_string())?;
    }
    finish_recording(&mut file_guard).map_err(|e| e.to_string())?;
    *meta = RecordingMeta::default();

    *RECORDING_PREV_TS.lock().await = None;

    println!("[replay] recording stopped");
    Ok("stopped".to_string())
}

#[cfg(test)]
mod stats_tests {
    use super::*;

    #[test]
    fn only_one_source_feeds_the_app_at_a_time() {
        assert_eq!(claim_rule(None, "live", false), Ok(false));
        assert_eq!(
            claim_rule(Some("live"), "live", false),
            Ok(true),
            "a second start is a no-op"
        );
        assert!(claim_rule(Some("live"), "replay", false).is_err());
        assert!(claim_rule(Some("live"), "demo", false).is_err());
        assert_eq!(
            claim_rule(Some("live"), "replay", true),
            Ok(false),
            "a stale live claim never locks replay out"
        );
        assert!(claim_rule(Some("replay"), "live", false).is_err());
        assert!(claim_rule(Some("demo"), "live", false).is_err());
        assert_eq!(
            claim_rule(Some("demo"), "replay", false),
            Ok(false),
            "replay and demo replace each other"
        );
    }

    #[test]
    fn a_counter_restart_mid_recording_keeps_writing() {
        let mut st = RecordingStats::default();
        assert!(st.admit(&s("TP9", 0.0, Some(30000), "live")));
        assert!(
            st.admit(&s("TP9", 50.0, Some(5), "live")),
            "headset restarted its counter"
        );
        assert!(st.admit(&s("TP9", 100.0, Some(6), "live")));
        assert_eq!(st.channels[0].packets, 3);
        assert_eq!(st.continuity.counts()[0].counter_resets, 1);
    }

    fn s(channel: &str, ts: f64, seq: Option<u16>, source: &'static str) -> EegSample {
        EegSample {
            timestamp: ts,
            channel: channel.into(),
            samples: vec![0.0; 12],
            sequence: seq,
            source,
        }
    }

    #[test]
    fn a_recording_keeps_one_source_and_never_writes_a_packet_twice() {
        let mut st = RecordingStats::default();
        assert!(st.admit(&s("TP9", 0.0, Some(1), "live")));
        assert!(!st.admit(&s("TP9", 1.0, Some(1), "live")), "repeat");
        assert!(!st.admit(&s("AF7", 1.0, None, "demo")), "other source");
        assert!(
            st.admit(&s("TP9", 2.0, Some(3), "live")),
            "a gap is still new data"
        );
        assert_eq!(st.data_source, Some("live"));
        assert_eq!(st.other_source_dropped, 1);
        let c = st.continuity.counts();
        assert_eq!((c[0].missed_packets, c[0].duplicates_dropped), (1, 1));
        assert_eq!(st.channels[0].packets, 2);
    }

    #[test]
    fn the_measured_rate_comes_from_what_was_written() {
        let mut st = RecordingStats::default();
        // 64 packets of 12 samples, 46.875 ms apart: 256 Hz.
        for i in 0..64u16 {
            st.admit(&s("TP9", f64::from(i) * 46.875, Some(i), "live"));
        }
        let r = RecordingStats::measured_rate(&st.channels[0]).unwrap();
        assert!((r - 256.0).abs() < 0.01, "{r}");
        // Under 2 s: no rate, never the nominal 256 filled in.
        let mut short = RecordingStats::default();
        short.admit(&s("TP9", 0.0, Some(0), "live"));
        short.admit(&s("TP9", 500.0, Some(1), "live"));
        assert_eq!(RecordingStats::measured_rate(&short.channels[0]), None);
    }

    #[test]
    fn a_demo_recording_says_demo_in_its_header() {
        let mut meta = RecordingMeta {
            pending_header: Some(PendingHeader {
                start_ts: 1.0,
                session_id: "x".into(),
            }),
            ..Default::default()
        };
        meta.stats.admit(&s("TP9", 0.0, None, "demo"));
        let mut out = Vec::new();
        write_recorded_line(&mut out, &mut meta, &StreamSet::default(), "{}").unwrap();
        let head: serde_json::Value =
            serde_json::from_str(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap();
        assert_eq!(head["data_source"], "demo");
    }
}

#[cfg(test)]
mod recording_tests {
    use super::*;

    fn pending() -> PendingHeader {
        PendingHeader {
            start_ts: 1000.0,
            session_id: "sid".to_string(),
        }
    }

    fn meta() -> RecordingMeta {
        RecordingMeta {
            pending_header: Some(pending()),
            ..Default::default()
        }
    }

    fn lines(bytes: &[u8]) -> Vec<serde_json::Value> {
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn all_streams() -> StreamSet {
        StreamSet {
            aux: true,
            ppg: vec!["red".into(), "ambient".into(), "infrared".into()],
            motion: vec!["gyroscope".into(), "accelerometer".into()],
        }
    }

    #[test]
    fn header_is_lazy_and_lists_only_what_subscribed() {
        let mut out = Vec::new();
        let mut m = meta();
        // Nothing written until the first line.
        assert!(out.is_empty());
        write_recorded_line(&mut out, &mut m, &all_streams(), r#"{"kind":"ppg"}"#).unwrap();
        write_recorded_line(&mut out, &mut m, &all_streams(), r#"{"kind":"ppg"}"#).unwrap();
        let l = lines(&out);
        assert_eq!(l.len(), 3, "one header, then the two lines");
        assert_eq!(
            l[0],
            serde_json::json!({
                "kind": "header", "format": 2, "sample_rate": 256,
                "channels": ["TP9", "AF7", "AF8", "TP10", "AUX"],
                "ppg": ["ambient", "infrared", "red"],
                "motion": ["accelerometer", "gyroscope"],
                "start_ts": 1000.0, "session_id": "sid",
                // Decision 0057: reproducibility fields (additive).
                "data_source": "live", "sample_rate_basis": "nominal",
                "timestamp_basis": "host_receive_ms_per_packet", "samples_per_packet": 12,
                "eeg_decoder": EEG_DECODER, "app_version": env!("CARGO_PKG_VERSION")
            })
        );

        // Headset with no optional streams: four channels, nothing else.
        let mut out = Vec::new();
        let mut m = meta();
        write_recorded_line(&mut out, &mut m, &StreamSet::default(), "{}").unwrap();
        let h = &lines(&out)[0];
        assert_eq!(
            h["channels"],
            serde_json::json!(["TP9", "AF7", "AF8", "TP10"])
        );
        assert_eq!(h["ppg"], serde_json::json!([]));
        assert_eq!(h["motion"], serde_json::json!([]));
        assert_eq!(h["format"], 2);
    }

    #[test]
    fn stop_writes_header_if_missing_then_trailer_once() {
        let mut w = BufWriter::new(Vec::new());
        let mut m = meta();
        let aux = AuxInfo {
            detected: true,
            placement: "occipital".into(),
            detected_by: "signal-quality".into(),
            detected_at: Some("2026-11-11T00:00:00Z".into()),
        };
        m.aux = Some(aux);
        write_trailer_and_flush(&mut w, &mut m, &StreamSet::default(), 5000.0).unwrap();
        write_trailer_and_flush(&mut w, &mut m, &StreamSet::default(), 9000.0).unwrap();
        let l = lines(w.get_ref());
        assert_eq!(
            l.len(),
            2,
            "header-only recording still gets header + one trailer"
        );
        assert_eq!(l[0]["kind"], "header");
        assert_eq!(
            l[1],
            serde_json::json!({"kind": "trailer", "end_ts": 5000.0, "aux": {
                "detected": true, "placement": "occipital", "detectedBy": "signal-quality",
                "detectedAt": "2026-11-11T00:00:00Z"},
                "stats": {"channels": [], "write_errors": 0, "other_source_dropped": 0}})
        );

        let mut w = BufWriter::new(Vec::new());
        let mut m = meta();
        write_trailer_and_flush(&mut w, &mut m, &StreamSet::default(), 1.0).unwrap();
        assert_eq!(lines(w.get_ref())[1]["aux"], serde_json::Value::Null);
    }

    #[test]
    fn ppg_and_motion_lines_have_the_documented_shape() {
        let ppg = serde_json::to_value(ReplayPpgLine {
            kind: "ppg",
            source: "infrared",
            timestamp: 7.0,
            samples: &[1, 2, 3, 4, 5, 6],
        })
        .unwrap();
        assert_eq!(
            ppg,
            serde_json::json!({"kind":"ppg","source":"infrared","timestamp":7.0,"samples":[1,2,3,4,5,6]})
        );
        let motion = serde_json::to_value(ReplayMotionLine {
            kind: "motion",
            sensor: "gyroscope",
            timestamp: 8.0,
            samples: &[[0.0, 1.0, 2.0], [3.0, 4.0, 5.0], [6.0, 7.0, 8.0]],
        })
        .unwrap();
        assert_eq!(motion["kind"], "motion");
        assert_eq!(motion["sensor"], "gyroscope");
        assert_eq!(motion["samples"][2], serde_json::json!([6.0, 7.0, 8.0]));
        assert_eq!(motion.as_object().unwrap().len(), 4, "no sequence field");
    }

    #[test]
    fn the_left_aux_is_not_recorded() {
        assert!(!is_recorded_channel("AUX_L"));
        for c in ["TP9", "AF7", "AF8", "TP10", "AUX"] {
            assert!(is_recorded_channel(c));
        }
    }

    #[derive(Default)]
    struct RetryWriter {
        fail: bool,
        bytes: Vec<u8>,
    }

    impl Write for RetryWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                return Err(std::io::Error::other("disk unavailable"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn failed_flush_keeps_pending_samples_for_retry() {
        let mut writer = BufWriter::new(RetryWriter {
            fail: true,
            ..Default::default()
        });
        writer.write_all(b"recorded samples").unwrap();
        let mut slot = Some(writer);
        assert!(finish_recording(&mut slot).is_err());
        let retained = slot.as_mut().expect("writer must remain available");
        assert_eq!(retained.buffer(), b"recorded samples");
        retained.get_mut().fail = false;
        retained.flush().unwrap();
        assert_eq!(retained.get_ref().bytes, b"recorded samples");
        finish_recording(&mut slot).unwrap();
        assert!(slot.is_none());
        finish_recording(&mut slot).unwrap();
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_replays() -> Result<Vec<ReplayFileInfo>, String> {
    let dir = replays_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<ReplayFileInfo> = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ndjson") {
            continue;
        }
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let modified = meta
            .modified()
            .ok()
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            })
            .unwrap_or_default();

        // Estimate duration from file: read last sample line's timestamp vs header start_ts
        let duration_secs = estimate_duration(&path);

        files.push(ReplayFileInfo {
            filename: entry.file_name().to_string_lossy().to_string(),
            size_bytes: meta.len(),
            modified,
            duration_secs,
        });
    }

    // Sort by modified descending
    files.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(files)
}

fn estimate_duration(path: &PathBuf) -> Option<f64> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut total_dt = 0.0f64;
    for line in reader.lines() {
        let line = line.ok()?;
        // Quick check without full parse
        if line.contains("\"kind\":\"sample\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(dt) = v.get("dt_ms").and_then(|d| d.as_f64()) {
                    total_dt += dt;
                }
            }
        }
    }
    if total_dt > 0.0 {
        Some(total_dt / 1000.0)
    } else {
        None
    }
}

#[tauri::command]
#[specta::specta]
pub async fn start_replay(app: AppHandle, filename: String, speed: f64) -> Result<(), String> {
    super::path_safety::validate_file_name(&filename, &["ndjson"])?;
    let path = super::path_safety::safe_join(&replays_dir(), &filename)?;
    if !path.is_file() {
        return Err(format!("Replay file not found: {filename}"));
    }
    claim_source("replay").await?;

    // Stop any existing replay/demo
    *REPLAY_ACTIVE.lock().await = false;
    tokio::time::sleep(Duration::from_millis(100)).await;
    *REPLAY_ACTIVE.lock().await = true;

    let speed = if speed <= 0.0 { 1.0 } else { speed };

    println!("[replay] starting replay: {filename} speed={speed}x");

    tokio::spawn(async move {
        replay_task(&app, &path, speed).await;
    });

    Ok(())
}

async fn replay_task(app: &AppHandle, path: &PathBuf, speed: f64) {
    emit_streaming(app, "replay");

    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            println!("[replay] failed to open file: {e}");
            *REPLAY_ACTIVE.lock().await = false;
            release_source("replay").await;
            return;
        }
    };
    let reader = BufReader::new(file);
    let mut packets_emitted: u64 = 0;

    for line in reader.lines() {
        // Check active flag
        if !*REPLAY_ACTIVE.lock().await {
            println!("[replay] stopped by flag");
            break;
        }

        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        // Parse the line
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "header" {
            continue;
        }
        if kind != "sample" {
            continue;
        }

        // Extract dt_ms for timing
        let dt_ms = v.get("dt_ms").and_then(|d| d.as_f64()).unwrap_or(0.0);

        // Sleep for the inter-packet delay, adjusted by speed
        if dt_ms > 0.0 {
            let delay_us = ((dt_ms * 1000.0) / speed) as u64;
            if delay_us > 0 {
                tokio::time::sleep(Duration::from_micros(delay_us)).await;
            }
        }

        // Re-check active flag after sleep
        if !*REPLAY_ACTIVE.lock().await {
            break;
        }

        // Parse and emit as EegSample
        let sample = EegSample {
            timestamp: chrono::Utc::now().timestamp_millis() as f64,
            channel: v
                .get("channel")
                .and_then(|c| c.as_str())
                .unwrap_or("TP9")
                .to_string(),
            samples: v
                .get("samples")
                .and_then(|s| s.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<f64>>())
                .unwrap_or_default(),
            sequence: v
                .get("seq")
                .and_then(|q| q.as_u64())
                .and_then(|q| u16::try_from(q).ok()),
            source: "replay",
        };

        let _ = app.emit("muse-eeg", &sample);
        packets_emitted += 1;

        if packets_emitted <= 3 || packets_emitted.is_multiple_of(500) {
            println!(
                "[replay] emitted={packets_emitted} ch={} samples={}",
                sample.channel,
                sample.samples.len()
            );
        }
    }

    println!("[replay] replay finished: total_emitted={packets_emitted}");
    *REPLAY_ACTIVE.lock().await = false;
    release_source("replay").await;
    emit_idle(app);
}

fn emit_idle(app: &AppHandle) {
    let _ = app.emit(
        "muse-ble-state",
        BleStatePayload {
            state: "idle".to_string(),
            data_source: "live".to_string(),
        },
    );
}

#[tauri::command]
#[specta::specta]
pub async fn stop_replay(app: AppHandle) -> Result<(), String> {
    println!("[replay] stop requested");
    *REPLAY_ACTIVE.lock().await = false;
    release_source("replay").await;
    release_source("demo").await;
    // Give the task a moment to exit
    tokio::time::sleep(Duration::from_millis(50)).await;
    emit_idle(&app);
    Ok(())
}

// ── Synthetic Demo Generator ────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn start_demo_eeg(app: AppHandle, mode: String, speed: f64) -> Result<(), String> {
    claim_source("demo").await?;
    // Stop any existing replay/demo
    *REPLAY_ACTIVE.lock().await = false;
    tokio::time::sleep(Duration::from_millis(100)).await;
    *REPLAY_ACTIVE.lock().await = true;

    let speed = if speed <= 0.0 { 1.0 } else { speed };

    println!("[replay] starting demo: mode={mode} speed={speed}x");

    tokio::spawn(async move {
        demo_task(&app, &mode, speed).await;
    });

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_demo_eeg(app: AppHandle) -> Result<(), String> {
    // Same as stop_replay — same flag
    stop_replay(app).await
}

/// Deterministic xorshift64 PRNG — no external dependency needed for noise
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Returns a value in approximately [-1.0, 1.0] (uniform)
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64 / u64::MAX as f64) * 2.0 - 1.0
    }

    /// Approximate gaussian via sum of 4 uniforms (central limit theorem)
    fn next_gaussian(&mut self) -> f64 {
        let sum: f64 = (0..4).map(|_| self.next_f64()).sum();
        sum / 2.0 // roughly N(0,1)
    }
}

async fn demo_task(app: &AppHandle, mode: &str, speed: f64) {
    emit_streaming(app, "demo");

    let channels = ["TP9", "AF7", "AF8", "TP10"];
    let mut t: f64 = 0.0;
    let mut blink_timer: f64 = 8.0; // seconds until next blink
    let mut rng = Xorshift64::new(42);
    let mut packets_emitted: u64 = 0;

    // Mode-dependent amplitude multipliers
    let (alpha_mult, beta_mult, theta_mult) = match mode {
        "eyes_closed" => (2.5, 0.6, 1.2),
        "focus" => (0.8, 2.0, 0.7),
        _ => (1.0, 1.0, 1.0), // "mixed"
    };

    let pi2 = std::f64::consts::PI * 2.0;

    loop {
        if !*REPLAY_ACTIVE.lock().await {
            break;
        }

        for ch in &channels {
            let mut samples = Vec::with_capacity(12);
            for i in 0..12 {
                let s = t + (i as f64) / 256.0;

                // Multi-band synthesis
                let alpha = 15.0 * alpha_mult * (pi2 * 10.0 * s).sin();
                let beta = 8.0 * beta_mult * (pi2 * 18.0 * s).sin();
                let theta = 10.0 * theta_mult * (pi2 * 6.0 * s).sin();
                let delta = 12.0 * (pi2 * 2.5 * s).sin();
                let gamma = 3.0 * (pi2 * 40.0 * s).sin();
                let noise = rng.next_gaussian() * 3.0;

                // Blink artifact on frontal channels
                let blink = if (0.0..=0.15).contains(&blink_timer) && (*ch == "AF7" || *ch == "AF8")
                {
                    120.0 * (pi2 * blink_timer / 0.15).sin().abs()
                } else {
                    0.0
                };

                samples.push(alpha + beta + theta + delta + gamma + noise + blink);
            }

            let sample = EegSample {
                timestamp: chrono::Utc::now().timestamp_millis() as f64,
                channel: ch.to_string(),
                samples,
                sequence: None,
                source: "demo",
            };
            // A recording opened during the demo is labelled `demo` in its header and never mixes in live data.
            maybe_record_sample(&sample).await;
            let _ = app.emit("muse-eeg", &sample);

            packets_emitted += 1;
        }

        t += 12.0 / 256.0;
        blink_timer -= 12.0 / 256.0;
        if blink_timer <= -0.05 {
            // Random-ish interval between 5-10 seconds
            blink_timer = 5.0 + (rng.next_u64() % 5000) as f64 / 1000.0;
        }

        if packets_emitted <= 12 || packets_emitted.is_multiple_of(2000) {
            println!("[replay] demo emitted={packets_emitted} t={t:.2}s");
        }

        // 12 samples / 256 Hz = 46.875ms per packet group (4 channels)
        // Sleep once per channel group, not per channel
        let sleep_us = (46_875.0 / speed) as u64;
        tokio::time::sleep(Duration::from_micros(sleep_us)).await;
    }

    println!("[replay] demo stopped: total_emitted={packets_emitted}");
    *REPLAY_ACTIVE.lock().await = false;
    release_source("demo").await;
    emit_idle(app);
}
