use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::ble_common::{push_proof_event, BleState};
use super::replay;

// Muse BLE Service UUID
#[allow(dead_code)]
const MUSE_SERVICE: Uuid = Uuid::from_u128(0x0000fe8d_0000_1000_8000_00805f9b34fb);

// Muse characteristic UUID prefixes (first 4 bytes).
// The suffix varies by model (Muse 2016 vs Muse 2/S), so we match by prefix only.
// Control: 273e0001, TP9: 273e0003, AF7: 273e0004, AF8: 273e0005, TP10: 273e0006
const MUSE_CONTROL_PREFIX: [u8; 4] = [0x27, 0x3e, 0x00, 0x01];
const MUSE_TP9_PREFIX: [u8; 4] = [0x27, 0x3e, 0x00, 0x03];
const MUSE_AF7_PREFIX: [u8; 4] = [0x27, 0x3e, 0x00, 0x04];
const MUSE_AF8_PREFIX: [u8; 4] = [0x27, 0x3e, 0x00, 0x05];
const MUSE_TP10_PREFIX: [u8; 4] = [0x27, 0x3e, 0x00, 0x06];

/// Data streams beyond the four EEG channels. UUIDs, layouts and scales follow muse-lsl's
/// `constants.py` and `muse.py`, and muse-js's `muse.ts` and `muse-parse.ts`, which agree.
/// None of these is required: a headset without one still streams EEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalStream {
    /// Same packet and scale as EEG, emitted on `muse-eeg`. `AUX` is what the frontend reads.
    Aux(&'static str),
    Accelerometer,
    Gyroscope,
    Ppg(&'static str),
}

const OPTIONAL_STREAMS: [([u8; 4], OptionalStream); 7] = [
    ([0x27, 0x3e, 0x00, 0x02], OptionalStream::Aux("AUX_L")),
    ([0x27, 0x3e, 0x00, 0x07], OptionalStream::Aux("AUX")),
    ([0x27, 0x3e, 0x00, 0x09], OptionalStream::Gyroscope),
    ([0x27, 0x3e, 0x00, 0x0a], OptionalStream::Accelerometer),
    ([0x27, 0x3e, 0x00, 0x0f], OptionalStream::Ppg("ambient")),
    ([0x27, 0x3e, 0x00, 0x10], OptionalStream::Ppg("infrared")),
    ([0x27, 0x3e, 0x00, 0x11], OptionalStream::Ppg("red")),
];

/// One optional subscribe may not hold up EEG for long.
const OPTIONAL_SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Silence on the EEG channels this long counts as a stall.
const EEG_SILENCE_CHECK: Duration = Duration::from_secs(5);

const ACCELEROMETER_G_PER_LSB: f64 = 0.0000610352;
const GYROSCOPE_DPS_PER_LSB: f64 = 0.0074768;
/// A Muse data packet: a 16-bit sequence number and 18 bytes of samples.
const MUSE_PACKET_BYTES: usize = 20;

/// Match a characteristic UUID by its first 4 bytes (works across all Muse models).
fn uuid_prefix_match(uuid: &Uuid, prefix: &[u8; 4]) -> bool {
    uuid.as_bytes()[..4] == prefix[..]
}

pub fn optional_stream(uuid: &Uuid) -> Option<OptionalStream> {
    OPTIONAL_STREAMS
        .iter()
        .find(|(prefix, _)| uuid_prefix_match(uuid, prefix))
        .map(|(_, stream)| *stream)
}

pub fn is_control_characteristic(uuid: &Uuid) -> bool {
    uuid_prefix_match(uuid, &MUSE_CONTROL_PREFIX)
}

/// EEG, or one of the optional streams.
pub fn is_data_characteristic(uuid: &Uuid) -> bool {
    let eeg = [
        MUSE_TP9_PREFIX,
        MUSE_AF7_PREFIX,
        MUSE_AF8_PREFIX,
        MUSE_TP10_PREFIX,
    ];
    eeg.iter().any(|prefix| uuid_prefix_match(uuid, prefix)) || optional_stream(uuid).is_some()
}

/// A readable name for a Muse characteristic, by the same prefix match.
pub fn characteristic_name(uuid: &Uuid) -> &'static str {
    const NAMES: [(u8, &str); 17] = [
        (0x01, "control"),
        (0x02, "aux_left"),
        (0x03, "eeg_tp9"),
        (0x04, "eeg_af7"),
        (0x05, "eeg_af8"),
        (0x06, "eeg_tp10"),
        (0x07, "aux_right"),
        (0x08, "ref_drl"),
        (0x09, "gyroscope"),
        (0x0a, "accelerometer"),
        (0x0b, "telemetry"),
        (0x0f, "ppg_ambient"),
        (0x10, "ppg_infrared"),
        (0x11, "ppg_red"),
        (0x12, "thermistor"),
        (0x13, "athena_data_1"),
        (0x14, "athena_data_2"),
    ];
    NAMES
        .iter()
        .find(|(id, _)| uuid_prefix_match(uuid, &[0x27, 0x3e, 0x00, *id]))
        .map_or("unknown", |(_, name)| name)
}

/// One log line per characteristic: names and UUIDs only.
pub fn gatt_log_text(stamp: &str, characteristics: &[(Uuid, Uuid)]) -> String {
    let mut text = format!("{stamp} characteristics={}\n", characteristics.len());
    for (service, uuid) in characteristics {
        let name = characteristic_name(uuid);
        text.push_str(&format!(
            "{stamp} service={service} characteristic={uuid} name={name}\n"
        ));
    }
    text
}

/// Appends to the day's GATT log in `dir`, created private, and returns its path.
pub fn append_gatt_log(
    dir: &std::path::Path,
    characteristics: &[(Uuid, Uuid)],
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    crate::app_paths::create_private_dir_all(dir)?;
    let now = chrono::Local::now();
    let path = dir.join(format!("muse-gatt-{}.log", now.format("%Y-%m-%d")));
    let mut file = crate::app_paths::private_open_options()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(gatt_log_text(&now.to_rfc3339(), characteristics).as_bytes())?;
    Ok(path)
}

/// Where Muse logs and captures live, under the app data folder.
pub fn muse_data_dir() -> std::path::PathBuf {
    crate::app_paths::data_dir().join("muse")
}

lazy_static::lazy_static! {
    static ref MUSE_ADAPTER: Arc<Mutex<Option<btleplug::platform::Adapter>>> = Arc::new(Mutex::new(None));
    /// Peripheral objects from the most recent scan, keyed by p.id().to_string().
    /// This is the authoritative lookup for connect — no re-querying the adapter.
    static ref MUSE_DISCOVERED: Arc<Mutex<HashMap<String, Peripheral>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref MUSE_PERIPHERAL: Arc<Mutex<Option<Peripheral>>> = Arc::new(Mutex::new(None));
    static ref MUSE_STREAMING: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref MUSE_SCAN_ACTIVE: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref MUSE_BLE_STATE: Arc<Mutex<BleState>> = Arc::new(Mutex::new(BleState::Idle));
}

lazy_static::lazy_static! {
    /// Optional streams that subscribed on the current connection; the replay header lists these.
    static ref SUBSCRIBED: std::sync::Mutex<replay::StreamSet> = std::sync::Mutex::new(replay::StreamSet::default());
}

