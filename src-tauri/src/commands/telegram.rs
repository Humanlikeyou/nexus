use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn nexus_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join("Library/Application Support"))
        .join("com.taisun.electron")
}

fn config_path() -> PathBuf {
    nexus_data_dir().join("config.json")
}

#[derive(Serialize, Deserialize, Clone, Default)]
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

#[derive(Serialize, Deserialize, Clone)]
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

pub fn load_config() -> NexusConfig {
    let path = config_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(config: &NexusConfig) -> Result<(), String> {
    let dir = nexus_data_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_path(), json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_config() -> Result<NexusConfig, String> {
    Ok(load_config())
}

#[tauri::command]
pub fn save_settings(config: NexusConfig) -> Result<(), String> {
    save_config(&config)
}

#[tauri::command]
pub async fn get_telegram_messages(limit: Option<u32>) -> Result<Vec<TelegramMessage>, String> {
    let config = load_config();
    let token = config.telegram_bot_token.as_deref().unwrap_or("");
    let chat_id = config.telegram_chat_id.as_deref().unwrap_or("");

    if token.is_empty() || chat_id.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.unwrap_or(50);
    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?offset=-{}&limit={}",
        token, limit, limit
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let body: TgResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;

    if !body.ok {
        return Err("Telegram API returned an error".to_string());
    }

    let mut messages = Vec::new();

    if let Some(results) = body.result {
        if let Some(updates) = results.as_array() {
            for update in updates {
                if let Some(msg) = update.get("message") {
                    let chat = msg.get("chat").and_then(|c| c.get("id")).and_then(|id| id.as_i64());
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
                    let date = msg
                        .get("date")
                        .and_then(|d| d.as_i64())
                        .unwrap_or(0);
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

#[tauri::command]
pub async fn send_telegram_message(text: String) -> Result<TelegramMessage, String> {
    let config = load_config();
    let token = config.telegram_bot_token.as_deref().unwrap_or("");
    let chat_id = config.telegram_chat_id.as_deref().unwrap_or("");

    if token.is_empty() || chat_id.is_empty() {
        return Err("Telegram not configured. Set bot token and chat ID in Settings.".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let body: TgResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;

    if !body.ok {
        return Err("Telegram API returned an error. Check your bot token and chat ID.".to_string());
    }

    let result = body.result.unwrap_or_default();
    let message_id = result.get("message_id").and_then(|id| id.as_i64()).unwrap_or(0);
    let date = result.get("date").and_then(|d| d.as_i64()).unwrap_or(0);

    Ok(TelegramMessage {
        id: message_id,
        from_name: "You".to_string(),
        text,
        date,
        is_outgoing: false,
    })
}

#[tauri::command]
pub async fn test_telegram_connection() -> Result<String, String> {
    let config = load_config();
    let token = config.telegram_bot_token.as_deref().unwrap_or("");

    if token.is_empty() {
        return Err("No bot token configured".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/getMe", token);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.map_err(|e| format!("Network error: {}", e))?;
    let body: TgResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;

    if !body.ok {
        return Err("Invalid bot token".to_string());
    }

    let name = body
        .result
        .and_then(|r| r.get("first_name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .unwrap_or("Bot".to_string());

    Ok(format!("Connected to bot: {}", name))
}
