//! "Who is working right now" (Agents page feed). A read-only aggregator over in-memory state that only the
//! backend knows: Amara turns in flight (the turn registry) and supervisor service tasks (starting or
//! restarting). It never touches the database, the network or a file, and the output holds only fixed labels,
//! a kind, a status and a start time. Prompt text, message text, ids, paths and health values never leave here.
//! Sources that live elsewhere (crew runs, Amara actions, the outbox, renderer jobs) are read by the page
//! from the system snapshot it already has.

use super::amara_turn::TurnRegistry;
use super::supervisor::machine::ServiceState;
use super::supervisor::registry::ServiceId;
use super::supervisor::{services_status, ServiceStatus};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NowItem {
    /// "amara" or "service".
    pub kind: String,
    /// Who: a fixed label.
    pub who: String,
    /// What: a fixed label.
    pub what: String,
    /// "running" or "queued".
    pub status: String,
    /// Epoch ms when it started, when known.
    pub since_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentsNow {
    pub items: Vec<NowItem>,
}

fn service_label(id: ServiceId) -> &'static str {
    match id {
        ServiceId::Docker => "Docker",
        ServiceId::ModelRuntime => "Model runtime",
        ServiceId::AmaraRouter => "Amara router",
        ServiceId::HermesGateway => "Amara gateway",
        ServiceId::AmaraVoice => "Amara voice",
        ServiceId::N8n => "Automations",
        ServiceId::CrewService => "Crew service",
        ServiceId::Postgres => "Database",
        ServiceId::Letta => "Memory",
        ServiceId::Weaviate => "Knowledge base",
        ServiceId::HealthReader => "Health reader",
        ServiceId::HealthSync => "Health sync",
        ServiceId::Homeassistant => "Home Assistant",
        ServiceId::Freqtrade => "Trading",
        ServiceId::OpenWebui => "Web chat",
    }
}

/// Pure core: turn start times plus service statuses in, rows out.
pub(crate) fn build(turns: &[u64], services: &[ServiceStatus]) -> AgentsNow {
    let mut items: Vec<NowItem> = turns
        .iter()
        .map(|t| NowItem {
            kind: "amara".into(),
            who: "Amara".into(),
            what: "Answering you".into(),
            status: "running".into(),
            since_ms: Some(*t),
        })
        .collect();
    for s in services {
        let what = match s.state {
            ServiceState::Starting => "Starting up",
            ServiceState::Restarting => "Restarting",
            _ => continue,
        };
        items.push(NowItem {
            kind: "service".into(),
            who: service_label(s.id).into(),
            what: what.into(),
            status: "running".into(),
            since_ms: if s.since_ms > 0 {
                Some(s.since_ms)
            } else {
                None
            },
        });
    }
    AgentsNow { items }
}

#[tauri::command]
#[specta::specta]
pub async fn agents_now(registry: State<'_, TurnRegistry>) -> Result<AgentsNow, String> {
    let turns = registry.started_ms();
    let services = services_status().await;
    Ok(build(&turns, &services))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::supervisor::machine::ReasonCode;
    use crate::commands::supervisor::{Can, Owner};

    fn svc(id: ServiceId, state: ServiceState, since: u64) -> ServiceStatus {
        ServiceStatus {
            id,
            state,
            reason_code: ReasonCode::NotChecked,
            since_ms: since,
            checked_ms: 0,
            attempt: 0,
            max_attempts: 3,
            owner: Owner::User,
            adopted: false,
            can: Can {
                restart: false,
                start: false,
                stop: false,
            },
        }
    }

    #[test]
    fn nothing_running_is_empty() {
        let r = build(
            &[],
            &[
                svc(ServiceId::N8n, ServiceState::Healthy, 5),
                svc(ServiceId::Letta, ServiceState::Stopped, 5),
            ],
        );
        assert!(r.items.is_empty());
    }

    #[test]
    fn turns_and_transitioning_services_become_rows() {
        let r = build(
            &[100, 200],
            &[
                svc(ServiceId::CrewService, ServiceState::Starting, 300),
                svc(ServiceId::Weaviate, ServiceState::Restarting, 0),
                svc(ServiceId::N8n, ServiceState::Degraded, 9),
            ],
        );
        assert_eq!(r.items.len(), 4);
        assert_eq!(r.items[0].who, "Amara");
        assert_eq!(r.items[0].since_ms, Some(100));
        assert_eq!(r.items[2].who, "Crew service");
        assert_eq!(r.items[2].what, "Starting up");
        assert_eq!(r.items[3].since_ms, None);
    }

    #[test]
    fn registry_exposes_only_start_times() {
        let reg = TurnRegistry::default();
        let _n = reg.begin("conv-with-secret-text", "turn-1").unwrap();
        let started = reg.started_ms();
        assert_eq!(started.len(), 1);
        let json = serde_json::to_string(&build(&started, &[])).unwrap();
        assert!(!json.contains("secret") && !json.contains("turn-1") && !json.contains("conv"));
        reg.finish("turn-1");
        assert!(reg.started_ms().is_empty());
    }

    #[test]
    fn output_is_fixed_labels_only() {
        let r = build(
            &[1],
            &[svc(ServiceId::HermesGateway, ServiceState::Restarting, 2)],
        );
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"items":[{"kind":"amara","who":"Amara","what":"Answering you","status":"running","sinceMs":1},{"kind":"service","who":"Amara gateway","what":"Restarting","status":"running","sinceMs":2}]}"#
        );
    }
}
