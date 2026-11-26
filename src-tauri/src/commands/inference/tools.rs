//! Tool authorization: models propose, application code validates and executes.
//!
//! A proposed call runs only when all of these hold:
//! 1. the tool was offered in this request,
//! 2. the caller's [`ToolGrant`] names it,
//! 3. a state-changing tool also has `allow_state_changing` on the grant (Phase 3's execution
//!    controller will own approvals; until then this stays off for every caller),
//! 4. its arguments validate against the tool's own schema.
//!
//! Anything else is denied and *not run*; the model is told "not permitted" so it can answer.
//! A tool's result is the deterministic service's output, fed back as a `tool` message. It is not
//! proof of a successful outcome: callers that change state verify separately.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

use super::{
    infer_with, schema, InferenceError, InferenceOutcome, InferenceRequest, Message, PolicyInputs,
    ToolCall,
};
use crate::commands::llm_router::RouterTransport;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    ReadOnly,
    StateChanging,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments (the subset in [`schema`]).
    pub parameters: Value,
    pub effect: ToolEffect,
}

impl ToolSpec {
    pub(crate) fn openai_json(&self) -> Value {
        json!({"type": "function", "function": {
            "name": self.name, "description": self.description, "parameters": self.parameters}})
    }
}

/// What the caller allows this request to run.
#[derive(Clone, Debug, Default)]
pub struct ToolGrant {
    pub allowed: HashSet<String>,
    pub allow_state_changing: bool,
}

impl ToolGrant {
    pub fn read_only(names: &[&str]) -> Self {
        Self {
            allowed: names.iter().map(|s| s.to_string()).collect(),
            allow_state_changing: false,
        }
    }
}

/// Why a call was refused (fixed labels, safe to log).
pub(crate) fn authorize<'a>(
    call: &ToolCall,
    offered: &'a [ToolSpec],
    grant: &ToolGrant,
) -> Result<&'a ToolSpec, &'static str> {
    let spec = offered
        .iter()
        .find(|t| t.name == call.name)
        .ok_or("not_offered")?;
    if !grant.allowed.contains(&call.name) {
        return Err("not_granted");
    }
    if spec.effect == ToolEffect::StateChanging && !grant.allow_state_changing {
        return Err("state_change_needs_approval");
    }
    schema::validate(&spec.parameters, &call.arguments).map_err(|_| "invalid_arguments")?;
    Ok(spec)
}

/// The deterministic service behind the tools. Only called for authorized calls.
pub(crate) trait ToolExecutor {
    async fn execute(&self, name: &str, args: &Value) -> Result<Value, String>;
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolEvent {
    Executed { name: String },
    Denied { name: String, reason: &'static str },
    Failed { name: String },
}

const MAX_TOOL_RESULT_CHARS: usize = 8_000;

fn capped(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() <= MAX_TOOL_RESULT_CHARS {
        return s;
    }
    let cut: String = s.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    json!({"truncated": true, "partial": cut}).to_string()
}

/// Inference with an application-controlled tool loop: at most `max_steps` model turns. Only
/// granted tools are offered; every proposal still goes through [`authorize`].
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_with_tools(
    transport: &impl RouterTransport,
    base: &str,
    persona: &str,
    req: &InferenceRequest,
    inputs: &PolicyInputs,
    grant: &ToolGrant,
    executor: &impl ToolExecutor,
    max_steps: u8,
) -> Result<(InferenceOutcome, Vec<ToolEvent>), InferenceError> {
    let mut turn = req.clone();
    turn.tools.retain(|t| grant.allowed.contains(&t.name));
    let mut events = vec![];
    for _ in 0..max_steps.max(1) {
        let outcome = infer_with(transport, base, persona, &turn, inputs).await?;
        if outcome.tool_calls.is_empty() {
            return Ok((outcome, events));
        }
        turn.messages.push(Message {
            tool_calls: outcome.tool_calls.clone(),
            ..Message::assistant(outcome.content.clone())
        });
        for call in &outcome.tool_calls {
            let result = match authorize(call, &turn.tools, grant) {
                Err(reason) => {
                    eprintln!(
                        "[inference] tool denied purpose={} reason={reason}",
                        req.purpose
                    );
                    events.push(ToolEvent::Denied {
                        name: call.name.clone(),
                        reason,
                    });
                    json!({"error": "not permitted"})
                }
                Ok(_) => match executor.execute(&call.name, &call.arguments).await {
                    Ok(v) => {
                        events.push(ToolEvent::Executed {
                            name: call.name.clone(),
                        });
                        v
                    }
                    Err(_) => {
                        events.push(ToolEvent::Failed {
                            name: call.name.clone(),
                        });
                        json!({"error": "the tool failed"})
                    }
                },
            };
            turn.messages
                .push(Message::tool_result(call.id.clone(), capped(&result)));
        }
    }
    Err(InferenceError::ToolLoopLimit)
}
