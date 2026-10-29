use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// App data dir; honours NEXUS_DATA_DIR (app_paths::data_dir).
fn nexus_data_dir() -> PathBuf {
    crate::app_paths::data_dir()
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
    save_config_at(&config_path(), config)
}

/// config.json holds the bot token and gateway auth: always write it
/// atomically with mode 0600, whatever mode the previous file had.
fn save_config_at(path: &std::path::Path, config: &NexusConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    crate::commands::continuity::atomic_write_with_mode(path, json.as_bytes(), Some(0o600))
        .map_err(|e| e.to_string())
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
/// when launching for QA. Reads (getUpdates, getMe, health checks) still work.
pub const OUTBOUND_DISABLED_ENV: &str = "NEXUS_OUTBOUND_DISABLED";

pub fn outbound_disabled() -> bool {
    parse_outbound_disabled(std::env::var(OUTBOUND_DISABLED_ENV).ok().as_deref())
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
pub async fn send_telegram_message(text: String) -> Result<TelegramMessage, String> {
    send_telegram_message_gated(text, outbound_disabled()).await
}

/// The send with the kill switch injected, so tests never touch process env (set_var is
/// process-wide and races other tests under cargo's parallel runner).
async fn send_telegram_message_gated(
    text: String,
    disabled: bool,
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
pub async fn test_telegram_connection() -> Result<String, String> {
    let config = load_config();
    let token = config.telegram_bot_token.as_deref().unwrap_or("");

    if token.is_empty() {
        return Err("No bot token configured".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/getMe", token);
    let client = reqwest::Client::new();
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
#[derive(Serialize, Deserialize, Clone)]
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
pub async fn poll_telegram_updates(last_update_id: i64) -> Result<Vec<TelegramUpdate>, String> {
    let config = load_config();
    let token = config.telegram_bot_token.as_deref().unwrap_or("");
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
pub async fn send_to_terrance(text: String) -> Result<String, String> {
    let client = reqwest::Client::new();
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
}

#[cfg(test)]
mod outbound_tests {
    use super::*;

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
