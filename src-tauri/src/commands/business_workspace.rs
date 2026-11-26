//! Amara Enterprise business workspaces (decision 0063, Phase 11 foundation).
//!
//! One workspace per business. TaiyoSkin is the first; more can be created. Everything a workspace
//! owns lives in `<data>/business/workspaces.db` (business scope, beside `ledger.json`,
//! `treasury.db` and `amara_business.db`), and its document copies under
//! `<data>/business/workspaces/<business id>/documents/`.
//!
//! **Tenant isolation.** Every table but `business` has `business_id NOT NULL` in its primary key
//! and a composite foreign key to its parent, so a row can't point at another business's row.
//! Triggers refuse any change of `business_id`. Rust reaches the rows only through a [`Tenant`],
//! obtained for one validated business id; every statement it runs binds that id (a source-pin
//! test checks each one). There is no cross-business query except [`WorkspaceStore::list_businesses`],
//! which returns identities only.
//!
//! **Approved versus not.** Context items and documents are `draft`, `unverified` or `approved`
//! (or `archived`). Imported and synced material is `unverified`. Only the user approves, and
//! editing approved text makes it a draft again. Every change keeps a version.
//!
//! **Separation.** Nothing here reads or writes personal stores, Letta, chat context or ARM. The
//! activity it records goes to the business file only, tagged with its business (`biz:` refs).
//! No network: connectors are registered and checked locally only (`connectors.rs`).

pub(crate) mod commands;
pub(crate) mod connectors;
pub(crate) mod drive_folder;
pub(crate) mod intelligence;
pub(crate) mod net;
pub(crate) mod ops;
pub(crate) mod site_ops;
pub(crate) mod supabase;
pub(crate) mod sync_local;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests12;
#[cfg(test)]
mod tests13;
pub(crate) mod website;

use super::amara_store::{self, NewActivity, Scope};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// v1 (0063): workspaces. v2 (0064): the Phase 12 connector registry, check results, sync cursors,
/// first-party site analytics, customer aggregates, site facts and interpretation notes.
pub(crate) const SCHEMA_VERSION: i64 = 3;
const MAX_TITLE: usize = 160;
const MAX_BODY: usize = 20_000;
const MAX_MEMORY: usize = 2_000;
pub(crate) const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
/// Text kept from a text document for knowledge search.
const MAX_DOCUMENT_TEXT: usize = 200 * 1024;

/// The ten context sections (the brief's list), in display order.
pub(crate) const SECTIONS: &[(&str, &str)] = &[
    ("identity", "Brand identity and voice"),
    ("products", "Product catalogue"),
    ("formulations", "Approved formulations"),
    ("ingredients", "Ingredient information"),
    ("audiences", "Target audiences"),
    ("positioning", "Brand positioning"),
    ("objectives", "Business objectives"),
    ("website", "Website structure"),
    ("documents", "Approved business documents"),
    ("decisions", "Historical decisions and recommendations"),
];
pub(crate) const MATERIAL_STATUSES: &[&str] = &["draft", "unverified", "approved", "archived"];
pub(crate) const ACTORS: &[&str] = &["model", "scheduler", "integration"];
pub(crate) const CAPABILITIES: &[(&str, &str)] = &[
    ("read_approved_context", "Read approved context"),
    (
        "read_unverified_context",
        "Read drafts and unverified context",
    ),
    ("read_documents", "Read documents"),
    ("propose_changes", "Propose changes for approval"),
    ("sync_connectors", "Start a synchronization"),
];
/// Defaults for a new workspace: Amara may read approved context and propose changes; nothing
/// else is allowed until the user turns it on. Writes outside the app are not a capability at all.
fn default_allowed(actor: &str, capability: &str) -> bool {
    actor == "model" && matches!(capability, "read_approved_context" | "propose_changes")
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
fn err(e: impl std::fmt::Display) -> String {
    format!("workspace store: {e}")
}
fn check(list: &[&str], v: &str, what: &str) -> Result<(), String> {
    list.contains(&v)
        .then_some(())
        .ok_or_else(|| format!("{what}: {v} is not allowed"))
}
fn section_ok(s: &str) -> Result<(), String> {
    SECTIONS
        .iter()
        .any(|(k, _)| *k == s)
        .then_some(())
        .ok_or_else(|| format!("section: {s} is not allowed"))
}
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn text_ok(s: &str, max: usize, what: &str) -> Result<(), String> {
    if s.chars().count() > max {
        return Err(format!("{what} is too long"));
    }
    if s.chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err(format!("{what} has control characters"));
    }
    Ok(())
}

// ─── Business id ─────────────────────────────────────────────────────────────

/// `[a-z0-9][a-z0-9-]{1,39}`: safe in a file path, a Keychain service name and a `biz:` ref.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BusinessId(String);

impl BusinessId {
    pub(crate) fn parse(s: &str) -> Result<Self, String> {
        let ok = (2..=40).contains(&s.len())
            && s.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        ok.then(|| Self(s.to_string()))
            .ok_or_else(|| "business id must be 2–40 lowercase letters, digits or dashes".into())
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    /// From a display name: "Taiyo Skin" → "taiyo-skin".
    pub(crate) fn from_name(name: &str) -> Result<Self, String> {
        let mut out = String::new();
        for c in name.trim().to_lowercase().chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
        let out = out
            .trim_end_matches('-')
            .chars()
            .take(40)
            .collect::<String>();
        Self::parse(out.trim_end_matches('-'))
    }
}

/// The `biz:` record reference for a workspace record, as `amara_store` parses it.
pub(crate) fn biz_ref(b: &BusinessId, kind: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("biz:{}/{kind}/{id}", b.as_str()),
        None => format!("biz:{}/{kind}", b.as_str()),
    }
}

