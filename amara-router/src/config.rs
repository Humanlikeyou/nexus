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
    /// Allow `ollama_url` to be a non-loopback host (prompts then leave this Mac). Off by default;
    /// refused anyway while `NEXUS_OUTBOUND_DISABLED` is set. Independent of `cloud.anthropic_enabled`.
    #[serde(default)]
    pub allow_remote_ollama: bool,
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
    /// Context size the fast model is always loaded with. Every fast request, the classifier
    /// and every warm-up send it as `options.num_ctx`, so callers asking for other sizes never
    /// force a reload (and Ollama never falls back to the model's huge default context).
    /// Must stay at or above the Hermes floor (see `DEFAULT_FAST_NUM_CTX`).
    #[serde(default = "default_fast_num_ctx")]
    pub fast_num_ctx: u32,
    /// Context size for the deep model (below the Hermes floor: a big KV cache on a 27B model
    /// costs GBs). A long Hermes session routed to deep can exceed it.
    #[serde(default = "default_deep_num_ctx")]
    pub deep_num_ctx: u32,
    /// `think` sent to the fast model when the caller did not set it. Default false: the
    /// fast model's hidden reasoning otherwise costs seconds on short answers.
    #[serde(default = "default_fast_think")]
    pub fast_think: Option<bool>,
    /// `think` sent to the deep model when the caller did not set it. Default false: hidden
    /// reasoning ran 280 s with no reply and blocked other clients behind 503s.
    #[serde(default = "default_deep_think")]
    pub deep_think: Option<bool>,
}

/// Default deep `num_ctx` (see README "Context size").
pub const DEFAULT_DEEP_NUM_CTX: u32 = 32_768;
/// Default fast `num_ctx`: Hermes refuses any window under 64,000 tokens.
pub const DEFAULT_FAST_NUM_CTX: u32 = 65_536;
/// Accepted `num_ctx` range.
pub const NUM_CTX_RANGE: std::ops::RangeInclusive<u32> = 512..=262_144;
/// Accepted `routing.classifier_timeout_ms` range.
pub const CLASSIFIER_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 100..=10_000;
/// Accepted `routing.residency_check_timeout_ms` range.
pub const RESIDENCY_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 50..=2_000;
/// Accepted `loading.gpu_budget_gb` range.
pub const GPU_BUDGET_GB_RANGE: std::ops::RangeInclusive<f64> = 1.0..=512.0;
/// Accepted `loading.first_token_timeout_ms` range.
pub const FIRST_TOKEN_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 1_000..=300_000;
/// Accepted `loading.first_token_timeout_resident_ms` range.
pub const FIRST_TOKEN_RESIDENT_RANGE: std::ops::RangeInclusive<u64> = 1_000..=600_000;
/// Accepted `loading.deep_load_timeout_ms` range.
pub const DEEP_LOAD_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 100..=300_000;
/// Accepted `loading.deep_grace_secs` range.
pub const DEEP_GRACE_RANGE: std::ops::RangeInclusive<u64> = 0..=3_600;
/// Accepted `loading.fast_wait_ms` range.
pub const FAST_WAIT_RANGE: std::ops::RangeInclusive<u64> = 100..=600_000;

fn default_fast_num_ctx() -> u32 {
    DEFAULT_FAST_NUM_CTX
}

fn default_deep_num_ctx() -> u32 {
    DEFAULT_DEEP_NUM_CTX
}

fn default_fast_think() -> Option<bool> {
    Some(false)
}

