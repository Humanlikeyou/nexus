//! Tauri commands for business workspaces (decision 0063). Every command names one business and
//! reaches its records only through a `Tenant` for that id. Secrets are write-only: no command
//! returns one. Nothing here reaches the network.

use super::connectors::{BusinessConnector, BusinessSyncRun};
use super::ops::{content_sha, APPLY_ACTION};
use super::{
    live, store_in, Business, BusinessContextInput, BusinessContextItem, BusinessContextVersion,
    BusinessDocument, BusinessKnowledgeHit, BusinessMemoryNote, BusinessPermission,
    BusinessProposal, BusinessSectionCount, WorkspaceStore,
};
use crate::commands::amara_store::commands::{ActivityRow, RecommendationRow};
use crate::commands::amara_store::{store_for, Scope};
use crate::commands::execution::{Controller, ExecRequest, ExecStatus, Origin};
use crate::commands::keychain::{Keychain, MacKeychain};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("workspace task failed: {e}"))?
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessConnectorSummary {
    pub connector_id: String,
    pub label: String,
    pub state: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessWorkspaceOverview {
    pub business: Business,
    pub sections: Vec<BusinessSectionCount>,
    pub connectors: Vec<BusinessConnectorSummary>,
    pub documents: i64,
    pub approved_documents: i64,
    pub pending_proposals: i64,
    pub memory_notes: i64,
}

pub(crate) fn overview_in(
    store: &WorkspaceStore,
    kc: &dyn Keychain,
    business_id: &str,
) -> Result<BusinessWorkspaceOverview, String> {
    let t = store.tenant(business_id)?;
    let docs = t.documents()?;
    Ok(BusinessWorkspaceOverview {
        business: t.business()?,
        sections: t.section_counts()?,
        connectors: t
            .connectors(kc)?
            .into_iter()
            .map(|c| BusinessConnectorSummary {
                connector_id: c.connector_id,
                label: c.label,
                state: c.state,
            })
            .collect(),
        documents: docs.iter().filter(|d| d.status != "archived").count() as i64,
        approved_documents: docs.iter().filter(|d| d.status == "approved").count() as i64,
        pending_proposals: t.proposals(Some("pending"))?.len() as i64,
        memory_notes: t
            .memory_list()?
            .iter()
            .filter(|m| m.status == "kept")
            .count() as i64,
    })
}

/// Applies a pending proposal after the user's approval: grant bound to the exact text, submit as
/// origin `model`, then settle the proposal from the controller's verified result. The dialog is the
/// caller's; this is the part tests drive.
pub(crate) fn approve_proposal_in(
    data: &Path,
    controller: &Controller,
    business_id: &str,
    proposal_id: &str,
) -> Result<BusinessProposal, String> {
    let store = store_in(data);
    let t = store.tenant(business_id)?;
    let p = t.proposal_get(proposal_id)?.ok_or("no such proposal")?;
    if p.status != "pending" {
        return Err("this proposal was already decided".into());
    }
    let params = serde_json::json!({
        "business_id": business_id,
        "proposal_id": proposal_id,
        "content_sha256": content_sha(&p.title, &p.body),
    });
    let approval = controller
        .grant_user_approval(APPLY_ACTION, &params, Duration::from_secs(60))
        .map_err(|e| e.to_string())?;
    let rec = controller
        .submit(&ExecRequest {
            action_id: APPLY_ACTION.into(),
            workflow_id: format!("business:{business_id}"),
            idempotency_key: format!("biz-proposal:{business_id}:{proposal_id}"),
            origin: Origin::Model,
            params,
            approval_id: Some(approval),
        })
        .map_err(|e| e.to_string())?;
    let r = rec.record();
    if r.status == ExecStatus::Verified {
        t.proposal_decide(proposal_id, "applied", Some(&r.execution_id), None)
    } else {
        let why = r
            .error
            .clone()
            .unwrap_or_else(|| format!("ended {}", r.status.as_str()));
        t.proposal_decide(proposal_id, "failed", Some(&r.execution_id), Some(&why))
    }
}

/// The dialog text: everything the change does, in full. Too long to show in full → refused.
pub(crate) fn proposal_dialog_text(
    data: &Path,
    business_id: &str,
    proposal_id: &str,
) -> Result<String, String> {
    let t = store_in(data).tenant(business_id)?;
    let b = t.business()?;
    let p = t.proposal_get(proposal_id)?.ok_or("no such proposal")?;
    if p.status != "pending" {
        return Err("this proposal was already decided".into());
    }
    let item = t.context_get(&p.item_id)?.ok_or("the item is gone")?;
    let detail = format!(
        "Business: {}\nSection: {}\nItem: {} (now {})\n\nAmara proposes this text. Allow replaces the item and marks it approved.\n\nTitle: {}\n\n{}",
        b.name,
        super::section_label(&item.section),
        item.title,
        item.status,
        p.title,
        p.body
    );
    crate::commands::approval_gate::approval_text(
        "Apply Amara's change to business material",
        &detail,
    )
}

// ─── Workspaces ──────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_list() -> Result<Vec<Business>, String> {
    blocking(|| live().list_businesses()).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_create(
    name: String,
    description: Option<String>,
    venture_id: Option<String>,
) -> Result<Business, String> {
    blocking(move || {
        live().create_business(&name, None, description.as_deref(), venture_id.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_update(
    business_id: String,
    name: String,
    description: Option<String>,
    venture_id: Option<String>,
    status: String,
) -> Result<Business, String> {
    blocking(move || {
        live().tenant(&business_id)?.update_identity(
            &name,
            description.as_deref(),
            venture_id.as_deref(),
            &status,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_overview(business_id: String) -> Result<BusinessWorkspaceOverview, String> {
    blocking(move || overview_in(&live(), &MacKeychain, &business_id)).await
}

// ─── Context and memory ──────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_context_list(
    business_id: String,
    section: Option<String>,
) -> Result<Vec<BusinessContextItem>, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .context_list(section.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_context_save(
    business_id: String,
    input: BusinessContextInput,
) -> Result<BusinessContextItem, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .context_save(&input, "taisun", "user", None)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_context_set_status(
    business_id: String,
    item_id: String,
    status: String,
    expected_version: i64,
) -> Result<BusinessContextItem, String> {
    blocking(move || {
        live().tenant(&business_id)?.context_set_status(
            &item_id,
            &status,
            expected_version,
            "taisun",
        )
    })
    .await
}

/// Exactly what Amara may be given about this business under its current permissions (approved
/// items only by default). Refused when reading context is not permitted.
#[tauri::command]
#[specta::specta]
pub async fn business_context_for_amara(
    business_id: String,
) -> Result<Vec<BusinessContextItem>, String> {
    blocking(move || live().tenant(&business_id)?.context_for_model()).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_context_history(
    business_id: String,
    item_id: String,
) -> Result<Vec<BusinessContextVersion>, String> {
    blocking(move || live().tenant(&business_id)?.context_history(&item_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_memory_list(business_id: String) -> Result<Vec<BusinessMemoryNote>, String> {
    blocking(move || live().tenant(&business_id)?.memory_list()).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_memory_add(
    business_id: String,
    text: String,
) -> Result<BusinessMemoryNote, String> {
    blocking(move || live().tenant(&business_id)?.memory_add(&text, "user")).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_memory_decide(
    business_id: String,
    memory_id: String,
    status: String,
) -> Result<BusinessMemoryNote, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .memory_decide(&memory_id, &status)
    })
    .await
}

// ─── Documents and knowledge ─────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_documents(business_id: String) -> Result<Vec<BusinessDocument>, String> {
    blocking(move || live().tenant(&business_id)?.documents()).await
}

/// Opens the native file picker; the chosen file is copied in as a draft. None = cancelled.
#[tauri::command]
#[specta::specta]
pub async fn business_document_add(
    app: tauri::AppHandle,
    business_id: String,
) -> Result<Option<BusinessDocument>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    live().tenant(&business_id)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut d = app.dialog().file().set_title("Add a business document");
        if let Some(w) = app.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        d.blocking_pick_file()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    blocking(move || {
        let meta =
            std::fs::symlink_metadata(&path).map_err(|_| "the file can't be read".to_string())?;
        if !meta.is_file() || meta.len() as usize > super::MAX_DOCUMENT_BYTES {
            return Err("choose a regular file under 10 MB".into());
        }
        let bytes = std::fs::read(&path).map_err(|_| "the file can't be read".to_string())?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        let title = name
            .rsplit_once('.')
            .map(|(a, _)| a.to_string())
            .unwrap_or_else(|| name.clone());
        live()
            .tenant(&business_id)?
            .document_store(&title, &name, &bytes, "draft", "user", None, None)
            .map(Some)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_document_set_status(
    business_id: String,
    doc_id: String,
    status: String,
    expected_version: i64,
) -> Result<BusinessDocument, String> {
    blocking(move || {
        live().tenant(&business_id)?.document_set_status(
            &doc_id,
            &status,
            expected_version,
            "taisun",
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_knowledge_search(
    business_id: String,
    query: String,
) -> Result<Vec<BusinessKnowledgeHit>, String> {
    blocking(move || live().tenant(&business_id)?.knowledge_search(&query)).await
}

// ─── Connectors and sync ─────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_connectors(business_id: String) -> Result<Vec<BusinessConnector>, String> {
    blocking(move || live().tenant(&business_id)?.connectors(&MacKeychain)).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_connector_configure(
    business_id: String,
    connector_id: String,
    values: BTreeMap<String, String>,
) -> Result<BusinessConnector, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .connector_configure(&MacKeychain, &connector_id, &values)
    })
    .await
}

/// The native folder picker for the local repository connector. None = cancelled.
#[tauri::command]
#[specta::specta]
pub async fn business_connector_pick_folder(
    app: tauri::AppHandle,
    business_id: String,
    connector_id: String,
) -> Result<Option<BusinessConnector>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    live().tenant(&business_id)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut d = app
            .dialog()
            .file()
            .set_title("Choose the website's source folder");
        if let Some(w) = app.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        d.blocking_pick_folder()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    blocking(move || {
        let mut v = BTreeMap::new();
        v.insert("path".to_string(), path.to_string_lossy().into_owned());
        live()
            .tenant(&business_id)?
            .connector_configure(&MacKeychain, &connector_id, &v)
            .map(Some)
    })
    .await
}

/// Write-only: the value goes straight to the Keychain. The reply is the connector's status only.
#[tauri::command]
#[specta::specta]
pub async fn business_connector_secret_set(
    business_id: String,
    connector_id: String,
    field: String,
    value: String,
) -> Result<BusinessConnector, String> {
    blocking(move || {
        live().tenant(&business_id)?.connector_secret_set(
            &MacKeychain,
            &connector_id,
            &field,
            value,
        )
    })
    .await
}

/// Disconnect after a native confirmation: settings cleared and Keychain items removed.
#[tauri::command]
#[specta::specta]
pub async fn business_connector_disconnect(
    app: tauri::AppHandle,
    business_id: String,
    connector_id: String,
) -> Result<BusinessConnector, String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let label = {
        let id = business_id.clone();
        let cid = connector_id.clone();
        blocking(move || {
            live()
                .tenant(&id)?
                .connector_view(&MacKeychain, &cid)
                .map(|v| v.label)
        })
        .await?
    };
    let ok = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(format!("Disconnect {label}? Its settings are cleared and its keys are removed from the Keychain. Synced material stays."))
            .title("Amara OS — Disconnect")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Disconnect".into(), "Cancel".into()))
            .blocking_show()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    if !ok {
        return Err(crate::commands::approval_gate::denied_message());
    }
    blocking(move || {
        live()
            .tenant(&business_id)?
            .connector_disconnect(&MacKeychain, &connector_id)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_sync_start(
    business_id: String,
    connector_id: String,
) -> Result<BusinessSyncRun, String> {
    blocking(move || {
        let t = live().tenant(&business_id)?;
        sync_in(
            &t,
            &MacKeychain,
            &super::net::Live,
            super::net::real_sleep,
            &connector_id,
            "user",
        )
    })
    .await
}

/// One explicit sync of one connector, recorded as a run. Network connectors go through `net.rs`
/// (approved hosts, the outbound switch, retries) or the pinned Supabase connection.
pub(crate) fn sync_in(
    t: &super::Tenant,
    kc: &dyn Keychain,
    http: &dyn super::net::Transport,
    sleep: super::net::Sleep,
    connector_id: &str,
    actor: &str,
) -> Result<BusinessSyncRun, String> {
    let (kind, _) = t.config_of(connector_id)?;
    let def = super::connectors::definition(&kind)?;
    if !def.syncs {
        return Err(format!(
            "{} has no synchronization in this phase",
            def.label
        ));
    }
    match kind.as_str() {
        "local_repo" => super::sync_local::run(t, connector_id, actor),
        "supabase" => super::supabase::sync_live(t, kc, actor),
        _ => {
            t.require(actor, "sync_connectors")?;
            let run_id = t.begin_run(connector_id)?;
            let outcome = match kind.as_str() {
                "website" => super::website::sync_website(t, http, sleep, &run_id),
                "github" => super::website::sync_github(t, kc, http, sleep, &run_id),
                "drive_folder" => super::drive_folder::sync(t, &run_id),
                _ => Err("no sync for this connector".to_string()),
            };
            t.finish_run(&run_id, def.label, outcome, actor)
        }
    }
}

/// Check connection: local folders are checked on every read; network connectors here, on request.
pub(crate) fn check_in(
    t: &super::Tenant,
    kc: &dyn Keychain,
    http: &dyn super::net::Transport,
    sleep: super::net::Sleep,
    connector_id: &str,
) -> Result<BusinessConnector, String> {
    let (kind, _) = t.config_of(connector_id)?;
    match kind.as_str() {
        "supabase" => {
            super::supabase::check_live(t, kc)?;
        }
        "website" => {
            super::website::check_website(t, http, sleep)?;
        }
        "github" => {
            super::website::check_github(t, kc, http, sleep)?;
        }
        _ => {}
    }
    t.connector_view(kc, connector_id)
}

#[tauri::command]
#[specta::specta]
pub async fn business_connector_check(
    business_id: String,
    connector_id: String,
) -> Result<BusinessConnector, String> {
    blocking(move || {
        let t = live().tenant(&business_id)?;
        check_in(
            &t,
            &MacKeychain,
            &super::net::Live,
            super::net::real_sleep,
            &connector_id,
        )
    })
    .await
}

/// The native file picker for a connector's file setting (the Supabase CA certificate). None = cancelled.
#[tauri::command]
#[specta::specta]
pub async fn business_connector_pick_file(
    app: tauri::AppHandle,
    business_id: String,
    connector_id: String,
    field: String,
) -> Result<Option<BusinessConnector>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    if field != "ca_cert" {
        return Err("that setting isn't a file".into());
    }
    live().tenant(&business_id)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut d = app
            .dialog()
            .file()
            .set_title("Choose the Supabase CA certificate")
            .add_filter("Certificate", &["crt", "pem", "cer"]);
        if let Some(w) = app.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        d.blocking_pick_file()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    blocking(move || {
        let mut v = BTreeMap::new();
        v.insert(field, path.to_string_lossy().into_owned());
        live()
            .tenant(&business_id)?
            .connector_configure(&MacKeychain, &connector_id, &v)
            .map(Some)
    })
    .await
}

/// The SQL that creates the read-only Supabase role (no secret in it; the password is a placeholder).
#[tauri::command]
#[specta::specta]
pub fn business_supabase_setup_sql() -> String {
    super::supabase::setup_sql()
}

#[tauri::command]
#[specta::specta]
pub async fn business_report(
    business_id: String,
    report: String,
) -> Result<super::intelligence::BusinessReport, String> {
    blocking(move || {
        let t = live().tenant(&business_id)?;
        match report.as_str() {
            "website" => super::intelligence::website_report(&t),
            "customers" => super::intelligence::customer_report(&t),
            _ => Err("report must be website or customers".into()),
        }
    })
    .await
}

/// Amara's notes on a report, on the local model only (never the cloud tier). Needs the model's
/// read permission for this business. Returns the report with the new notes.
#[tauri::command]
#[specta::specta]
pub async fn business_report_interpret(
    business_id: String,
    report: String,
) -> Result<super::intelligence::BusinessReport, String> {
    use crate::commands::inference::{
        infer, InferenceRequest, Message, ModelClass, OutputSpec, ProviderPolicy,
    };
    let (r, input) = {
        let id = business_id.clone();
        let rep = report.clone();
        blocking(move || {
            let t = live().tenant(&id)?;
            t.require("model", "read_approved_context")?;
            let r = match rep.as_str() {
                "website" => super::intelligence::website_report(&t)?,
                "customers" => super::intelligence::customer_report(&t)?,
                _ => return Err("report must be website or customers".into()),
            };
            let input = super::intelligence::interpretation_input(&r);
            Ok((r, input))
        })
        .await?
    };
    if input.as_array().is_none_or(|a| a.is_empty()) {
        return Err(
            "There is nothing measured yet for Amara to interpret. Sync the connectors first."
                .into(),
        );
    }
    let mut req = InferenceRequest::new("business.report_interpret", ModelClass::Fast);
    req.policy = ProviderPolicy::LocalOnly;
    req.persona = false;
    req.instruction = Some(super::intelligence::INSTRUCTION.into());
    req.messages = vec![Message::user(format!("Report ({report}):\n{input}"))];
    req.output = OutputSpec::Json {
        name: "report_notes".into(),
        schema: super::intelligence::notes_schema(),
    };
    req.params.temperature = Some(0.2);
    let out = infer(&req).await.map_err(|e| {
        format!(
            "Amara couldn't interpret the report ({}). Nothing was saved.",
            e.kind()
        )
    })?;
    let answer = out
        .json
        .ok_or("Amara's answer wasn't structured, so nothing was saved.")?;
    let tier = format!("{:?}", out.tier).to_lowercase();
    blocking(move || {
        let t = live().tenant(&business_id)?;
        super::intelligence::store_notes(&t, &r, &answer, &tier)?;
        match report.as_str() {
            "website" => super::intelligence::website_report(&t),
            _ => super::intelligence::customer_report(&t),
        }
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_sync_runs(
    business_id: String,
    connector_id: Option<String>,
) -> Result<Vec<BusinessSyncRun>, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .sync_runs(connector_id.as_deref(), 50)
    })
    .await
}

// ─── Permissions and approvals ───────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_permissions(business_id: String) -> Result<Vec<BusinessPermission>, String> {
    blocking(move || live().tenant(&business_id)?.permissions()).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_permission_set(
    business_id: String,
    actor: String,
    capability: String,
    allowed: bool,
) -> Result<Vec<BusinessPermission>, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .set_permission(&actor, &capability, allowed)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_proposals(
    business_id: String,
    status: Option<String>,
) -> Result<Vec<BusinessProposal>, String> {
    blocking(move || live().tenant(&business_id)?.proposals(status.as_deref())).await
}

/// Amara's way to propose a change. Inert until the user approves it; needs `propose_changes`.
#[tauri::command]
#[specta::specta]
pub async fn business_proposal_create(
    business_id: String,
    item_id: String,
    title: String,
    body: String,
    rationale: Option<String>,
) -> Result<BusinessProposal, String> {
    blocking(move || {
        live().tenant(&business_id)?.proposal_create(
            &item_id,
            &title,
            &body,
            rationale.as_deref(),
            "amara",
        )
    })
    .await
}

/// Shows the full proposed text in the native approval dialog; on Allow, applies it through the
/// execution controller.
#[tauri::command]
#[specta::specta]
pub async fn business_proposal_approve(
    app: tauri::AppHandle,
    business_id: String,
    proposal_id: String,
) -> Result<BusinessProposal, String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    blocking(move || {
        let data = crate::app_paths::data_dir();
        let text = proposal_dialog_text(&data, &business_id, &proposal_id)?;
        let _guard = crate::commands::approval_gate::InFlightGuard::acquire_all(&[format!(
            "biz-proposal:{business_id}:{proposal_id}"
        )])?;
        let allowed = app
            .dialog()
            .message(text)
            .title(crate::commands::approval_gate::APPROVAL_DIALOG_TITLE)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Allow".into(),
                "Deny".into(),
            ))
            .blocking_show();
        if !allowed {
            return Err(crate::commands::approval_gate::denied_message());
        }
        approve_proposal_in(
            &data,
            &crate::commands::execution::live_controller(),
            &business_id,
            &proposal_id,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_proposal_reject(
    business_id: String,
    proposal_id: String,
) -> Result<BusinessProposal, String> {
    blocking(move || {
        live()
            .tenant(&business_id)?
            .proposal_decide(&proposal_id, "rejected", None, None)
    })
    .await
}

// ─── History ─────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_activity(
    business_id: String,
    limit: Option<u32>,
) -> Result<Vec<ActivityRow>, String> {
    blocking(move || {
        let store = live();
        let t = store.tenant(&business_id)?;
        store_for(&store.data_dir(), Scope::Business)
            .list_business_activity(t.id().as_str(), limit.unwrap_or(100))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_recommendations(
    business_id: String,
) -> Result<Vec<RecommendationRow>, String> {
    blocking(move || {
        let store = live();
        let t = store.tenant(&business_id)?;
        store_for(&store.data_dir(), Scope::Business)
            .list_business_recommendations(t.id().as_str(), 100)
    })
    .await
}

// ─── Website operations (0065) ───────────────────────────────────────────────

use super::site_ops::{
    BusinessSiteChange, BusinessSiteChangeInput, BusinessSiteEvent, BusinessSiteMatch,
    BusinessSiteSnapshot,
};

#[tauri::command]
#[specta::specta]
pub async fn business_site_snapshot(business_id: String) -> Result<BusinessSiteSnapshot, String> {
    blocking(move || {
        super::site_ops::snapshot_live(
            &live().tenant(&business_id)?,
            &super::net::Live,
            super::net::real_sleep,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_locate(
    business_id: String,
    query: String,
) -> Result<Vec<BusinessSiteMatch>, String> {
    blocking(move || super::site_ops::locate(&live().tenant(&business_id)?, &query)).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_draft(
    business_id: String,
    input: BusinessSiteChangeInput,
) -> Result<BusinessSiteChange, String> {
    blocking(move || super::site_ops::draft(&live().tenant(&business_id)?, &input, "taisun")).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_changes(business_id: String) -> Result<Vec<BusinessSiteChange>, String> {
    blocking(move || live().tenant(&business_id)?.site_changes()).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_events(
    business_id: String,
    change_id: String,
) -> Result<Vec<BusinessSiteEvent>, String> {
    blocking(move || live().tenant(&business_id)?.site_events(&change_id)).await
}

/// Stage 8: the native dialog shows the target, every flag and the diff; Allow approves exactly that.
#[tauri::command]
#[specta::specta]
pub async fn business_site_approve(
    app: tauri::AppHandle,
    business_id: String,
    change_id: String,
) -> Result<BusinessSiteChange, String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    blocking(move || {
        let t = live().tenant(&business_id)?;
        let text = super::site_ops::approval_text(&t.site_change(&change_id)?)?;
        let _guard = crate::commands::approval_gate::InFlightGuard::acquire_all(&[format!(
            "site-change:{business_id}:{change_id}"
        )])?;
        let allowed = app
            .dialog()
            .message(text.clone())
            .title(crate::commands::approval_gate::APPROVAL_DIALOG_TITLE)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Allow".into(),
                "Deny".into(),
            ))
            .blocking_show();
        if !allowed {
            return Err(crate::commands::approval_gate::denied_message());
        }
        super::site_ops::approve(&t, &change_id, &text)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_reject(
    business_id: String,
    change_id: String,
) -> Result<BusinessSiteChange, String> {
    blocking(move || super::site_ops::reject(&live().tenant(&business_id)?, &change_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_confirm_backup(
    business_id: String,
    change_id: String,
) -> Result<BusinessSiteChange, String> {
    blocking(move || {
        super::site_ops::confirm_hpanel_backup(&live().tenant(&business_id)?, &change_id)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_package(
    business_id: String,
    change_id: String,
) -> Result<BusinessSiteChange, String> {
    blocking(move || {
        super::site_ops::package(
            &live().tenant(&business_id)?,
            &change_id,
            &super::net::Live,
            super::net::real_sleep,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn business_site_verify(
    business_id: String,
    change_id: String,
    rollback: bool,
) -> Result<BusinessSiteChange, String> {
    blocking(move || {
        super::site_ops::verify(
            &live().tenant(&business_id)?,
            &change_id,
            &super::net::Live,
            super::net::real_sleep,
            rollback,
        )
    })
    .await
}
