//! The integration registry, connector configuration and connector status (decisions 0063, 0064).
//!
//! Rules (each has a test):
//! - A connector is `connected` only when a real check passed: a local folder is checked on every
//!   read; a network connector by its last explicit check (stored with its time). Never by default.
//! - An authentication failure is `auth_required`; any other failed check is `error`; a connector that
//!   isn't set up, or is deferred, is `disconnected` with the reason.
//! - Secrets go only to the macOS Keychain (`com.taisun.nexus.biz.<business>.<connector>.<field>`).
//!   No command returns a secret; the view says `stored: true|false`. Non-secret settings live in
//!   `workspaces.db`.
//! - No connector writes outside the app (`externalWrites` is always false).

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// A folder on this Mac, checked on every read.
    Local,
    /// Checked over the network on request; the result is stored with its time.
    Network,
    /// Registered but not available in this phase, with the reason.
    Deferred(&'static str),
}

pub(crate) struct ConnectorDef {
    pub kind: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub config: &'static [Field],
    pub secrets: &'static [Field],
    pub check: Check,
    /// Has an explicit manual sync in this phase.
    pub syncs: bool,
}

const KEYCHAIN_HINT: &str = "Kept in the Keychain; never shown again";

pub(crate) const DEFINITIONS: &[ConnectorDef] = &[
    ConnectorDef {
        kind: "supabase",
        label: "Supabase (read-only database)",
        description: "The site's database, through a read-only role that can see only non-identifying columns. Website analytics are copied; customer information arrives as aggregates only.",
        config: &[
            Field { name: "pooler_host", label: "Connection pooler host", hint: "aws-1-ap-northeast-1.pooler.supabase.com" },
            Field { name: "project_ref", label: "Project reference", hint: "20 lowercase letters" },
            Field { name: "ca_cert", label: "Supabase CA certificate", hint: "Download it from Database settings › SSL configuration" },
        ],
        secrets: &[Field { name: "db_password", label: "Read-only role password", hint: KEYCHAIN_HINT }],
        check: Check::Network,
        syncs: true,
    },
    ConnectorDef {
        kind: "website",
        label: "Website (Hostinger)",
        description: "The public site: response headers, robots.txt and the sitemap. Hostinger deploys by manual upload, so there is no account to read.",
        config: &[Field { name: "domain", label: "Domain", hint: "example.com" }],
        secrets: &[],
        check: Check::Network,
        syncs: true,
    },
    ConnectorDef {
        kind: "github",
        label: "GitHub (site repository)",
        description: "Deployment history from the site's repository: branches and recent commits. Read-only token.",
        config: &[Field { name: "repo", label: "Repository", hint: "owner/name" }],
        secrets: &[Field { name: "token", label: "Read-only access token", hint: KEYCHAIN_HINT }],
        check: Check::Network,
        syncs: true,
    },
    ConnectorDef {
        kind: "google_analytics",
        label: "Google Analytics 4",
        description: "Traffic and conversions from GA4.",
        config: &[],
        secrets: &[],
        check: Check::Deferred("Deferred (decision 0064): the live site doesn't load the GA4 tag, so it is very likely collecting nothing, and Google access isn't set up. Website figures come from the site's own analytics instead."),
        syncs: false,
    },
    ConnectorDef {
        kind: "search_console",
        label: "Google Search Console",
        description: "Search queries, impressions, clicks, positions and indexed pages.",
        config: &[],
        secrets: &[],
        check: Check::Deferred("Deferred (decision 0064): needs Google access, which isn't set up. Search figures are shown as unavailable until then."),
        syncs: false,
    },
    ConnectorDef {
        kind: "instagram",
        label: "Instagram",
        description: "The brand's Instagram account.",
        config: &[],
        secrets: &[],
        check: Check::Deferred("Not part of Phase 12."),
        syncs: false,
    },
    ConnectorDef {
        kind: "local_repo",
        label: "Website source on this Mac",
        description: "A read-only look at the website's source folder: pages, products, ingredients and docs.",
        config: &[Field { name: "path", label: "Folder", hint: "Choose the site's folder" }],
        secrets: &[],
        check: Check::Local,
        syncs: true,
    },
    ConnectorDef {
        kind: "drive_folder",
        label: "Google Drive folder on this Mac",
        description: "Business documents from the Drive folder synced to this Mac. Read-only; Google Docs and Sheets are listed by name only, because their text isn't stored on the Mac.",
        config: &[Field { name: "path", label: "Folder", hint: "Choose a folder inside Google Drive" }],
        secrets: &[],
        check: Check::Local,
        syncs: true,
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
    /// When the state was established: now for local folders, the last check for network ones.
    pub checked_at: Option<String>,
    /// local | network | deferred.
    pub check: String,
    pub sync: BusinessSyncState,
    /// Always false in this phase: no connector writes outside the app.
    pub external_writes: bool,
}

/// The stored result of a network check.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckResult {
    /// ok | auth | error.
    pub outcome: String,
    pub detail: String,
    pub at: String,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Pure: the state and its sentence from what is known. `secrets`: Err = the Keychain couldn't be read.
/// `check`: the local check just run, or the stored network check.
pub(crate) fn derive_state(
    def: &ConnectorDef,
    config_complete: bool,
    secrets: &Result<Vec<bool>, String>,
    check: Option<&CheckResult>,
) -> (&'static str, String) {
    if let Check::Deferred(why) = def.check {
        return ("disconnected", why.to_string());
    }
    if let Err(e) = secrets {
        return (
            "error",
            format!("Couldn't read the Keychain ({e}). Nothing was changed."),
        );
    }
    if !config_complete {
        return ("disconnected", "Not set up.".into());
    }
    let all_stored = secrets
        .as_ref()
        .map(|v| v.iter().all(|s| *s))
        .unwrap_or(false);
    if !all_stored {
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
    match check {
        None if def.check == Check::Network => (
            if def.secrets.is_empty() {
                "disconnected"
            } else {
                "auth_required"
            },
            "Set up but not checked yet. Choose Check connection.".into(),
        ),
        None => ("disconnected", "Not checked yet.".into()),
        Some(c) => match c.outcome.as_str() {
            "ok" => ("connected", c.detail.clone()),
            "auth" => ("auth_required", c.detail.clone()),
            _ => ("error", c.detail.clone()),
        },
    }
}

pub(crate) fn service_name(b: &BusinessId, connector_id: &str, field: &str) -> String {
    format!("{SERVICE_PREFIX}biz.{}.{connector_id}.{field}", b.as_str())
}

/// Validates one config value. Returns the value to store.
pub(crate) fn validate_config(kind: &str, field: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.len() > 500 || v.chars().any(char::is_control) {
        return Err(format!("{field}: not a valid value"));
    }
    let ok = match (kind, field) {
        ("supabase", "pooler_host") => super::net::supabase_pooler_ok(v),
        ("supabase", "project_ref") => v.len() == 20 && v.bytes().all(|b| b.is_ascii_lowercase()),
        ("supabase", "ca_cert") => super::supabase::read_ca(Path::new(v)).is_ok(),
        // Only hosts Taisun approved for network use (net.rs); another domain needs its own approval.
        ("website", "domain") => {
            super::net::APPROVED_HTTP_HOSTS.contains(&v)
                && v != "api.github.com"
                && (3..=253).contains(&v.len())
                && v.contains('.')
                && !v.starts_with('.')
                && !v.ends_with('.')
                && v.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        }
        ("github", "repo") => {
            let mut parts = v.split('/');
            let ok_part = |p: Option<&str>| {
                p.is_some_and(|p| {
                    (1..=100).contains(&p.len())
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                        && p != "."
                        && p != ".."
                })
            };
            ok_part(parts.next()) && ok_part(parts.next()) && parts.next().is_none()
        }
        ("local_repo", "path") | ("drive_folder", "path") => local_path_ok(Path::new(v)).is_ok(),
        _ => return Err(format!("{kind} has no setting {field}")),
    };
    if !ok {
        return Err(match (kind, field) {
            ("supabase", "pooler_host") => "The pooler host must be a Supabase pooler, like aws-1-ap-northeast-1.pooler.supabase.com".into(),
            ("supabase", "project_ref") => "The project reference is 20 lowercase letters".into(),
            ("supabase", "ca_cert") => "Choose the Supabase CA certificate file (a .crt with one certificate, inside your home folder)".into(),
            ("website", _) => "That domain isn't approved for network use (decision 0064 approved taiyoskin.com only).".into(),
            ("github", _) => "Repository must look like owner/name".into(),
            _ => "Folder must be an absolute folder inside your home folder".into(),
        });
    }
    Ok(v.to_string())
}

#[cfg(test)]
thread_local! {
    /// Tests point "home" at their temp folder (tests never use the real home folder).
    pub(crate) static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn home_dir() -> Option<PathBuf> {
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

/// A real folder (not a symlink) that can be read. `marker`: a file the folder must contain.
pub(crate) fn check_folder(path: &str, marker: Option<&str>) -> Result<String, String> {
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
    if let Some(m) = marker {
        if !p.join(m).is_file() {
            return Err(format!(
                "The folder has no {m}, so it doesn't look like the site's source."
            ));
        }
    }
    Ok("Folder found and readable on this Mac (checked just now, read-only).".into())
}

/// The local repository check (kept for 0063 callers).
pub(crate) fn check_local_repo(path: &str) -> Result<String, String> {
    check_folder(path, Some("package.json"))
}

// ─── Tenant methods ──────────────────────────────────────────────────────────

impl Tenant {
    pub(crate) fn config_of(
        &self,
        connector_id: &str,
    ) -> Result<(String, BTreeMap<String, String>), String> {
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

    pub(crate) fn last_check(&self, connector_id: &str) -> Result<Option<CheckResult>, String> {
        self.conn()
            .query_row(
                "SELECT outcome, detail, checked_at FROM connector_check WHERE business_id = ?1 AND connector_id = ?2",
                params![self.id().as_str(), connector_id],
                |r| Ok(CheckResult { outcome: r.get(0)?, detail: r.get(1)?, at: r.get(2)? }),
            )
            .optional()
            .map_err(err)
    }

    /// Stores the outcome of a network check (`ok`, `auth` or `error`).
    pub(crate) fn record_check(
        &self,
        connector_id: &str,
        outcome: &str,
        detail: &str,
    ) -> Result<(), String> {
        super::check(&["ok", "auth", "error"], outcome, "outcome")?;
        self.conn()
            .execute(
                "INSERT INTO connector_check (business_id, connector_id, outcome, detail, checked_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (business_id, connector_id) DO UPDATE SET outcome = excluded.outcome, detail = excluded.detail, checked_at = excluded.checked_at",
                params![self.id().as_str(), connector_id, outcome, detail, now()],
            )
            .map_err(err)?;
        Ok(())
    }

    fn clear_check(&self, connector_id: &str) -> Result<(), String> {
        self.conn()
            .execute(
                "DELETE FROM connector_check WHERE business_id = ?1 AND connector_id = ?2",
                params![self.id().as_str(), connector_id],
            )
            .map_err(err)?;
        Ok(())
    }

    /// A secret for Rust-side use only (a connector call). Never returned over IPC.
    pub(crate) fn secret(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
        field: &str,
    ) -> Result<zeroize::Zeroizing<String>, String> {
        match kc.get(&service_name(self.id(), connector_id, field)) {
            Ok(v) => String::from_utf8(v)
                .map(zeroize::Zeroizing::new)
                .map_err(|_| "the stored key isn't text".into()),
            Err(KeychainError::NotFound) => Err("the key isn't stored yet".into()),
            Err(KeychainError::Other(e)) => Err(format!("Couldn't read the Keychain ({e})")),
        }
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
        let check = match def.check {
            Check::Local if complete => {
                let marker = (kind == "local_repo").then_some("package.json");
                let r = check_folder(config.get("path").map(String::as_str).unwrap_or(""), marker);
                Some(CheckResult {
                    outcome: if r.is_ok() { "ok" } else { "error" }.into(),
                    detail: r.unwrap_or_else(|e| e),
                    at: now(),
                })
            }
            Check::Network => self.last_check(connector_id)?,
            _ => None,
        };
        let (state, detail) = derive_state(def, complete, &secrets, check.as_ref());
        let stored = secrets.clone().unwrap_or_default();
        let last = self.sync_runs(Some(connector_id), 1)?.into_iter().next();
        let sync = if !def.syncs {
            BusinessSyncState {
                state: "unavailable".into(),
                detail: "No synchronization in this phase.".into(),
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
            checked_at: check.map(|c| c.at),
            check: match def.check {
                Check::Local => "local",
                Check::Network => "network",
                Check::Deferred(_) => "deferred",
            }
            .into(),
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

    /// Sets non-secret settings. Unknown fields are refused; an empty value clears the field. A
    /// changed setting clears the last check (it described other settings).
    pub(crate) fn connector_configure(
        &self,
        kc: &dyn Keychain,
        connector_id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<BusinessConnector, String> {
        let (kind, mut config) = self.config_of(connector_id)?;
        let def = definition(&kind)?;
        if matches!(def.check, Check::Deferred(_)) {
            return Err(format!("{} isn't available in this phase", def.label));
        }
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
        self.clear_check(connector_id)?;
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
            self.clear_check(connector_id)?;
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

    /// Disconnect: clears the settings, the last check and this connector's Keychain items. Synced
    /// material and history stay.
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
        self.clear_check(connector_id)?;
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

    /// Starts a sync run row (`running`), refusing a second one for the same connector.
    pub(crate) fn begin_run(&self, connector_id: &str) -> Result<String, String> {
        let running: i64 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_run WHERE business_id = ?1 AND connector_id = ?2 AND status = 'running'",
                params![self.id().as_str(), connector_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if running > 0 {
            return Err("a sync of this connector is already running".into());
        }
        let run_id = uuid::Uuid::new_v4().simple().to_string();
        self.conn()
            .execute(
                "INSERT INTO sync_run (business_id, run_id, connector_id, started_at, status) VALUES (?1, ?2, ?3, ?4, 'running')",
                params![self.id().as_str(), run_id, connector_id, now()],
            )
            .map_err(err)?;
        Ok(run_id)
    }

    /// Finishes a run and records one activity row for it.
    pub(crate) fn finish_run(
        &self,
        run_id: &str,
        connector_label: &str,
        outcome: Result<(i64, i64, i64, String), String>,
        actor: &str,
    ) -> Result<BusinessSyncRun, String> {
        let (status, seen, changed, conflicts, detail) = match outcome {
            Ok((s, c, k, d)) => ("succeeded", s, c, k, d),
            Err(e) => ("failed", 0, 0, 0, e),
        };
        self.conn()
            .execute(
                "UPDATE sync_run SET status = ?3, finished_at = ?4, items_seen = ?5, items_changed = ?6, conflicts = ?7, detail = ?8
                 WHERE business_id = ?1 AND run_id = ?2",
                params![self.id().as_str(), run_id, status, now(), seen, changed, conflicts, detail],
            )
            .map_err(err)?;
        super::note_activity(
            &self.data_path(),
            "business.sync_finished",
            &super::biz_ref(self.id(), "sync", Some(run_id)),
            if actor == "user" { "taisun" } else { "system" },
            &format!("{connector_label} sync {status}: {detail}"),
        );
        self.sync_runs(None, 200)?
            .into_iter()
            .find(|r| r.run_id == run_id)
            .ok_or_else(|| "sync run vanished".into())
    }
}