fn default_deep_think() -> Option<bool> {
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
    /// Send long prompts, code blocks and classifier/heuristic HARD verdicts to deep
    /// automatically. Off (the default) keeps deep to explicit asks (`/deep`, a route header,
    /// the deep alias or a trigger phrase).
    pub auto_deep: bool,
    /// Case-insensitive phrases that send a message deep (stripped before forwarding).
    pub trigger_phrases: Vec<String>,
    /// Run the EASY/HARD classifier on the fast model when no earlier rule matched.
    pub classifier: bool,
    /// Hard timeout for the classifier call (100-10000 ms).
    pub classifier_timeout_ms: u64,
    /// Before classifying, `/api/ps` is checked with this timeout (50-2000 ms). If the fast
    /// model is not resident (or `/api/ps` does not answer in time) the classifier is skipped:
    /// it would only time out while fast loads.
    pub residency_check_timeout_ms: u64,
    /// Only the first this-many characters of the message go to the classifier (prompt
    /// evaluation dominates its latency; longer messages are caught by the token rule).
    pub classifier_max_chars: usize,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            token_threshold: 2000,
            auto_deep: false,
            trigger_phrases: vec![
                "think deeper".into(),
                "think harder".into(),
                "think deeply".into(),
                "think more deeply".into(),
                "deep think".into(),
            ],
            classifier: true,
            classifier_timeout_ms: 2000,
            residency_check_timeout_ms: 1000,
            classifier_max_chars: 400,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Loading {
    /// Load the fast model at startup (in the background; startup never waits for it).
    pub warm_fast_on_start: bool,
    /// Also load the deep model at startup, after fast. Off by default: on low RAM both may
    /// not fit, and loading deep could evict fast.
    pub warm_deep_on_start: bool,
    /// Re-warm fast in the background once deep's keep-alive has ended.
    pub rewarm_fast_after_deep: bool,
    /// Seconds between `/api/ps` checks after deep work, to re-warm fast once deep has unloaded.
    pub poll_secs: u64,
    /// GPU memory the router may fill (GB, 1-512). Before a fast request moves an idle deep model
    /// out of memory, `/api/ps` is read: if everything loaded plus fast's last seen `size_vram`
    /// fits in this budget, deep stays and fast loads beside it.
    pub gpu_budget_gb: f64,
    /// Seconds after the last deep reply during which a fast request does NOT unload deep (0-3600;
    /// 0 = unload at once, the old behaviour). The fast request waits out the grace (up to
    /// `fast_wait_ms`) unless fast and deep fit together in `gpu_budget_gb`.
    pub deep_grace_secs: u64,
    /// Ollama must send its first byte within this time (1000-300000 ms), or the request gets
    /// a 503 "model loading" reply while the load continues in the background.
    pub first_token_timeout_ms: u64,
    /// First-byte timeout when the model is already resident per `/api/ps` (1000-600000 ms).
    /// Ollama streams nothing during prompt evaluation, so long prompts need longer.
    pub first_token_timeout_resident_ms: u64,
    /// How long a deep request may wait for fast to go quiet and deep to load (100-300000 ms),
    /// before it is answered on fast with reason `deep_unavailable`. Prompt evaluation comes after
    /// this, under `first_token_timeout_resident_ms`.
    pub deep_load_timeout_ms: u64,
    /// How long a fast request waits while deep is loading or answering (100-600000 ms), before it
    /// gets a 503 with `Retry-After`.
    pub fast_wait_ms: u64,
}

impl Default for Loading {
    fn default() -> Self {
        Self {
            warm_fast_on_start: true,
            warm_deep_on_start: false,
            rewarm_fast_after_deep: true,
            poll_secs: 20,
            gpu_budget_gb: 21.0,
            deep_grace_secs: 60,
            first_token_timeout_ms: 20_000,
            first_token_timeout_resident_ms: 300_000,
            deep_load_timeout_ms: 90_000,
            fast_wait_ms: 120_000,
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
        Self::parse_with(text, outbound_disabled())
    }

    /// Like [`Config::parse`] but with the `NEXUS_OUTBOUND_DISABLED` value passed in
    /// (tests use this so they never touch the process environment).
    pub fn parse_with(text: &str, outbound_disabled: bool) -> Result<Self, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        cfg.validate_with(outbound_disabled)?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_with(outbound_disabled())
    }

    /// Validates the config; `outbound_disabled` is the `NEXUS_OUTBOUND_DISABLED` state.
    pub fn validate_with(&self, outbound_disabled: bool) -> Result<(), String> {
        if self.models.fast.trim().is_empty() || self.models.deep.trim().is_empty() {
            return Err(
                "config: models.fast and models.deep must be set (see `ollama list`)".into(),
            );
        }
        if !self.ollama_url.starts_with("http://") && !self.ollama_url.starts_with("https://") {
            return Err("config: ollama_url must start with http:// or https://".into());
        }
        let host = url_host(&self.ollama_url).ok_or("config: ollama_url has no host")?;
        if !is_loopback_host(host) {
            if !self.allow_remote_ollama {
                return Err("config: ollama_url must be a loopback host (127.0.0.0/8, ::1, localhost) unless allow_remote_ollama is true".into());
            }
            if outbound_disabled {
                return Err("config: ollama_url must be a loopback host while NEXUS_OUTBOUND_DISABLED is set".into());
            }
        }
        if !anthropic_url_allowed(&self.cloud.anthropic_url) {
            return Err("config: cloud.anthropic_url must be https://api.anthropic.com (port 443) or a loopback mock".into());
        }
        for (name, n) in [
            ("models.fast_num_ctx", self.models.fast_num_ctx),
            ("models.deep_num_ctx", self.models.deep_num_ctx),
        ] {
            if !NUM_CTX_RANGE.contains(&n) {
                return Err(format!(
                    "config: {name} must be between {} and {}",
                    NUM_CTX_RANGE.start(),
                    NUM_CTX_RANGE.end()
                ));
            }
        }
        for (name, v, r) in [
            (
                "routing.classifier_timeout_ms",
                self.routing.classifier_timeout_ms,
                CLASSIFIER_TIMEOUT_RANGE,
            ),
            (
                "routing.residency_check_timeout_ms",
                self.routing.residency_check_timeout_ms,
                RESIDENCY_TIMEOUT_RANGE,
            ),
            (
                "loading.first_token_timeout_ms",
                self.loading.first_token_timeout_ms,
                FIRST_TOKEN_TIMEOUT_RANGE,
            ),
            (
                "loading.first_token_timeout_resident_ms",
                self.loading.first_token_timeout_resident_ms,
                FIRST_TOKEN_RESIDENT_RANGE,
            ),
            (
                "loading.deep_load_timeout_ms",
                self.loading.deep_load_timeout_ms,
                DEEP_LOAD_TIMEOUT_RANGE,
            ),
            (
                "loading.fast_wait_ms",
                self.loading.fast_wait_ms,
                FAST_WAIT_RANGE,
            ),
            (
                "loading.deep_grace_secs",
                self.loading.deep_grace_secs,
                DEEP_GRACE_RANGE,
            ),
        ] {
            if !r.contains(&v) {
                return Err(format!(
                    "config: {name} must be between {} and {}",
                    r.start(),
                    r.end()
                ));
            }
        }
        if self.loading.deep_grace_secs.saturating_mul(1000) > self.loading.fast_wait_ms {
            return Err("config: loading.deep_grace_secs must not exceed loading.fast_wait_ms (a fast request waits out the grace and would otherwise get a 503 every time)".into());
        }
        if !GPU_BUDGET_GB_RANGE.contains(&self.loading.gpu_budget_gb) {
            return Err(format!(
                "config: loading.gpu_budget_gb must be between {} and {}",
                GPU_BUDGET_GB_RANGE.start(),
                GPU_BUDGET_GB_RANGE.end()
            ));
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

/// Host part of an `http(s)://host[:port][/...]` URL (brackets kept off IPv6). None if empty.
fn url_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return None;
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map_or(authority, |(h, _)| h)
    };
    (!host.is_empty()).then_some(host)
}

/// The only Anthropic endpoints allowed: `https://api.anthropic.com[:443]/...` exactly, or a
/// loopback mock (127.0.0.0/8 dotted-quad, `::1`, `localhost`) over http or https.
fn anthropic_url_allowed(url: &str) -> bool {
    let https = url.starts_with("https://");
    if !https && !url.starts_with("http://") {
        return false;
    }
    let Some(host) = url_host(url) else {
        return false;
    };
    if is_spoofy_host(host) {
        return false;
    }
    if is_loopback_host(host) {
        return true;
    }
    https
        && host.eq_ignore_ascii_case("api.anthropic.com")
        && matches!(url_port(url), None | Some("443"))
}

/// Explicit port of an `http(s)://host:port/...` URL, if any.
fn url_port(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let after_host = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once(']').map_or("", |(_, r)| r),
        None => authority.find(':').map_or("", |i| &authority[i..]),
    };
    after_host.strip_prefix(':')
}

/// Hosts that only look local or numeric: `localhost.<anything>`, and integer/hex/octal or
/// short-form IPv4 (`2130706433`, `0x7f000001`, `0177.0.0.1`, `127.1`) that some resolvers
/// expand to 127.0.0.1. Refused for anthropic_url in any scheme.
fn is_spoofy_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower.starts_with("localhost.") {
        return true;
    }
    let numeric = !lower.is_empty()
        && lower.split('.').all(|p| {
            !p.is_empty() && (p.chars().all(|c| c.is_ascii_digit()) || p.starts_with("0x"))
        });
    numeric && lower.parse::<std::net::Ipv4Addr>().is_err()
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Minimum length of `AMARA_ROUTER_API_KEY` for the LAN listener.
pub const MIN_API_KEY_LEN: usize = 24;

/// The one LAN-key rule, used by `run` and by `--check`: when `lan_bind` is set the API key
/// must be present and at least [`MIN_API_KEY_LEN`] chars. Returns the key (or None if no LAN).
pub fn lan_api_key(cfg: &Config, secrets: &Secrets) -> Result<Option<String>, String> {
    if cfg.lan_bind.is_none() {
        return Ok(None);
    }
    secrets
        .api_key
        .clone()
        .filter(|k| k.len() >= MIN_API_KEY_LEN)
        .map(Some)
        .ok_or_else(|| {
            format!("lan_bind needs AMARA_ROUTER_API_KEY ({MIN_API_KEY_LEN}+ chars) in the 0600 secrets file")
        })
}

/// Startup secret rules: the LAN key rule, and: with `cloud.anthropic_enabled` the loopback
/// listener (which can spend the Anthropic key and send data off the Mac) needs a bearer key too.
pub fn check_secrets(cfg: &Config, secrets: &Secrets) -> Result<(), String> {
    lan_api_key(cfg, secrets)?;
    if cfg.cloud.anthropic_enabled
        && secrets
            .api_key
            .as_ref()
            .is_none_or(|k| k.len() < MIN_API_KEY_LEN)
    {
        return Err(format!(
            "cloud.anthropic_enabled needs AMARA_ROUTER_API_KEY ({MIN_API_KEY_LEN}+ chars) in the 0600 secrets file: the loopback listener then requires Authorization: Bearer"
        ));
    }
    Ok(())
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
        assert_eq!(c.routing.residency_check_timeout_ms, 1000);
        assert_eq!(c.models.fast_num_ctx, 65536);
        assert!(c.models.fast_num_ctx >= 64_000, "Hermes floor");
        assert_eq!(c.models.deep_think, Some(false));
        assert_eq!(c.loading.first_token_timeout_resident_ms, 300_000);
        assert_eq!(c.models.deep_num_ctx, 32768);
        assert_eq!(c.loading.first_token_timeout_ms, 20_000);
        assert!(c.loading.warm_fast_on_start);
        assert!(!c.loading.warm_deep_on_start);
        assert_eq!(c.loading.gpu_budget_gb, 21.0);
        assert_eq!(c.models.fast_keep_alive, KeepAlive::Secs(-1));
        assert_eq!(c.models.deep_keep_alive, KeepAlive::Duration("5m".into()));
        assert!(!c.cloud.anthropic_enabled);
        assert!(!c.filler.enabled);
        assert!(c.lan_bind.is_none());
    }

    #[test]
    fn timeouts_and_num_ctx_are_range_checked() {
        let with = |section: &str, line: &str| {
            let text = if section == "models" {
                MIN.replacen("[models]", &format!("[models]\n{line}"), 1)
            } else {
                format!("{MIN}\n[{section}]\n{line}\n")
            };
            Config::parse_with(&text, false)
        };
        assert!(with("routing", "classifier_timeout_ms = 99").is_err());
        assert!(with("routing", "classifier_timeout_ms = 10001").is_err());
        assert_eq!(
            with("routing", "classifier_timeout_ms = 750")
                .unwrap()
                .routing
                .classifier_timeout_ms,
            750
        );
        assert!(with("routing", "residency_check_timeout_ms = 0").is_err());
        assert!(with("routing", "residency_check_timeout_ms = 5000").is_err());
        assert!(with("loading", "first_token_timeout_ms = 500").is_err());
        assert!(with("loading", "first_token_timeout_ms = 300001").is_err());
        assert!(with("loading", "first_token_timeout_ms = 30000").is_ok());
        assert!(with("loading", "first_token_timeout_resident_ms = 999").is_err());
        assert!(with("loading", "first_token_timeout_resident_ms = 600001").is_err());
        assert!(with("loading", "first_token_timeout_resident_ms = 600000").is_ok());
        assert!(with("loading", "gpu_budget_gb = 0.5").is_err());
        assert!(with("loading", "gpu_budget_gb = 1000.0").is_err());
        assert!(with("loading", "gpu_budget_gb = nan").is_err());
        assert_eq!(
            with("loading", "gpu_budget_gb = 24.5")
                .unwrap()
                .loading
                .gpu_budget_gb,
            24.5
        );
        assert!(with("models", "fast_num_ctx = 0").is_err());
        assert!(with("models", "deep_num_ctx = 1048576").is_err());
        assert_eq!(
            with("models", "fast_num_ctx = 16384")
                .unwrap()
                .models
                .fast_num_ctx,
            16384
        );
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
        assert_eq!(c.models.fast_num_ctx, DEFAULT_FAST_NUM_CTX);
        assert_eq!(c.models.deep_num_ctx, DEFAULT_DEEP_NUM_CTX);
        assert_eq!(c.models.deep_think, Some(false));
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

    fn with_ollama(url: &str, cloud: bool) -> String {
        let mut t = MIN.replace("http://127.0.0.1:11434", url);
        if cloud {
            t.push_str("\n[cloud]\nanthropic_enabled = true\n");
        }
        t
    }

    #[test]
    fn remote_ollama_needs_its_own_flag_not_anthropic_enabled() {
        let remote = "http://192.168.1.5:11434";
        // anthropic_enabled alone no longer opens a remote Ollama.
        let e = Config::parse_with(&with_ollama(remote, true), false).unwrap_err();
        assert!(e.contains("allow_remote_ollama"), "{e}");
        // The flag does, unless outbound is disabled.
        let mut t = with_ollama(remote, false);
        t = format!("allow_remote_ollama = true\n{t}");
        assert!(Config::parse_with(&t, false).is_ok());
        assert!(Config::parse_with(&t, true).is_err());
        // Default is off.
        assert!(!Config::parse(MIN).unwrap().allow_remote_ollama);
    }

    #[test]
    fn cloud_tier_needs_a_bearer_key() {
        let mut c = Config::parse(MIN).unwrap();
        let none = Secrets::default();
        let keyed = Secrets {
            api_key: Some("k".repeat(MIN_API_KEY_LEN)),
            anthropic_key: None,
        };
        let short = Secrets {
            api_key: Some("k".into()),
            anthropic_key: None,
        };
        assert!(check_secrets(&c, &none).is_ok());
        c.cloud.anthropic_enabled = true;
        assert!(check_secrets(&c, &none)
            .unwrap_err()
            .contains("AMARA_ROUTER_API_KEY"));
        assert!(check_secrets(&c, &short).is_err());
        assert!(check_secrets(&c, &keyed).is_ok());
    }

    #[test]
    fn deep_grace_cannot_exceed_fast_wait() {
        let t = |extra: &str| format!("{MIN}\n[loading]\n{extra}\n");
        assert!(Config::parse(&t("deep_grace_secs = 120\nfast_wait_ms = 120000")).is_ok());
        let e = Config::parse(&t("deep_grace_secs = 121\nfast_wait_ms = 120000")).unwrap_err();
        assert!(e.contains("deep_grace_secs"), "{e}");
        assert!(Config::parse(&t("deep_grace_secs = 60\nfast_wait_ms = 100")).is_err());
    }

    #[test]
    fn auto_deep_defaults_to_off() {
        assert!(!Routing::default().auto_deep);
        assert!(!Config::parse(MIN).unwrap().routing.auto_deep);
    }

    #[test]
    fn ollama_url_must_be_loopback_unless_cloud_and_outbound_allowed() {
        for ok in [
            "http://127.0.0.1:11434",
            "http://127.5.6.7:11434",
            "http://localhost:11434",
            "http://[::1]:11434",
        ] {
            assert!(
                Config::parse_with(&with_ollama(ok, false), true).is_ok(),
                "{ok}"
            );
        }
        for bad in [
            "http://192.168.1.5:11434",
            "http://evil.example.com",
            "http://127.0.0.1.evil.com:11434",
            "http://user@127.0.0.1:11434",
            "http://",
        ] {
            assert!(
                Config::parse_with(&with_ollama(bad, false), false).is_err(),
                "{bad}"
            );
        }
        let lan = format!(
            "allow_remote_ollama = true\n{}",
            with_ollama("http://192.168.1.5:11434", false)
        );
        assert!(Config::parse_with(&lan, false).is_ok());
        let e = Config::parse_with(&lan, true).unwrap_err();
        assert!(e.contains("NEXUS_OUTBOUND_DISABLED"));
    }

    #[test]
    fn anthropic_url_must_be_https() {
        let t =
            format!("{MIN}\n[cloud]\nanthropic_url = \"http://api.anthropic.com/v1/messages\"\n");
        assert!(Config::parse_with(&t, false).unwrap_err().contains("https"));
        let t = format!("{MIN}\n[cloud]\nanthropic_url = \"ftp://x/y\"\n");
        assert!(Config::parse_with(&t, false).is_err());
        let t = format!("{MIN}\n[cloud]\nanthropic_url = \"http://127.0.0.1:9/v1/messages\"\n");
        assert!(
            Config::parse_with(&t, false).is_ok(),
            "loopback mock allowed"
        );
        for bad in [
            "localhost.evil.com",
            "http://localhost.evil.com/v1/messages",
            "https://localhost.evil.com/v1/messages",
            "http://localhost@evil.com/v1/messages",
            "https://localhost@evil.com/v1/messages",
            "http://2130706433/v1/messages",
            "http://0x7f000001/v1/messages",
            "https://2130706433/v1/messages",
            "http://0177.0.0.1/v1/messages",
            "http://127.1/v1/messages",
        ] {
            let t = format!("{MIN}\n[cloud]\nanthropic_url = \"{bad}\"\n");
            assert!(Config::parse_with(&t, false).is_err(), "{bad}");
        }
        for bad in [
            "https://evil.com/v1/messages",
            "https://api.anthropic.com.evil.com/v1/messages",
            "https://anthropic.com/v1/messages",
            "http://api.anthropic.com/v1/messages",
            "https://api.anthropic.com:8443/v1/messages",
            "https://api.anthropic.com:/v1/messages",
            "https://127.1/v1/messages",
        ] {
            assert!(!anthropic_url_allowed(bad), "{bad}");
            let t = format!("{MIN}\n[cloud]\nanthropic_url = \"{bad}\"\n");
            assert!(Config::parse_with(&t, false).is_err(), "{bad}");
        }
        for good in [
            "https://api.anthropic.com/v1/messages",
            "https://API.anthropic.com:443/v1/messages",
            "https://api.anthropic.com",
            "http://127.0.0.1:9/v1/messages",
            "https://127.9.9.9/v1/messages",
            "http://[::1]:9/v1/messages",
            "https://localhost:9/v1/messages",
        ] {
            assert!(anthropic_url_allowed(good), "{good}");
        }
        let ok =
            format!("{MIN}\n[cloud]\nanthropic_url = \"https://api.anthropic.com/v1/messages\"\n");
        assert!(Config::parse_with(&ok, false).is_ok());
    }

    #[test]
    fn lan_key_rule_is_shared() {
        let mut c = Config::parse_with(MIN, false).unwrap();
        let s = |k: &str| Secrets {
            api_key: Some(k.into()),
            anthropic_key: None,
        };
        assert_eq!(lan_api_key(&c, &Secrets::default()).unwrap(), None);
        c.lan_bind = Some("0.0.0.0:8788".into());
        assert!(lan_api_key(&c, &Secrets::default()).is_err());
        assert!(lan_api_key(&c, &s(&"k".repeat(MIN_API_KEY_LEN - 1))).is_err());
        let k = "k".repeat(MIN_API_KEY_LEN);
        assert_eq!(
            lan_api_key(&c, &s(&k)).unwrap().as_deref(),
            Some(k.as_str())
        );
    }
}
