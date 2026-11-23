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

#[derive(Debug, Clone, Serialize, specta::Type)]
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

/// What one delivery tick reads and writes, injected so the HTTP path can be tested against a
/// local mock gateway and a temp DB. `delivery_tick` passes today's sources.
pub(crate) struct DeliveryDeps<S: FnMut(&OutboxStatus)> {
    /// Opens the events DB (a fresh connection per blocking step, like get_events_db).
    pub open_db: OpenDb,
    pub gateway_url: String,
    pub batch_size: u32,
    pub retry_base: u64,
    pub retry_max: u64,
    pub auth_token: Option<String>,
    /// NEXUS_OUTBOUND_DISABLED for this session.
    pub outbound_disabled: bool,
    /// simulate_gateway_down() state.
    pub sim_down: bool,
    /// Receives each status the tick reports (production: the "outbox-status" event).
    pub emit_status: S,
    /// Wall clock (production: chrono::Utc::now).
    pub now: Clock,
}

async fn delivery_tick(app: &AppHandle) -> Result<(), String> {
    let config = load_config();
    let deps = DeliveryDeps {
        open_db: Arc::new(get_events_db),
        gateway_url: config
            .gateway_url
            .unwrap_or_else(|| "http://127.0.0.1:18789".to_string()),
        batch_size: config.gateway_batch_size.unwrap_or(50),
        retry_base: config.gateway_retry_base_secs.unwrap_or(5) as u64,
        retry_max: config.gateway_retry_max_secs.unwrap_or(300) as u64,
        auth_token: config.gateway_auth_token.clone(),
        outbound_disabled: super::telegram::outbound_disabled(),
        sim_down: *GATEWAY_SIM_DOWN.lock().await,
        emit_status: |status: &OutboxStatus| {
            let _ = app.emit("outbox-status", status);
        },
        now: Arc::new(chrono::Utc::now),
    };
    delivery_tick_with(deps).await
}

/// The spec for talking to the gateway (OpenClaw, third party: never the launch token).
/// A loopback gateway (the default) gets no proxy and no redirects, so a call that must stay on the
/// Mac cannot leave it. A user-configured non-loopback gateway keeps the client defaults it always
/// had (environment proxy, default redirect policy): the choice is parsed, never a string prefix.
fn gateway_spec(url: &str) -> super::shared::http::HttpSpec {
    if super::loopback_http::is_loopback_url(url) {
        super::shared::http::HttpSpec::loopback()
    } else {
        super::shared::http::HttpSpec::default()
    }
}

fn gateway_client(url: &str) -> reqwest::Client {
    super::shared::http::builder(gateway_spec(url))
        .build()
        .unwrap_or_else(|_| super::loopback_http::safe_fallback_client())
}

