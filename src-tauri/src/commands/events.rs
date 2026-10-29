use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn db_path() -> PathBuf {
    nexus_data_dir().join("events.db")
}

fn get_db() -> Result<Connection, String> {
    let path = db_path();
    crate::app_paths::create_private_dir_all(path.parent().unwrap())
        .map_err(|e| format!("Failed to create data dir: {}", e))?;
    crate::app_paths::ensure_private_file(&path)
        .map_err(|e| format!("Failed to create events.db: {}", e))?;

    let conn = Connection::open(&path).map_err(|e| format!("Failed to open events.db: {}", e))?;

    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("Failed to set WAL mode: {}", e))?;
    crate::app_paths::restrict_sqlite_files(&path)
        .map_err(|e| format!("Failed to secure events.db: {}", e))?;

    init_db(&conn)?;
    Ok(conn)
}

pub(crate) fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS events (
            id             TEXT PRIMARY KEY,
            session_id     TEXT NOT NULL DEFAULT '',
            timestamp      TEXT NOT NULL,
            source         TEXT NOT NULL,
            metric         TEXT NOT NULL,
            value          REAL NOT NULL,
            unit           TEXT NOT NULL DEFAULT '',
            confidence     REAL NOT NULL DEFAULT 1.0,
            tags           TEXT NOT NULL DEFAULT '[]',
            raw_ref        TEXT NOT NULL DEFAULT '',
            created_at     TEXT NOT NULL,
            schema_version INTEGER NOT NULL DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS idx_events_source ON events(source);
        CREATE INDEX IF NOT EXISTS idx_events_metric ON events(metric);
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_source_metric ON events(source, metric);
        CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);

        CREATE TABLE IF NOT EXISTS outbox (
            outbox_id     INTEGER PRIMARY KEY AUTOINCREMENT,
            event_ids     TEXT NOT NULL,
            destination   TEXT NOT NULL,
            payload_hash  TEXT NOT NULL,
            created_at    TEXT NOT NULL,
            attempts      INTEGER NOT NULL DEFAULT 0,
            next_retry_at TEXT,
            last_error    TEXT,
            status        TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE INDEX IF NOT EXISTS idx_outbox_status ON outbox(status);
        CREATE INDEX IF NOT EXISTS idx_outbox_next_retry ON outbox(next_retry_at);
        ",
    )
    .map_err(|e| format!("Failed to init events schema: {}", e))?;

    Ok(())
}

/// Public accessor so the outbox module can share the same DB.
pub fn get_events_db() -> Result<Connection, String> {
    get_db()
}

