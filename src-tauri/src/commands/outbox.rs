use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use super::events::get_events_db;
use super::telegram::load_config;

static WORKER_STARTED: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref GATEWAY_SIM_DOWN: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
}

/// Gateway ACK contract: POST /events must return { "received": ["event_id", ...] }
/// Only event IDs listed in `received` are considered delivered.
#[derive(Debug, Clone, Deserialize)]
struct GatewayAck {
    received: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboxStatus {
    pub gateway_online: bool,
    pub queue_depth: u64,
    pub last_delivered_at: Option<String>,
    pub last_error: Option<String>,
    pub total_delivered: u64,
    /// Items older than OUTBOX_EXPIRY_HOURS: kept on disk, never sent.
    pub expired: u64,
}

/// Pending items older than this are marked `expired` (kept, not sent, not deleted).
pub const OUTBOX_EXPIRY_HOURS: i64 = 24;

/// Mark pending items created before `now - OUTBOX_EXPIRY_HOURS` as expired. Returns the count.
/// `created_at` is parsed as a timestamp (RFC 3339 with any offset, or SQLite
/// `YYYY-MM-DD HH:MM:SS` as UTC); it is never compared as a string, since mixed offsets
/// or formats do not order lexically. Unparsable rows are left pending.
pub(crate) fn expire_stale(
    conn: &rusqlite::Connection,
    now: chrono::DateTime<chrono::Utc>,
) -> rusqlite::Result<usize> {
    let cutoff = now - chrono::Duration::hours(OUTBOX_EXPIRY_HOURS);
    let stale: Vec<i64> = {
        let mut stmt =
            conn.prepare("SELECT outbox_id, created_at FROM outbox WHERE status = 'pending'")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok())
            .filter(|(_, created)| parse_created_at(created).is_some_and(|t| t < cutoff))
            .map(|(id, _)| id)
            .collect()
    };
    let mut n = 0;
    for id in stale {
        n += conn.execute(
            "UPDATE outbox SET status = 'expired' WHERE outbox_id = ?1 AND status = 'pending'",
            rusqlite::params![id],
        )?;
    }
    Ok(n)
}

/// Parse a stored `created_at`: RFC 3339 (any offset, any fraction) or SQLite
/// `datetime('now')` format, which is UTC.
pub(crate) fn parse_created_at(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&chrono::Utc));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(t.and_utc());
        }
    }
    None
}

/// Why delivery must not POST this tick, if any. Checked before any row is loaded, so a
/// paused tick never posts, never changes a row's status, and never bumps `attempts`.
pub(crate) fn delivery_pause_reason(online: bool, outbound_disabled: bool) -> Option<&'static str> {
    if !online {
        Some("Gateway offline")
    } else if outbound_disabled {
        Some("Delivery paused (NEXUS_OUTBOUND_DISABLED)")
    } else {
        None
    }
}

fn count_status(conn: &rusqlite::Connection, status: &str) -> u64 {
    conn.query_row(
        "SELECT COUNT(*) FROM outbox WHERE status = ?1",
        rusqlite::params![status],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Start the background delivery worker (called once from setup).
/// Uses tauri::async_runtime::spawn so it works from the sync setup() context.
/// Guarded by AtomicBool to ensure only one worker ever runs.
pub fn start_delivery_worker(app: AppHandle) {
    if WORKER_STARTED.swap(true, Ordering::SeqCst) {
        println!("[outbox] delivery_worker: already running, skipping");
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Small delay to let the app finish initializing
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        println!("[outbox] delivery_worker: started interval_secs=15");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            if let Err(e) = delivery_tick(&app).await {
                println!("[outbox] tick_error: {e}");
            }
        }
    });
}