pub(crate) async fn delivery_tick_with<S: FnMut(&OutboxStatus)>(
    mut deps: DeliveryDeps<S>,
) -> Result<(), String> {
    let gateway_url = deps.gateway_url.clone();
    let batch_size = deps.batch_size;
    let retry_base = deps.retry_base;
    let retry_max = deps.retry_max;
    let auth_token = deps.auth_token.clone();
    let sim_down = deps.sim_down;
    let open_db = deps.open_db.clone();
    let clock = deps.now.clone();

    // Health check
    let online = if sim_down {
        false
    } else {
        match gateway_client(&gateway_url)
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
    let (db, now) = (open_db.clone(), clock.clone());
    let (queue_depth, total_delivered, last_delivered_at, expired) =
        tokio::task::spawn_blocking(move || -> Result<(u64, u64, Option<String>, u64), String> {
            let conn = db()?;
            // Stale items are expired (kept, not sent) before anything else; works while paused too.
            let _ = expire_stale(&conn, now());
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

    if let Some(reason) = delivery_pause_reason(online, deps.outbound_disabled) {
        let status = OutboxStatus {
            gateway_online: online,
            queue_depth,
            last_delivered_at,
            last_error: Some(reason.to_string()),
            total_delivered,
            expired,
        };
        (deps.emit_status)(&status);
        return Ok(());
    }

    // Load pending outbox items
    let gateway_url_clone = gateway_url.clone();
    let auth_clone = auth_token.clone();

    let (db, at) = (open_db.clone(), clock.clone());
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<(i64, String)>, String> {
        let conn = db()?;
        let now = at().to_rfc3339();
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
            // Nothing due this tick, but rows waiting on retry backoff are still queued.
            queue_depth,
            last_delivered_at,
            last_error: None,
            total_delivered,
            expired,
        };
        (deps.emit_status)(&status);
        return Ok(());
    }

    // Load event payloads for the batch
    let outbox_ids: Vec<i64> = result.iter().map(|(id, _)| *id).collect();
    let all_event_ids: Vec<String> = result
        .iter()
        .flat_map(|(_, ids_json)| serde_json::from_str::<Vec<String>>(ids_json).unwrap_or_default())
        .collect();

    let db = open_db.clone();
    let events_payload = tokio::task::spawn_blocking(move || -> Result<Vec<serde_json::Value>, String> {
        let conn = db()?;
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
    let mut request = gateway_client(&gateway_url_clone)
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
                let error_text = no_ack_error(&body);
                println!("[outbox] {}", error_text);
                apply_backoff(
                    &open_db,
                    &clock,
                    &outbox_ids,
                    &error_text,
                    retry_base,
                    retry_max,
                )
                .await?;

                let status = OutboxStatus {
                    gateway_online: true,
                    queue_depth,
                    last_delivered_at,
                    last_error: Some(error_text),
                    total_delivered,
                    expired,
                };
                (deps.emit_status)(&status);
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
                    let db = open_db.clone();
                    tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let conn = db()?;
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
                    apply_backoff(
                        &open_db,
                        &clock,
                        &unacked_outbox_ids,
                        &error_text,
                        retry_base,
                        retry_max,
                    )
                    .await?;
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
                        Some(clock().to_rfc3339())
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
                (deps.emit_status)(&status);
            }
        }
        Ok(resp) => {
            let error_text = format!("HTTP {}", resp.status());
            apply_backoff(
                &open_db,
                &clock,
                &outbox_ids,
                &error_text,
                retry_base,
                retry_max,
            )
            .await?;

            let status = OutboxStatus {
                gateway_online: true,
                queue_depth,
                last_delivered_at,
                last_error: Some(error_text),
                total_delivered,
                expired,
            };
            (deps.emit_status)(&status);
        }
        Err(e) => {
            let error_text = e.to_string();
            apply_backoff(
                &open_db,
                &clock,
                &outbox_ids,
                &error_text,
                retry_base,
                retry_max,
            )
            .await?;

            let status = OutboxStatus {
                gateway_online: false,
                queue_depth,
                last_delivered_at,
                last_error: Some(error_text),
                total_delivered,
                expired,
            };
            (deps.emit_status)(&status);
        }
    }

    Ok(())
}

/// Error text for a 2xx without a valid ACK. The body is cut to 200 chars on a char
/// boundary (byte slicing panicked on multi-byte bodies and killed the delivery tick).
fn no_ack_error(body: &str) -> String {
    let shown: String = body.chars().take(200).collect();
    format!("No ACK: gateway returned 2xx but body missing or invalid received[] (body={shown})")
}

type OpenDb = Arc<dyn Fn() -> Result<rusqlite::Connection, String> + Send + Sync>;
type Clock = Arc<dyn Fn() -> chrono::DateTime<chrono::Utc> + Send + Sync>;

async fn apply_backoff(
    open_db: &OpenDb,
    clock: &Clock,
    outbox_ids: &[i64],
    error: &str,
    retry_base: u64,
    retry_max: u64,
) -> Result<(), String> {
    let ids = outbox_ids.to_vec();
    let error = error.to_string();
    let (open_db, clock) = (open_db.clone(), clock.clone());
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = open_db()?;
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
            let next_retry = clock() + chrono::Duration::seconds((backoff_secs + jitter) as i64);
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
#[specta::specta]
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
#[specta::specta]
pub async fn simulate_gateway_down() -> Result<(), String> {
    let mut sim = GATEWAY_SIM_DOWN.lock().await;
    *sim = true;
    println!("[outbox] simulate: gateway_down=true");
    Ok(())
}

#[tauri::command]
#[specta::specta]
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
    fn no_ack_error_truncates_multibyte_bodies_without_panicking() {
        let body = format!("{}é{}", "x".repeat(199), "y".repeat(50));
        let text = no_ack_error(&body);
        assert!(text.ends_with(&format!("(body={}é)", "x".repeat(199))));
        assert_eq!(
            no_ack_error("{}"),
            "No ACK: gateway returned 2xx but body missing or invalid received[] (body={})"
        );
        let emoji = "📦".repeat(300);
        assert_eq!(no_ack_error(&emoji).matches('📦').count(), 200);
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

/// delivery_tick_with against a scripted mock gateway on 127.0.0.1:0 and a temp events DB.
/// No env vars, no real endpoints: every flag and the clock are passed in.
#[cfg(test)]
mod delivery_tests {
    use super::*;

    #[test]
    fn gateway_spec_is_loopback_only_for_loopback_urls() {
        for u in [
            "http://127.0.0.1:18789",
            "http://localhost:18789",
            "http://[::1]:18789",
        ] {
            let s = gateway_spec(u);
            assert!(s.no_proxy && s.no_redirects && !s.launch_token, "{u}");
        }
        for u in [
            "https://gateway.example.com",
            "http://10.0.0.5:18789",
            "http://127.0.0.1@evil.example",
            "not a url",
        ] {
            let s = gateway_spec(u);
            assert!(!s.no_proxy && !s.no_redirects && !s.https_only, "{u}");
            assert!(!s.launch_token, "{u}");
        }
    }

    #[tokio::test]
    async fn loopback_gateway_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b).await;
            let _ = s
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/x\r\nContent-Length: 0\r\n\r\n")
                .await;
        });
        let url = format!("http://{addr}");
        let r = gateway_client(&url).get(&url).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 302);
    }

    #[test]
    fn outbox_prod_code_has_no_bare_client_new() {
        let src = include_str!("outbox.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(!prod.contains("Client::new()"));
    }

    #[test]
    fn n8n_status_prod_code_has_no_bare_client_new() {
        let src = include_str!("n8n_status.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        assert!(!prod.contains("Client::new()"));
    }
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};

    const FAKE_TOKEN: &str = "FAKE-TEST-TOKEN-not-a-real-secret";

    #[derive(Debug, Clone)]
    struct Req {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Req {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    struct Mock {
        url: String,
        log: Arc<std::sync::Mutex<Vec<Req>>>,
    }

    impl Mock {
        fn requests(&self) -> Vec<Req> {
            self.log.lock().unwrap().clone()
        }
        fn posts(&self) -> Vec<Req> {
            self.requests()
                .into_iter()
                .filter(|r| r.path == "/events")
                .collect()
        }
    }

    /// Answers GET /health with `health` and POST /events with (`status`, `body`); records
    /// every request. One request per connection (Connection: close).
    fn mock(health: u16, events: (u16, &str)) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (events_status, events_body) = (events.0, events.1.to_string());
        let rec = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut headers = Vec::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(':') {
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
                let len = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let (code, resp) = if path == "/health" {
                    (health, String::from("{}"))
                } else {
                    (events_status, events_body.clone())
                };
                rec.lock().unwrap().push(Req {
                    method,
                    path,
                    headers,
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                let _ = write!(
                    stream,
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                    resp.len()
                );
            }
        });
        Mock { url, log }
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-10-29T02:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn ago(secs: i64) -> String {
        (now() - chrono::Duration::seconds(secs)).to_rfc3339()
    }

    fn open(path: &Path) -> Result<rusqlite::Connection, String> {
        let c = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        super::super::events::init_db(&c)?;
        Ok(c)
    }

    struct Db {
        path: PathBuf,
    }

    impl Drop for Db {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
        }
    }

    /// Temp events DB on the real schema. Rows: (event ids, created_at, next_retry_at, status).
    fn db(rows: &[(&[&str], &str, Option<&str>, &str)]) -> Db {
        let dir =
            std::env::temp_dir().join(format!("nexus-outbox-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.db");
        let c = open(&path).unwrap();
        for (ids, created, next, status) in rows {
            for id in *ids {
                c.execute(
                    "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, created_at) VALUES (?1, 's', ?2, 'muse', 'calm', 0.5, ?2)",
                    rusqlite::params![id, created],
                )
                .unwrap();
            }
            c.execute(
                "INSERT INTO outbox (event_ids, destination, payload_hash, created_at, next_retry_at, status) VALUES (?1, 'gateway', 'h', ?2, ?3, ?4)",
                rusqlite::params![serde_json::to_string(ids).unwrap(), created, next, status],
            )
            .unwrap();
        }
        Db { path }
    }

    #[derive(Debug, PartialEq)]
    struct Row {
        status: String,
        attempts: u32,
        next_retry_at: Option<String>,
        last_error: Option<String>,
    }

    fn rows(db: &Db) -> Vec<Row> {
        let c = open(&db.path).unwrap();
        let mut stmt = c
            .prepare(
                "SELECT status, attempts, next_retry_at, last_error FROM outbox ORDER BY outbox_id",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok(Row {
                status: r.get(0)?,
                attempts: r.get(1)?,
                next_retry_at: r.get(2)?,
                last_error: r.get(3)?,
            })
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
    }

    struct Tick<'a> {
        url: &'a str,
        token: Option<&'a str>,
        outbound_disabled: bool,
        sim_down: bool,
    }

    fn tick<'a>(url: &'a str) -> Tick<'a> {
        Tick {
            url,
            token: None,
            outbound_disabled: false,
            sim_down: false,
        }
    }

    async fn run(db: &Db, t: Tick<'_>) -> Vec<OutboxStatus> {
        let mut statuses = Vec::new();
        let path = db.path.clone();
        delivery_tick_with(DeliveryDeps {
            open_db: Arc::new(move || open(&path)),
            gateway_url: t.url.to_string(),
            batch_size: 50,
            retry_base: 5,
            retry_max: 300,
            auth_token: t.token.map(String::from),
            outbound_disabled: t.outbound_disabled,
            sim_down: t.sim_down,
            emit_status: |s: &OutboxStatus| statuses.push(s.clone()),
            now: Arc::new(now),
        })
        .await
        .unwrap();
        statuses
    }

    fn untouched(r: &Row) -> bool {
        r.status == "pending"
            && r.attempts == 0
            && r.next_retry_at.is_none()
            && r.last_error.is_none()
    }

    /// apply_backoff: min(retry_max, retry_base * 2^attempts) plus up to 25 % jitter.
    fn assert_backed_off(r: &Row, attempts: u32, error_prefix: &str) {
        assert_eq!(
            (r.status.as_str(), r.attempts),
            ("pending", attempts),
            "{r:?}"
        );
        let next = parse_created_at(r.next_retry_at.as_deref().unwrap()).unwrap();
        let secs = std::cmp::min(300, 5 * 2u64.pow(attempts)) as i64;
        assert!(next >= now() + chrono::Duration::seconds(secs), "{r:?}");
        assert!(
            next <= now() + chrono::Duration::milliseconds(secs * 1250),
            "{r:?}"
        );
        assert!(
            r.last_error.as_deref().unwrap().starts_with(error_prefix),
            "{r:?}"
        );
    }

    #[tokio::test]
    async fn queue_depth_counts_rows_waiting_on_backoff_but_not_expired() {
        let later = (now() + chrono::Duration::minutes(10)).to_rfc3339();
        let db = db(&[
            (&["e1"], &ago(600), Some(later.as_str()), "pending"),
            (&["e2"], &ago(300), Some(later.as_str()), "pending"),
            (&["e3"], &ago(60 * 60 * 30), None, "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":[]}"#));
        let s = run(&db, tick(&m.url)).await;
        let st = s.last().unwrap();
        assert_eq!(st.expired, 1, "30h-old row expired");
        assert_eq!(
            st.queue_depth, 2,
            "both backed-off rows still count as queued"
        );
        assert!(m.posts().is_empty(), "nothing due, nothing posted");
    }

    #[tokio::test]
    async fn full_ack_delivers_every_row() {
        let db = db(&[
            (&["e0"], &ago(7200), None, "delivered"),
            (&["e1"], &ago(3600), None, "pending"),
            (&["e2", "e3"], &ago(3600), None, "pending"),
            (&["e4"], &ago(60), None, "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["e1","e2","e3","e4"]}"#));
        let s = run(&db, tick(&m.url)).await;
        let st = s.last().unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((st.total_delivered, st.queue_depth), (1 + 3, 0));
        assert_eq!(st.last_error, None);
        assert_eq!(
            st.last_delivered_at.as_deref(),
            Some(now().to_rfc3339().as_str())
        );
        assert!(rows(&db)
            .iter()
            .all(|r| r.status == "delivered" && r.attempts == 0));
        let posts = m.posts();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].method, "POST");
        let sent: Vec<serde_json::Value> = serde_json::from_str(&posts[0].body).unwrap();
        let ids: Vec<&str> = sent.iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["e1", "e2", "e3", "e4"]);
    }

    #[tokio::test]
    async fn partial_ack_delivers_only_fully_acked_rows() {
        let db = db(&[
            (&["e1"], &ago(60), None, "pending"),
            (&["e2", "e3"], &ago(60), None, "pending"),
            (&["e4"], &ago(60), None, "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["e1","e2","e4"]}"#));
        let st = run(&db, tick(&m.url)).await.pop().unwrap();
        let r = rows(&db);
        assert_eq!(r[0].status, "delivered");
        assert_backed_off(&r[1], 1, "Partial ACK: 1/3 outbox items not confirmed");
        assert_eq!(r[2].status, "delivered");
        assert_eq!(
            st.last_error.as_deref(),
            Some("Partial ACK: 1/3 unconfirmed")
        );
        assert_eq!((st.total_delivered, st.queue_depth), (2, 1));
    }

    #[tokio::test]
    async fn two_xx_without_a_valid_ack_delivers_nothing_and_backs_off() {
        let long_multibyte = format!(r#"{{"note":"{}"}}"#, "é📦".repeat(150));
        for body in [
            "",
            "{}",
            r#"{"received":"e1"}"#,
            r#"{"received":[]}"#,
            "not json",
            long_multibyte.as_str(),
        ] {
            let db = db(&[
                (&["e1"], &ago(60), None, "pending"),
                (&["e2"], &ago(60), None, "pending"),
            ]);
            let m = mock(200, (200, body));
            let st = run(&db, tick(&m.url)).await.pop().unwrap();
            for r in rows(&db) {
                assert_backed_off(&r, 1, "No ACK: gateway returned 2xx");
            }
            assert_eq!((st.total_delivered, st.queue_depth), (0, 2), "{body}");
            let err = st.last_error.unwrap();
            assert!(err.starts_with("No ACK:"), "{err}");
            assert!(
                err.chars().count() <= 90 + 200,
                "body cut to 200 chars: {err}"
            );
        }
    }

    #[tokio::test]
    async fn http_500_backs_off_with_the_status() {
        let db = db(&[(&["e1"], &ago(60), None, "pending")]);
        let m = mock(200, (500, "oops"));
        let st = run(&db, tick(&m.url)).await.pop().unwrap();
        assert_backed_off(&rows(&db)[0], 1, "HTTP 500");
        assert!(
            st.last_error.as_deref().unwrap().starts_with("HTTP 500"),
            "{st:?}"
        );
        assert!(st.gateway_online);
        // A second failure doubles the delay (attempts 2 → 5 * 2^2 = 20 s).
        let c = open(&db.path).unwrap();
        c.execute("UPDATE outbox SET next_retry_at = NULL", [])
            .unwrap();
        run(&db, tick(&m.url)).await;
        assert_backed_off(&rows(&db)[0], 2, "HTTP 500");
    }

    #[tokio::test]
    async fn health_down_posts_nothing_and_leaves_rows() {
        let db = db(&[(&["e1"], &ago(60), None, "pending")]);
        let m = mock(503, (200, r#"{"received":["e1"]}"#));
        let st = run(&db, tick(&m.url)).await.pop().unwrap();
        assert_eq!(
            m.requests()
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/health"]
        );
        assert!(untouched(&rows(&db)[0]));
        assert!(!st.gateway_online);
        assert_eq!(st.last_error.as_deref(), Some("Gateway offline"));
        // Simulated down: not even a health request.
        let m = mock(200, (200, r#"{"received":["e1"]}"#));
        run(
            &db,
            Tick {
                sim_down: true,
                ..tick(&m.url)
            },
        )
        .await;
        assert!(m.requests().is_empty());
        assert!(untouched(&rows(&db)[0]));
    }

    #[tokio::test]
    async fn outbound_disabled_sends_nothing_and_leaves_rows() {
        let db = db(&[
            (&["e1"], &ago(60), None, "pending"),
            (&["e2"], &ago(60), None, "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["e1","e2"]}"#));
        let st = run(
            &db,
            Tick {
                outbound_disabled: true,
                ..tick(&m.url)
            },
        )
        .await
        .pop()
        .unwrap();
        assert!(m.posts().is_empty(), "zero requests to /events");
        assert!(rows(&db).iter().all(untouched));
        assert_eq!(
            st.last_error.as_deref(),
            Some("Delivery paused (NEXUS_OUTBOUND_DISABLED)")
        );
        assert_eq!(st.queue_depth, 2);
    }

    #[tokio::test]
    async fn expired_rows_are_never_posted_or_counted() {
        let sqlite_25h = (now() - chrono::Duration::hours(25))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let db = db(&[
            (&["old1"], &ago(25 * 3600), None, "pending"), // RFC 3339 +00:00
            (&["old2"], &sqlite_25h, None, "pending"),     // SQLite datetime('now')
            (&["new"], &ago(60), None, "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["old1","old2","new"]}"#));
        let paused = run(
            &db,
            Tick {
                outbound_disabled: true,
                ..tick(&m.url)
            },
        )
        .await
        .pop()
        .unwrap();
        assert_eq!((paused.queue_depth, paused.expired), (1, 2));
        let st = run(&db, tick(&m.url)).await.pop().unwrap();
        let posts = m.posts();
        assert_eq!(posts.len(), 1);
        let sent: Vec<serde_json::Value> = serde_json::from_str(&posts[0].body).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["id"], "new");
        let status: Vec<String> = rows(&db).into_iter().map(|r| r.status).collect();
        assert_eq!(status, vec!["expired", "expired", "delivered"]);
        assert_eq!((st.expired, st.total_delivered), (2, 1));
    }

    /// The cutoff is strict (created_at < now − 24h expires): 23:59:59 and exactly 24:00:00 old
    /// stay pending; 24:00:01 expires. Same for SQLite-format timestamps.
    #[tokio::test]
    async fn the_24h_boundary() {
        let sq = |secs: i64| {
            (now() - chrono::Duration::seconds(secs))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        let day = 24 * 3600;
        let db = db(&[
            (&["a"], &ago(day - 1), None, "pending"),
            (&["b"], &ago(day), None, "pending"),
            (&["c"], &ago(day + 1), None, "pending"),
            (&["d"], &sq(day - 1), None, "pending"),
            (&["e"], &sq(day), None, "pending"),
            (&["f"], &sq(day + 1), None, "pending"),
        ]);
        let m = mock(200, (200, "{}"));
        let st = run(
            &db,
            Tick {
                outbound_disabled: true,
                ..tick(&m.url)
            },
        )
        .await
        .pop()
        .unwrap();
        let status: Vec<String> = rows(&db).into_iter().map(|r| r.status).collect();
        assert_eq!(
            status,
            vec!["pending", "pending", "expired", "pending", "pending", "expired"]
        );
        assert_eq!((st.queue_depth, st.expired), (4, 2));
    }

    #[tokio::test]
    async fn rows_waiting_for_their_retry_are_skipped() {
        let soon = (now() + chrono::Duration::seconds(60)).to_rfc3339();
        let past = (now() - chrono::Duration::seconds(1)).to_rfc3339();
        let db = db(&[
            (&["later"], &ago(60), Some(&soon), "pending"),
            (&["due"], &ago(60), Some(&past), "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["later","due"]}"#));
        run(&db, tick(&m.url)).await;
        let sent: Vec<serde_json::Value> = serde_json::from_str(&m.posts()[0].body).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["id"], "due");
        let r = rows(&db);
        assert_eq!((r[0].status.as_str(), r[0].attempts), ("pending", 0));
        assert_eq!(r[1].status, "delivered");
    }

    /// When every pending row is waiting for its retry the batch is empty, but the status still
    /// reports the real pending count (matching get_outbox_status), not 0.
    #[tokio::test]
    async fn empty_batch_reports_real_queue_depth_with_backed_off_rows_pending() {
        let soon = (now() + chrono::Duration::seconds(60)).to_rfc3339();
        let db = db(&[
            (&["e1"], &ago(60), Some(&soon), "pending"),
            (&["e2"], &ago(60), Some(&soon), "pending"),
        ]);
        let m = mock(200, (200, r#"{"received":["e1","e2"]}"#));
        let st = run(&db, tick(&m.url)).await.pop().unwrap();
        assert!(m.posts().is_empty());
        assert_eq!(
            rows(&db).iter().filter(|r| r.status == "pending").count(),
            2
        );
        assert_eq!(st.queue_depth, 2, "backed-off rows are still queued");
        assert_eq!(st.last_error, None);
    }

    #[tokio::test]
    async fn bearer_header_only_when_a_token_is_set() {
        for (token, expect) in [
            (Some(FAKE_TOKEN), Some(format!("Bearer {FAKE_TOKEN}"))),
            (Some(""), None),
            (None, None),
        ] {
            let db = db(&[(&["e1"], &ago(60), None, "pending")]);
            let m = mock(200, (200, r#"{"received":["e1"]}"#));
            run(
                &db,
                Tick {
                    token,
                    ..tick(&m.url)
                },
            )
            .await;
            let reqs = m.requests();
            let post = reqs.iter().find(|r| r.path == "/events").unwrap();
            assert_eq!(
                post.header("authorization").map(String::from),
                expect,
                "{token:?}"
            );
            let health = reqs.iter().find(|r| r.path == "/health").unwrap();
            assert_eq!(health.method, "GET");
            assert_eq!(
                health.header("authorization"),
                None,
                "health check never carries the token"
            );
        }
    }
}
