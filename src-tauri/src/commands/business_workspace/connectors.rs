//! The integration registry, connector configuration and connector status (decision 0063).
//!
//! Rules (each has a test):
//! - A connector is never shown as `connected` unless it was checked and the check passed. Phase 11
//!   has no network, so only the local repository can be checked; a remote connector with its key
//!   stored is `auth_required` ("not verified"), never `connected`.
//! - Secrets go only to the macOS Keychain (`com.taisun.nexus.biz.<business>.<connector>.<field>`).
//!   No command returns a secret; the view says `stored: true|false` only. Config (non-secret) lives
//!   in `workspaces.db`.
//! - No connector writes outside the app (`externalWrites` is always false), and none reads the network.

use super::{err, now, BusinessId, Tenant};
use crate::commands::keychain::{Keychain, KeychainError, SERVICE_PREFIX};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

pub(crate) struct Field {
    pub name: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
}

pub(crate) struct ConnectorDef {
    pub kind: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub config: &'static [Field],
    pub secrets: &'static [Field],
    /// Checked on this Mac without the network (the local repository only).
    pub local: bool,
}

pub(crate) const DEFINITIONS: &[ConnectorDef] = &[
    ConnectorDef {
        kind: "supabase",
        label: "Supabase",
        description: "The website's backend: orders, customers and quiz data. Read-only when it arrives.",
        config: &[Field { name: "project_url", label: "Project URL", hint: "https://<project>.supabase.co" }],
        secrets: &[Field { name: "service_role_key", label: "Service-role key", hint: "Kept in the Keychain; never shown again" }],
        local: false,
    },
    ConnectorDef {
        kind: "hostinger",
        label: "Hostinger",
        description: "Where the website is hosted. Status only; publishing is not part of this phase.",
        config: &[Field { name: "domain", label: "Domain", hint: "example.com" }],
        secrets: &[Field { name: "api_token", label: "API token", hint: "Kept in the Keychain; never shown again" }],
        local: false,
    },
    ConnectorDef {
        kind: "instagram",
        label: "Instagram",
        description: "The brand's Instagram account. Read-only when it arrives.",
        config: &[Field { name: "handle", label: "Account", hint: "@handle" }],
        secrets: &[Field { name: "access_token", label: "Access token", hint: "Kept in the Keychain; never shown again" }],
        local: false,
    },
    ConnectorDef {
        kind: "local_repo",
        label: "Website source on this Mac",
        description: "A read-only look at the website's source folder: pages, products, ingredients and docs.",
        config: &[Field { name: "path", label: "Folder", hint: "Choose the site's folder" }],
        secrets: &[],
        local: true,
    },
];

pub(crate) fn definition(kind: &str) -> Result<&'static ConnectorDef, String> {
    DEFINITIONS
        .iter()
        .find(|d| d.kind == kind)
        .ok_or_else(|| format!("connector: {kind} is not registered"))
}

