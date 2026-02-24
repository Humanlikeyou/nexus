use reqwest::Client;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join("Library/Application Support"))
        .join("com.taisun.electron")
}

fn db_path() -> PathBuf {
    nexus_data_dir().join("events.db")
}

fn get_db() -> Result<Connection, String> {
    let path = db_path();
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("Failed to create data dir: {}", e))?;

    let conn =
        Connection::open(&path).map_err(|e| format!("Failed to open events.db: {}", e))?;

    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("Failed to set WAL mode: {}", e))?;

    init_arm_db(&conn)?;
    Ok(conn)
}

fn init_arm_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS arm_strategies (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            type            TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'draft',
            config          TEXT NOT NULL DEFAULT '{}',
            capital_limit   REAL NOT NULL DEFAULT 0.0,
            daily_loss_cap  REAL NOT NULL DEFAULT 0.0,
            total_revenue   REAL NOT NULL DEFAULT 0.0,
            total_expenses  REAL NOT NULL DEFAULT 0.0,
            last_health     TEXT,
            health_status   TEXT NOT NULL DEFAULT 'unknown',
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS arm_decisions (
            id              TEXT PRIMARY KEY,
            strategy_id     TEXT NOT NULL,
            action          TEXT NOT NULL,
            reasoning       TEXT NOT NULL,
            context         TEXT NOT NULL DEFAULT '{}',
            outcome         TEXT,
            outcome_notes   TEXT,
            timestamp       TEXT NOT NULL,
            FOREIGN KEY (strategy_id) REFERENCES arm_strategies(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_arm_decisions_strategy ON arm_decisions(strategy_id);
        CREATE INDEX IF NOT EXISTS idx_arm_decisions_timestamp ON arm_decisions(timestamp);
        CREATE INDEX IF NOT EXISTS idx_arm_strategies_status ON arm_strategies(status);
        ",
    )
    .map_err(|e| format!("Failed to init ARM schema: {}", e))?;

    Ok(())
}

fn make_id(prefix: &str, name: &str, timestamp: &str) -> String {
    let input = format!("{}|{}|{}", prefix, name, timestamp);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8])
}

fn arm_config_path() -> PathBuf {
    nexus_data_dir().join("arm_config.json")
}

