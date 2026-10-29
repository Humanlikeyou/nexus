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

const CREW_TOKEN_ENV: &str = "CREW_SERVICE_TOKEN";
/// File under the app data dir holding the token for Finder-launched apps
/// (written by scripts/sync-crew-token.sh). Must be mode 0600.
pub(crate) const CREW_TOKEN_FILE: &str = "crew_service_token";
const NOT_CONFIGURED: &str =
    "Crew service token is not configured. Run scripts/sync-crew-token.sh, then reopen Amara OS.";

/// Shared secret (must match stack/.env): env `CREW_SERVICE_TOKEN` first,
/// then `<app data dir>/crew_service_token`.
fn crew_token() -> Result<String, String> {
    crew_token_from(
        std::env::var(CREW_TOKEN_ENV).ok(),
        &crate::app_paths::data_dir().join(CREW_TOKEN_FILE),
    )
}

pub(crate) fn crew_token_from(
    env_value: Option<String>,
    file: &std::path::Path,
) -> Result<String, String> {
    if let Some(v) = env_value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(v);
    }
    let meta = match std::fs::symlink_metadata(file) {
        Ok(m) => m,
        Err(_) => return Err(NOT_CONFIGURED.to_string()),
    };
    if !meta.is_file() {
        return Err("Crew token file is not a regular file; refusing to read it.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o777 != 0o600 {
            return Err(
                "Crew token file must be mode 0600 (chmod 600 it or rerun scripts/sync-crew-token.sh)."
                    .to_string(),
            );
        }
    }
    let value = std::fs::read_to_string(file)
        .map_err(|_| "Crew token file could not be read.".to_string())?;
    let value = value.trim();
    if value.is_empty() {
        return Err(NOT_CONFIGURED.to_string());
    }
    Ok(value.to_string())
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
        .map_err(|_| "Crew service unreachable.".to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(http_error("list", status));
    }

    resp.json::<Vec<CrewInfo>>()
        .await
        .map_err(|_| "Unexpected crew list from the crew service.".to_string())
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
        .map_err(|_| "Crew service unreachable.".to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(http_error("start", status));
    }

    resp.json::<CrewInfo>()
        .await
        .map_err(|_| "Unexpected crew start response from the crew service.".to_string())
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
        .map_err(|_| "Crew service unreachable.".to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(http_error("status", status));
    }

    resp.json::<CrewInfo>()
        .await
        .map_err(|_| "Unexpected crew status from the crew service.".to_string())
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
        .map_err(|_| "Crew service unreachable.".to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(http_error("stop", status));
    }

    let stop = resp
        .json::<CrewStopResponse>()
        .await
        .map_err(|_| "Unexpected crew stop response from the crew service.".to_string())?;
    if stop.stopped {
        Ok("stopped".to_string())
    } else {
        // Already finished: report the real terminal state, not "stopped".
        Ok(format!("not running ({})", stop.status))
    }
}

/// One of "not_configured" (no token), "offline" (unreachable), "degraded"
/// (reachable but unhealthy) or "operational". Never an error: every state is
/// something the UI shows.
#[tauri::command]
pub async fn crew_health() -> Result<String, String> {
    if crew_token().is_err() {
        return Ok("not_configured".to_string());
    }
    let client = reqwest::Client::new();
    let resp = match client
        .get(format!("{}/health", crew_service_url()))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return Ok("offline".to_string()),
    };

    if resp.status().is_success() {
        Ok("operational".to_string())
    } else {
        Ok("degraded".to_string())
    }
}

/// Fixed text plus the HTTP status only: the response body may echo paths or upstream errors.
fn http_error(op: &str, status: reqwest::StatusCode) -> String {
    format!("Crew {op} failed (HTTP {}).", status.as_u16())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_is_fixed_text_with_status_only() {
        let msg = http_error("list", reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(msg, "Crew list failed (HTTP 500).");
    }

    #[test]
    fn crew_errors_never_embed_raw_errors_or_bodies() {
        let src = include_str!("crew.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        for banned in [".text().await", "|e| format!", "{}\", e)", "{e}"] {
            assert!(
                !code.contains(banned),
                "crew.rs error path contains {banned}"
            );
        }
    }
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn token_file(tag: &str, contents: &str, mode: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nexus-crew-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CREW_TOKEN_FILE);
        fs::write(&path, contents).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn env_token_wins_over_the_file() {
        let path = token_file("env", "from-file\n", 0o600);
        assert_eq!(
            crew_token_from(Some(" from-env ".into()), &path).unwrap(),
            "from-env"
        );
        // blank env falls through to the file
        assert_eq!(
            crew_token_from(Some("  ".into()), &path).unwrap(),
            "from-file"
        );
        assert_eq!(crew_token_from(None, &path).unwrap(), "from-file");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn token_file_must_be_0600() {
        for mode in [0o644, 0o640, 0o400, 0o700] {
            let path = token_file("perm", "secret", mode);
            let err = crew_token_from(None, &path).unwrap_err();
            assert!(err.contains("0600"), "mode {mode:o}: {err}");
            assert!(!err.contains("secret"));
            let _ = fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn empty_or_missing_file_is_not_configured() {
        let path = token_file("empty", " \n", 0o600);
        assert_eq!(crew_token_from(None, &path).unwrap_err(), NOT_CONFIGURED);
        let missing = path.with_file_name("absent");
        assert_eq!(crew_token_from(None, &missing).unwrap_err(), NOT_CONFIGURED);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn symlinked_token_file_is_refused() {
        let real = token_file("link", "secret", 0o600);
        let link = real.with_file_name("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(crew_token_from(None, &link)
            .unwrap_err()
            .contains("regular file"));
        let _ = fs::remove_dir_all(real.parent().unwrap());
    }
}
