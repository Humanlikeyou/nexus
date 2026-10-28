use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn openclaw_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
}

/// What the UI may see about OpenClaw. Deliberately a closed, typed shape: the raw
/// openclaw.json (tokens, keys, paths) and heartbeat file never leave Rust.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OpenClawStatus {
    /// "online" (fresh heartbeat or gateway reachable), "configured" (config present,
    /// no live signal) or "offline" (no config).
    pub status: OpenClawState,
    /// `status == online`, kept for older callers.
    pub online: bool,
    pub model: String,
    /// scheme://host[:port] only; userinfo, path, query and fragment are dropped.
    pub gateway_url: String,
    /// Normalised RFC 3339 (UTC) time of the last heartbeat, if any.
    pub last_heartbeat: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OpenClawState {
    Online,
    Configured,
    Offline,
}

/// A heartbeat newer than this counts as "online".
const HEARTBEAT_FRESH_SECS: i64 = 5 * 60;
/// Tolerated clock skew for heartbeats stamped slightly in the future.
const HEARTBEAT_SKEW_SECS: i64 = 60;
const DEFAULT_GATEWAY_URL: &str = "ws://127.0.0.1:18789";
const MODEL_MAX_LEN: usize = 80;

#[derive(Serialize, Deserialize, Default)]
pub struct CronJob {
    pub name: String,
    pub schedule: String,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Skill {
    pub name: String,
    pub status: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct TaskItem {
    pub title: String,
    pub status: String,
    pub priority: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ContentPipelineItem {
    pub title: String,
    pub stage: String,
    pub content_type: String,
    pub notes: Option<String>,
}

/// Keep only scheme://host[:port] of a gateway URL; "" if it is not a ws/http URL.
fn sanitize_gateway_url(raw: &str) -> String {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return String::new();
    };
    if !matches!(url.scheme(), "ws" | "wss" | "http" | "https") {
        return String::new();
    }
    let Some(host) = url.host_str() else {
        return String::new();
    };
    match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    }
}

/// Loopback host and port of a sanitised gateway URL, for the reachability probe.
/// Remote gateways are not probed (no network calls from a status read).
fn loopback_target(url: &str) -> Option<(String, u16)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return None;
    }
    let port = parsed.port_or_known_default().or(match parsed.scheme() {
        "ws" => Some(80),
        "wss" => Some(443),
        _ => None,
    })?;
    Some((host, port))
}

/// A model id is shown as-is only if it looks like one ("anthropic/claude-x", "llama3:8b").
/// Paths are reduced to their file name; anything else (keys, URLs, objects) is "unknown".
fn sanitize_model(raw: Option<&serde_json::Value>) -> String {
    let Some(s) = raw.and_then(|v| v.as_str()).map(str::trim) else {
        return "unknown".to_string();
    };
    let s = if s.starts_with('/') || s.starts_with('~') || s.contains('\\') {
        s.rsplit(['/', '\\']).next().unwrap_or("")
    } else {
        s
    };
    let ok_chars = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/' | '@'));
    let looks_secret =
        s.starts_with("sk-") || s.contains("://") || s.contains('@') && s.contains(':');
    if s.is_empty() || s.len() > MODEL_MAX_LEN || !ok_chars || looks_secret {
        "unknown".to_string()
    } else {
        s.to_string()
    }
}

/// `last_run` as RFC 3339, or epoch seconds / milliseconds.
fn parse_heartbeat(hb: Option<&serde_json::Value>) -> Option<chrono::DateTime<chrono::Utc>> {
    let v = hb?.get("last_run")?;
    if let Some(s) = v.as_str() {
        return chrono::DateTime::parse_from_rfc3339(s.trim())
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc));
    }
    let n = v.as_f64()?;
    if !n.is_finite() || n <= 0.0 {
        return None;
    }
    let ms = if n > 1e12 { n } else { n * 1000.0 } as i64;
    chrono::DateTime::from_timestamp_millis(ms)
}

fn heartbeat_is_fresh(
    hb: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    hb.map(|t| {
        let age = (now - t).num_seconds();
        (-HEARTBEAT_SKEW_SECS..=HEARTBEAT_FRESH_SECS).contains(&age)
    })
    .unwrap_or(false)
}

fn derive_state(
    config_present: bool,
    heartbeat_fresh: bool,
    gateway_reachable: bool,
) -> OpenClawState {
    if heartbeat_fresh || gateway_reachable {
        OpenClawState::Online
    } else if config_present {
        OpenClawState::Configured
    } else {
        OpenClawState::Offline
    }
}

/// Pure part of `read_openclaw_status`: picks typed, redacted fields out of the files.
fn build_status(
    config: Option<&serde_json::Value>,
    heartbeat: Option<&serde_json::Value>,
    now: chrono::DateTime<chrono::Utc>,
    gateway_reachable: bool,
) -> OpenClawStatus {
    let raw_url = config
        .and_then(|v| v.get("gateway"))
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_GATEWAY_URL);
    let model_value = config.and_then(|v| v.get("model")).or_else(|| {
        config
            .and_then(|v| v.get("agents"))
            .and_then(|v| v.get("defaults"))
            .and_then(|v| v.get("model"))
            .and_then(|v| v.get("primary"))
    });
    let hb = parse_heartbeat(heartbeat);
    let status = derive_state(
        config.is_some(),
        heartbeat_is_fresh(hb, now),
        gateway_reachable,
    );
    OpenClawStatus {
        status,
        online: status == OpenClawState::Online,
        model: sanitize_model(model_value),
        gateway_url: sanitize_gateway_url(raw_url),
        last_heartbeat: hb.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    }
}

