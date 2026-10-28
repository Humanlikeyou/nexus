use serde::{Deserialize, Serialize};

const DEFAULT_CREW_SERVICE_URL: &str = "http://127.0.0.1:8400";
const CREW_TOKEN_HEADER: &str = "X-Crew-Token";

/// Base URL from `CREW_SERVICE_URL`, defaulting to the localhost compose port.
fn crew_service_url() -> String {
    std::env::var("CREW_SERVICE_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_CREW_SERVICE_URL.to_string())
}

/// Shared secret from `CREW_SERVICE_TOKEN` (must match stack/.env).
fn crew_token() -> Result<String, String> {
    std::env::var("CREW_SERVICE_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "CREW_SERVICE_TOKEN is not set for the Amara OS app".to_string())
}

#[derive(Debug, Deserialize)]
struct CrewStopResponse {
    status: String,
    #[serde(default)]
    stopped: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CrewInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub agents: Vec<String>,
    pub started_at: Option<String>,
    pub task: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrewStartRequest {
    pub name: String,
    pub task: String,
    pub agents: Option<Vec<String>>,
}

#[tauri::command]
pub async fn crew_list() -> Result<Vec<CrewInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/crews/list", crew_service_url()))
        .header(CREW_TOKEN_HEADER, crew_token()?)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Crew service unreachable: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Crew list failed ({}): {}", status, text));
    }

    resp.json::<Vec<CrewInfo>>()
        .await
        .map_err(|e| format!("Failed to parse crew list: {}", e))
}

#[tauri::command]
pub async fn crew_start(
    name: String,
    task: String,
    agents: Option<Vec<String>>,
) -> Result<CrewInfo, String> {
    let client = reqwest::Client::new();
    let body = CrewStartRequest { name, task, agents };
    let resp = client
        .post(format!("{}/crews/start", crew_service_url()))
        .header(CREW_TOKEN_HEADER, crew_token()?)
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Crew service unreachable: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Crew start failed ({}): {}", status, text));
    }

    resp.json::<CrewInfo>()
        .await
        .map_err(|e| format!("Failed to parse crew start response: {}", e))
}

#[tauri::command]
pub async fn crew_status(id: String) -> Result<CrewInfo, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/crews/status", crew_service_url()))
        .header(CREW_TOKEN_HEADER, crew_token()?)
        .query(&[("id", id)])
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Crew service unreachable: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Crew status failed ({}): {}", status, text));
    }

    resp.json::<CrewInfo>()
        .await
        .map_err(|e| format!("Failed to parse crew status: {}", e))
}

#[tauri::command]
pub async fn crew_stop(id: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/crews/stop", crew_service_url()))
        .header(CREW_TOKEN_HEADER, crew_token()?)
        .json(&serde_json::json!({ "id": id }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Crew service unreachable: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Crew stop failed ({}): {}", status, text));
    }

    let stop = resp
        .json::<CrewStopResponse>()
        .await
        .map_err(|e| format!("Failed to parse crew stop response: {}", e))?;
    if stop.stopped {
        Ok("stopped".to_string())
    } else {
        // Already finished: report the real terminal state, not "stopped".
        Ok(format!("not running ({})", stop.status))
    }
}

#[tauri::command]
pub async fn crew_health() -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", crew_service_url()))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| "Crew service offline".to_string())?;

    if resp.status().is_success() {
        Ok("operational".to_string())
    } else {
        Ok("degraded".to_string())
    }
}
