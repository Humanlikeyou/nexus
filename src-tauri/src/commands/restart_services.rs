//! Batch service restart for auto-heal: one native approval for the whole set.
//!
//! TS: `invoke<ServiceRestartResult[]>("restart_services", { services, deadlineMs })`
//! where `services` are ids like `"openclaw"` or `"docker:<container>"`.

use super::approval_gate;
use serde::Serialize;

/// Max services in one request.
pub const MAX_SERVICES: usize = 8;
pub const EXPIRED_PREFIX: &str = "expired:";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Service {
    OpenClaw,
    Docker(String),
}

impl Service {
    fn id(&self) -> String {
        match self {
            Service::OpenClaw => "openclaw".to_string(),
            Service::Docker(name) => format!("docker:{name}"),
        }
    }

    fn command_line(&self) -> String {
        match self {
            Service::OpenClaw => super::openclaw::OPENCLAW_RESTART_OPERATION.to_string(),
            Service::Docker(name) => super::system::docker_restart_prompt(name).0,
        }
    }

    async fn restart(&self) -> Result<String, String> {
        match self {
            Service::OpenClaw => super::openclaw::kickstart_openclaw().await,
            Service::Docker(name) => super::system::docker_restart(name).await,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRestartResult {
    pub service: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Parse and validate the whole list before any dialog: non-empty, capped,
/// allow-listed ids only, no duplicates.
fn parse_services(services: &[String]) -> Result<Vec<Service>, String> {
    if services.is_empty() {
        return Err("No services to restart.".to_string());
    }
    if services.len() > MAX_SERVICES {
        return Err(format!("Too many services (max {MAX_SERVICES})."));
    }
    let mut parsed: Vec<Service> = Vec::with_capacity(services.len());
    for raw in services {
        let service = match raw.as_str() {
            "openclaw" => Service::OpenClaw,
            other => match other.strip_prefix("docker:") {
                Some(name) => {
                    super::system::validate_container_name(name)?;
                    Service::Docker(name.to_string())
                }
                None => return Err(format!("Unknown service: {raw:?}")),
            },
        };
        if parsed.contains(&service) {
            return Err(format!("Duplicate service: {raw:?}"));
        }
        parsed.push(service);
    }
    Ok(parsed)
}

fn lock_key(services: &[Service]) -> String {
    let mut ids: Vec<String> = services.iter().map(Service::id).collect();
    ids.sort();
    format!("restart_services:{}", ids.join(","))
}

fn dialog_text(services: &[Service]) -> Result<String, String> {
    let lines: Vec<String> = services.iter().map(Service::command_line).collect();
    approval_gate::approval_text(
        &lines.join("\n"),
        "Auto-heal restarts these services in order. Running requests may be interrupted.",
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn check_deadline(deadline_ms: Option<u64>, now: u64) -> Result<(), String> {
    match deadline_ms {
        Some(deadline) if now > deadline => Err(format!(
            "{EXPIRED_PREFIX} The restart request's deadline passed; nothing was run."
        )),
        _ => Ok(()),
    }
}

#[tauri::command]
pub async fn restart_services(
    app: tauri::AppHandle,
    services: Vec<String>,
    deadline_ms: Option<u64>,
) -> Result<Vec<ServiceRestartResult>, String> {
    let parsed = parse_services(&services)?;
    check_deadline(deadline_ms, now_ms())?;
    let _approval =
        approval_gate::require_approval(&app, &lock_key(&parsed), dialog_text(&parsed)?).await?;
    check_deadline(deadline_ms, now_ms())?;

    let mut results = Vec::with_capacity(parsed.len());
    for service in &parsed {
        let outcome = service.restart().await;
        results.push(to_result(service, outcome));
    }
    Ok(results)
}

fn to_result(service: &Service, outcome: Result<String, String>) -> ServiceRestartResult {
    ServiceRestartResult {
        service: service.id(),
        ok: outcome.is_ok(),
        error: outcome.err(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn validates_list_before_any_dialog() {
        assert_eq!(
            parse_services(&s(&["openclaw", "docker:stack-n8n-1"])).unwrap(),
            vec![Service::OpenClaw, Service::Docker("stack-n8n-1".into())]
        );
        assert!(parse_services(&[]).is_err());
        assert!(parse_services(&s(&["nginx"])).is_err());
        assert!(parse_services(&s(&["docker:-rm"])).is_err());
        assert!(parse_services(&s(&["docker:"])).is_err());
        assert!(parse_services(&s(&["openclaw", "openclaw"])).is_err());
        assert!(parse_services(&s(&["docker:a", "docker:a"])).is_err());
        let many: Vec<String> = (0..=MAX_SERVICES).map(|i| format!("docker:c{i}")).collect();
        assert!(parse_services(&many).is_err());
    }

    #[test]
    fn lock_key_is_order_independent_and_dialog_lists_every_command() {
        let a = parse_services(&s(&["openclaw", "docker:n8n"])).unwrap();
        let b = parse_services(&s(&["docker:n8n", "openclaw"])).unwrap();
        assert_eq!(lock_key(&a), lock_key(&b));
        let text = dialog_text(&a).unwrap();
        assert!(text.contains("Run: launchctl kickstart -k gui/<your uid>/ai.openclaw.gateway"));
        assert!(text.contains("Run: docker restart n8n"));
    }

    #[test]
    fn deadline_expiry() {
        assert!(check_deadline(None, 10).is_ok());
        assert!(check_deadline(Some(10), 10).is_ok());
        let err = check_deadline(Some(10), 11).unwrap_err();
        assert!(err.starts_with(EXPIRED_PREFIX), "{err}");
    }

    #[test]
    fn result_shape_is_camel_case() {
        let ok = to_result(&Service::OpenClaw, Ok("done".into()));
        let bad = to_result(&Service::Docker("n8n".into()), Err("boom".into()));
        assert_eq!(
            serde_json::to_value([ok, bad]).unwrap(),
            serde_json::json!([
                {"service": "openclaw", "ok": true, "error": null},
                {"service": "docker:n8n", "ok": false, "error": "boom"}
            ])
        );
    }
}
