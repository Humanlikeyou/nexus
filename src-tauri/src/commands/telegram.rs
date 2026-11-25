use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

pub(crate) fn config_path() -> PathBuf {
    nexus_data_dir().join("config.json")
}

#[derive(Serialize, Deserialize, Clone, Default, specta::Type)]
pub struct NexusConfig {
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    pub openclaw_path: Option<String>,
    pub tasks_path: Option<String>,
    pub pipeline_path: Option<String>,
    #[serde(default = "default_gateway_url")]
    pub gateway_url: Option<String>,
    pub gateway_auth_token: Option<String>,
    #[serde(default = "default_gateway_batch_size")]
    pub gateway_batch_size: Option<u32>,
    #[serde(default = "default_gateway_retry_base")]
    pub gateway_retry_base_secs: Option<u32>,
    #[serde(default = "default_gateway_retry_max")]
    pub gateway_retry_max_secs: Option<u32>,
    #[serde(default)]
    pub n8n_webhook_secret: Option<String>,
    #[serde(default)]
    pub preferred_muse_address: Option<String>,
    // LLM Router
    #[serde(default)]
    pub compute_mode: Option<String>,
    #[serde(default)]
    pub llm_ollama_model: Option<String>,
    #[serde(default)]
    pub llm_anthropic_model: Option<String>,
    #[serde(default)]
    pub llm_token_threshold: Option<u32>,
    #[serde(default)]
    pub llm_daily_cap_usd: Option<f64>,
    #[serde(default)]
    pub llm_per_task_cap_usd: Option<f64>,
    #[serde(default)]
    pub llm_show_cost_estimate: Option<bool>,
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    /// Settings › "Allow Claude" (llm_router claude tier). Off unless set.
    #[serde(default)]
    pub allow_claude: Option<bool>,
    /// Amara Router base URL (compute mode "amara"). None = http://127.0.0.1:8787.
    #[serde(default)]
    pub amara_router_url: Option<String>,
    /// Home Assistant base URL (loopback only). None = HA_URL env var, then the default.
    #[serde(default)]
    pub ha_url: Option<String>,
}

fn default_gateway_url() -> Option<String> {
    Some("http://127.0.0.1:18789".to_string())
}

fn default_gateway_batch_size() -> Option<u32> {
    Some(50)
}

fn default_gateway_retry_base() -> Option<u32> {
    Some(5)
}

fn default_gateway_retry_max() -> Option<u32> {
    Some(300)
}

#[derive(Serialize, Deserialize, Clone, specta::Type)]
pub struct TelegramMessage {
    pub id: i64,
    pub from_name: String,
    pub text: String,
    pub date: i64,
    pub is_outgoing: bool,
}

#[derive(Deserialize)]
struct TgResponse {
    ok: bool,
    result: Option<serde_json::Value>,
}

/// The bot token: the Keychain item first (Stage G, decision 0033 Q7), else the legacy plaintext
/// `config.json` value. Empty string when neither has one. Never logged.
pub(crate) fn telegram_token(config: &NexusConfig) -> zeroize::Zeroizing<String> {
    super::keychain_migrate::with_live_keychain(|kc| {
        super::keychain_migrate::secret_prefer_keychain(
            kc,
            "telegram_bot_token",
            |v| v,
            || {
                zeroize::Zeroizing::new(
                    config
                        .telegram_bot_token
                        .as_deref()
                        .unwrap_or("")
                        .to_string(),
                )
            },
        )
    })
}

/// One process-wide lock around every read-modify-write of config.json (Stage G review MED-3), so
/// a settings save can never interleave with the token removal and bring the token back.
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn config_lock() -> std::sync::MutexGuard<'static, ()> {
    CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read a file without following a final symlink, into zeroizing memory (it may hold the token).
fn read_nofollow(path: &std::path::Path) -> Option<zeroize::Zeroizing<Vec<u8>>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = zeroize::Zeroizing::new(Vec::new());
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn parse_config_value(bytes: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(bytes).ok()
}

/// The plaintext bot token in the config file at `path`, if any (read as plain JSON so unknown
/// keys never matter; no symlink followed). Never logged.
pub(crate) fn plaintext_token_at(path: &std::path::Path) -> Option<zeroize::Zeroizing<String>> {
    let bytes = read_nofollow(path)?;
    let v = parse_config_value(&bytes)?;
    let t = v.get("telegram_bot_token")?.as_str()?.trim();
    (!t.is_empty()).then(|| zeroize::Zeroizing::new(t.to_string()))
}

