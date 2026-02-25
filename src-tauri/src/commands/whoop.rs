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

// Whoop custom GATT service and characteristics
#[allow(dead_code)]
const WHOOP_SERVICE: Uuid = Uuid::from_u128(0x61080000_8d6d_82b8_614a_1c8cb0f8dcc6);
const CMD_TO_STRAP: Uuid = Uuid::from_u128(0x61080002_8d6d_82b8_614a_1c8cb0f8dcc6);
const CMD_FROM_STRAP: Uuid = Uuid::from_u128(0x61080003_8d6d_82b8_614a_1c8cb0f8dcc6);
const EVENTS_FROM_STRAP: Uuid = Uuid::from_u128(0x61080004_8d6d_82b8_614a_1c8cb0f8dcc6);
const DATA_FROM_STRAP: Uuid = Uuid::from_u128(0x61080005_8d6d_82b8_614a_1c8cb0f8dcc6);

// Standard BLE Heart Rate Service
#[allow(dead_code)]
const HR_SERVICE: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);
const HR_MEASUREMENT: Uuid = Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

lazy_static::lazy_static! {
    static ref WHOOP_ADAPTER: Arc<Mutex<Option<btleplug::platform::Adapter>>> = Arc::new(Mutex::new(None));
    static ref WHOOP_DISCOVERED: Arc<Mutex<HashMap<String, Peripheral>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref WHOOP_PERIPHERAL: Arc<Mutex<Option<Peripheral>>> = Arc::new(Mutex::new(None));
    static ref WHOOP_STREAMING: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref WHOOP_SCAN_ACTIVE: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref WHOOP_BLE_STATE: Arc<Mutex<BleState>> = Arc::new(Mutex::new(BleState::Idle));
}

async fn emit_whoop_state(app: &AppHandle, state: BleState) {
    *WHOOP_BLE_STATE.lock().await = state.clone();
    let _ = app.emit("whoop-ble-state", &state);
    push_proof_event("whoop", "state_change", &format!("{:?}", state)).await;
}

async fn get_or_init_adapter() -> Result<btleplug::platform::Adapter, String> {
    let mut stored = WHOOP_ADAPTER.lock().await;
    if let Some(adapter) = stored.as_ref() {
        return Ok(adapter.clone());
    }
    println!("[whoop] init: status=starting");
    let manager = Manager::new().await.map_err(|e| {
        println!("[whoop] init: error=manager_failed reason={e}");
        e.to_string()
    })?;
    let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
    let adapter = adapters.into_iter().next().ok_or("No Bluetooth adapter found")?;
    println!("[whoop] init: status=adapter_ready");
    *stored = Some(adapter.clone());
    Ok(adapter)
}

#[derive(Serialize, Clone)]
pub struct WhoopDevice {
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
}

#[derive(Serialize, Clone)]
pub struct HeartRateData {
    pub timestamp: f64,
    pub heart_rate: u16,
    pub rr_interval: Option<f64>,
    pub source: String, // "custom" or "standard"
}

#[derive(Serialize, Clone)]
pub struct WhoopConnectionState {
    pub connected: bool,
    pub device_name: Option<String>,
    pub streaming: bool,
}

#[tauri::command]
pub async fn scan_whoop(app: AppHandle) -> Result<Vec<WhoopDevice>, String> {
    println!("[whoop] scan: status=starting");
    emit_whoop_state(&app, BleState::Scanning).await;
    let adapter = get_or_init_adapter().await?;

    // If a scan is already running, stop it first (idempotent reset)
    {
        let mut active = WHOOP_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[whoop] scan: stopping_previous_scan=true");
            adapter.stop_scan().await.ok();
            *active = false;
        }
    }

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| {
            println!("[whoop] scan: error=start_scan_failed reason={e}");
            e.to_string()
        })?;
    println!("[whoop] scan: status=scanning duration_secs=5");

    tokio::time::sleep(Duration::from_secs(5)).await;

    let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
    println!("[whoop] scan: total_peripherals={}", peripherals.len());

    let mut devices = Vec::new();
    let mut discovered = WHOOP_DISCOVERED.lock().await;
    discovered.clear();

    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            let name = props.local_name.unwrap_or_default();
            let id = p.id().to_string();
            if !name.is_empty() {
                println!(
                    "[whoop] scan: peripheral id={id} name={name:?} rssi={:?}",
                    props.rssi
                );
            }
            if name.to_uppercase().starts_with("WHOOP") {
                println!("[whoop] scan: match=true id={id} name={name}");
                push_proof_event("whoop", "scan_result", &format!("id={id} name={name} rssi={:?}", props.rssi)).await;
                discovered.insert(id.clone(), p);
                devices.push(WhoopDevice {
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
        let mut active = WHOOP_SCAN_ACTIVE.lock().await;
        *active = true;
    }

    // Spawn 30s background timeout to stop the scan (battery safety net)
    let timeout_adapter = adapter.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut active = WHOOP_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[whoop] scan_timeout: stopping_scan after_secs=30");
            timeout_adapter.stop_scan().await.ok();
            *active = false;
        }
    });

    println!(
        "[whoop] scan: status=done whoop_devices={} cached={}",
        devices.len(),
        discovered.len()
    );

    if devices.is_empty() {
        emit_whoop_state(&app, BleState::Idle).await;
    } else {
        emit_whoop_state(&app, BleState::Discovered).await;
    }

    Ok(devices)
}

