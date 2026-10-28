//! Private persistent stdio bridge to the Nexus-scoped Hermes runtime.
//!
//! This is intentionally not an HTTP server. The desktop app owns one child
//! process and serializes requests through it, so proposal-path environment
//! variables cannot leak into Telegram or another Hermes client.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const NEXUS_SESSION_SOURCE: &str = "nexus";
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct NexusRuntimeBridge {
    process: Mutex<Option<BridgeProcess>>,
}

impl Default for NexusRuntimeBridge {
    fn default() -> Self {
        Self {
            process: Mutex::new(None),
        }
    }
}

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusBridgeRequest {
    pub id: String,
    pub conversation_id: String,
    pub message: String,
    pub query: String,
    pub system_instruction: String,
    pub proposal_paths: NexusProposalPaths,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusProposalPaths {
    pub action: Option<PathBuf>,
    pub decision: Option<PathBuf>,
    pub decision_resolution: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub context: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NexusBridgeResponse {
    id: Option<String>,
    ok: bool,
    assistant_message: Option<String>,
    session_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug)]
pub struct NexusBridgeReply {
    pub assistant_message: String,
    pub session_id: Option<String>,
}

impl NexusRuntimeBridge {
    pub async fn request(
        &self,
        python: &Path,
        hermes_home: &Path,
        request: NexusBridgeRequest,
    ) -> Result<NexusBridgeReply, String> {
        let mut process = self.process.lock().await;
        if process
            .as_mut()
            .is_some_and(|bridge| bridge.child.try_wait().ok().flatten().is_some())
        {
            *process = None;
        }
        if process.is_none() {
            *process = Some(spawn_bridge(python, hermes_home)?);
        }

        let encoded = serde_json::to_string(&request)
            .map_err(|_| "Amara OS could not prepare the local Hermes request.".to_string())?;
        let expected_id = request.id.clone();
        let bridge = process.as_mut().expect("bridge process was initialized");
        if let Err(error) = bridge.stdin.write_all(encoded.as_bytes()).await {
            reset_process(&mut process);
            return Err(format!(
                "Amara OS local Hermes runtime stopped before the request: {error}"
            ));
        }
        if let Err(error) = bridge.stdin.write_all(b"\n").await {
            reset_process(&mut process);
            return Err(format!(
                "Amara OS local Hermes runtime stopped before the request: {error}"
            ));
        }
        if let Err(error) = bridge.stdin.flush().await {
            reset_process(&mut process);
            return Err(format!(
                "Amara OS local Hermes runtime stopped before the request: {error}"
            ));
        }

        let response_line = match tokio::time::timeout(BRIDGE_TIMEOUT, bridge.stdout.next_line())
            .await
        {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => {
                reset_process(&mut process);
                return Err("Amara OS local Hermes runtime ended without a response.".to_string());
            }
            Ok(Err(error)) => {
                reset_process(&mut process);
                return Err(format!(
                    "Amara OS could not read the local Hermes response: {error}"
                ));
            }
            Err(_) => {
                reset_process(&mut process);
                return Err("Hermes did not complete within 30 minutes.".to_string());
            }
        };
        let response: NexusBridgeResponse = match serde_json::from_str(&response_line) {
            Ok(response) => response,
            Err(_) => {
                reset_process(&mut process);
                return Err(
                    "Amara OS local Hermes runtime returned an invalid response.".to_string(),
                );
            }
        };
        if response.id.as_deref() != Some(expected_id.as_str()) {
            reset_process(&mut process);
            return Err(
                "Amara OS local Hermes runtime returned a mismatched response.".to_string(),
            );
        }
        if !response.ok {
            return Err(response
                .error
                .filter(|error| !error.trim().is_empty())
                .unwrap_or_else(|| "Hermes could not complete the request.".to_string()));
        }
        let assistant_message = response
            .assistant_message
            .filter(|message| !message.trim().is_empty())
            .ok_or_else(|| "Hermes completed without a response.".to_string())?;
        Ok(NexusBridgeReply {
            assistant_message,
            session_id: response.session_id.filter(|id| !id.trim().is_empty()),
        })
    }
}

fn spawn_bridge(python: &Path, hermes_home: &Path) -> Result<BridgeProcess, String> {
    let agent_root = hermes_home.join("hermes-agent");
    let mut child = Command::new(python)
        .args(["-m", "hermes_cli.nexus_runtime"])
        .current_dir(agent_root)
        .env("HERMES_HOME", hermes_home)
        .env("HERMES_SESSION_SOURCE", NEXUS_SESSION_SOURCE)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("Unable to start the local Hermes runtime: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Amara OS could not open the local Hermes input channel.".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Amara OS could not open the local Hermes output channel.".to_string())?;
    Ok(BridgeProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout).lines(),
    })
}

fn reset_process(process: &mut Option<BridgeProcess>) {
    if let Some(mut bridge) = process.take() {
        let _ = bridge.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_request_uses_camel_case_protocol_fields() {
        let request = NexusBridgeRequest {
            id: "request-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            message: "Hello".to_string(),
            query: "Amara OS context\n\nUser message: Hello".to_string(),
            system_instruction: "Amara OS system instruction".to_string(),
            proposal_paths: NexusProposalPaths {
                action: Some(PathBuf::from("/tmp/action.json")),
                decision: None,
                decision_resolution: None,
                project: None,
                context: None,
            },
        };

        let value = serde_json::to_value(request).expect("serialize bridge request");
        assert_eq!(value["conversationId"], "conversation-1");
        assert_eq!(value["systemInstruction"], "Amara OS system instruction");
        assert_eq!(value["proposalPaths"]["action"], "/tmp/action.json");
        assert!(value.get("conversation_id").is_none());
    }
}
