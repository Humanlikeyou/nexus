//! Synchronization from the website's source folder on this Mac (decision 0063). Read-only; no
//! network, no process.
//!
//! What it reads (each capped, never through a symlink, never outside the chosen folder):
//! - `src/App.tsx`: the route paths → one Website structure item;
//! - `src/data/products.ts`: each product's name, tagline, price, benefits and ingredients →
//!   a Product catalogue item and an Ingredient information item per product;
//! - Markdown files at the top of the folder and in `src/content/` → documents.
//!
//! Everything it writes is `unverified`, with the file it came from. A sync never changes an item
//! or document that is approved, a draft or archived: when the source differs from one of those, it
//! counts a conflict and leaves it. Formulations are not in the site source, so that section stays
//! empty rather than guessed.

use super::connectors::{check_local_repo, BusinessSyncRun};
use super::{err, now, BusinessContextInput, Tenant};
use rusqlite::params;
use std::path::Path;

const MAX_FILE: u64 = 512 * 1024;
const MAX_DOCS: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExtractedItem {
    pub section: &'static str,
    pub title: String,
    pub body: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExtractedDoc {
    pub title: String,
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub source_ref: String,
}

#[derive(Debug, Default)]
pub(crate) struct Extracted {
    pub items: Vec<ExtractedItem>,
    pub docs: Vec<ExtractedDoc>,
    /// Files that were expected but missing or unreadable, for the run's detail line.
    pub skipped: Vec<String>,
}

/// Reads a regular file inside `root` (no symlink anywhere on the way), at most `MAX_FILE` bytes.
fn read_in(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let mut p = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("{rel}: bad path"));
        }
        p.push(part);
        let m = std::fs::symlink_metadata(&p).map_err(|_| format!("{rel}: not found"))?;
        if m.file_type().is_symlink() {
            return Err(format!("{rel}: is a symbolic link"));
        }
    }
    let m = std::fs::metadata(&p).map_err(|_| format!("{rel}: not found"))?;
    if !m.is_file() {
        return Err(format!("{rel}: not a file"));
    }
    if m.len() > MAX_FILE {
        return Err(format!("{rel}: over 512 KB"));
    }
    std::fs::read(&p).map_err(|_| format!("{rel}: can't be read"))
}

/// Double-quoted string literals in `s`, with `\"` and `\\` unescaped. Other quoting is ignored.
pub(crate) fn string_literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut lit = String::new();
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        lit.push(match n {
                            'n' => '\n',
                            other => other,
                        });
                    }
                }
                '"' => {
                    closed = true;
                    break;
                }
                '\n' => break,
                other => lit.push(other),
            }
        }
        if closed {
            out.push(lit);
        }
    }
    out
}

/// The text inside the bracket that opens at the first `key: [` after `from`, matched by depth.
fn array_after<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let start = block.find(&format!("{key}: ["))? + key.len() + 3;
    let mut depth = 1;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in block[start..].char_indices() {
        match c {
            '"' if prev != '\\' => in_str = !in_str,
            '[' if !in_str => depth += 1,
            ']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(&block[start..start + i]);
                }
            }
            _ => {}
        }
        prev = c;
    }
    None
}

fn first_string_after(block: &str, key: &str) -> Option<String> {
    let at = block.find(&format!("{key}: \""))? + key.len() + 2;
    string_literals(&block[at..]).into_iter().next()
}

