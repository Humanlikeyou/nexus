use serde::{Deserialize, Serialize};

/// Loopback-only crew-service base URL (no env override, by design).
const CREW_SERVICE_URL: &str = "http://127.0.0.1:8400";
const CREW_TOKEN_HEADER: &str = "X-Crew-Token";

/// Base URL of the local crew-service. Always loopback.
fn crew_service_url() -> String {
    CREW_SERVICE_URL.to_string()
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
    // R13-B totals; absent on older rows and older services.
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub tokens: Option<i64>,
    #[serde(default)]
    pub requests: Option<i64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

/// One timeline step of a run (crew-service `crew_steps`). Labels are short role/tool names,
/// never prompt text.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CrewStepRow {
    pub id: i64,
    pub agent: String,
    pub kind: String,
    pub label: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CrewHandoffRow {
    pub from: String,
    pub to: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct CrewRunSteps {
    #[serde(default)]
    pub steps: Vec<CrewStepRow>,
    #[serde(default)]
    pub handoffs: Vec<CrewHandoffRow>,
}

/// Run ids are UUIDs minted by the crew service: 36 chars of hex and dashes, nothing else, so
/// an id can never alter the request path.
fn valid_run_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
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
pub async fn crew_run_steps(id: String) -> Result<CrewRunSteps, String> {
    if !valid_run_id(&id) {
        return Err("Invalid crew run id.".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/crews/runs/{}/steps", crew_service_url(), id))
        .header(CREW_TOKEN_HEADER, crew_token()?)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "Crew service unreachable.".to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(http_error("steps", status));
    }

    resp.json::<CrewRunSteps>()
        .await
        .map_err(|_| "Unexpected crew steps from the crew service.".to_string())
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
    fn run_id_validation_accepts_uuids_only() {
        assert!(valid_run_id("11111111-2222-3333-4444-555555555555"));
        assert!(valid_run_id("AbCdEf01-2222-3333-4444-555555555555"));
        for bad in [
            "",
            "../etc/passwd",
            "11111111-2222-3333-4444-55555555555/",
            "11111111-2222-3333-4444-55555555555?",
            "11111111-2222-3333-4444-5555555555555",
            "11111111-2222-3333-4444-55555555555g",
            "11111111/2222/3333/4444/555555555555",
        ] {
            assert!(!valid_run_id(bad), "{bad:?} accepted");
        }
    }

    #[tokio::test]
    async fn crew_run_steps_rejects_a_bad_id_before_any_request() {
        let err = crew_run_steps("../x".into()).await.unwrap_err();
        assert_eq!(err, "Invalid crew run id.");
    }

    #[test]
    fn steps_payload_and_extended_info_deserialize_with_defaults() {
        let doc = r#"{"steps":[{"id":1,"agent":"researcher","kind":"task","label":"researcher: task",
            "start_ms":0,"end_ms":900,"status":"ok","error":null}],
            "handoffs":[{"from":"researcher","to":"analyst","status":"ok"}]}"#;
        let r: CrewRunSteps = serde_json::from_str(doc).unwrap();
        assert_eq!(r.steps[0].end_ms, 900);
        assert_eq!(r.handoffs[0].to, "analyst");
        assert_eq!(
            serde_json::from_str::<CrewRunSteps>("{}").unwrap(),
            CrewRunSteps::default()
        );
        let old: CrewInfo =
            serde_json::from_str(r#"{"id":"a","name":"n","status":"running"}"#).unwrap();
        assert!(old.tokens.is_none() && old.cost_usd.is_none() && old.finished_at.is_none());
        let new: CrewInfo = serde_json::from_str(
            r#"{"id":"a","name":"n","status":"completed","tokens":12,"requests":2,"cost_usd":0.0,"finished_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(
            (new.tokens, new.requests, new.cost_usd),
            (Some(12), Some(2), Some(0.0))
        );
    }

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

#[cfg(test)]
mod crew_url_tests {
    use super::*;

    #[test]
    fn crew_service_url_is_loopback_8400() {
        // No env mutation: the URL is a compile-time constant, so there is
        // nothing to override and no cross-test env race.
        let url = crew_service_url();
        let rest = url.strip_prefix("http://").expect("http scheme");
        let authority = rest.split('/').next().unwrap();
        let (host, port) = authority.rsplit_once(':').expect("explicit port");
        assert!(
            matches!(host, "127.0.0.1" | "localhost" | "[::1]"),
            "host {host} not loopback"
        );
        assert_eq!(port, "8400");
    }
}
