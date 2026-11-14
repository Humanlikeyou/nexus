use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Canonical BLE connection states — single source of truth for frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BleState {
    Idle,
    Scanning,
    Discovered,
    Connecting,
    Connected,
    Streaming,
}

/// A timestamped proof event for the debug/proof log.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BleProofEvent {
    pub timestamp: f64,
    pub device: String,
    pub event_type: String,
    pub detail: String,
}

const PROOF_LOG_CAP: usize = 50;

lazy_static::lazy_static! {
    pub static ref PROOF_LOG: Arc<Mutex<VecDeque<BleProofEvent>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(PROOF_LOG_CAP)));
    pub static ref PROOF_MODE_ENABLED: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
}

/// Push a proof event into the ring buffer (if proof mode is on).
pub async fn push_proof_event(device: &str, event_type: &str, detail: &str) {
    let enabled = *PROOF_MODE_ENABLED.lock().await;
    if !enabled {
        return;
    }
    let event = BleProofEvent {
        timestamp: chrono::Utc::now().timestamp_millis() as f64,
        device: device.to_string(),
        event_type: event_type.to_string(),
        detail: detail.to_string(),
    };
    let mut log = PROOF_LOG.lock().await;
    if log.len() >= PROOF_LOG_CAP {
        log.pop_front();
    }
    log.push_back(event);
}

// ── Tauri commands ──────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn set_proof_mode(enabled: bool) -> Result<(), String> {
    let mut mode = PROOF_MODE_ENABLED.lock().await;
    *mode = enabled;
    if !enabled {
        PROOF_LOG.lock().await.clear();
    }
    println!("[ble_common] proof_mode={enabled}");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_proof_log() -> Result<Vec<BleProofEvent>, String> {
    let log = PROOF_LOG.lock().await;
    Ok(log.iter().cloned().collect())
}
