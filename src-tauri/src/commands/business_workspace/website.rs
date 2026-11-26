//! Website and deployment metadata (decision 0064), read-only, over `net.rs` only.
//!
//! - **Website (Hostinger):** the public site. `GET /` (status and headers: platform, server,
//!   last-modified), `/robots.txt` and `/sitemap.xml` (URL count and list). No account: Hostinger
//!   deploys by manual upload.
//! - **GitHub:** the site repository with a read-only token: the repository, its branches and the
//!   latest commits on the default branch, paginated by `Link` headers.
//!
//! Facts are stored by key (`site_fact`), so a repeat sync changes only what changed. Each fact keeps
//! the URL it came from. A sitemap URL count is "listed in the sitemap", never "indexed".

use super::connectors::CheckResult;
use super::net::{fetch, next_link, Request, Response, Sleep, Transport};
use super::{err, now, Tenant};
use crate::commands::keychain::Keychain;
use rusqlite::params;
use serde_json::{json, Value};

const MAX_PAGE: usize = 2 * 1024 * 1024;

fn store_facts(
    t: &Tenant,
    connector: &str,
    facts: &[(String, String, String)],
    run_id: &str,
) -> Result<i64, String> {
    let b = t.id().as_str();
    let at = now();
    let mut changed = 0;
    for (k, v, src) in facts {
        changed += t
            .conn()
            .execute(
                "INSERT INTO site_fact (business_id, connector_id, key, value, source_ref, run_id, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (business_id, connector_id, key) DO UPDATE SET value = excluded.value, source_ref = excluded.source_ref,
                     run_id = excluded.run_id, captured_at = excluded.captured_at
                 WHERE site_fact.value IS NOT excluded.value",
                params![b, connector, k, v, src, run_id, at],
            )
            .map_err(err)? as i64;
    }
    Ok(changed)
}

/// `<loc>` values from a sitemap, in order, at most 1000.
pub(crate) fn sitemap_urls(xml: &str) -> Vec<String> {
    xml.split("<loc>")
        .skip(1)
        .filter_map(|p| p.split("</loc>").next())
        .map(|u| u.trim().replace("&amp;", "&"))
        .filter(|u| !u.is_empty() && u.len() < 500)
        .take(1000)
        .collect()
}

fn domain(t: &Tenant) -> Result<String, String> {
    t.connector_config_value("website", "domain")?
        .ok_or_else(|| "Set the domain first.".into())
}

fn ok_2xx(r: &Response) -> bool {
    (200..300).contains(&r.status)
}

pub(crate) fn check_website(
    t: &Tenant,
    http: &dyn Transport,
    sleep: Sleep,
) -> Result<CheckResult, String> {
    let d = domain(t)?;
    let r = fetch(
        http,
        sleep,
        &Request {
            url: format!("https://{d}/"),
            headers: vec![],
        },
        MAX_PAGE,
    );
    let (outcome, detail) = match r {
        Ok(r) if ok_2xx(&r) => (
            "ok",
            format!(
                "The site answered {} over HTTPS (checked just now).",
                r.status
            ),
        ),
        Ok(r) => ("error", format!("The site answered {}.", r.status)),
        Err(e) => ("error", e),
    };
    t.record_check("website", outcome, &detail)?;
    t.last_check("website")?
        .ok_or_else(|| "check vanished".into())
}