async fn loopback_reachable(url: &str) -> bool {
    let Some((host, port)) = loopback_target(url) else {
        return false;
    };
    tokio::time::timeout(
        Duration::from_millis(300),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    read_capped(path, MAX_READ_BYTES)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Largest OpenClaw file (config, heartbeat, cron, tasks, logs, one skill) read into memory.
const MAX_READ_BYTES: u64 = 1024 * 1024;
/// Largest skill body `save_skill` will write.
const MAX_SKILL_BYTES: usize = 256 * 1024;

/// Read a UTF-8 file, refusing anything over `max` bytes (checked before and while reading).
fn read_capped(path: &std::path::Path, max: u64) -> Result<String, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map(|m| m.len()).unwrap_or(0) > max {
        return Err(format!("file is larger than {max} bytes"));
    }
    let mut buf = String::new();
    file.take(max + 1)
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err(format!("file is larger than {max} bytes"));
    }
    Ok(buf)
}

/// A UI-supplied workspace file must be an existing `.md` inside ~/.openclaw/workspace
/// (after resolving symlinks); `None` means the default file there.
fn workspace_file(
    workspace: &std::path::Path,
    requested: Option<String>,
    default_name: &str,
) -> Result<PathBuf, String> {
    let Some(raw) = requested.filter(|s| !s.trim().is_empty()) else {
        return Ok(workspace.join(default_name));
    };
    let candidate = PathBuf::from(raw.trim());
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    let base = workspace
        .canonicalize()
        .map_err(|_| "OpenClaw workspace not found".to_string())?;
    let resolved = candidate
        .canonicalize()
        .map_err(|_| "Workspace file not found".to_string())?;
    if !resolved.starts_with(&base)
        || !resolved.is_file()
        || resolved.extension().and_then(|e| e.to_str()) != Some("md")
    {
        return Err("Only .md files inside the OpenClaw workspace can be read".to_string());
    }
    Ok(resolved)
}

/// Daily log names are exactly `YYYY-MM-DD`.
fn daily_log_date(date: Option<String>) -> Result<String, String> {
    match date {
        None => Ok(chrono::Local::now().format("%Y-%m-%d").to_string()),
        Some(d) => {
            let d = d.trim();
            let ok = d.len() == 10 && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok();
            if ok {
                Ok(d.to_string())
            } else {
                Err("Invalid date: expected YYYY-MM-DD".to_string())
            }
        }
    }
}

/// Version strings from config/CLI are shown only if they look like versions.
fn sanitize_version(raw: &str) -> Option<String> {
    let v = raw.trim();
    let ok = !v.is_empty()
        && v.len() <= 40
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | ' ' | '_'));
    ok.then(|| v.to_string())
}