// ─── Views ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessConnectorSetting {
    pub name: String,
    pub label: String,
    pub hint: String,
    pub value: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessConnectorSecret {
    pub name: String,
    pub label: String,
    pub hint: String,
    /// Whether the Keychain holds a value. The value itself never leaves Rust.
    pub stored: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSyncRun {
    pub run_id: String,
    pub connector_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// running | succeeded | failed | interrupted.
    pub status: String,
    pub items_seen: i64,
    pub items_changed: i64,
    /// Approved items the source now differs from (never changed by a sync).
    pub conflicts: i64,
    pub detail: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSyncState {
    /// unavailable | never | running | succeeded | failed | interrupted.
    pub state: String,
    pub detail: String,
    pub last: Option<BusinessSyncRun>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessConnector {
    pub connector_id: String,
    pub kind: String,
    pub label: String,
    pub description: String,
    pub config: Vec<BusinessConnectorSetting>,
    pub secrets: Vec<BusinessConnectorSecret>,
    /// disconnected | auth_required | connected | error.
    pub state: String,
    /// One plain sentence: why it is in that state and what to do next.
    pub detail: String,
    /// When the state above was worked out (now: it is checked on every read).
    pub checked_at: String,
    pub sync: BusinessSyncState,
    /// Always false in this phase: no connector writes outside the app.
    pub external_writes: bool,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Pure: the state and its sentence from what is known. `secrets`: Err = the Keychain couldn't be read.
pub(crate) fn derive_state(
    def: &ConnectorDef,
    config_complete: bool,
    secrets: &Result<Vec<bool>, String>,
    local_check: Option<&Result<String, String>>,
) -> (&'static str, String) {
    if let Err(e) = secrets {
        return (
            "error",
            format!("Couldn't read the Keychain ({e}). Nothing was changed."),
        );
    }
    if !config_complete {
        return ("disconnected", "Not set up.".into());
    }
    if def.local {
        return match local_check {
            Some(Ok(note)) => ("connected", note.clone()),
            Some(Err(e)) => ("error", e.clone()),
            None => ("disconnected", "Not checked yet.".into()),
        };
    }
    let stored = secrets
        .as_ref()
        .map(|v| v.iter().all(|s| *s))
        .unwrap_or(false);
    if !stored {
        let missing = def
            .secrets
            .iter()
            .zip(secrets.as_ref().map(|v| v.as_slice()).unwrap_or(&[]))
            .find(|(_, s)| !**s)
            .map(|(f, _)| f.label)
            .unwrap_or("key");
        return (
            "auth_required",
            format!(
                "Add the {} to finish setting this up.",
                missing.to_lowercase()
            ),
        );
    }
    (
        "auth_required",
        "Key stored in the Keychain but not verified: checking it needs the network, which arrives with read-only integrations (Phase 12). Not shown as connected until then.".into(),
    )
}

pub(crate) fn service_name(b: &BusinessId, connector_id: &str, field: &str) -> String {
    format!("{SERVICE_PREFIX}biz.{}.{connector_id}.{field}", b.as_str())
}

/// Validates one config value. Returns the trimmed value.
pub(crate) fn validate_config(kind: &str, field: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.len() > 500 || v.chars().any(char::is_control) {
        return Err(format!("{field}: not a valid value"));
    }
    let ok = match (kind, field) {
        ("supabase", "project_url") => v
            .strip_prefix("https://")
            .and_then(|h| h.strip_suffix(".supabase.co"))
            .is_some_and(|sub| {
                (3..=40).contains(&sub.len()) && sub.bytes().all(|b| b.is_ascii_alphanumeric())
            }),
        ("hostinger", "domain") => {
            (3..=253).contains(&v.len())
                && v.contains('.')
                && !v.starts_with('.')
                && !v.ends_with('.')
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        }
        ("instagram", "handle") => {
            let h = v.strip_prefix('@').unwrap_or(v);
            (1..=30).contains(&h.len())
                && h.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
        }
        ("local_repo", "path") => local_path_ok(Path::new(v)).is_ok(),
        _ => return Err(format!("{kind} has no setting {field}")),
    };
    if !ok {
        return Err(match (kind, field) {
            ("supabase", _) => "Project URL must look like https://<project>.supabase.co".into(),
            ("hostinger", _) => "Domain must look like example.com".into(),
            ("instagram", _) => "Account must look like @handle".into(),
            _ => "Folder must be an absolute folder inside your home folder".into(),
        });
    }
    Ok(match (kind, field) {
        ("instagram", "handle") => format!("@{}", v.trim_start_matches('@')),
        _ => v.to_string(),
    })
}

#[cfg(test)]
thread_local! {
    /// Tests point "home" at their temp folder (tests never use the real home folder).
    pub(crate) static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(h) = TEST_HOME.with(|h| h.borrow().clone()) {
        return Some(h);
    }
    dirs::home_dir()
}

/// An absolute path under the home folder, with no `..`.
pub(crate) fn local_path_ok(p: &Path) -> Result<PathBuf, String> {
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Folder must be an absolute path with no '..'".into());
    }
    let home = home_dir().ok_or("no home folder")?;
    if !p.starts_with(&home) || p == home {
        return Err("Folder must be inside your home folder".into());
    }
    Ok(p.to_path_buf())
}

/// The local repository check: a real folder (not a symlink) that can be read and looks like a site.
pub(crate) fn check_local_repo(path: &str) -> Result<String, String> {
    let p = local_path_ok(Path::new(path))?;
    let meta = std::fs::symlink_metadata(&p)
        .map_err(|_| "The folder isn't there any more. Choose it again.".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("The folder is a symbolic link; choose the real folder.".into());
    }
    if !meta.is_dir() {
        return Err("That path is a file, not a folder.".into());
    }
    std::fs::read_dir(&p).map_err(|_| "The folder can't be read.".to_string())?;
    if !p.join("package.json").is_file() {
        return Err(
            "The folder has no package.json, so it doesn't look like the site's source.".into(),
        );
    }
    Ok("Folder found and readable on this Mac (checked just now, read-only).".into())
}

// ─── Tenant methods ──────────────────────────────────────────────────────────

impl Tenant {
    fn config_of(&self, connector_id: &str) -> Result<(String, BTreeMap<String, String>), String> {
        let row: Option<(String, String)> = self
            .conn()
            .query_row(
                "SELECT kind, config_json FROM connector WHERE business_id = ?1 AND connector_id = ?2",
                params![self.id().as_str(), connector_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        let (kind, json) = row.ok_or("no such connector in this business")?;
        let config: BTreeMap<String, String> = serde_json::from_str(&json).unwrap_or_default();
        Ok((kind, config))
    }

    pub(crate) fn connector_ids(&self) -> Result<Vec<String>, String> {
        let mut st = self
            .conn()
            .prepare(
                "SELECT connector_id FROM connector WHERE business_id = ?1 ORDER BY connector_id",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![self.id().as_str()], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(err)?;
        // Registry order, so the UI is stable.
        let mut ordered: Vec<String> = DEFINITIONS
            .iter()
            .map(|d| d.kind.to_string())
            .filter(|k| rows.contains(k))
            .collect();
        ordered.extend(
            rows.into_iter()
                .filter(|r| !DEFINITIONS.iter().any(|d| d.kind == r)),
        );
        Ok(ordered)
    }

    pub(crate) fn connector_view(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
    ) -> Result<BusinessConnector, String> {
        let (kind, config) = self.config_of(connector_id)?;
        let def = definition(&kind)?;
        let complete = def
            .config
            .iter()
            .all(|f| config.get(f.name).is_some_and(|v| !v.is_empty()));
        let secrets: Result<Vec<bool>, String> = def
            .secrets
            .iter()
            .map(
                |f| match kc.get(&service_name(self.id(), connector_id, f.name)) {
                    Ok(mut v) => {
                        v.zeroize();
                        Ok(true)
                    }
                    Err(KeychainError::NotFound) => Ok(false),
                    Err(KeychainError::Other(e)) => Err(e),
                },
            )
            .collect();
        let local = if def.local && complete {
            Some(check_local_repo(
                config.get("path").map(String::as_str).unwrap_or(""),
            ))
        } else {
            None
        };
        let (state, detail) = derive_state(def, complete, &secrets, local.as_ref());
        let stored = secrets.clone().unwrap_or_default();
        let last = self.sync_runs(Some(connector_id), 1)?.into_iter().next();
        let sync = if !def.local {
            BusinessSyncState {
                state: "unavailable".into(),
                detail: "Sync arrives with read-only integrations (Phase 12).".into(),
                last,
            }
        } else {
            match &last {
                None => BusinessSyncState {
                    state: "never".into(),
                    detail: "Not synchronized yet.".into(),
                    last: None,
                },
                Some(r) => BusinessSyncState {
                    state: r.status.clone(),
                    detail: r.detail.clone().unwrap_or_default(),
                    last: last.clone(),
                },
            }
        };
        Ok(BusinessConnector {
            connector_id: connector_id.to_string(),
            kind: kind.clone(),
            label: def.label.into(),
            description: def.description.into(),
            config: def
                .config
                .iter()
                .map(|f| BusinessConnectorSetting {
                    name: f.name.into(),
                    label: f.label.into(),
                    hint: f.hint.into(),
                    value: config.get(f.name).cloned(),
                })
                .collect(),
            secrets: def
                .secrets
                .iter()
                .enumerate()
                .map(|(i, f)| BusinessConnectorSecret {
                    name: f.name.into(),
                    label: f.label.into(),
                    hint: f.hint.into(),
                    stored: stored.get(i).copied().unwrap_or(false),
                })
                .collect(),
            state: state.into(),
            detail,
            checked_at: now(),
            sync,
            external_writes: false,
        })
    }

    pub(crate) fn connectors(&self, kc: &dyn Keychain) -> Result<Vec<BusinessConnector>, String> {
        self.connector_ids()?
            .iter()
            .map(|id| self.connector_view(kc, id))
            .collect()
    }

    /// Sets non-secret settings. Unknown fields are refused; an empty value clears the field.
    pub(crate) fn connector_configure(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<BusinessConnector, String> {
        let (kind, mut config) = self.config_of(connector_id)?;
        let def = definition(&kind)?;
        for (k, v) in values {
            if !def.config.iter().any(|f| f.name == k) {
                return Err(format!("{} has no setting {k}", def.label));
            }
            if v.trim().is_empty() {
                config.remove(k);
            } else {
                config.insert(k.clone(), validate_config(&kind, k, v)?);
            }
        }
        self.conn()
            .execute(
                "UPDATE connector SET config_json = ?3, updated_at = ?4 WHERE business_id = ?1 AND connector_id = ?2",
                params![self.id().as_str(), connector_id, serde_json::to_string(&config).map_err(err)?, now()],
            )
            .map_err(err)?;
        super::note_activity(
            &self.data_path(),
            "business.connector_changed",
            &super::biz_ref(self.id(), "connector", Some(connector_id)),
            "taisun",
            &format!("{}: settings changed", def.label),
        );
        self.connector_view(kc, connector_id)
    }

    /// Write-only: the value goes to the Keychain and is zeroized. Nothing about it is returned.
    pub(crate) fn connector_secret_set(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
        field: &str,
        mut value: String,
    ) -> Result<BusinessConnector, String> {
        let result = (|| {
            let (kind, _) = self.config_of(connector_id)?;
            let def = definition(&kind)?;
            if !def.secrets.iter().any(|f| f.name == field) {
                return Err(format!("{} has no secret {field}", def.label));
            }
            let v = value.trim();
            if v.is_empty() {
                return Err("value is empty".to_string());
            }
            if v.len() > 8192 || !v.bytes().all(|b| b.is_ascii_graphic()) {
                return Err(
                    "that doesn't look like a key (printable characters only, under 8 KB)"
                        .to_string(),
                );
            }
            kc.set(&service_name(self.id(), connector_id, field), v.as_bytes())
                .map_err(|e| match e {
                    KeychainError::NotFound => "keychain item missing".to_string(),
                    KeychainError::Other(m) => m,
                })?;
            super::note_activity(
                &self.data_path(),
                "business.connector_changed",
                &super::biz_ref(self.id(), "connector", Some(connector_id)),
                "taisun",
                &format!("{}: key stored in the Keychain", def.label),
            );
            Ok(())
        })();
        value.zeroize();
        result?;
        self.connector_view(kc, connector_id)
    }

    /// Disconnect: clears the settings and removes this connector's Keychain items. Sync history stays.
    pub(crate) fn connector_disconnect(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
    ) -> Result<BusinessConnector, String> {
        let (kind, _) = self.config_of(connector_id)?;
        let def = definition(&kind)?;
        for f in def.secrets {
            match kc.delete(&service_name(self.id(), connector_id, f.name)) {
                Ok(()) | Err(KeychainError::NotFound) => {}
                Err(KeychainError::Other(e)) => {
                    return Err(format!(
                    "Couldn't remove the key from the Keychain ({e}); nothing else was changed."
                ))
                }
            }
        }
        self.conn()
            .execute(
                "UPDATE connector SET config_json = '{}', updated_at = ?3 WHERE business_id = ?1 AND connector_id = ?2",
                params![self.id().as_str(), connector_id, now()],
            )
            .map_err(err)?;
        super::note_activity(
            &self.data_path(),
            "business.connector_changed",
            &super::biz_ref(self.id(), "connector", Some(connector_id)),
            "taisun",
            &format!("{}: disconnected", def.label),
        );
        self.connector_view(kc, connector_id)
    }

    pub(crate) fn connector_config_value(
        &self,
        connector_id: &str,
        field: &str,
    ) -> Result<Option<String>, String> {
        Ok(self.config_of(connector_id)?.1.get(field).cloned())
    }

    pub(crate) fn sync_runs(
        &self,
        connector_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<BusinessSyncRun>, String> {
        let mut st = self
            .conn()
            .prepare(
                "SELECT run_id, connector_id, started_at, finished_at, status, items_seen, items_changed, conflicts, detail
                 FROM sync_run WHERE business_id = ?1 AND (?2 IS NULL OR connector_id = ?2)
                 ORDER BY started_at DESC LIMIT ?3",
            )
            .map_err(err)?;
        let rows = st
            .query_map(
                params![self.id().as_str(), connector_id, limit.min(200)],
                |r| {
                    Ok(BusinessSyncRun {
                        run_id: r.get(0)?,
                        connector_id: r.get(1)?,
                        started_at: r.get(2)?,
                        finished_at: r.get(3)?,
                        status: r.get(4)?,
                        items_seen: r.get(5)?,
                        items_changed: r.get(6)?,
                        conflicts: r.get(7)?,
                        detail: r.get(8)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }
}
