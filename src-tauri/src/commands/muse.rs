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

/// Match a characteristic UUID by its first 4 bytes (works across all Muse models).
fn uuid_prefix_match(uuid: &Uuid, prefix: &[u8; 4]) -> bool {
    uuid.as_bytes()[..4] == prefix[..]
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

#[derive(Serialize, Clone)]
pub struct MuseDevice {
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
}

#[derive(Serialize, Clone)]
pub struct EegSample {
    pub timestamp: f64,
    pub channel: String,
    pub samples: Vec<f64>,
}

#[derive(Serialize, Clone)]
pub struct MuseConnectionState {
    pub connected: bool,
    pub device_name: Option<String>,
    pub streaming: bool,
}

#[tauri::command]
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
pub async fn connect_muse(app: AppHandle, address: String) -> Result<MuseConnectionState, String> {
    println!("[muse] connect: target_id={address}");
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

    let mut stored = MUSE_PERIPHERAL.lock().await;
    *stored = Some(peripheral);

    emit_muse_state(&app, BleState::Connected).await;

    Ok(MuseConnectionState {
        connected: true,
        device_name: name,
        streaming: false,
    })
}

#[tauri::command]
pub async fn disconnect_muse(app: AppHandle) -> Result<(), String> {
    println!("[muse] disconnect: status=starting");
    let mut streaming = MUSE_STREAMING.lock().await;
    *streaming = false;

    let mut stored = MUSE_PERIPHERAL.lock().await;
    if let Some(peripheral) = stored.take() {
        peripheral.disconnect().await.map_err(|e| e.to_string())?;
        println!("[muse] disconnect: status=done");
    }

    emit_muse_state(&app, BleState::Idle).await;
    Ok(())
}

#[tauri::command]
pub async fn start_eeg_stream(app: AppHandle) -> Result<(), String> {
    println!("[muse] eeg_stream: status=starting");
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
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
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
pub async fn get_muse_state() -> Result<MuseConnectionState, String> {
    let stored = MUSE_PERIPHERAL.lock().await;
    let streaming = MUSE_STREAMING.lock().await;

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

/// Encode a Muse control command with the required length-prefix byte.
/// Protocol: [length_of_remaining_bytes, ...command_ascii, 0x0a]
/// e.g. "d" → [0x02, 0x64, 0x0a], "p50" → [0x04, 0x70, 0x35, 0x30, 0x0a]
fn muse_cmd(cmd: &str) -> Vec<u8> {
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
