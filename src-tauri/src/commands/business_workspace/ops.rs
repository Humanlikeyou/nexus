//! Business action approvals through the execution controller (decisions 0052, 0063).
//!
//! A change Amara proposes to a business's context is a `proposal` row; it does nothing on its own.
//! Applying it is the registered operation `business.context.apply_proposal`: reversible, local,
//! business scope, submitted with origin `model`, so the controller's permission table requires the
//! user's approval. The approval is granted only from the native dialog (`commands.rs`), and it is
//! bound to the exact parameters, including the SHA-256 of the text the dialog showed. The
//! controller then reads the item back and records VERIFIED only when it holds that text, approved.

use super::{local_id_ok, sha256_hex, store_in, BusinessId};
use crate::commands::execution::{
    ActionClass, DataScope, OpError, Operation, OperationSpec, RetryPolicy, Risk,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const APPLY_ACTION: &str = "business.context.apply_proposal";

const APPLY_SPEC: OperationSpec = OperationSpec {
    action_id: APPLY_ACTION,
    tool_id: "business_workspace.context",
    target_system: "business_workspace",
    class: ActionClass::ReversibleLocal,
    risk: Risk::Medium,
    scope: DataScope::Business,
    timeout: Duration::from_secs(10),
    retry: RetryPolicy::Never,
    // Business text is not stored in the execution ledger; the workspace keeps it with its history.
    personal_fields: &[],
};

/// The hash an approval is bound to: the proposed title and body exactly as shown.
pub(crate) fn content_sha(title: &str, body: &str) -> String {
    sha256_hex(format!("{title}\n\u{1f}\n{body}").as_bytes())
}

pub(crate) struct ApplyProposal {
    pub data: PathBuf,
}

impl ApplyProposal {
    fn ids(p: &Value) -> Result<(BusinessId, String, String), String> {
        let b = BusinessId::parse(p["business_id"].as_str().ok_or("business_id required")?)?;
        let id = p["proposal_id"]
            .as_str()
            .ok_or("proposal_id required")?
            .to_string();
        local_id_ok(&id)?;
        let sha = p["content_sha256"]
            .as_str()
            .ok_or("content_sha256 required")?
            .to_string();
        if sha.len() != 64 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("content_sha256 is not a SHA-256".into());
        }
        Ok((b, id, sha))
    }
}

impl Operation for ApplyProposal {
    fn spec(&self) -> &OperationSpec {
        &APPLY_SPEC
    }
    fn validate(&self, p: &Value) -> Result<(), String> {
        Self::ids(p).map(|_| ())
    }
    fn target_ref(&self, p: &Value) -> Option<String> {
        let (b, id, _) = Self::ids(p).ok()?;
        Some(super::biz_ref(&b, "proposal", Some(&id)))
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        let (_, _, sha) = Self::ids(p).ok()?;
        Some(json!({ "status": "approved", "content_sha256": sha }))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let fail = |message: String| OpError::Failed {
            message,
            submitted: false,
        };
        let (b, id, sha) = Self::ids(p).map_err(fail)?;
        let t = store_in(&self.data).tenant(b.as_str()).map_err(fail)?;
        let prop = t
            .proposal_get(&id)
            .map_err(fail)?
            .ok_or_else(|| fail("no such proposal".into()))?;
        if content_sha(&prop.title, &prop.body) != sha {
            return Err(fail(
                "the proposal is not the text that was approved".into(),
            ));
        }
        let item = t.apply_proposal(&id).map_err(fail)?;
        Ok(json!({ "item_id": item.item_id, "version": item.version }))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        let (b, id, _) = Self::ids(p).ok()?;
        Some((|| {
            let t = store_in(&self.data).tenant(b.as_str())?;
            let prop = t.proposal_get(&id)?.ok_or("no such proposal")?;
            let item = t.context_get(&prop.item_id)?.ok_or("the item is gone")?;
            Ok(
                json!({ "status": item.status, "content_sha256": content_sha(&item.title, &item.body) }),
            )
        })())
    }
}

pub(crate) fn operations() -> Vec<Arc<dyn Operation>> {
    vec![Arc::new(ApplyProposal {
        data: crate::app_paths::data_dir(),
    })]
}
