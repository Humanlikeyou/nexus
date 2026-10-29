use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
pub(crate) fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

fn db_path() -> PathBuf {
    nexus_data_dir().join("chat_history.db")
}

fn get_db() -> Result<Connection, String> {
    let path = db_path();
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("Failed to create data dir: {}", e))?;

    let conn =
        Connection::open(&path).map_err(|e| format!("Failed to open chat_history.db: {}", e))?;

    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

    init_db(&conn)?;
    Ok(conn)
}

fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS chat_sessions (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_messages (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
            role        TEXT NOT NULL,
            text        TEXT NOT NULL,
            ts          INTEGER NOT NULL,
            provider    TEXT,
            model       TEXT,
            latency_ms  INTEGER,
            cost_usd    REAL
        );

        CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id);
        CREATE INDEX IF NOT EXISTS idx_chat_messages_ts ON chat_messages(ts);
        ",
    )
    .map_err(|e| format!("Failed to init chat schema: {}", e))?;

    Ok(())
}

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub text: String,
    pub ts: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<i64>,
    pub cost_usd: Option<f64>,
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_chat_session(title: String) -> Result<ChatSession, String> {
    let conn = get_db()?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    conn.execute(
        "INSERT INTO chat_sessions (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, title, now, now],
    )
    .map_err(|e| format!("Failed to create session: {}", e))?;

    Ok(ChatSession {
        id,
        title,
        created_at: now,
        updated_at: now,
        message_count: 0,
    })
}

#[tauri::command]
pub fn list_chat_sessions() -> Result<Vec<ChatSession>, String> {
    let conn = get_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at,
                    (SELECT COUNT(*) FROM chat_messages WHERE session_id = s.id) as msg_count
             FROM chat_sessions s
             ORDER BY s.updated_at DESC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let sessions = stmt
        .query_map([], |row| {
            Ok(ChatSession {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to list sessions: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect sessions: {}", e))?;

    Ok(sessions)
}

#[tauri::command]
pub fn save_chat_message(message: ChatMessage) -> Result<i64, String> {
    let conn = get_db()?;

    conn.execute(
        "INSERT INTO chat_messages (session_id, role, text, ts, provider, model, latency_ms, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            message.session_id,
            message.role,
            message.text,
            message.ts,
            message.provider,
            message.model,
            message.latency_ms,
            message.cost_usd,
        ],
    )
    .map_err(|e| format!("Failed to save message: {}", e))?;

    let row_id = conn.last_insert_rowid();

    // Update session's updated_at timestamp
    conn.execute(
        "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
        params![message.ts, message.session_id],
    )
    .map_err(|e| format!("Failed to update session timestamp: {}", e))?;

    Ok(row_id)
}

#[tauri::command]
pub fn load_chat_history(
    session_id: String,
    limit: Option<u32>,
) -> Result<Vec<ChatMessage>, String> {
    let conn = get_db()?;
    let lim = limit.unwrap_or(200) as i64;

    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, text, ts, provider, model, latency_ms, cost_usd
             FROM chat_messages
             WHERE session_id = ?1
             ORDER BY ts ASC
             LIMIT ?2",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let messages = stmt
        .query_map(params![session_id, lim], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                text: row.get(3)?,
                ts: row.get(4)?,
                provider: row.get(5)?,
                model: row.get(6)?,
                latency_ms: row.get(7)?,
                cost_usd: row.get(8)?,
            })
        })
        .map_err(|e| format!("Failed to load history: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect messages: {}", e))?;

    Ok(messages)
}

#[tauri::command]
pub fn delete_chat_session(session_id: String) -> Result<(), String> {
    let conn = get_db()?;

    conn.execute(
        "DELETE FROM chat_messages WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|e| format!("Failed to delete messages: {}", e))?;

    conn.execute(
        "DELETE FROM chat_sessions WHERE id = ?1",
        params![session_id],
    )
    .map_err(|e| format!("Failed to delete session: {}", e))?;

    Ok(())
}