/// `price: 48,` → "48"; `price: null` → None; `price: FORGE_PRICE_USD` → the name (not a number).
fn price_after(block: &str) -> Option<String> {
    let at = block.find("price: ")? + 7;
    let v: String = block[at..]
        .chars()
        .take_while(|c| *c != ',' && *c != '\n')
        .collect();
    let v = v.trim();
    (!v.is_empty() && v != "null").then(|| v.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Product {
    pub slug: String,
    pub name: String,
    pub tagline: Option<String>,
    pub price: Option<String>,
    pub benefits: Vec<String>,
    pub ingredients: Vec<String>,
    pub detailed: Vec<(String, String)>,
}

/// Products from `src/data/products.ts`: one block per `slug: "…"`.
pub(crate) fn parse_products(src: &str) -> Vec<Product> {
    let body = match src.find("export const products") {
        Some(i) => &src[i..],
        None => return Vec::new(),
    };
    let starts: Vec<usize> = body.match_indices("slug: \"").map(|(i, _)| i).collect();
    let mut out = Vec::new();
    for (n, &s) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(body.len());
        let block = &body[s..end];
        let Some(slug) = first_string_after(block, "slug") else {
            continue;
        };
        let Some(name) = first_string_after(block, "name") else {
            continue;
        };
        let detailed = array_after(block, "detailedIngredients")
            .map(|a| {
                let lits = string_literals(a);
                let mut pairs = Vec::new();
                // Literals come as name, benefit, name, benefit … in `{ name: "…", benefit: "…" }` objects.
                for ch in lits.chunks(2) {
                    if let [n, b] = ch {
                        pairs.push((n.clone(), b.clone()));
                    }
                }
                pairs
            })
            .unwrap_or_default();
        out.push(Product {
            slug,
            name,
            tagline: first_string_after(block, "tagline"),
            price: price_after(block),
            benefits: array_after(block, "benefits")
                .map(string_literals)
                .unwrap_or_default(),
            ingredients: array_after(block, "ingredients")
                .map(string_literals)
                .unwrap_or_default(),
            detailed,
        });
    }
    out
}

/// Route paths from `src/App.tsx` (`path="…"`), in order, without duplicates or the catch-all.
pub(crate) fn parse_routes(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in src.match_indices("path=\"") {
        if let Some(p) = string_literals(&src[i + 5..]).into_iter().next() {
            let p = if p.starts_with('/') {
                p
            } else {
                format!("/{p}")
            };
            if p != "/*" && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

pub(crate) fn extract(root: &Path) -> Extracted {
    let mut x = Extracted::default();
    match read_in(root, "src/App.tsx").map(|b| String::from_utf8_lossy(&b).into_owned()) {
        Ok(app) => {
            let routes = parse_routes(&app);
            if !routes.is_empty() {
                let (admin, public): (Vec<_>, Vec<_>) =
                    routes.iter().partition(|r| r.starts_with("/admin"));
                let mut body = String::from(
                    "Pages the site's router defines (read from src/App.tsx).\n\nPublic pages:\n",
                );
                body.push_str(
                    &public
                        .iter()
                        .map(|r| format!("- {r}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                if !admin.is_empty() {
                    body.push_str("\n\nAdmin pages:\n");
                    body.push_str(
                        &admin
                            .iter()
                            .map(|r| format!("- {r}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
                x.items.push(ExtractedItem {
                    section: "website",
                    title: "Site pages (from the source)".into(),
                    body,
                    source_ref: "local_repo:src/App.tsx#routes".into(),
                });
            }
        }
        Err(e) => x.skipped.push(e),
    }
    match read_in(root, "src/data/products.ts").map(|b| String::from_utf8_lossy(&b).into_owned()) {
        Ok(src) => {
            for p in parse_products(&src) {
                let mut body = String::new();
                if let Some(t) = &p.tagline {
                    body.push_str(t);
                    body.push_str("\n\n");
                }
                body.push_str(&format!(
                    "Price in the source: {}\n",
                    match &p.price {
                        None => "none (not for sale)".to_string(),
                        Some(v) if v.chars().all(|c| c.is_ascii_digit() || c == '.') => v.clone(),
                        Some(v) => format!("set by {v} in the source"),
                    }
                ));
                if !p.benefits.is_empty() {
                    body.push_str("\nBenefits stated in the source:\n");
                    body.push_str(
                        &p.benefits
                            .iter()
                            .map(|b| format!("- {b}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    body.push('\n');
                }
                x.items.push(ExtractedItem {
                    section: "products",
                    title: p.name.clone(),
                    body: body.trim_end().to_string(),
                    source_ref: format!("local_repo:src/data/products.ts#{}", p.slug),
                });
                if !p.ingredients.is_empty() || !p.detailed.is_empty() {
                    let mut ib = String::new();
                    if !p.detailed.is_empty() {
                        ib.push_str(
                            &p.detailed
                                .iter()
                                .map(|(n, b)| format!("- {n}: {b}"))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    } else {
                        ib.push_str(
                            &p.ingredients
                                .iter()
                                .map(|n| format!("- {n}"))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                    x.items.push(ExtractedItem {
                        section: "ingredients",
                        title: format!("Ingredients: {}", p.name),
                        body: ib,
                        source_ref: format!(
                            "local_repo:src/data/products.ts#{}/ingredients",
                            p.slug
                        ),
                    });
                }
            }
        }
        Err(e) => x.skipped.push(e),
    }
    let mut md: Vec<String> = Vec::new();
    for dir in ["", "src/content"] {
        let path = if dir.is_empty() {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        if let Ok(rd) = std::fs::read_dir(&path) {
            let mut names: Vec<String> = rd
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|n| n.to_ascii_lowercase().ends_with(".md"))
                .collect();
            names.sort();
            md.extend(names.into_iter().map(|n| {
                if dir.is_empty() {
                    n
                } else {
                    format!("{dir}/{n}")
                }
            }));
        }
    }
    for rel in md.into_iter().take(MAX_DOCS) {
        match read_in(root, &rel) {
            Ok(bytes) if !bytes.is_empty() => {
                let file_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                x.docs.push(ExtractedDoc {
                    title: rel.clone(),
                    file_name,
                    bytes,
                    source_ref: format!("local_repo:{rel}"),
                });
            }
            Ok(_) => {}
            Err(e) => x.skipped.push(e),
        }
    }
    x
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct Applied {
    pub seen: i64,
    pub changed: i64,
    pub conflicts: i64,
}

/// Writes what was extracted under the never-overwrite rules above.
pub(crate) fn apply(t: &Tenant, x: &Extracted) -> Result<Applied, String> {
    let mut a = Applied::default();
    for it in &x.items {
        a.seen += 1;
        match t.context_by_source_ref(&it.source_ref)? {
            None => {
                t.context_save(
                    &BusinessContextInput {
                        item_id: None,
                        section: it.section.into(),
                        title: it.title.clone(),
                        body: it.body.clone(),
                        status: "unverified".into(),
                        expected_version: None,
                    },
                    "system",
                    "sync",
                    Some(&it.source_ref),
                )?;
                a.changed += 1;
            }
            Some(cur) if cur.title == it.title && cur.body == it.body => {}
            Some(cur) if cur.source == "sync" && cur.status == "unverified" => {
                t.context_save(
                    &BusinessContextInput {
                        item_id: Some(cur.item_id.clone()),
                        section: it.section.into(),
                        title: it.title.clone(),
                        body: it.body.clone(),
                        status: "unverified".into(),
                        expected_version: Some(cur.version),
                    },
                    "system",
                    "sync",
                    Some(&it.source_ref),
                )?;
                a.changed += 1;
            }
            Some(cur) if cur.status == "archived" => {}
            Some(_) => a.conflicts += 1,
        }
    }
    for d in &x.docs {
        a.seen += 1;
        let sha = super::sha256_hex(&d.bytes);
        match t.document_by_source_ref(&d.source_ref)? {
            None => {
                t.document_store(
                    &d.title,
                    &d.file_name,
                    &d.bytes,
                    "unverified",
                    "sync",
                    Some(&d.source_ref),
                    None,
                )?;
                a.changed += 1;
            }
            Some(cur) if cur.sha256 == sha => {}
            Some(cur) if cur.source == "sync" && cur.status == "unverified" => {
                t.document_store(
                    &d.title,
                    &d.file_name,
                    &d.bytes,
                    "unverified",
                    "sync",
                    Some(&d.source_ref),
                    Some(&cur.doc_id),
                )?;
                a.changed += 1;
            }
            Some(cur) if cur.status == "archived" => {}
            Some(_) => a.conflicts += 1,
        }
    }
    Ok(a)
}

/// One sync of the local repository connector, recorded as a run from start to finish.
/// `actor`: "user" (a click) or "scheduler" (needs the `sync_connectors` permission).
pub(crate) fn run(t: &Tenant, connector_id: &str, actor: &str) -> Result<BusinessSyncRun, String> {
    t.require(actor, "sync_connectors")?;
    let kind: String = t
        .conn()
        .query_row(
            "SELECT kind FROM connector WHERE business_id = ?1 AND connector_id = ?2",
            params![t.id().as_str(), connector_id],
            |r| r.get(0),
        )
        .map_err(|_| "no such connector in this business".to_string())?;
    if kind != "local_repo" {
        return Err("only the website source on this Mac can be synchronized in this phase".into());
    }
    let running: i64 = t
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM sync_run WHERE business_id = ?1 AND connector_id = ?2 AND status = 'running'",
            params![t.id().as_str(), connector_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if running > 0 {
        return Err("a sync of this connector is already running".into());
    }
    let path = t
        .connector_config_value(connector_id, "path")?
        .ok_or("choose the site's folder first")?;
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    t.conn()
        .execute(
            "INSERT INTO sync_run (business_id, run_id, connector_id, started_at, status) VALUES (?1, ?2, ?3, ?4, 'running')",
            params![t.id().as_str(), run_id, connector_id, now()],
        )
        .map_err(err)?;
    let outcome = check_local_repo(&path).and_then(|_| {
        let x = extract(Path::new(&path));
        t.set_quiet(true);
        let applied = apply(t, &x);
        t.set_quiet(false);
        applied.map(|a| (a, x.skipped))
    });
    let (status, seen, changed, conflicts, detail) = match &outcome {
        Ok((a, skipped)) => {
            let mut d = format!("{} found, {} added or updated", a.seen, a.changed);
            if a.conflicts > 0 {
                d.push_str(&format!(
                    ", {} differ from approved or draft material and were left as they are",
                    a.conflicts
                ));
            }
            if !skipped.is_empty() {
                d.push_str(&format!(". Not read: {}", skipped.join("; ")));
            }
            ("succeeded", a.seen, a.changed, a.conflicts, d)
        }
        Err(e) => ("failed", 0, 0, 0, e.clone()),
    };
    t.conn()
        .execute(
            "UPDATE sync_run SET status = ?3, finished_at = ?4, items_seen = ?5, items_changed = ?6, conflicts = ?7, detail = ?8
             WHERE business_id = ?1 AND run_id = ?2",
            params![t.id().as_str(), run_id, status, now(), seen, changed, conflicts, detail],
        )
        .map_err(err)?;
    super::note_activity(
        &t.data_path(),
        "business.sync_finished",
        &super::biz_ref(t.id(), "sync", Some(&run_id)),
        if actor == "user" { "taisun" } else { "system" },
        &format!("Website source sync {status}: {detail}"),
    );
    t.sync_runs(Some(connector_id), 1)?
        .into_iter()
        .next()
        .ok_or_else(|| "sync run vanished".into())
}