pub(crate) fn sync_website(
    t: &Tenant,
    http: &dyn Transport,
    sleep: Sleep,
    run_id: &str,
) -> Result<(i64, i64, i64, String), String> {
    let d = domain(t)?;
    let home_url = format!("https://{d}/");
    let home = fetch(
        http,
        sleep,
        &Request {
            url: home_url.clone(),
            headers: vec![],
        },
        MAX_PAGE,
    )?;
    if !ok_2xx(&home) {
        t.record_check(
            "website",
            "error",
            &format!("The site answered {}.", home.status),
        )?;
        return Err(format!("the site answered {}", home.status));
    }
    t.record_check(
        "website",
        "ok",
        &format!(
            "The site answered {} over HTTPS (checked at the last sync).",
            home.status
        ),
    )?;
    let mut facts: Vec<(String, String, String)> = vec![(
        "home.status".into(),
        home.status.to_string(),
        home_url.clone(),
    )];
    for (h, key) in [
        ("platform", "home.platform"),
        ("server", "home.server"),
        ("last-modified", "home.last_modified"),
    ] {
        if let Some(v) = home.header(h) {
            facts.push((key.into(), v.chars().take(200).collect(), home_url.clone()));
        }
    }
    let robots_url = format!("https://{d}/robots.txt");
    let mut notes = Vec::new();
    match fetch(
        http,
        sleep,
        &Request {
            url: robots_url.clone(),
            headers: vec![],
        },
        256 * 1024,
    ) {
        Ok(r) if ok_2xx(&r) => {
            let text = String::from_utf8_lossy(&r.body);
            let declared: Vec<String> = text
                .lines()
                .filter_map(|l| {
                    l.trim()
                        .strip_prefix("Sitemap:")
                        .map(|s| s.trim().to_string())
                })
                .collect();
            facts.push(("robots.present".into(), "yes".into(), robots_url.clone()));
            facts.push((
                "robots.sitemaps".into(),
                json!(declared).to_string(),
                robots_url.clone(),
            ));
            facts.push((
                "robots.disallow_all".into(),
                (text
                    .lines()
                    .any(|l| l.trim().eq_ignore_ascii_case("disallow: /")))
                .to_string(),
                robots_url,
            ));
        }
        Ok(r) => notes.push(format!("robots.txt answered {}", r.status)),
        Err(e) => notes.push(format!("robots.txt: {e}")),
    }
    let sm_url = format!("https://{d}/sitemap.xml");
    match fetch(
        http,
        sleep,
        &Request {
            url: sm_url.clone(),
            headers: vec![],
        },
        MAX_PAGE,
    ) {
        Ok(r) if ok_2xx(&r) => {
            let urls = sitemap_urls(&String::from_utf8_lossy(&r.body));
            facts.push((
                "sitemap.url_count".into(),
                urls.len().to_string(),
                sm_url.clone(),
            ));
            facts.push(("sitemap.urls".into(), json!(urls).to_string(), sm_url));
        }
        Ok(r) => notes.push(format!("sitemap.xml answered {}", r.status)),
        Err(e) => notes.push(format!("sitemap.xml: {e}")),
    }
    let changed = store_facts(t, "website", &facts, run_id)?;
    let mut detail = format!("{} facts read, {changed} changed", facts.len());
    if !notes.is_empty() {
        detail.push_str(&format!(". {}", notes.join("; ")));
    }
    Ok((facts.len() as i64, changed, 0, detail))
}

// ─── GitHub ──────────────────────────────────────────────────────────────────

fn gh_headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("authorization".into(), format!("Bearer {token}")),
        ("accept".into(), "application/vnd.github+json".into()),
        ("x-github-api-version".into(), "2022-11-28".into()),
    ]
}

fn gh_json(r: &Response) -> Result<Value, String> {
    serde_json::from_slice(&r.body).map_err(|_| "GitHub sent something that isn't JSON".to_string())
}

/// 401 → authentication; 403/404 on the repository → the token can't see it; else an error.
fn gh_status(r: &Response) -> Result<(), (&'static str, String)> {
    match r.status {
        200..=299 => Ok(()),
        401 => Err(("auth", "GitHub refused the token (expired or revoked). Store a new one.".into())),
        403 if r.header("x-ratelimit-remaining") == Some("0") => Err(("error", "GitHub's rate limit is used up; try again later.".into())),
        403 | 404 => Err(("auth", "The token can't see this repository. Give it read access to the repository's contents and metadata.".into())),
        s => Err(("error", format!("GitHub answered {s}."))),
    }
}

fn repo(t: &Tenant) -> Result<String, String> {
    t.connector_config_value("github", "repo")?
        .ok_or_else(|| "Set the repository first.".into())
}

pub(crate) fn check_github(
    t: &Tenant,
    kc: &dyn Keychain,
    http: &dyn Transport,
    sleep: Sleep,
) -> Result<CheckResult, String> {
    let repo = repo(t)?;
    let token = t.secret(kc, "github", "token")?;
    let r = fetch(
        http,
        sleep,
        &Request {
            url: format!("https://api.github.com/repos/{repo}"),
            headers: gh_headers(&token),
        },
        MAX_PAGE,
    );
    let (outcome, detail) = match r {
        Ok(r) => match gh_status(&r) {
            Ok(()) => {
                let v = gh_json(&r)?;
                let private = v["private"]
                    .as_bool()
                    .map(|p| if p { "private" } else { "public" })
                    .unwrap_or("unknown visibility");
                (
                    "ok",
                    format!("Repository readable ({private}); checked just now."),
                )
            }
            Err((o, d)) => (o, d),
        },
        Err(e) => ("error", e),
    };
    t.record_check("github", outcome, &detail)?;
    t.last_check("github")?
        .ok_or_else(|| "check vanished".into())
}