/// Item and document ids: `[A-Za-z0-9_-]{1,64}`.
pub(crate) fn local_id_ok(s: &str) -> Result<(), String> {
    ((1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then_some(())
    .ok_or_else(|| "id must be 1–64 letters, digits, dashes or underscores".into())
}

// ─── Records ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Business {
    pub business_id: String,
    pub name: String,
    pub description: Option<String>,
    /// The ledger venture this workspace belongs to (`ledger.json`), when linked.
    pub venture_id: Option<String>,
    /// active | archived.
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessContextItem {
    pub item_id: String,
    pub section: String,
    pub title: String,
    pub body: String,
    /// draft | unverified | approved | archived.
    pub status: String,
    /// user | import | sync | amara.
    pub source: String,
    pub source_ref: Option<String>,
    pub version: i64,
    pub approved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessContextVersion {
    pub version: i64,
    pub title: String,
    pub body: String,
    pub status: String,
    /// taisun | amara | system.
    pub actor: String,
    pub note: Option<String>,
    pub at: String,
}

#[derive(Deserialize, Clone, Debug, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessContextInput {
    /// None = a new item.
    pub item_id: Option<String>,
    pub section: String,
    pub title: String,
    pub body: String,
    /// draft | unverified. Approval is a separate step.
    pub status: String,
    /// Required when editing: the version the edit was made against.
    pub expected_version: Option<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessMemoryNote {
    pub memory_id: String,
    pub text: String,
    /// proposed | kept | dismissed.
    pub status: String,
    /// user | amara.
    pub source: String,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessDocument {
    pub doc_id: String,
    pub title: String,
    /// markdown | text | pdf | image | other.
    pub kind: String,
    pub status: String,
    pub file_name: String,
    pub sha256: String,
    pub bytes: i64,
    /// user | sync.
    pub source: String,
    pub source_ref: Option<String>,
    /// Whether text is kept for knowledge search (markdown and plain text only).
    pub searchable: bool,
    pub version: i64,
    pub approved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessPermission {
    pub actor: String,
    pub capability: String,
    pub label: String,
    pub allowed: bool,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessProposal {
    pub proposal_id: String,
    /// context_update.
    pub kind: String,
    pub item_id: String,
    /// The item version the proposal was made against; applying refuses a newer one.
    pub base_version: i64,
    pub title: String,
    pub body: String,
    pub rationale: Option<String>,
    /// pending | applied | rejected | failed.
    pub status: String,
    /// amara | scheduler.
    pub proposed_by: String,
    pub proposed_at: String,
    pub decided_at: Option<String>,
    pub execution_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessKnowledgeHit {
    /// context | document.
    pub kind: String,
    pub id: String,
    pub title: String,
    pub section: Option<String>,
    pub status: String,
    /// A short excerpt around the first match.
    pub excerpt: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSectionCount {
    pub section: String,
    pub label: String,
    pub approved: i64,
    pub draft: i64,
    pub unverified: i64,
}

// ─── Store ───────────────────────────────────────────────────────────────────

pub(crate) struct WorkspaceStore {
    /// `<data>/business/workspaces.db`.
    path: PathBuf,
}

pub(crate) fn store_in(data: &Path) -> WorkspaceStore {
    WorkspaceStore {
        path: data.join("business").join("workspaces.db"),
    }
}
pub(crate) fn live() -> WorkspaceStore {
    store_in(&crate::app_paths::data_dir())
}

impl WorkspaceStore {
    /// The app data folder (two levels up from the database).
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }
    pub(crate) fn documents_dir(&self, b: &BusinessId) -> PathBuf {
        self.data_dir()
            .join("business")
            .join("workspaces")
            .join(b.as_str())
            .join("documents")
    }

    /// Opens (creating if needed). A newer schema is refused, never repaired. A brand-new file gets
    /// the first workspace, TaiyoSkin, linked to its ledger venture.
    pub(crate) fn open(&self) -> Result<Connection, String> {
        let fresh = !self.path.exists();
        let conn = super::shared::sqlite::open_private_wal(&self.path)
            .map_err(|s| err(format!("open failed: {s:?}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(err)?;
        init(&conn)?;
        if fresh {
            self.seed_first(&conn)?;
        }
        Ok(conn)
    }

    fn seed_first(&self, conn: &Connection) -> Result<(), String> {
        let id = BusinessId::parse("taiyo-skin")?;
        let exists: bool = conn
            .query_row("SELECT COUNT(*) FROM business", [], |r| r.get::<_, i64>(0))
            .map_err(err)?
            > 0;
        if exists {
            return Ok(());
        }
        insert_business(
            conn,
            &id,
            "TaiyoSkin",
            Some("Skin care brand. The first business workspace."),
            Some("taiyo-skin"),
        )?;
        note_activity(
            &self.data_dir(),
            "business.created",
            &biz_ref(&id, "business", None),
            "system",
            "Workspace created: TaiyoSkin",
        );
        Ok(())
    }

    /// Identities only: the one query that spans businesses.
    pub(crate) fn list_businesses(&self) -> Result<Vec<Business>, String> {
        let conn = self.open()?;
        let mut st = conn
            .prepare(&format!(
                "SELECT {BIZ_COLS} FROM business ORDER BY status, name"
            ))
            .map_err(err)?;
        let rows = st
            .query_map([], business_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn create_business(
        &self,
        name: &str,
        id: Option<&str>,
        description: Option<&str>,
        venture_id: Option<&str>,
    ) -> Result<Business, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a business needs a name".into());
        }
        text_ok(name, 80, "name")?;
        if let Some(d) = description {
            text_ok(d, 500, "description")?;
        }
        if let Some(v) = venture_id {
            local_id_ok(v)?;
        }
        let id = match id {
            Some(i) => BusinessId::parse(i)?,
            None => BusinessId::from_name(name)?,
        };
        let conn = self.open()?;
        insert_business(&conn, &id, name, description, venture_id)?;
        note_activity(
            &self.data_dir(),
            "business.created",
            &biz_ref(&id, "business", None),
            "taisun",
            &format!("Workspace created: {name}"),
        );
        self.tenant(id.as_str())?.business()
    }

    /// The only way to reach a business's records. Refuses an unknown id.
    pub(crate) fn tenant(&self, business_id: &str) -> Result<Tenant, String> {
        let id = BusinessId::parse(business_id)?;
        let conn = self.open()?;
        let found: Option<String> = conn
            .query_row(
                "SELECT status FROM business WHERE business_id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if found.is_none() {
            return Err("no such business".into());
        }
        Ok(Tenant {
            id,
            conn,
            data: self.data_dir(),
            docs: self.documents_dir(&BusinessId::parse(business_id)?),
            quiet: std::cell::Cell::new(false),
        })
    }
}

fn insert_business(
    conn: &Connection,
    id: &BusinessId,
    name: &str,
    description: Option<&str>,
    venture_id: Option<&str>,
) -> Result<(), String> {
    let t = now();
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO business (business_id, name, description, venture_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![id.as_str(), name, description, venture_id, t],
        )
        .map_err(err)?;
    if n == 0 {
        return Err("a business with that id already exists".into());
    }
    for actor in ACTORS {
        for (cap, _) in CAPABILITIES {
            conn.execute(
                "INSERT INTO permission (business_id, actor, capability, allowed, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id.as_str(), actor, cap, default_allowed(actor, cap) as i64, t],
            )
            .map_err(err)?;
        }
    }
    for def in connectors::DEFINITIONS {
        conn.execute(
            "INSERT INTO connector (business_id, connector_id, kind, config_json, created_at, updated_at)
             VALUES (?1, ?2, ?2, '{}', ?3, ?3)",
            params![id.as_str(), def.kind, t],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Records one business activity row in the business file only (best effort, like every owner's
/// activity: a failure is logged and never fails the change). The subject is a `biz:` ref, so the
/// store tags the row with its business.
pub(crate) fn note_activity(data: &Path, kind: &str, subject: &str, actor: &str, summary: &str) {
    let at = now();
    amara_store::note(
        data,
        NewActivity {
            scope: Scope::Business,
            certainty: "confirmed",
            kind,
            subject_ref: subject,
            occurred_at: &at,
            source: match actor {
                "taisun" => "user",
                "amara" => "model",
                _ => "scheduler",
            },
            source_ref: None,
            actor,
            summary: Some(summary),
            data_json: None,
        },
    );
}

const BIZ_COLS: &str = "business_id, name, description, venture_id, status, created_at, updated_at";
fn business_row(r: &rusqlite::Row) -> rusqlite::Result<Business> {
    Ok(Business {
        business_id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        venture_id: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}
const CTX_COLS: &str = "item_id, section, title, body, status, source, source_ref, version, approved_at, created_at, updated_at";
fn context_row(r: &rusqlite::Row) -> rusqlite::Result<BusinessContextItem> {
    Ok(BusinessContextItem {
        item_id: r.get(0)?,
        section: r.get(1)?,
        title: r.get(2)?,
        body: r.get(3)?,
        status: r.get(4)?,
        source: r.get(5)?,
        source_ref: r.get(6)?,
        version: r.get(7)?,
        approved_at: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}
const DOC_COLS: &str = "doc_id, title, kind, status, file_name, sha256, bytes, source, source_ref, text IS NOT NULL, version, approved_at, created_at, updated_at";
fn document_row(r: &rusqlite::Row) -> rusqlite::Result<BusinessDocument> {
    Ok(BusinessDocument {
        doc_id: r.get(0)?,
        title: r.get(1)?,
        kind: r.get(2)?,
        status: r.get(3)?,
        file_name: r.get(4)?,
        sha256: r.get(5)?,
        bytes: r.get(6)?,
        source: r.get(7)?,
        source_ref: r.get(8)?,
        searchable: r.get(9)?,
        version: r.get(10)?,
        approved_at: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}
const PROP_COLS: &str = "proposal_id, kind, item_id, base_version, title, body, rationale, status, proposed_by, proposed_at, decided_at, execution_id, detail";
fn proposal_row(r: &rusqlite::Row) -> rusqlite::Result<BusinessProposal> {
    Ok(BusinessProposal {
        proposal_id: r.get(0)?,
        kind: r.get(1)?,
        item_id: r.get(2)?,
        base_version: r.get(3)?,
        title: r.get(4)?,
        body: r.get(5)?,
        rationale: r.get(6)?,
        status: r.get(7)?,
        proposed_by: r.get(8)?,
        proposed_at: r.get(9)?,
        decided_at: r.get(10)?,
        execution_id: r.get(11)?,
        detail: r.get(12)?,
    })
}

// ─── Tenant ──────────────────────────────────────────────────────────────────

/// One business's view of the store. Every statement binds `self.id` as `?1`.
pub(crate) struct Tenant {
    id: BusinessId,
    conn: Connection,
    data: PathBuf,
    docs: PathBuf,
    /// Set during a sync: per-item activity is folded into the one `business.sync_finished` row.
    quiet: std::cell::Cell<bool>,
}

impl Tenant {
    pub(crate) fn id(&self) -> &BusinessId {
        &self.id
    }
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
    pub(crate) fn data_path(&self) -> PathBuf {
        self.data.clone()
    }
    pub(crate) fn docs_path(&self) -> &Path {
        &self.docs
    }
    fn b(&self) -> &str {
        self.id.as_str()
    }
    fn note(&self, kind: &str, subject: &str, actor: &str, summary: &str) {
        if !self.quiet.get() {
            note_activity(&self.data, kind, subject, actor, summary);
        }
    }
    pub(crate) fn set_quiet(&self, q: bool) {
        self.quiet.set(q);
    }

    pub(crate) fn business(&self) -> Result<Business, String> {
        self.conn
            .query_row(
                &format!("SELECT {BIZ_COLS} FROM business WHERE business_id = ?1"),
                params![self.b()],
                business_row,
            )
            .map_err(err)
    }

    pub(crate) fn update_identity(
        &self,
        name: &str,
        description: Option<&str>,
        venture_id: Option<&str>,
        status: &str,
    ) -> Result<Business, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a business needs a name".into());
        }
        text_ok(name, 80, "name")?;
        if let Some(d) = description {
            text_ok(d, 500, "description")?;
        }
        if let Some(v) = venture_id {
            local_id_ok(v)?;
        }
        check(&["active", "archived"], status, "status")?;
        self.conn
            .execute(
                "UPDATE business SET name = ?2, description = ?3, venture_id = ?4, status = ?5, updated_at = ?6
                 WHERE business_id = ?1",
                params![self.b(), name, description, venture_id, status, now()],
            )
            .map_err(err)?;
        self.note(
            "business.updated",
            &biz_ref(&self.id, "business", None),
            "taisun",
            &format!("Workspace details updated: {name}"),
        );
        self.business()
    }

    // ── Context ──

    pub(crate) fn context_list(
        &self,
        section: Option<&str>,
    ) -> Result<Vec<BusinessContextItem>, String> {
        if let Some(s) = section {
            section_ok(s)?;
        }
        let mut st = self
            .conn
            .prepare(&format!(
                "SELECT {CTX_COLS} FROM context_item WHERE business_id = ?1 AND (?2 IS NULL OR section = ?2)
                 ORDER BY section, CASE status WHEN 'approved' THEN 0 WHEN 'draft' THEN 1 WHEN 'unverified' THEN 2 ELSE 3 END, title"
            ))
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b(), section], context_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn context_get(&self, item_id: &str) -> Result<Option<BusinessContextItem>, String> {
        local_id_ok(item_id)?;
        self.conn
            .query_row(
                &format!(
                    "SELECT {CTX_COLS} FROM context_item WHERE business_id = ?1 AND item_id = ?2"
                ),
                params![self.b(), item_id],
                context_row,
            )
            .optional()
            .map_err(err)
    }

    pub(crate) fn context_by_source_ref(
        &self,
        source_ref: &str,
    ) -> Result<Option<BusinessContextItem>, String> {
        self.conn
            .query_row(
                &format!("SELECT {CTX_COLS} FROM context_item WHERE business_id = ?1 AND source_ref = ?2"),
                params![self.b(), source_ref],
                context_row,
            )
            .optional()
            .map_err(err)
    }

    /// Create or edit an item as draft or unverified. Editing an approved item makes it a draft.
    pub(crate) fn context_save(
        &self,
        input: &BusinessContextInput,
        actor: &str,
        source: &str,
        source_ref: Option<&str>,
    ) -> Result<BusinessContextItem, String> {
        section_ok(&input.section)?;
        check(&["draft", "unverified"], &input.status, "status")?;
        check(&["user", "import", "sync", "amara"], source, "source")?;
        let title = input.title.trim();
        if title.is_empty() {
            return Err("an item needs a title".into());
        }
        text_ok(title, MAX_TITLE, "title")?;
        text_ok(&input.body, MAX_BODY, "body")?;
        let t = now();
        let tx = self.tx()?;
        let item_id = match &input.item_id {
            None => {
                let id = uuid::Uuid::new_v4().simple().to_string();
                tx.execute(
                    "INSERT INTO context_item (business_id, item_id, section, title, body, status, source, source_ref,
                        version, approved_at, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, NULL, ?9, ?9)",
                    params![self.b(), id, input.section, title, input.body, input.status, source, source_ref, t],
                )
                .map_err(err)?;
                version_row(&tx, self.b(), &id, actor, Some("created"))?;
                id
            }
            Some(id) => {
                local_id_ok(id)?;
                let expected = input
                    .expected_version
                    .ok_or("editing needs the version it was made against")?;
                let n = tx
                    .execute(
                        "UPDATE context_item SET section = ?3, title = ?4, body = ?5, status = ?6, version = version + 1,
                            approved_at = NULL, updated_at = ?7
                         WHERE business_id = ?1 AND item_id = ?2 AND version = ?8",
                        params![self.b(), id, input.section, title, input.body, input.status, t, expected],
                    )
                    .map_err(err)?;
                if n == 0 {
                    return Err("this item changed since you opened it (or it doesn't exist); reload and try again".into());
                }
                version_row(&tx, self.b(), id, actor, Some("edited"))?;
                id.clone()
            }
        };
        tx.commit().map_err(err)?;
        let item = self.context_get(&item_id)?.ok_or("item vanished")?;
        self.note(
            "business.context_changed",
            &biz_ref(&self.id, "context", Some(&item_id)),
            actor,
            &format!(
                "{}: {} ({})",
                section_label(&item.section),
                item.title,
                item.status
            ),
        );
        Ok(item)
    }

    /// Approve, return to draft, mark unverified or archive. Only the user approves.
    pub(crate) fn context_set_status(
        &self,
        item_id: &str,
        status: &str,
        expected_version: i64,
        actor: &str,
    ) -> Result<BusinessContextItem, String> {
        local_id_ok(item_id)?;
        check(MATERIAL_STATUSES, status, "status")?;
        if status == "approved" && actor != "taisun" {
            return Err("only you can approve business material".into());
        }
        let t = now();
        let tx = self.tx()?;
        let n = tx
            .execute(
                "UPDATE context_item SET status = ?3, version = version + 1, updated_at = ?4,
                    approved_at = CASE WHEN ?3 = 'approved' THEN ?4 ELSE NULL END
                 WHERE business_id = ?1 AND item_id = ?2 AND version = ?5 AND status <> ?3",
                params![self.b(), item_id, status, t, expected_version],
            )
            .map_err(err)?;
        if n == 0 {
            return Err(
                "this item changed since you opened it, already has that status, or doesn't exist"
                    .into(),
            );
        }
        version_row(&tx, self.b(), item_id, actor, Some(status))?;
        tx.commit().map_err(err)?;
        let item = self.context_get(item_id)?.ok_or("item vanished")?;
        let kind = if status == "approved" {
            "business.context_approved"
        } else {
            "business.context_changed"
        };
        self.note(
            kind,
            &biz_ref(&self.id, "context", Some(item_id)),
            actor,
            &format!(
                "{}: {} is now {status}",
                section_label(&item.section),
                item.title
            ),
        );
        Ok(item)
    }

    pub(crate) fn context_history(
        &self,
        item_id: &str,
    ) -> Result<Vec<BusinessContextVersion>, String> {
        local_id_ok(item_id)?;
        let mut st = self
            .conn
            .prepare(
                "SELECT version, title, body, status, actor, note, at FROM context_version
                 WHERE business_id = ?1 AND item_id = ?2 ORDER BY version DESC",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b(), item_id], |r| {
                Ok(BusinessContextVersion {
                    version: r.get(0)?,
                    title: r.get(1)?,
                    body: r.get(2)?,
                    status: r.get(3)?,
                    actor: r.get(4)?,
                    note: r.get(5)?,
                    at: r.get(6)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn section_counts(&self) -> Result<Vec<BusinessSectionCount>, String> {
        let mut out = Vec::new();
        for (s, label) in SECTIONS {
            let (a, d, u): (i64, i64, i64) = self
                .conn
                .query_row(
                    "SELECT COALESCE(SUM(status = 'approved'), 0), COALESCE(SUM(status = 'draft'), 0),
                        COALESCE(SUM(status = 'unverified'), 0)
                     FROM context_item WHERE business_id = ?1 AND section = ?2",
                    params![self.b(), s],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(err)?;
            out.push(BusinessSectionCount {
                section: (*s).into(),
                label: (*label).into(),
                approved: a,
                draft: d,
                unverified: u,
            });
        }
        Ok(out)
    }

    /// What Amara may be given about this business: approved items only, unless the user also allowed
    /// drafts and unverified items (then each keeps its status label). Refused without permission.
    pub(crate) fn context_for_model(&self) -> Result<Vec<BusinessContextItem>, String> {
        self.require("model", "read_approved_context")?;
        let unverified = self.allowed("model", "read_unverified_context")?;
        Ok(self
            .context_list(None)?
            .into_iter()
            .filter(|i| {
                i.status == "approved"
                    || (unverified && matches!(i.status.as_str(), "draft" | "unverified"))
            })
            .collect())
    }

    // ── Memory ──

    pub(crate) fn memory_list(&self) -> Result<Vec<BusinessMemoryNote>, String> {
        let mut st = self
            .conn
            .prepare(
                "SELECT memory_id, text, status, source, created_at, decided_at FROM memory
                 WHERE business_id = ?1 ORDER BY CASE status WHEN 'proposed' THEN 0 WHEN 'kept' THEN 1 ELSE 2 END, created_at DESC",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b()], |r| {
                Ok(BusinessMemoryNote {
                    memory_id: r.get(0)?,
                    text: r.get(1)?,
                    status: r.get(2)?,
                    source: r.get(3)?,
                    created_at: r.get(4)?,
                    decided_at: r.get(5)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// A note you add is kept at once; one Amara proposes waits for you (and needs `propose_changes`).
    pub(crate) fn memory_add(
        &self,
        text: &str,
        source: &str,
    ) -> Result<BusinessMemoryNote, String> {
        check(&["user", "amara"], source, "source")?;
        let text = text.trim();
        if text.is_empty() {
            return Err("a memory note needs text".into());
        }
        text_ok(text, MAX_MEMORY, "memory note")?;
        if source == "amara" {
            self.require("model", "propose_changes")?;
        }
        let (status, decided) = if source == "user" {
            ("kept", Some(now()))
        } else {
            ("proposed", None)
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.conn
            .execute(
                "INSERT INTO memory (business_id, memory_id, text, status, source, created_at, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![self.b(), id, text, status, source, now(), decided],
            )
            .map_err(err)?;
        let actor = if source == "user" { "taisun" } else { "amara" };
        self.note(
            "business.memory_changed",
            &biz_ref(&self.id, "memory", Some(&id)),
            actor,
            &format!("Memory note {status}"),
        );
        self.memory_list()?
            .into_iter()
            .find(|m| m.memory_id == id)
            .ok_or_else(|| "note vanished".into())
    }

    pub(crate) fn memory_decide(
        &self,
        memory_id: &str,
        status: &str,
    ) -> Result<BusinessMemoryNote, String> {
        local_id_ok(memory_id)?;
        check(&["kept", "dismissed"], status, "status")?;
        let n = self
            .conn
            .execute(
                "UPDATE memory SET status = ?3, decided_at = ?4 WHERE business_id = ?1 AND memory_id = ?2 AND status <> ?3",
                params![self.b(), memory_id, status, now()],
            )
            .map_err(err)?;
        if n == 0 {
            return Err("no such note, or it already has that status".into());
        }
        self.note(
            "business.memory_changed",
            &biz_ref(&self.id, "memory", Some(memory_id)),
            "taisun",
            &format!("Memory note {status}"),
        );
        self.memory_list()?
            .into_iter()
            .find(|m| m.memory_id == memory_id)
            .ok_or_else(|| "note vanished".into())
    }

    // ── Documents and knowledge ──

    pub(crate) fn documents(&self) -> Result<Vec<BusinessDocument>, String> {
        let mut st = self
            .conn
            .prepare(&format!(
                "SELECT {DOC_COLS} FROM document WHERE business_id = ?1
                 ORDER BY CASE status WHEN 'approved' THEN 0 WHEN 'draft' THEN 1 WHEN 'unverified' THEN 2 ELSE 3 END, title"
            ))
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b()], document_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn document_get(&self, doc_id: &str) -> Result<Option<BusinessDocument>, String> {
        local_id_ok(doc_id)?;
        self.conn
            .query_row(
                &format!("SELECT {DOC_COLS} FROM document WHERE business_id = ?1 AND doc_id = ?2"),
                params![self.b(), doc_id],
                document_row,
            )
            .optional()
            .map_err(err)
    }

    pub(crate) fn document_by_source_ref(
        &self,
        source_ref: &str,
    ) -> Result<Option<BusinessDocument>, String> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {DOC_COLS} FROM document WHERE business_id = ?1 AND source_ref = ?2"
                ),
                params![self.b(), source_ref],
                document_row,
            )
            .optional()
            .map_err(err)
    }

    /// Copies the bytes into this business's documents folder (0600), reads the copy back and checks
    /// its hash before the row is written. Text documents keep their text for knowledge search.
    /// `replace`: the id of a document from the same source to update in place (sync only).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn document_store(
        &self,
        title: &str,
        file_name: &str,
        bytes: &[u8],
        status: &str,
        source: &str,
        source_ref: Option<&str>,
        replace: Option<&str>,
    ) -> Result<BusinessDocument, String> {
        check(&["draft", "unverified"], status, "status")?;
        check(&["user", "sync"], source, "source")?;
        let title = title.trim();
        if title.is_empty() {
            return Err("a document needs a title".into());
        }
        text_ok(title, MAX_TITLE, "title")?;
        if bytes.is_empty() {
            return Err("the file is empty".into());
        }
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("the file is over 10 MB".into());
        }
        let safe_name = safe_file_name(file_name);
        let kind = kind_of(&safe_name);
        let text = match kind {
            "markdown" | "text" => std::str::from_utf8(bytes)
                .ok()
                .map(|t| t.chars().take(MAX_DOCUMENT_TEXT).collect::<String>()),
            _ => None,
        };
        let sha = sha256_hex(bytes);
        let doc_id = match replace {
            Some(id) => {
                local_id_ok(id)?;
                id.to_string()
            }
            None => uuid::Uuid::new_v4().simple().to_string(),
        };
        crate::app_paths::create_private_dir_all(&self.docs).map_err(err)?;
        let dest = self.docs.join(format!("{doc_id}-{safe_name}"));
        crate::app_paths::write_private(&dest, bytes).map_err(err)?;
        let back = std::fs::read(&dest).map_err(err)?;
        if sha256_hex(&back) != sha {
            let _ = std::fs::remove_file(&dest);
            return Err("the stored copy did not match the file; nothing was saved".into());
        }
        let t = now();
        let stored_name = format!("{doc_id}-{safe_name}");
        match replace {
            None => {
                self.conn
                    .execute(
                        "INSERT INTO document (business_id, doc_id, title, kind, status, file_name, sha256, bytes, source,
                            source_ref, text, version, approved_at, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, NULL, ?12, ?12)",
                        params![self.b(), doc_id, title, kind, status, stored_name, sha, bytes.len() as i64, source, source_ref, text, t],
                    )
                    .map_err(err)?;
                self.note(
                    "business.document_added",
                    &biz_ref(&self.id, "document", Some(&doc_id)),
                    if source == "user" { "taisun" } else { "system" },
                    &format!("Document added: {title} ({status})"),
                );
            }
            Some(_) => {
                let n = self
                    .conn
                    .execute(
                        "UPDATE document SET title = ?3, kind = ?4, status = ?5, file_name = ?6, sha256 = ?7, bytes = ?8,
                            text = ?9, version = version + 1, approved_at = NULL, updated_at = ?10
                         WHERE business_id = ?1 AND doc_id = ?2 AND status <> 'approved'",
                        params![self.b(), doc_id, title, kind, status, stored_name, sha, bytes.len() as i64, text, t],
                    )
                    .map_err(err)?;
                if n == 0 {
                    return Err("that document is approved or missing; it was not replaced".into());
                }
                self.note(
                    "business.document_changed",
                    &biz_ref(&self.id, "document", Some(&doc_id)),
                    "system",
                    &format!("Document updated from its source: {title}"),
                );
            }
        }
        self.document_get(&doc_id)?
            .ok_or_else(|| "document vanished".into())
    }

    pub(crate) fn document_set_status(
        &self,
        doc_id: &str,
        status: &str,
        expected_version: i64,
        actor: &str,
    ) -> Result<BusinessDocument, String> {
        local_id_ok(doc_id)?;
        check(MATERIAL_STATUSES, status, "status")?;
        if status == "approved" && actor != "taisun" {
            return Err("only you can approve business material".into());
        }
        let t = now();
        let n = self
            .conn
            .execute(
                "UPDATE document SET status = ?3, version = version + 1, updated_at = ?4,
                    approved_at = CASE WHEN ?3 = 'approved' THEN ?4 ELSE NULL END
                 WHERE business_id = ?1 AND doc_id = ?2 AND version = ?5 AND status <> ?3",
                params![self.b(), doc_id, status, t, expected_version],
            )
            .map_err(err)?;
        if n == 0 {
            return Err("this document changed since you opened it, already has that status, or doesn't exist".into());
        }
        let d = self.document_get(doc_id)?.ok_or("document vanished")?;
        self.note(
            "business.document_changed",
            &biz_ref(&self.id, "document", Some(doc_id)),
            actor,
            &format!("Document {}: now {status}", d.title),
        );
        Ok(d)
    }

    /// Case-insensitive search over this business's context and searchable documents. Archived
    /// material is left out; each hit says its status.
    pub(crate) fn knowledge_search(
        &self,
        query: &str,
    ) -> Result<Vec<BusinessKnowledgeHit>, String> {
        let q = query.trim();
        if q.chars().count() < 2 {
            return Err("search needs at least 2 characters".into());
        }
        text_ok(q, 200, "search")?;
        let like = format!(
            "%{}%",
            q.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut hits = Vec::new();
        let mut st = self
            .conn
            .prepare(
                "SELECT item_id, title, section, status, body FROM context_item
                 WHERE business_id = ?1 AND status <> 'archived'
                   AND (title LIKE ?2 ESCAPE '\\' OR body LIKE ?2 ESCAPE '\\') LIMIT 50",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b(), like], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (id, title, section, status, body) = row.map_err(err)?;
            hits.push(BusinessKnowledgeHit {
                kind: "context".into(),
                excerpt: excerpt(&format!("{title}\n{body}"), q),
                id,
                title,
                section: Some(section),
                status,
            });
        }
        let mut st = self
            .conn
            .prepare(
                "SELECT doc_id, title, status, text FROM document
                 WHERE business_id = ?1 AND status <> 'archived' AND text IS NOT NULL
                   AND (title LIKE ?2 ESCAPE '\\' OR text LIKE ?2 ESCAPE '\\') LIMIT 50",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b(), like], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (id, title, status, text) = row.map_err(err)?;
            hits.push(BusinessKnowledgeHit {
                kind: "document".into(),
                excerpt: excerpt(&format!("{title}\n{text}"), q),
                id,
                title,
                section: None,
                status,
            });
        }
        Ok(hits)
    }

    // ── Permissions ──

    pub(crate) fn permissions(&self) -> Result<Vec<BusinessPermission>, String> {
        let mut st = self
            .conn
            .prepare("SELECT actor, capability, allowed, updated_at FROM permission WHERE business_id = ?1")
            .map_err(err)?;
        let mut rows = st
            .query_map(params![self.b()], |r| {
                let cap: String = r.get(1)?;
                Ok(BusinessPermission {
                    actor: r.get(0)?,
                    label: CAPABILITIES
                        .iter()
                        .find(|(c, _)| *c == cap)
                        .map(|(_, l)| (*l).to_string())
                        .unwrap_or_else(|| cap.clone()),
                    capability: cap,
                    allowed: r.get::<_, i64>(2)? == 1,
                    updated_at: r.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let order = |p: &BusinessPermission| {
            (
                ACTORS.iter().position(|a| *a == p.actor).unwrap_or(99),
                CAPABILITIES
                    .iter()
                    .position(|(c, _)| *c == p.capability)
                    .unwrap_or(99),
            )
        };
        rows.sort_by_key(order);
        Ok(rows)
    }

    pub(crate) fn set_permission(
        &self,
        actor: &str,
        capability: &str,
        allowed: bool,
    ) -> Result<Vec<BusinessPermission>, String> {
        check(ACTORS, actor, "actor")?;
        if !CAPABILITIES.iter().any(|(c, _)| *c == capability) {
            return Err(format!("capability: {capability} is not allowed"));
        }
        self.conn
            .execute(
                "UPDATE permission SET allowed = ?4, updated_at = ?5 WHERE business_id = ?1 AND actor = ?2 AND capability = ?3",
                params![self.b(), actor, capability, allowed as i64, now()],
            )
            .map_err(err)?;
        self.note(
            "business.permission_changed",
            &biz_ref(
                &self.id,
                "permission",
                Some(&format!("{actor}-{capability}")),
            ),
            "taisun",
            &format!(
                "{actor}: {capability} {}",
                if allowed { "allowed" } else { "not allowed" }
            ),
        );
        self.permissions()
    }

    pub(crate) fn allowed(&self, actor: &str, capability: &str) -> Result<bool, String> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT allowed FROM permission WHERE business_id = ?1 AND actor = ?2 AND capability = ?3",
                params![self.b(), actor, capability],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        Ok(v == Some(1))
    }

    /// The user is the owner and always allowed; any other actor needs the row set.
    pub(crate) fn require(&self, actor: &str, capability: &str) -> Result<(), String> {
        if actor == "user" || actor == "taisun" {
            return Ok(());
        }
        if self.allowed(actor, capability)? {
            Ok(())
        } else {
            Err(format!(
                "not permitted: {actor} may not {capability} for this business"
            ))
        }
    }

    // ── Proposals (business action approvals) ──

    /// Amara (or a job) proposes a change to an existing item. It does nothing until the user
    /// approves it in the native dialog (`ops.rs`).
    pub(crate) fn proposal_create(
        &self,
        item_id: &str,
        title: &str,
        body: &str,
        rationale: Option<&str>,
        proposed_by: &str,
    ) -> Result<BusinessProposal, String> {
        check(&["amara", "scheduler"], proposed_by, "proposed by")?;
        let actor = if proposed_by == "amara" {
            "model"
        } else {
            "scheduler"
        };
        self.require(actor, "propose_changes")?;
        let item = self
            .context_get(item_id)?
            .ok_or("no such item in this business")?;
        let title = title.trim();
        if title.is_empty() {
            return Err("a proposal needs a title".into());
        }
        text_ok(title, MAX_TITLE, "title")?;
        text_ok(body, MAX_BODY, "body")?;
        if let Some(r) = rationale {
            text_ok(r, 1000, "rationale")?;
        }
        if item.title == title && item.body == body {
            return Err("the proposal changes nothing".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.conn
            .execute(
                "INSERT INTO proposal (business_id, proposal_id, kind, item_id, base_version, title, body, rationale, status,
                    proposed_by, proposed_at)
                 VALUES (?1, ?2, 'context_update', ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
                params![self.b(), id, item_id, item.version, title, body, rationale, proposed_by, now()],
            )
            .map_err(err)?;
        self.proposal_get(&id)?
            .ok_or_else(|| "proposal vanished".into())
    }

    pub(crate) fn proposals(&self, status: Option<&str>) -> Result<Vec<BusinessProposal>, String> {
        if let Some(s) = status {
            check(&["pending", "applied", "rejected", "failed"], s, "status")?;
        }
        let mut st = self
            .conn
            .prepare(&format!(
                "SELECT {PROP_COLS} FROM proposal WHERE business_id = ?1 AND (?2 IS NULL OR status = ?2)
                 ORDER BY proposed_at DESC LIMIT 200"
            ))
            .map_err(err)?;
        let rows = st
            .query_map(params![self.b(), status], proposal_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn proposal_get(
        &self,
        proposal_id: &str,
    ) -> Result<Option<BusinessProposal>, String> {
        local_id_ok(proposal_id)?;
        self.conn
            .query_row(
                &format!(
                    "SELECT {PROP_COLS} FROM proposal WHERE business_id = ?1 AND proposal_id = ?2"
                ),
                params![self.b(), proposal_id],
                proposal_row,
            )
            .optional()
            .map_err(err)
    }

    /// pending → rejected | applied | failed, once.
    pub(crate) fn proposal_decide(
        &self,
        proposal_id: &str,
        status: &str,
        execution_id: Option<&str>,
        detail: Option<&str>,
    ) -> Result<BusinessProposal, String> {
        local_id_ok(proposal_id)?;
        check(&["applied", "rejected", "failed"], status, "status")?;
        let n = self
            .conn
            .execute(
                "UPDATE proposal SET status = ?3, decided_at = ?4, execution_id = ?5, detail = ?6
                 WHERE business_id = ?1 AND proposal_id = ?2 AND status = 'pending'",
                params![self.b(), proposal_id, status, now(), execution_id, detail],
            )
            .map_err(err)?;
        if n == 0 {
            return Err("no such pending proposal".into());
        }
        let p = self.proposal_get(proposal_id)?.ok_or("proposal vanished")?;
        self.note(
            "business.proposal_decided",
            &biz_ref(&self.id, "proposal", Some(proposal_id)),
            "taisun",
            &format!("Proposed change to \"{}\": {status}", p.title),
        );
        Ok(p)
    }

    /// Applies an approved proposal: the item gets the proposed text as a new, approved version
    /// (the user read the full text in the dialog). Refused when the item moved on since.
    pub(crate) fn apply_proposal(&self, proposal_id: &str) -> Result<BusinessContextItem, String> {
        let p = self.proposal_get(proposal_id)?.ok_or("no such proposal")?;
        if p.status != "pending" {
            return Err("this proposal was already decided".into());
        }
        let t = now();
        let tx = self.tx()?;
        let n = tx
            .execute(
                "UPDATE context_item SET title = ?3, body = ?4, status = 'approved', approved_at = ?5,
                    version = version + 1, updated_at = ?5
                 WHERE business_id = ?1 AND item_id = ?2 AND version = ?6",
                params![self.b(), p.item_id, p.title, p.body, t, p.base_version],
            )
            .map_err(err)?;
        if n == 0 {
            return Err("the item changed after this was proposed; nothing was applied".into());
        }
        version_row(
            &tx,
            self.b(),
            &p.item_id,
            "taisun",
            Some("approved proposal"),
        )?;
        tx.commit().map_err(err)?;
        let item = self.context_get(&p.item_id)?.ok_or("item vanished")?;
        self.note(
            "business.context_approved",
            &biz_ref(&self.id, "context", Some(&p.item_id)),
            "taisun",
            &format!("Approved Amara's change: {}", item.title),
        );
        Ok(item)
    }

    fn tx(&self) -> Result<rusqlite::Transaction<'_>, String> {
        // `Tenant` holds the only handle to this connection, so an immediate transaction is safe here.
        rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .map_err(err)
    }
}

fn version_row(
    tx: &Connection,
    b: &str,
    item_id: &str,
    actor: &str,
    note: Option<&str>,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO context_version (business_id, item_id, version, title, body, status, actor, note, at)
         SELECT business_id, item_id, version, title, body, status, ?3, ?4, ?5
         FROM context_item WHERE business_id = ?1 AND item_id = ?2",
        params![b, item_id, actor, note, now()],
    )
    .map_err(err)?;
    Ok(())
}

pub(crate) fn section_label(s: &str) -> &'static str {
    SECTIONS
        .iter()
        .find(|(k, _)| *k == s)
        .map(|(_, l)| *l)
        .unwrap_or("Context")
}

/// About 180 characters around the first case-insensitive match. Works on chars, never byte offsets.
fn excerpt(text: &str, q: &str) -> String {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let chars: Vec<char> = text.chars().collect();
    let hay: Vec<char> = chars.iter().map(|c| fold(*c)).collect();
    let needle: Vec<char> = q.chars().map(fold).collect();
    let at = if needle.is_empty() || needle.len() > hay.len() {
        0
    } else {
        (0..=hay.len() - needle.len())
            .find(|&i| hay[i..i + needle.len()] == needle[..])
            .unwrap_or(0)
    };
    let start = at.saturating_sub(60);
    let s: String = chars[start..].iter().take(180).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn safe_file_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "document".into()
    } else {
        cleaned
    }
}

fn kind_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "markdown",
        "txt" | "csv" | "json" | "html" | "htm" => "text",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "webp" | "gif" => "image",
        _ => "other",
    }
}

// ─── Schema ──────────────────────────────────────────────────────────────────

/// The v2 connector kinds (0064). `hostinger` became `website`: its job is the public site check.
pub(crate) const CONNECTOR_KINDS_SQL: &str =
    "'supabase','website','github','google_analytics','search_console','instagram','local_repo','drive_folder'";

/// The tables v2 adds; every one is tenant-scoped (the isolation pin lists them).
pub(crate) const V2_TABLES: &[&str] = &[
    "connector_check",
    "source_cursor",
    "site_page_view",
    "site_session",
    "site_event",
    "site_checkout",
    "aggregate_snapshot",
    "site_fact",
    "business_salt",
    "insight_note",
];

/// v1 → v2 (0064). A file that already holds workspace rows is copied first (`VACUUM INTO
/// <file>.v1-<timestamp>.bak`, 0600). The connector table is rebuilt (SQLite can't change a CHECK)
/// with foreign keys off, inside one transaction, and foreign keys are checked before it commits.
/// On any error nothing changes and the file stays v1.
fn migrate_to_v2(conn: &Connection) -> Result<(), String> {
    let rows: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM context_item) + (SELECT COUNT(*) FROM document) + (SELECT COUNT(*) FROM sync_run)",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if rows > 0 {
        let path: String = conn
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name = 'main'",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        let dest = format!("{path}.v1-{}.bak", Utc::now().format("%Y%m%dT%H%M%S%.6fZ"));
        crate::app_paths::ensure_private_file(Path::new(&dest)).map_err(err)?;
        conn.execute("VACUUM INTO ?1", params![dest]).map_err(|e| {
            err(format!(
                "backup before migration failed, nothing changed: {e}"
            ))
        })?;
    }
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(err)?;
    let result = (|| -> Result<(), String> {
        conn.execute_batch("BEGIN IMMEDIATE").map_err(err)?;
        let now_v: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if now_v.parse::<i64>().unwrap_or(0) >= 2 {
            return Ok(());
        }
        conn.execute_batch(&format!(
            "
            CREATE TABLE connector_v2 (
                business_id  TEXT NOT NULL REFERENCES business(business_id),
                connector_id TEXT NOT NULL,
                kind         TEXT NOT NULL CHECK (kind IN ({CONNECTOR_KINDS_SQL})),
                config_json  TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL,
                PRIMARY KEY (business_id, connector_id)
            );
            INSERT INTO connector_v2 (business_id, connector_id, kind, config_json, created_at, updated_at)
                SELECT business_id,
                       CASE connector_id WHEN 'hostinger' THEN 'website' ELSE connector_id END,
                       CASE kind WHEN 'hostinger' THEN 'website' ELSE kind END,
                       config_json, created_at, updated_at FROM connector;
            UPDATE sync_run SET connector_id = 'website' WHERE connector_id = 'hostinger';
            DROP TABLE connector;
            ALTER TABLE connector_v2 RENAME TO connector;
            CREATE TRIGGER IF NOT EXISTS connector_tenant_fixed BEFORE UPDATE OF business_id ON connector
                BEGIN SELECT RAISE(ABORT, 'business_id is fixed'); END;

            -- The last real check of a connector that can't be checked on every read (network ones).
            CREATE TABLE connector_check (
                business_id  TEXT NOT NULL,
                connector_id TEXT NOT NULL,
                outcome      TEXT NOT NULL CHECK (outcome IN ('ok','auth','error')),
                detail       TEXT NOT NULL,
                checked_at   TEXT NOT NULL,
                PRIMARY KEY (business_id, connector_id),
                FOREIGN KEY (business_id, connector_id) REFERENCES connector(business_id, connector_id)
            );
            -- Where the last sync of each stream stopped (keyset pagination resumes after it).
            CREATE TABLE source_cursor (
                business_id  TEXT NOT NULL,
                connector_id TEXT NOT NULL,
                stream       TEXT NOT NULL,
                cursor_at    TEXT NOT NULL,
                cursor_id    TEXT NOT NULL,
                updated_at   TEXT NOT NULL,
                PRIMARY KEY (business_id, connector_id, stream),
                FOREIGN KEY (business_id, connector_id) REFERENCES connector(business_id, connector_id)
            );
            -- First-party site analytics, pseudonymous only: no user ids, emails, user agents or query strings.
            CREATE TABLE site_page_view (
                business_id   TEXT NOT NULL REFERENCES business(business_id),
                source_id     TEXT NOT NULL,
                path          TEXT NOT NULL,
                referrer_host TEXT,
                session_key   TEXT,
                at            TEXT NOT NULL,
                run_id        TEXT NOT NULL,
                PRIMARY KEY (business_id, source_id)
            );
            CREATE TABLE site_session (
                business_id  TEXT NOT NULL REFERENCES business(business_id),
                source_id    TEXT NOT NULL,
                session_key  TEXT,
                entry_path   TEXT,
                exit_path    TEXT,
                time_spent_s INTEGER,
                bounced      INTEGER,
                started_at   TEXT NOT NULL,
                ended_at     TEXT,
                run_id       TEXT NOT NULL,
                PRIMARY KEY (business_id, source_id)
            );
            CREATE TABLE site_event (
                business_id TEXT NOT NULL REFERENCES business(business_id),
                source_id   TEXT NOT NULL,
                event_name  TEXT NOT NULL,
                page        TEXT,
                quiz_step   INTEGER,
                session_key TEXT,
                at          TEXT NOT NULL,
                run_id      TEXT NOT NULL,
                PRIMARY KEY (business_id, source_id)
            );
            CREATE TABLE site_checkout (
                business_id TEXT NOT NULL REFERENCES business(business_id),
                source_id   TEXT NOT NULL,
                event_type  TEXT NOT NULL,
                session_key TEXT,
                cart_value  REAL,
                products    TEXT NOT NULL,
                at          TEXT NOT NULL,
                run_id      TEXT NOT NULL,
                PRIMARY KEY (business_id, source_id)
            );
            -- Customer information as aggregates only, computed in the source database. A group of
            -- fewer than 5 is stored suppressed (value and n NULL), never as its count.
            CREATE TABLE aggregate_snapshot (
                business_id  TEXT NOT NULL REFERENCES business(business_id),
                connector_id TEXT NOT NULL,
                metric       TEXT NOT NULL,
                dimension    TEXT NOT NULL,
                value        REAL,
                n            INTEGER,
                suppressed   INTEGER NOT NULL CHECK (suppressed IN (0,1)),
                source_query TEXT NOT NULL,
                run_id       TEXT NOT NULL,
                captured_at  TEXT NOT NULL,
                PRIMARY KEY (business_id, connector_id, metric, dimension),
                CHECK (suppressed = 0 OR (value IS NULL AND n IS NULL))
            );
            -- Website and deployment facts (sitemap size, last-modified, latest commit …), one per key.
            CREATE TABLE site_fact (
                business_id  TEXT NOT NULL REFERENCES business(business_id),
                connector_id TEXT NOT NULL,
                key          TEXT NOT NULL,
                value        TEXT NOT NULL,
                source_ref   TEXT NOT NULL,
                run_id       TEXT NOT NULL,
                captured_at  TEXT NOT NULL,
                PRIMARY KEY (business_id, connector_id, key)
            );
            -- A random value per business so session ids are kept only as keyed hashes.
            CREATE TABLE business_salt (
                business_id TEXT PRIMARY KEY REFERENCES business(business_id),
                salt        TEXT NOT NULL
            );
            -- Amara's interpretations and hypotheses about a report, each tied to the insights it used.
            CREATE TABLE insight_note (
                business_id TEXT NOT NULL REFERENCES business(business_id),
                note_id     TEXT NOT NULL,
                report      TEXT NOT NULL CHECK (report IN ('website','customers')),
                kind        TEXT NOT NULL CHECK (kind IN ('interpretation','hypothesis')),
                statement   TEXT NOT NULL,
                based_on    TEXT NOT NULL,
                model_tier  TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                PRIMARY KEY (business_id, note_id)
            );
            "
        ))
        .map_err(err)?;
        for t in V2_TABLES {
            conn.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {t}_tenant_fixed BEFORE UPDATE OF business_id ON {t}
                     BEGIN SELECT RAISE(ABORT, 'business_id is fixed'); END;"
            ))
            .map_err(err)?;
        }
        // Every existing business gets every registered connector, disconnected.
        let t = now();
        for def in connectors::DEFINITIONS {
            conn.execute(
                "INSERT OR IGNORE INTO connector (business_id, connector_id, kind, config_json, created_at, updated_at)
                 SELECT business_id, ?1, ?1, '{}', ?2, ?2 FROM business",
                params![def.kind, t],
            )
            .map_err(err)?;
        }
        conn.execute(
            "UPDATE schema_meta SET value = '2' WHERE key = 'schema_version'",
            [],
        )
        .map_err(err)?;
        Ok(())
    })();
    let finish = match result {
        Ok(()) => {
            let bad: i64 = conn
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })
                .map_err(err)?;
            if bad > 0 {
                let _ = conn.execute_batch("ROLLBACK");
                Err(err(
                    "migration to v2 left broken references; nothing changed",
                ))
            } else if conn.is_autocommit() {
                Ok(())
            } else {
                conn.execute_batch("COMMIT").map_err(err)
            }
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    };
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(err)?;
    finish
}

fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS business (
            business_id TEXT PRIMARY KEY
                CHECK (length(business_id) BETWEEN 2 AND 40 AND business_id NOT GLOB '*[^a-z0-9-]*'),
            name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
            description TEXT,
            venture_id  TEXT,
            status      TEXT NOT NULL CHECK (status IN ('active','archived')),
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS context_item (
            business_id TEXT NOT NULL REFERENCES business(business_id),
            item_id     TEXT NOT NULL,
            section     TEXT NOT NULL CHECK (section IN ('identity','products','formulations','ingredients',
                            'audiences','positioning','objectives','website','documents','decisions')),
            title       TEXT NOT NULL,
            body        TEXT NOT NULL,
            status      TEXT NOT NULL CHECK (status IN ('draft','unverified','approved','archived')),
            source      TEXT NOT NULL CHECK (source IN ('user','import','sync','amara')),
            source_ref  TEXT,
            version     INTEGER NOT NULL CHECK (version >= 1),
            approved_at TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            PRIMARY KEY (business_id, item_id),
            CHECK ((status = 'approved') = (approved_at IS NOT NULL))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_context_source ON context_item(business_id, source_ref)
            WHERE source_ref IS NOT NULL;
        CREATE TABLE IF NOT EXISTS context_version (
            business_id TEXT NOT NULL,
            item_id     TEXT NOT NULL,
            version     INTEGER NOT NULL,
            title       TEXT NOT NULL,
            body        TEXT NOT NULL,
            status      TEXT NOT NULL,
            actor       TEXT NOT NULL CHECK (actor IN ('taisun','amara','system')),
            note        TEXT,
            at          TEXT NOT NULL,
            PRIMARY KEY (business_id, item_id, version),
            FOREIGN KEY (business_id, item_id) REFERENCES context_item(business_id, item_id)
        );
        CREATE TRIGGER IF NOT EXISTS context_version_append_only BEFORE UPDATE ON context_version
            BEGIN SELECT RAISE(ABORT, 'context history is append-only'); END;
        CREATE TABLE IF NOT EXISTS memory (
            business_id TEXT NOT NULL REFERENCES business(business_id),
            memory_id   TEXT NOT NULL,
            text        TEXT NOT NULL,
            status      TEXT NOT NULL CHECK (status IN ('proposed','kept','dismissed')),
            source      TEXT NOT NULL CHECK (source IN ('user','amara')),
            created_at  TEXT NOT NULL,
            decided_at  TEXT,
            PRIMARY KEY (business_id, memory_id)
        );
        CREATE TABLE IF NOT EXISTS document (
            business_id TEXT NOT NULL REFERENCES business(business_id),
            doc_id      TEXT NOT NULL,
            title       TEXT NOT NULL,
            kind        TEXT NOT NULL CHECK (kind IN ('markdown','text','pdf','image','other')),
            status      TEXT NOT NULL CHECK (status IN ('draft','unverified','approved','archived')),
            file_name   TEXT NOT NULL,
            sha256      TEXT NOT NULL,
            bytes       INTEGER NOT NULL,
            source      TEXT NOT NULL CHECK (source IN ('user','sync')),
            source_ref  TEXT,
            text        TEXT,
            version     INTEGER NOT NULL CHECK (version >= 1),
            approved_at TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            PRIMARY KEY (business_id, doc_id),
            CHECK ((status = 'approved') = (approved_at IS NOT NULL))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_document_source ON document(business_id, source_ref)
            WHERE source_ref IS NOT NULL;
        CREATE TABLE IF NOT EXISTS connector (
            business_id  TEXT NOT NULL REFERENCES business(business_id),
            connector_id TEXT NOT NULL,
            kind         TEXT NOT NULL CHECK (kind IN ('supabase','hostinger','instagram','local_repo')),
            config_json  TEXT NOT NULL,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL,
            PRIMARY KEY (business_id, connector_id)
        );
        CREATE TABLE IF NOT EXISTS sync_run (
            business_id   TEXT NOT NULL,
            run_id        TEXT NOT NULL,
            connector_id  TEXT NOT NULL,
            started_at    TEXT NOT NULL,
            finished_at   TEXT,
            status        TEXT NOT NULL CHECK (status IN ('running','succeeded','failed','interrupted')),
            items_seen    INTEGER NOT NULL DEFAULT 0,
            items_changed INTEGER NOT NULL DEFAULT 0,
            conflicts     INTEGER NOT NULL DEFAULT 0,
            detail        TEXT,
            PRIMARY KEY (business_id, run_id),
            FOREIGN KEY (business_id, connector_id) REFERENCES connector(business_id, connector_id)
        );
        CREATE TABLE IF NOT EXISTS permission (
            business_id TEXT NOT NULL REFERENCES business(business_id),
            actor       TEXT NOT NULL CHECK (actor IN ('model','scheduler','integration')),
            capability  TEXT NOT NULL,
            allowed     INTEGER NOT NULL CHECK (allowed IN (0,1)),
            updated_at  TEXT NOT NULL,
            PRIMARY KEY (business_id, actor, capability)
        );
        CREATE TABLE IF NOT EXISTS proposal (
            business_id  TEXT NOT NULL,
            proposal_id  TEXT NOT NULL,
            kind         TEXT NOT NULL CHECK (kind IN ('context_update')),
            item_id      TEXT NOT NULL,
            base_version INTEGER NOT NULL,
            title        TEXT NOT NULL,
            body         TEXT NOT NULL,
            rationale    TEXT,
            status       TEXT NOT NULL CHECK (status IN ('pending','applied','rejected','failed')),
            proposed_by  TEXT NOT NULL CHECK (proposed_by IN ('amara','scheduler')),
            proposed_at  TEXT NOT NULL,
            decided_at   TEXT,
            execution_id TEXT,
            detail       TEXT,
            PRIMARY KEY (business_id, proposal_id),
            FOREIGN KEY (business_id, item_id) REFERENCES context_item(business_id, item_id)
        );
        ",
    )
    .map_err(err)?;
    // business_id never changes on any row (tenant isolation also holds against a direct UPDATE).
    for t in [
        "business",
        "context_item",
        "context_version",
        "memory",
        "document",
        "connector",
        "sync_run",
        "permission",
        "proposal",
    ] {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS {t}_tenant_fixed BEFORE UPDATE OF business_id ON {t}
                 BEGIN SELECT RAISE(ABORT, 'business_id is fixed'); END;"
        ))
        .map_err(err)?;
    }
    let found: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let version = match found.and_then(|v| v.parse::<i64>().ok()) {
        None => {
            // A new file starts at v1 (the tables above) and is migrated forward like any other.
            conn.execute(
                "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', '1')",
                [],
            )
            .map_err(err)?;
            1
        }
        Some(v) if v > SCHEMA_VERSION => {
            return Err(format!(
                "workspace store schema {v} is newer than this app ({SCHEMA_VERSION}); not touched"
            ));
        }
        Some(v) => v,
    };
    if version < 2 {
        migrate_to_v2(conn)?;
    }
    if version < 3 {
        // v3 (0065): website operations tables. Additive and idempotent, in one transaction.
        conn.execute_batch("BEGIN IMMEDIATE").map_err(err)?;
        let r = (|| {
            conn.execute_batch(site_ops::V3_SQL).map_err(err)?;
            for t in site_ops::V3_TABLES {
                conn.execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS {t}_tenant_fixed BEFORE UPDATE OF business_id ON {t}
                         BEGIN SELECT RAISE(ABORT, 'business_id is fixed'); END;"
                ))
                .map_err(err)?;
            }
            conn.execute(
                "UPDATE schema_meta SET value = '3' WHERE key = 'schema_version'",
                [],
            )
            .map_err(err)?;
            Ok::<_, String>(())
        })();
        match r {
            Ok(()) => conn.execute_batch("COMMIT").map_err(err)?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    // A sync that was running when the app stopped can't still be running.
    conn.execute(
        "UPDATE sync_run SET status = 'interrupted', finished_at = ?1, detail = 'The app stopped during this sync.'
         WHERE status = 'running' AND started_at < ?2",
        params![now(), (Utc::now() - chrono::Duration::minutes(10)).to_rfc3339()],
    )
    .map_err(err)?;
    Ok(())
}