#[tauri::command]
pub async fn connect_whoop(app: AppHandle, address: String) -> Result<WhoopConnectionState, String> {
    println!("[whoop] connect: target_id={address}");
    emit_whoop_state(&app, BleState::Connecting).await;

    // ---- Step 0: Stop active scan ----
    // Cached peripherals remain valid because we hold Arc references.
    {
        let adapter = get_or_init_adapter().await?;
        let mut active = WHOOP_SCAN_ACTIVE.lock().await;
        if *active {
            println!("[whoop] connect: stopping_active_scan=true");
            adapter.stop_scan().await.ok();
            *active = false;
        }
    }

    // ---- Step 1: Resolve the Peripheral object ----
    // Priority: WHOOP_DISCOVERED cache → adapter peripherals → 5s re-scan
    let peripheral = {
        let discovered = WHOOP_DISCOVERED.lock().await;
        println!(
            "[whoop] resolve: cache_entries={} keys={:?}",
            discovered.len(),
            discovered.keys().collect::<Vec<_>>()
        );

        if let Some(p) = discovered.get(&address) {
            println!("[whoop] resolve: source=cache");
            p.clone()
        } else {
            println!("[whoop] resolve: cache_miss=true fallback=adapter_peripherals");
            drop(discovered);
            let adapter = get_or_init_adapter().await?;
            let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
            println!(
                "[whoop] resolve: adapter_peripherals={} ids={:?}",
                peripherals.len(),
                peripherals
                    .iter()
                    .map(|p| p.id().to_string())
                    .collect::<Vec<_>>()
            );

            if let Some(p) = peripherals.into_iter().find(|p| p.id().to_string() == address) {
                println!("[whoop] resolve: source=adapter");
                p
            } else {
                // Last resort: 5-second re-scan
                println!("[whoop] resolve: adapter_miss=true fallback=rescan duration_secs=5");
                adapter
                    .start_scan(ScanFilter::default())
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::time::sleep(Duration::from_secs(5)).await;
                let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
                adapter.stop_scan().await.ok();

                let ids: Vec<_> = peripherals.iter().map(|p| p.id().to_string()).collect();
                println!("[whoop] resolve: rescan_peripherals={} ids={:?}", ids.len(), ids);

                peripherals
                    .into_iter()
                    .find(|p| p.id().to_string() == address)
                    .ok_or_else(|| {
                        let msg = format!(
                            "Device handle no longer valid (id: {}). Please rescan.",
                            address
                        );
                        println!("[whoop] resolve: error=device_not_found id={address}");
                        msg
                    })?
            }
        }
    };

    // ---- Step 2: BLE connect with retries ----
    let max_retries = 3;
    let mut last_err = String::new();

    for attempt in 1..=max_retries {
        println!("[whoop] ble_connect: attempt={attempt}/{max_retries}");
        push_proof_event("whoop", "connect_attempt", &format!("attempt={attempt}/{max_retries} id={address}")).await;
        match peripheral.connect().await {
            Ok(()) => {
                println!("[whoop] ble_connect: status=success attempt={attempt}");
                push_proof_event("whoop", "connect_success", &format!("attempt={attempt}")).await;
                last_err.clear();
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                println!("[whoop] ble_connect: status=failed attempt={attempt} reason={last_err}");
                push_proof_event("whoop", "connect_failed", &format!("attempt={attempt} reason={last_err}")).await;
                if attempt < max_retries {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    if !last_err.is_empty() {
        let connected = peripheral.is_connected().await.unwrap_or(false);
        if !connected {
            let msg = format!(
                "Connect failed after {max_retries} attempts: {last_err}"
            );
            println!("[whoop] ble_connect: error=retries_exhausted");
            emit_whoop_state(&app, BleState::Discovered).await;
            return Err(msg);
        }
    }

    // ---- Step 3: Service discovery ----
    println!("[whoop] discovery: status=starting");
    peripheral.discover_services().await.map_err(|e| {
        let msg = format!("Service discovery failed: {e}");
        println!("[whoop] discovery: error=failed reason={e}");
        msg
    })?;

    let chars = peripheral.characteristics();
    println!(
        "[whoop] discovery: status=done characteristics={}",
        chars.len()
    );

    let name = peripheral
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|p| p.local_name);

    println!(
        "[whoop] connect: status=success device={:?} id={address}",
        name
    );

    let mut stored = WHOOP_PERIPHERAL.lock().await;
    *stored = Some(peripheral);

    emit_whoop_state(&app, BleState::Connected).await;

    Ok(WhoopConnectionState {
        connected: true,
        device_name: name,
        streaming: false,
    })
}

#[tauri::command]
pub async fn disconnect_whoop(app: AppHandle) -> Result<(), String> {
    println!("[whoop] disconnect: status=starting");
    let mut streaming = WHOOP_STREAMING.lock().await;
    *streaming = false;

    let mut stored = WHOOP_PERIPHERAL.lock().await;
    if let Some(peripheral) = stored.take() {
        peripheral.disconnect().await.map_err(|e| e.to_string())?;
        println!("[whoop] disconnect: status=done");
    }

    emit_whoop_state(&app, BleState::Idle).await;
    Ok(())
}

#[tauri::command]
pub async fn start_hr_stream(app: AppHandle) -> Result<(), String> {
    println!("[whoop] hr_stream: status=starting");
    let stored = WHOOP_PERIPHERAL.lock().await;
    let peripheral = stored.as_ref().ok_or("Whoop not connected")?.clone();
    drop(stored);

    let chars = peripheral.characteristics();

    // Try to subscribe to the custom Whoop data characteristic first
    let _use_custom = if let Some(data_char) = chars.iter().find(|c| c.uuid == DATA_FROM_STRAP) {
        peripheral
            .subscribe(data_char)
            .await
            .map_err(|e| e.to_string())?;
        println!("[whoop] hr_stream: subscribed characteristic=DATA_FROM_STRAP");

        // Also subscribe to command responses and events
        if let Some(cmd_from) = chars.iter().find(|c| c.uuid == CMD_FROM_STRAP) {
            let _ = peripheral.subscribe(cmd_from).await;
            println!("[whoop] hr_stream: subscribed characteristic=CMD_FROM_STRAP");
        }
        if let Some(events) = chars.iter().find(|c| c.uuid == EVENTS_FROM_STRAP) {
            let _ = peripheral.subscribe(events).await;
            println!("[whoop] hr_stream: subscribed characteristic=EVENTS_FROM_STRAP");
        }

        // Send HR broadcast enable command
        if let Some(cmd_to) = chars.iter().find(|c| c.uuid == CMD_TO_STRAP) {
            // Enable heart rate broadcast: category 0x0E
            let cmd = build_whoop_command(0x0E, &[0x01]);
            let _ = peripheral.write(cmd_to, &cmd, WriteType::WithResponse).await;
            println!("[whoop] hr_stream: hr_broadcast_enable=sent");
        }

        true
    } else {
        println!("[whoop] hr_stream: custom_service=not_found fallback=standard_hr");
        false
    };

    // Also try standard HR service as fallback
    if let Some(hr_char) = chars.iter().find(|c| c.uuid == HR_MEASUREMENT) {
        let _ = peripheral.subscribe(hr_char).await;
        println!("[whoop] hr_stream: subscribed characteristic=HR_MEASUREMENT");
    }

    {
        let mut streaming = WHOOP_STREAMING.lock().await;
        *streaming = true;
    }

    emit_whoop_state(&app, BleState::Streaming).await;
    push_proof_event("whoop", "stream_start", "hr").await;

    let app_handle = app.clone();
    tokio::spawn(async move {
        const MAX_BACKOFF_MS: u64 = 60_000; // cap at 60s
        const MAX_RECONNECT_ATTEMPTS: u32 = 10;
        let mut backoff_ms: u64;

        'outer: loop {
            let mut notification_stream = match peripheral.notifications().await {
                Ok(s) => s,
                Err(e) => {
                    println!("[whoop] hr_stream: notification_stream_error={e}");
                    break;
                }
            };

            // Reset backoff on each successful stream start
            backoff_ms = 1000;

            while let Some(notification) = notification_stream.next().await {
                let streaming = WHOOP_STREAMING.lock().await;
                if !*streaming {
                    break 'outer; // user stopped streaming
                }
                drop(streaming);

                let data = &notification.value;

                if notification.uuid == DATA_FROM_STRAP {
                    if let Some(hr_data) = parse_whoop_data_packet(data) {
                        let _ = app_handle.emit("whoop-hr", &hr_data);
                    }
                } else if notification.uuid == HR_MEASUREMENT {
                    if let Some(hr_data) = parse_standard_hr(data) {
                        let _ = app_handle.emit("whoop-hr", &hr_data);
                    }
                }
            }

            // Stream ended — check if user stopped or connection dropped
            let streaming = WHOOP_STREAMING.lock().await;
            if !*streaming {
                break; // user stopped
            }
            drop(streaming);

            // Connection dropped — attempt reconnect with exponential backoff
            println!("[whoop] hr_stream: connection_dropped — starting reconnect");
            emit_whoop_state(&app_handle, BleState::Connecting).await;

            let mut reconnected = false;
            for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
                let streaming = WHOOP_STREAMING.lock().await;
                if !*streaming {
                    break 'outer; // user stopped during backoff
                }
                drop(streaming);

                println!("[whoop] reconnect: attempt={attempt}/{MAX_RECONNECT_ATTEMPTS} backoff_ms={backoff_ms}");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                match peripheral.connect().await {
                    Ok(()) => {
                        // Re-discover services and re-subscribe
                        if peripheral.discover_services().await.is_ok() {
                            let chars = peripheral.characteristics();
                            // Re-subscribe to HR characteristics
                            if let Some(data_char) = chars.iter().find(|c| c.uuid == DATA_FROM_STRAP) {
                                let _ = peripheral.subscribe(data_char).await;
                            }
                            if let Some(hr_char) = chars.iter().find(|c| c.uuid == HR_MEASUREMENT) {
                                let _ = peripheral.subscribe(hr_char).await;
                            }
                            // Re-enable HR broadcast
                            if let Some(cmd_to) = chars.iter().find(|c| c.uuid == CMD_TO_STRAP) {
                                let cmd = build_whoop_command(0x0E, &[0x01]);
                                let _ = peripheral.write(cmd_to, &cmd, WriteType::WithResponse).await;
                            }

                            println!("[whoop] reconnect: status=success attempt={attempt}");
                            emit_whoop_state(&app_handle, BleState::Streaming).await;
                            reconnected = true;
                            break;
                        }
                    }
                    Err(e) => {
                        println!("[whoop] reconnect: status=failed attempt={attempt} reason={e}");
                    }
                }

                // Exponential backoff: double up to max
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }

            if !reconnected {
                println!("[whoop] reconnect: exhausted after {MAX_RECONNECT_ATTEMPTS} attempts");
                let mut streaming = WHOOP_STREAMING.lock().await;
                *streaming = false;
                emit_whoop_state(&app_handle, BleState::Idle).await;
                break;
            }
            // Loop back to 'outer to re-acquire the notification stream
        }

        println!("[whoop] hr_stream: status=task_exited");
    });

    println!("[whoop] hr_stream: status=started");
    Ok(())
}

#[tauri::command]
pub async fn stop_hr_stream(app: AppHandle) -> Result<(), String> {
    println!("[whoop] hr_stream: status=stopping");
    let mut streaming = WHOOP_STREAMING.lock().await;
    *streaming = false;

    emit_whoop_state(&app, BleState::Connected).await;
    push_proof_event("whoop", "stream_stop", "hr").await;
    Ok(())
}

#[tauri::command]
pub async fn get_whoop_state() -> Result<WhoopConnectionState, String> {
    let stored = WHOOP_PERIPHERAL.lock().await;
    let streaming = WHOOP_STREAMING.lock().await;

    if let Some(peripheral) = stored.as_ref() {
        let connected = peripheral.is_connected().await.unwrap_or(false);
        let name = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name);
        Ok(WhoopConnectionState {
            connected,
            device_name: name,
            streaming: connected && *streaming,
        })
    } else {
        Ok(WhoopConnectionState {
            connected: false,
            device_name: None,
            streaming: false,
        })
    }
}

