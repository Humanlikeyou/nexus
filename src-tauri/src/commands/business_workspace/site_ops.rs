//! Website operations (decision 0065, Phase 13). The workflow, stage by stage:
//!
//! 1–3 **Locate, approved information, current content.** The live site is the source of truth
//!     (Taisun: "taiyoskin.com"): a snapshot downloads the public files (index.html, robots.txt,
//!     sitemap.xml and every same-site file index.html and its CSS link to) over `net.rs`.
//!     Approved workspace context is searched alongside.
//! 4–6 **Propose, compare, validate.** A change is computed from the snapshot's bytes: SEO title or
//!     description in index.html, robots.txt, sitemap.xml, replacing an image with an approved
//!     document, or deleting a file. A line diff, checks (lengths, structure, links, sitemap and robots
//!     syntax) and sensitivity flags (pricing, formulation, customer communication, deletion).
//!     Content compiled into the JavaScript bundle (product pages, descriptions, ingredients, FAQs,
//!     blog, landing pages) is refused: no source that reproduces the live build is confirmed.
//! 7   **Preview** is rendered in the app (no listener: the Rust rules forbid new listeners).
//! 8   **Approval** only in the native dialog, listing every flag. A model can draft, never approve.
//! 9   **Backup:** a fresh snapshot right before packaging (refused if the live file changed since the
//!     draft), plus Taisun's confirmation that he took an hPanel backup.
//! 10  **Publish:** Taisun uploads the package in Hostinger's File Manager (decision 0065). Nothing
//!     here writes to the website.
//! 11  **Verify:** the live file is fetched and its hash compared with the approved bytes.
//! 12  **Audit:** every transition is an append-only `site_change_event` and a business activity row.
//! 13  **Rollback:** the package holds the backed-up files; after Taisun uploads them, verification
//!     checks the live file matches the backup.
//!
//! Never overwrites website files: there is no write path to the site at all.