fn make_event_id(source: &str, metric: &str, timestamp: &str, session_id: &str) -> String {
    let input = format!("{}|{}|{}|{}", source, metric, timestamp, session_id);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 16 hex chars
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Option<String>,
    pub session_id: Option<String>,
    pub timestamp: String,
    pub source: String,
    pub metric: String,
    pub value: f64,
    pub unit: Option<String>,
    pub confidence: Option<f64>,
    pub tags: Option<Vec<String>>,
    pub raw_ref: Option<String>,
    pub created_at: Option<String>,
    pub schema_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventQuery {
    pub source: Option<String>,
    pub metric: Option<String>,
    pub session_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

/// Insert events + outbox row in a single IMMEDIATE transaction.
/// If ANY step fails, the entire transaction is rolled back (via Transaction::drop).
/// Guarantees: events and their outbox entry are atomic — you never get orphan events
/// without an outbox row, or an outbox row referencing non-existent events.
#[tauri::command]
pub fn insert_events(events: Vec<Event>) -> Result<u64, String> {
    let mut conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    // BEGIN IMMEDIATE — acquires write lock up front (safe under WAL concurrency).
    // Transaction auto-rolls back on drop if .commit() is never called.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| format!("BEGIN IMMEDIATE failed: {}", e))?;

    let mut count: u64 = 0;
    let mut event_ids: Vec<String> = Vec::with_capacity(events.len());

    for event in &events {
        let session_id = event.session_id.as_deref().unwrap_or("");
        let id = event.id.clone().unwrap_or_else(|| {
            make_event_id(&event.source, &event.metric, &event.timestamp, session_id)
        });
        let unit = event.unit.as_deref().unwrap_or("");
        let confidence = event.confidence.unwrap_or(1.0);
        let tags = event
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".to_string()))
            .unwrap_or_else(|| "[]".to_string());
        let raw_ref = event.raw_ref.as_deref().unwrap_or("");
        let created_at = event.created_at.as_deref().unwrap_or(&now);
        let schema_version = event.schema_version.unwrap_or(1);

        let rows = tx
            .execute(
                "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    id,
                    session_id,
                    event.timestamp,
                    event.source,
                    event.metric,
                    event.value,
                    unit,
                    confidence,
                    tags,
                    raw_ref,
                    created_at,
                    schema_version,
                ],
            )
            .map_err(|e| format!("Insert failed: {}", e))?;

        event_ids.push(id);
        count += rows as u64;
    }

    // Insert outbox row referencing the same event IDs — same transaction
    if !event_ids.is_empty() {
        let ids_json = serde_json::to_string(&event_ids).unwrap_or_else(|_| "[]".to_string());
        let payload_hash = {
            let mut hasher = Sha256::new();
            hasher.update(ids_json.as_bytes());
            hex::encode(&hasher.finalize()[..8])
        };
        tx.execute(
            "INSERT INTO outbox (event_ids, destination, payload_hash, created_at, status) VALUES (?1, ?2, ?3, ?4, 'pending')",
            params![ ids_json, "gateway", payload_hash, now ],
        ).map_err(|e| format!("Outbox insert failed: {}", e))?;
    }

    // COMMIT — if this line is never reached (early return via ?), Transaction::drop runs ROLLBACK
    tx.commit().map_err(|e| format!("COMMIT failed: {}", e))?;

    Ok(count)
}

#[tauri::command]
pub fn query_events(query: EventQuery) -> Result<Vec<Event>, String> {
    let conn = get_db()?;

    let mut sql = String::from("SELECT id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version FROM events WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref source) = query.source {
        sql.push_str(&format!(" AND source = ?{}", param_idx));
        param_values.push(Box::new(source.clone()));
        param_idx += 1;
    }
    if let Some(ref metric) = query.metric {
        sql.push_str(&format!(" AND metric = ?{}", param_idx));
        param_values.push(Box::new(metric.clone()));
        param_idx += 1;
    }
    if let Some(ref session_id) = query.session_id {
        sql.push_str(&format!(" AND session_id = ?{}", param_idx));
        param_values.push(Box::new(session_id.clone()));
        param_idx += 1;
    }
    if let Some(ref from) = query.from {
        sql.push_str(&format!(" AND timestamp >= ?{}", param_idx));
        param_values.push(Box::new(from.clone()));
        param_idx += 1;
    }
    if let Some(ref to) = query.to {
        sql.push_str(&format!(" AND timestamp <= ?{}", param_idx));
        param_values.push(Box::new(to.clone()));
        #[allow(unused_assignments)]
        {
            param_idx += 1;
        }
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(limit) = query.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Prepare failed: {}", e))?;

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let tags_str: String = row.get(8)?;
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

            Ok(Event {
                id: Some(row.get(0)?),
                session_id: Some(row.get(1)?),
                timestamp: row.get(2)?,
                source: row.get(3)?,
                metric: row.get(4)?,
                value: row.get(5)?,
                unit: Some(row.get(6)?),
                confidence: Some(row.get(7)?),
                tags: Some(tags),
                raw_ref: Some(row.get(9)?),
                created_at: Some(row.get(10)?),
                schema_version: Some(row.get(11)?),
            })
        })
        .map_err(|e| format!("Query failed: {}", e))?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row.map_err(|e| format!("Row read failed: {}", e))?);
    }

    Ok(events)
}