/// Snapshot of the optional streams subscribed so far.
pub fn subscribed_streams() -> replay::StreamSet {
    SUBSCRIBED.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Test hook: pretend these optional streams subscribed.
#[doc(hidden)]
pub fn set_subscribed_streams(set: replay::StreamSet) {
    if let Ok(mut s) = SUBSCRIBED.lock() {
        *s = set;
    }
}

fn reset_subscribed() {
    if let Ok(mut s) = SUBSCRIBED.lock() {
        *s = replay::StreamSet::default();
    }
}

/// Adds `stream` to `set`. `AUX_L` is subscribed for the stream but is not recorded, so it is
/// not listed.
fn note_subscribed(set: &mut replay::StreamSet, stream: OptionalStream) {
    let add = |v: &mut Vec<String>, name: &str| {
        if !v.iter().any(|n| n == name) {
            v.push(name.to_string());
        }
    };
    match stream {
        OptionalStream::Aux("AUX") => set.aux = true,
        OptionalStream::Aux(_) => {}
        OptionalStream::Accelerometer => add(&mut set.motion, "accelerometer"),
        OptionalStream::Gyroscope => add(&mut set.motion, "gyroscope"),
        OptionalStream::Ppg(name) => add(&mut set.ppg, name),
    }
}

/// Structured BLE state payload with data_source discriminator.
#[derive(Serialize, Clone)]
struct MuseBleStatePayload {
    state: String,
    data_source: String,
}

async fn emit_muse_state(app: &AppHandle, state: BleState) {
    *MUSE_BLE_STATE.lock().await = state.clone();
    let state_str = match &state {
        BleState::Idle => "idle",
        BleState::Scanning => "scanning",
        BleState::Discovered => "discovered",
        BleState::Connecting => "connecting",
        BleState::Connected => "connected",
        BleState::Streaming => "streaming",
    };
    let _ = app.emit(
        "muse-ble-state",
        MuseBleStatePayload {
            state: state_str.to_string(),
            data_source: "live".to_string(),
        },
    );
    push_proof_event("muse", "state_change", &format!("{:?}", state)).await;
}

async fn get_or_init_adapter() -> Result<btleplug::platform::Adapter, String> {
    let mut stored = MUSE_ADAPTER.lock().await;
    if let Some(adapter) = stored.as_ref() {
        return Ok(adapter.clone());
    }
    println!("[muse] init: status=starting");
    let manager = Manager::new().await.map_err(|e| {
        println!("[muse] init: error=manager_failed reason={e}");
        e.to_string()
    })?;
    let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or("No Bluetooth adapter found")?;
    println!("[muse] init: status=adapter_ready");
    *stored = Some(adapter.clone());
    Ok(adapter)
}

#[derive(Serialize, Clone, specta::Type)]
pub struct MuseDevice {
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EegSample {
    pub timestamp: f64,
    pub channel: String,
    pub samples: Vec<f64>,
}

/// `muse-motion`: 3 samples per packet, each `[x, y, z]`, about 52 Hz.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MotionSample {
    pub timestamp: f64,
    /// `accelerometer` or `gyroscope`.
    pub sensor: String,
    /// `g` or `deg/s`.
    pub unit: String,
    pub sequence: u16,
    pub samples: Vec<[f64; 3]>,
}

/// `muse-ppg`: 6 raw unsigned 24-bit counts per packet, about 64 Hz.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PpgSample {
    pub timestamp: f64,
    /// `ambient`, `infrared` or `red`.
    pub channel: String,
    pub sequence: u16,
    pub samples: Vec<u32>,
}

#[derive(Debug, PartialEq)]
pub enum OptionalEvent {
    Aux(EegSample),
    Motion(MotionSample),
    Ppg(PpgSample),
}

#[derive(Serialize, Clone, specta::Type)]
pub struct MuseConnectionState {
    pub connected: bool,
    pub device_name: Option<String>,
    pub streaming: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn scan_muse(app: AppHandle) -> Result<Vec<MuseDevice>, String> {
    println!("[muse] scan: status=starting");
    emit_muse_state(&app, BleState::Scanning).await;
    let adapter = get_or_init_adapter().await?;

    // If a scan is already running, stop it first (idempotent reset)
    {
        let mut active = MUSE_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[muse] scan: stopping_previous_scan=true");
            adapter.stop_scan().await.ok();
            *active = false;
        }
    }

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| {
            println!("[muse] scan: error=start_scan_failed reason={e}");
            e.to_string()
        })?;
    println!("[muse] scan: status=scanning duration_secs=5");

    tokio::time::sleep(Duration::from_secs(5)).await;

    let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
    println!("[muse] scan: total_peripherals={}", peripherals.len());

    let mut devices = Vec::new();
    let mut discovered = MUSE_DISCOVERED.lock().await;
    discovered.clear();

    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            let name = props.local_name.unwrap_or_default();
            let id = p.id().to_string();
            if !name.is_empty() {
                println!(
                    "[muse] scan: peripheral id={id} name={name:?} rssi={:?}",
                    props.rssi
                );
            }
            if name.to_lowercase().contains("muse") {
                println!("[muse] scan: match=true id={id} name={name}");
                push_proof_event(
                    "muse",
                    "scan_result",
                    &format!("id={id} name={name} rssi={:?}", props.rssi),
                )
                .await;
                discovered.insert(id.clone(), p);
                devices.push(MuseDevice {
                    name,
                    address: id,
                    rssi: props.rssi,
                });
            }
        }
    }

    // Do NOT stop scan — keep it alive so cached peripherals remain valid
    // on macOS CoreBluetooth. The scan will be stopped lazily when connect
    // begins, or after a 30s safety timeout.
    {
        let mut active = MUSE_SCAN_ACTIVE.lock().await;
        *active = true;
    }

    // Spawn 30s background timeout to stop the scan (battery safety net)
    let timeout_adapter = adapter.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut active = MUSE_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[muse] scan_timeout: stopping_scan after_secs=30");
            timeout_adapter.stop_scan().await.ok();
            *active = false;
        }
    });

    println!(
        "[muse] scan: status=done muse_devices={} cached={}",
        devices.len(),
        discovered.len()
    );

    if devices.is_empty() {
        emit_muse_state(&app, BleState::Idle).await;
    } else {
        emit_muse_state(&app, BleState::Discovered).await;
    }

    Ok(devices)
}

