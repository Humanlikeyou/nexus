use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use super::muse::EegSample;

// ── Data directory ──────────────────────────────────────────────

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap()
                .join("Library/Application Support")
        })
        .join(crate::app_paths::DATA_DIR_NAME)
}

fn replays_dir() -> PathBuf {
    nexus_data_dir().join("replays")
}

// ── Global state ────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref REPLAY_ACTIVE: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref RECORDING_ENABLED: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref RECORDING_FILE: Arc<Mutex<Option<BufWriter<File>>>> = Arc::new(Mutex::new(None));
    /// Monotonic packet index for recording — increments per sample line written
    static ref RECORDING_PREV_TS: Arc<Mutex<Option<f64>>> = Arc::new(Mutex::new(None));
}

// ── NDJSON Schema ───────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct ReplayHeader {
    kind: String, // "header"
    sample_rate: u16,
    channels: Vec<String>,
    start_ts: f64,
    session_id: String,
}

#[derive(Serialize, Deserialize)]
struct ReplaySampleLine {
    kind: String, // "sample"
    dt_ms: f64,   // delta from previous sample (0.0 for first)
    timestamp: f64,
    channel: String,
    samples: Vec<f64>,
}

#[derive(Serialize, Clone)]
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

/// Called from muse.rs after each muse-eeg emit. Fast path: checks bool, returns immediately if not recording.
pub async fn maybe_record_sample(sample: &EegSample) {
    let enabled = *RECORDING_ENABLED.lock().await;
    if !enabled {
        return;
    }

    let mut file_guard = RECORDING_FILE.lock().await;
    let file = match file_guard.as_mut() {
        Some(f) => f,
        None => return,
    };

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
    };

    if let Ok(json) = serde_json::to_string(&line) {
        let _ = writeln!(file, "{}", json);
    }
}

// ── Tauri commands ──────────────────────────────────────────────

#[tauri::command]
pub async fn start_eeg_recording() -> Result<String, String> {
    let dir = replays_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create replays dir: {e}"))?;

    let now = chrono::Utc::now();
    let filename = format!("eeg_{}.ndjson", now.format("%Y%m%d_%H%M%S"));
    let path = dir.join(&filename);

    let file = File::create(&path).map_err(|e| format!("Failed to create file: {e}"))?;
    let mut writer = BufWriter::new(file);

    // Write header line
    let header = ReplayHeader {
        kind: "header".to_string(),
        sample_rate: 256,
        channels: vec![
            "TP9".to_string(),
            "AF7".to_string(),
            "AF8".to_string(),
            "TP10".to_string(),
        ],
        start_ts: now.timestamp_millis() as f64,
        session_id: uuid::Uuid::new_v4().to_string(),
    };
    let header_json = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    writeln!(writer, "{}", header_json).map_err(|e| e.to_string())?;

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
pub async fn stop_eeg_recording() -> Result<String, String> {
    *RECORDING_ENABLED.lock().await = false;

    let mut file_guard = RECORDING_FILE.lock().await;
    finish_recording(&mut file_guard).map_err(|e| e.to_string())?;

    *RECORDING_PREV_TS.lock().await = None;

    println!("[replay] recording stopped");
    Ok("stopped".to_string())
}

#[cfg(test)]
mod recording_tests {
    use super::*;

    #[derive(Default)]
    struct RetryWriter { fail: bool, bytes: Vec<u8> }

    impl Write for RetryWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.fail { return Err(std::io::Error::other("disk unavailable")); }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    #[test]
    fn failed_flush_keeps_pending_samples_for_retry() {
        let mut writer = BufWriter::new(RetryWriter { fail: true, ..Default::default() });
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
pub async fn start_replay(app: AppHandle, filename: String, speed: f64) -> Result<(), String> {
    super::path_safety::validate_file_name(&filename, &["ndjson"])?;
    let path = super::path_safety::safe_join(&replays_dir(), &filename)?;
    if !path.is_file() {
        return Err(format!("Replay file not found: {filename}"));
    }

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
pub async fn stop_replay(app: AppHandle) -> Result<(), String> {
    println!("[replay] stop requested");
    *REPLAY_ACTIVE.lock().await = false;
    // Give the task a moment to exit
    tokio::time::sleep(Duration::from_millis(50)).await;
    emit_idle(&app);
    Ok(())
}

// ── Synthetic Demo Generator ────────────────────────────────────

#[tauri::command]
pub async fn start_demo_eeg(app: AppHandle, mode: String, speed: f64) -> Result<(), String> {
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
                let blink = if (0.0..=0.15).contains(&blink_timer)
                    && (*ch == "AF7" || *ch == "AF8")
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
            };
            let _ = app.emit("muse-eeg", &sample);

            // If recording is active, record demo samples too
            maybe_record_sample(&sample).await;

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
    emit_idle(app);
}
