use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

pub const REDUCE_TRANSPARENCY_EVENT: &str = "reduce-transparency-changed";

/// Last value seen: 0 none yet, 1 off, 2 on.
static REDUCE_TRANSPARENCY: AtomicU8 = AtomicU8::new(0);

/// macOS's Reduce Transparency. The system WKWebView has no media query for it.
#[cfg(target_os = "macos")]
fn read_reduce_transparency() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

#[cfg(not(target_os = "macos"))]
fn read_reduce_transparency() -> bool {
    false
}

/// Stores `now` and says whether it differs from the last value seen.
fn note_reduce_transparency(last: &AtomicU8, now: bool) -> bool {
    let value = if now { 2 } else { 1 };
    let before = last.swap(value, Ordering::SeqCst);
    before != 0 && before != value
}

#[tauri::command]
pub fn get_reduce_transparency() -> bool {
    let now = read_reduce_transparency();
    note_reduce_transparency(&REDUCE_TRANSPARENCY, now);
    now
}

/// Emits `reduce-transparency-changed` with the new value when the setting has changed.
pub fn sync_reduce_transparency(app: &tauri::AppHandle) {
    let now = read_reduce_transparency();
    if note_reduce_transparency(&REDUCE_TRANSPARENCY, now) {
        let _ = app.emit(REDUCE_TRANSPARENCY_EVENT, now);
    }
}

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

/// An app opened from Finder or the Dock has no shell PATH, so a bare "docker" isn't found.
fn docker() -> Result<&'static str, String> {
    super::ha::docker_bin().ok_or_else(|| "Docker isn't installed.".to_string())
}

/// Check the status of a Docker container by name.
/// Returns "running", "exited", "paused", "not_found", or the raw status string.
#[tauri::command]
pub async fn check_docker_service(name: String) -> Result<String, String> {
    validate_container_name(&name)?;
    let output = tokio::process::Command::new(docker()?)
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
        &[super::approval_gate::service_key(&format!("docker:{name}"))],
        super::approval_gate::approval_text(&operation, &detail)?,
    )
    .await?;
    docker_restart(&name).await
}

/// The ungated restart. Only call with a validated name after approval.
pub(super) async fn docker_restart(name: &str) -> Result<String, String> {
    let output = tokio::process::Command::new(docker()?)
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
    use super::{note_reduce_transparency, validate_container_name};
    use std::sync::atomic::AtomicU8;

    #[test]
    fn reduce_transparency_reports_only_real_changes() {
        let last = AtomicU8::new(0);
        assert!(!note_reduce_transparency(&last, false), "the first read");
        assert!(!note_reduce_transparency(&last, false));
        assert!(note_reduce_transparency(&last, true));
        assert!(!note_reduce_transparency(&last, true));
        assert!(note_reduce_transparency(&last, false));
    }

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
