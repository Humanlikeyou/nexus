//! The static service table: ids (the TS ids in `adapters.ts`), container names, LaunchAgent labels,
//! loopback bases and per-service policy. Everything here is a constant; nothing is read from disk,
//! the network or the environment. `Registry::new` refuses any health base that is not a clean
//! loopback `http://host[:port]` (parsed, so userinfo tricks and `host.docker.internal` fail).

use serde::{Deserialize, Serialize};

use crate::commands::loopback_http::loopback_base;

pub(crate) const LABEL_ROUTER: &str = "com.taisun.amara-router";
pub(crate) const LABEL_HERMES: &str = "ai.hermes.gateway";
pub(crate) const LABEL_VOICE: &str = "com.thevisualdude.amara-voice";
pub(crate) const LABEL_OPEN_WEBUI: &str = "com.thevisualdude.open-webui";

pub(crate) const CONTAINER_N8N: &str = "stack-n8n-1";
pub(crate) const CONTAINER_CREW: &str = "stack-crew-1";
pub(crate) const CONTAINER_POSTGRES: &str = "stack-postgres-1";
pub(crate) const CONTAINER_LETTA: &str = "stack-letta-1";
pub(crate) const CONTAINER_WEAVIATE: &str = "stack-weaviate-1";
pub(crate) const CONTAINER_VERBA: &str = "stack-verba-1";
pub(crate) const CONTAINER_HA: &str = "homeassistant";
pub(crate) const CONTAINER_FREQTRADE: &str = "freqtrade-live";

/// The ids the Services screen uses (`src-console/standalone/adapters.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub enum ServiceId {
    #[serde(rename = "docker")]
    Docker,
    #[serde(rename = "model-runtime")]
    ModelRuntime,
    #[serde(rename = "amara-router")]
    AmaraRouter,
    #[serde(rename = "hermes-gateway")]
    HermesGateway,
    #[serde(rename = "amara-voice")]
    AmaraVoice,
    #[serde(rename = "n8n")]
    N8n,
    #[serde(rename = "crew-service")]
    CrewService,
    #[serde(rename = "postgres")]
    Postgres,
    #[serde(rename = "letta")]
    Letta,
    #[serde(rename = "weaviate")]
    Weaviate,
    #[serde(rename = "health-reader")]
    HealthReader,
    #[serde(rename = "health-sync")]
    HealthSync,
    #[serde(rename = "homeassistant")]
    Homeassistant,
    #[serde(rename = "freqtrade")]
    Freqtrade,
    #[serde(rename = "open-webui")]
    OpenWebui,
}

/// What runs the service. Only used to pick the grace period and (later lane) the verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Launchd(&'static str),
    Docker,
    DockerDesktop,
    App,
    BuiltIn,
}

/// How health is read. Always read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// `GET <base><path>`; the base is validated loopback.
    Http {
        base: &'static str,
        path: &'static str,
    },
    /// Bare TCP connect to 127.0.0.1:port, nothing sent.
    Tcp {
        port: u16,
    },
    /// Only the container state decides (Postgres).
    ContainerOnly,
    /// `docker info`.
    DockerInfo,
    /// In-process, from `whoop_status`.
    BuiltInReader,
    BuiltInSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Probed, and restarted by the machine when act is on.
    Supervised,
    /// Probed and shown, never acted on (Ollama, built-ins).
    ObserveOnly,
    /// Down means "not running", not "needs you" (Amara Voice runs only while the app is open).
    Optional,
    /// Someone else's service (Open WebUI): shown, never acted on.
    Separate,
}

/// launchd may be mid-restart (KeepAlive, ThrottleInterval 30): wait this long before acting.
pub(crate) const LAUNCHD_GRACE_MS: u64 = 45_000;
pub(crate) const DOCKER_GRACE_MS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceDef {
    pub id: ServiceId,
    pub runtime: Runtime,
    /// Container names, in order. Two for Weaviate (+ Verba): the row is the worst of them.
    pub containers: &'static [&'static str],
    pub probe: ProbeKind,
    pub policy: Policy,
    pub grace_ms: u64,
    /// The machine may restart it when act is on (a verb exists for it in the plan).
    pub auto_restart: bool,
    /// The existing approval-gated `service_restart` command handles it today.
    pub manual_restart: bool,
    /// Manual start/stop verbs (later lane; always false in the status until act is on).
    pub manual_start_stop: bool,
}

#[allow(clippy::too_many_arguments)]
const fn def(
    id: ServiceId,
    runtime: Runtime,
    containers: &'static [&'static str],
    probe: ProbeKind,
    policy: Policy,
    auto_restart: bool,
    manual_restart: bool,
    manual_start_stop: bool,
) -> ServiceDef {
    let grace_ms = match runtime {
        Runtime::Docker => DOCKER_GRACE_MS,
        _ => LAUNCHD_GRACE_MS,
    };
    ServiceDef {
        id,
        runtime,
        containers,
        probe,
        policy,
        grace_ms,
        auto_restart,
        manual_restart,
        manual_start_stop,
    }
}

