//! The existing Amara local tools as controller operations (decision 0052).
//!
//! Execution is unchanged (`execute_local_tool`, the same code the approval path always ran). What
//! the controller adds: the ledger, approval binding, verification and restart recovery.
//!
//! Read-back exists only where the destination can be read independently:
//! - `system.set_volume` / `system.set_mute`: a separate `osascript` read of the volume settings.
//! - Everything else (open a link, open an app, reveal in Finder, adjust volume by a delta, a Mac
//!   workflow, organising a folder) has no independent read-back here, so it is recorded
//!   UNVERIFIED, never VERIFIED.

use super::{execute_local_tool, normalize_parameters};
use crate::commands::execution::{
    ActionClass, DataScope, OpError, Operation, OperationSpec, RetryPolicy, Risk,
};
use serde_json::{json, Value};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

struct LocalTool {
    spec: OperationSpec,
}

const fn local(
    id: &'static str,
    class: ActionClass,
    risk: Risk,
    scope: DataScope,
    timeout_secs: u64,
) -> OperationSpec {
    OperationSpec {
        action_id: id,
        tool_id: id,
        target_system: "macos",
        class,
        risk,
        scope,
        timeout: Duration::from_secs(timeout_secs),
        // The existing manual Retry button creates a new action; the controller never auto-retries.
        retry: RetryPolicy::Never,
        personal_fields: &[],
    }
}

/// Every approval-gated local tool, with its permission class.
pub(super) const SPECS: [OperationSpec; 8] = [
    local(
        "system.open_url",
        ActionClass::ConsequentialExternal,
        Risk::Medium,
        DataScope::None,
        15,
    ),
    local(
        "system.open_application",
        ActionClass::ReversibleLocal,
        Risk::Low,
        DataScope::System,
        15,
    ),
    local(
        "filesystem.reveal",
        ActionClass::ReversibleLocal,
        Risk::Low,
        DataScope::Personal,
        15,
    ),
    local(
        "system.set_volume",
        ActionClass::ReversibleLocal,
        Risk::Low,
        DataScope::System,
        15,
    ),
    local(
        "system.adjust_volume",
        ActionClass::ReversibleLocal,
        Risk::Low,
        DataScope::System,
        15,
    ),
    local(
        "system.set_mute",
        ActionClass::ReversibleLocal,
        Risk::Low,
        DataScope::System,
        15,
    ),
    // Steps can open links and run Shortcuts, so treat the whole workflow as external.
    local(
        "macos.workflow",
        ActionClass::ConsequentialExternal,
        Risk::Medium,
        DataScope::System,
        120,
    ),
    local(
        "filesystem.organize_directory",
        ActionClass::ReversibleLocal,
        Risk::Medium,
        DataScope::Personal,
        120,
    ),
];

impl Operation for LocalTool {
    fn spec(&self) -> &OperationSpec {
        &self.spec
    }

    fn validate(&self, p: &Value) -> Result<(), String> {
        normalize_parameters(self.spec.tool_id, p.clone()).map(|_| ())
    }

    fn target_ref(&self, p: &Value) -> Option<String> {
        match self.spec.tool_id {
            "system.open_url" => p["url"].as_str().map(str::to_string),
            "system.open_application" => p["application"].as_str().map(str::to_string),
            "filesystem.reveal" => p["path"].as_str().map(str::to_string),
            "system.set_volume" | "system.adjust_volume" | "system.set_mute" => {
                Some("output volume".into())
            }
            _ => None,
        }
    }

