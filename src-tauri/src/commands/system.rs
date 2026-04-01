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

/// Check the status of a Docker container by name.
/// Returns "running", "exited", "paused", "not_found", or the raw status string.
#[tauri::command]
pub fn check_docker_service(name: String) -> Result<String, String> {
    let output = std::process::Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
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

/// Restart a Docker container by name.
#[tauri::command]
pub fn restart_docker_service(name: String) -> Result<String, String> {
    let output = std::process::Command::new("docker")
        .args(["restart", &name])
        .output()
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
