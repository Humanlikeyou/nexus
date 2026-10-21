use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub uptime_secs: u64,
    pub timestamp: u64,
}

#[tauri::command]
pub fn get_system_info() -> Result<SystemInfo, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        hostname,
        uptime_secs: timestamp,
        timestamp,
    })
}

#[tauri::command]
pub fn health_check() -> Result<String, String> {
    Ok("ok".to_string())
}

// ─── Docker Service Management ──────────────────────────────────────────────

/// Docker container names: `[A-Za-z0-9][A-Za-z0-9_.-]*`. Also stops a name
/// starting with `-` from being parsed as a docker CLI flag.
pub(super) fn validate_container_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let ok = name.len() <= 128
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err("Invalid container name".to_string())
    }
}

/// Check the status of a Docker container by name.
/// Returns "running", "exited", "paused", "not_found", or the raw status string.
#[tauri::command]
pub async fn check_docker_service(name: String) -> Result<String, String> {
    validate_container_name(&name)?;
    let output = tokio::process::Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
        .await
        .map_err(|e| format!("Failed to run docker inspect: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("No such object") {
            Ok("not_found".to_string())
        } else {
            Err(stderr)
        }
    }
}

/// Exact operation shown in the approval dialog for a container restart.
pub(super) fn docker_restart_prompt(name: &str) -> (String, String) {
    (
        format!("Run: docker restart {name}"),
        "Restarts this Docker container. Running requests in it will be interrupted.".to_string(),
    )
}

/// Restart a Docker container by name (requires native approval).
#[tauri::command]
pub async fn restart_docker_service(app: tauri::AppHandle, name: String) -> Result<String, String> {
    validate_container_name(&name)?;
    let (operation, detail) = docker_restart_prompt(&name);
    let _approval = super::approval_gate::require_approval(
        &app,
        &format!("docker_restart:{name}"),
        super::approval_gate::approval_text(&operation, &detail)?,
    )
    .await?;
    docker_restart(&name).await
}

/// The ungated restart. Only call with a validated name after approval.
pub(super) async fn docker_restart(name: &str) -> Result<String, String> {
    let output = tokio::process::Command::new("docker")
        .args(["restart", name])
        .output()
        .await
        .map_err(|e| format!("Failed to run docker restart: {}", e))?;

    if output.status.success() {
        Ok(format!("Container {} restarted", name))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Check if a TCP port is open on a given host.
/// Returns "open" or "closed".
#[tauri::command]
pub async fn check_service_port(host: String, port: u16) -> Result<String, String> {
    let addr = format!("{}:{}", host, port);
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => Ok("open".to_string()),
        _ => Ok("closed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_container_name;

    #[test]
    fn container_names() {
        assert!(validate_container_name("crew-service").is_ok());
        assert!(validate_container_name("n8n_1.x").is_ok());
        for bad in ["", "-rm", "--help", "a b", "a;b", "../x"] {
            assert!(validate_container_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn docker_restart_prompt_shows_exact_command() {
        let (op, _) = super::docker_restart_prompt("stack-n8n-1");
        assert_eq!(op, "Run: docker restart stack-n8n-1");
    }
}
