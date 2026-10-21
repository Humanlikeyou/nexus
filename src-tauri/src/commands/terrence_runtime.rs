use super::{
    continuity, noop, terrence_actions, terrence_automations, terrence_context,
    terrence_priorities, terrence_projects, terrence_runtime_bridge,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tauri::State;
use uuid::Uuid;

const RUNTIME_NAME: &str = "hermes";
const NEXUS_RUNTIME_PROFILE: &str = "nexus-managed-actions-v5";
const MAX_MESSAGE_CHARS: usize = 20_000;
const NEXUS_ACTION_OPEN: &str = "<nexus_action>";
const NEXUS_ACTION_CLOSE: &str = "</nexus_action>";
const MAX_NEXUS_ACTION_PROTOCOL_CHARS: usize = 4_000;
const MAX_NEXUS_CONTEXT_PROTOCOL_CHARS: usize = 16_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceRuntimeHealth {
    pub runtime: String,
    pub status: String,
    pub model: String,
    pub provider: String,
    pub gateway_running: bool,
    pub telegram_status: String,
    pub active_schedules: u64,
    pub configured_toolsets: Vec<String>,
    pub checked_at: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceCapability {
    pub name: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceInteractionRequest {
    pub message: String,
    pub conversation_id: String,
    pub hermes_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrenceInteraction {
    pub id: String,
    pub conversation_id: String,
    pub hermes_session_id: Option<String>,
    pub runtime: String,
    pub model: String,
    pub provider: String,
    pub status: String,
    pub user_message: String,
    pub assistant_message: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub context_item_ids: Vec<String>,
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default)]
    pub runtime_profile: String,
}

#[derive(Debug, Deserialize)]
struct NexusActionProposal {
    #[serde(alias = "toolId")]
    tool_id: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NexusDecisionProposal {
    id: Option<String>,
    question: String,
    constraints: Option<Vec<String>>,
    evidence: Option<Vec<terrence_priorities::DecisionEvidence>>,
    alternatives: Option<Vec<terrence_priorities::DecisionAlternative>>,
    recommendation: String,
    uncertainty: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NexusDecisionResolutionProposal {
    id: String,
    status: String,
    decision: Option<String>,
    outcome: Option<String>,
}

type NexusProjectProposal = terrence_projects::UpsertTerrenceProjectInput;
type NexusContextProposal = terrence_context::HermesContextEvent;

struct NexusActionProposalFile {
    path: PathBuf,
}

impl NexusActionProposalFile {
    fn create() -> Result<Self, String> {
        let directory = app_data_dir().join("nexus_action_proposals");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Failed to prepare Nexus action proposal store: {error}"))?;
        Ok(Self {
            path: directory.join(format!("{}.json", Uuid::new_v4())),
        })
    }

    fn take(&self) -> Result<Option<NexusActionProposal>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("Failed to read Nexus action proposal: {error}"));
            }
        };
        let _ = fs::remove_file(&self.path);
        if contents.chars().count() > MAX_NEXUS_ACTION_PROTOCOL_CHARS {
            return Err("Nexus action proposal exceeded the allowed size.".to_string());
        }
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| "Nexus action proposal was invalid.".to_string())
    }
}

impl Drop for NexusActionProposalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct NexusDecisionProposalFile {
    path: PathBuf,
}

impl NexusDecisionProposalFile {
    fn create() -> Result<Self, String> {
        let directory = app_data_dir().join("nexus_decision_proposals");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Failed to prepare Nexus decision proposal store: {error}"))?;
        Ok(Self {
            path: directory.join(format!("{}.json", Uuid::new_v4())),
        })
    }

    fn take(&self) -> Result<Option<NexusDecisionProposal>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("Failed to read Nexus decision proposal: {error}"));
            }
        };
        let _ = fs::remove_file(&self.path);
        if contents.chars().count() > MAX_NEXUS_ACTION_PROTOCOL_CHARS {
            return Err("Nexus decision proposal exceeded the allowed size.".to_string());
        }
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| "Nexus decision proposal was invalid.".to_string())
    }
}

impl Drop for NexusDecisionProposalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct NexusDecisionResolutionProposalFile {
    path: PathBuf,
}

impl NexusDecisionResolutionProposalFile {
    fn create() -> Result<Self, String> {
        let directory = app_data_dir().join("nexus_decision_resolutions");
        fs::create_dir_all(&directory).map_err(|error| {
            format!("Failed to prepare Nexus decision resolution store: {error}")
        })?;
        Ok(Self {
            path: directory.join(format!("{}.json", Uuid::new_v4())),
        })
    }

    fn take(&self) -> Result<Option<NexusDecisionResolutionProposal>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("Failed to read Nexus decision resolution: {error}"));
            }
        };
        let _ = fs::remove_file(&self.path);
        if contents.chars().count() > MAX_NEXUS_ACTION_PROTOCOL_CHARS {
            return Err("Nexus decision resolution exceeded the allowed size.".to_string());
        }
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| "Nexus decision resolution was invalid.".to_string())
    }
}

impl Drop for NexusDecisionResolutionProposalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct NexusProjectProposalFile {
    path: PathBuf,
}

impl NexusProjectProposalFile {
    fn create() -> Result<Self, String> {
        let directory = app_data_dir().join("nexus_project_proposals");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Failed to prepare Nexus project proposal store: {error}"))?;
        Ok(Self {
            path: directory.join(format!("{}.json", Uuid::new_v4())),
        })
    }

    fn take(&self) -> Result<Option<NexusProjectProposal>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Failed to read Nexus project proposal: {error}")),
        };
        let _ = fs::remove_file(&self.path);
        if contents.chars().count() > MAX_NEXUS_ACTION_PROTOCOL_CHARS {
            return Err("Nexus project proposal exceeded the allowed size.".to_string());
        }
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| "Nexus project proposal was invalid.".to_string())
    }
}

impl Drop for NexusProjectProposalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct NexusContextProposalFile {
    path: PathBuf,
}

impl NexusContextProposalFile {
    fn create() -> Result<Self, String> {
        let directory = app_data_dir().join("nexus_context_proposals");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Failed to prepare Nexus context proposal store: {error}"))?;
        Ok(Self {
            path: directory.join(format!("{}.json", Uuid::new_v4())),
        })
    }

    fn take(&self) -> Result<Option<NexusContextProposal>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Failed to read Nexus context proposal: {error}")),
        };
        let _ = fs::remove_file(&self.path);
        if contents.chars().count() > MAX_NEXUS_CONTEXT_PROTOCOL_CHARS {
            return Err("Nexus context proposal exceeded the allowed size.".to_string());
        }
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|_| "Nexus context proposal was invalid.".to_string())
    }
}

impl Drop for NexusContextProposalFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn hermes_home() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".hermes"))
        .filter(|path| path.exists())
        .ok_or_else(|| "Hermes home was not found at ~/.hermes.".to_string())
}

fn hermes_python_path(home: &PathBuf) -> Result<PathBuf, String> {
    let path = home
        .join("hermes-agent")
        .join("venv")
        .join("bin")
        .join("python");
    if path.exists() {
        Ok(path)
    } else {
        Err(
            "Hermes Python runtime was not found. Re-run Hermes setup before using Nexus chat."
                .to_string(),
        )
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Library/Application Support")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.taisun.electron")
}

fn interaction_dir() -> PathBuf {
    app_data_dir().join("terrence_interactions")
}

fn config_text(home: &PathBuf) -> String {
    fs::read_to_string(home.join("config.yaml")).unwrap_or_default()
}

fn config_value(config: &str, section: &str, key: &str, fallback: &str) -> String {
    let mut in_section = false;
    for line in config.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 0 && trimmed == format!("{section}:") {
            in_section = true;
            continue;
        }
        if in_section && indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            break;
        }
        if in_section {
            if let Some(value) = trimmed.strip_prefix(&format!("{key}:")) {
                let cleaned = value.trim().trim_matches('"').trim_matches('\'');
                if !cleaned.is_empty() {
                    return cleaned.to_string();
                }
            }
        }
    }
    fallback.to_string()
}

fn configured_toolsets(config: &str) -> Vec<String> {
    let mut toolsets = Vec::new();
    let mut in_toolsets = false;

    for line in config.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 0 && trimmed == "toolsets:" {
            in_toolsets = true;
            continue;
        }
        if in_toolsets {
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    toolsets.push(name.to_string());
                }
                continue;
            }
            if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
                break;
            }
        }
    }

    toolsets
}

fn gateway_running() -> bool {
    std::process::Command::new("ps")
        .args(["-ax", "-o", "command="])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.contains("hermes_cli.main gateway run"))
        })
        .unwrap_or(false)
}

fn telegram_status(home: &PathBuf, running: bool) -> String {
    let log = fs::read_to_string(home.join("logs").join("gateway.log")).unwrap_or_default();
    let tail: String = log
        .chars()
        .rev()
        .take(16_000)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    telegram_status_from_log(&tail, running)
}

fn latest_log_marker(log: &str, markers: &[&str]) -> Option<usize> {
    markers.iter().filter_map(|marker| log.rfind(marker)).max()
}

fn telegram_status_from_log(log: &str, running: bool) -> String {
    if !running {
        return "offline".to_string();
    }

    let last_connected = latest_log_marker(
        log,
        &[
            "Connected to Telegram",
            "Telegram polling resumed after network error",
            "Telegram polling resumed after conflict retry",
        ],
    );
    let last_failure = latest_log_marker(
        log,
        &[
            "Telegram startup failed",
            "Failed to connect to Telegram",
            "Gateway failed to connect any configured messaging platform",
        ],
    );

    match (last_connected, last_failure) {
        (Some(connected), Some(failure)) if connected > failure => "connected".to_string(),
        (Some(_), None) => "connected".to_string(),
        _ => "unknown".to_string(),
    }
}

