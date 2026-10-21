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

/// Per-service keys shared with the single-service commands.
fn lock_keys(services: &[Service]) -> Vec<String> {
    let mut keys: Vec<String> = services
        .iter()
        .map(|s| approval_gate::service_key(&s.id()))
        .collect();
    keys.sort();
    keys
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

/// Longest a restart approval stays valid, whatever the caller asks for.
pub const MAX_DEADLINE_WINDOW_MS: u64 = 5 * 60 * 1000;

/// min(requested, now + 5 min); now + 5 min when not given.
fn effective_deadline(requested: Option<u64>, now: u64) -> u64 {
    let cap = now.saturating_add(MAX_DEADLINE_WINDOW_MS);
    requested.map_or(cap, |d| d.min(cap))
}

fn check_deadline(deadline: u64, now: u64) -> Result<(), String> {
    if now > deadline {
        Err(format!(
            "{EXPIRED_PREFIX} The restart request's deadline passed; nothing was run."
        ))
    } else {
        Ok(())
    }
}

/// Run restarts in order, re-checking the deadline before each one. If the
/// deadline passes before anything ran, the whole call is `Err(expired:)`;
/// otherwise each remaining service gets `{ok:false, error:"expired: …"}`.
async fn run_sequential<S, R, Fut, N>(
    services: &[S],
    deadline: u64,
    mut now: N,
    mut restart: R,
    id: impl Fn(&S) -> String,
) -> Result<Vec<ServiceRestartResult>, String>
where
    R: FnMut(&S) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
    N: FnMut() -> u64,
{
    let mut results = Vec::with_capacity(services.len());
    for service in services {
        if let Err(expired) = check_deadline(deadline, now()) {
            if results.is_empty() {
                return Err(expired);
            }
            results.push(ServiceRestartResult {
                service: id(service),
                ok: false,
                error: Some(format!(
                    "{EXPIRED_PREFIX} Deadline passed before this service was restarted."
                )),
            });
            continue;
        }
        let outcome = restart(service).await;
        results.push(ServiceRestartResult {
            service: id(service),
            ok: outcome.is_ok(),
            error: outcome.err(),
        });
    }
    Ok(results)
}

#[tauri::command]
pub async fn restart_services(
    app: tauri::AppHandle,
    services: Vec<String>,
    deadline_ms: Option<u64>,
) -> Result<Vec<ServiceRestartResult>, String> {
    let parsed = parse_services(&services)?;
    let deadline = effective_deadline(deadline_ms, now_ms());
    check_deadline(deadline, now_ms())?;
    let _approval =
        approval_gate::require_approval(&app, &lock_keys(&parsed), dialog_text(&parsed)?).await?;
    check_deadline(deadline, now_ms())?;
    run_sequential(
        &parsed,
        deadline,
        now_ms,
        |s| {
            let s = s.clone();
            async move { s.restart().await }
        },
        Service::id,
    )
    .await
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
        assert_eq!(lock_keys(&a), lock_keys(&b));
        assert_eq!(
            lock_keys(&a),
            vec!["svc:docker:n8n".to_string(), "svc:openclaw".to_string()]
        );
        let text = dialog_text(&a).unwrap();
        assert!(text.contains("Run: launchctl kickstart -k gui/<your uid>/ai.openclaw.gateway"));
        assert!(text.contains("Run: docker restart n8n"));
    }

    #[test]
    fn deadline_is_capped_at_five_minutes_and_defaults_to_it() {
        let now = 1_000_000;
        assert_eq!(effective_deadline(None, now), now + MAX_DEADLINE_WINDOW_MS);
        assert_eq!(effective_deadline(Some(now + 10), now), now + 10);
        assert_eq!(
            effective_deadline(Some(u64::MAX), now),
            now + MAX_DEADLINE_WINDOW_MS
        );
        assert!(check_deadline(10, 10).is_ok());
        assert!(check_deadline(10, 11)
            .unwrap_err()
            .starts_with(EXPIRED_PREFIX));
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[tokio::test]
    async fn expiry_before_anything_ran_is_an_error() {
        let svcs = ids(&["a", "b"]);
        let err = run_sequential(
            &svcs,
            10,
            || 11,
            |_| async { Ok("x".into()) },
            |s| s.clone(),
        )
        .await
        .unwrap_err();
        assert!(err.starts_with(EXPIRED_PREFIX));
    }

    #[tokio::test]
    async fn expiry_mid_batch_marks_remaining_services_and_failures_continue() {
        let svcs = ids(&["a", "b", "c"]);
        let clock = std::cell::Cell::new(0u64);
        let results = run_sequential(
            &svcs,
            10,
            || {
                let t = clock.get();
                clock.set(t + 6); // 0, 6, 12
                t
            },
            |s: &String| {
                let fail = s == "a";
                async move {
                    if fail {
                        Err("boom".to_string())
                    } else {
                        Ok("ok".to_string())
                    }
                }
            },
            |s| s.clone(),
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(&results).unwrap(),
            serde_json::json!([
                {"service": "a", "ok": false, "error": "boom"},
                {"service": "b", "ok": true, "error": null},
                {"service": "c", "ok": false,
                 "error": "expired: Deadline passed before this service was restarted."}
            ])
        );
    }
}