async fn delivery_tick(app: &AppHandle) -> Result<(), String> {
    let config = load_config();
    let gateway_url = config
        .gateway_url
        .unwrap_or_else(|| "http://127.0.0.1:18789".to_string());
    let batch_size = config.gateway_batch_size.unwrap_or(50);
    let retry_base = config.gateway_retry_base_secs.unwrap_or(5) as u64;
    let retry_max = config.gateway_retry_max_secs.unwrap_or(300) as u64;
    let auth_token = config.gateway_auth_token.clone();

    // Check simulated down state
    let sim_down = *GATEWAY_SIM_DOWN.lock().await;

    // Health check
    let online = if sim_down {
        false
    } else {
        match reqwest::Client::new()
            .get(format!("{}/health", gateway_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    };

    // Load queue stats
    let (queue_depth, total_delivered, last_delivered_at, expired) =
        tokio::task::spawn_blocking(|| -> Result<(u64, u64, Option<String>, u64), String> {
            let conn = get_events_db()?;
            // Stale items are expired (kept, not sent) before anything else; works while paused too.
            let _ = expire_stale(&conn, chrono::Utc::now());
            let expired = count_status(&conn, "expired");
            let pending: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let delivered: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let last: Option<String> = conn
                .query_row(
                    "SELECT created_at FROM outbox WHERE status = 'delivered' ORDER BY outbox_id DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            Ok((pending, delivered, last, expired))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    if let Some(reason) = delivery_pause_reason(online, super::telegram::outbound_disabled()) {
        let status = OutboxStatus {
            gateway_online: online,
            queue_depth,
            last_delivered_at,
            last_error: Some(reason.to_string()),
            total_delivered,
            expired,
        };
        let _ = app.emit("outbox-status", &status);
        return Ok(());
    }

    // Load pending outbox items
    let gateway_url_clone = gateway_url.clone();
    let auth_clone = auth_token.clone();

    let result = tokio::task::spawn_blocking(move || -> Result<Vec<(i64, String)>, String> {
        let conn = get_events_db()?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT outbox_id, event_ids FROM outbox WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= ?1) ORDER BY outbox_id ASC LIMIT ?2",
            )
            .map_err(|e| format!("Prepare failed: {e}"))?;
        let rows: Vec<(i64, String)> = stmt
            .query_map(rusqlite::params![now, batch_size], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| format!("Query failed: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if result.is_empty() {
        let status = OutboxStatus {
            gateway_online: true,
            queue_depth: 0,
            last_delivered_at,
            last_error: None,
            total_delivered,
            expired,
        };
        let _ = app.emit("outbox-status", &status);
        return Ok(());
    }

    // Load event payloads for the batch
    let outbox_ids: Vec<i64> = result.iter().map(|(id, _)| *id).collect();
    let all_event_ids: Vec<String> = result
        .iter()
        .flat_map(|(_, ids_json)| serde_json::from_str::<Vec<String>>(ids_json).unwrap_or_default())
        .collect();

    let events_payload = tokio::task::spawn_blocking(move || -> Result<Vec<serde_json::Value>, String> {
        let conn = get_events_db()?;
        let mut events = Vec::new();
        for event_id in &all_event_ids {
            let row: Option<String> = conn
                .query_row(
                    "SELECT json_object('id', id, 'session_id', session_id, 'timestamp', timestamp, 'source', source, 'metric', metric, 'value', value, 'unit', unit, 'confidence', confidence, 'tags', tags) FROM events WHERE id = ?1",
                    rusqlite::params![event_id],
                    |row| row.get(0),
                )
                .ok();
            if let Some(json_str) = row {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    events.push(val);
                }
            }
        }
        Ok(events)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // POST to gateway
    let mut request = reqwest::Client::new()
        .post(format!("{}/events", gateway_url_clone))
        .json(&events_payload)
        .timeout(std::time::Duration::from_secs(10));

    if let Some(token) = auth_clone {
        if !token.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
    }

    let delivery_result = request.send().await;

    match delivery_result {
        Ok(resp) if resp.status().is_success() => {
            // Parse ACK: gateway MUST return { "received": ["event_id", ...] }
            let body = resp.text().await.unwrap_or_default();
            let ack: Option<GatewayAck> = serde_json::from_str(&body).ok();

            let received_set: std::collections::HashSet<String> = match &ack {
                Some(a) => a.received.iter().cloned().collect(),
                None => std::collections::HashSet::new(),
            };

            if received_set.is_empty() {
                // No valid ACK — do NOT mark delivered
                let error_text = format!(
                    "No ACK: gateway returned 2xx but body missing or invalid received[] (body={})",
                    if body.len() > 200 {
                        &body[..200]
                    } else {
                        &body
                    }
                );
                println!("[outbox] {}", error_text);
                apply_backoff(&outbox_ids, &error_text, retry_base, retry_max).await?;

                let status = OutboxStatus {
                    gateway_online: true,
                    queue_depth,
                    last_delivered_at,
                    last_error: Some(error_text),
                    total_delivered,
                    expired,
                };
                let _ = app.emit("outbox-status", &status);
            } else {
                // Check each outbox row: only mark delivered if ALL its event_ids are ACKed
                let mut acked_outbox_ids: Vec<i64> = Vec::new();
                let mut unacked_outbox_ids: Vec<i64> = Vec::new();

                for (outbox_id, ids_json) in &result {
                    let event_ids: Vec<String> = serde_json::from_str(ids_json).unwrap_or_default();
                    let all_acked = !event_ids.is_empty()
                        && event_ids.iter().all(|id| received_set.contains(id));
                    if all_acked {
                        acked_outbox_ids.push(*outbox_id);
                    } else {
                        unacked_outbox_ids.push(*outbox_id);
                    }
                }

                // Mark fully-ACKed outbox rows as delivered
                if !acked_outbox_ids.is_empty() {
                    let ids = acked_outbox_ids.clone();
                    tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let conn = get_events_db()?;
                        for id in &ids {
                            conn.execute(
                                "UPDATE outbox SET status = 'delivered' WHERE outbox_id = ?1",
                                rusqlite::params![id],
                            )
                            .map_err(|e| format!("Update failed: {e}"))?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                }

                // Backoff unACKed rows
                if !unacked_outbox_ids.is_empty() {
                    let error_text = format!(
                        "Partial ACK: {}/{} outbox items not confirmed",
                        unacked_outbox_ids.len(),
                        result.len()
                    );
                    println!("[outbox] {}", error_text);
                    apply_backoff(&unacked_outbox_ids, &error_text, retry_base, retry_max).await?;
                }

                let acked_count = acked_outbox_ids.len() as u64;
                println!(
                    "[outbox] delivered: acked={} unacked={} events_sent={}",
                    acked_count,
                    unacked_outbox_ids.len(),
                    events_payload.len()
                );

                let status = OutboxStatus {
                    gateway_online: true,
                    queue_depth: queue_depth.saturating_sub(acked_count),
                    last_delivered_at: if acked_count > 0 {
                        Some(chrono::Utc::now().to_rfc3339())
                    } else {
                        last_delivered_at
                    },
                    last_error: if !unacked_outbox_ids.is_empty() {
                        Some(format!(
                            "Partial ACK: {}/{} unconfirmed",
                            unacked_outbox_ids.len(),
                            result.len()
                        ))
                    } else {
                        None
                    },
                    total_delivered: total_delivered + acked_count,
                    expired,
                };
                let _ = app.emit("outbox-status", &status);
            }
        }
        Ok(resp) => {
            let error_text = format!("HTTP {}", resp.status());
            apply_backoff(&outbox_ids, &error_text, retry_base, retry_max).await?;

            let status = OutboxStatus {
                gateway_online: true,
                queue_depth,
                last_delivered_at,
                last_error: Some(error_text),
                total_delivered,
                expired,
            };
            let _ = app.emit("outbox-status", &status);
        }
        Err(e) => {
            let error_text = e.to_string();
            apply_backoff(&outbox_ids, &error_text, retry_base, retry_max).await?;

            let status = OutboxStatus {
                gateway_online: false,
                queue_depth,
                last_delivered_at,
                last_error: Some(error_text),
                total_delivered,
                expired,
            };
            let _ = app.emit("outbox-status", &status);
        }
    }

    Ok(())
}

async fn apply_backoff(
    outbox_ids: &[i64],
    error: &str,
    retry_base: u64,
    retry_max: u64,
) -> Result<(), String> {
    let ids = outbox_ids.to_vec();
    let error = error.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = get_events_db()?;
        for id in &ids {
            let attempts: u32 = conn
                .query_row(
                    "SELECT attempts FROM outbox WHERE outbox_id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let new_attempts = attempts + 1;
            let backoff_secs = std::cmp::min(
                retry_max,
                retry_base * 2u64.pow(new_attempts.min(10)),
            );
            // Add jitter (0-25% of backoff)
            let jitter = (backoff_secs as f64 * 0.25 * rand_simple()) as u64;
            let next_retry = chrono::Utc::now()
                + chrono::Duration::seconds((backoff_secs + jitter) as i64);
            conn.execute(
                "UPDATE outbox SET attempts = ?1, next_retry_at = ?2, last_error = ?3 WHERE outbox_id = ?4",
                rusqlite::params![new_attempts, next_retry.to_rfc3339(), error, id],
            )
            .map_err(|e| format!("Backoff update failed: {e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Simple deterministic-ish jitter without pulling in a rand crate.
fn rand_simple() -> f64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (t % 1000) as f64 / 1000.0
}

// ── Tauri Commands ──────────────────────────────────────────────

#[tauri::command]
pub async fn get_outbox_status() -> Result<OutboxStatus, String> {
    tokio::task::spawn_blocking(|| -> Result<OutboxStatus, String> {
        let conn = get_events_db()?;
        if let Ok(n) = expire_stale(&conn, chrono::Utc::now()) {
            if n > 0 {
                println!("[outbox] expired {n} item(s) older than {OUTBOX_EXPIRY_HOURS}h (kept, not sent)");
            }
        }
        let expired = count_status(&conn, "expired");
        let pending: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'pending'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let delivered: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let last: Option<String> = conn
            .query_row(
                "SELECT created_at FROM outbox WHERE status = 'delivered' ORDER BY outbox_id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        let last_error: Option<String> = conn
            .query_row(
                "SELECT last_error FROM outbox WHERE status = 'pending' AND last_error IS NOT NULL ORDER BY outbox_id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        let sim_down = false; // can't easily check async from sync context

        Ok(OutboxStatus {
            gateway_online: !sim_down && pending == 0,
            queue_depth: pending,
            last_delivered_at: last,
            last_error,
            total_delivered: delivered,
            expired,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn simulate_gateway_down() -> Result<(), String> {
    let mut sim = GATEWAY_SIM_DOWN.lock().await;
    *sim = true;
    println!("[outbox] simulate: gateway_down=true");
    Ok(())
}

#[tauri::command]
pub async fn simulate_gateway_up() -> Result<(), String> {
    let mut sim = GATEWAY_SIM_DOWN.lock().await;
    *sim = false;
    println!("[outbox] simulate: gateway_down=false");
    Ok(())
}

#[cfg(test)]
mod expiry_tests {
    use super::*;

    fn db(rows: &[(&str, &str)]) -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE outbox (outbox_id INTEGER PRIMARY KEY AUTOINCREMENT, event_ids TEXT NOT NULL, destination TEXT NOT NULL, payload_hash TEXT NOT NULL, created_at TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, next_retry_at TEXT, last_error TEXT, status TEXT NOT NULL DEFAULT 'pending');").unwrap();
        for (created, status) in rows {
            c.execute(
                "INSERT INTO outbox (event_ids, destination, payload_hash, created_at, status) VALUES ('[]', 'gateway', 'h', ?1, ?2)",
                rusqlite::params![created, status],
            )
            .unwrap();
        }
        c
    }

    #[test]
    fn expires_only_pending_items_older_than_24h_and_keeps_them() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-29T02:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let c = db(&[
            ("2026-03-05T10:53:07.732766+00:00", "pending"),
            ("2026-10-28T01:59:59+00:00", "pending"),
            ("2026-10-28T02:00:01+00:00", "pending"),
            ("2026-03-01T00:00:00+00:00", "delivered"),
        ]);
        assert_eq!(expire_stale(&c, now).unwrap(), 2);
        assert_eq!(count_status(&c, "expired"), 2);
        assert_eq!(count_status(&c, "pending"), 1);
        assert_eq!(count_status(&c, "delivered"), 1);
        let total: u64 = c
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 4, "nothing deleted");
        assert_eq!(expire_stale(&c, now).unwrap(), 0, "idempotent");
    }

    fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
        parse_created_at(s).unwrap()
    }

    #[test]
    fn expire_stale_parses_real_and_mixed_created_at_formats() {
        let now = at("2026-10-29T02:00:00Z");
        // Real saved format (events.rs: Utc::now().to_rfc3339(), e.g. the live
        // "2026-03-05T10:53:07.732766+00:00") plus Z, +09:00 and SQLite datetime('now').
        let real_new = chrono::Utc::now().to_rfc3339();
        let c = db(&[
            ("2026-10-28T01:59:59.732766+00:00", "pending"), // 24h00m01s old: expire
            ("2026-10-28T02:00:59.000001+00:00", "pending"), // 23h59m: keep
            ("2026-10-28T10:30:00+09:00", "pending"),        // 01:30Z, 24.5h: expire
            ("2026-10-28T11:30:00+09:00", "pending"), // 02:30Z, 23.5h: keep (lexically < cutoff!)
            ("2026-10-28 01:00:00", "pending"),       // SQLite UTC, 25h: expire
            ("2026-10-28 03:00:00", "pending"),       // 23h: keep
            ("2026-10-28T01:00:00Z", "pending"),      // 25h: expire
            ("not a date", "pending"),                // unparsable: left pending
            (real_new.as_str(), "pending"),           // just enqueued: keep
        ]);
        assert_eq!(expire_stale(&c, now).unwrap(), 4);
        let expired: Vec<String> = c
            .prepare("SELECT created_at FROM outbox WHERE status='expired' ORDER BY outbox_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            expired,
            vec![
                "2026-10-28T01:59:59.732766+00:00",
                "2026-10-28T10:30:00+09:00",
                "2026-10-28 01:00:00",
                "2026-10-28T01:00:00Z"
            ]
        );
        assert_eq!(count_status(&c, "pending"), 5);
    }

    #[test]
    fn kill_switch_pauses_delivery_before_any_row_is_touched() {
        assert_eq!(delivery_pause_reason(true, false), None);
        assert_eq!(
            delivery_pause_reason(true, true),
            Some("Delivery paused (NEXUS_OUTBOUND_DISABLED)")
        );
        assert_eq!(delivery_pause_reason(false, true), Some("Gateway offline"));
        // The gate sits before row loading and POSTs in delivery_tick (source check), so a
        // paused tick sends nothing, rows stay pending, and attempts are not counted.
        let src = include_str!("outbox.rs");
        let tick = &src[src.find("async fn delivery_tick").expect("delivery_tick")..];
        let gate = tick.find("delivery_pause_reason(online").expect("gate");
        let load = tick.find("Load pending outbox items").expect("load");
        let post = tick.find(".post(").expect("post");
        let bump = tick
            .find("attempts = attempts + 1")
            .or_else(|| tick.find("attempts + 1"))
            .expect("attempts bump");
        assert!(gate < load && gate < post && gate < bump);
    }
}