/// Set `telegram_bot_token` to null in the config file at `path`, keeping every other key as it is
/// (edited as plain JSON, so nothing is dropped), written atomically with mode 0600. Takes the
/// config lock and re-reads the file under it. Refuses a file that is not a JSON object.
pub(crate) fn clear_plaintext_token_at(path: &std::path::Path) -> Result<(), String> {
    let _lock = config_lock();
    let bytes = read_nofollow(path).ok_or_else(|| "config unreadable".to_string())?;
    let mut v = parse_config_value(&bytes).ok_or_else(|| "config is not valid JSON".to_string())?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| "config is not an object".to_string())?;
    obj.insert("telegram_bot_token".into(), serde_json::Value::Null);
    let json = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    crate::commands::shared::fsx::atomic_write_with_mode(path, json.as_bytes(), Some(0o600))
        .map_err(|e| e.to_string())
}

pub fn load_config() -> NexusConfig {
    let path = config_path();
    read_nofollow(&path)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_config(config: &NexusConfig) -> Result<(), String> {
    save_config_at(&config_path(), config)
}

fn keychain_has_token() -> bool {
    super::keychain_migrate::with_live_keychain(|kc| {
        super::keychain_migrate::keychain_value(kc, "telegram_bot_token").is_some()
    })
}

/// config.json holds gateway auth: always write it atomically with mode 0600, whatever mode the
/// previous file had. The bot token never comes from the caller: it is written as null when the
/// Keychain holds it (the Keychain is the source). Only an UNMOVED plaintext token already on disk
/// is kept, so a save cannot destroy it before it has been moved. Takes the config lock.
fn save_config_at(path: &std::path::Path, config: &NexusConfig) -> Result<(), String> {
    let _lock = config_lock();
    write_config_unlocked(path, config, keychain_has_token())
}

fn write_config_unlocked(
    path: &std::path::Path,
    config: &NexusConfig,
    keychain_has_token: bool,
) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let keep = if keychain_has_token {
        None
    } else {
        plaintext_token_at(path).map(|t| t.to_string())
    };
    let mut out = config.clone();
    out.telegram_bot_token = keep;
    let json = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    crate::commands::shared::fsx::atomic_write_with_mode(path, json.as_bytes(), Some(0o600))
        .map_err(|e| e.to_string())
}

/// Load, change one thing, save (0600, atomic), all under the config lock.
pub(crate) fn update_config(change: impl FnOnce(&mut NexusConfig)) -> Result<(), String> {
    let _lock = config_lock();
    let mut config = load_config();
    change(&mut config);
    write_config_unlocked(&config_path(), &config, keychain_has_token())
}

/// The webview never receives the bot token (Stage G review MED-3): it is always null here. The
/// token is managed through `secrets_status` / `secret_set` (Keychain).
#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<NexusConfig, String> {
    let mut c = load_config();
    c.telegram_bot_token = None;
    Ok(c)
}

#[tauri::command]
#[specta::specta]
pub fn save_settings(config: NexusConfig) -> Result<(), String> {
    save_config(&config)
}