use Policy::{ObserveOnly, Optional, Separate, Supervised};
use ProbeKind::{BuiltInReader, BuiltInSync, ContainerOnly, DockerInfo, Http, Tcp};
use Runtime::{App, BuiltIn, Docker, DockerDesktop, Launchd};
use ServiceId as S;

/// Dependency order (Docker, data, then Amara's own services). Order is also display order.
pub(crate) const DEFS: &[ServiceDef] = &[
    def(
        S::Docker,
        DockerDesktop,
        &[],
        DockerInfo,
        Supervised,
        false,
        false,
        false,
    ),
    def(
        S::ModelRuntime,
        App,
        &[],
        Tcp { port: 11434 },
        ObserveOnly,
        false,
        false,
        false,
    ),
    def(
        S::AmaraRouter,
        Launchd(LABEL_ROUTER),
        &[],
        Http {
            base: "http://127.0.0.1:8787",
            path: "/health",
        },
        Supervised,
        true,
        true,
        false,
    ),
    def(
        S::HermesGateway,
        Launchd(LABEL_HERMES),
        &[],
        Tcp { port: 8642 },
        Supervised,
        true,
        true,
        false,
    ),
    def(
        S::AmaraVoice,
        Launchd(LABEL_VOICE),
        &[],
        Tcp { port: 8791 },
        Optional,
        false,
        false,
        false,
    ),
    def(
        S::N8n,
        Docker,
        &[CONTAINER_N8N],
        Http {
            base: "http://127.0.0.1:5678",
            path: "/healthz",
        },
        Supervised,
        true,
        true,
        true,
    ),
    def(
        S::CrewService,
        Docker,
        &[CONTAINER_CREW],
        Http {
            base: "http://127.0.0.1:8400",
            path: "/health",
        },
        Supervised,
        true,
        true,
        true,
    ),
    def(
        S::Postgres,
        Docker,
        &[CONTAINER_POSTGRES],
        ContainerOnly,
        Supervised,
        true,
        true,
        true,
    ),
    def(
        S::Letta,
        Docker,
        &[CONTAINER_LETTA],
        Tcp { port: 8283 },
        Supervised,
        true,
        true,
        true,
    ),
    def(
        S::Weaviate,
        Docker,
        &[CONTAINER_WEAVIATE, CONTAINER_VERBA],
        Http {
            base: "http://127.0.0.1:8001",
            path: "/api/health",
        },
        Supervised,
        true,
        true,
        true,
    ),
    def(
        S::HealthReader,
        BuiltIn,
        &[],
        BuiltInReader,
        ObserveOnly,
        false,
        false,
        false,
    ),
    def(
        S::HealthSync,
        BuiltIn,
        &[],
        BuiltInSync,
        ObserveOnly,
        false,
        false,
        false,
    ),
    def(
        S::Homeassistant,
        Docker,
        &[CONTAINER_HA],
        Tcp { port: 8123 },
        Supervised,
        true,
        false,
        true,
    ),
    // Restart and start only: stopping Freqtrade stays gated.
    def(
        S::Freqtrade,
        Docker,
        &[CONTAINER_FREQTRADE],
        Tcp { port: 8082 },
        Supervised,
        true,
        false,
        false,
    ),
    def(
        S::OpenWebui,
        Launchd(LABEL_OPEN_WEBUI),
        &[],
        Tcp { port: 3000 },
        Separate,
        false,
        false,
        false,
    ),
];

impl ServiceId {
    /// The UI id (`"amara-router"`, `"n8n"`, ...) as a typed id; anything else is None.
    pub fn parse(id: &str) -> Option<ServiceId> {
        serde_json::from_value(serde_json::Value::String(id.to_string())).ok()
    }

    #[cfg(test)]
    pub const ALL: [ServiceId; 15] = [
        S::Docker,
        S::ModelRuntime,
        S::AmaraRouter,
        S::HermesGateway,
        S::AmaraVoice,
        S::N8n,
        S::CrewService,
        S::Postgres,
        S::Letta,
        S::Weaviate,
        S::HealthReader,
        S::HealthSync,
        S::Homeassistant,
        S::Freqtrade,
        S::OpenWebui,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    /// A health base is not a clean loopback http URL.
    NotLoopback(ServiceId),
    /// Two rows share an id.
    Duplicate(ServiceId),
    /// A container name is not a valid docker name.
    BadContainer(ServiceId),
}

#[derive(Debug, Clone)]
pub struct Registry {
    defs: Vec<ServiceDef>,
}

impl Registry {
    /// Refuses a non-loopback base, a duplicate id or an invalid container name.
    pub fn new(defs: &[ServiceDef]) -> Result<Self, RegistryError> {
        for (i, d) in defs.iter().enumerate() {
            if defs[..i].iter().any(|o| o.id == d.id) {
                return Err(RegistryError::Duplicate(d.id));
            }
            if let ProbeKind::Http { base, .. } = d.probe {
                // loopback_base also refuses userinfo, a path, a query and https.
                if loopback_base(base).as_deref() != Some(base) {
                    return Err(RegistryError::NotLoopback(d.id));
                }
            }
            if d.containers
                .iter()
                .any(|c| crate::commands::system::validate_container_name(c).is_err())
            {
                return Err(RegistryError::BadContainer(d.id));
            }
        }
        Ok(Self {
            defs: defs.to_vec(),
        })
    }