#[tauri::command]
pub fn migrate_existing_data() -> Result<String, String> {
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();
    let data_dir = nexus_data_dir();
    let mut total: u64 = 0;

    // Migrate snapshots
    let snapshots_dir = data_dir.join("snapshots");
    if snapshots_dir.exists() {
        conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
        if let Ok(entries) = fs::read_dir(&snapshots_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let data = match fs::read_to_string(&path) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let snap: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let ts = snap["timestamp"].as_str().unwrap_or("").to_string();
                let session_id = snap["id"].as_str().unwrap_or("").to_string();
                let raw_ref = path.to_string_lossy().to_string();

                let metrics = [
                    ("focus_score", snap["focusScore"].as_f64()),
                    ("relaxation_score", snap["relaxationScore"].as_f64()),
                    ("alpha_peak", snap["alphaPeak"].as_f64()),
                    ("beta_balance", snap["betaBalance"].as_f64()),
                    ("fatigue", snap["fatigue"].as_f64()),
                    ("engagement", snap["engagement"].as_f64()),
                ];

                for (metric, value) in &metrics {
                    if let Some(val) = value {
                        let id = make_event_id("snapshot", metric, &ts, &session_id);
                        let _ = conn.execute(
                            "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                            params![id, session_id, ts, "snapshot", metric, val, "", 1.0, "[]", raw_ref, now, 1],
                        );
                        total += 1;
                    }
                }
            }
        }
        conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
    }

    // Migrate sessions
    let sessions_dir = data_dir.join("sessions");
    if sessions_dir.exists() {
        conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
        if let Ok(entries) = fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let data = match fs::read_to_string(&path) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let parsed: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let session = match parsed.get("session") {
                    Some(s) => s,
                    None => continue,
                };

                let session_id = session["id"].as_str().unwrap_or("").to_string();
                let ts = session["start"].as_str().unwrap_or("").to_string();
                let device_type = session["device_type"].as_str().unwrap_or("muse");
                let source = device_type;
                let raw_ref = path.to_string_lossy().to_string();
                let duration = session["duration_secs"].as_u64().unwrap_or(0) as f64;

                // Duration event
                let id = make_event_id(source, "duration_secs", &ts, &session_id);
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![id, session_id, ts, source, "duration_secs", duration, "seconds", 1.0, "[]", raw_ref, now, 1],
                );
                total += 1;

                // Summary metrics
                if let Some(summary) = session.get("summary") {
                    let summary_metrics = [
                        ("avg_focus", summary["avg_focus"].as_f64(), "score"),
                        (
                            "avg_relaxation",
                            summary["avg_relaxation"].as_f64(),
                            "score",
                        ),
                        ("avg_hr", summary["avg_hr"].as_f64(), "bpm"),
                        ("max_hr", summary["max_hr"].as_f64(), "bpm"),
                        ("avg_hrv", summary["avg_hrv"].as_f64(), "ms"),
                    ];

                    for (metric, value, unit) in &summary_metrics {
                        if let Some(val) = value {
                            let id = make_event_id(source, metric, &ts, &session_id);
                            let _ = conn.execute(
                                "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                                params![id, session_id, ts, source, metric, val, unit, 1.0, "[]", raw_ref, now, 1],
                            );
                            total += 1;
                        }
                    }
                }

                // NF-specific metrics
                if let Some(score) = session.get("total_score").and_then(|v| v.as_f64()) {
                    let id = make_event_id(source, "total_score", &ts, &session_id);
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        params![id, session_id, ts, source, "total_score", score, "score", 1.0, "[]", raw_ref, now, 1],
                    );
                    total += 1;
                }
                if let Some(zone) = session.get("time_in_zone_pct").and_then(|v| v.as_f64()) {
                    let id = make_event_id(source, "time_in_zone_pct", &ts, &session_id);
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        params![id, session_id, ts, source, "time_in_zone_pct", zone, "percent", 1.0, "[]", raw_ref, now, 1],
                    );
                    total += 1;
                }
            }
        }
        conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
    }

    // Migrate goals
    let goals_path = data_dir.join("goals.json");
    if goals_path.exists() {
        conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
        if let Ok(data) = fs::read_to_string(&goals_path) {
            if let Ok(goals) = serde_json::from_str::<Vec<serde_json::Value>>(&data) {
                for goal in &goals {
                    let goal_id = goal["id"].as_str().unwrap_or("").to_string();
                    let ts = goal["updated_at"].as_str().unwrap_or("").to_string();
                    let title = goal["title"].as_str().unwrap_or("");
                    let category = goal["category"].as_str().unwrap_or("");

                    if let Some(current) = goal["current"].as_f64() {
                        let unit = goal["unit"].as_str().unwrap_or("");
                        let tags = serde_json::to_string(&vec![category, title])
                            .unwrap_or_else(|_| "[]".to_string());
                        let id = make_event_id("goal", "current", &ts, &goal_id);
                        let _ = conn.execute(
                            "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                            params![id, goal_id, ts, "goal", "current", current, unit, 1.0, tags, "", now, 1],
                        );
                        total += 1;
                    }

                    if let Some(target) = goal["target"].as_f64() {
                        let unit = goal["unit"].as_str().unwrap_or("");
                        let tags = serde_json::to_string(&vec![category, title])
                            .unwrap_or_else(|_| "[]".to_string());
                        let id = make_event_id("goal", "target", &ts, &goal_id);
                        let _ = conn.execute(
                            "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                            params![id, goal_id, ts, "goal", "target", target, unit, 1.0, tags, "", now, 1],
                        );
                        total += 1;
                    }
                }
            }
        }
        conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
    }

    Ok(format!("Migration complete: {} events inserted", total))
}