/// Parse a Whoop custom data packet from DATA_FROM_STRAP
/// Format: Header + Unix timestamp (4B LE) + HR (2B) + RR interval (2B) + flags + CRC-32
fn parse_whoop_data_packet(data: &[u8]) -> Option<HeartRateData> {
    // Minimum packet size check
    if data.len() < 12 {
        return None;
    }

    // Skip header bytes (typically starts with 0xAA)
    let offset = if data[0] == 0xAA {
        // Find data payload after header
        6 // Standard header is ~6 bytes
    } else {
        0
    };

    if offset + 8 > data.len() {
        return None;
    }

    // Parse timestamp (4 bytes, little-endian)
    let timestamp = u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as f64
        * 1000.0; // Convert to milliseconds

    // Parse heart rate (2 bytes)
    let heart_rate = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);

    // Parse RR interval (2 bytes)
    let rr_raw = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
    let rr_interval = if rr_raw > 0 {
        Some(rr_raw as f64) // In milliseconds
    } else {
        None
    };

    // Validate reasonable HR range
    if heart_rate == 0 || heart_rate > 250 {
        return None;
    }

    Some(HeartRateData {
        timestamp,
        heart_rate,
        rr_interval,
        source: "custom".to_string(),
    })
}

/// Parse standard BLE Heart Rate Measurement characteristic
fn parse_standard_hr(data: &[u8]) -> Option<HeartRateData> {
    if data.is_empty() {
        return None;
    }

    let flags = data[0];
    let hr_16bit = flags & 0x01 != 0;
    let rr_present = flags & 0x10 != 0;

    let (heart_rate, offset) = if hr_16bit {
        if data.len() < 3 {
            return None;
        }
        (u16::from_le_bytes([data[1], data[2]]), 3)
    } else {
        if data.len() < 2 {
            return None;
        }
        (data[1] as u16, 2)
    };

    let rr_interval = if rr_present && offset + 1 < data.len() {
        let rr = u16::from_le_bytes([data[offset], data[offset + 1]]);
        Some(rr as f64 * 1000.0 / 1024.0) // Convert from 1/1024 sec to ms
    } else {
        None
    };

    Some(HeartRateData {
        timestamp: chrono::Utc::now().timestamp_millis() as f64,
        heart_rate,
        rr_interval,
        source: "standard".to_string(),
    })
}