fn active_schedule_count(home: &PathBuf) -> u64 {
    let path = home.join("cron").join("jobs.json");
    let Ok(content) = fs::read_to_string(path) else {
        return 0;
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return 0;
    };
    let jobs = value
        .as_array()
        .or_else(|| value.get("jobs").and_then(Value::as_array));

    jobs.map(|jobs| {
        jobs.iter()
            .filter(|job| job.get("enabled").and_then(Value::as_bool).unwrap_or(true))
            .count() as u64
    })
    .unwrap_or(0)
}

fn persist_interaction(interaction: &TerrenceInteraction) -> Result<(), String> {
    let directory = interaction_dir();
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create interaction store: {error}"))?;
    let path = directory.join(format!("{}.json", interaction.id));
    let temporary = directory.join(format!("{}.tmp", interaction.id));
    let json = serde_json::to_vec_pretty(interaction)
        .map_err(|error| format!("Failed to serialize interaction: {error}"))?;
    fs::write(&temporary, json).map_err(|error| format!("Failed to write interaction: {error}"))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Failed to finalize interaction: {error}"))
}

fn parse_hermes_output(output: &str) -> (String, Option<String>) {
    let is_hermes_preamble = |line: &&str| {
        let trimmed = line.trim();
        trimmed.is_empty()
            || (trimmed.starts_with('\u{21bb}') && trimmed.contains("Resumed session"))
            || (trimmed.starts_with('\u{256d}') && trimmed.contains("Hermes"))
            || trimmed.starts_with('\u{250a}')
            || trimmed == "(empty)"
    };
    let mut lines: Vec<&str> = output.lines().collect();
    if let Some(banner_index) = lines.iter().rposition(|line| {
        let trimmed = line.trim();
        trimmed.starts_with('\u{256d}') && trimmed.contains("Hermes")
    }) {
        lines.drain(..=banner_index);
    }
    while lines.first().is_some_and(is_hermes_preamble) {
        lines.remove(0);
    }
    let session_index = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with("session_id:"));
    let session_id = session_index.and_then(|index| {
        lines[index]
            .trim()
            .strip_prefix("session_id:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    });
    if let Some(index) = session_index {
        lines.truncate(index);
    }

    (lines.join("\n").trim().to_string(), session_id)
}

fn extract_nexus_action_proposal(output: &str) -> (String, Option<NexusActionProposal>, bool) {
    let Some(open_index) = output.find(NEXUS_ACTION_OPEN) else {
        return (output.trim().to_string(), None, false);
    };
    let payload_start = open_index + NEXUS_ACTION_OPEN.len();
    let Some(close_offset) = output[payload_start..].find(NEXUS_ACTION_CLOSE) else {
        return (output.trim().to_string(), None, false);
    };
    let payload_end = payload_start + close_offset;
    let closing_end = payload_end + NEXUS_ACTION_CLOSE.len();
    let visible = format!("{}{}", &output[..open_index], &output[closing_end..])
        .trim()
        .to_string();
    let payload = output[payload_start..payload_end].trim();
    let malformed = visible.contains(NEXUS_ACTION_OPEN)
        || visible.contains(NEXUS_ACTION_CLOSE)
        || payload.chars().count() > MAX_NEXUS_ACTION_PROTOCOL_CHARS;
    if malformed {
        return (visible, None, true);
    }

    (
        visible,
        serde_json::from_str::<NexusActionProposal>(payload).ok(),
        true,
    )
}

fn nexus_runtime_system_instruction() -> &'static str {
    r#"You are Amara, Taisun's calm, perceptive operating partner inside Nexus. Taisun is the center: support his focus, health, relationships, craft, leadership, money, and projects without making any one business the default. Be direct, natural, warm, and brief unless depth is useful. Notice pressure, drift, avoidance, fatigue, confidence, and momentum without diagnosing or acting as a clinician. Help turn intentions into a clear next move.

Nexus policy: you cannot directly change the Mac from this conversation. nexus_observe is read-only. When the user explicitly requests a supported local change and all values are known, call nexus_request_action once to prepare it for approval. It does not execute anything. Never claim an app, browser, file, volume, message, or other external state changed until Nexus reports a verified outcome. Do not give terminal commands or a manual workaround for an available Nexus action. Ask a concise question when required values are missing.

For a substantive choice with viable alternatives, give a recommendation and call nexus_record_decision once. Use nexus_update_decision only when the user reports a choice or outcome for the supplied decision ID. For a meaningful personal, work, health, or creative project change, call nexus_upsert_project once using known facts only. For a durable user-stated goal, preference, commitment, observation, or identity fact, call the relevant nexus context tool once; do not store guesses, diagnoses, or casual chat.

Supported prepared actions: open an http(s) URL; open an application; reveal an inspected absolute path; set, adjust, or mute volume; organize an inspected Desktop, Documents, or Downloads root; or a bounded macOS workflow using the documented action schema. Use nexus_observe before directory organization or a workflow that needs filesystem details. Prepare at most one action after required observation. The user must approve it in Nexus before it runs."#
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TurnContextSelection {
    decisions: bool,
    projects: bool,
    actions: bool,
    automations: bool,
}

fn turn_context_selection(message: &str) -> TurnContextSelection {
    let message = format!(" {} ", message.to_ascii_lowercase());
    let contains_any = |terms: &[&str]| terms.iter().any(|term| message.contains(term));
    let local_action = contains_any(&[
        " open ",
        "launch",
        "browser",
        "website",
        "url",
        "youtube",
        "safari",
        "chrome",
        "volume",
        "mute",
        "desktop",
        "documents",
        "downloads",
        "folder",
        "file",
        "finder",
        "organize",
        "rename",
        " move ",
        "shortcut",
    ]);

    TurnContextSelection {
        decisions: contains_any(&[
            "decide",
            "decision",
            "choose",
            "choice",
            "option",
            "tradeoff",
            "trade-off",
            "should i",
            "pros and cons",
            "compare",
        ]),
        projects: contains_any(&[
            "project",
            "business",
            "taiyoskin",
            "taiyo skin",
            "film",
            "quiver",
            "career",
            "work",
            "build",
            "launch",
            "milestone",
            "blocker",
        ]),
        actions: local_action
            || contains_any(&[
                "again",
                "last time",
                "previous",
                "what happened",
                "did it",
                "completed",
                "status",
            ]),
        automations: contains_any(&[
            "remind",
            "reminder",
            "schedule",
            "automation",
            "automate",
            "recurring",
            "cron",
            "morning",
            "midday",
            "evening",
            "check in",
            "check-in",
            "follow up",
            "follow-up",
        ]),
    }
}

fn nexus_turn_query(
    message: &str,
    runtime_context: &terrence_context::RuntimeContextBrief,
    priority_brief: &str,
    decision_brief: &str,
    project_brief: &str,
    action_brief: &str,
    automation_brief: &str,
    health_brief: &str,
) -> String {
    let selection = turn_context_selection(message);
    let mut background_sections = Vec::new();
    if !runtime_context.text.is_empty() {
        background_sections.push(format!(
            "<nexus_context>\n{}\n</nexus_context>",
            runtime_context.text
        ));
    }
    if !priority_brief.is_empty() {
        background_sections.push(format!(
            "<nexus_priorities>\n{}\n</nexus_priorities>",
            priority_brief
        ));
    }
    if selection.decisions && !decision_brief.is_empty() {
        background_sections.push(format!(
            "<nexus_recent_decisions>\n{}\n</nexus_recent_decisions>",
            decision_brief
        ));
    }
    if selection.projects && !project_brief.is_empty() {
        background_sections.push(format!(
            "<nexus_project_state>\n{}\n</nexus_project_state>",
            project_brief
        ));
    }
    if selection.actions && !action_brief.is_empty() {
        background_sections.push(format!(
            "<nexus_verified_actions>\n{}\n</nexus_verified_actions>",
            action_brief
        ));
    }
    if selection.automations && !automation_brief.is_empty() {
        background_sections.push(format!(
            "<nexus_automation_history>\n{}\n</nexus_automation_history>",
            automation_brief
        ));
    }
    if !health_brief.is_empty() {
        background_sections.push(format!(
            "<noop_health_context>\n{}\n</noop_health_context>",
            health_brief
        ));
    }
    if background_sections.is_empty() {
        format!("User message:\n{message}")
    } else {
        format!(
            "{}\n\nUser message:\n{}",
            background_sections.join("\n\n"),
            message
        )
    }
}

fn append_runtime_notice(message: String, notice: String) -> String {
    if message.trim().is_empty() {
        notice
    } else {
        format!("{}\n\n{}", message.trim(), notice)
    }
}

fn prepared_action_message(title: &str) -> String {
    format!("I prepared {title} for your approval in Nexus. It will not run unless you approve it.")
}

fn user_facing_runtime_error(error: &str, fallback: &str) -> String {
    let redacted = continuity::redact_sensitive_text(error).trim().to_string();
    if redacted.is_empty()
        || redacted.contains("Resumed session")
        || redacted.contains("session_id:")
        || redacted.contains('\n')
        || redacted.contains('\r')
        || redacted.contains('┊')
        || redacted.contains('╭')
    {
        fallback.to_string()
    } else {
        redacted.chars().take(1_500).collect()
    }
}

fn sanitize_stored_interaction(
    mut interaction: TerrenceInteraction,
) -> (TerrenceInteraction, bool) {
    let mut changed = false;
    if let Some(message) = interaction.assistant_message.clone() {
        let (cleaned, session_id) = parse_hermes_output(&message);
        if cleaned != message {
            interaction.assistant_message = Some(cleaned);
            changed = true;
        }
        if interaction.hermes_session_id.is_none() && session_id.is_some() {
            interaction.hermes_session_id = session_id;
            changed = true;
        }
    }
    if let Some(error) = interaction.error.clone() {
        let cleaned = user_facing_runtime_error(&error, "Hermes could not complete the request.");
        if cleaned != error {
            interaction.error = Some(cleaned);
            changed = true;
        }
    }
    (interaction, changed)
}

fn interaction_failure(
    request: &TerrenceInteractionRequest,
    model: &str,
    provider: &str,
    started_at: String,
    elapsed: u64,
    error: String,
) -> TerrenceInteraction {
    TerrenceInteraction {
        id: Uuid::new_v4().to_string(),
        conversation_id: request.conversation_id.clone(),
        hermes_session_id: request.hermes_session_id.clone(),
        runtime: RUNTIME_NAME.to_string(),
        model: model.to_string(),
        provider: provider.to_string(),
        status: "failed".to_string(),
        user_message: request.message.trim().to_string(),
        assistant_message: None,
        error: Some(error),
        started_at,
        completed_at: Utc::now().to_rfc3339(),
        duration_ms: elapsed,
        context_item_ids: Vec::new(),
        action_id: None,
        runtime_profile: NEXUS_RUNTIME_PROFILE.to_string(),
    }
}

#[tauri::command]
pub fn get_terrence_runtime_health() -> Result<TerrenceRuntimeHealth, String> {
    let home = hermes_home()?;
    let config = config_text(&home);
    let model = config_value(&config, "model", "default", "unknown");
    let provider = config_value(&config, "model", "provider", "unknown");
    let running = gateway_running();
    let telegram = telegram_status(&home, running);
    let status = if running && telegram == "connected" {
        "online"
    } else if running {
        "degraded"
    } else {
        "offline"
    };
    let detail = match status {
        "online" => Some("Hermes gateway and Telegram are connected.".to_string()),
        "degraded" => Some(
            "Hermes gateway process is running, but Telegram health is not confirmed.".to_string(),
        ),
        _ => Some("Hermes gateway process is not running.".to_string()),
    };

    Ok(TerrenceRuntimeHealth {
        runtime: RUNTIME_NAME.to_string(),
        status: status.to_string(),
        model,
        provider,
        gateway_running: running,
        telegram_status: telegram,
        active_schedules: active_schedule_count(&home),
        configured_toolsets: configured_toolsets(&config),
        checked_at: Utc::now().to_rfc3339(),
        detail,
    })
}

#[tauri::command]
pub fn list_terrence_capabilities() -> Result<Vec<TerrenceCapability>, String> {
    let home = hermes_home()?;
    let config = config_text(&home);
    Ok(configured_toolsets(&config)
        .into_iter()
        .map(|name| TerrenceCapability {
            description: format!("Configured Hermes toolset: {name}"),
            name,
            status: "configured".to_string(),
        })
        .collect())
}

#[tauri::command]
pub async fn interact_with_terrence(
    bridge: State<'_, terrence_runtime_bridge::NexusRuntimeBridge>,
    request: TerrenceInteractionRequest,
) -> Result<TerrenceInteraction, String> {
    let started = Instant::now();
    let started_at = Utc::now().to_rfc3339();
    let message = request.message.trim().to_string();
    let home = hermes_home()?;
    let config = config_text(&home);
    let model = config_value(&config, "model", "default", "unknown");
    let provider = config_value(&config, "model", "provider", "unknown");

    if request.conversation_id.trim().is_empty() || message.is_empty() {
        let error = "A conversation ID and message are required.".to_string();
        let interaction = interaction_failure(
            &request,
            &model,
            &provider,
            started_at,
            started.elapsed().as_millis() as u64,
            error.clone(),
        );
        let _ = persist_interaction(&interaction);
        return Err(error);
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        let error = format!("Messages are limited to {MAX_MESSAGE_CHARS} characters.");
        let interaction = interaction_failure(
            &request,
            &model,
            &provider,
            started_at,
            started.elapsed().as_millis() as u64,
            error.clone(),
        );
        let _ = persist_interaction(&interaction);
        return Err(error);
    }

    let runtime_context = terrence_context::runtime_context_brief(3_200).unwrap_or_else(|_| {
        terrence_context::RuntimeContextBrief {
            text: String::new(),
            item_ids: Vec::new(),
        }
    });
    let priority_brief = terrence_priorities::runtime_priority_brief(2_000).unwrap_or_default();
    let decision_brief = terrence_priorities::runtime_decision_brief(1_600).unwrap_or_default();
    let project_brief = terrence_projects::runtime_project_brief(1_800).unwrap_or_default();
    let action_brief = terrence_actions::runtime_action_brief(1_200).unwrap_or_default();
    let automation_brief =
        terrence_automations::runtime_automation_brief(1_200).unwrap_or_default();
    let health_brief = noop::runtime_health_brief(1_200);
    let query = nexus_turn_query(
        &message,
        &runtime_context,
        &priority_brief,
        &decision_brief,
        &project_brief,
        &action_brief,
        &automation_brief,
        &health_brief,
    );

    let python = hermes_python_path(&home)?;
    let action_id = terrence_actions::begin_hermes_conversation_action(
        &request.conversation_id,
        runtime_context.item_ids.clone(),
    )
    .map(|action| action.id)
    .map_err(|error| {
        eprintln!("[terrence-runtime] could not create action ledger entry: {error}");
        error
    })
    .ok();
    let proposal_file = NexusActionProposalFile::create().ok();
    let decision_proposal_file = NexusDecisionProposalFile::create().ok();
    let decision_resolution_file = NexusDecisionResolutionProposalFile::create().ok();
    let project_proposal_file = NexusProjectProposalFile::create().ok();
    let context_proposal_file = NexusContextProposalFile::create().ok();

    let bridge_response = match bridge
        .request(
            &python,
            &home,
            terrence_runtime_bridge::NexusBridgeRequest {
                id: Uuid::new_v4().to_string(),
                conversation_id: request.conversation_id.clone(),
                message: message.clone(),
                query,
                system_instruction: nexus_runtime_system_instruction().to_string(),
                proposal_paths: terrence_runtime_bridge::NexusProposalPaths {
                    action: proposal_file.as_ref().map(|file| file.path.clone()),
                    decision: decision_proposal_file
                        .as_ref()
                        .map(|file| file.path.clone()),
                    decision_resolution: decision_resolution_file
                        .as_ref()
                        .map(|file| file.path.clone()),
                    project: project_proposal_file.as_ref().map(|file| file.path.clone()),
                    context: context_proposal_file.as_ref().map(|file| file.path.clone()),
                },
            },
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            if let Some(action_id) = action_id.as_deref() {
                let _ = terrence_actions::fail_terrence_action(action_id, error.clone());
            }
            let interaction = interaction_failure(
                &request,
                &model,
                &provider,
                started_at,
                started.elapsed().as_millis() as u64,
                error.clone(),
            );
            let _ = persist_interaction(&interaction);
            return Err(error);
        }
    };

    let raw_assistant_message = bridge_response.assistant_message;
    let session_id = bridge_response.session_id;
    let (mut assistant_message, inline_action_proposal, action_protocol_present) =
        extract_nexus_action_proposal(&raw_assistant_message);
    let proposal_file_result = proposal_file
        .as_ref()
        .map(NexusActionProposalFile::take)
        .transpose();
    let action_proposal_file_failed = proposal_file_result.is_err();
    let action_proposal = proposal_file_result
        .ok()
        .flatten()
        .flatten()
        .or(inline_action_proposal);
    let decision_proposal_file_result = decision_proposal_file
        .as_ref()
        .map(NexusDecisionProposalFile::take)
        .transpose();
    let decision_proposal_file_failed = decision_proposal_file_result.is_err();
    let decision_proposal = decision_proposal_file_result.ok().flatten().flatten();
    let decision_resolution_file_result = decision_resolution_file
        .as_ref()
        .map(NexusDecisionResolutionProposalFile::take)
        .transpose();
    let decision_resolution_file_failed = decision_resolution_file_result.is_err();
    let decision_resolution = decision_resolution_file_result.ok().flatten().flatten();
    let project_proposal_file_result = project_proposal_file
        .as_ref()
        .map(NexusProjectProposalFile::take)
        .transpose();
    let project_proposal_file_failed = project_proposal_file_result.is_err();
    let project_proposal = project_proposal_file_result.ok().flatten().flatten();
    let context_proposal_file_result = context_proposal_file
        .as_ref()
        .map(NexusContextProposalFile::take)
        .transpose();
    let context_proposal_file_failed = context_proposal_file_result.is_err();
    let context_proposal = context_proposal_file_result.ok().flatten().flatten();
    if let Some(proposal) = action_proposal {
        let prepared_action =
            terrence_actions::create_terrence_action(terrence_actions::CreateTerrenceActionInput {
                tool_id: proposal.tool_id,
                title: None,
                parameters: proposal.parameters,
                conversation_id: Some(request.conversation_id.clone()),
                context_item_ids: Some(runtime_context.item_ids.clone()),
            });
        assistant_message = match prepared_action {
            Ok(action) => prepared_action_message(&action.title),
            Err(_) => "I couldn't safely prepare that local action. Please rephrase it with the exact app, link, path, or volume you want.".to_string(),
        };
    } else if action_protocol_present || action_proposal_file_failed {
        assistant_message = append_runtime_notice(
            assistant_message,
            "I couldn't safely prepare that local action. Please rephrase it with the exact app, link, path, or volume you want.".to_string(),
        );
    }
    if let Some(proposal) = decision_proposal {
        if terrence_priorities::create_terrence_decision(
            terrence_priorities::CreateTerrenceDecisionInput {
                id: proposal.id,
                question: proposal.question,
                constraints: proposal.constraints,
                evidence: proposal.evidence,
                alternatives: proposal.alternatives,
                recommendation: proposal.recommendation,
                uncertainty: proposal.uncertainty,
                conversation_id: Some(request.conversation_id.clone()),
                context_item_ids: Some(runtime_context.item_ids.clone()),
                provenance_source: Some("hermes_nexus_runtime".to_string()),
            },
        )
        .is_err()
        {
            assistant_message = append_runtime_notice(
                assistant_message,
                "I couldn't save the decision notes this time.".to_string(),
            );
        }
    } else if decision_proposal_file_failed {
        assistant_message = append_runtime_notice(
            assistant_message,
            "I couldn't save the decision notes this time.".to_string(),
        );
    }
    if let Some(proposal) = decision_resolution {
        if terrence_priorities::resolve_terrence_decision(
            terrence_priorities::ResolveTerrenceDecisionInput {
                id: proposal.id,
                status: proposal.status,
                decision: proposal.decision,
                outcome: proposal.outcome,
            },
        )
        .is_err()
        {
            assistant_message = append_runtime_notice(
                assistant_message,
                "I couldn't update the earlier decision record this time.".to_string(),
            );
        }
    } else if decision_resolution_file_failed {
        assistant_message = append_runtime_notice(
            assistant_message,
            "I couldn't update the earlier decision record this time.".to_string(),
        );
    }
    if let Some(mut proposal) = project_proposal {
        proposal.provenance_source = Some("hermes_nexus_runtime".to_string());
        if terrence_projects::upsert_terrence_project(proposal).is_err() {
            assistant_message = append_runtime_notice(
                assistant_message,
                "I couldn't save the project state this time.".to_string(),
            );
        }
    } else if project_proposal_file_failed {
        assistant_message = append_runtime_notice(
            assistant_message,
            "I couldn't save the project state this time.".to_string(),
        );
    }
    if let Some(proposal) = context_proposal {
        if terrence_context::apply_hermes_context_event(proposal, "hermes_nexus_runtime").is_err() {
            assistant_message = append_runtime_notice(
                assistant_message,
                "I couldn't save the durable context this time.".to_string(),
            );
        }
    } else if context_proposal_file_failed {
        assistant_message = append_runtime_notice(
            assistant_message,
            "I couldn't save the durable context this time.".to_string(),
        );
    }
    if assistant_message.is_empty() {
        let error = "Hermes completed without a response.".to_string();
        if let Some(action_id) = action_id.as_deref() {
            let _ = terrence_actions::fail_terrence_action(action_id, error.clone());
        }
        let interaction = interaction_failure(
            &request,
            &model,
            &provider,
            started_at,
            started.elapsed().as_millis() as u64,
            error.clone(),
        );
        let _ = persist_interaction(&interaction);
        return Err(error);
    }

    let interaction = TerrenceInteraction {
        id: Uuid::new_v4().to_string(),
        conversation_id: request.conversation_id,
        hermes_session_id: session_id.or(request.hermes_session_id),
        runtime: RUNTIME_NAME.to_string(),
        model,
        provider,
        status: "succeeded".to_string(),
        user_message: message,
        assistant_message: Some(assistant_message),
        error: None,
        started_at,
        completed_at: Utc::now().to_rfc3339(),
        duration_ms: started.elapsed().as_millis() as u64,
        context_item_ids: runtime_context.item_ids,
        action_id: action_id.clone(),
        runtime_profile: NEXUS_RUNTIME_PROFILE.to_string(),
    };
    if let Some(action_id) = action_id.as_deref() {
        let _ = terrence_actions::complete_terrence_action(
            action_id,
            format!("Hermes replied in {} ms.", interaction.duration_ms),
        );
    }
    persist_interaction(&interaction)?;
    Ok(interaction)
}

#[tauri::command]
pub fn list_terrence_interactions(
    conversation_id: String,
    limit: Option<u32>,
) -> Result<Vec<TerrenceInteraction>, String> {
    if conversation_id.trim().is_empty() {
        return Ok(Vec::new());
    }
    let directory = interaction_dir();
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("Failed to read interactions: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to read interaction entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<TerrenceInteraction>(&content) else {
            continue;
        };
        let (record, changed) = sanitize_stored_interaction(record);
        if changed {
            let _ = persist_interaction(&record);
        }
        if record.conversation_id == conversation_id {
            records.push(record);
        }
    }
    records.sort_by(|left, right| left.started_at.cmp(&right.started_at));
    if let Some(limit) = limit {
        let skip = records.len().saturating_sub(limit as usize);
        return Ok(records.split_off(skip));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quiet_hermes_output() {
        let (response, session_id) =
            parse_hermes_output("Ready.\n\nsession_id: nexus-session-42\n");
        assert_eq!(response, "Ready.");
        assert_eq!(session_id.as_deref(), Some("nexus-session-42"));
    }

    #[test]
    fn removes_the_hermes_terminal_banner() {
        let output = "\u{256d}\u{2500} \u{2695} Hermes \u{2500}\nNEXUS_UI_OK\n\nsession_id: nexus-session-42\n";
        let (response, session_id) = parse_hermes_output(output);
        assert_eq!(response, "NEXUS_UI_OK");
        assert_eq!(session_id.as_deref(), Some("nexus-session-42"));
    }

    #[test]
    fn removes_tool_previews_before_the_hermes_terminal_banner() {
        let output = "  \u{250a} N preparing nexus_request_action...\n\u{256d}\u{2500} \u{2695} Hermes \u{2500}\nAction ready.\n\nsession_id: nexus-session-42\n";
        let (response, session_id) = parse_hermes_output(output);
        assert_eq!(response, "Action ready.");
        assert_eq!(session_id.as_deref(), Some("nexus-session-42"));
    }

    #[test]
    fn removes_tool_previews_when_the_terminal_banner_is_absent() {
        let output = "  \u{250a} N preparing nexus_request_action...\n  \u{250a} \u{26a1} nexus_req 0.0s\n\n(empty)\n\nAction ready.\n\nsession_id: nexus-session-42\n";
        let (response, session_id) = parse_hermes_output(output);
        assert_eq!(response, "Action ready.");
        assert_eq!(session_id.as_deref(), Some("nexus-session-42"));
    }

    #[test]
    fn prepared_action_message_does_not_claim_execution() {
        assert_eq!(
            prepared_action_message("Open https://example.com"),
            "I prepared Open https://example.com for your approval in Nexus. It will not run unless you approve it."
        );
    }

    #[test]
    fn removes_resumed_session_preamble_before_the_terminal_banner() {
        let output = "\u{21bb} Resumed session nexus-session-42 (1 user message, 2 total messages)\n\n\u{256d}\u{2500} \u{2695} Hermes \u{2500}\nNEXUS_CONTEXT_OK\n\nsession_id: nexus-session-42\n";
        let (response, session_id) = parse_hermes_output(output);
        assert_eq!(response, "NEXUS_CONTEXT_OK");
        assert_eq!(session_id.as_deref(), Some("nexus-session-42"));
    }

    #[test]
    fn keeps_response_when_session_metadata_is_absent() {
        let (response, session_id) = parse_hermes_output("A complete response.");
        assert_eq!(response, "A complete response.");
        assert!(session_id.is_none());
    }

    #[test]
    fn keeps_specialized_history_out_of_a_simple_turn() {
        let context = terrence_context::RuntimeContextBrief {
            text: "Current personal context".to_string(),
            item_ids: Vec::new(),
        };
        let query = nexus_turn_query(
            "Hey, how are you?",
            &context,
            "Current priorities",
            "Decision history",
            "Project history",
            "Action history",
            "Automation history",
            "NOOP health context",
        );

        assert!(query.contains("Current personal context"));
        assert!(query.contains("Current priorities"));
        assert!(!query.contains("Decision history"));
        assert!(!query.contains("Project history"));
        assert!(!query.contains("Action history"));
        assert!(!query.contains("Automation history"));
    }

    #[test]
    fn includes_only_relevant_operating_history_for_a_local_action() {
        let context = terrence_context::RuntimeContextBrief {
            text: "Current personal context".to_string(),
            item_ids: Vec::new(),
        };
        let query = nexus_turn_query(
            "Open YouTube in my browser again.",
            &context,
            "Current priorities",
            "Decision history",
            "Project history",
            "Action history",
            "Automation history",
            "NOOP health context",
        );

        assert!(query.contains("Action history"));
        assert!(!query.contains("Decision history"));
        assert!(!query.contains("Project history"));
        assert!(!query.contains("Automation history"));
    }

    #[test]
    fn nexus_system_instruction_preserves_approval_and_personal_boundaries() {
        let instruction = nexus_runtime_system_instruction();
        assert!(instruction.contains("Taisun is the center"));
        assert!(instruction.contains("does not execute anything"));
        assert!(instruction.contains("without diagnosing"));
        assert!(instruction.chars().count() < 5_000);
    }

    #[test]
    fn extracts_a_single_nexus_action_without_showing_protocol_markup() {
        let output = "I'll prepare that.\n<nexus_action>{\"tool_id\":\"system.open_url\",\"parameters\":{\"url\":\"https://example.com\"}}</nexus_action>";
        let (visible, proposal, present) = extract_nexus_action_proposal(output);
        assert_eq!(visible, "I'll prepare that.");
        assert!(present);
        let proposal = proposal.expect("valid action proposal");
        assert_eq!(proposal.tool_id, "system.open_url");
        assert_eq!(proposal.parameters["url"], "https://example.com");
    }

    #[test]
    fn removes_malformed_action_protocol_from_the_visible_reply() {
        let output = "<nexus_action>{not-json}</nexus_action>";
        let (visible, proposal, present) = extract_nexus_action_proposal(output);
        assert!(visible.is_empty());
        assert!(proposal.is_none());
        assert!(present);
    }

    #[test]
    fn reads_nested_model_configuration() {
        let config =
            "model:\n  default: local-amara-agent\n  provider: custom\ntoolsets:\n- hermes-cli\n";
        assert_eq!(
            config_value(config, "model", "default", "unknown"),
            "local-amara-agent"
        );
        assert_eq!(
            config_value(config, "model", "provider", "unknown"),
            "custom"
        );
        assert_eq!(configured_toolsets(config), vec!["hermes-cli"]);
    }

    #[test]
    fn telegram_health_reports_connected_after_a_recovered_poll() {
        let log = "[Telegram] Connected to Telegram (polling mode)\n\
                   [Telegram] Telegram network error (attempt 1/10)\n\
                   [Telegram] Telegram polling resumed after network error (attempt 1)";
        assert_eq!(telegram_status_from_log(log, true), "connected");
    }

    #[test]
    fn telegram_health_reports_unknown_when_the_latest_startup_failed() {
        let log = "[Telegram] Connected to Telegram (polling mode)\n\
                   [Telegram] Telegram startup failed: invalid token";
        assert_eq!(telegram_status_from_log(log, true), "unknown");
        assert_eq!(telegram_status_from_log(log, false), "offline");
    }
}