#[tauri::command]
#[specta::specta]
pub async fn get_telegram_messages(limit: Option<u32>) -> Result<Vec<TelegramMessage>, String> {
    let config = load_config();
    let token_owned = telegram_token(&config);
    let token = token_owned.as_str();
    let chat_id = config.telegram_chat_id.as_deref().unwrap_or("");

    if token.is_empty() || chat_id.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.unwrap_or(50);
    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?offset=-{}&limit={}",
        token, limit, limit
    );

    let client = super::shared::http::builder(super::shared::http::HttpSpec {
        timeout: Some(std::time::Duration::from_secs(15)),
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        ..Default::default()
    })
    .build()
    .map_err(|_| "Network error contacting Telegram.".to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "Network error contacting Telegram.".to_string())?;

    let body: TgResponse = resp
        .json()
        .await
        .map_err(|_| "Telegram returned an unreadable response.".to_string())?;

    if !body.ok {
        return Err("Telegram API returned an error".to_string());
    }

    let mut messages = Vec::new();

    if let Some(results) = body.result {
        if let Some(updates) = results.as_array() {
            for update in updates {
                if let Some(msg) = update.get("message") {
                    let chat = msg
                        .get("chat")
                        .and_then(|c| c.get("id"))
                        .and_then(|id| id.as_i64());
                    if chat.map(|id| id.to_string()) != Some(chat_id.to_string()) {
                        continue;
                    }

                    let from = msg.get("from");
                    let from_name = from
                        .and_then(|f| f.get("first_name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let is_bot = from
                        .and_then(|f| f.get("is_bot"))
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);

                    let text = msg
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    let date = msg.get("date").and_then(|d| d.as_i64()).unwrap_or(0);
                    let message_id = msg
                        .get("message_id")
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    if !text.is_empty() {
                        messages.push(TelegramMessage {
                            id: message_id,
                            from_name,
                            text,
                            date,
                            is_outgoing: is_bot,
                        });
                    }
                }
            }
        }
    }

    messages.sort_by_key(|m| m.date);
    Ok(messages)
}

/// Session kill switch for every outbound send (Telegram messages and outbox
/// delivery to the gateway): set `NEXUS_OUTBOUND_DISABLED=1` (or `true`/`yes`/`on`)
/// when launching for QA. Reads (getUpdates, getMe, health checks) still work. Outbound is also
/// off by default: Settings must enable it (`outbound.rs`, approval-gated), and this env var still
/// forces it off when set. `Info.plist` no longer bakes the variable.
pub const OUTBOUND_DISABLED_ENV: &str = "NEXUS_OUTBOUND_DISABLED";

pub fn outbound_disabled() -> bool {
    let env = std::env::var(OUTBOUND_DISABLED_ENV).ok();
    let allowed = super::outbound::outbound_allowed();
    local_only_on() || parse_outbound_disabled(env.as_deref()) || !allowed
}

/// Session-only "nothing leaves this Mac" switch (navy mode, design-1111x). In memory only: it
/// resets to off on restart and is never written to disk. While on, everything that honours
/// NEXUS_OUTBOUND_DISABLED is blocked the same way (Telegram, outbox, research, the Claude tier).
static LOCAL_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn local_only_on() -> bool {
    LOCAL_ONLY.load(std::sync::atomic::Ordering::SeqCst)
}