    /// The real table. Panics only if the constants above are edited to something invalid; the
    /// tests below build it, so that cannot ship.
    pub fn builtin() -> Self {
        Self::new(DEFS).expect("the built-in service table is valid")
    }

    pub fn defs(&self) -> &[ServiceDef] {
        &self.defs
    }

    pub fn get(&self, id: ServiceId) -> Option<&ServiceDef> {
        self.defs.iter().find(|d| d.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(base: &'static str) -> ServiceDef {
        def(
            S::N8n,
            Docker,
            &[CONTAINER_N8N],
            Http {
                base,
                path: "/healthz",
            },
            Supervised,
            true,
            true,
            true,
        )
    }

    #[test]
    fn the_built_in_table_is_valid_and_covers_every_ui_id_once() {
        let r = Registry::builtin();
        assert_eq!(r.defs().len(), ServiceId::ALL.len());
        for id in ServiceId::ALL {
            assert!(r.get(id).is_some(), "{id:?} missing");
        }
    }

    #[test]
    fn ids_serialize_as_the_ui_ids() {
        let ids: Vec<String> = ServiceId::ALL
            .iter()
            .map(|i| {
                serde_json::to_value(i)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            ids.join(","),
            "docker,model-runtime,amara-router,hermes-gateway,amara-voice,n8n,crew-service,postgres,letta,weaviate,health-reader,health-sync,homeassistant,freqtrade,open-webui"
        );
    }

    #[test]
    fn refuses_every_non_loopback_base_at_construction() {
        for bad in [
            "http://0.0.0.0:5678",
            "http://192.168.1.20:5678",
            "http://10.0.0.5:5678",
            "http://host.docker.internal:5678",
            "http://127.0.0.1@evil.example:5678",
            "http://localhost:1@evil.example:5678",
            "http://[::]:5678",
            "https://127.0.0.1:5678",
            "http://127.0.0.1:5678/extra",
            "http://example.com",
        ] {
            assert_eq!(
                Registry::new(&[http(bad)]).unwrap_err(),
                RegistryError::NotLoopback(S::N8n),
                "{bad} must be refused"
            );
        }
        assert!(Registry::new(&[http("http://127.0.0.1:5678")]).is_ok());
        assert!(Registry::new(&[http("http://localhost:5678")]).is_ok());
    }

    #[test]
    fn refuses_duplicates_and_bad_container_names() {
        let a = http("http://127.0.0.1:5678");
        assert_eq!(
            Registry::new(&[a, a]).unwrap_err(),
            RegistryError::Duplicate(S::N8n)
        );
        let flag = def(
            S::N8n,
            Docker,
            &["--rm"],
            ContainerOnly,
            Supervised,
            true,
            true,
            true,
        );
        assert_eq!(
            Registry::new(&[flag]).unwrap_err(),
            RegistryError::BadContainer(S::N8n)
        );
    }

    #[test]
    fn constants_match_the_plan() {
        let r = Registry::builtin();
        assert_eq!(
            r.get(S::Weaviate).unwrap().containers,
            &["stack-weaviate-1", "stack-verba-1"]
        );
        assert_eq!(
            r.get(S::AmaraRouter).unwrap().runtime,
            Runtime::Launchd("com.taisun.amara-router")
        );
        assert_eq!(
            r.get(S::HermesGateway).unwrap().runtime,
            Runtime::Launchd("ai.hermes.gateway")
        );
        assert_eq!(r.get(S::OpenWebui).unwrap().policy, Policy::Separate);
        assert!(!r.get(S::OpenWebui).unwrap().auto_restart);
        assert!(!r.get(S::Freqtrade).unwrap().manual_start_stop);
        assert_eq!(r.get(S::N8n).unwrap().grace_ms, DOCKER_GRACE_MS);
        assert_eq!(r.get(S::AmaraRouter).unwrap().grace_ms, LAUNCHD_GRACE_MS);
        // Manual restart matches what service_control.rs supports today.
        for id in [S::N8n, S::CrewService, S::Postgres, S::Letta, S::Weaviate] {
            assert!(r.get(id).unwrap().manual_restart);
        }
    }
}