#[tauri::command]
#[specta::specta]
pub async fn connect_muse(app: AppHandle, address: String) -> Result<MuseConnectionState, String> {
    println!("[muse] connect: target_id={address}");
    // Supersede any disconnect that began earlier, then wait out a detached teardown
    // (it skips itself if it starts after this bump).
    bump_conn_gen();
    drop(TEARDOWN.lock().await);
    emit_muse_state(&app, BleState::Connecting).await;

    // ---- Step 0: Stop active scan ----
    // Cached peripherals remain valid because we hold Arc references.
    {
        let adapter = get_or_init_adapter().await?;
        let mut active = MUSE_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[muse] connect: stopping_active_scan=true");
            adapter.stop_scan().await.ok();
            *active = false;
        }
    }

    // ---- Step 1: Resolve the Peripheral object ----
    // Priority: MUSE_DISCOVERED cache → adapter peripherals → 5s re-scan
    let peripheral = {
        let discovered = MUSE_DISCOVERED.lock().await;
        println!(
            "[muse] resolve: cache_entries={} keys={:?}",
            discovered.len(),
            discovered.keys().collect::<Vec<_>>()
        );

        if let Some(p) = discovered.get(&address) {
            println!("[muse] resolve: source=cache");
            p.clone()
        } else {
            println!("[muse] resolve: cache_miss=true fallback=adapter_peripherals");
            drop(discovered);
            let adapter = get_or_init_adapter().await?;
            let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
            println!(
                "[muse] resolve: adapter_peripherals={} ids={:?}",
                peripherals.len(),
                peripherals
                    .iter()
                    .map(|p| p.id().to_string())
                    .collect::<Vec<_>>()
            );

            if let Some(p) = peripherals
                .into_iter()
                .find(|p| p.id().to_string() == address)
            {
                println!("[muse] resolve: source=adapter");
                p
            } else {
                // Last resort: 5-second re-scan
                println!("[muse] resolve: adapter_miss=true fallback=rescan duration_secs=5");
                adapter
                    .start_scan(ScanFilter::default())
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::time::sleep(Duration::from_secs(5)).await;
                let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
                adapter.stop_scan().await.ok();

                let ids: Vec<_> = peripherals.iter().map(|p| p.id().to_string()).collect();
                println!(
                    "[muse] resolve: rescan_peripherals={} ids={:?}",
                    ids.len(),
                    ids
                );

                peripherals
                    .into_iter()
                    .find(|p| p.id().to_string() == address)
                    .ok_or_else(|| {
                        let msg = format!(
                            "Device handle no longer valid (id: {}). Please rescan.",
                            address
                        );
                        println!("[muse] resolve: error=device_not_found id={address}");
                        msg
                    })?
            }
        }
    };

    // ---- Step 2: BLE connect with retries ----
    let max_retries = 3;
    let mut last_err = String::new();

    for attempt in 1..=max_retries {
        println!("[muse] ble_connect: attempt={attempt}/{max_retries}");
        push_proof_event(
            "muse",
            "connect_attempt",
            &format!("attempt={attempt}/{max_retries} id={address}"),
        )
        .await;
        match peripheral.connect().await {
            Ok(()) => {
                println!("[muse] ble_connect: status=success attempt={attempt}");
                push_proof_event("muse", "connect_success", &format!("attempt={attempt}")).await;
                last_err.clear();
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                println!("[muse] ble_connect: status=failed attempt={attempt} reason={last_err}");
                push_proof_event(
                    "muse",
                    "connect_failed",
                    &format!("attempt={attempt} reason={last_err}"),
                )
                .await;
                if attempt < max_retries {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    if !last_err.is_empty() {
        let connected = peripheral.is_connected().await.unwrap_or(false);
        if !connected {
            let msg = format!("Connect failed after {max_retries} attempts: {last_err}");
            println!("[muse] ble_connect: error=retries_exhausted");
            emit_muse_state(&app, BleState::Discovered).await;
            return Err(msg);
        }
    }

    // ---- Step 3: Service discovery ----
    println!("[muse] discovery: status=starting");
    peripheral.discover_services().await.map_err(|e| {
        let msg = format!("Service discovery failed: {e}");
        println!("[muse] discovery: error=failed reason={e}");
        msg
    })?;

    let chars = peripheral.characteristics();
    println!(
        "[muse] discovery: status=done characteristics={}",
        chars.len()
    );
    // Print every discovered characteristic for debugging
    for ch in &chars {
        let props = format!("{:?}", ch.properties);
        println!("[muse] gatt: uuid={} properties={props}", ch.uuid);
    }
    let listing: Vec<(Uuid, Uuid)> = chars.iter().map(|c| (c.service_uuid, c.uuid)).collect();
    if let Err(e) = append_gatt_log(&muse_data_dir(), &listing) {
        println!("[muse] gatt_log: error={e}");
    }

    let name = peripheral
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|p| p.local_name);

    println!(
        "[muse] connect: status=success device={:?} id={address}",
        name
    );

    // Lock, store, shadow write and generation bump are one step under the guard.
    let new_id = peripheral.id();
    let replaced = install_peripheral(&MUSE_PERIPHERAL, peripheral.clone(), |p| {
        if let Ok(mut link) = MUSE_LINK.lock() {
            *link = Some(p.clone());
        }
    })
    .await;
    if let Some(old) = replaced {
        // A different peripheral being replaced must not keep its OS link; the same
        // handle is the new link and is left alone.
        if old.id() != new_id {
            let _ = tokio::time::timeout(DISCONNECT_BOUND, old.disconnect()).await;
        }
    }

    emit_muse_state(&app, BleState::Connected).await;

    Ok(MuseConnectionState {
        connected: true,
        device_name: name,
        streaming: false,
    })
}

/// Whole-command bound for `disconnect_muse`, lock waits included.
const DISCONNECT_BOUND: std::time::Duration = std::time::Duration::from_secs(8);
/// Fixed text the UI shows when the headset (or a held lock) does not answer.
const DISCONNECT_TIMEOUT_TEXT: &str = "The headset didn't answer; try again";

/// Bumped on every connect and every disconnect. A disconnect that began under
/// an older value must not touch a headset connected afterwards.
static MUSE_CONN_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

lazy_static::lazy_static! {
    /// Shadow clone of the connected peripheral, readable without the async locks,
    /// so a timed-out disconnect can still tear the OS link down.
    static ref MUSE_LINK: std::sync::Mutex<Option<Peripheral>> = std::sync::Mutex::new(None);
}

fn conn_gen() -> u64 {
    MUSE_CONN_GEN.load(std::sync::atomic::Ordering::SeqCst)
}

fn bump_conn_gen() -> u64 {
    MUSE_CONN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// True while no connect or disconnect has happened since `started_under`.
fn gen_is_current(started_under: u64) -> bool {
    conn_gen() == started_under
}

/// Fixed error for a disconnect that a newer connect or disconnect overtook.
const SUPERSEDED: &str = "superseded";

/// Serialises a detached teardown against a reconnect: connect waits for it.
static TEARDOWN: Mutex<()> = Mutex::const_new(());

/// Take the stored value only while `gen_is_current(started_under)`, checked right
/// after each lock is acquired (streaming first, then stored). A stale caller touches
/// nothing and gets `Err(SUPERSEDED)`.
async fn take_if_current<T>(
    started_under: u64,
    streaming: &Mutex<bool>,
    stored: &Mutex<Option<T>>,
) -> Result<Option<T>, &'static str> {
    let mut streaming = streaming.lock().await;
    if !gen_is_current(started_under) {
        return Err(SUPERSEDED);
    }
    let mut stored = stored.lock().await;
    if !gen_is_current(started_under) {
        return Err(SUPERSEDED);
    }
    *streaming = false;
    Ok(stored.take())
}

/// Store a new value under the guard, run `shadow` and bump the generation before the
/// guard is released. Returns what was replaced.
async fn install_peripheral<T>(
    stored: &Mutex<Option<T>>,
    new: T,
    shadow: impl FnOnce(&T),
) -> Option<T> {
    let mut guard = stored.lock().await;
    shadow(&new);
    let old = guard.replace(new);
    bump_conn_gen();
    old
}

/// Lock order everywhere: streaming, then stored.
async fn lock_streaming_then_stored<'a, T>(
    streaming: &'a Mutex<bool>,
    stored: &'a Mutex<Option<T>>,
) -> (
    tokio::sync::MutexGuard<'a, bool>,
    tokio::sync::MutexGuard<'a, Option<T>>,
) {
    let streaming = streaming.lock().await;
    let stored = stored.lock().await;
    (streaming, stored)
}

/// Run `work` under one bound; a timeout becomes the fixed UI error.
async fn bounded<F: std::future::Future<Output = Result<(), String>>>(
    limit: std::time::Duration,
    work: F,
) -> Result<(), String> {
    match tokio::time::timeout(limit, work).await {
        Ok(result) => result,
        Err(_) => Err(DISCONNECT_TIMEOUT_TEXT.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect_muse(app: AppHandle) -> Result<(), String> {
    println!("[muse] disconnect: status=starting");
    let started_under = bump_conn_gen();
    let body = async {
        let taken = take_if_current(started_under, &MUSE_STREAMING, &MUSE_PERIPHERAL)
            .await
            .map_err(|e| e.to_string())?;
        // Held across the real disconnect so a same-device reconnect waits for it.
        let _teardown = TEARDOWN.lock().await;
        if let Some(peripheral) = taken {
            if let Err(e) = peripheral.disconnect().await {
                println!("[muse] disconnect: status=error detail={e}");
                return Err(DISCONNECT_TIMEOUT_TEXT.to_string());
            }
            if gen_is_current(started_under) {
                if let Ok(mut link) = MUSE_LINK.lock() {
                    link.take();
                }
            }
            println!("[muse] disconnect: status=done");
        } else if !disconnect_shadow(&MUSE_LINK, DISCONNECT_BOUND, |p| async move {
            p.disconnect().await.map_err(|e| e.to_string())
        })
        .await
        {
            // An earlier timed-out disconnect left the OS link up and it could not be closed now.
            return Err(DISCONNECT_TIMEOUT_TEXT.to_string());
        }
        Ok(())
    };
    if let Err(message) = bounded(DISCONNECT_BOUND, body).await {
        if message == SUPERSEDED {
            println!("[muse] disconnect: status=superseded (nothing touched)");
            return Err(message);
        }
        // Distinct from a clean disconnect: no state is emitted.
        println!("[muse] disconnect: status=failed_unclean (no clean disconnect)");
        let link = MUSE_LINK.lock().ok().and_then(|l| l.clone());
        if let Some(peripheral) = link {
            println!("[muse] disconnect: teardown=detached_scheduled");
            // Best effort, detached, bounded, serialised against reconnects. MUSE_LINK
            // is cleared only after the real disconnect finishes.
            tokio::spawn(async move {
                let _guard = TEARDOWN.lock().await;
                if !gen_is_current(started_under) {
                    println!("[muse] disconnect: teardown=skipped (newer connection)");
                    return;
                }
                match tokio::time::timeout(DISCONNECT_BOUND, peripheral.disconnect()).await {
                    Ok(Ok(())) => {
                        if gen_is_current(started_under) {
                            if let Ok(mut link) = MUSE_LINK.lock() {
                                link.take();
                            }
                        }
                        println!("[muse] disconnect: teardown=completed");
                    }
                    Ok(Err(e)) => println!("[muse] disconnect: teardown=error detail={e}"),
                    Err(_) => println!("[muse] disconnect: teardown=timed_out"),
                }
            });
        }
        return Err(message);
    }
    // A connect may have installed a peripheral meanwhile; then this disconnect is stale.
    if !gen_is_current(started_under) {
        println!("[muse] disconnect: status=superseded (nothing touched)");
        return Err(DISCONNECT_TIMEOUT_TEXT.to_string());
    }
    // A recording still open keeps its subscribed list for the header.
    if replay::active_recording_name().await.is_none() {
        reset_subscribed();
    }

    emit_muse_state(&app, BleState::Idle).await;
    Ok(())
}

/// Close a link left up by an earlier timed-out disconnect: clone the shadow, disconnect
/// it under `limit`, and clear the shadow only on success. True when nothing is left up.
/// The caller holds TEARDOWN.
async fn disconnect_shadow<T, F, Fut>(
    shadow: &std::sync::Mutex<Option<T>>,
    limit: std::time::Duration,
    disconnect: F,
) -> bool
where
    T: Clone,
    F: FnOnce(T) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let link = shadow.lock().ok().and_then(|l| l.clone());
    let Some(link) = link else { return true };
    match tokio::time::timeout(limit, disconnect(link)).await {
        Ok(Ok(())) => {
            if let Ok(mut l) = shadow.lock() {
                l.take();
            }
            println!("[muse] disconnect: shadow_link=closed");
            true
        }
        _ => {
            println!("[muse] disconnect: shadow_link=still_up");
            false
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn start_eeg_stream(app: AppHandle) -> Result<(), String> {
    println!("[muse] eeg_stream: status=starting");
    super::sessions::begin_body_context_capture();
    let stored = MUSE_PERIPHERAL.lock().await;
    let peripheral = stored.as_ref().ok_or("Muse not connected")?.clone();
    drop(stored);

    let chars = peripheral.characteristics();

    // CRITICAL: Create notification stream BEFORE subscribing or sending commands.
    // btleplug only delivers notifications that arrive after the stream is created.
    let mut notification_stream = peripheral
        .notifications()
        .await
        .map_err(|e| format!("notifications() failed: {e}"))?;
    println!("[muse] eeg_task: notifications() stream acquired BEFORE subscribe");

    // Resolve actual characteristic UUIDs by prefix match (works across all Muse models)
    let eeg_prefixes: [(&[u8; 4], &str); 4] = [
        (&MUSE_TP9_PREFIX, "TP9"),
        (&MUSE_AF7_PREFIX, "AF7"),
        (&MUSE_AF8_PREFIX, "AF8"),
        (&MUSE_TP10_PREFIX, "TP10"),
    ];

    // Build channel map: actual UUID -> channel name (for notification routing)
    let mut channel_map: Vec<(Uuid, String)> = Vec::new();

    for (prefix, name) in &eeg_prefixes {
        if let Some(ch) = chars.iter().find(|c| uuid_prefix_match(&c.uuid, prefix)) {
            peripheral
                .subscribe(ch)
                .await
                .map_err(|e| format!("subscribe {name} failed: {e}"))?;
            println!("[muse] subscribe_ok uuid={} channel={name}", ch.uuid);
            push_proof_event(
                "muse",
                "subscribe_ok",
                &format!("uuid={} channel={name}", ch.uuid),
            )
            .await;
            channel_map.push((ch.uuid, name.to_string()));
        } else {
            println!("[muse] eeg_stream: characteristic_not_found channel={name} prefix={:02x}{:02x}{:02x}{:02x}", prefix[0], prefix[1], prefix[2], prefix[3]);
            push_proof_event("muse", "subscribe_missing", &format!("channel={name}")).await;
        }
    }

    if channel_map.is_empty() {
        return Err("No EEG characteristics found — cannot stream".to_string());
    }

    subscribe_optional_streams(&peripheral).await;

    // 250ms settle time after subscriptions before sending commands
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Send start commands to Muse control characteristic (prefix match).
    // Commands MUST be length-prefixed: [len, ...ascii, 0x0a].
    // Sequence: p50 (Muse 2 preset) → s (start) → d (data delivery).
    if let Some(control) = chars
        .iter()
        .find(|c| uuid_prefix_match(&c.uuid, &MUSE_CONTROL_PREFIX))
    {
        let commands: &[(&str, Vec<u8>)] = &[
            ("p50", muse_cmd("p50")), // Muse 2 EEG preset (p21 for 2016, p50 for Muse 2/S)
            ("s", muse_cmd("s")),     // Resume/start streaming
            ("d", muse_cmd("d")),     // Start data delivery
        ];
        for (label, cmd) in commands {
            peripheral
                .write(control, cmd, WriteType::WithoutResponse)
                .await
                .map_err(|e| format!("write {label} failed: {e}"))?;
            println!(
                "[muse] write_ok uuid={} cmd={} bytes={} hex={}",
                control.uuid,
                label,
                cmd.len(),
                hex::encode(cmd)
            );
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        push_proof_event(
            "muse",
            "commands_sent",
            &format!("uuid={} cmds=p50,s,d (length-prefixed)", control.uuid),
        )
        .await;
    } else {
        println!("[muse] eeg_stream: control_characteristic_not_found");
        push_proof_event("muse", "control_missing", "no 273e0001 prefix found").await;
        return Err("Control characteristic not found — cannot start stream".to_string());
    }

    {
        let mut streaming = MUSE_STREAMING.lock().await;
        *streaming = true;
    }

    emit_muse_state(&app, BleState::Streaming).await;
    push_proof_event("muse", "stream_start", "eeg").await;

    // Spawn task to read notifications — stream already created above
    let app_handle = app.clone();
    tokio::spawn(async move {
        println!("[muse] eeg_task: polling notification stream...");
        push_proof_event("muse", "stream_waiting", "polling for BLE notifications").await;

        let mut packets_total: u64 = 0;
        let mut packets_emitted: u64 = 0;
        let mut packets_dropped: u64 = 0;
        let mut emit_errors: u64 = 0;
        let mut last_report = std::time::Instant::now();
        let mut last_sizes: Vec<usize> = Vec::new();
        let task_start = std::time::Instant::now();
        let mut got_first = false;
        // Silence means EEG silence: motion and PPG arriving must not hide a dead EEG stream.
        let mut silent_since = std::time::Instant::now();

        loop {
            // Check streaming flag
            {
                let streaming = MUSE_STREAMING.lock().await;
                if !*streaming {
                    println!("[muse] eeg_task: streaming=false, exiting loop");
                    break;
                }
            }

            // Use select to add a 5-second timeout for detecting silent streams
            let notification = tokio::select! {
                n = notification_stream.next() => n,
                _ = tokio::time::sleep(EEG_SILENCE_CHECK.saturating_sub(silent_since.elapsed())) => {
                    silent_since = std::time::Instant::now();
                    if !got_first {
                        let elapsed = task_start.elapsed().as_secs();
                        let msg = format!("no BLE notifications received after {elapsed}s — stream may be dead");
                        println!("[muse] eeg_task: WARNING {msg}");
                        push_proof_event("muse", "stream_timeout", &msg).await;
                        if elapsed >= 15 {
                            println!("[muse] eeg_task: giving up after 15s with 0 notifications");
                            push_proof_event("muse", "stream_dead", "0 notifications after 15s").await;
                            break;
                        }
                    } else {
                        // Got packets before but stream went silent
                        let msg = format!("no notifications for 5s (total_so_far={packets_total})");
                        println!("[muse] eeg_task: WARNING {msg}");
                        push_proof_event("muse", "stream_stall", &msg).await;
                    }
                    continue;
                }
            };

            let notification = match notification {
                Some(n) => n,
                None => {
                    println!("[muse] eeg_task: notification_stream ended (None)");
                    push_proof_event("muse", "stream_ended", "notification_stream returned None")
                        .await;
                    break;
                }
            };

            if let Some(stream) = optional_stream(&notification.uuid) {
                emit_optional(&app_handle, stream, &notification.value).await;
                continue;
            }
            silent_since = std::time::Instant::now();

            if !got_first {
                got_first = true;
                let wait_ms = task_start.elapsed().as_millis();
                println!(
                    "[muse] eeg_task: first notification received after {wait_ms}ms uuid={} len={}",
                    notification.uuid,
                    notification.value.len()
                );
                push_proof_event(
                    "muse",
                    "first_notification",
                    &format!(
                        "after={wait_ms}ms uuid={} len={}",
                        notification.uuid,
                        notification.value.len()
                    ),
                )
                .await;
            }

            packets_total += 1;

            let channel_name = channel_map
                .iter()
                .find(|(uuid, _)| *uuid == notification.uuid)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("unknown:{}", notification.uuid));

            let data = &notification.value;
            last_sizes.push(data.len());
            if last_sizes.len() > 20 {
                last_sizes.remove(0);
            }

            // Parse Muse EEG packet: 16-bit timestamp + 12x 12-bit samples
            if data.len() >= 20 {
                let samples = parse_eeg_packet(data);
                let sample = EegSample {
                    timestamp: chrono::Utc::now().timestamp_millis() as f64,
                    channel: channel_name.clone(),
                    samples,
                };
                match app_handle.emit("muse-eeg", &sample) {
                    Ok(()) => {
                        packets_emitted += 1;
                        // Record sample if recording is active (fast path: checks bool)
                        replay::maybe_record_sample(&sample).await;
                    }
                    Err(e) => {
                        emit_errors += 1;
                        if emit_errors <= 5 {
                            println!("[muse] eeg_task: EMIT_ERROR channel={channel_name} err={e}");
                            push_proof_event(
                                "muse",
                                "emit_error",
                                &format!("channel={channel_name} err={e}"),
                            )
                            .await;
                        }
                    }
                }
            } else {
                packets_dropped += 1;
                if packets_dropped <= 10 {
                    println!(
                        "[muse] eeg_task: dropped len={} uuid={} channel={} hex={}",
                        data.len(),
                        notification.uuid,
                        channel_name,
                        hex::encode(data)
                    );
                }
            }

            // Report stats every 2 seconds to STDOUT
            if last_report.elapsed() >= std::time::Duration::from_secs(2) {
                let elapsed = last_report.elapsed().as_secs_f64();
                let rate = packets_emitted as f64 / elapsed;
                println!(
                    "[muse] eeg_emit_stats emitted={packets_emitted} dropped={packets_dropped} emit_errors={emit_errors} rate={rate:.0}pkt/s total={packets_total} recent_sizes={last_sizes:?}"
                );
                push_proof_event("muse", "eeg_emit_stats", &format!(
                    "emitted={packets_emitted} dropped={packets_dropped} emit_errors={emit_errors} rate={rate:.0}pkt/s total={packets_total}"
                )).await;
                // Reset interval counters
                packets_emitted = 0;
                packets_dropped = 0;
                emit_errors = 0;
                last_report = std::time::Instant::now();
            }
        }
        let detail = format!("total_packets={packets_total} total_emit_errors={emit_errors}");
        println!("[muse] eeg_task: exited {detail}");
        push_proof_event("muse", "stream_exit", &detail).await;
    });

    println!("[muse] eeg_stream: status=started");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_eeg_stream(app: AppHandle) -> Result<(), String> {
    println!("[muse] eeg_stream: status=stopping");
    let mut streaming = MUSE_STREAMING.lock().await;
    *streaming = false;

    let stored = MUSE_PERIPHERAL.lock().await;
    if let Some(peripheral) = stored.as_ref() {
        let chars = peripheral.characteristics();
        // Send halt command (length-prefixed)
        if let Some(control) = chars
            .iter()
            .find(|c| uuid_prefix_match(&c.uuid, &MUSE_CONTROL_PREFIX))
        {
            let cmd = muse_cmd("h");
            let _ = peripheral
                .write(control, &cmd, WriteType::WithoutResponse)
                .await;
        }
    }

    emit_muse_state(&app, BleState::Connected).await;
    push_proof_event("muse", "stream_stop", "eeg").await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_muse_state() -> Result<MuseConnectionState, String> {
    let (streaming, stored) = lock_streaming_then_stored(&MUSE_STREAMING, &MUSE_PERIPHERAL).await;

    if let Some(peripheral) = stored.as_ref() {
        let connected = peripheral.is_connected().await.unwrap_or(false);
        let name = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name);
        Ok(MuseConnectionState {
            connected,
            device_name: name,
            streaming: connected && *streaming,
        })
    } else {
        Ok(MuseConnectionState {
            connected: false,
            device_name: None,
            streaming: false,
        })
    }
}

/// Subscribes to whichever optional streams the headset has. A stream that is missing, refuses or
/// doesn't answer is logged and skipped; EEG goes on without it.
pub async fn subscribe_optional_streams(peripheral: &Peripheral) {
    let chars = peripheral.characteristics();
    reset_subscribed();
    for (prefix, stream) in &OPTIONAL_STREAMS {
        let Some(ch) = chars.iter().find(|c| uuid_prefix_match(&c.uuid, prefix)) else {
            println!("[muse] optional_stream: missing stream={stream:?}");
            push_proof_event("muse", "optional_missing", &format!("{stream:?}")).await;
            continue;
        };
        let subscribed =
            tokio::time::timeout(OPTIONAL_SUBSCRIBE_TIMEOUT, peripheral.subscribe(ch)).await;
        let detail = format!("uuid={} stream={stream:?}", ch.uuid);
        let event = if matches!(subscribed, Ok(Ok(()))) {
            if let Ok(mut set) = SUBSCRIBED.lock() {
                note_subscribed(&mut set, *stream);
            }
            "optional_subscribed"
        } else {
            "optional_subscribe_failed"
        };
        println!("[muse] {event} {detail}");
        push_proof_event("muse", event, &detail).await;
    }
}

/// Decodes one packet of an optional stream. None if it is shorter than a full packet.
pub fn decode_optional(
    stream: OptionalStream,
    data: &[u8],
    timestamp: f64,
) -> Option<OptionalEvent> {
    if data.len() < MUSE_PACKET_BYTES {
        return None;
    }
    let sequence = u16::from_be_bytes([data[0], data[1]]);
    let motion = |sensor: &str, unit: &str, scale| {
        OptionalEvent::Motion(MotionSample {
            timestamp,
            sensor: sensor.to_string(),
            unit: unit.to_string(),
            sequence,
            samples: parse_imu_samples(data, scale),
        })
    };
    Some(match stream {
        OptionalStream::Aux(channel) => OptionalEvent::Aux(EegSample {
            timestamp,
            channel: channel.to_string(),
            samples: parse_eeg_packet(data),
        }),
        OptionalStream::Accelerometer => motion("accelerometer", "g", ACCELEROMETER_G_PER_LSB),
        OptionalStream::Gyroscope => motion("gyroscope", "deg/s", GYROSCOPE_DPS_PER_LSB),
        OptionalStream::Ppg(channel) => OptionalEvent::Ppg(PpgSample {
            timestamp,
            channel: channel.to_string(),
            sequence,
            samples: parse_ppg_samples(data),
        }),
    })
}

/// After the sequence number: 3 samples of big-endian signed 16-bit x, y, z.
fn parse_imu_samples(data: &[u8], scale: f64) -> Vec<[f64; 3]> {
    let axis = |at: usize| scale * f64::from(i16::from_be_bytes([data[at], data[at + 1]]));
    (0..3)
        .map(|sample| {
            let at = 2 + sample * 6;
            [axis(at), axis(at + 2), axis(at + 4)]
        })
        .collect()
}

/// After the sequence number: 6 big-endian unsigned 24-bit counts.
fn parse_ppg_samples(data: &[u8]) -> Vec<u32> {
    data[2..MUSE_PACKET_BYTES]
        .chunks_exact(3)
        .map(|b| u32::from_be_bytes([0, b[0], b[1], b[2]]))
        .collect()
}

/// Emits one optional packet and records it when a recording is open. `AUX` is recorded with
/// the EEG channels; `AUX_L` is emitted but never recorded.
async fn emit_optional(app: &AppHandle, stream: OptionalStream, data: &[u8]) {
    let timestamp = chrono::Utc::now().timestamp_millis() as f64;
    let emitted = match decode_optional(stream, data, timestamp) {
        Some(OptionalEvent::Aux(sample)) => {
            let r = app.emit("muse-eeg", &sample);
            replay::maybe_record_sample(&sample).await;
            r
        }
        Some(OptionalEvent::Motion(sample)) => {
            let r = app.emit("muse-motion", &sample);
            replay::maybe_record_motion(&sample).await;
            r
        }
        Some(OptionalEvent::Ppg(sample)) => {
            let r = app.emit("muse-ppg", &sample);
            replay::maybe_record_ppg(&sample).await;
            r
        }
        None => Ok(()),
    };
    if let Err(e) = emitted {
        println!("[muse] optional_emit: error={e}");
    }
}

pub fn muse_cmd(cmd: &str) -> Vec<u8> {
    let cmd_bytes = cmd.as_bytes();
    let len = (cmd_bytes.len() + 1) as u8; // +1 for trailing 0x0a
    let mut buf = Vec::with_capacity(cmd_bytes.len() + 2);
    buf.push(len);
    buf.extend_from_slice(cmd_bytes);
    buf.push(0x0a);
    buf
}

/// Parse a Muse EEG packet: 16-bit timestamp followed by 12x 12-bit samples
fn parse_eeg_packet(data: &[u8]) -> Vec<f64> {
    let mut samples = Vec::with_capacity(12);

    // Skip first 2 bytes (timestamp), then parse 12-bit samples
    // Each pair of 3 bytes contains 2 samples (12 bits each)
    let sample_bytes = &data[2..];
    let mut bit_offset = 0;

    for _ in 0..12 {
        let byte_idx = bit_offset / 8;
        let bit_idx = bit_offset % 8;

        if byte_idx + 1 >= sample_bytes.len() {
            break;
        }

        let raw = if bit_idx <= 4 {
            (((sample_bytes[byte_idx] as u16) << 8 | sample_bytes[byte_idx + 1] as u16)
                >> (4 - bit_idx))
                & 0x0FFF
        } else {
            let triple = (sample_bytes[byte_idx] as u32) << 16
                | (sample_bytes[byte_idx + 1] as u32) << 8
                | if byte_idx + 2 < sample_bytes.len() {
                    sample_bytes[byte_idx + 2] as u32
                } else {
                    0
                };
            ((triple >> (12 - (bit_idx - 4))) & 0x0FFF) as u16
        };

        // Convert to microvolts: 0.48828125 * (raw - 2048)
        let uv = 0.48828125 * (raw as f64 - 2048.0);
        samples.push(uv);

        bit_offset += 12;
    }

    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(text: &str) -> Uuid {
        Uuid::parse_str(text).unwrap()
    }

    /// A packet in the headset's format: sequence 0x0102, then 18 bytes filled from `payload`.
    fn packet(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0x01, 0x02];
        data.extend_from_slice(payload);
        data.resize(MUSE_PACKET_BYTES, 0);
        data
    }

    #[test]
    fn the_published_uuids_route_to_their_streams() {
        let uuids = [
            ("273e0002", OptionalStream::Aux("AUX_L")),
            ("273e0007", OptionalStream::Aux("AUX")),
            ("273e0009", OptionalStream::Gyroscope),
            ("273e000a", OptionalStream::Accelerometer),
            ("273e000f", OptionalStream::Ppg("ambient")),
            ("273e0010", OptionalStream::Ppg("infrared")),
            ("273e0011", OptionalStream::Ppg("red")),
        ];
        for (prefix, stream) in uuids {
            let full = uuid(&format!("{prefix}-4c4d-454d-96be-f03bac821358"));
            assert_eq!(optional_stream(&full), Some(stream), "{prefix}");
            // The suffix varies by model, so only the prefix counts.
            let other = uuid(&format!("{prefix}-0000-1000-8000-00805f9b34fb"));
            assert_eq!(optional_stream(&other), Some(stream), "{prefix}");
        }
        for eeg in ["273e0003", "273e0004", "273e0005", "273e0006"] {
            let full = uuid(&format!("{eeg}-4c4d-454d-96be-f03bac821358"));
            assert_eq!(optional_stream(&full), None);
            assert!(is_data_characteristic(&full));
        }
        let control = uuid("273e0001-4c4d-454d-96be-f03bac821358");
        assert_eq!(optional_stream(&control), None);
        assert!(!is_data_characteristic(&control));
        assert!(is_control_characteristic(&control));
    }

    #[test]
    fn an_eeg_packet_decodes_twelve_samples_at_the_published_scale() {
        // Pairs 0x000 0xFFF, then 0x800 0x801, then padding at the midpoint.
        let mut payload = vec![0x00, 0x0f, 0xff, 0x80, 0x08, 0x01];
        payload.extend([0x80, 0x08, 0x00].repeat(4));
        let samples = parse_eeg_packet(&packet(&payload));
        assert_eq!(samples.len(), 12);
        assert_eq!(samples[0], -1000.0);
        assert_eq!(samples[1], 0.48828125 * 2047.0);
        assert_eq!(samples[2], 0.0);
        assert_eq!(samples[3], 0.48828125);
        assert!(samples[4..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn aux_uses_the_eeg_packet_on_the_channel_the_frontend_reads() {
        let data = packet(&[0x00, 0x0f, 0xff]);
        let event = decode_optional(OptionalStream::Aux("AUX"), &data, 7.0).unwrap();
        let OptionalEvent::Aux(sample) = event else {
            panic!("{event:?}")
        };
        assert_eq!((sample.channel.as_str(), sample.timestamp), ("AUX", 7.0));
        assert_eq!(sample.samples, parse_eeg_packet(&data));
    }

    #[test]
    fn motion_packets_decode_signed_axes_with_sequence_and_scale() {
        // x, y, z for three samples: +1 g, -1 LSB, 0; then two all-zero samples except a max.
        let payload = [
            0x40, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x7f, 0xff, 0x80, 0x00,
        ];
        let data = packet(&payload);
        let Some(OptionalEvent::Motion(acc)) =
            decode_optional(OptionalStream::Accelerometer, &data, 1.0)
        else {
            panic!("not motion")
        };
        assert_eq!(
            (acc.sensor.as_str(), acc.unit.as_str()),
            ("accelerometer", "g")
        );
        assert_eq!(acc.sequence, 0x0102);
        assert_eq!(acc.samples.len(), 3);
        assert!((acc.samples[0][0] - 1.0).abs() < 1e-4);
        assert_eq!(acc.samples[0][1], -ACCELEROMETER_G_PER_LSB);
        assert_eq!(acc.samples[0][2], 0.0);
        assert_eq!(acc.samples[1], [0.0, 0.0, 0.0]);
        assert_eq!(acc.samples[2][1], 32767.0 * ACCELEROMETER_G_PER_LSB);
        assert_eq!(acc.samples[2][2], -32768.0 * ACCELEROMETER_G_PER_LSB);

        let Some(OptionalEvent::Motion(gyro)) =
            decode_optional(OptionalStream::Gyroscope, &data, 1.0)
        else {
            panic!("not motion")
        };
        assert_eq!(
            (gyro.sensor.as_str(), gyro.unit.as_str()),
            ("gyroscope", "deg/s")
        );
        assert_eq!(gyro.samples[0][1], -GYROSCOPE_DPS_PER_LSB);
    }

    #[test]
    fn ppg_packets_decode_six_raw_counts() {
        let payload = [
            0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x12, 0x34, 0x56, 0xff, 0xff, 0xff, 0x00, 0x00,
            0x00, 0x7f, 0x00, 0x80,
        ];
        let data = packet(&payload);
        let event = decode_optional(OptionalStream::Ppg("infrared"), &data, 2.0);
        let Some(OptionalEvent::Ppg(ppg)) = event else {
            panic!("{event:?}")
        };
        assert_eq!(ppg.channel, "infrared");
        assert_eq!(ppg.sequence, 0x0102);
        assert_eq!(ppg.samples, [1, 0x010000, 0x123456, 0xffffff, 0, 0x7f0080]);
    }

    #[test]
    fn only_the_back_electrode_and_real_streams_are_listed_as_subscribed() {
        let mut set = replay::StreamSet::default();
        for (_, stream) in &OPTIONAL_STREAMS {
            note_subscribed(&mut set, *stream);
            note_subscribed(&mut set, *stream);
        }
        assert!(set.aux);
        assert_eq!(set.ppg.len(), 3);
        assert_eq!(set.motion.len(), 2);
        let mut left_only = replay::StreamSet::default();
        note_subscribed(&mut left_only, OptionalStream::Aux("AUX_L"));
        assert_eq!(left_only, replay::StreamSet::default());
    }

    #[test]
    fn short_packets_are_skipped_not_fatal() {
        for stream in OPTIONAL_STREAMS.map(|(_, stream)| stream) {
            assert_eq!(decode_optional(stream, &[0; 19], 0.0), None);
            assert_eq!(decode_optional(stream, &[], 0.0), None);
        }
    }

    #[test]
    fn event_payloads_keep_their_field_names() {
        let motion = MotionSample {
            timestamp: 1.0,
            sensor: "gyroscope".into(),
            unit: "deg/s".into(),
            sequence: 2,
            samples: vec![[0.5, 0.0, -0.5]],
        };
        assert_eq!(
            serde_json::to_value(&motion).unwrap(),
            serde_json::json!({"timestamp": 1.0, "sensor": "gyroscope", "unit": "deg/s",
                               "sequence": 2, "samples": [[0.5, 0.0, -0.5]]})
        );
        let ppg = PpgSample {
            timestamp: 1.0,
            channel: "red".into(),
            sequence: 2,
            samples: vec![3],
        };
        assert_eq!(
            serde_json::to_value(&ppg).unwrap(),
            serde_json::json!({"timestamp": 1.0, "channel": "red", "sequence": 2, "samples": [3]})
        );
    }

    #[test]
    fn the_gatt_log_holds_names_and_uuids_in_a_private_dated_file() {
        use std::os::unix::fs::PermissionsExt;
        let service = uuid("0000fe8d-0000-1000-8000-00805f9b34fb");
        let found = [
            (service, uuid("273e0007-4c4d-454d-96be-f03bac821358")),
            (service, uuid("0000abcd-0000-1000-8000-00805f9b34fb")),
        ];
        let text = gatt_log_text("T", &found);
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains(
            "T service=0000fe8d-0000-1000-8000-00805f9b34fb \
             characteristic=273e0007-4c4d-454d-96be-f03bac821358 name=aux_right\n"
        ));
        assert!(text.contains("name=unknown\n"));

        let dir = std::env::temp_dir().join(format!("nexus-muse-{}", Uuid::new_v4()));
        let path = append_gatt_log(&dir, &found).unwrap();
        append_gatt_log(&dir, &found).unwrap();
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(path.ends_with(format!("muse-gatt-{day}.log")));
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 6);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preset_commands_are_length_prefixed() {
        assert_eq!(muse_cmd("p50"), [0x04, b'p', b'5', b'0', 0x0a]);
        assert_eq!(muse_cmd("p20"), [0x04, b'p', b'2', b'0', 0x0a]);
    }

    /// The generation counter is process-wide; tests that read or bump it run one at a time.
    static GEN_TESTS: Mutex<()> = Mutex::const_new(());

    #[tokio::test]
    async fn a_stale_caller_leaves_streaming_and_the_peripheral_alone() {
        let _serial = GEN_TESTS.lock().await;
        let streaming = Mutex::new(true);
        let stored = Mutex::new(Some(7u8));
        let started = bump_conn_gen();
        bump_conn_gen(); // a newer connect or disconnect happened
        assert!(!gen_is_current(started));
        assert_eq!(
            take_if_current(started, &streaming, &stored).await,
            Err(SUPERSEDED)
        );
        assert!(*streaming.lock().await, "a live stream is not stopped");
        assert_eq!(*stored.lock().await, Some(7));
        let now = conn_gen();
        assert_eq!(take_if_current(now, &streaming, &stored).await, Ok(Some(7)));
        assert!(!*streaming.lock().await);
    }

    #[tokio::test]
    async fn a_caller_overtaken_while_waiting_for_the_lock_touches_nothing() {
        let _serial = GEN_TESTS.lock().await;
        let streaming = Mutex::new(true);
        let stored = Mutex::new(Some(3u8));
        let started = conn_gen();
        let held = stored.lock().await;
        let fut = take_if_current(started, &streaming, &stored);
        tokio::pin!(fut);
        // Parked on `stored` with `streaming` held; a newer operation bumps now.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut fut)
                .await
                .is_err()
        );
        bump_conn_gen();
        drop(held);
        assert_eq!(fut.await, Err(SUPERSEDED));
        assert!(*streaming.lock().await);
        assert_eq!(*stored.lock().await, Some(3));
    }

    #[tokio::test]
    async fn install_replaces_under_the_guard_and_bumps_once() {
        let _serial = GEN_TESTS.lock().await;
        let stored = Mutex::new(Some(1u8));
        let before = conn_gen();
        let seen = std::cell::Cell::new(0u8);
        let old = install_peripheral(&stored, 2u8, |n| seen.set(*n)).await;
        assert_eq!(old, Some(1));
        assert_eq!(seen.get(), 2);
        assert_eq!(*stored.lock().await, Some(2));
        assert!(conn_gen() > before);
    }

    #[tokio::test]
    async fn state_reads_lock_streaming_before_stored() {
        let streaming = Mutex::new(false);
        let stored = Mutex::new(Some(1u8));
        let held = streaming.lock().await;
        let blocked = tokio::time::timeout(
            std::time::Duration::from_millis(30),
            lock_streaming_then_stored(&streaming, &stored),
        )
        .await;
        assert!(blocked.is_err());
        // Waiting on streaming must not have taken `stored` (no inversion).
        assert!(stored.try_lock().is_ok());
        drop(held);
        let (_s, st) = lock_streaming_then_stored(&streaming, &stored).await;
        assert_eq!(*st, Some(1));
    }

    #[tokio::test]
    async fn a_held_lock_hits_the_whole_body_bound() {
        let _serial = GEN_TESTS.lock().await;
        let streaming = Mutex::new(false);
        let stored = Mutex::new(Some(1u8));
        let _held = streaming.lock().await; // a hung stop_eeg_stream
        let started = conn_gen();
        let result = bounded(std::time::Duration::from_millis(50), async {
            let _ = take_if_current(started, &streaming, &stored).await;
            Ok(())
        })
        .await;
        assert_eq!(result, Err(DISCONNECT_TIMEOUT_TEXT.to_string()));
        assert_eq!(*stored.lock().await, Some(1), "nothing was taken");
    }

    #[tokio::test]
    async fn teardown_guard_makes_a_reconnect_wait() {
        let g = TEARDOWN.lock().await;
        let waited =
            tokio::time::timeout(std::time::Duration::from_millis(30), TEARDOWN.lock()).await;
        assert!(waited.is_err());
        drop(g);
        assert!(TEARDOWN.try_lock().is_ok());
    }

    #[tokio::test]
    async fn a_slow_future_times_out_and_a_fast_one_passes() {
        let slow = bounded(std::time::Duration::from_millis(20), async {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            Ok(())
        })
        .await;
        assert_eq!(slow, Err(DISCONNECT_TIMEOUT_TEXT.to_string()));
        assert_eq!(
            bounded(std::time::Duration::from_secs(1), async { Ok(()) }).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn a_leftover_shadow_link_is_disconnected_and_cleared_on_success() {
        let shadow = std::sync::Mutex::new(Some(9u8));
        let ran = std::sync::atomic::AtomicBool::new(false);
        let ok = disconnect_shadow(&shadow, std::time::Duration::from_secs(1), |n| {
            ran.store(n == 9, std::sync::atomic::Ordering::SeqCst);
            async { Ok(()) }
        })
        .await;
        assert!(ok && ran.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(*shadow.lock().unwrap(), None);
    }

    #[tokio::test]
    async fn a_failed_or_hung_shadow_disconnect_keeps_the_shadow() {
        let shadow = std::sync::Mutex::new(Some(9u8));
        let failed = disconnect_shadow(&shadow, std::time::Duration::from_secs(1), |_| async {
            Err("x".to_string())
        })
        .await;
        assert!(!failed);
        let hung = disconnect_shadow(&shadow, std::time::Duration::from_millis(20), |_| async {
            std::future::pending::<Result<(), String>>().await
        })
        .await;
        assert!(!hung);
        assert_eq!(*shadow.lock().unwrap(), Some(9));
        let empty: std::sync::Mutex<Option<u8>> = std::sync::Mutex::new(None);
        assert!(
            disconnect_shadow(&empty, std::time::Duration::from_millis(20), |_| async {
                Ok(())
            })
            .await
        );
    }
}