/// Bumped each time local-only is switched on, so a Hermes turn that started earlier can tell.
static NAVY_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Hermes turns currently in flight (for tests and so the switch can account for them).
static HERMES_IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn navy_epoch() -> u64 {
    NAVY_EPOCH.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub fn hermes_in_flight() -> usize {
    HERMES_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Counts one Hermes turn while alive.
pub struct HermesTurnGuard;

impl HermesTurnGuard {
    pub fn begin() -> Self {
        HERMES_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        HermesTurnGuard
    }
}

impl Drop for HermesTurnGuard {
    fn drop(&mut self) {
        HERMES_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Resolves once local-only has been switched on after `epoch` (or is on now). Used to drop an
/// in-flight Hermes request, which closes its connection.
pub async fn navy_switched_on_since(epoch: u64) {
    while !(local_only_on() || navy_epoch() != epoch) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// File the amara-voice loop reads before every turn (it talks to Hermes directly, outside this
/// app). Holds "1" while local-only is on, "0" otherwise; written "0" at every app start.
pub const NAVY_FLAG_FILE: &str = "navy_local_only";

/// Atomic, owner-only write of the flag file ("1" / "0") into `dir`.
pub(crate) fn write_navy_flag_in(dir: &std::path::Path, on: bool) -> std::io::Result<()> {
    crate::app_paths::create_private_dir_all(dir)?;
    let tmp = dir.join(format!("{NAVY_FLAG_FILE}.tmp"));
    crate::app_paths::write_private(&tmp, if on { "1" } else { "0" })?;
    std::fs::rename(&tmp, dir.join(NAVY_FLAG_FILE))
}

/// App start: the session-only switch is off, and the file says so.
pub fn init_navy_flag() {
    #[cfg(not(test))]
    let _ = write_navy_flag_in(&crate::app_paths::data_dir(), false);
}

/// Turns the session's local-only switch on or off; returns whether the state the UI asked for is
/// in force. Turning on also writes the voice loop's flag file; if that can't be written the
/// in-memory switch stays on (safe) but this returns false so the look doesn't claim a guarantee.
#[tauri::command]
#[specta::specta]
pub fn set_local_only(on: bool) -> bool {
    #[cfg(not(test))]
    let flag_ok = write_navy_flag_in(&crate::app_paths::data_dir(), on).is_ok();
    #[cfg(test)]
    let flag_ok = true;
    if on {
        NAVY_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    LOCAL_ONLY.store(on, std::sync::atomic::Ordering::SeqCst);
    if on {
        super::home_windows::close_all(); // no web window stays open under Local only
    }
    // A Hermes turn that started before the switch may still be running its tools on Hermes's side
    // (dropping our request doesn't cancel it there), so don't confirm local-only while one is in
    // flight. The switch itself stays on (safe); the UI won't claim the guarantee and can retry.
    let hermes_busy = on && HERMES_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) > 0;
    local_only_on() && (!on || (flag_ok && !hermes_busy))
}

/// NEXUS_OUTBOUND_DISABLED value → kill switch on? Trimmed, case-insensitive "1", "true", "yes"
/// or "on"; anything else and unset are off.
pub(crate) fn parse_outbound_disabled(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[tauri::command]
#[specta::specta]
pub async fn send_telegram_message(text: String) -> Result<TelegramMessage, String> {
    send_telegram_message_gated(text, outbound_disabled()).await
}

/// The send with the kill switch injected, so tests never touch process env (set_var is
/// process-wide and races other tests under cargo's parallel runner).
async fn send_telegram_message_gated(
    text: String,
    disabled: bool,
) -> Result<TelegramMessage, String> {
    send_inner(text, disabled, Some("Markdown")).await
}

/// Plain text (no `parse_mode`), for the fixed check-in questions (decision 0045): nothing in the
/// text can be misread as Markdown. Same kill switch, same config, same host as the command above.
pub(crate) async fn send_plain_gated(text: String, disabled: bool) -> Result<(), String> {
    send_inner(text, disabled, None).await.map(|_| ())
}

async fn send_inner(
    text: String,
    disabled: bool,
    parse_mode: Option<&str>,
) -> Result<TelegramMessage, String> {
    if disabled {
        println!(
            "[telegram] send suppressed ({OUTBOUND_DISABLED_ENV}), {} chars",
            text.chars().count()
        );
        return Err(format!(
            "Outbound sends are disabled for this session ({OUTBOUND_DISABLED_ENV})."
        ));
    }
    let config = load_config();
    let token_owned = telegram_token(&config);
    let token = token_owned.as_str();
    let chat_id = config.telegram_chat_id.as_deref().unwrap_or("");

    if token.is_empty() || chat_id.is_empty() {
        return Err("Telegram not configured. Set bot token and chat ID in Settings.".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);

    let client = super::shared::http::builder(super::shared::http::HttpSpec {
        timeout: Some(std::time::Duration::from_secs(15)),
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        ..Default::default()
    })
    .build()
    .map_err(|_| "Network error contacting Telegram.".to_string())?;
    let mut payload = serde_json::json!({ "chat_id": chat_id, "text": text });
    if let Some(mode) = parse_mode {
        payload["parse_mode"] = serde_json::Value::from(mode);
    }
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|_| "Network error contacting Telegram.".to_string())?;

    let body: TgResponse = resp
        .json()
        .await
        .map_err(|_| "Telegram returned an unreadable response.".to_string())?;

    if !body.ok {
        return Err(
            "Telegram API returned an error. Check your bot token and chat ID.".to_string(),
        );
    }

    let result = body.result.unwrap_or_default();
    let message_id = result
        .get("message_id")
        .and_then(|id| id.as_i64())
        .unwrap_or(0);
    let date = result.get("date").and_then(|d| d.as_i64()).unwrap_or(0);

    Ok(TelegramMessage {
        id: message_id,
        from_name: "You".to_string(),
        text,
        date,
        is_outgoing: true,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn test_telegram_connection() -> Result<String, String> {
    let config = load_config();
    let token_owned = telegram_token(&config);
    let token = token_owned.as_str();

    if token.is_empty() {
        return Err("No bot token configured".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/getMe", token);
    let client = super::shared::http::builder(super::shared::http::HttpSpec {
        timeout: Some(std::time::Duration::from_secs(15)),
        connect_timeout: Some(std::time::Duration::from_secs(5)),
        ..Default::default()
    })
    .build()
    .map_err(|_| "Network error contacting Telegram.".to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "Network error contacting Telegram.".to_string())?;
    let body: TgResponse = resp
        .json()
        .await
        .map_err(|_| "Telegram returned an unreadable response.".to_string())?;

    if !body.ok {
        return Err("Invalid bot token".to_string());
    }

    let name = body
        .result
        .and_then(|r| {
            r.get("first_name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or("Bot".to_string());

    Ok(format!("Connected to bot: {}", name))
}

/// A single Telegram update from getUpdates.
#[derive(Serialize, Deserialize, Clone, specta::Type)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message_id: i64,
    pub from_name: String,
    pub text: String,
    pub date: i64,
    pub is_bot: bool,
}

/// Poll Telegram for new updates using offset-based sequential polling.
/// Pass the last processed update_id (or 0 for first poll).
/// Returns new updates with update_id > last_update_id.
#[tauri::command]
#[specta::specta]
pub async fn poll_telegram_updates(last_update_id: i64) -> Result<Vec<TelegramUpdate>, String> {
    let config = load_config();
    let token_owned = telegram_token(&config);
    let token = token_owned.as_str();
    let chat_id = config.telegram_chat_id.as_deref().unwrap_or("");

    if token.is_empty() || chat_id.is_empty() {
        return Ok(Vec::new());
    }

    // offset = last_update_id + 1 to only get new updates
    let offset = if last_update_id > 0 {
        last_update_id + 1
    } else {
        // First poll: use -1 to get only the most recent update (to establish baseline)
        -1
    };

    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?offset={}&limit=100&timeout=5",
        token, offset
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "Unable to configure a Telegram client.".to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "Network error contacting Telegram.".to_string())?;

    let body: TgResponse = resp
        .json()
        .await
        .map_err(|_| "Telegram returned an unreadable response.".to_string())?;

    if !body.ok {
        return Err("Telegram API returned an error".to_string());
    }

    let mut updates = Vec::new();

    if let Some(results) = body.result {
        if let Some(update_arr) = results.as_array() {
            for update in update_arr {
                let uid = update
                    .get("update_id")
                    .and_then(|id| id.as_i64())
                    .unwrap_or(0);

                if let Some(msg) = update.get("message") {
                    let chat = msg
                        .get("chat")
                        .and_then(|c| c.get("id"))
                        .and_then(|id| id.as_i64());
                    if chat.map(|id| id.to_string()) != Some(chat_id.to_string()) {
                        continue;
                    }

                    let from = msg.get("from");
                    let from_name = from
                        .and_then(|f| f.get("first_name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let is_bot = from
                        .and_then(|f| f.get("is_bot"))
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);

                    // Skip messages from the bot itself
                    if is_bot {
                        continue;
                    }

                    let text = msg
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    let date = msg.get("date").and_then(|d| d.as_i64()).unwrap_or(0);
                    let message_id = msg
                        .get("message_id")
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    if !text.is_empty() {
                        updates.push(TelegramUpdate {
                            update_id: uid,
                            message_id,
                            from_name,
                            text,
                            date,
                            is_bot,
                        });
                    }
                }
            }
        }
    }

    updates.sort_by_key(|u| u.update_id);
    Ok(updates)
}

/// The only model the Telegram path may use (rule: Ollama gemma4 only). No fallback to any
/// other installed model: if gemma4 is missing or Ollama is down, fail cleanly with a reason.
fn select_ollama_model(installed: &[String]) -> Result<String, String> {
    let want = super::llm_router::DEFAULT_OLLAMA_MODEL; // "gemma4:latest"
    let family = want.split(':').next().unwrap_or(want);
    if installed.iter().any(|m| m == want) {
        return Ok(want.to_string());
    }
    installed
        .iter()
        .find(|m| *m == family || m.starts_with(&format!("{family}:")))
        .cloned()
        .ok_or_else(|| {
            format!("Ollama model {family} is not installed (no fallback model is used)")
        })
}

async fn pick_ollama_model(client: &reqwest::Client) -> Result<String, String> {
    let resp = client
        .get("http://127.0.0.1:11434/api/tags")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "Ollama unreachable (is it running?)".to_string())?;
    let body = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "Bad model list from Ollama".to_string())?;
    let installed: Vec<String> = body["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    select_ollama_model(&installed)
}

#[tauri::command]
#[specta::specta]
pub async fn send_to_terrance(text: String) -> Result<String, String> {
    // Local Ollama: loopback client, no token (not an Amara service).
    let client =
        super::loopback_http::loopback_client(super::loopback_http::LOOPBACK_CONNECT_TIMEOUT);
    let model = pick_ollama_model(&client).await.inspect_err(|reason| {
        eprintln!("[telegram] send_to_terrance: {reason}");
    })?;

    let resp = client
        .post("http://127.0.0.1:11434/api/chat")
        .json(&serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": text}],
            "stream": false
        }))
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("Ollama unreachable — is it running? ({})", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad response from Ollama: {}", e))?;

    if let Some(err) = body["error"].as_str() {
        return Err(format!("Ollama error: {}", err));
    }

    let content = body["message"]["content"]
        .as_str()
        .ok_or_else(|| "No response from Ollama".to_string())?;

    // Strip DeepSeek R1 thinking tags (<think>...</think>) from output
    let cleaned = {
        let s = content.trim();
        if let (Some(start), Some(end)) = (s.find("<think>"), s.find("</think>")) {
            if start < end {
                s[end + "</think>".len()..].trim().to_string()
            } else {
                s.to_string()
            }
        } else {
            s.to_string()
        }
    };

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn navy_flag_file_is_atomic_owner_only_and_one_or_zero() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("navy-flag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_navy_flag_in(&dir, true).unwrap();
        let f = dir.join(NAVY_FLAG_FILE);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "1");
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            !dir.join(format!("{NAVY_FLAG_FILE}.tmp")).exists(),
            "tmp renamed away"
        );
        write_navy_flag_in(&dir, false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "0");
        let _ = std::fs::remove_dir_all(&dir);
        // set_local_only writes the file and app start resets it (source-scan; tests skip the real dir).
        let src = include_str!("telegram.rs");
        let set = &src[src.find("pub fn set_local_only").unwrap()..];
        assert!(set[..set.find("local_only_on() &&").unwrap()]
            .contains("write_navy_flag_in(&crate::app_paths::data_dir(), on)"));
        assert!(src[src.find("pub fn init_navy_flag").unwrap()..]
            .contains("write_navy_flag_in(&crate::app_paths::data_dir(), false)"));
        assert!(include_str!("../lib.rs").contains("telegram::init_navy_flag()"));
    }

    /// Serialises tests that touch the process-wide navy switch and Hermes in-flight counter.
    static NAVY_GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn hermes_turns_are_counted_and_the_epoch_moves_when_local_only_turns_on() {
        let _g = NAVY_GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let before = hermes_in_flight();
        let g = HermesTurnGuard::begin();
        assert_eq!(hermes_in_flight(), before + 1);
        drop(g);
        assert_eq!(hermes_in_flight(), before);
        let e = navy_epoch();
        set_local_only(true);
        assert!(navy_epoch() > e);
        set_local_only(false);
    }

    #[test]
    fn local_only_is_not_confirmed_while_a_hermes_turn_is_in_flight() {
        let _g = NAVY_GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        // The switch still goes on (safe), but the UI is told it isn't guaranteed yet.
        let g = HermesTurnGuard::begin();
        assert!(!set_local_only(true));
        assert!(local_only_on());
        drop(g);
        set_local_only(false);
        let src = include_str!("telegram.rs");
        let set = &src[src.find("pub fn set_local_only").unwrap()..];
        assert!(set[..set.find("\n}\n").unwrap()].contains("!hermes_busy"));
    }

    #[tokio::test]
    async fn an_in_flight_wait_resolves_when_local_only_switches_on() {
        let e = navy_epoch();
        let waiter = tokio::spawn(navy_switched_on_since(e));
        set_local_only(true);
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        set_local_only(false);
    }

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn telegram_model_is_gemma4_only_with_no_fallback() {
        let want = crate::commands::llm_router::DEFAULT_OLLAMA_MODEL;
        assert_eq!(want, "gemma4:latest");
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            select_ollama_model(&names(&["llama3.2:latest", "gemma4:latest"])).unwrap(),
            "gemma4:latest"
        );
        assert_eq!(
            select_ollama_model(&names(&["gemma4:12b"])).unwrap(),
            "gemma4:12b"
        );
        // This Mac's real tags: gemma4:latest wins over other gemma4 tags; the gemma4-12b-uncensored
        // family is a different model and never counts as gemma4.
        assert_eq!(
            select_ollama_model(&names(&[
                "gemma4:12b-mlx",
                "gemma4-12b-uncensored:latest",
                "gemma4:latest"
            ]))
            .unwrap(),
            "gemma4:latest"
        );
        assert_eq!(
            select_ollama_model(&names(&["gemma4-12b-uncensored:latest", "gemma4:12b-mlx"]))
                .unwrap(),
            "gemma4:12b-mlx"
        );
        for other in [
            &[
                "gemma4-12b-uncensored:latest",
                "qwen3.8-uncensored:latest",
                "local-gemma-agent:latest",
            ][..],
            &["deepseek-r1:14b"][..],
            &["qwen3.5:latest", "llama3.3"],
            &["gemma4b-other"],
            &[],
        ] {
            let err = select_ollama_model(&names(other)).unwrap_err();
            assert!(err.contains("gemma4 is not installed"), "{err}");
        }
        assert!(!include_str!("telegram.rs").contains(concat!("deepseek", "-r1:32b")));
    }

    const TOK: &str = "SENTINEL-tg-9c41-do-not-leak-zzzzzzzzzz";

    fn cfg_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-tgcfg-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn with_keychain_token<R>(f: impl FnOnce() -> R) -> R {
        use super::super::keychain::{service_for, FakeKeychain, Keychain};
        let kc = FakeKeychain::default();
        kc.set(&service_for("telegram_bot_token"), TOK.as_bytes())
            .unwrap();
        super::super::keychain_migrate::TEST_KEYCHAIN
            .with(|k| *k.borrow_mut() = Some(std::sync::Arc::new(kc)));
        let r = f();
        super::super::keychain_migrate::TEST_KEYCHAIN.with(|k| *k.borrow_mut() = None);
        r
    }

    #[test]
    fn a_save_after_the_move_does_not_resurrect_the_token() {
        let d = cfg_dir("resurrect");
        let path = d.join("config.json");
        // The plaintext is gone from the file and the Keychain holds the token.
        fs::write(
            &path,
            r#"{"telegram_bot_token":null,"telegram_chat_id":"42"}"#,
        )
        .unwrap();
        let cfg = NexusConfig {
            telegram_bot_token: Some(TOK.into()),
            telegram_chat_id: Some("43".into()),
            ..Default::default()
        };
        with_keychain_token(|| save_config_at(&path, &cfg).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains(TOK), "the caller's token is never written");
        assert!(text.contains("\"43\""), "other settings are saved");
        // Even a stale plaintext on disk is nulled once the Keychain has the token.
        fs::write(&path, format!(r#"{{"telegram_bot_token":"{TOK}"}}"#)).unwrap();
        with_keychain_token(|| save_config_at(&path, &cfg).unwrap());
        assert!(!fs::read_to_string(&path).unwrap().contains(TOK));
        // An unmoved on-disk token survives a save while the Keychain has none.
        fs::write(&path, format!(r#"{{"telegram_bot_token":"{TOK}"}}"#)).unwrap();
        save_config_at(&path, &NexusConfig::default()).unwrap();
        assert_eq!(
            plaintext_token_at(&path).as_deref().map(String::as_str),
            Some(TOK)
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn concurrent_saves_and_the_token_removal_never_bring_the_token_back() {
        let d = cfg_dir("race");
        let path = d.join("config.json");
        for round in 0..40 {
            fs::write(
                &path,
                format!(r#"{{"telegram_bot_token":"{TOK}","telegram_chat_id":"{round}"}}"#),
            )
            .unwrap();
            let (p1, p2) = (path.clone(), path.clone());
            let saver = std::thread::spawn(move || {
                for i in 0..10 {
                    let cfg = NexusConfig {
                        telegram_chat_id: Some(format!("{i}")),
                        ..Default::default()
                    };
                    save_config_at(&p1, &cfg).unwrap();
                }
            });
            let clearer = std::thread::spawn(move || clear_plaintext_token_at(&p2).unwrap());
            saver.join().unwrap();
            clearer.join().unwrap();
            // One more save after the removal: the token must stay gone.
            save_config_at(&path, &NexusConfig::default()).unwrap();
            let text = fs::read_to_string(&path).unwrap();
            assert!(!text.contains(TOK), "round {round}: token came back");
            serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON");
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn get_config_never_returns_the_token_and_reads_do_not_follow_symlinks() {
        let src = include_str!("telegram.rs");
        let g = &src[src.find("pub fn get_config").unwrap()..];
        let g = &g[..g.find("pub fn save_settings").unwrap()];
        assert!(g.contains("telegram_bot_token = None"));
        // A symlinked config is not read.
        let d = cfg_dir("link");
        let real = d.join("real.json");
        fs::write(&real, format!(r#"{{"telegram_bot_token":"{TOK}"}}"#)).unwrap();
        let link = d.join("config.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(plaintext_token_at(&link).is_none());
        assert!(clear_plaintext_token_at(&link).is_err());
        assert!(fs::read_to_string(&real).unwrap().contains(TOK));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn save_config_writes_0600_atomically_over_a_0644_file() {
        let dir = std::env::temp_dir().join(format!("nexus-savecfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let cfg = NexusConfig {
            telegram_chat_id: Some("42".into()),
            ..Default::default()
        };
        save_config_at(&path, &cfg).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let back: NexusConfig = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.telegram_chat_id.as_deref(), Some("42"));
        // no temp files left behind
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.json")]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Source pin: no bare `Client::new()` anywhere in prod code. Telegram sends use the shared
    /// builder with timeouts; send_to_terrance's local Ollama client is a loopback client.
    #[test]
    fn telegram_prod_code_has_no_bare_client_new() {
        let src = include_str!("telegram.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").unwrap()];
        assert_eq!(
            prod.matches("Client::new()").count(),
            0,
            "telegram.rs must not build a bare Client::new()"
        );
        let f = &prod[prod.find("pub async fn send_to_terrance").unwrap()..];
        let f = &f[..f.find("\n}\n").unwrap()];
        assert!(
            f.contains("loopback_client("),
            "Ollama client must be a loopback client"
        );
    }
}

#[cfg(test)]
mod outbound_tests {
    use super::*;

    /// "Cloud off by default" no longer rides on a baked env var: the bundle must not set
    /// NEXUS_OUTBOUND_DISABLED (a baked value could never be enabled from Settings), and the
    /// default with no env and no saved setting is disabled.
    #[test]
    fn finder_launches_default_to_outbound_disabled_without_a_baked_env() {
        let plist: String = include_str!("../../Info.plist")
            .split_whitespace()
            .collect();
        assert!(!plist.contains("LSEnvironment"));
        assert!(!plist.contains("NEXUS_OUTBOUND_DISABLED"));
        // A Finder launch: no env, nothing saved, not navy.
        assert!(outbound_disabled());
    }

    /// Pinned semantics (no env access: the pure parser only). "on" counts as on (lead decision 2026-10-29).
    #[test]
    fn outbound_kill_switch_values() {
        let table: &[(Option<&str>, bool)] = &[
            (Some("1"), true),
            (Some("true"), true),
            (Some("TRUE"), true),
            (Some("True"), true),
            (Some("yes"), true),
            (Some("YES"), true),
            (Some(" 1 "), true),
            (Some(" yes "), true),
            (Some("\ttrue\n"), true),
            (Some("on"), true),
            (Some("ON"), true),
            (Some(" On "), true),
            (Some("onn"), false),
            (Some("o"), false),
            (Some(""), false),
            (Some("   "), false),
            (Some("0"), false),
            (Some("false"), false),
            (Some("no"), false),
            (Some("off"), false),
            (Some("2"), false),
            (Some("y"), false),
            (None, false),
        ];
        for (value, want) in table {
            assert_eq!(parse_outbound_disabled(*value), *want, "{value:?}");
        }
    }

    #[test]
    fn send_is_refused_without_network_when_disabled() {
        // Flag injected; no set_var, so nothing leaks to parallel tests.
        let err =
            tauri::async_runtime::block_on(send_telegram_message_gated("qa".into(), true)).err();
        assert_eq!(
            err.as_deref(),
            Some("Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED).")
        );
    }
}