#[tauri::command]
pub async fn read_openclaw_status() -> Result<OpenClawStatus, String> {
    let base = openclaw_dir();
    let config = read_json(&base.join("openclaw.json"));
    let heartbeat = read_json(&base.join("memory").join("heartbeat-state.json"));
    let now = chrono::Utc::now();

    // Only probe the (loopback) gateway when the heartbeat alone does not prove liveness.
    let mut status = build_status(config.as_ref(), heartbeat.as_ref(), now, false);
    if !status.online && config.is_some() && loopback_reachable(&status.gateway_url).await {
        status = build_status(config.as_ref(), heartbeat.as_ref(), now, true);
    }
    Ok(status)
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn gateway_url_keeps_only_scheme_host_port() {
        assert_eq!(
            sanitize_gateway_url("ws://127.0.0.1:18789"),
            "ws://127.0.0.1:18789"
        );
        assert_eq!(
            sanitize_gateway_url("wss://user:s3cret@gw.example.com:8443/rpc?token=abc#x"),
            "wss://gw.example.com:8443"
        );
        assert_eq!(
            sanitize_gateway_url("https://gw.example.com/?key=k"),
            "https://gw.example.com"
        );
        assert_eq!(
            sanitize_gateway_url("http://[::1]:18789/a"),
            "http://[::1]:18789"
        );
        for bad in [
            "",
            "not a url",
            "file:///Users/me/.openclaw/sock",
            "javascript:alert(1)",
            "/tmp/gw.sock",
        ] {
            assert_eq!(sanitize_gateway_url(bad), "", "{bad:?}");
        }
    }

    #[test]
    fn model_is_redacted_unless_it_looks_like_an_id() {
        assert_eq!(
            sanitize_model(Some(&json!("anthropic/claude-sonnet-4"))),
            "anthropic/claude-sonnet-4"
        );
        assert_eq!(sanitize_model(Some(&json!("llama3.1:8b"))), "llama3.1:8b");
        assert_eq!(
            sanitize_model(Some(&json!("/Users/me/models/qwen-7b.gguf"))),
            "qwen-7b.gguf"
        );
        assert_eq!(sanitize_model(Some(&json!("~/m/x.gguf"))), "x.gguf");
        for bad in [
            json!("sk-ant-abc123"),
            json!("https://u:p@host/m"),
            json!("has space"),
            json!({"primary": "x"}),
            json!("a".repeat(81)),
        ] {
            assert_eq!(sanitize_model(Some(&bad)), "unknown", "{bad}");
        }
        assert_eq!(sanitize_model(None), "unknown");
    }

    #[test]
    fn heartbeat_parses_rfc3339_and_epochs() {
        let t = at("2026-10-28T05:00:00Z");
        assert_eq!(
            parse_heartbeat(Some(&json!({"last_run": "2026-10-28T14:00:00+09:00"}))),
            Some(t)
        );
        assert_eq!(
            parse_heartbeat(Some(&json!({"last_run": t.timestamp()}))),
            Some(t)
        );
        assert_eq!(
            parse_heartbeat(Some(&json!({"last_run": t.timestamp_millis()}))),
            Some(t)
        );
        for bad in [
            json!({}),
            json!({"last_run": "yesterday"}),
            json!({"last_run": -5}),
            json!({"last_run": null}),
        ] {
            assert_eq!(parse_heartbeat(Some(&bad)), None, "{bad}");
        }
        assert_eq!(parse_heartbeat(None), None);
    }

    #[test]
    fn freshness_window_is_five_minutes_with_small_skew() {
        let now = at("2026-10-28T05:10:00Z");
        assert!(heartbeat_is_fresh(Some(at("2026-10-28T05:05:00Z")), now));
        assert!(heartbeat_is_fresh(Some(at("2026-10-28T05:10:30Z")), now));
        assert!(!heartbeat_is_fresh(Some(at("2026-10-28T05:04:59Z")), now));
        assert!(!heartbeat_is_fresh(Some(at("2026-10-28T05:12:00Z")), now));
        assert!(!heartbeat_is_fresh(None, now));
    }

    #[test]
    fn state_needs_a_live_signal_to_be_online() {
        assert_eq!(derive_state(true, true, false), OpenClawState::Online);
        assert_eq!(derive_state(true, false, true), OpenClawState::Online);
        assert_eq!(derive_state(true, false, false), OpenClawState::Configured);
        assert_eq!(derive_state(false, false, false), OpenClawState::Offline);
    }

    #[test]
    fn loopback_only_probe_targets() {
        assert_eq!(
            loopback_target("ws://127.0.0.1:18789"),
            Some(("127.0.0.1".into(), 18789))
        );
        assert_eq!(
            loopback_target("ws://localhost"),
            Some(("localhost".into(), 80))
        );
        assert_eq!(loopback_target("http://[::1]:9"), Some(("::1".into(), 9)));
        assert_eq!(loopback_target("wss://gw.example.com:8443"), None);
        assert_eq!(loopback_target(""), None);
    }

    #[test]
    fn build_status_serialises_only_typed_redacted_fields() {
        let config = json!({
            "model": "anthropic/claude-sonnet-4",
            "gateway": {"url": "ws://admin:hunter2@127.0.0.1:18789/ws?token=tok_live_123", "auth": {"token": "tok_live_123"}},
            "env": {"ANTHROPIC_API_KEY": "sk-ant-SECRET"},
            "workspace": "/Users/me/.openclaw/workspace"
        });
        let hb = json!({"last_run": "2026-10-28T05:08:00Z", "notes": "sk-ant-SECRET in /Users/me"});
        let status = build_status(Some(&config), Some(&hb), at("2026-10-28T05:10:00Z"), false);
        assert_eq!(status.status, OpenClawState::Online);
        assert!(status.online);
        assert_eq!(status.gateway_url, "ws://127.0.0.1:18789");
        assert_eq!(
            status.last_heartbeat.as_deref(),
            Some("2026-10-28T05:08:00Z")
        );

        let v = serde_json::to_value(&status).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["gateway_url", "last_heartbeat", "model", "online", "status"]
        );
        assert_eq!(v["status"], "online");
        let text = v.to_string();
        for leak in [
            "hunter2", "admin", "tok_live", "sk-ant", "/Users", "token", "notes",
        ] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
    }

    #[test]
    fn build_status_without_live_signal() {
        let now = at("2026-10-28T05:10:00Z");
        let stale = json!({"last_run": "2026-10-28T04:00:00Z"});
        let s = build_status(Some(&json!({})), Some(&stale), now, false);
        assert_eq!((s.status, s.online), (OpenClawState::Configured, false));
        assert_eq!(s.gateway_url, DEFAULT_GATEWAY_URL);
        assert_eq!(s.model, "unknown");
        assert_eq!(s.last_heartbeat.as_deref(), Some("2026-10-28T04:00:00Z"));
        let s = build_status(Some(&json!({})), None, now, true);
        assert_eq!(s.status, OpenClawState::Online);
        let s = build_status(None, None, now, false);
        assert_eq!(
            (s.status, s.online, s.last_heartbeat),
            (OpenClawState::Offline, false, None)
        );
    }
}

