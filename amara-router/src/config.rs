//! Router configuration (`config.toml`) and the optional 0600 secrets file.
//!
//! Model names are never hard-coded: they come from `[models]`. The example file
//! (`config.example.toml`) is committed; the real `config.toml` is local, 0600 and git-ignored.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Ollama `keep_alive`: an integer number of seconds (`-1` = forever, `0` = unload now)
/// or a duration string such as `"5m"`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum KeepAlive {
    Secs(i64),
    Duration(String),
}

impl KeepAlive {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            KeepAlive::Secs(n) => serde_json::json!(n),
            KeepAlive::Duration(s) => serde_json::json!(s),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Ollama base URL, e.g. `http://127.0.0.1:11434`.
    pub ollama_url: String,
    /// Loopback listener, e.g. `127.0.0.1:8787`. No auth (loopback only).
    pub bind: String,
    /// Optional LAN listener, e.g. `0.0.0.0:8787` or `192.168.1.20:8788`. Requires
    /// `AMARA_ROUTER_API_KEY` in `secrets_file`; every request on it needs `Authorization: Bearer`.
    #[serde(default)]
    pub lan_bind: Option<String>,
    /// 0600 env file (`KEY=value` lines) holding `AMARA_ROUTER_API_KEY` and/or `ANTHROPIC_API_KEY`.
    #[serde(default)]
    pub secrets_file: Option<String>,
    pub models: Models,
    #[serde(default)]
    pub routing: Routing,
    #[serde(default)]
    pub loading: Loading,
    #[serde(default)]
    pub filler: Filler,
    #[serde(default)]
    pub cloud: Cloud,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Models {
    /// Resident fast model (exact `ollama list` name).
    pub fast: String,
    /// On-demand deep model (exact `ollama list` name).
    pub deep: String,
    #[serde(default = "default_fast_keep_alive")]
    pub fast_keep_alive: KeepAlive,
    #[serde(default = "default_deep_keep_alive")]
    pub deep_keep_alive: KeepAlive,
    /// Context size the fast model is always loaded with. Every fast request (and the
    /// classifier) uses it, so callers asking for other sizes never force a reload.
    #[serde(default)]
    pub fast_num_ctx: Option<u32>,
    /// Context size for the deep model (kept small: a big KV cache on a 27B model costs GBs).
    #[serde(default)]
    pub deep_num_ctx: Option<u32>,
    /// `think` sent to the fast model when the caller did not set it. Default false: the
    /// fast model's hidden reasoning otherwise costs seconds on short answers.
    #[serde(default = "default_fast_think")]
    pub fast_think: Option<bool>,
    /// `think` sent to the deep model when the caller did not set it (unset = model default).
    #[serde(default)]
    pub deep_think: Option<bool>,
}

fn default_fast_think() -> Option<bool> {
    Some(false)
}

fn default_fast_keep_alive() -> KeepAlive {
    KeepAlive::Secs(-1)
}
fn default_deep_keep_alive() -> KeepAlive {
    KeepAlive::Duration("5m".into())
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Routing {
    /// Latest user message over this many estimated tokens (~4 chars each) goes deep.
    pub token_threshold: u32,
    /// Case-insensitive phrases that send a message deep (stripped before forwarding).
    pub trigger_phrases: Vec<String>,
    /// Run the EASY/HARD classifier on the fast model when no earlier rule matched.
    pub classifier: bool,
    pub classifier_timeout_ms: u64,
    /// Only the first this-many characters of the message go to the classifier (prompt
    /// evaluation dominates its latency; longer messages are caught by the token rule).
    pub classifier_max_chars: usize,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            token_threshold: 2000,
            trigger_phrases: vec![
                "think deeper".into(),
                "think harder".into(),
                "think deeply".into(),
                "think more deeply".into(),
                "deep think".into(),
            ],
            classifier: true,
            classifier_timeout_ms: 2000,
            classifier_max_chars: 400,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Loading {
    /// Load the fast model at startup.
    pub warm_fast_on_start: bool,
    /// After a deep answer, watch `/api/ps`; when deep has unloaded and fast is gone, reload fast.
    pub rewarm_fast_after_deep: bool,
    pub poll_secs: u64,
}

impl Default for Loading {
    fn default() -> Self {
        Self {
            warm_fast_on_start: true,
            rewarm_fast_after_deep: true,
            poll_secs: 20,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Filler {
    /// Stream a short line before a deep-route answer (streamed responses only).
    pub enabled: bool,
    pub text: String,
}

impl Default for Filler {
    fn default() -> Self {
        Self {
            enabled: false,
            text: "Give me a moment to think this through.".into(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Cloud {
    /// Anthropic tier. OFF by default. Even when on, it is used only when a request asks for
    /// it, the caller has not sent `X-Amara-Allow-Claude: 0`, `NEXUS_OUTBOUND_DISABLED` is unset
    /// and `ANTHROPIC_API_KEY` is in the secrets file.
    pub anthropic_enabled: bool,
    pub anthropic_model: String,
    pub anthropic_url: String,
}

impl Default for Cloud {
    fn default() -> Self {
        Self {
            anthropic_enabled: false,
            anthropic_model: "claude-haiku-4-5-20251001".into(),
            anthropic_url: "https://api.anthropic.com/v1/messages".into(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    /// JSONL route log (no message content). `~` expands to $HOME.
    pub path: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            path: "~/Library/Logs/amara-router/routes.jsonl".into(),
        }
    }
}

pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.models.fast.trim().is_empty() || self.models.deep.trim().is_empty() {
            return Err(
                "config: models.fast and models.deep must be set (see `ollama list`)".into(),
            );
        }
        if !self.ollama_url.starts_with("http://") && !self.ollama_url.starts_with("https://") {
            return Err("config: ollama_url must start with http:// or https://".into());
        }
        let bind: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|e| format!("config: bind {:?}: {e}", self.bind))?;
        if !bind.ip().is_loopback() {
            return Err("config: bind must be a loopback address; use lan_bind for the LAN".into());
        }
        if let Some(lan) = &self.lan_bind {
            lan.parse::<std::net::SocketAddr>()
                .map_err(|e| format!("config: lan_bind {lan:?}: {e}"))?;
            if self.secrets_file.is_none() {
                return Err(
                    "config: lan_bind requires secrets_file with AMARA_ROUTER_API_KEY".into(),
                );
            }
        }
        Ok(())
    }

    pub fn ollama(&self, path: &str) -> String {
        format!("{}{}", self.ollama_url.trim_end_matches('/'), path)
    }
}

/// Secrets read from the 0600 env file.
#[derive(Clone, Default)]
pub struct Secrets {
    pub api_key: Option<String>,
    pub anthropic_key: Option<String>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .field(
                "anthropic_key",
                &self.anthropic_key.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

/// Parses `KEY=value` lines (`#` comments, optional `export `, optional quotes).
pub fn parse_env(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            Some((k.trim().to_string(), v))
        })
        .collect()
}

/// Reads the secrets file, refusing it unless it is a regular file readable only by its owner.
pub fn load_secrets(path: &Path) -> Result<Secrets, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("secrets file {}: {e}", path.display()))?;
    if !meta.file_type().is_file() {
        return Err(format!(
            "secrets file {} must be a regular file",
            path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!(
                "secrets file {} is mode {mode:o}; run: chmod 600 {}",
                path.display(),
                path.display()
            ));
        }
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("secrets file: {e}"))?;
    let env = parse_env(&text);
    let get = |k: &str| env.get(k).filter(|v| !v.is_empty()).cloned();
    Ok(Secrets {
        api_key: get("AMARA_ROUTER_API_KEY"),
        anthropic_key: get("ANTHROPIC_API_KEY"),
    })
}

/// `NEXUS_OUTBOUND_DISABLED` = 1/true/yes/on (trimmed, any case), same rule as the Nexus app.
pub fn outbound_disabled() -> bool {
    std::env::var("NEXUS_OUTBOUND_DISABLED")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: &str = r#"
ollama_url = "http://127.0.0.1:11434"
bind = "127.0.0.1:8787"
[models]
fast = "f:latest"
deep = "d:latest"
"#;

    #[test]
    fn defaults_match_the_spec() {
        let c = Config::parse(MIN).unwrap();
        assert_eq!(c.routing.token_threshold, 2000);
        assert_eq!(c.routing.classifier_timeout_ms, 2000);
        assert_eq!(c.models.fast_keep_alive, KeepAlive::Secs(-1));
        assert_eq!(c.models.deep_keep_alive, KeepAlive::Duration("5m".into()));
        assert!(!c.cloud.anthropic_enabled);
        assert!(!c.filler.enabled);
        assert!(c.lan_bind.is_none());
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../config.example.toml");
        let c = Config::parse(text).unwrap();
        assert!(
            !c.cloud.anthropic_enabled,
            "cloud must be off in the example"
        );
        assert!(c.lan_bind.is_none(), "LAN bind must be off in the example");
        assert_eq!(c.bind, "127.0.0.1:8787");
    }

    #[test]
    fn rejects_non_loopback_bind_and_lan_without_secrets() {
        let bad = MIN.replace("127.0.0.1:8787", "0.0.0.0:8787");
        assert!(Config::parse(&bad).unwrap_err().contains("loopback"));
        let lan = MIN.replace(
            "bind = \"127.0.0.1:8787\"",
            "bind = \"127.0.0.1:8787\"\nlan_bind = \"0.0.0.0:8788\"",
        );
        assert!(Config::parse(&lan).unwrap_err().contains("secrets_file"));
        let unknown = format!("{MIN}\nbogus = 1");
        assert!(Config::parse(&unknown).is_err());
    }

    #[test]
    fn secrets_file_must_be_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.env");
        std::fs::write(
            &p,
            "export AMARA_ROUTER_API_KEY=\"abc\"\n# c\nANTHROPIC_API_KEY=\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_secrets(&p).unwrap_err().contains("chmod 600"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        let s = load_secrets(&p).unwrap();
        assert_eq!(s.api_key.as_deref(), Some("abc"));
        assert!(s.anthropic_key.is_none());
        assert!(!format!("{s:?}").contains("abc"));
    }
}