// ─── Types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmStrategy {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub strategy_type: String,
    pub status: String,
    pub config: serde_json::Value,
    pub capital_limit: f64,
    pub daily_loss_cap: f64,
    pub total_revenue: f64,
    pub total_expenses: f64,
    pub last_health: Option<String>,
    pub health_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmDecision {
    pub id: String,
    pub strategy_id: String,
    pub action: String,
    pub reasoning: String,
    pub context: serde_json::Value,
    pub outcome: Option<String>,
    pub outcome_notes: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmConfig {
    pub wallet_address: Option<String>,
    pub global_capital_limit: f64,
    pub global_daily_loss_cap: f64,
    pub poll_interval_secs: u64,
    pub kill_switch_active: bool,
    pub worker_api_keys: Vec<WorkerApiKey>,
}

impl Default for ArmConfig {
    fn default() -> Self {
        Self {
            wallet_address: None,
            global_capital_limit: 100.0,
            global_daily_loss_cap: 10.0,
            poll_interval_secs: 300,
            kill_switch_active: false,
            worker_api_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerApiKey {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmRiskStatus {
    pub global_capital_deployed: f64,
    pub global_capital_limit: f64,
    pub global_daily_pnl: f64,
    pub global_daily_loss_cap: f64,
    pub kill_switch_active: bool,
    pub active_strategies: u32,
    pub strategies_at_risk: Vec<String>,
    pub risk_index: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueSummary {
    pub total_revenue: f64,
    pub total_expenses: f64,
    pub net_income: f64,
    pub today_revenue: f64,
    pub today_expenses: f64,
    pub week_revenue: f64,
    pub month_revenue: f64,
    pub by_strategy: Vec<StrategyRevenue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRevenue {
    pub strategy_id: String,
    pub strategy_name: String,
    pub revenue: f64,
    pub expenses: f64,
    pub net: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHealthResponse {
    pub status: String,
    pub uptime_secs: Option<u64>,
    pub requests_24h: Option<u64>,
    pub error_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRevenueResponse {
    pub revenue_usd: f64,
    pub expenses_usd: f64,
    pub period: String,
    pub transaction_count: u64,
}

// ─── Helpers ─────────────────────────────────────────────

fn read_strategy_row(row: &rusqlite::Row) -> rusqlite::Result<ArmStrategy> {
    let config_str: String = row.get(4)?;
    let config: serde_json::Value =
        serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Object(Default::default()));
    Ok(ArmStrategy {
        id: row.get(0)?,
        name: row.get(1)?,
        strategy_type: row.get(2)?,
        status: row.get(3)?,
        config,
        capital_limit: row.get(5)?,
        daily_loss_cap: row.get(6)?,
        total_revenue: row.get(7)?,
        total_expenses: row.get(8)?,
        last_health: row.get(9)?,
        health_status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn read_decision_row(row: &rusqlite::Row) -> rusqlite::Result<ArmDecision> {
    let context_str: String = row.get(4)?;
    let context: serde_json::Value =
        serde_json::from_str(&context_str).unwrap_or(serde_json::Value::Object(Default::default()));
    Ok(ArmDecision {
        id: row.get(0)?,
        strategy_id: row.get(1)?,
        action: row.get(2)?,
        reasoning: row.get(3)?,
        context,
        outcome: row.get(5)?,
        outcome_notes: row.get(6)?,
        timestamp: row.get(7)?,
    })
}

fn get_strategy_by_id(conn: &Connection, id: &str) -> Result<ArmStrategy, String> {
    conn.query_row(
        "SELECT id, name, type, status, config, capital_limit, daily_loss_cap, total_revenue, total_expenses, last_health, health_status, created_at, updated_at FROM arm_strategies WHERE id = ?1",
        params![id],
        read_strategy_row,
    )
    .map_err(|e| format!("Strategy not found: {}", e))
}

fn insert_decision(conn: &Connection, strategy_id: &str, action: &str, reasoning: &str, context: &serde_json::Value) -> Result<ArmDecision, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = make_id("decision", action, &now);
    let context_str = serde_json::to_string(context).unwrap_or_else(|_| "{}".to_string());

    conn.execute(
        "INSERT INTO arm_decisions (id, strategy_id, action, reasoning, context, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, strategy_id, action, reasoning, context_str, now],
    )
    .map_err(|e| format!("Failed to insert decision: {}", e))?;

    println!("[arm] decision: action={action} strategy_id={strategy_id}");

    Ok(ArmDecision {
        id,
        strategy_id: strategy_id.to_string(),
        action: action.to_string(),
        reasoning: reasoning.to_string(),
        context: context.clone(),
        outcome: None,
        outcome_notes: None,
        timestamp: now,
    })
}

fn insert_revenue_event(conn: &Connection, strategy_id: &str, metric: &str, value: f64, unit: &str, tags: &[&str], raw_ref: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = make_id("arm", &format!("{}|{}", metric, strategy_id), &now);
    let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "INSERT OR IGNORE INTO events (id, session_id, timestamp, source, metric, value, unit, confidence, tags, raw_ref, created_at, schema_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![id, strategy_id, now, "arm", metric, value, unit, 1.0, tags_json, raw_ref, now, 1],
    )
    .map_err(|e| format!("Failed to insert revenue event: {}", e))?;

    Ok(())
}

fn load_arm_config() -> ArmConfig {
    let path = arm_config_path();
    if let Ok(data) = fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        ArmConfig::default()
    }
}

fn save_arm_config_to_disk(config: &ArmConfig) -> Result<(), String> {
    let path = arm_config_path();
    let data = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&path, data).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}

// ─── Strategy CRUD ───────────────────────────────────────

#[tauri::command]
pub fn arm_list_strategies() -> Result<Vec<ArmStrategy>, String> {
    println!("[arm] list_strategies: status=querying");
    let conn = get_db()?;
    let mut stmt = conn
        .prepare("SELECT id, name, type, status, config, capital_limit, daily_loss_cap, total_revenue, total_expenses, last_health, health_status, created_at, updated_at FROM arm_strategies ORDER BY updated_at DESC")
        .map_err(|e| format!("Prepare failed: {}", e))?;

    let rows = stmt
        .query_map([], read_strategy_row)
        .map_err(|e| format!("Query failed: {}", e))?;

    let mut strategies = Vec::new();
    for row in rows {
        strategies.push(row.map_err(|e| format!("Row read failed: {}", e))?);
    }

    println!("[arm] list_strategies: count={}", strategies.len());
    Ok(strategies)
}

#[tauri::command]
pub fn arm_create_strategy(
    name: String,
    strategy_type: String,
    config: serde_json::Value,
    capital_limit: f64,
    daily_loss_cap: f64,
) -> Result<ArmStrategy, String> {
    println!("[arm] create_strategy: name={name} type={strategy_type}");
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = make_id("strategy", &name, &now);
    let config_str = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());

    conn.execute(
        "INSERT INTO arm_strategies (id, name, type, status, config, capital_limit, daily_loss_cap, total_revenue, total_expenses, health_status, created_at, updated_at) VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, 0.0, 0.0, 'unknown', ?7, ?7)",
        params![id, name, strategy_type, config_str, capital_limit, daily_loss_cap, now],
    )
    .map_err(|e| format!("Insert failed: {}", e))?;

    insert_decision(&conn, &id, "deploy", &format!("Created strategy: {name}"), &serde_json::json!({ "type": strategy_type, "capital_limit": capital_limit }))?;

    println!("[arm] create_strategy: id={id} status=created");
    get_strategy_by_id(&conn, &id)
}

#[tauri::command]
pub fn arm_update_strategy(
    id: String,
    name: Option<String>,
    config: Option<serde_json::Value>,
    capital_limit: Option<f64>,
    daily_loss_cap: Option<f64>,
) -> Result<ArmStrategy, String> {
    println!("[arm] update_strategy: id={id}");
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    let existing = get_strategy_by_id(&conn, &id)?;

    let new_name = name.unwrap_or(existing.name);
    let new_config = config.unwrap_or(existing.config);
    let new_capital = capital_limit.unwrap_or(existing.capital_limit);
    let new_loss_cap = daily_loss_cap.unwrap_or(existing.daily_loss_cap);
    let config_str = serde_json::to_string(&new_config).unwrap_or_else(|_| "{}".to_string());

    conn.execute(
        "UPDATE arm_strategies SET name = ?1, config = ?2, capital_limit = ?3, daily_loss_cap = ?4, updated_at = ?5 WHERE id = ?6",
        params![new_name, config_str, new_capital, new_loss_cap, now, id],
    )
    .map_err(|e| format!("Update failed: {}", e))?;

    println!("[arm] update_strategy: id={id} status=updated");
    get_strategy_by_id(&conn, &id)
}

#[tauri::command]
pub fn arm_delete_strategy(id: String) -> Result<(), String> {
    println!("[arm] delete_strategy: id={id}");
    let conn = get_db()?;
    conn.execute("DELETE FROM arm_strategies WHERE id = ?1", params![id])
        .map_err(|e| format!("Delete failed: {}", e))?;
    println!("[arm] delete_strategy: id={id} status=deleted");
    Ok(())
}

// ─── Strategy Status Management ──────────────────────────

#[tauri::command]
pub fn arm_activate_strategy(id: String) -> Result<ArmStrategy, String> {
    println!("[arm] activate_strategy: id={id}");
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    let existing = get_strategy_by_id(&conn, &id)?;
    if existing.status == "killed" {
        return Err("Cannot activate a killed strategy. Create a new one.".to_string());
    }

    conn.execute(
        "UPDATE arm_strategies SET status = 'active', updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| format!("Activate failed: {}", e))?;

    insert_decision(&conn, &id, "activate", &format!("Activated strategy: {}", existing.name), &serde_json::json!({ "previous_status": existing.status }))?;

    println!("[arm] activate_strategy: id={id} status=active");
    get_strategy_by_id(&conn, &id)
}

#[tauri::command]
pub fn arm_pause_strategy(id: String, reasoning: String) -> Result<ArmStrategy, String> {
    println!("[arm] pause_strategy: id={id}");
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    let existing = get_strategy_by_id(&conn, &id)?;

    conn.execute(
        "UPDATE arm_strategies SET status = 'paused', updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| format!("Pause failed: {}", e))?;

    insert_decision(&conn, &id, "pause", &reasoning, &serde_json::json!({ "previous_status": existing.status }))?;

    // Try to send pause signal to worker
    if let Some(pause_url) = existing.config.get("pause_endpoint").and_then(|v| v.as_str()) {
        println!("[arm] pause_strategy: sending_pause_signal url={pause_url}");
        // Fire and forget — don't fail the command if worker is unreachable
        let url = pause_url.to_string();
        let key = get_worker_key(&existing.config);
        tokio::spawn(async move {
            let client = Client::new();
            let mut req = client.post(&url);
            if let Some(k) = key {
                req = req.header("X-ARM-Key", k);
            }
            let _ = req.send().await;
        });
    }

    println!("[arm] pause_strategy: id={id} status=paused");
    get_strategy_by_id(&conn, &id)
}

#[tauri::command]
pub fn arm_kill_strategy(id: String, reasoning: String) -> Result<ArmStrategy, String> {
    println!("[arm] kill_strategy: id={id}");
    let conn = get_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    let existing = get_strategy_by_id(&conn, &id)?;

    conn.execute(
        "UPDATE arm_strategies SET status = 'killed', updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| format!("Kill failed: {}", e))?;

    insert_decision(&conn, &id, "kill", &reasoning, &serde_json::json!({ "previous_status": existing.status, "total_revenue": existing.total_revenue, "total_expenses": existing.total_expenses }))?;

    // Try to send kill signal to worker
    if let Some(kill_url) = existing.config.get("kill_endpoint").and_then(|v| v.as_str()) {
        println!("[arm] kill_strategy: sending_kill_signal url={kill_url}");
        let url = kill_url.to_string();
        let key = get_worker_key(&existing.config);
        tokio::spawn(async move {
            let client = Client::new();
            let mut req = client.post(&url);
            if let Some(k) = key {
                req = req.header("X-ARM-Key", k);
            }
            let _ = req.send().await;
        });
    }

    println!("[arm] kill_strategy: id={id} status=killed");
    get_strategy_by_id(&conn, &id)
}

fn get_worker_key(config: &serde_json::Value) -> Option<String> {
    config.get("auth_key").and_then(|v| v.as_str()).map(|s| s.to_string())
}

// ─── Revenue Tracking ────────────────────────────────────

#[tauri::command]
pub async fn arm_poll_revenue(strategy_id: String) -> Result<f64, String> {
    println!("[arm] poll_revenue: strategy_id={strategy_id}");
    let conn = get_db()?;
    let strategy = get_strategy_by_id(&conn, &strategy_id)?;

    let revenue_url = strategy.config.get("revenue_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("Strategy has no revenue_endpoint configured")?
        .to_string();

    let client = Client::new();
    let mut req = client.get(&revenue_url);
    if let Some(key) = get_worker_key(&strategy.config) {
        req = req.header("X-ARM-Key", key);
    }

    let resp = req.send().await.map_err(|e| format!("HTTP request failed: {}", e))?;
    let body: WorkerRevenueResponse = resp.json().await.map_err(|e| format!("Invalid response: {}", e))?;

    println!("[arm] poll_revenue: strategy_id={strategy_id} revenue={} expenses={} transactions={}", body.revenue_usd, body.expenses_usd, body.transaction_count);

    let now = chrono::Utc::now().to_rfc3339();
    let strategy_name = &strategy.name;
    let strategy_type = &strategy.strategy_type;

    // Insert revenue events into canonical store
    if body.revenue_usd > 0.0 {
        insert_revenue_event(&conn, &strategy_id, "revenue", body.revenue_usd, "usd", &["autonomous", "arm", strategy_type, strategy_name], &revenue_url)?;
    }
    if body.expenses_usd > 0.0 {
        insert_revenue_event(&conn, &strategy_id, "expense", body.expenses_usd, "usd", &["autonomous", "arm", strategy_type, strategy_name], &revenue_url)?;
    }
    let net = body.revenue_usd - body.expenses_usd;
    if body.revenue_usd > 0.0 || body.expenses_usd > 0.0 {
        insert_revenue_event(&conn, &strategy_id, "net_income", net, "usd", &["autonomous", "arm", strategy_type, strategy_name], &revenue_url)?;
    }

    // Update strategy totals
    conn.execute(
        "UPDATE arm_strategies SET total_revenue = total_revenue + ?1, total_expenses = total_expenses + ?2, updated_at = ?3 WHERE id = ?4",
        params![body.revenue_usd, body.expenses_usd, now, strategy_id],
    )
    .map_err(|e| format!("Update totals failed: {}", e))?;

    Ok(net)
}

#[tauri::command]
pub fn arm_get_revenue_summary(from: Option<String>, to: Option<String>) -> Result<RevenueSummary, String> {
    println!("[arm] get_revenue_summary: from={:?} to={:?}", from, to);
    let conn = get_db()?;

    // Get totals from strategies table
    let (total_revenue, total_expenses): (f64, f64) = conn
        .query_row(
            "SELECT COALESCE(SUM(total_revenue), 0.0), COALESCE(SUM(total_expenses), 0.0) FROM arm_strategies",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("Query failed: {}", e))?;

    // Today's revenue from events
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let today_revenue: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'revenue' AND timestamp >= ?1",
            params![format!("{}T00:00:00", today)],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    let today_expenses: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'expense' AND timestamp >= ?1",
            params![format!("{}T00:00:00", today)],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Week revenue
    let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let week_revenue: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'revenue' AND timestamp >= ?1",
            params![week_ago],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Month revenue
    let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let month_revenue: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'revenue' AND timestamp >= ?1",
            params![month_ago],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Per-strategy breakdown
    let mut stmt = conn
        .prepare("SELECT id, name, total_revenue, total_expenses FROM arm_strategies WHERE status != 'killed' ORDER BY total_revenue DESC")
        .map_err(|e| format!("Prepare failed: {}", e))?;

    let by_strategy: Vec<StrategyRevenue> = stmt
        .query_map([], |row| {
            let rev: f64 = row.get(2)?;
            let exp: f64 = row.get(3)?;
            Ok(StrategyRevenue {
                strategy_id: row.get(0)?,
                strategy_name: row.get(1)?,
                revenue: rev,
                expenses: exp,
                net: rev - exp,
            })
        })
        .map_err(|e| format!("Query failed: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(RevenueSummary {
        total_revenue,
        total_expenses,
        net_income: total_revenue - total_expenses,
        today_revenue,
        today_expenses,
        week_revenue,
        month_revenue,
        by_strategy,
    })
}

// ─── Decision Journal ────────────────────────────────────

#[tauri::command]
pub fn arm_log_decision(
    strategy_id: String,
    action: String,
    reasoning: String,
    context: Option<serde_json::Value>,
) -> Result<ArmDecision, String> {
    println!("[arm] log_decision: strategy_id={strategy_id} action={action}");
    let conn = get_db()?;
    let ctx = context.unwrap_or(serde_json::Value::Object(Default::default()));
    insert_decision(&conn, &strategy_id, &action, &reasoning, &ctx)
}

#[tauri::command]
pub fn arm_list_decisions(strategy_id: Option<String>, limit: Option<u32>) -> Result<Vec<ArmDecision>, String> {
    println!("[arm] list_decisions: strategy_id={:?} limit={:?}", strategy_id, limit);
    let conn = get_db()?;

    let sql = if strategy_id.is_some() {
        format!(
            "SELECT id, strategy_id, action, reasoning, context, outcome, outcome_notes, timestamp FROM arm_decisions WHERE strategy_id = ?1 ORDER BY timestamp DESC LIMIT {}",
            limit.unwrap_or(100)
        )
    } else {
        format!(
            "SELECT id, strategy_id, action, reasoning, context, outcome, outcome_notes, timestamp FROM arm_decisions ORDER BY timestamp DESC LIMIT {}",
            limit.unwrap_or(100)
        )
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| format!("Prepare failed: {}", e))?;

    let rows = if let Some(ref sid) = strategy_id {
        stmt.query_map(params![sid], read_decision_row)
    } else {
        stmt.query_map([], read_decision_row)
    }
    .map_err(|e| format!("Query failed: {}", e))?;

    let mut decisions = Vec::new();
    for row in rows {
        decisions.push(row.map_err(|e| format!("Row read failed: {}", e))?);
    }

    println!("[arm] list_decisions: count={}", decisions.len());
    Ok(decisions)
}

#[tauri::command]
pub fn arm_update_decision_outcome(id: String, outcome: String, notes: Option<String>) -> Result<ArmDecision, String> {
    println!("[arm] update_decision_outcome: id={id} outcome={outcome}");
    let conn = get_db()?;

    conn.execute(
        "UPDATE arm_decisions SET outcome = ?1, outcome_notes = ?2 WHERE id = ?3",
        params![outcome, notes, id],
    )
    .map_err(|e| format!("Update failed: {}", e))?;

    conn.query_row(
        "SELECT id, strategy_id, action, reasoning, context, outcome, outcome_notes, timestamp FROM arm_decisions WHERE id = ?1",
        params![id],
        read_decision_row,
    )
    .map_err(|e| format!("Decision not found: {}", e))
}

// ─── Risk Controls ───────────────────────────────────────

#[tauri::command]
pub fn arm_get_risk_status() -> Result<ArmRiskStatus, String> {
    println!("[arm] get_risk_status: status=computing");
    let conn = get_db()?;
    let config = load_arm_config();

    let active_count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM arm_strategies WHERE status = 'active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let capital_deployed: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(capital_limit), 0.0) FROM arm_strategies WHERE status IN ('active', 'testing')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Today's P&L
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let today_start = format!("{}T00:00:00", today);

    let today_revenue: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'revenue' AND timestamp >= ?1",
            params![today_start],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    let today_expenses: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'expense' AND timestamp >= ?1",
            params![today_start],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    let daily_pnl = today_revenue - today_expenses;

    // Find strategies near their caps
    let mut stmt = conn
        .prepare("SELECT id, daily_loss_cap FROM arm_strategies WHERE status = 'active' AND daily_loss_cap > 0")
        .map_err(|e| format!("Prepare failed: {}", e))?;

    let at_risk: Vec<String> = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let _cap: f64 = row.get(1)?;
            Ok(id)
        })
        .map_err(|e| format!("Query failed: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    // Compute risk index (0.0 safe → 1.0 danger)
    let capital_ratio = if config.global_capital_limit > 0.0 {
        capital_deployed / config.global_capital_limit
    } else {
        0.0
    };
    let loss_ratio = if config.global_daily_loss_cap > 0.0 && daily_pnl < 0.0 {
        (-daily_pnl) / config.global_daily_loss_cap
    } else {
        0.0
    };
    let risk_index = (capital_ratio * 0.5 + loss_ratio * 0.5).min(1.0);

    println!("[arm] get_risk_status: risk_index={risk_index:.2} capital_deployed={capital_deployed} daily_pnl={daily_pnl}");

    Ok(ArmRiskStatus {
        global_capital_deployed: capital_deployed,
        global_capital_limit: config.global_capital_limit,
        global_daily_pnl: daily_pnl,
        global_daily_loss_cap: config.global_daily_loss_cap,
        kill_switch_active: config.kill_switch_active,
        active_strategies: active_count,
        strategies_at_risk: at_risk,
        risk_index,
    })
}

#[tauri::command]
pub fn arm_set_global_limits(capital_limit: f64, daily_loss_cap: f64) -> Result<(), String> {
    println!("[arm] set_global_limits: capital={capital_limit} daily_loss={daily_loss_cap}");
    let mut config = load_arm_config();
    config.global_capital_limit = capital_limit;
    config.global_daily_loss_cap = daily_loss_cap;
    save_arm_config_to_disk(&config)
}

#[tauri::command]
pub fn arm_kill_switch(active: bool, reasoning: String) -> Result<(), String> {
    println!("[arm] kill_switch: active={active}");
    let conn = get_db()?;
    let mut config = load_arm_config();
    config.kill_switch_active = active;
    save_arm_config_to_disk(&config)?;

    if active {
        // Pause all active strategies
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare("SELECT id, name FROM arm_strategies WHERE status = 'active'")
            .map_err(|e| format!("Prepare failed: {}", e))?;

        let active_strategies: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("Query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        for (sid, sname) in &active_strategies {
            conn.execute(
                "UPDATE arm_strategies SET status = 'paused', updated_at = ?1 WHERE id = ?2",
                params![now, sid],
            )
            .map_err(|e| format!("Pause failed: {}", e))?;

            insert_decision(&conn, sid, "kill", &format!("Kill switch activated: {reasoning}"), &serde_json::json!({ "kill_switch": true, "strategy_name": sname }))?;
        }

        println!("[arm] kill_switch: paused_strategies={}", active_strategies.len());
    }

    Ok(())
}

// ─── Health Monitoring ───────────────────────────────────

#[tauri::command]
pub async fn arm_health_check(strategy_id: String) -> Result<WorkerHealthResponse, String> {
    println!("[arm] health_check: strategy_id={strategy_id}");
    let conn = get_db()?;
    let strategy = get_strategy_by_id(&conn, &strategy_id)?;

    let health_url = strategy.config.get("health_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("Strategy has no health_endpoint configured")?
        .to_string();

    let client = Client::new();
    let mut req = client.get(&health_url);
    if let Some(key) = get_worker_key(&strategy.config) {
        req = req.header("X-ARM-Key", key);
    }

    let result = req.send().await;

    let now = chrono::Utc::now().to_rfc3339();

    match result {
        Ok(resp) => {
            let body: WorkerHealthResponse = resp.json().await.map_err(|e| format!("Invalid health response: {}", e))?;

            conn.execute(
                "UPDATE arm_strategies SET health_status = ?1, last_health = ?2, updated_at = ?2 WHERE id = ?3",
                params![body.status, now, strategy_id],
            )
            .map_err(|e| format!("Update health failed: {}", e))?;

            println!("[arm] health_check: strategy_id={strategy_id} status={}", body.status);
            Ok(body)
        }
        Err(e) => {
            conn.execute(
                "UPDATE arm_strategies SET health_status = 'down', last_health = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, strategy_id],
            )
            .map_err(|e| format!("Update health failed: {}", e))?;

            println!("[arm] health_check: strategy_id={strategy_id} status=down error={e}");
            Ok(WorkerHealthResponse {
                status: "down".to_string(),
                uptime_secs: None,
                requests_24h: None,
                error_rate: None,
            })
        }
    }
}

#[tauri::command]
pub async fn arm_poll_all() -> Result<Vec<ArmStrategy>, String> {
    println!("[arm] poll_all: status=starting");
    let config = load_arm_config();

    if config.kill_switch_active {
        println!("[arm] poll_all: kill_switch=active skipping");
        return arm_list_strategies();
    }

    // Collect active strategy IDs in a block so conn is dropped before .await
    let active_ids: Vec<String> = {
        let conn = get_db()?;
        let mut stmt = conn
            .prepare("SELECT id FROM arm_strategies WHERE status = 'active'")
            .map_err(|e| format!("Prepare failed: {}", e))?;
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        println!("[arm] poll_all: active_strategies={}", ids.len());
        ids
    };

    for sid in &active_ids {
        let _ = arm_health_check(sid.clone()).await;
        let _ = arm_poll_revenue(sid.clone()).await;
    }

    // Check daily loss caps (new conn after async calls)
    {
        let conn = get_db()?;
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let today_start = format!("{}T00:00:00", today);
        let now = chrono::Utc::now().to_rfc3339();

        let today_expenses: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(value), 0.0) FROM events WHERE source = 'arm' AND metric = 'expense' AND timestamp >= ?1",
                params![today_start],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        if today_expenses > config.global_daily_loss_cap && config.global_daily_loss_cap > 0.0 {
            println!("[arm] poll_all: daily_loss_cap_exceeded expenses={today_expenses} cap={}", config.global_daily_loss_cap);
            for sid in &active_ids {
                conn.execute(
                    "UPDATE arm_strategies SET status = 'paused', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
                    params![now, sid],
                ).ok();
                insert_decision(&conn, sid, "pause", &format!("Auto-paused: daily loss cap exceeded (${today_expenses:.2} > ${:.2})", config.global_daily_loss_cap), &serde_json::json!({ "auto": true, "daily_expenses": today_expenses })).ok();
            }
        }
    }

    println!("[arm] poll_all: status=done");
    arm_list_strategies()
}

// ─── Config ──────────────────────────────────────────────

#[tauri::command]
pub fn arm_get_config() -> Result<ArmConfig, String> {
    println!("[arm] get_config: status=loading");
    Ok(load_arm_config())
}

#[tauri::command]
pub fn arm_save_config(config: ArmConfig) -> Result<(), String> {
    println!("[arm] save_config: status=saving");
    save_arm_config_to_disk(&config)?;
    println!("[arm] save_config: status=saved");
    Ok(())
}