#[tauri::command]
pub fn read_cron_jobs() -> Result<Vec<CronJob>, String> {
    let path = openclaw_dir().join("cron").join("jobs.json");
    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_else(|_| "[]".to_string());
    let jobs: Vec<CronJob> = serde_json::from_str(&content).unwrap_or_default();
    Ok(jobs)
}

#[tauri::command]
pub fn read_skills() -> Result<Vec<Skill>, String> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Helper: load .md skills from a directory
    let load_from = |dir: &std::path::Path,
                     skills: &mut Vec<Skill>,
                     seen: &mut std::collections::HashSet<String>| {
        if !dir.exists() {
            return;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !seen.insert(name.clone()) {
                    continue;
                }
                // Oversized or unreadable skill files are skipped, not loaded.
                let Ok(content) = read_capped(&path, MAX_READ_BYTES) else {
                    continue;
                };
                // Extract first line after "# " as description
                let description = content
                    .lines()
                    .find(|l| l.starts_with("# "))
                    .map(|l| l.trim_start_matches("# ").to_string())
                    .unwrap_or_default();
                skills.push(Skill {
                    name,
                    status: "active".to_string(),
                    description,
                    content,
                });
            }
        }
    };

    // 1. Project skills: config/skills/ (walk up from binary to find project root)
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            let candidate = parent.join("config").join("skills");
            if candidate.exists() {
                load_from(&candidate, &mut skills, &mut seen);
                break;
            }
            dir = parent;
        }
    }
    // Fallback: well-known project path
    if let Some(home) = dirs::home_dir() {
        let fallback = home.join("projects/nexus/config/skills");
        if fallback.exists() {
            load_from(&fallback, &mut skills, &mut seen);
        }
    }

    // 2. OpenClaw skills: ~/.openclaw/skills/
    load_from(&openclaw_dir().join("skills"), &mut skills, &mut seen);

    Ok(skills)
}

/// Write or update a skill file in ~/.openclaw/skills/.
/// Used by the skills self-improvement loop to create/update learned skills.
#[tauri::command]
pub fn save_skill(name: String, content: String) -> Result<(), String> {
    let dir = openclaw_dir().join("skills");
    if content.len() > MAX_SKILL_BYTES {
        return Err(format!("Skill is larger than {MAX_SKILL_BYTES} bytes"));
    }
    let path = skill_path(&dir, &name)?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

/// Map a user-facing skill name ("Daily Brief") to `<dir>/daily-brief.md`,
/// rejecting anything that could escape the skills directory.
fn skill_path(dir: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    let stem = name.trim().replace(' ', "-").to_lowercase();
    super::path_safety::validate_name(&stem)?;
    super::path_safety::safe_join(dir, &format!("{stem}.md"))
}

/// Delete a skill file from ~/.openclaw/skills/.
#[tauri::command]
pub fn delete_skill(name: String) -> Result<bool, String> {
    let dir = openclaw_dir().join("skills");
    let path = skill_path(&dir, &name)?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub fn read_tasks(workspace_path: Option<String>) -> Result<Vec<TaskItem>, String> {
    let path = workspace_file(
        &openclaw_dir().join("workspace"),
        workspace_path,
        "TASKS.md",
    )?;
    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_default();
    let tasks = parse_tasks_md(&content);
    Ok(tasks)
}

fn parse_tasks_md(content: &str) -> Vec<TaskItem> {
    let mut tasks = Vec::new();
    let mut current_status = "backlog".to_string();

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect section headers
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let header = trimmed.trim_start_matches('#').trim().to_lowercase();
            if header.contains("active")
                || header.contains("sprint")
                || header.contains("in progress")
            {
                current_status = "active".to_string();
            } else if header.contains("backlog") || header.contains("todo") {
                current_status = "backlog".to_string();
            } else if header.contains("waiting") || header.contains("blocked") {
                current_status = "waiting".to_string();
            } else if header.contains("done") || header.contains("completed") {
                current_status = "done".to_string();
            }
        }

        // Parse task items (- [ ] or - [x] format)
        if trimmed.starts_with("- [") {
            let checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
            let title = trimmed
                .trim_start_matches("- [x] ")
                .trim_start_matches("- [X] ")
                .trim_start_matches("- [ ] ")
                .to_string();

            let status = if checked {
                "done".to_string()
            } else {
                current_status.clone()
            };

            // Extract tags (words starting with #)
            let tags: Vec<String> = title
                .split_whitespace()
                .filter(|w| w.starts_with('#'))
                .map(|w| w.trim_start_matches('#').to_string())
                .collect();

            tasks.push(TaskItem {
                title: title.split('#').next().unwrap_or(&title).trim().to_string(),
                status,
                priority: None,
                tags,
            });
        }
    }

    if tasks.is_empty() {
        // Placeholder tasks
        tasks = vec![
            TaskItem {
                title: "Launch TaiyoSkin landing page".to_string(),
                status: "active".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["launch".to_string()],
            },
            TaskItem {
                title: "Design product photography shoot".to_string(),
                status: "active".to_string(),
                priority: Some("medium".to_string()),
                tags: vec!["content".to_string()],
            },
            TaskItem {
                title: "Set up Stripe payments".to_string(),
                status: "backlog".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["payments".to_string()],
            },
            TaskItem {
                title: "Create Instagram content calendar".to_string(),
                status: "backlog".to_string(),
                priority: Some("medium".to_string()),
                tags: vec!["content".to_string(), "social".to_string()],
            },
            TaskItem {
                title: "Research shipping providers".to_string(),
                status: "waiting".to_string(),
                priority: Some("low".to_string()),
                tags: vec!["logistics".to_string()],
            },
            TaskItem {
                title: "Brand identity guidelines".to_string(),
                status: "done".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["design".to_string()],
            },
        ];
    }

    tasks
}

