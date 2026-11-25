//! Skill lifecycle (decision 0052): PROPOSED → VALIDATED → STORED → READ_BACK_VERIFIED → ACTIVE,
//! with REJECTED and RETIRED, version history and restore.
//!
//! Storage (all under `<data>/workspace/`):
//! - `skills/<stem>.md`: the live skills `read_skills` loads (unchanged, the authoritative place).
//! - `skill_versions/<stem>/v<N>.md`: every version ever stored, never overwritten.
//! - `skill_versions/manifest.json`: per skill, each version's state history and the active one.
//!
//! Writes go through the execution controller with independent read-back: storing a version is
//! VERIFIED only when the file read back from disk has the expected SHA-256, and activating is
//! VERIFIED only when `read_skills`' own precedence resolves the name to the expected content.
//! Generated skills are never activated automatically: there is no configured permission for it,
//! so activation needs the user (a native approval dialog).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::execution::{
    ActionClass, Controller, DataScope, ExecRequest, ExecStatus, OpError, Operation, OperationSpec,
    Origin, RetryPolicy, Risk,
};

const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SkillState {
    Proposed,
    Validated,
    Stored,
    ReadBackVerified,
    Active,
    Rejected,
    /// Was active; replaced by another version.
    Retired,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StateEntry {
    pub state: SkillState,
    pub at: String,
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersion {
    pub version: u32,
    pub sha256: String,
    pub created_at: String,
    /// `generated` (skills evolution), `user`, or `existing` (a live file imported before replace).
    pub source: String,
    pub state: SkillState,
    pub history: Vec<StateEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub name: String,
    pub versions: Vec<SkillVersion>,
    pub active_version: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Manifest {
    skills: BTreeMap<String, SkillRecord>,
}

/// Where skills live. Live code uses [`SkillDirs::live`]; tests point it at a temp folder.
#[derive(Clone, Debug)]
pub(crate) struct SkillDirs {
    /// Earlier entries win by name (same order as `read_skills`).
    pub search_order: Vec<PathBuf>,
    /// Where activated skills are written (must be in `search_order`).
    pub learned: PathBuf,
    pub versions: PathBuf,
}

impl SkillDirs {
    pub(crate) fn live() -> Self {
        let learned = super::openclaw::learned_skills_dir();
        Self {
            search_order: super::openclaw::skill_search_order(),
            versions: learned
                .parent()
                .map(|p| p.join("skill_versions"))
                .unwrap_or_else(|| PathBuf::from("skill_versions")),
            learned,
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.versions.join("manifest.json")
    }

    fn version_path(&self, stem: &str, v: u32) -> Result<PathBuf, String> {
        let dir = super::path_safety::safe_join(&self.versions, stem)?;
        super::path_safety::safe_join(&dir, &format!("v{v}.md"))
    }

    fn live_path(&self, stem: &str) -> Result<PathBuf, String> {
        super::path_safety::safe_join(&self.learned, &format!("{stem}.md"))
    }

    /// The file `read_skills` would load for `stem`: the first folder in search order that has it.
    pub(crate) fn resolve(&self, stem: &str) -> Option<PathBuf> {
        self.search_order
            .iter()
            .map(|d| d.join(format!("{stem}.md")))
            .find(|p| p.is_file())
    }
}

pub(crate) fn stem_of(name: &str) -> Result<String, String> {
    let stem = name.trim().replace(' ', "-").to_lowercase();
    super::path_safety::validate_name(&stem)?;
    Ok(stem)
}

fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn load(dirs: &SkillDirs) -> Result<Manifest, String> {
    super::local_store::read_json(&dirs.manifest_path(), MAX_MANIFEST_BYTES)
        .map_err(|e| format!("skill manifest: {e}"))
}

fn save(dirs: &SkillDirs, m: &Manifest) -> Result<(), String> {
    super::local_store::write_json_atomic(&dirs.manifest_path(), m, MAX_MANIFEST_BYTES)
        .map_err(|e| format!("skill manifest: {e}"))?;
    // Read the manifest back: a "saved" manifest that doesn't parse is a failed save.
    let back = load(dirs)?;
    if back.skills.len() != m.skills.len() {
        return Err("skill manifest: read-back does not match what was written".into());
    }
    Ok(())
}

fn push_state(v: &mut SkillVersion, state: SkillState, note: Option<String>) {
    v.state = state;
    v.history.push(StateEntry {
        state,
        at: now(),
        note,
    });
}

/// Why a skill's content is refused (fixed labels).
pub(crate) fn validate_content(content: &str) -> Result<(), &'static str> {
    if content.trim().is_empty() {
        return Err("empty");
    }
    if content.len() > super::openclaw::MAX_SKILL_BYTES {
        return Err("too_large");
    }
    if !content.lines().any(|l| l.starts_with("# ")) {
        return Err("no_title");
    }
    let scan = super::terrence_runtime::guard_scan(content);
    if !scan.is_empty() {
        return Err("injection_pattern");
    }
    Ok(())
}

// ─── Operations (registered with the controller) ─────────────────────────────

const STORE_SPEC: OperationSpec = OperationSpec {
    action_id: "skill.store_version",
    tool_id: "skills.version_store",
    target_system: "skill_store",
    class: ActionClass::Staging,
    risk: Risk::Low,
    scope: DataScope::System,
    timeout: Duration::from_secs(10),
    retry: RetryPolicy::SafeIdempotent { max_attempts: 2 },
    personal_fields: &[],
};

const ACTIVATE_SPEC: OperationSpec = OperationSpec {
    action_id: "skill.activate",
    tool_id: "skills.live_store",
    target_system: "skill_store",
    // Changes what Amara is told on every prompt: needs the user unless the user started it.
    class: ActionClass::ReversibleLocal,
    risk: Risk::Medium,
    scope: DataScope::System,
    timeout: Duration::from_secs(10),
    retry: RetryPolicy::Never,
    personal_fields: &[],
};

struct StoreVersion(SkillDirs);

impl Operation for StoreVersion {
    fn spec(&self) -> &OperationSpec {
        &STORE_SPEC
    }
    fn validate(&self, p: &Value) -> Result<(), String> {
        let stem = p["stem"].as_str().ok_or("stem required")?;
        stem_of(stem)?;
        p["version"].as_u64().ok_or("version required")?;
        p["sha256"].as_str().ok_or("sha256 required")?;
        let content = p["content"].as_str().ok_or("content required")?;
        validate_content(content).map_err(|e| e.to_string())?;
        if sha256_hex(content.as_bytes()) != p["sha256"].as_str().unwrap_or("") {
            return Err("content does not match its sha256".into());
        }
        Ok(())
    }
    fn target_ref(&self, p: &Value) -> Option<String> {
        Some(format!(
            "{}@v{}",
            p["stem"].as_str()?,
            p["version"].as_u64()?
        ))
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        Some(json!({"sha256": p["sha256"]}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let fail = |m: String, submitted| OpError::Failed {
            message: m,
            submitted,
        };
        let stem = p["stem"].as_str().unwrap_or_default();
        let v = p["version"].as_u64().unwrap_or_default() as u32;
        let path = self.0.version_path(stem, v).map_err(|e| fail(e, false))?;
        let content = p["content"].as_str().unwrap_or_default();
        if path.exists() {
            // A version file is never overwritten. Same content = an earlier attempt landed.
            let existing = std::fs::read(&path).map_err(|e| fail(e.to_string(), false))?;
            if sha256_hex(&existing) == p["sha256"].as_str().unwrap_or("") {
                return Ok(json!({"path": format!("{stem}/v{v}.md"), "already_present": true}));
            }
            return Err(fail(
                "a different file already holds this version".into(),
                false,
            ));
        }
        super::local_store::write_text_atomic(
            &path,
            content,
            super::openclaw::MAX_SKILL_BYTES as u64,
        )
        .map_err(|e| fail(e, false))?;
        Ok(json!({"path": format!("{stem}/v{v}.md")}))
    }
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        let stem = p["stem"].as_str()?;
        let v = p["version"].as_u64()? as u32;
        Some(
            self.0
                .version_path(stem, v)
                .and_then(|path| std::fs::read(path).map_err(|e| e.to_string()))
                .map(|b| json!({"sha256": sha256_hex(&b)})),
        )
    }
}

struct Activate(SkillDirs);

impl Operation for Activate {
    fn spec(&self) -> &OperationSpec {
        &ACTIVATE_SPEC
    }
    fn validate(&self, p: &Value) -> Result<(), String> {
        let stem = stem_of(p["stem"].as_str().ok_or("stem required")?)?;
        p["version"].as_u64().ok_or("version required")?;
        p["sha256"].as_str().ok_or("sha256 required")?;
        // A project skill with the same name always wins in read_skills: activating would be a lie.
        let live = self.0.live_path(&stem)?;
        if let Some(winner) = self.0.resolve(&stem) {
            if winner != live && !winner.starts_with(&self.0.learned) {
                let earlier = self
                    .0
                    .search_order
                    .iter()
                    .position(|d| winner.starts_with(d))
                    < self
                        .0
                        .search_order
                        .iter()
                        .position(|d| d == &self.0.learned);
                if earlier {
                    return Err(format!(
                        "a project skill named {stem} takes precedence, so this version would never load"
                    ));
                }
            }
        }
        Ok(())
    }
    fn target_ref(&self, p: &Value) -> Option<String> {
        Some(format!(
            "{}@v{}",
            p["stem"].as_str()?,
            p["version"].as_u64()?
        ))
    }
    fn expected_state(&self, p: &Value) -> Option<Value> {
        Some(json!({"sha256": p["sha256"]}))
    }
    fn execute(&self, p: &Value) -> Result<Value, OpError> {
        let fail = |m: String| OpError::Failed {
            message: m,
            submitted: false,
        };
        let stem = p["stem"].as_str().unwrap_or_default();
        let v = p["version"].as_u64().unwrap_or_default() as u32;
        let src = self.0.version_path(stem, v).map_err(fail)?;
        let bytes = std::fs::read(&src).map_err(|e| fail(e.to_string()))?;
        if sha256_hex(&bytes) != p["sha256"].as_str().unwrap_or("") {
            return Err(fail(
                "the stored version no longer matches its checksum".into(),
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| fail("version is not text".into()))?;
        let live = self.0.live_path(stem).map_err(fail)?;
        super::local_store::write_text_atomic(
            &live,
            &text,
            super::openclaw::MAX_SKILL_BYTES as u64,
        )
        .map_err(|e| OpError::Failed {
            message: e,
            submitted: true,
        })?;
        Ok(json!({"active": format!("{stem}.md"), "version": v}))
    }
    /// Read back the way `read_skills` will: first match by name across the search order.
    fn read_back(&self, p: &Value) -> Option<Result<Value, String>> {
        let stem = p["stem"].as_str()?;
        Some(match self.0.resolve(stem) {
            None => Ok(json!({"sha256": null})),
            Some(path) => std::fs::read(path)
                .map(|b| json!({"sha256": sha256_hex(&b)}))
                .map_err(|e| e.to_string()),
        })
    }
}

pub(crate) fn operations() -> Vec<Arc<dyn Operation>> {
    operations_for(SkillDirs::live())
}

pub(crate) fn operations_for(dirs: SkillDirs) -> Vec<Arc<dyn Operation>> {
    vec![
        Arc::new(StoreVersion(dirs.clone())),
        Arc::new(Activate(dirs)),
    ]
}

// ─── Lifecycle functions ──────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillOutcome {
    pub name: String,
    pub version: u32,
    pub state: SkillState,
    /// The ledger execution that stored or activated it.
    pub execution_id: Option<String>,
    pub note: Option<String>,
}

/// Proposes a new version (generated or user-written). Ends READ_BACK_VERIFIED, or REJECTED with a
/// reason, or an error if storing could not be verified. Never activates.
pub(crate) fn propose(
    dirs: &SkillDirs,
    controller: &Controller,
    name: &str,
    content: &str,
    source: &str,
    origin: Origin,
) -> Result<SkillOutcome, String> {
    let stem = stem_of(name)?;
    let _g = MANIFEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut m = load(dirs)?;
    let rec = m.skills.entry(stem.clone()).or_insert_with(|| SkillRecord {
        name: stem.clone(),
        ..Default::default()
    });
    let version = rec.versions.iter().map(|v| v.version).max().unwrap_or(0) + 1;
    let sha = sha256_hex(content.as_bytes());
    let mut v = SkillVersion {
        version,
        sha256: sha.clone(),
        created_at: now(),
        source: source.to_string(),
        state: SkillState::Proposed,
        history: vec![],
    };
    push_state(&mut v, SkillState::Proposed, None);
    if let Err(why) = validate_content(content) {
        push_state(&mut v, SkillState::Rejected, Some(why.to_string()));
        rec.versions.push(v);
        save(dirs, &m)?;
        return Ok(SkillOutcome {
            name: stem,
            version,
            state: SkillState::Rejected,
            execution_id: None,
            note: Some(why.to_string()),
        });
    }
    push_state(&mut v, SkillState::Validated, None);
    let submitted = controller
        .submit(&ExecRequest {
            action_id: STORE_SPEC.action_id.to_string(),
            workflow_id: format!("skill:{stem}"),
            idempotency_key: format!("skill:{stem}:v{version}:{sha}"),
            origin,
            params: json!({"stem": stem, "version": version, "sha256": sha, "content": content}),
            approval_id: None,
        })
        .map_err(|e| e.to_string())?;
    let exec = submitted.record().clone();
    let (state, note) = match exec.status {
        ExecStatus::Verified => {
            push_state(&mut v, SkillState::Stored, None);
            push_state(
                &mut v,
                SkillState::ReadBackVerified,
                Some("sha256 read back from disk".into()),
            );
            (SkillState::ReadBackVerified, None)
        }
        other => {
            let why = format!("store not verified: {}", other.as_str());
            v.history.push(StateEntry {
                state: SkillState::Validated,
                at: now(),
                note: Some(why.clone()),
            });
            (SkillState::Validated, Some(why))
        }
    };
    let rec = m.skills.get_mut(&stem).expect("inserted above");
    rec.versions.push(v);
    save(dirs, &m)?;
    if state != SkillState::ReadBackVerified {
        return Err(format!(
            "Skill {stem} v{version} was not stored: {}",
            note.unwrap_or_default()
        ));
    }
    Ok(SkillOutcome {
        name: stem,
        version,
        state,
        execution_id: Some(exec.execution_id),
        note: None,
    })
}

/// Makes a verified version the live skill (also how an older version is restored). A live file
/// that predates the manifest is imported as a version first, so nothing existing is lost.
pub(crate) fn activate(
    dirs: &SkillDirs,
    controller: &Controller,
    name: &str,
    version: u32,
    origin: Origin,
    approval_id: Option<String>,
) -> Result<SkillOutcome, String> {
    let stem = stem_of(name)?;
    let _g = MANIFEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut m = load(dirs)?;
    let rec = m
        .skills
        .get_mut(&stem)
        .ok_or_else(|| format!("no stored versions of {stem}"))?;
    let target = rec
        .versions
        .iter()
        .find(|v| v.version == version)
        .ok_or_else(|| format!("{stem} has no version {version}"))?
        .clone();
    if !matches!(
        target.state,
        SkillState::ReadBackVerified | SkillState::Retired | SkillState::Active
    ) {
        return Err(format!(
            "{stem} v{version} is {:?}; only a read-back-verified version can be activated",
            target.state
        ));
    }
    // Preserve a live skill the manifest doesn't know yet.
    let live = dirs.live_path(&stem)?;
    if rec.active_version.is_none() && live.is_file() {
        let bytes = std::fs::read(&live).map_err(|e| e.to_string())?;
        let sha = sha256_hex(&bytes);
        if !rec.versions.iter().any(|v| v.sha256 == sha) {
            let n = rec.versions.iter().map(|v| v.version).max().unwrap_or(0) + 1;
            let path = dirs.version_path(&stem, n)?;
            super::local_store::write_text_atomic(
                &path,
                &String::from_utf8_lossy(&bytes),
                super::openclaw::MAX_SKILL_BYTES as u64,
            )?;
            if std::fs::read(&path).map(|b| sha256_hex(&b)).ok().as_deref() != Some(sha.as_str()) {
                return Err("could not preserve the existing live skill; nothing changed".into());
            }
            let mut v = SkillVersion {
                version: n,
                sha256: sha,
                created_at: now(),
                source: "existing".into(),
                state: SkillState::ReadBackVerified,
                history: vec![],
            };
            push_state(
                &mut v,
                SkillState::ReadBackVerified,
                Some("imported before replace".into()),
            );
            rec.versions.push(v);
            save(dirs, &m)?;
        }
    }
    let submitted = controller
        .submit(&ExecRequest {
            action_id: ACTIVATE_SPEC.action_id.to_string(),
            workflow_id: format!("skill:{stem}"),
            idempotency_key: format!("skill:{stem}:activate:v{version}:{}", uuid::Uuid::new_v4()),
            origin,
            params: json!({"stem": stem, "version": version, "sha256": target.sha256}),
            approval_id,
        })
        .map_err(|e| e.to_string())?;
    let exec = submitted.record().clone();
    if exec.status != ExecStatus::Verified {
        return Err(format!(
            "{stem} v{version} was not activated ({}): {}",
            exec.status.as_str(),
            exec.error.unwrap_or_default()
        ));
    }
    let rec = m.skills.get_mut(&stem).expect("present");
    let previous = rec.active_version;
    for v in rec.versions.iter_mut() {
        if Some(v.version) == previous && v.version != version {
            push_state(
                v,
                SkillState::Retired,
                Some(format!("replaced by v{version}")),
            );
        }
        if v.version == version {
            push_state(v, SkillState::Active, None);
        }
    }
    rec.active_version = Some(version);
    save(dirs, &m)?;
    Ok(SkillOutcome {
        name: stem,
        version,
        state: SkillState::Active,
        execution_id: Some(exec.execution_id),
        note: previous.map(|p| format!("v{p} retired")),
    })
}

pub(crate) fn reject(
    dirs: &SkillDirs,
    name: &str,
    version: u32,
    why: &str,
) -> Result<SkillOutcome, String> {
    let stem = stem_of(name)?;
    let _g = MANIFEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut m = load(dirs)?;
    let rec = m.skills.get_mut(&stem).ok_or("no such skill")?;
    if rec.active_version == Some(version) {
        return Err("the active version can't be rejected; activate another first".into());
    }
    let v = rec
        .versions
        .iter_mut()
        .find(|v| v.version == version)
        .ok_or("no such version")?;
    push_state(
        v,
        SkillState::Rejected,
        Some(why.chars().take(200).collect()),
    );
    save(dirs, &m)?;
    Ok(SkillOutcome {
        name: stem,
        version,
        state: SkillState::Rejected,
        execution_id: None,
        note: None,
    })
}

pub(crate) fn list(dirs: &SkillDirs) -> Result<Vec<SkillRecord>, String> {
    Ok(load(dirs)?.skills.into_values().collect())
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn list_skill_versions() -> Result<Vec<SkillRecord>, String> {
    list(&SkillDirs::live())
}

/// Skills evolution stores a generated version here. It is never activated by this call.
#[tauri::command]
#[specta::specta]
pub async fn propose_skill_version(name: String, content: String) -> Result<SkillOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        propose(
            &SkillDirs::live(),
            &super::execution::live_controller(),
            &name,
            &content,
            "generated",
            Origin::Model,
        )
    })
    .await
    .map_err(|e| format!("Skill task failed: {e}"))?
}

/// Activates (or restores) a version after the user allows it in the native approval dialog.
#[tauri::command]
#[specta::specta]
pub async fn activate_skill_version(
    app: tauri::AppHandle,
    name: String,
    version: u32,
) -> Result<SkillOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let stem = stem_of(&name)?;
        let key = format!("skill:{stem}");
        let _guard = super::approval_gate::InFlightGuard::acquire_all(&[key])?;
        let text = super::approval_gate::approval_text(
            "Make a skill active",
            &format!("{stem}, version {version}. Amara will use it on every reply."),
        )?;
        let allowed = app
            .dialog()
            .message(text)
            .title(super::approval_gate::APPROVAL_DIALOG_TITLE)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Allow".into(),
                "Deny".into(),
            ))
            .blocking_show();
        if !allowed {
            return Err(super::approval_gate::denied_message());
        }
        activate(
            &SkillDirs::live(),
            &super::execution::live_controller(),
            &stem,
            version,
            Origin::User,
            None,
        )
    })
    .await
    .map_err(|e| format!("Skill task failed: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub fn reject_skill_version(name: String, version: u32) -> Result<SkillOutcome, String> {
    reject(&SkillDirs::live(), &name, version, "rejected by the user")
}

#[cfg(test)]
mod tests;
