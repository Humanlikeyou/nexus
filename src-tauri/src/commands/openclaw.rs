use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn openclaw_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
}

#[derive(Serialize, Deserialize, Default)]
pub struct OpenClawStatus {
    pub online: bool,
    pub model: String,
    pub gateway_url: String,
    pub last_heartbeat: Option<String>,
    pub heartbeat_data: Option<serde_json::Value>,
    pub config: Option<serde_json::Value>,
}

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

#[tauri::command]
pub fn read_openclaw_status() -> Result<OpenClawStatus, String> {
    let base = openclaw_dir();

    // Read main config
    let config_path = base.join("openclaw.json");
    let config: Option<serde_json::Value> = fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    // Read heartbeat state
    let hb_path = base.join("memory").join("heartbeat-state.json");
    let heartbeat_data: Option<serde_json::Value> = fs::read_to_string(&hb_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let last_heartbeat = heartbeat_data
        .as_ref()
        .and_then(|v| v.get("last_run"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let gateway_url = config
        .as_ref()
        .and_then(|v| v.get("gateway"))
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("ws://127.0.0.1:18789")
        .to_string();

    let model = config
        .as_ref()
        .and_then(|v| v.get("model"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let online = config.is_some();

    Ok(OpenClawStatus {
        online,
        model,
        gateway_url,
        last_heartbeat,
        heartbeat_data,
        config,
    })
}

#[tauri::command]
pub fn read_cron_jobs() -> Result<Vec<CronJob>, String> {
    let path = openclaw_dir().join("cron").join("jobs.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
    let jobs: Vec<CronJob> = serde_json::from_str(&content).unwrap_or_default();
    Ok(jobs)
}

#[tauri::command]
pub fn read_skills() -> Result<Vec<Skill>, String> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Helper: load .md skills from a directory
    let load_from = |dir: &std::path::Path, skills: &mut Vec<Skill>, seen: &mut std::collections::HashSet<String>| {
        if !dir.exists() { return; }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                let name = path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !seen.insert(name.clone()) { continue; }
                let content = fs::read_to_string(&path).unwrap_or_default();
                // Extract first line after "# " as description
                let description = content.lines()
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

#[tauri::command]
pub fn read_tasks(workspace_path: Option<String>) -> Result<Vec<TaskItem>, String> {
    let path = workspace_path
        .map(PathBuf::from)
        .unwrap_or_else(|| openclaw_dir().join("workspace").join("TASKS.md"));

    let content = fs::read_to_string(&path).unwrap_or_default();
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
            if header.contains("active") || header.contains("sprint") || header.contains("in progress") {
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
pub fn read_content_pipeline(workspace_path: Option<String>) -> Result<Vec<ContentPipelineItem>, String> {
    let path = workspace_path
        .map(PathBuf::from)
        .unwrap_or_else(|| openclaw_dir().join("workspace").join("CONTENT-PIPELINE.md"));

    let content = fs::read_to_string(&path).unwrap_or_default();
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
    let date_str = date.unwrap_or_else(|| {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    });

    let path = openclaw_dir()
        .join("memory")
        .join(format!("{}.md", date_str));

    let content = fs::read_to_string(&path).unwrap_or_else(|_| {
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
pub async fn restart_openclaw() -> Result<String, String> {
    // Resolve the current user UID so we can address the gui/<uid>/ domain.
    let uid_out = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| format!("Failed to get UID: {}", e))?;
    let uid = String::from_utf8_lossy(&uid_out.stdout).trim().to_string();

    let output = std::process::Command::new("launchctl")
        .args([
            "kickstart",
            "-k",
            &format!("gui/{}/ai.openclaw.gateway", uid),
        ])
        .output()
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
// OpenClaw Health Check
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct OpenClawChannels {
    pub telegram: String, // "connected" | "disconnected" | "unknown"
    pub whatsapp: String, // "connected" | "not_linked" | "unknown"
}

#[derive(Serialize, Deserialize, Clone)]
pub struct OpenClawHealth {
    pub status: String,          // "healthy" | "degraded" | "offline"
    pub version: Option<String>,
    pub gateway: bool,
    pub rpc: bool,
    pub memory_search: String,   // "configured" | "missing"
    pub channels: OpenClawChannels,
    pub timestamp: String,
}

fn get_openclaw_version() -> Option<String> {
    std::process::Command::new("openclaw")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
        "openai" | "anthropic" | "ollama" | "cohere" | "azure" => "configured".to_string(),
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
    let config: Option<serde_json::Value> = fs::read_to_string(base.join("openclaw.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    // Port + RPC checks run concurrently to minimise latency
    let (gateway, rpc) = tokio::join!(check_gateway_port(), probe_gateway_rpc());

    let version = get_openclaw_version();
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