#[tauri::command]
pub fn read_content_pipeline(
    workspace_path: Option<String>,
) -> Result<Vec<ContentPipelineItem>, String> {
    let path = workspace_file(
        &openclaw_dir().join("workspace"),
        workspace_path,
        "CONTENT-PIPELINE.md",
    )?;
    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_default();
    let items = parse_content_pipeline(&content);
    Ok(items)
}

fn parse_content_pipeline(content: &str) -> Vec<ContentPipelineItem> {
    let mut items = Vec::new();
    let mut current_stage = "ideation".to_string();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let header = trimmed.trim_start_matches('#').trim().to_lowercase();
            if header.contains("idea") {
                current_stage = "ideation".to_string();
            } else if header.contains("draft") || header.contains("writing") {
                current_stage = "drafting".to_string();
            } else if header.contains("review") || header.contains("edit") {
                current_stage = "review".to_string();
            } else if header.contains("ready") || header.contains("scheduled") {
                current_stage = "scheduled".to_string();
            } else if header.contains("published") || header.contains("live") {
                current_stage = "published".to_string();
            }
        }

        if trimmed.starts_with("- ") && !trimmed.starts_with("- [") {
            let title = trimmed.trim_start_matches("- ").to_string();
            items.push(ContentPipelineItem {
                title,
                stage: current_stage.clone(),
                content_type: "post".to_string(),
                notes: None,
            });
        }
    }

    if items.is_empty() {
        items = vec![
            ContentPipelineItem {
                title: "Skincare routine for melanin-rich skin".to_string(),
                stage: "ideation".to_string(),
                content_type: "blog".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                title: "Morning glow tutorial — TikTok".to_string(),
                stage: "drafting".to_string(),
                content_type: "video".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                title: "Ingredient spotlight: Niacinamide".to_string(),
                stage: "review".to_string(),
                content_type: "carousel".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                title: "Product launch announcement".to_string(),
                stage: "scheduled".to_string(),
                content_type: "post".to_string(),
                notes: Some("Scheduled for next Monday".to_string()),
            },
            ContentPipelineItem {
                title: "Brand story — our 'why'".to_string(),
                stage: "published".to_string(),
                content_type: "blog".to_string(),
                notes: None,
            },
        ];
    }

    items
}