use super::net::{fetch, Request, Sleep, Transport};
use super::{err, local_id_ok, now, sha256_hex, Tenant};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 200;
const MAX_FILE: usize = 5 * 1024 * 1024;
const MAX_TOTAL: usize = 25 * 1024 * 1024;
pub(crate) const BUNDLED_TYPES: &[&str] = &[
    "product_page",
    "product_description",
    "ingredients",
    "faq",
    "blog",
    "landing_page",
];

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteFile {
    pub path: String,
    pub sha256: String,
    pub bytes: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteSnapshot {
    pub snapshot_id: String,
    pub created_at: String,
    pub files: Vec<BusinessSiteFile>,
    /// What a public download can't include.
    pub limits: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessDiffLine {
    /// " " | "-" | "+".
    pub op: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteCheck {
    /// ok | warn | error.
    pub level: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteChange {
    pub change_id: String,
    pub content_type: String,
    pub target: String,
    pub title: String,
    /// draft | approved | packaged | verified | verify_failed | rolled_back | rollback_failed | rejected.
    pub status: String,
    pub before: String,
    pub after: String,
    pub diff: Vec<BusinessDiffLine>,
    pub checks: Vec<BusinessSiteCheck>,
    /// pricing | formulation | customer_communication | deletion.
    pub flags: Vec<String>,
    pub source_refs: Vec<String>,
    pub base_snapshot: String,
    pub hpanel_backup_confirmed: bool,
    pub package_dir: Option<String>,
    pub verify_detail: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteEvent {
    pub seq: i64,
    pub at: String,
    pub from_status: Option<String>,
    pub to_status: String,
    pub actor: String,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteMatch {
    /// site | context.
    pub kind: String,
    pub reference: String,
    pub excerpt: String,
    pub status: Option<String>,
}

#[derive(Deserialize, Clone, Debug, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessSiteChangeInput {
    /// seo_title | seo_description | robots | sitemap | asset_replace | asset_delete (or a bundled type, refused).
    pub content_type: String,
    /// The text for text changes; empty for assets.
    pub value: String,
    /// The file path for asset changes ("/favicon.png").
    pub path: Option<String>,
    /// An approved workspace document supplying the new asset bytes.
    pub document_id: Option<String>,
    /// Approved context items the text is based on.
    pub source_refs: Vec<String>,
}

// ─── Schema (workspace store v3, additive) ───────────────────────────────────

pub(crate) const V3_SQL: &str = "
    CREATE TABLE IF NOT EXISTS site_snapshot (
        business_id TEXT NOT NULL REFERENCES business(business_id),
        snapshot_id TEXT NOT NULL,
        created_at  TEXT NOT NULL,
        manifest    TEXT NOT NULL,
        PRIMARY KEY (business_id, snapshot_id)
    );
    CREATE TABLE IF NOT EXISTS site_change (
        business_id  TEXT NOT NULL REFERENCES business(business_id),
        change_id    TEXT NOT NULL,
        content_type TEXT NOT NULL,
        target       TEXT NOT NULL,
        title        TEXT NOT NULL,
        status       TEXT NOT NULL CHECK (status IN ('draft','approved','packaged','verified','verify_failed','rolled_back','rollback_failed','rejected')),
        before_text  TEXT NOT NULL,
        after_text   TEXT NOT NULL,
        before_sha   TEXT,
        after_sha    TEXT,
        diff_json    TEXT NOT NULL,
        checks_json  TEXT NOT NULL,
        flags_json   TEXT NOT NULL,
        sources_json TEXT NOT NULL,
        base_snapshot TEXT NOT NULL,
        hpanel_backup INTEGER NOT NULL DEFAULT 0,
        approval_sha TEXT,
        package_dir  TEXT,
        verify_detail TEXT,
        created_at   TEXT NOT NULL,
        updated_at   TEXT NOT NULL,
        PRIMARY KEY (business_id, change_id),
        FOREIGN KEY (business_id, base_snapshot) REFERENCES site_snapshot(business_id, snapshot_id)
    );
    CREATE TABLE IF NOT EXISTS site_change_event (
        business_id TEXT NOT NULL,
        change_id   TEXT NOT NULL,
        seq         INTEGER NOT NULL,
        at          TEXT NOT NULL,
        from_status TEXT,
        to_status   TEXT NOT NULL,
        actor       TEXT NOT NULL,
        detail      TEXT NOT NULL,
        PRIMARY KEY (business_id, change_id, seq),
        FOREIGN KEY (business_id, change_id) REFERENCES site_change(business_id, change_id)
    );
    CREATE TRIGGER IF NOT EXISTS site_change_event_append_only BEFORE UPDATE ON site_change_event
        BEGIN SELECT RAISE(ABORT, 'site history is append-only'); END;
    CREATE TRIGGER IF NOT EXISTS site_change_event_no_delete BEFORE DELETE ON site_change_event
        BEGIN SELECT RAISE(ABORT, 'site history is append-only'); END;
";
pub(crate) const V3_TABLES: &[&str] = &["site_snapshot", "site_change", "site_change_event"];

// ─── Pure helpers ────────────────────────────────────────────────────────────

/// A same-site path from an href/src, without query or fragment; None for other sites and data URLs.
pub(crate) fn site_path(link: &str) -> Option<String> {
    let l = link.trim();
    let l = l
        .strip_prefix("https://taiyoskin.com")
        .or_else(|| l.strip_prefix("https://www.taiyoskin.com"))
        .unwrap_or(l);
    if !l.starts_with('/') || l.starts_with("//") {
        return None;
    }
    let p = l.split(['?', '#']).next()?.to_string();
    (!p.contains("..") && p.len() <= 300).then_some(p)
}

/// `src="…"`, `href="…"` and CSS `url(…)` values.
pub(crate) fn links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["src=\"", "href=\"", "content=\"https://"] {
        for (i, _) in text.match_indices(key) {
            let start = i + key.len() - if key.starts_with("content") { 8 } else { 0 };
            if let Some(end) = text[start..].find('"') {
                out.push(text[start..start + end].to_string());
            }
        }
    }
    for (i, _) in text.match_indices("url(") {
        if let Some(end) = text[i + 4..].find(')') {
            out.push(
                text[i + 4..i + 4 + end]
                    .trim_matches(['\'', '"', ' '])
                    .to_string(),
            );
        }
    }
    out
}

/// A line diff (LCS), with up to 3 unchanged lines of context around each change.
pub(crate) fn diff(before: &str, after: &str) -> Vec<BusinessDiffLine> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let (n, m) = (a.len(), b.len());
    if n * m > 4_000_000 {
        return vec![
            BusinessDiffLine {
                op: "-".into(),
                text: format!("({n} lines)"),
            },
            BusinessDiffLine {
                op: "+".into(),
                text: format!("({m} lines)"),
            },
        ];
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut all = Vec::new();
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            all.push((" ", a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            all.push(("+", b[j]));
            j += 1;
        } else {
            all.push(("-", a[i]));
            i += 1;
        }
    }
    let near = |k: usize| {
        all.iter()
            .enumerate()
            .any(|(x, (op, _))| *op != " " && x.abs_diff(k) <= 3)
    };
    all.iter()
        .enumerate()
        .filter(|(k, (op, _))| *op != " " || near(*k))
        .map(|(_, (op, t))| BusinessDiffLine {
            op: (*op).into(),
            text: t.chars().take(400).collect(),
        })
        .collect()
}

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Replaces the content of every `<meta (name|property)="key" content="…">`; returns how many.
fn set_meta(html: &str, key: &str, value: &str) -> (String, usize) {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut n = 0;
    while let Some(i) = rest.find("<meta") {
        let end = match rest[i..].find('>') {
            Some(e) => i + e + 1,
            None => break,
        };
        let tag = &rest[i..end];
        out.push_str(&rest[..i]);
        if tag.contains(&format!("name=\"{key}\"")) || tag.contains(&format!("property=\"{key}\""))
        {
            if let Some(c) = tag.find("content=\"") {
                let s = c + 9;
                if let Some(e) = tag[s..].find('"') {
                    out.push_str(&tag[..s]);
                    out.push_str(&attr_escape(value));
                    out.push_str(&tag[s + e..]);
                    n += 1;
                    rest = &rest[end..];
                    continue;
                }
            }
        }
        out.push_str(tag);
        rest = &rest[end..];
    }
    out.push_str(rest);
    (out, n)
}

/// Pricing, formulation, customer-communication and deletion flags from the added and removed lines.
pub(crate) fn flags(
    content_type: &str,
    d: &[BusinessDiffLine],
    ingredient_words: &[String],
) -> Vec<String> {
    let changed: String = d
        .iter()
        .filter(|l| l.op != " ")
        .map(|l| l.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let mut f = Vec::new();
    let price = ["$", "¥", "€", "usd", "jpy", "price", "円"]
        .iter()
        .any(|k| changed.contains(k));
    if price {
        f.push("pricing".to_string());
    }
    let formulation = [
        "formula",
        "formulation",
        "concentration",
        "niacinamide",
        "spf",
        "active ingredient",
        "ingredient",
    ]
    .iter()
    .any(|k| changed.contains(k))
        || changed.contains('%')
        || ingredient_words
            .iter()
            .any(|w| w.len() > 3 && changed.contains(&w.to_lowercase()));
    if formulation {
        f.push("formulation".to_string());
    }
    if ["mailto:", "newsletter", "unsubscribe", "email", "subscribe"]
        .iter()
        .any(|k| changed.contains(k))
    {
        f.push("customer_communication".to_string());
    }
    let removed_urls = d.iter().any(|l| l.op == "-" && l.text.contains("<loc>"));
    let blocks = d
        .iter()
        .any(|l| l.op == "+" && l.text.trim().to_lowercase().starts_with("disallow:"));
    if content_type == "asset_delete" || removed_urls || blocks {
        f.push("deletion".to_string());
    }
    f
}

fn chk(level: &str, text: impl Into<String>) -> BusinessSiteCheck {
    BusinessSiteCheck {
        level: level.into(),
        text: text.into(),
    }
}

// ─── Snapshots ───────────────────────────────────────────────────────────────

impl Tenant {
    fn site_dir(&self) -> PathBuf {
        self.data_path()
            .join("business")
            .join("workspaces")
            .join(self.id().as_str())
            .join("site")
    }

    fn snapshot_file(&self, snapshot: &str, path: &str) -> Result<Vec<u8>, String> {
        local_id_ok(snapshot)?;
        if path.contains("..") || !path.starts_with('/') {
            return Err("bad path".into());
        }
        let p = self
            .site_dir()
            .join("snapshots")
            .join(snapshot)
            .join(path.trim_start_matches('/').replace('/', "__"));
        std::fs::read(p).map_err(|_| format!("{path} isn't in the snapshot"))
    }

    pub(crate) fn snapshot_manifest(
        &self,
        snapshot: &str,
    ) -> Result<Vec<BusinessSiteFile>, String> {
        let m: String = self
            .conn()
            .query_row(
                "SELECT manifest FROM site_snapshot WHERE business_id = ?1 AND snapshot_id = ?2",
                params![self.id().as_str(), snapshot],
                |r| r.get(0),
            )
            .map_err(|_| "no such snapshot".to_string())?;
        Ok(serde_json::from_str::<Vec<(String, String, i64)>>(&m)
            .unwrap_or_default()
            .into_iter()
            .map(|(path, sha256, bytes)| BusinessSiteFile {
                path,
                sha256,
                bytes,
            })
            .collect())
    }

    pub(crate) fn latest_snapshot(&self) -> Result<Option<String>, String> {
        self.conn()
            .query_row("SELECT snapshot_id FROM site_snapshot WHERE business_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1", params![self.id().as_str()], |r| r.get(0))
            .optional()
            .map_err(err)
    }
}

pub(crate) const SNAPSHOT_LIMITS: &str = "A public download holds only files the site links to: it can't include server-only files (such as .htaccess), images referenced only from the JavaScript bundle, or anything not linked. Take an hPanel backup as well before uploading.";

/// Stage 3 and 9: downloads the live site's public files into a dated, hashed snapshot.
pub(crate) fn snapshot_live(
    t: &Tenant,
    http: &dyn Transport,
    sleep: Sleep,
) -> Result<BusinessSiteSnapshot, String> {
    let domain = t
        .connector_config_value("website", "domain")?
        .ok_or("Set the Website connector's domain first.")?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = t.site_dir().join("snapshots").join(&id);
    crate::app_paths::create_private_dir_all(&dir).map_err(err)?;
    let mut queue: Vec<String> = vec!["/".into(), "/robots.txt".into(), "/sitemap.xml".into()];
    let mut seen: Vec<String> = Vec::new();
    let mut manifest: Vec<(String, String, i64)> = Vec::new();
    let mut total = 0;
    while let Some(path) = queue.pop() {
        if seen.contains(&path) || seen.len() >= MAX_FILES {
            continue;
        }
        seen.push(path.clone());
        let r = fetch(
            http,
            sleep,
            &Request {
                url: format!("https://{domain}{path}"),
                headers: vec![],
            },
            MAX_FILE,
        )?;
        if !(200..300).contains(&r.status) {
            continue;
        }
        total += r.body.len();
        if total > MAX_TOTAL {
            return Err("the site is over 25 MB; snapshot stopped".into());
        }
        let stored = if path == "/" {
            "/index.html".to_string()
        } else {
            path.clone()
        };
        let file = dir.join(stored.trim_start_matches('/').replace('/', "__"));
        crate::app_paths::write_private(&file, &r.body).map_err(err)?;
        if sha256_hex(&std::fs::read(&file).map_err(err)?) != sha256_hex(&r.body) {
            return Err("a snapshot file didn't read back; snapshot stopped".into());
        }
        manifest.push((stored.clone(), sha256_hex(&r.body), r.body.len() as i64));
        if stored.ends_with(".html") || stored.ends_with(".css") {
            for l in links(&String::from_utf8_lossy(&r.body)) {
                if let Some(p) = site_path(&l) {
                    if !seen.contains(&p) && p != "/" {
                        queue.push(p);
                    }
                }
            }
        }
    }
    manifest.sort();
    let at = now();
    t.conn()
        .execute("INSERT INTO site_snapshot (business_id, snapshot_id, created_at, manifest) VALUES (?1, ?2, ?3, ?4)", params![t.id().as_str(), id, at, json!(manifest).to_string()])
        .map_err(err)?;
    super::note_activity(
        &t.data_path(),
        "business.sync_finished",
        &super::biz_ref(t.id(), "site_snapshot", Some(&id)),
        "taisun",
        &format!("Live site snapshot: {} files", manifest.len()),
    );
    Ok(BusinessSiteSnapshot {
        snapshot_id: id,
        created_at: at,
        files: manifest
            .into_iter()
            .map(|(path, sha256, bytes)| BusinessSiteFile {
                path,
                sha256,
                bytes,
            })
            .collect(),
        limits: SNAPSHOT_LIMITS.into(),
    })
}

/// Stage 1–2: where a term appears in the latest snapshot's text files and in approved context.
pub(crate) fn locate(t: &Tenant, query: &str) -> Result<Vec<BusinessSiteMatch>, String> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 2 {
        return Err("search needs at least 2 characters".into());
    }
    let mut out = Vec::new();
    if let Some(snap) = t.latest_snapshot()? {
        for f in t.snapshot_manifest(&snap)? {
            if !(f.path.ends_with(".html")
                || f.path.ends_with(".xml")
                || f.path.ends_with(".txt")
                || f.path.ends_with(".js"))
            {
                if f.path.to_lowercase().contains(&q) {
                    out.push(BusinessSiteMatch {
                        kind: "site".into(),
                        reference: f.path.clone(),
                        excerpt: format!("file, {} bytes", f.bytes),
                        status: None,
                    });
                }
                continue;
            }
            let text = String::from_utf8_lossy(&t.snapshot_file(&snap, &f.path)?).into_owned();
            if let Some(i) = text.to_lowercase().find(&q) {
                let start = text[..i]
                    .char_indices()
                    .rev()
                    .nth(60)
                    .map(|(x, _)| x)
                    .unwrap_or(0);
                let ex: String = text[start..].chars().take(160).collect();
                let editable =
                    f.path == "/index.html" || f.path == "/robots.txt" || f.path == "/sitemap.xml";
                out.push(BusinessSiteMatch {
                    kind: "site".into(),
                    reference: f.path.clone(),
                    excerpt: ex.split_whitespace().collect::<Vec<_>>().join(" "),
                    status: Some(if editable {
                        "editable".into()
                    } else {
                        "compiled into the bundle: not editable yet".into()
                    }),
                });
            }
        }
    }
    for i in t
        .context_list(None)?
        .into_iter()
        .filter(|i| i.status == "approved")
    {
        if format!("{} {}", i.title, i.body)
            .to_lowercase()
            .contains(&q)
        {
            out.push(BusinessSiteMatch {
                kind: "context".into(),
                reference: i.item_id.clone(),
                excerpt: format!(
                    "{}: {}",
                    i.title,
                    i.body.chars().take(140).collect::<String>()
                ),
                status: Some("approved".into()),
            });
        }
    }
    Ok(out)
}

// ─── Changes ─────────────────────────────────────────────────────────────────

const CH_COLS: &str = "change_id, content_type, target, title, status, before_text, after_text, diff_json, checks_json, flags_json, sources_json, base_snapshot, hpanel_backup, package_dir, verify_detail, created_at, updated_at";
fn ch_row(r: &rusqlite::Row) -> rusqlite::Result<BusinessSiteChange> {
    let j = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    Ok(BusinessSiteChange {
        change_id: r.get(0)?,
        content_type: r.get(1)?,
        target: r.get(2)?,
        title: r.get(3)?,
        status: r.get(4)?,
        before: r.get(5)?,
        after: r.get(6)?,
        diff: serde_json::from_str::<Vec<(String, String)>>(&j(7)?)
            .unwrap_or_default()
            .into_iter()
            .map(|(op, text)| BusinessDiffLine { op, text })
            .collect(),
        checks: serde_json::from_str::<Vec<(String, String)>>(&j(8)?)
            .unwrap_or_default()
            .into_iter()
            .map(|(level, text)| BusinessSiteCheck { level, text })
            .collect(),
        flags: serde_json::from_str(&j(9)?).unwrap_or_default(),
        source_refs: serde_json::from_str(&j(10)?).unwrap_or_default(),
        base_snapshot: r.get(11)?,
        hpanel_backup_confirmed: r.get::<_, i64>(12)? == 1,
        package_dir: r.get(13)?,
        verify_detail: r.get(14)?,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
    })
}

impl Tenant {
    pub(crate) fn site_change(&self, change_id: &str) -> Result<BusinessSiteChange, String> {
        local_id_ok(change_id)?;
        self.conn()
            .query_row(
                &format!(
                    "SELECT {CH_COLS} FROM site_change WHERE business_id = ?1 AND change_id = ?2"
                ),
                params![self.id().as_str(), change_id],
                ch_row,
            )
            .map_err(|_| "no such change in this business".to_string())
    }

    pub(crate) fn site_changes(&self) -> Result<Vec<BusinessSiteChange>, String> {
        let mut st = self.conn().prepare(&format!("SELECT {CH_COLS} FROM site_change WHERE business_id = ?1 ORDER BY created_at DESC LIMIT 200")).map_err(err)?;
        let rows = st
            .query_map(params![self.id().as_str()], ch_row)
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    pub(crate) fn site_events(&self, change_id: &str) -> Result<Vec<BusinessSiteEvent>, String> {
        let mut st = self
            .conn()
            .prepare("SELECT seq, at, from_status, to_status, actor, detail FROM site_change_event WHERE business_id = ?1 AND change_id = ?2 ORDER BY seq")
            .map_err(err)?;
        let rows = st
            .query_map(params![self.id().as_str(), change_id], |r| {
                Ok(BusinessSiteEvent {
                    seq: r.get(0)?,
                    at: r.get(1)?,
                    from_status: r.get(2)?,
                    to_status: r.get(3)?,
                    actor: r.get(4)?,
                    detail: r.get(5)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// Moves a change from `from` (compare-and-swap) and appends the audit event.
    fn site_move(
        &self,
        change_id: &str,
        from: &[&str],
        to: &str,
        actor: &str,
        detail: &str,
        extra: Option<(&str, &str)>,
    ) -> Result<BusinessSiteChange, String> {
        let cur = self.site_change(change_id)?;
        if !from.contains(&cur.status.as_str()) {
            return Err(format!(
                "this change is {}, so it can't move to {to}",
                cur.status
            ));
        }
        let tx = rusqlite::Transaction::new_unchecked(
            self.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(err)?;
        let n = tx
            .execute("UPDATE site_change SET status = ?3, updated_at = ?4 WHERE business_id = ?1 AND change_id = ?2 AND status = ?5", params![self.id().as_str(), change_id, to, now(), cur.status])
            .map_err(err)?;
        if n == 0 {
            return Err("this change moved meanwhile; reload".into());
        }
        if let Some((col, val)) = extra {
            let sql = match col {
                "package_dir" => "UPDATE site_change SET package_dir = ?3 WHERE business_id = ?1 AND change_id = ?2",
                "verify_detail" => "UPDATE site_change SET verify_detail = ?3 WHERE business_id = ?1 AND change_id = ?2",
                "approval_sha" => "UPDATE site_change SET approval_sha = ?3 WHERE business_id = ?1 AND change_id = ?2",
                _ => return Err("bad column".into()),
            };
            tx.execute(sql, params![self.id().as_str(), change_id, val])
                .map_err(err)?;
        }
        let seq: i64 = tx
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM site_change_event WHERE business_id = ?1 AND change_id = ?2", params![self.id().as_str(), change_id], |r| r.get(0))
            .map_err(err)?;
        tx.execute(
            "INSERT INTO site_change_event (business_id, change_id, seq, at, from_status, to_status, actor, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![self.id().as_str(), change_id, seq, now(), cur.status, to, actor, detail],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        super::note_activity(
            &self.data_path(),
            "business.action_done",
            &super::biz_ref(self.id(), "site_change", Some(change_id)),
            actor,
            &format!("Website change \"{}\": {to}. {detail}", cur.title),
        );
        self.site_change(change_id)
    }
}

/// Stages 4–6: computes the revision from the latest snapshot, its diff, checks and flags. Drafting
/// needs no publishing permission; a model drafting needs `propose_changes`.
pub(crate) fn draft(
    t: &Tenant,
    input: &BusinessSiteChangeInput,
    actor: &str,
) -> Result<BusinessSiteChange, String> {
    if actor != "taisun" {
        t.require("model", "propose_changes")?;
    }
    if BUNDLED_TYPES.contains(&input.content_type.as_str()) {
        return Err("That content is compiled into the site's JavaScript bundle. It can't be changed safely until a source that reproduces the live build is confirmed (decision 0065).".into());
    }
    let snap = t
        .latest_snapshot()?
        .ok_or("Take a snapshot of the live site first.")?;
    let manifest = t.snapshot_manifest(&snap)?;
    let has = |p: &str| manifest.iter().any(|f| f.path == p);
    let mut checks = Vec::new();
    // Every cited source must be approved and current.
    let mut ingredient_words = Vec::new();
    for r in &input.source_refs {
        match t.context_get(r)? {
            Some(i) if i.status == "approved" => {
                checks.push(chk(
                    "ok",
                    format!("Source \"{}\" is approved (v{}).", i.title, i.version),
                ));
                if matches!(i.section.as_str(), "ingredients" | "formulations") {
                    ingredient_words.extend(
                        i.body
                            .split(|c: char| !c.is_alphanumeric())
                            .filter(|w| w.len() > 4)
                            .map(str::to_string),
                    );
                }
            }
            Some(i) => {
                return Err(format!(
                    "\"{}\" is {}; only approved material can be published.",
                    i.title, i.status
                ))
            }
            None => return Err("a cited source isn't in this business".into()),
        }
    }
    let (target, title, before, after, before_bytes, after_bytes): (
        String,
        String,
        String,
        String,
        Vec<u8>,
        Option<Vec<u8>>,
    ) = match input.content_type.as_str() {
        "seo_title" | "seo_description" => {
            let html_b = t.snapshot_file(&snap, "/index.html")?;
            let html = String::from_utf8(html_b.clone()).map_err(|_| "index.html isn't text")?;
            let v = input.value.trim();
            let new = if input.content_type == "seo_title" {
                let (s, e) = (
                    html.find("<title>").ok_or("index.html has no <title>")?,
                    html.find("</title>").ok_or("index.html has no </title>")?,
                );
                if html.matches("<title>").count() != 1 {
                    return Err("index.html has more than one <title>".into());
                }
                let mut h = format!(
                    "{}<title>{}</title>{}",
                    &html[..s],
                    attr_escape(v),
                    &html[e + 8..]
                );
                h = set_meta(&h, "og:title", v).0;
                set_meta(&h, "twitter:title", v).0
            } else {
                let (h, n) = set_meta(&html, "description", v);
                if n != 1 {
                    return Err(format!(
                        "index.html has {n} description tags; expected exactly one"
                    ));
                }
                set_meta(
                    &set_meta(&h, "og:description", v).0,
                    "twitter:description",
                    v,
                )
                .0
            };
            let (lo, hi) = if input.content_type == "seo_title" {
                (10, 60)
            } else {
                (50, 160)
            };
            let len = v.chars().count();
            checks.push(if len == 0 {
                chk("error", "The text is empty.")
            } else if len < lo || len > hi {
                chk(
                    "warn",
                    format!("{len} characters; search results show about {lo}–{hi}."),
                )
            } else {
                chk("ok", format!("{len} characters."))
            });
            (
                "/index.html".into(),
                if input.content_type == "seo_title" {
                    "SEO title".into()
                } else {
                    "SEO description".into()
                },
                html.clone(),
                new.clone(),
                html_b,
                Some(new.into_bytes()),
            )
        }
        "robots" | "sitemap" => {
            let path = if input.content_type == "robots" {
                "/robots.txt"
            } else {
                "/sitemap.xml"
            };
            let old_b = t.snapshot_file(&snap, path).unwrap_or_default();
            let old = String::from_utf8_lossy(&old_b).into_owned();
            let v = input.value.replace("\r\n", "\n");
            if input.content_type == "sitemap" {
                let locs = super::website::sitemap_urls(&v);
                let bad: Vec<&String> = locs
                    .iter()
                    .filter(|u| !u.starts_with("https://taiyoskin.com/"))
                    .collect();
                checks.push(if !v.contains("<urlset") || !v.contains("</urlset>") {
                    chk("error", "The sitemap needs <urlset> … </urlset>.")
                } else if !bad.is_empty() {
                    chk(
                        "error",
                        format!("{} URLs aren't on https://taiyoskin.com/.", bad.len()),
                    )
                } else {
                    chk("ok", format!("{} URLs, all on the site.", locs.len()))
                });
            } else {
                let unknown: Vec<&str> = v
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .filter(|l| {
                        ![
                            "user-agent:",
                            "disallow:",
                            "allow:",
                            "sitemap:",
                            "crawl-delay:",
                        ]
                        .iter()
                        .any(|k| l.to_lowercase().starts_with(k))
                    })
                    .collect();
                checks.push(if unknown.is_empty() {
                    chk("ok", "Every line is a known directive.")
                } else {
                    chk("error", format!("Unknown lines: {}", unknown.join(" | ")))
                });
                if v.lines()
                    .any(|l| l.trim().eq_ignore_ascii_case("disallow: /"))
                {
                    checks.push(chk(
                        "warn",
                        "Disallow: / blocks every search engine from the whole site.",
                    ));
                }
            }
            (
                path.into(),
                if input.content_type == "robots" {
                    "robots.txt".into()
                } else {
                    "sitemap.xml".into()
                },
                old,
                v.clone(),
                old_b,
                Some(v.into_bytes()),
            )
        }
        "asset_replace" | "asset_delete" => {
            let path = input.path.clone().ok_or("choose the file")?;
            let path = super::site_ops::site_path(&path).ok_or("that isn't a path on the site")?;
            if !has(&path) {
                return Err(format!("{path} isn't in the live snapshot"));
            }
            let old_b = t.snapshot_file(&snap, &path)?;
            if input.content_type == "asset_delete" {
                (
                    path.clone(),
                    format!("Delete {path}"),
                    format!("{path} ({} bytes)", old_b.len()),
                    "(deleted)".into(),
                    old_b,
                    None,
                )
            } else {
                let doc = t
                    .document_get(
                        input
                            .document_id
                            .as_deref()
                            .ok_or("choose an approved document")?,
                    )?
                    .ok_or("no such document")?;
                if doc.status != "approved" {
                    return Err(format!(
                        "\"{}\" is {}; only an approved document can replace a site file.",
                        doc.title, doc.status
                    ));
                }
                let ext = |p: &str| p.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if ext(&doc.file_name) != ext(&path) {
                    return Err(format!(
                        "the document is .{}, the site file is .{}",
                        ext(&doc.file_name),
                        ext(&path)
                    ));
                }
                let bytes = std::fs::read(t.docs_path().join(&doc.file_name))
                    .map_err(|_| "the document copy is missing")?;
                if sha256_hex(&bytes) != doc.sha256 {
                    return Err("the document copy doesn't match its hash".into());
                }
                checks.push(chk(
                    "ok",
                    format!(
                        "Approved document \"{}\" (v{}), {} bytes.",
                        doc.title,
                        doc.version,
                        bytes.len()
                    ),
                ));
                (
                    path.clone(),
                    format!("Replace {path}"),
                    format!("{path} ({} bytes)", old_b.len()),
                    format!("{} ({} bytes)", doc.title, bytes.len()),
                    old_b,
                    Some(bytes),
                )
            }
        }
        other => return Err(format!("{other} isn't a supported change")),
    };
    if after_bytes.as_deref() == Some(before_bytes.as_slice()) {
        return Err("the change makes no difference".into());
    }
    // Links in the new text: same-site ones must exist; others aren't fetched.
    let added: String = diff(&before, &after)
        .iter()
        .filter(|l| l.op == "+")
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    for l in links(&added) {
        if l.starts_with("http://") {
            checks.push(chk("error", format!("{l} isn't HTTPS.")));
        } else if let Some(p) = site_path(&l) {
            let known = has(&p)
                || super::website::sitemap_urls(&String::from_utf8_lossy(
                    &t.snapshot_file(&snap, "/sitemap.xml").unwrap_or_default(),
                ))
                .iter()
                .any(|u| u.ends_with(&p));
            checks.push(chk(
                if known { "ok" } else { "warn" },
                format!(
                    "{p} {}",
                    if known {
                        "exists on the live site."
                    } else {
                        "isn't in the snapshot or the sitemap."
                    }
                ),
            ));
        } else if l.starts_with("https://") {
            checks.push(chk("warn", format!("{l} wasn't checked (another site).")));
        }
    }
    let d = diff(&before, &after);
    let mut f = flags(&input.content_type, &d, &ingredient_words);
    if f.contains(&"formulation".to_string())
        && !input.source_refs.iter().any(|r| {
            t.context_get(r)
                .ok()
                .flatten()
                .is_some_and(|i| i.section == "formulations" || i.section == "ingredients")
        })
    {
        checks.push(chk("error", "This touches formulation or ingredient wording but cites no approved ingredient or formulation item."));
    }
    if f.contains(&"pricing".to_string())
        && !input.source_refs.iter().any(|r| {
            t.context_get(r)
                .ok()
                .flatten()
                .is_some_and(|i| i.section == "products")
        })
    {
        checks.push(chk(
            "error",
            "This touches a price but cites no approved product item.",
        ));
    }
    f.sort();
    f.dedup();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let at = now();
    let dl: Vec<(String, String)> = d.iter().map(|l| (l.op.clone(), l.text.clone())).collect();
    let cl: Vec<(String, String)> = checks
        .iter()
        .map(|c| (c.level.clone(), c.text.clone()))
        .collect();
    // The candidate bytes are kept beside the change, for packaging and verification.
    let cdir = t.site_dir().join("changes").join(&id);
    crate::app_paths::create_private_dir_all(&cdir).map_err(err)?;
    if let Some(b) = &after_bytes {
        crate::app_paths::write_private(&cdir.join("after"), b).map_err(err)?;
    }
    t.conn()
        .execute(
            "INSERT INTO site_change (business_id, change_id, content_type, target, title, status, before_text, after_text, before_sha, after_sha,
                diff_json, checks_json, flags_json, sources_json, base_snapshot, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                t.id().as_str(), id, input.content_type, target, title, before.chars().take(200_000).collect::<String>(), after.chars().take(200_000).collect::<String>(),
                sha256_hex(&before_bytes), after_bytes.as_ref().map(|b| sha256_hex(b)), json!(dl).to_string(), json!(cl).to_string(), json!(f).to_string(), json!(input.source_refs).to_string(), snap, at
            ],
        )
        .map_err(err)?;
    t.conn()
        .execute(
            "INSERT INTO site_change_event (business_id, change_id, seq, at, from_status, to_status, actor, detail) VALUES (?1, ?2, 1, ?3, NULL, 'draft', ?4, 'Drafted from the live snapshot')",
            params![t.id().as_str(), id, at, actor],
        )
        .map_err(err)?;
    t.site_change(&id)
}

/// The native-dialog text for stage 8: target, every flag, every check that isn't ok, and the diff.
pub(crate) fn approval_text(c: &BusinessSiteChange) -> Result<String, String> {
    if c.checks.iter().any(|x| x.level == "error") {
        return Err(
            "Fix the checks marked as errors first; this change can't be approved as it is.".into(),
        );
    }
    let flag_text: Vec<&str> = c
        .flags
        .iter()
        .map(|f| match f.as_str() {
            "pricing" => "THIS CHANGES PRICING.",
            "formulation" => "THIS CHANGES FORMULATION OR INGREDIENT WORDING.",
            "customer_communication" => "THIS CHANGES CUSTOMER COMMUNICATION.",
            _ => "THIS DELETES CONTENT.",
        })
        .collect();
    let warns: Vec<String> = c
        .checks
        .iter()
        .filter(|x| x.level == "warn")
        .map(|x| format!("Warning: {}", x.text))
        .collect();
    let lines: Vec<String> = c
        .diff
        .iter()
        .filter(|l| l.op != " ")
        .map(|l| format!("{} {}", l.op, l.text.chars().take(160).collect::<String>()))
        .collect();
    let detail = format!(
        "{}\nFile: {}\n{}{}\n\nChanges:\n{}\n\nAllow prepares an upload package and a rollback package. Nothing is published until you upload it.",
        c.title,
        c.target,
        if flag_text.is_empty() { String::new() } else { format!("\n{}", flag_text.join("\n")) },
        if warns.is_empty() { String::new() } else { format!("\n{}", warns.join("\n")) },
        lines.join("\n")
    );
    crate::commands::approval_gate::approval_text("Approve a website change", &detail)
}

pub(crate) fn approve(
    t: &Tenant,
    change_id: &str,
    shown: &str,
) -> Result<BusinessSiteChange, String> {
    let c = t.site_change(change_id)?;
    if approval_text(&c)? != shown {
        return Err("the change differs from what was shown; nothing was approved".into());
    }
    t.site_move(
        change_id,
        &["draft"],
        "approved",
        "taisun",
        "Approved in the system dialog",
        Some(("approval_sha", &sha256_hex(shown.as_bytes()))),
    )
}

pub(crate) fn reject(t: &Tenant, change_id: &str) -> Result<BusinessSiteChange, String> {
    t.site_move(
        change_id,
        &["draft", "approved"],
        "rejected",
        "taisun",
        "Rejected",
        None,
    )
}

pub(crate) fn confirm_hpanel_backup(
    t: &Tenant,
    change_id: &str,
) -> Result<BusinessSiteChange, String> {
    let c = t.site_change(change_id)?;
    if !matches!(c.status.as_str(), "draft" | "approved") {
        return Err("only before packaging".into());
    }
    t.conn().execute("UPDATE site_change SET hpanel_backup = 1, updated_at = ?3 WHERE business_id = ?1 AND change_id = ?2", params![t.id().as_str(), change_id, now()]).map_err(err)?;
    t.site_change(change_id)
}

fn tar_gz(dest: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let f = crate::app_paths::private_open_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dest)
        .map_err(err)?;
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        f,
        flate2::Compression::default(),
    ));
    for (p, bytes) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
        h.set_cksum();
        b.append_data(&mut h, p.trim_start_matches('/'), bytes.as_slice())
            .map_err(err)?;
    }
    b.into_inner().map_err(err)?.finish().map_err(err)?;
    Ok(())
}

/// Stage 9–10: a fresh backup snapshot (refused when the live file moved since the draft), then the
/// upload package and the rollback package. Needs approval and the hPanel backup confirmation.
pub(crate) fn package(
    t: &Tenant,
    change_id: &str,
    http: &dyn Transport,
    sleep: Sleep,
) -> Result<BusinessSiteChange, String> {
    let c = t.site_change(change_id)?;
    if c.status != "approved" {
        return Err("only an approved change can be packaged".into());
    }
    if !c.hpanel_backup_confirmed {
        return Err("Confirm that you took an hPanel backup first.".into());
    }
    let before_sha: Option<String> = t
        .conn()
        .query_row(
            "SELECT before_sha FROM site_change WHERE business_id = ?1 AND change_id = ?2",
            params![t.id().as_str(), change_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let backup = snapshot_live(t, http, sleep)?;
    let live = backup.files.iter().find(|f| f.path == c.target);
    if live.map(|f| f.sha256.clone()) != before_sha {
        t.site_move(
            change_id,
            &["approved"],
            "rejected",
            "system",
            "The live file changed since this was drafted; draft it again from a new snapshot.",
            None,
        )?;
        return Err("The live file changed since this was drafted, so the change was withdrawn. Draft it again.".into());
    }
    let dir = t.site_dir().join("packages").join(change_id);
    crate::app_paths::create_private_dir_all(&dir).map_err(err)?;
    let before = t.snapshot_file(&backup.snapshot_id, &c.target)?;
    let upload_path = if c.target == "/index.html" {
        "/index.html".to_string()
    } else {
        c.target.clone()
    };
    let after = std::fs::read(t.site_dir().join("changes").join(change_id).join("after")).ok();
    let mut readme = format!(
        "Website change: {}\nFile on the site: public_html{}\n\n",
        c.title, upload_path
    );
    match &after {
        Some(a) => {
            tar_gz(&dir.join("upload.tar.gz"), &[(upload_path.clone(), a.clone())])?;
            readme.push_str("To publish: in Hostinger File Manager, open public_html, upload upload.tar.gz and extract it there (it replaces that one file). Then choose Verify in Amara OS.\n");
        }
        None => readme.push_str(&format!("To publish: in Hostinger File Manager, delete public_html{upload_path}. Then choose Verify in Amara OS.\n")),
    }
    tar_gz(
        &dir.join("rollback.tar.gz"),
        &[(upload_path.clone(), before)],
    )?;
    readme.push_str("To roll back: upload rollback.tar.gz to public_html and extract it, then choose Verify rollback.\n");
    crate::app_paths::write_private(&dir.join("README.txt"), readme.as_bytes()).map_err(err)?;
    t.site_move(
        change_id,
        &["approved"],
        "packaged",
        "taisun",
        &format!(
            "Backed up {} live files; upload and rollback packages ready",
            backup.files.len()
        ),
        Some(("package_dir", &dir.to_string_lossy())),
    )
}

/// Stage 11 (and 13): fetches the live file and compares it with the approved bytes (or, after a
/// rollback, with the backed-up bytes).
pub(crate) fn verify(
    t: &Tenant,
    change_id: &str,
    http: &dyn Transport,
    sleep: Sleep,
    rollback: bool,
) -> Result<BusinessSiteChange, String> {
    let c = t.site_change(change_id)?;
    let ok_from: &[&str] = if rollback {
        &["verify_failed", "verified", "rollback_failed"]
    } else {
        &["packaged", "verify_failed"]
    };
    if !ok_from.contains(&c.status.as_str()) {
        return Err(format!("this change is {}; nothing to verify", c.status));
    }
    let (before_sha, after_sha): (Option<String>, Option<String>) = t
        .conn()
        .query_row("SELECT before_sha, after_sha FROM site_change WHERE business_id = ?1 AND change_id = ?2", params![t.id().as_str(), change_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?;
    let domain = t
        .connector_config_value("website", "domain")?
        .ok_or("Set the Website connector's domain first.")?;
    let url_path = if c.target == "/index.html" {
        "/"
    } else {
        c.target.as_str()
    };
    let r = fetch(
        http,
        sleep,
        &Request {
            url: format!("https://{domain}{url_path}"),
            headers: vec![("cache-control".into(), "no-cache".into())],
        },
        MAX_FILE,
    )?;
    let live_sha = (200..300).contains(&r.status).then(|| sha256_hex(&r.body));
    let want = if rollback { before_sha } else { after_sha };
    let matched = live_sha == want;
    let detail = match (&want, &live_sha, matched) {
        (_, _, true) if want.is_none() => {
            format!("{url_path} is gone (answered {}), as approved.", r.status)
        }
        (_, _, true) => format!(
            "The live {url_path} matches the {} bytes exactly.",
            if rollback { "backed-up" } else { "approved" }
        ),
        (None, _, false) => format!(
            "{url_path} still answers {}; it hasn't been deleted.",
            r.status
        ),
        (_, None, false) => format!("{url_path} answered {}.", r.status),
        _ => format!(
            "The live {url_path} doesn't match yet (it may be cached; try again in a minute)."
        ),
    };
    let to = match (rollback, matched) {
        (false, true) => "verified",
        (false, false) => "verify_failed",
        (true, true) => "rolled_back",
        (true, false) => "rollback_failed",
    };
    t.site_move(
        change_id,
        ok_from,
        to,
        "system",
        &detail,
        Some(("verify_detail", &detail)),
    )
}