/// Build a command packet for the Whoop custom GATT protocol
fn build_whoop_command(category: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::new();

    // Header
    packet.push(0xAA);
    packet.push((payload.len() + 3) as u8); // Length
    packet.push(0x00);
    packet.push(0x57); // Whoop identifier
    packet.push(category);

    // Payload
    packet.extend_from_slice(payload);

    // Calculate CRC-32 (polynomial 0x4C11DB7, reflect I/O, init 0x00, XOR out 0xF43F44AC)
    let crc = compute_whoop_crc32(&packet);
    packet.extend_from_slice(&crc.to_le_bytes());

    packet
}

/// Compute CRC-32 with Whoop's custom parameters
fn compute_whoop_crc32(data: &[u8]) -> u32 {
    let poly: u32 = 0x04C11DB7;
    let mut crc: u32 = 0x00000000;

    for &byte in data {
        let mut reflected_byte = byte;
        // Reflect input byte
        reflected_byte = (reflected_byte & 0xF0) >> 4 | (reflected_byte & 0x0F) << 4;
        reflected_byte = (reflected_byte & 0xCC) >> 2 | (reflected_byte & 0x33) << 2;
        reflected_byte = (reflected_byte & 0xAA) >> 1 | (reflected_byte & 0x55) << 1;

        crc ^= (reflected_byte as u32) << 24;
        for _ in 0..8 {
            if crc & 0x80000000 != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
        }
    }

    // Reflect output
    let mut reflected_crc: u32 = 0;
    for i in 0..32 {
        if crc & (1 << i) != 0 {
            reflected_crc |= 1 << (31 - i);
        }
    }

    reflected_crc ^ 0xF43F44AC
}