#[tauri::command]
pub fn read_daily_log(date: Option<String>) -> Result<String, String> {
    let date_str = daily_log_date(date)?;

    let path = openclaw_dir()
        .join("memory")
        .join(format!("{}.md", date_str));

    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_else(|_| {
        format!(
            "# Daily Log — {}\n\nNo log entry found for this date. Logs are created automatically by Terrance during daily operations.\n\n## Notes\n- Start your day with intention\n- Review yesterday's accomplishments\n- Set today's priorities",
            date_str
        )
    });

    Ok(content)
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenClaw Process Control
// ─────────────────────────────────────────────────────────────────────────────

/// Restart the OpenClaw LaunchAgent via `launchctl kickstart -k`.
/// Works on macOS; returns an error string on failure.
#[tauri::command]
pub async fn restart_openclaw(app: tauri::AppHandle) -> Result<String, String> {
    let _approval = super::approval_gate::require_approval(
        &app,
        &[super::approval_gate::service_key("openclaw")],
        super::approval_gate::approval_text(
            OPENCLAW_RESTART_OPERATION,
            "Restarts the OpenClaw gateway LaunchAgent. Connected channels reconnect.",
        )?,
    )
    .await?;
    kickstart_openclaw().await
}

/// Exact operation shown for an OpenClaw restart (uid is resolved at run time).
pub(super) const OPENCLAW_RESTART_OPERATION: &str =
    "Run: launchctl kickstart -k gui/<your uid>/ai.openclaw.gateway";

/// The ungated restart. Only call after approval has been obtained.
pub(super) async fn kickstart_openclaw() -> Result<String, String> {
    // Resolve the current user UID so we can address the gui/<uid>/ domain.
    let uid_out = tokio::process::Command::new("id")
        .arg("-u")
        .output()
        .await
        .map_err(|e| format!("Failed to get UID: {}", e))?;
    if !uid_out.status.success() {
        return Err(
            "Could not determine the current user id; refusing to restart OpenClaw.".to_string(),
        );
    }
    let uid = parse_uid(&String::from_utf8_lossy(&uid_out.stdout))?;

    let output = tokio::process::Command::new("launchctl")
        .args([
            "kickstart",
            "-k",
            &format!("gui/{}/ai.openclaw.gateway", uid),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run launchctl: {}", e))?;

    if output.status.success() {
        Ok("OpenClaw gateway restarting…".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "launchctl kickstart failed — is the LaunchAgent installed?".to_string()
        } else {
            stderr
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenClaw Sandbox Toggle
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    pub mode: String,
    pub restarted: bool,
}

/// Read the current sandbox mode from ~/.openclaw/openclaw.json.
#[tauri::command]
pub fn get_openclaw_sandbox() -> Result<String, String> {
    let config_path = openclaw_dir().join("openclaw.json");
    let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    let mode = config
        .get("agents")
        .and_then(|v| v.get("defaults"))
        .and_then(|v| v.get("sandbox"))
        .and_then(|v| v.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("off");

    Ok(mode.to_string())
}

fn read_sandbox_config(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn sandbox_mode_of(config: &serde_json::Value) -> String {
    config
        .get("agents")
        .and_then(|v| v.get("defaults"))
        .and_then(|v| v.get("sandbox"))
        .and_then(|v| v.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("off")
        .to_string()
}

/// Re-read the config, require the mode to still be `expected_old` (what the
/// approval dialog showed), set `new_mode`, and write atomically.
fn apply_sandbox_change(
    config_path: &std::path::Path,
    expected_old: &str,
    new_mode: &str,
) -> Result<(), String> {
    let mut config = read_sandbox_config(config_path)?;
    let now = sandbox_mode_of(&config);
    if now != expected_old {
        return Err(format!(
            "changed: sandbox mode is now \"{now}\", not the \"{expected_old}\" shown for approval; nothing was written"
        ));
    }
    let agents = config
        .as_object_mut()
        .ok_or("invalid config")?
        .entry("agents")
        .or_insert_with(|| serde_json::json!({}));
    let defaults = agents
        .as_object_mut()
        .ok_or("invalid agents")?
        .entry("defaults")
        .or_insert_with(|| serde_json::json!({}));
    let sandbox = defaults
        .as_object_mut()
        .ok_or("invalid defaults")?
        .entry("sandbox")
        .or_insert_with(|| serde_json::json!({}));
    sandbox.as_object_mut().ok_or("invalid sandbox")?.insert(
        "mode".to_string(),
        serde_json::Value::String(new_mode.to_string()),
    );
    let out = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    super::continuity::atomic_write(config_path, out.as_bytes()).map_err(|e| e.to_string())
}

/// `id -u` output must be a plain non-empty decimal uid.
fn parse_uid(raw: &str) -> Result<u32, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Err(
            "Could not determine the current user id; refusing to restart OpenClaw.".to_string(),
        );
    }
    trimmed.parse::<u32>().map_err(|_| {
        "Could not determine the current user id; refusing to restart OpenClaw.".to_string()
    })
}

const SANDBOX_MODES: [&str; 3] = ["off", "non-main", "all"];

/// Explicit mode must be a known OpenClaw sandbox mode; `None` toggles off <-> all.
fn resolve_sandbox_mode(current: &str, requested: Option<String>) -> Result<String, String> {
    match requested {
        Some(m) if SANDBOX_MODES.contains(&m.as_str()) => Ok(m),
        Some(_) => Err(format!(
            "Invalid sandbox mode. Allowed: {}",
            SANDBOX_MODES.join(", ")
        )),
        None if current == "off" => Ok("all".to_string()),
        None => Ok("off".to_string()),
    }
}

fn sandbox_operation(config_path: &std::path::Path, current: &str, new_mode: &str) -> String {
    format!(
        "Set agents.defaults.sandbox.mode: \"{current}\" -> \"{new_mode}\"\nin {}\nthen run: launchctl kickstart -k gui/<your uid>/ai.openclaw.gateway",
        config_path.display()
    )
}

/// Toggle sandbox mode between "off" and "all", then restart OpenClaw
/// (requires native approval).
#[tauri::command]
pub async fn toggle_openclaw_sandbox(
    app: tauri::AppHandle,
    mode: Option<String>,
) -> Result<SandboxStatus, String> {
    let config_path = openclaw_dir().join("openclaw.json");
    let current = read_sandbox_config(&config_path).map(|c| sandbox_mode_of(&c))?;

    let new_mode = resolve_sandbox_mode(&current, mode)?;
    let _approval = super::approval_gate::require_approval(
        &app,
        // Same key as restarts: this command restarts OpenClaw too.
        &[super::approval_gate::service_key("openclaw")],
        super::approval_gate::approval_text(
            &sandbox_operation(&config_path, &current, &new_mode),
            "Writes this setting to the OpenClaw config, then restarts the OpenClaw gateway.",
        )?,
    )
    .await?;

    // Re-read after approval: refuse if the mode changed under the dialog,
    // then write atomically.
    apply_sandbox_change(&config_path, &current, &new_mode)?;

    // Restart OpenClaw so it picks up the change (covered by the same approval).
    let restarted = kickstart_openclaw().await.is_ok();

    Ok(SandboxStatus {
        mode: new_mode,
        restarted,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenClaw Health Check
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct OpenClawChannels {
    pub telegram: String, // "connected" | "disconnected" | "unknown"
    pub whatsapp: String, // "connected" | "not_linked" | "unknown"
}

#[derive(Serialize, Deserialize, Clone)]
pub struct OpenClawHealth {
    pub status: String, // "healthy" | "degraded" | "offline"
    pub version: Option<String>,
    pub gateway: bool,
    pub rpc: bool,
    pub memory_search: String, // "configured" | "missing"
    pub channels: OpenClawChannels,
    pub timestamp: String,
}

async fn get_openclaw_version() -> Option<String> {
    // Prefer reading from config metadata (always available, no PATH issues)
    let config_path = openclaw_dir().join("openclaw.json");
    if let Some(ver) = read_json(&config_path).as_ref().and_then(|v| {
        v.get("meta")
            .and_then(|m| m.get("lastTouchedVersion"))
            .and_then(|v| v.as_str())
            .and_then(sanitize_version)
    }) {
        return Some(ver);
    }
    // Fallback to CLI (fixed binary and args; bounded so a hung CLI cannot stall health).
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new("/opt/homebrew/bin/openclaw")
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    sanitize_version(&String::from_utf8_lossy(&out.stdout))
}

async fn check_gateway_port() -> bool {
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect("127.0.0.1:18789"),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

async fn probe_gateway_rpc() -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    client
        .get("http://127.0.0.1:18789/")
        .send()
        .await
        .map(|r| r.status().as_u16() < 500)
        .unwrap_or(false)
}

fn check_memory_search(config: &Option<serde_json::Value>) -> String {
    let provider = config
        .as_ref()
        .and_then(|v| v.get("agents"))
        .and_then(|v| v.get("defaults"))
        .and_then(|v| v.get("memorySearch"))
        .and_then(|v| v.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("none");

    match provider {
        "local" => {
            // First check explicit modelPath/localModel config keys
            let model_path = config
                .as_ref()
                .and_then(|v| v.get("agents"))
                .and_then(|v| v.get("defaults"))
                .and_then(|v| v.get("memorySearch"))
                .and_then(|v| v.get("modelPath").or_else(|| v.get("localModel")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !model_path.is_empty() && PathBuf::from(model_path).exists() {
                return "configured".to_string();
            }
            // Fallback: OpenClaw downloads models to ~/.node-llama-cpp/models/ and
            // references them via HF specifier (not a config path). Check if any
            // .gguf file exists there — presence means the model was auto-downloaded.
            let gguf_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".node-llama-cpp")
                .join("models");
            let has_model = gguf_dir
                .read_dir()
                .map(|rd| {
                    rd.flatten().any(|e| {
                        e.path()
                            .extension()
                            .and_then(|x| x.to_str())
                            .map(|x| x.eq_ignore_ascii_case("gguf"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if has_model {
                "configured".to_string()
            } else {
                "missing".to_string()
            }
        }
        "openai" | "anthropic" | "ollama" | "cohere" | "azure" | "gemini" | "minimax" => {
            "configured".to_string()
        }
        _ => "missing".to_string(),
    }
}

fn get_channel_status(config: &Option<serde_json::Value>) -> OpenClawChannels {
    let creds_dir = openclaw_dir().join("credentials");

    let telegram_enabled = config
        .as_ref()
        .and_then(|v| v.get("channels"))
        .and_then(|v| v.get("telegram"))
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let telegram = if telegram_enabled {
        let has_creds = creds_dir.join("telegram-default-allowFrom.json").exists()
            || creds_dir.join("telegram-allowFrom.json").exists();
        if has_creds {
            "connected".to_string()
        } else {
            "disconnected".to_string()
        }
    } else {
        "unknown".to_string()
    };

    // WhatsApp: check for session file in ~/.openclaw/sessions/
    let sessions_dir = openclaw_dir().join("sessions");
    let whatsapp_linked = sessions_dir
        .read_dir()
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("whatsapp")
            })
        })
        .unwrap_or(false);

    let whatsapp = if whatsapp_linked {
        "connected".to_string()
    } else {
        "not_linked".to_string()
    };

    OpenClawChannels { telegram, whatsapp }
}

#[tauri::command]
pub async fn get_openclaw_health() -> Result<OpenClawHealth, String> {
    let base = openclaw_dir();

    // Read main config
    let config = read_json(&base.join("openclaw.json"));

    // Port + RPC checks run concurrently to minimise latency
    let (gateway, rpc) = tokio::join!(check_gateway_port(), probe_gateway_rpc());

    let version = get_openclaw_version().await;
    let memory_search = check_memory_search(&config);
    let channels = get_channel_status(&config);

    let status = if !gateway || !rpc {
        "offline"
    } else if memory_search == "missing" || channels.telegram == "disconnected" {
        "degraded"
    } else {
        "healthy"
    };

    Ok(OpenClawHealth {
        status: status.to_string(),
        version,
        gateway,
        rpc,
        memory_search,
        channels,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod input_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nexus-oc-{tag}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn read_capped_refuses_oversized_files() {
        let d = tmp("cap");
        let small = d.join("a.md");
        fs::write(&small, "hello").unwrap();
        assert_eq!(read_capped(&small, 5).unwrap(), "hello");
        assert!(read_capped(&small, 4).is_err());
        assert!(read_capped(&d.join("missing.md"), 10).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn workspace_file_stays_inside_the_workspace() {
        let root = tmp("ws");
        let ws = root.join("workspace");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("TASKS.md"), "- [ ] a").unwrap();
        fs::write(ws.join("notes.txt"), "x").unwrap();
        fs::write(root.join("secret.md"), "x").unwrap();

        assert_eq!(
            workspace_file(&ws, None, "TASKS.md").unwrap(),
            ws.join("TASKS.md")
        );
        assert_eq!(
            workspace_file(&ws, Some("  ".into()), "TASKS.md").unwrap(),
            ws.join("TASKS.md")
        );
        assert_eq!(
            workspace_file(&ws, Some("TASKS.md".into()), "x").unwrap(),
            ws.join("TASKS.md")
        );
        let abs = ws.join("TASKS.md").to_string_lossy().to_string();
        assert!(workspace_file(&ws, Some(abs), "x").is_ok());
        for bad in [
            "../secret.md".to_string(),
            root.join("secret.md").to_string_lossy().to_string(),
            "/etc/hosts".to_string(),
            "notes.txt".to_string(),
            "missing.md".to_string(),
        ] {
            assert!(
                workspace_file(&ws, Some(bad.clone()), "x").is_err(),
                "{bad}"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("secret.md"), ws.join("link.md")).unwrap();
            assert!(
                workspace_file(&ws, Some("link.md".into()), "x").is_err(),
                "symlink escape"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn daily_log_date_must_be_iso() {
        assert_eq!(
            daily_log_date(Some("2026-10-28".into())).unwrap(),
            "2026-10-28"
        );
        assert_eq!(daily_log_date(None).unwrap().len(), 10);
        for bad in [
            "../../.ssh/id_rsa",
            "2026-10-28/../../x",
            "2026-13-01",
            "2026-1-2",
            "",
            "2026-10-28.md",
        ] {
            assert!(daily_log_date(Some(bad.into())).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn versions_are_sanitised() {
        assert_eq!(
            sanitize_version("2026.10.1\n").as_deref(),
            Some("2026.10.1")
        );
        assert_eq!(
            sanitize_version("openclaw 1.2.3-beta+4").as_deref(),
            Some("openclaw 1.2.3-beta+4")
        );
        assert_eq!(sanitize_version("token=abc; rm -rf /"), None);
        assert_eq!(sanitize_version(&"9".repeat(41)), None);
        assert_eq!(sanitize_version(""), None);
    }

    #[test]
    fn save_skill_refuses_oversized_content_before_touching_disk() {
        let err = save_skill("too-big".into(), "x".repeat(MAX_SKILL_BYTES + 1)).unwrap_err();
        assert!(err.contains("larger"), "{err}");
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;

    #[test]
    fn sandbox_mode_is_validated_and_toggles() {
        assert_eq!(resolve_sandbox_mode("off", None).unwrap(), "all");
        assert_eq!(resolve_sandbox_mode("all", None).unwrap(), "off");
        assert_eq!(
            resolve_sandbox_mode("off", Some("non-main".into())).unwrap(),
            "non-main"
        );
        assert!(resolve_sandbox_mode("off", Some("everything; rm -rf".into())).is_err());
    }

    fn tmp_config(body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nexus-sbx-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("openclaw.json");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn sandbox_change_applies_when_mode_unchanged() {
        let p = tmp_config(r#"{"agents":{"defaults":{"sandbox":{"mode":"off"}}},"keep":1}"#);
        apply_sandbox_change(&p, "off", "all").unwrap();
        let v = read_sandbox_config(&p).unwrap();
        assert_eq!(sandbox_mode_of(&v), "all");
        assert_eq!(v["keep"], 1);
        let leftovers = std::fs::read_dir(p.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temp files left behind");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn sandbox_change_refused_if_mode_changed_after_dialog() {
        let body = r#"{"agents":{"defaults":{"sandbox":{"mode":"non-main"}}}}"#;
        let p = tmp_config(body);
        let err = apply_sandbox_change(&p, "off", "all").unwrap_err();
        assert!(err.starts_with("changed:"), "{err}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), body, "file untouched");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn uid_must_be_numeric_and_non_empty() {
        assert_eq!(parse_uid("501\n").unwrap(), 501);
        for bad in ["", "  \n", "abc", "-1", "501 502", "5O1"] {
            assert!(parse_uid(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sandbox_operation_names_exact_change() {
        let op = sandbox_operation(
            std::path::Path::new("/h/.openclaw/openclaw.json"),
            "off",
            "all",
        );
        assert!(op.starts_with("Set agents.defaults.sandbox.mode: \"off\" -> \"all\""));
        assert!(op.contains("/h/.openclaw/openclaw.json"));
        assert!(op.contains("launchctl kickstart -k"));
    }
}