    fn expected_state(&self, p: &Value) -> Option<Value> {
        match self.spec.tool_id {
            "system.set_volume" => p["volume"].as_u64().map(|v| json!({"volume": v})),
            "system.set_mute" => p["muted"].as_bool().map(|m| json!({"muted": m})),
            _ => None,
        }
    }

    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        execute_local_tool(self.spec.tool_id, p)
            .map(|message| json!({ "message": message }))
            .map_err(|message| OpError::Failed {
                message,
                // The command may have run before it reported failure: never retry on our own.
                submitted: true,
            })
    }

    fn read_back(&self, _p: &Value) -> Option<Result<Value, String>> {
        match self.spec.tool_id {
            "system.set_volume" => {
                Some(read_volume_settings().map(|s| json!({"volume": s.volume})))
            }
            "system.set_mute" => Some(read_volume_settings().map(|s| json!({"muted": s.muted}))),
            _ => None,
        }
    }

    fn matches(&self, expected: &Value, actual: &Value) -> bool {
        match (expected["volume"].as_u64(), actual["volume"].as_u64()) {
            // macOS stores volume in steps, so a set of 33 can read back as 32 or 34.
            (Some(e), Some(a)) => e.abs_diff(a) <= 2,
            _ => expected == actual,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct VolumeSettings {
    pub volume: u64,
    pub muted: bool,
}

/// `output volume:40, input volume:50, alert volume:100, output muted:false`
pub(super) fn parse_volume_settings(s: &str) -> Result<VolumeSettings, String> {
    let field = |name: &str| {
        s.split(',')
            .map(str::trim)
            .find_map(|kv| kv.strip_prefix(name).map(str::trim))
            .ok_or_else(|| format!("volume settings have no {name}"))
    };
    let volume = field("output volume:")?
        .parse::<u64>()
        .map_err(|_| "output volume is not a number".to_string())?;
    let muted = match field("output muted:")? {
        "true" => true,
        "false" => false,
        _ => return Err("output muted is not true/false".into()),
    };
    Ok(VolumeSettings { volume, muted })
}

fn read_volume_settings() -> Result<VolumeSettings, String> {
    let out = Command::new("/usr/bin/osascript")
        .args(["-e", "get volume settings"])
        .output()
        .map_err(|e| format!("could not read volume settings: {e}"))?;
    if !out.status.success() {
        return Err("could not read volume settings".into());
    }
    parse_volume_settings(String::from_utf8_lossy(&out.stdout).trim())
}

pub(crate) fn operations() -> Vec<Arc<dyn Operation>> {
    SPECS
        .iter()
        .map(|s| Arc::new(LocalTool { spec: s.clone() }) as Arc<dyn Operation>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_settings_parse_and_odd_output_is_an_error_not_a_guess() {
        assert_eq!(
            parse_volume_settings(
                "output volume:40, input volume:50, alert volume:100, output muted:false"
            ),
            Ok(VolumeSettings {
                volume: 40,
                muted: false
            })
        );
        assert!(
            parse_volume_settings(
                "output volume:0, input volume:50, alert volume:100, output muted:true"
            )
            .unwrap()
            .muted
        );
        assert!(parse_volume_settings("output volume:missing value, output muted:false").is_err());
        assert!(parse_volume_settings("").is_err());
    }

    #[test]
    fn only_volume_and_mute_claim_a_read_back() {
        for op in operations() {
            let id = op.spec().tool_id;
            let has = op.read_back(&json!({})).is_some();
            assert_eq!(
                has,
                matches!(id, "system.set_volume" | "system.set_mute"),
                "{id}"
            );
            // Without expected state there's nothing to verify against.
            assert_eq!(
                op.expected_state(&json!({"volume": 3, "muted": true}))
                    .is_some(),
                has,
                "{id}"
            );
        }
    }

    /// Opt-in, read-only: reads this Mac's real volume settings through the same helper the
    /// verifier uses. Never sets anything. `NEXUS_LIVE_MAC=1 cargo test --lib live_volume -- --ignored`
    #[test]
    #[ignore]
    fn live_volume_read_back_parses_the_real_settings() {
        if std::env::var("NEXUS_LIVE_MAC").ok().as_deref() != Some("1") {
            return;
        }
        let s = read_volume_settings().expect("osascript read");
        assert!(s.volume <= 100);
        eprintln!(
            "live volume read-back ok: volume={} muted={}",
            s.volume, s.muted
        );
    }

    #[test]
    fn volume_matches_within_the_step_size() {
        let op = operations()
            .into_iter()
            .find(|o| o.spec().tool_id == "system.set_volume")
            .unwrap();
        assert!(op.matches(&json!({"volume": 33}), &json!({"volume": 31})));
        assert!(!op.matches(&json!({"volume": 33}), &json!({"volume": 20})));
    }
}