/// Follows `Link: rel="next"` up to `max_pages`.
fn gh_pages(
    http: &dyn Transport,
    sleep: Sleep,
    first: String,
    token: &str,
    max_pages: usize,
) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut url = Some(first);
    let mut pages = 0;
    while let Some(u) = url.take() {
        let r = fetch(
            http,
            sleep,
            &Request {
                url: u,
                headers: gh_headers(token),
            },
            MAX_PAGE,
        )?;
        gh_status(&r).map_err(|(_, d)| d)?;
        match gh_json(&r)? {
            Value::Array(items) => out.extend(items),
            _ => return Err("GitHub sent an unexpected page".into()),
        }
        pages += 1;
        if pages >= max_pages {
            break;
        }
        url = next_link(r.header("link"));
    }
    Ok(out)
}

pub(crate) fn sync_github(
    t: &Tenant,
    kc: &dyn Keychain,
    http: &dyn Transport,
    sleep: Sleep,
    run_id: &str,
) -> Result<(i64, i64, i64, String), String> {
    let repo = repo(t)?;
    let token = t.secret(kc, "github", "token")?;
    let base = format!("https://api.github.com/repos/{repo}");
    let r = fetch(
        http,
        sleep,
        &Request {
            url: base.clone(),
            headers: gh_headers(&token),
        },
        MAX_PAGE,
    )?;
    if let Err((o, d)) = gh_status(&r) {
        t.record_check("github", o, &d)?;
        return Err(d);
    }
    t.record_check(
        "github",
        "ok",
        "Repository readable (checked at the last sync).",
    )?;
    let meta = gh_json(&r)?;
    let default_branch = meta["default_branch"]
        .as_str()
        .unwrap_or("main")
        .to_string();
    let mut facts: Vec<(String, String, String)> = vec![
        (
            "repo.default_branch".into(),
            default_branch.clone(),
            base.clone(),
        ),
        (
            "repo.pushed_at".into(),
            meta["pushed_at"].as_str().unwrap_or("").into(),
            base.clone(),
        ),
        (
            "repo.private".into(),
            meta["private"].as_bool().unwrap_or(true).to_string(),
            base.clone(),
        ),
    ];
    let branches_url = format!("{base}/branches?per_page=100");
    let branches: Vec<String> = gh_pages(http, sleep, branches_url.clone(), &token, 5)?
        .iter()
        .filter_map(|b| b["name"].as_str().map(|s| s.chars().take(120).collect()))
        .collect();
    facts.push((
        "branches.count".into(),
        branches.len().to_string(),
        branches_url.clone(),
    ));
    facts.push((
        "branches.names".into(),
        json!(branches).to_string(),
        branches_url,
    ));
    let commits_url = format!("{base}/commits?sha={default_branch}&per_page=100");
    let commits: Vec<Value> = gh_pages(http, sleep, commits_url.clone(), &token, 3)?
        .iter()
        .map(|c| {
            json!({
                "sha": c["sha"].as_str().map(|s| &s[..s.len().min(7)]).unwrap_or(""),
                "date": c["commit"]["committer"]["date"].as_str().unwrap_or(""),
                "message": c["commit"]["message"].as_str().and_then(|m| m.lines().next()).map(|m| m.chars().take(120).collect::<String>()).unwrap_or_default(),
            })
        })
        .collect();
    if let Some(latest) = commits.first() {
        facts.push((
            "commits.latest".into(),
            latest.to_string(),
            commits_url.clone(),
        ));
    }
    facts.push((
        "commits.count_read".into(),
        commits.len().to_string(),
        commits_url.clone(),
    ));
    facts.push((
        "commits.recent".into(),
        json!(commits.iter().take(30).collect::<Vec<_>>()).to_string(),
        commits_url,
    ));
    let changed = store_facts(t, "github", &facts, run_id)?;
    Ok((
        facts.len() as i64,
        changed,
        0,
        format!(
            "{} branches, {} commits read; {changed} facts changed",
            branches.len(),
            commits.len()
        ),
    ))
}

impl Tenant {
    /// The stored facts of one connector, by key.
    pub(crate) fn facts(
        &self,
        connector_id: &str,
    ) -> Result<Vec<(String, String, String, String)>, String> {
        let mut st = self
            .conn()
            .prepare("SELECT key, value, source_ref, captured_at FROM site_fact WHERE business_id = ?1 AND connector_id = ?2 ORDER BY key")
            .map_err(err)?;
        let rows = st
            .query_map(params![self.id().as_str(), connector_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(rows)
    }
}
