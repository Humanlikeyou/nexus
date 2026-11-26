//! 0064 tests: read-only connectors and intelligence. Real SQLite in a temp folder; the database and
//! HTTP sources are fakes (no network in tests).

use super::connectors::TEST_HOME;
use super::net::{fetch, Request, Response, Transport, TEST_ALLOW_NET};
use super::supabase::{
    clean_path, referrer_host, store_aggregates, sync_with, Aggregate, Checkout, Cursor, Event,
    PageView, Session, Source, SourceError,
};
use super::*;
use crate::commands::keychain::FakeKeychain;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-biz12-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}
fn tenant(d: &Path) -> Tenant {
    store_in(d).tenant("taiyo-skin").unwrap()
}

// ─── The v1 → v2 migration ───────────────────────────────────────────────────

#[test]
fn a_v1_store_with_data_is_backed_up_and_migrated_with_hostinger_renamed() {
    let d = tmp("mig");
    std::fs::create_dir_all(d.0.join("business")).unwrap();
    let db = d.0.join("business/workspaces.db");
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO schema_meta VALUES ('schema_version', '1');
             CREATE TABLE business (business_id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, venture_id TEXT,
                 status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             INSERT INTO business VALUES ('taiyo-skin', 'TaiyoSkin', NULL, 'taiyo-skin', 'active', 't', 't');
             CREATE TABLE connector (business_id TEXT NOT NULL REFERENCES business(business_id), connector_id TEXT NOT NULL,
                 kind TEXT NOT NULL CHECK (kind IN ('supabase','hostinger','instagram','local_repo')), config_json TEXT NOT NULL,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY (business_id, connector_id));
             INSERT INTO connector VALUES ('taiyo-skin', 'hostinger', 'hostinger', '{\"domain\":\"taiyoskin.com\"}', 't', 't');
             INSERT INTO connector VALUES ('taiyo-skin', 'local_repo', 'local_repo', '{}', 't', 't');
             CREATE TABLE sync_run (business_id TEXT NOT NULL, run_id TEXT NOT NULL, connector_id TEXT NOT NULL, started_at TEXT NOT NULL,
                 finished_at TEXT, status TEXT NOT NULL, items_seen INTEGER NOT NULL DEFAULT 0, items_changed INTEGER NOT NULL DEFAULT 0,
                 conflicts INTEGER NOT NULL DEFAULT 0, detail TEXT, PRIMARY KEY (business_id, run_id),
                 FOREIGN KEY (business_id, connector_id) REFERENCES connector(business_id, connector_id));
             INSERT INTO sync_run VALUES ('taiyo-skin', 'r1', 'hostinger', '2026-01-01T00:00:00Z', 'x', 'succeeded', 1, 1, 0, NULL);",
        )
        .unwrap();
    }
    let t = tenant(&d.0);
    let ids = t.connector_ids().unwrap();
    assert!(ids.contains(&"website".to_string()) && !ids.contains(&"hostinger".to_string()));
    assert_eq!(ids.len(), 8, "every registered connector was added");
    assert_eq!(
        t.connector_config_value("website", "domain")
            .unwrap()
            .as_deref(),
        Some("taiyoskin.com")
    );
    assert_eq!(
        t.sync_runs(Some("website"), 5).unwrap().len(),
        1,
        "history moved with the rename"
    );
    let v: String = t
        .conn()
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // v1 → v2 → v3 in one open (0065 added the website-operations tables).
    assert_eq!(v, "3");
    assert!(
        std::fs::read_dir(d.0.join("business")).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("workspaces.db.v1-")),
        "a backup was taken first"
    );
    assert!(
        t.conn()
            .execute("UPDATE site_fact SET business_id = 'x'", [])
            .is_ok(),
        "no rows yet"
    );
    t.conn()
        .execute("INSERT INTO business_salt VALUES ('taiyo-skin', 's')", [])
        .unwrap();
    assert!(t
        .conn()
        .execute("UPDATE business_salt SET business_id = 'x'", [])
        .unwrap_err()
        .to_string()
        .contains("business_id is fixed"));
}

// ─── Supabase (fake source) ──────────────────────────────────────────────────

#[derive(Default)]
struct FakeDb {
    views: RefCell<Vec<PageView>>,
    sessions: RefCell<Vec<Session>>,
    events: Vec<Event>,
    checkouts: Vec<Checkout>,
    aggs: Vec<Aggregate>,
    check: Option<SourceError>,
    calls: RefCell<Vec<String>>,
}
fn after<T: Clone>(
    rows: &[T],
    key: impl Fn(&T) -> (String, String),
    c: &Cursor,
    limit: i64,
) -> Vec<T> {
    let mut v: Vec<T> = rows
        .iter()
        .filter(|r| key(r) > (c.at.clone(), c.id.clone()))
        .cloned()
        .collect();
    v.sort_by_key(|r| key(r));
    v.into_iter().take(limit as usize).collect()
}
impl Source for FakeDb {
    fn check(&self) -> Result<String, SourceError> {
        self.check
            .clone()
            .map(Err)
            .unwrap_or(Ok("read-only".into()))
    }
    fn page_views(&self, c: &Cursor, limit: i64) -> Result<Vec<PageView>, SourceError> {
        self.calls.borrow_mut().push(format!("views>{}", c.at));
        Ok(after(
            &self.views.borrow(),
            |r| (r.at.clone(), r.id.clone()),
            c,
            limit,
        ))
    }
    fn sessions(&self, c: &Cursor, limit: i64) -> Result<Vec<Session>, SourceError> {
        self.calls.borrow_mut().push(format!("sessions>{}", c.at));
        Ok(after(
            &self.sessions.borrow(),
            |r| (r.at.clone(), r.id.clone()),
            c,
            limit,
        ))
    }
    fn events(&self, c: &Cursor, limit: i64) -> Result<Vec<Event>, SourceError> {
        Ok(after(
            &self.events,
            |r| (r.at.clone(), r.id.clone()),
            c,
            limit,
        ))
    }
    fn checkouts(&self, c: &Cursor, limit: i64) -> Result<Vec<Checkout>, SourceError> {
        Ok(after(
            &self.checkouts,
            |r| (r.at.clone(), r.id.clone()),
            c,
            limit,
        ))
    }
    fn aggregates(&self) -> Result<Vec<Aggregate>, SourceError> {
        Ok(self.aggs.clone())
    }
}
fn ts(i: usize) -> String {
    format!(
        "2026-10-{:02}T{:02}:{:02}:00.000000Z",
        1 + i / 1440 % 28,
        i / 60 % 24,
        i % 60
    )
}
fn agg(metric: &str, dim: Option<&str>, n: Option<i64>) -> Aggregate {
    Aggregate {
        metric: metric.into(),
        dimension: dim.map(str::to_string),
        value: n.map(|x| x as f64),
        n,
        source_query: format!("public.quiz_results:{metric}"),
    }
}
fn db(views: usize) -> FakeDb {
    FakeDb {
        views: RefCell::new(
            (0..views)
                .map(|i| PageView {
                    id: format!("00000000-0000-0000-0000-{i:012}"),
                    path: Some(if i % 3 == 0 {
                        "/product/forge?email=a@b.c&ref=x".into()
                    } else {
                        "/shop".into()
                    }),
                    referrer: Some(if i % 2 == 0 {
                        "https://www.instagram.com/p/abc?u=me".into()
                    } else {
                        "".into()
                    }),
                    session_id: Some(format!("sess-{}", i % 40)),
                    at: ts(i),
                })
                .collect(),
        ),
        sessions: RefCell::new(
            (0..40)
                .map(|i| Session {
                    id: format!("s{i:03}"),
                    session_id: Some(format!("sess-{i}")),
                    entry_path: Some("/".into()),
                    exit_path: Some("/shop".into()),
                    time_spent_s: Some(30 + i as i64),
                    bounced: Some(i % 4 == 0),
                    started_at: Some(ts(i)),
                    ended_at: None,
                    at: ts(i),
                })
                .collect(),
        ),
        events: vec![Event {
            id: "e1".into(),
            event_name: Some("quiz_complete".into()),
            page: Some("/skin-assessment?step=4".into()),
            quiz_step: Some(4),
            session_id: Some("sess-1".into()),
            at: ts(5),
        }],
        checkouts: vec![Checkout {
            id: "c1".into(),
            event_type: Some("checkout_started".into()),
            session_id: Some("sess-2".into()),
            cart_value: Some(48.0),
            products: r#"[{"product":"forge","quantity":2}]"#.into(),
            at: ts(6),
        }],
        aggs: vec![
            agg("quiz.total", Some("all"), Some(12)),
            agg("waitlist.total", Some("all"), Some(10)),
            agg("contact.total", Some("all"), Some(1)),
            agg("profiles.total", Some("all"), Some(3)),
            agg("quiz.concern", Some("hyperpigmentation"), Some(7)),
            agg("quiz.concern", Some("dryness"), Some(5)),
            agg("quiz.concern", None, None),
            agg("quiz.concern", Some("leaked small label"), Some(2)),
            agg("contact.category", None, None),
            agg("waitlist.product", Some("forge"), Some(6)),
        ],
        ..Default::default()
    }
}

#[test]
fn supabase_sync_pages_by_cursor_never_duplicates_and_keeps_no_personal_detail() {
    let d = tmp("supa");
    let t = tenant(&d.0);
    let src = db(1203);
    let run = t.begin_run("supabase").unwrap();
    let (seen, changed, _, detail) = sync_with(&t, &src, &run).unwrap();
    assert!(detail.contains("1203 page views"), "{detail}");
    assert_eq!(t.one_count("site_page_view"), 1203);
    assert_eq!(t.one_count("site_session"), 40);
    assert!(changed >= 1203 + 40);
    assert!(seen > 1203);
    // Three pages of 500 were read for page views.
    assert_eq!(
        src.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with("views>"))
            .count(),
        3
    );
    // Stored values: no query strings, host names only, hashed session ids.
    let (path, host, key): (String, Option<String>, Option<String>) = t
        .conn()
        .query_row("SELECT path, referrer_host, session_key FROM site_page_view WHERE business_id = 'taiyo-skin' AND source_id = ?1", params!["00000000-0000-0000-0000-000000000000"], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!(path, "/product/forge");
    assert_eq!(host.as_deref(), Some("instagram.com"));
    assert!(key
        .as_deref()
        .is_some_and(|k| k.len() == 24 && !k.contains("sess")));
    let raw = String::from_utf8_lossy(&std::fs::read(d.0.join("business/workspaces.db")).unwrap())
        .into_owned();
    for leaked in ["a@b.c", "sess-1", "?email", "leaked small label"] {
        assert!(!raw.contains(leaked), "{leaked} reached the store");
    }

    // Again: the cursor resumes at the end, nothing new.
    src.calls.borrow_mut().clear();
    let run2 = t.begin_run("supabase");
    assert!(run2.is_err(), "the first run is still marked running");
    t.finish_run(&run, "Supabase", Ok((seen, changed, 0, detail)), "user")
        .unwrap();
    let run2 = t.begin_run("supabase").unwrap();
    let (_, changed2, _, _) = sync_with(&t, &src, &run2).unwrap();
    assert_eq!(changed2, 0, "a repeat sync adds nothing twice");
    assert_eq!(t.one_count("site_page_view"), 1203);
    assert!(
        src.calls
            .borrow()
            .iter()
            .any(|c| c == &format!("views>{}", ts(1202))),
        "resumed from the cursor"
    );
    // A session that changed after it started is updated through the 2-day look-back.
    src.sessions.borrow_mut()[39].exit_path = Some("/checkout".into());
    t.finish_run(&run2, "Supabase", Ok((0, 0, 0, String::new())), "user")
        .unwrap();
    let run3 = t.begin_run("supabase").unwrap();
    let (_, changed3, _, _) = sync_with(&t, &src, &run3).unwrap();
    assert_eq!(changed3, 1);
}

#[test]
fn customer_aggregates_suppress_small_groups_and_carry_no_label_for_them() {
    let d = tmp("agg");
    let t = tenant(&d.0);
    store_aggregates(&t, &db(0).aggs, "r").unwrap();
    let rows: Vec<(String, String, Option<f64>, i64)> = {
        let mut st = t.conn().prepare("SELECT metric, dimension, value, suppressed FROM aggregate_snapshot WHERE business_id = 'taiyo-skin' ORDER BY metric, dimension").unwrap();
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        v
    };
    let concern: Vec<_> = rows.iter().filter(|r| r.0 == "quiz.concern").collect();
    assert_eq!(
        concern.len(),
        3,
        "two shown plus one combined suppressed row: {concern:?}"
    );
    assert!(concern
        .iter()
        .any(|r| r.1 == "(groups under 5)" && r.2.is_none() && r.3 == 1));
    assert!(
        !rows.iter().any(|r| r.1.contains("leaked")),
        "a small group's label is never stored"
    );
    // Totals are kept as they are, even when small.
    assert!(rows
        .iter()
        .any(|r| r.0 == "contact.total" && r.2 == Some(1.0)));
    // The database refuses a suppressed row that carries a number.
    assert!(t.conn().execute("INSERT INTO aggregate_snapshot VALUES ('taiyo-skin','supabase','m','d',3,3,1,'q','r','t')", []).is_err());
}

#[test]
fn a_refused_check_fails_the_sync_and_an_auth_failure_reads_as_authentication_required() {
    let d = tmp("refuse");
    let t = tenant(&d.0);
    let mut src = db(3);
    src.check = Some(SourceError::Other("This role can write; refusing.".into()));
    let run = t.begin_run("supabase").unwrap();
    let e = sync_with(&t, &src, &run).unwrap_err();
    assert!(e.contains("refusing"));
    assert_eq!(
        t.one_count("site_page_view"),
        0,
        "nothing is read from a role that can write"
    );
    let c = super::supabase::record(&t, Err(SourceError::Auth("refused".into()))).unwrap();
    assert_eq!(c.outcome, "auth");
}

#[test]
fn the_setup_sql_grants_only_non_identifying_columns() {
    let sql = super::supabase::setup_sql();
    assert!(sql.contains("default_transaction_read_only = on") && sql.contains("nobypassrls"));
    for line in sql.lines().filter(|l| l.starts_with("grant select (")) {
        let (cols, table) = line
            .trim_start_matches("grant select (")
            .split_once(") on public.")
            .unwrap();
        let table = table.split_whitespace().next().unwrap();
        for c in cols.split(", ") {
            assert!(
                !super::supabase::PERSONAL.contains(&(table, c)),
                "{table}.{c} is personal"
            );
        }
    }
    for (t, _) in super::supabase::GRANTS {
        assert!(sql.contains(&format!("create policy amara_os_read_only on public.{t}")));
    }
    assert!(!sql.contains("orders"), "orders are never granted");
    assert!(
        sql.contains("REPLACE-WITH-A-LONG-RANDOM-PASSWORD"),
        "no real password in the SQL"
    );
}

#[test]
fn paths_and_referrers_are_reduced_before_they_are_stored() {
    assert_eq!(
        clean_path(Some("/confirm?token=abc#x")).as_deref(),
        Some("/confirm")
    );
    assert_eq!(
        clean_path(Some("https://taiyoskin.com/shop?x=1")).as_deref(),
        Some("/shop")
    );
    assert_eq!(clean_path(Some("")), None);
    assert_eq!(
        referrer_host(Some("https://www.Google.com/search?q=me")).as_deref(),
        Some("google.com")
    );
    assert_eq!(referrer_host(Some("android-app://x")), None);
    assert_eq!(referrer_host(None), None);
}

// ─── HTTP (fake transport) ───────────────────────────────────────────────────

type Call = (String, Vec<(String, String)>);

#[derive(Default)]
struct FakeHttp {
    answers: RefCell<HashMap<String, Vec<Response>>>,
    calls: RefCell<Vec<Call>>,
}
impl FakeHttp {
    fn on(self, url: &str, r: Vec<Response>) -> Self {
        self.answers.borrow_mut().insert(url.into(), r);
        self
    }
}
impl Transport for FakeHttp {
    fn get(&self, req: &Request, _max: usize) -> Result<Response, String> {
        self.calls
            .borrow_mut()
            .push((req.url.clone(), req.headers.clone()));
        let mut a = self.answers.borrow_mut();
        let list = a
            .get_mut(&req.url)
            .ok_or_else(|| format!("unexpected {}", req.url))?;
        Ok(if list.len() > 1 {
            list.remove(0)
        } else {
            list[0].clone()
        })
    }
}
fn resp(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}
thread_local! { static SLEPT: RefCell<Vec<Duration>> = const { RefCell::new(Vec::new()) }; }
fn rec_sleep(d: Duration) {
    SLEPT.with(|s| s.borrow_mut().push(d));
}

#[test]
fn nothing_is_sent_while_outbound_is_off_or_to_an_unapproved_host() {
    let http = FakeHttp::default().on("https://taiyoskin.com/", vec![resp(200, &[], "ok")]);
    TEST_ALLOW_NET.with(|a| a.set(false));
    if crate::commands::telegram::outbound_disabled() {
        assert!(fetch(
            &http,
            rec_sleep,
            &Request {
                url: "https://taiyoskin.com/".into(),
                headers: vec![]
            },
            1000
        )
        .unwrap_err()
        .contains("nothing was sent"));
        assert!(http.calls.borrow().is_empty());
    }
    TEST_ALLOW_NET.with(|a| a.set(true));
    assert!(fetch(
        &http,
        rec_sleep,
        &Request {
            url: "https://evil.example/".into(),
            headers: vec![]
        },
        1000
    )
    .unwrap_err()
    .contains("not an approved host"));
    assert!(
        fetch(
            &http,
            rec_sleep,
            &Request {
                url: "http://taiyoskin.com/".into(),
                headers: vec![]
            },
            1000
        )
        .is_err(),
        "plain HTTP is refused"
    );
    // A redirect to an unapproved host is not followed.
    let http = FakeHttp::default().on(
        "https://taiyoskin.com/x",
        vec![resp(301, &[("location", "https://evil.example/")], "")],
    );
    assert!(fetch(
        &http,
        rec_sleep,
        &Request {
            url: "https://taiyoskin.com/x".into(),
            headers: vec![]
        },
        1000
    )
    .unwrap_err()
    .contains("not an approved host"));
    assert_eq!(http.calls.borrow().len(), 1);
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn rate_limits_are_waited_out_or_stop_the_sync() {
    TEST_ALLOW_NET.with(|a| a.set(true));
    SLEPT.with(|s| s.borrow_mut().clear());
    let http = FakeHttp::default().on(
        "https://taiyoskin.com/",
        vec![
            resp(429, &[("retry-after", "2")], ""),
            resp(503, &[], ""),
            resp(200, &[], "ok"),
        ],
    );
    let r = fetch(
        &http,
        rec_sleep,
        &Request {
            url: "https://taiyoskin.com/".into(),
            headers: vec![],
        },
        1000,
    )
    .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(
        SLEPT.with(|s| s.borrow().clone()),
        [Duration::from_secs(2), Duration::from_secs(2)]
    );
    let far = (chrono::Utc::now().timestamp() + 3600).to_string();
    let http = FakeHttp::default().on(
        "https://api.github.com/x",
        vec![resp(
            429,
            &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", &far)],
            "",
        )],
    );
    assert!(fetch(
        &http,
        rec_sleep,
        &Request {
            url: "https://api.github.com/x".into(),
            headers: vec![]
        },
        1000
    )
    .unwrap_err()
    .contains("rate limited until"));
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn the_website_sync_reads_headers_robots_and_sitemap_as_facts_once() {
    TEST_ALLOW_NET.with(|a| a.set(true));
    let d = tmp("web");
    let t = tenant(&d.0);
    let kc = FakeKeychain::default();
    t.connector_configure(
        &kc,
        "website",
        &BTreeMap::from([("domain".into(), "taiyoskin.com".into())]),
    )
    .unwrap();
    assert_eq!(
        t.connector_view(&kc, "website").unwrap().state,
        "disconnected",
        "not checked yet"
    );
    let http = FakeHttp::default()
        .on("https://taiyoskin.com/", vec![resp(200, &[("platform", "hostinger"), ("last-modified", "Sun, 09 Aug 2026 08:04:57 GMT")], "<html>")])
        .on("https://taiyoskin.com/robots.txt", vec![resp(200, &[], "User-agent: *\nSitemap: https://taiyoskin.com/sitemap.xml\n")])
        .on("https://taiyoskin.com/sitemap.xml", vec![resp(200, &[], "<urlset><url><loc>https://taiyoskin.com/</loc></url><url><loc>https://taiyoskin.com/shop</loc></url></urlset>")]);
    let run = commands::sync_in(&t, &kc, &http, rec_sleep, "website", "user").unwrap();
    assert_eq!(run.status, "succeeded", "{:?}", run.detail);
    assert_eq!(t.connector_view(&kc, "website").unwrap().state, "connected");
    let facts: BTreeMap<String, String> = t
        .facts("website")
        .unwrap()
        .into_iter()
        .map(|f| (f.0, f.1))
        .collect();
    assert_eq!(facts["sitemap.url_count"], "2");
    assert_eq!(facts["home.platform"], "hostinger");
    let again = commands::sync_in(&t, &kc, &http, rec_sleep, "website", "user").unwrap();
    assert_eq!(again.items_changed, 0, "unchanged facts aren't rewritten");
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn github_pages_through_links_and_a_refused_token_is_authentication_required() {
    TEST_ALLOW_NET.with(|a| a.set(true));
    let d = tmp("gh");
    let t = tenant(&d.0);
    let kc = FakeKeychain::default();
    t.connector_configure(
        &kc,
        "github",
        &BTreeMap::from([("repo".into(), "owner/site".into())]),
    )
    .unwrap();
    t.connector_secret_set(&kc, "github", "token", "ghp_x".into())
        .unwrap();
    let commit = |sha: &str| {
        format!(
            r#"{{"sha":"{sha}","commit":{{"committer":{{"date":"2026-11-20T00:00:00Z"}},"message":"Change {sha}\nbody"}}}}"#
        )
    };
    let http = FakeHttp::default()
        .on("https://api.github.com/repos/owner/site", vec![resp(200, &[], r#"{"default_branch":"main","pushed_at":"2026-11-20T00:00:00Z","private":true}"#)])
        .on("https://api.github.com/repos/owner/site/branches?per_page=100", vec![resp(200, &[("link", "<https://api.github.com/repos/owner/site/branches?per_page=100&page=2>; rel=\"next\"")], r#"[{"name":"main"}]"#)])
        .on("https://api.github.com/repos/owner/site/branches?per_page=100&page=2", vec![resp(200, &[], r#"[{"name":"taiyo/prelaunch"}]"#)])
        .on("https://api.github.com/repos/owner/site/commits?sha=main&per_page=100", vec![resp(200, &[], &format!("[{},{}]", commit("aaaaaaa111"), commit("bbbbbbb222")))]);
    let run = commands::sync_in(&t, &kc, &http, rec_sleep, "github", "user").unwrap();
    assert_eq!(run.status, "succeeded", "{:?}", run.detail);
    let facts: BTreeMap<String, String> = t
        .facts("github")
        .unwrap()
        .into_iter()
        .map(|f| (f.0, f.1))
        .collect();
    assert_eq!(facts["branches.count"], "2");
    assert!(
        facts["commits.latest"].contains("aaaaaaa") && !facts["commits.latest"].contains("body")
    );
    assert!(http.calls.borrow().iter().all(|(_, h)| h
        .iter()
        .any(|(k, v)| k == "authorization" && v == "Bearer ghp_x")));
    let raw = String::from_utf8_lossy(&std::fs::read(d.0.join("business/workspaces.db")).unwrap())
        .into_owned();
    assert!(
        !raw.contains("ghp_x"),
        "the token is never stored in the database"
    );
    let refused = FakeHttp::default().on(
        "https://api.github.com/repos/owner/site",
        vec![resp(401, &[], "{}")],
    );
    commands::check_in(&t, &kc, &refused, rec_sleep, "github").unwrap();
    assert_eq!(
        t.connector_view(&kc, "github").unwrap().state,
        "auth_required"
    );
    TEST_ALLOW_NET.with(|a| a.set(false));
}

#[test]
fn deferred_connectors_have_no_sync_and_no_check_reaches_the_network() {
    let d = tmp("deferred");
    let t = tenant(&d.0);
    let kc = FakeKeychain::default();
    let http = FakeHttp::default();
    for c in ["google_analytics", "search_console", "instagram"] {
        assert!(commands::sync_in(&t, &kc, &http, rec_sleep, c, "user")
            .unwrap_err()
            .contains("no synchronization"));
        assert_eq!(
            commands::check_in(&t, &kc, &http, rec_sleep, c)
                .unwrap()
                .state,
            "disconnected"
        );
    }
    assert!(http.calls.borrow().is_empty());
}

// ─── Drive folder ────────────────────────────────────────────────────────────

#[test]
fn the_drive_folder_copies_real_files_lists_google_files_by_name_and_skips_links() {
    let d = tmp("drive");
    TEST_HOME.with(|h| *h.borrow_mut() = Some(d.0.clone()));
    let root = d.0.join("GoogleDrive/TaiyoSkin");
    std::fs::create_dir_all(root.join("Legal")).unwrap();
    std::fs::write(root.join("Pitch.pdf"), b"%PDF-1.4 pitch").unwrap();
    std::fs::write(root.join("Legal/Terms.md"), "# Terms\nPre-order terms.").unwrap();
    std::fs::write(
        root.join("Knowledge Base.gdoc"),
        r#"{"doc_id":"x","email":"owner@example.com"}"#,
    )
    .unwrap();
    std::fs::write(d.0.join("outside.md"), "secret").unwrap();
    std::os::unix::fs::symlink(d.0.join("outside.md"), root.join("link.md")).unwrap();
    let t = tenant(&d.0);
    let kc = FakeKeychain::default();
    t.connector_configure(
        &kc,
        "drive_folder",
        &BTreeMap::from([("path".into(), root.to_string_lossy().into_owned())]),
    )
    .unwrap();
    assert_eq!(
        t.connector_view(&kc, "drive_folder").unwrap().state,
        "connected"
    );
    let run = commands::sync_in(
        &t,
        &kc,
        &FakeHttp::default(),
        rec_sleep,
        "drive_folder",
        "user",
    )
    .unwrap();
    assert_eq!(run.status, "succeeded", "{:?}", run.detail);
    let docs: Vec<String> = t
        .documents()
        .unwrap()
        .into_iter()
        .map(|d| d.title)
        .collect();
    assert!(
        docs.contains(&"Pitch.pdf".to_string()) && docs.contains(&"Legal/Terms.md".to_string())
    );
    assert!(!docs
        .iter()
        .any(|d| d.contains("link") || d.contains(".gdoc")));
    let listed = t.context_list(Some("documents")).unwrap();
    assert!(listed[0].body.contains("Knowledge Base (Google Doc)"));
    let raw = String::from_utf8_lossy(&std::fs::read(d.0.join("business/workspaces.db")).unwrap())
        .into_owned();
    assert!(
        !raw.contains("owner@example.com"),
        "Google stubs are never stored"
    );
    let again = commands::sync_in(
        &t,
        &kc,
        &FakeHttp::default(),
        rec_sleep,
        "drive_folder",
        "user",
    )
    .unwrap();
    assert_eq!(again.items_changed, 0);
    TEST_HOME.with(|h| *h.borrow_mut() = None);
}

// ─── Intelligence ────────────────────────────────────────────────────────────

#[test]
fn reports_say_unavailable_instead_of_guessing_before_any_sync() {
    let d = tmp("empty");
    let t = tenant(&d.0);
    let w = super::intelligence::website_report(&t).unwrap();
    assert!(
        w.insights.iter().all(|i| i.status == "unavailable"),
        "{:?}",
        w.insights
            .iter()
            .filter(|i| i.status != "unavailable")
            .map(|i| &i.id)
            .collect::<Vec<_>>()
    );
    assert!(w.insights.iter().all(|i| i.value.is_none()));
    for id in [
        "search.queries",
        "search.impressions",
        "search.clicks",
        "search.positions",
        "search.indexed",
        "ga4",
    ] {
        assert!(w
            .insights
            .iter()
            .any(|i| i.id == id && i.status == "unavailable"));
    }
    let c = super::intelligence::customer_report(&t).unwrap();
    assert!(c.insights.iter().all(|i| i.status == "unavailable"));
}

#[test]
fn reports_label_measured_and_calculated_figures_with_their_n_and_sources() {
    let d = tmp("report");
    let t = tenant(&d.0);
    let run = t.begin_run("supabase").unwrap();
    let r = sync_with(&t, &db(30), &run).unwrap();
    t.finish_run(&run, "Supabase", Ok(r), "user").unwrap();
    let w = super::intelligence::website_report(&t).unwrap();
    let get = |id: &str| {
        w.insights
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };
    let views = get("web.views.total");
    assert_eq!((views.kind.as_str(), views.value), ("measured", Some(30.0)));
    assert!(views.sources[0].starts_with("Supabase page_views"));
    let top = get("web.pages.top");
    assert_eq!(top.kind, "calculated");
    assert_eq!(top.rows[0].label, "/shop");
    assert!((top.rows.iter().map(|r| r.share.unwrap()).sum::<f64>() - 1.0).abs() < 1e-9);
    let src = get("web.sources");
    assert!(
        src.rows.iter().any(|r| r.label == "instagram.com")
            && src.rows.iter().any(|r| r.label == "(direct or unknown)")
    );
    assert_eq!(get("web.bounce").n, Some(40));
    assert_eq!(get("web.products").rows[0].label, "forge");
    assert_eq!(get("web.conversion.rate").kind, "calculated");
    assert_eq!(get("search.clicks").status, "unavailable");

    let c = super::intelligence::customer_report(&t).unwrap();
    let base = c.insights.iter().find(|i| i.id == "cust.base").unwrap();
    assert!(
        base.note.as_deref().unwrap().contains("too few"),
        "12 quiz results is a small sample"
    );
    let concerns = c.insights.iter().find(|i| i.id == "cust.concerns").unwrap();
    assert_eq!(concerns.status, "too_few");
    assert_eq!(
        concerns
            .rows
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["hyperpigmentation", "dryness"]
    );
    assert!(concerns.note.as_deref().unwrap().contains("fewer than 5"));
    let faq = c.insights.iter().find(|i| i.id == "cust.faq").unwrap();
    assert_eq!(
        faq.status, "suppressed",
        "1 contact request: its topic is never shown"
    );
    assert_eq!(
        c.insights
            .iter()
            .find(|i| i.id == "cust.content_gaps")
            .unwrap()
            .kind,
        "hypothesis"
    );
    assert_eq!(
        c.insights
            .iter()
            .find(|i| i.id == "cust.products.checkout")
            .unwrap()
            .rows[0]
            .label,
        "forge"
    );
    // Amara is given measured and calculated insights only, with no person in them.
    let input = super::intelligence::interpretation_input(&c).to_string();
    assert!(!input.contains("hypothesis") && !input.contains("sess-"));
}

#[test]
fn amara_notes_are_kept_only_when_they_cite_real_insights() {
    let d = tmp("notes");
    let t = tenant(&d.0);
    let run = t.begin_run("supabase").unwrap();
    let r = sync_with(&t, &db(30), &run).unwrap();
    t.finish_run(&run, "Supabase", Ok(r), "user").unwrap();
    let rep = super::intelligence::website_report(&t).unwrap();
    let bad = serde_json::json!({"notes": [{"kind": "interpretation", "statement": "x", "basedOn": ["made.up"]}, {"kind": "fact", "statement": "y", "basedOn": ["web.views.total"]}]});
    assert!(super::intelligence::store_notes(&t, &rep, &bad, "light")
        .unwrap_err()
        .contains("didn't cite"));
    let good = serde_json::json!({"notes": [
        {"kind": "interpretation", "statement": "The shop page draws most views.", "basedOn": ["web.pages.top", "invented.id"]},
        {"kind": "hypothesis", "statement": "Instagram may bring product interest.", "basedOn": ["web.sources"]}
    ]});
    let notes = super::intelligence::store_notes(&t, &rep, &good, "light").unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes[0].based_on,
        ["web.pages.top"],
        "an invented id is dropped"
    );
    assert_eq!(
        super::intelligence::website_report(&t).unwrap().notes.len(),
        2
    );
    assert!(
        super::intelligence::customer_report(&t)
            .unwrap()
            .notes
            .is_empty(),
        "notes belong to their report"
    );
}

impl Tenant {
    fn one_count(&self, table: &str) -> i64 {
        self.conn()
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE business_id = ?1"),
                params![self.id().as_str()],
                |r| r.get(0),
            )
            .unwrap()
    }
}

/// Opt-in live checks into a temp data folder (network: the approved hosts only):
/// `NEXUS_LIVE_WEB=1 [NEXUS_LIVE_GH_REPO=owner/name NEXUS_LIVE_GH_TOKEN=…] [NEXUS_LIVE_DRIVE=<folder>]
///  cargo test --lib business_workspace::tests12::live -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_website_github_and_drive() {
    TEST_ALLOW_NET.with(|a| a.set(true));
    let d = tmp("live");
    let t = tenant(&d.0);
    let kc = FakeKeychain::default();
    if std::env::var("NEXUS_LIVE_WEB").is_ok() {
        t.connector_configure(
            &kc,
            "website",
            &BTreeMap::from([("domain".into(), "taiyoskin.com".into())]),
        )
        .unwrap();
        let c = commands::check_in(
            &t,
            &kc,
            &super::net::Live,
            super::net::real_sleep,
            "website",
        )
        .unwrap();
        println!("website check: {} · {}", c.state, c.detail);
        let r = commands::sync_in(
            &t,
            &kc,
            &super::net::Live,
            super::net::real_sleep,
            "website",
            "user",
        )
        .unwrap();
        println!(
            "website sync: {} · {}",
            r.status,
            r.detail.unwrap_or_default()
        );
        for f in t.facts("website").unwrap() {
            println!("  {} = {}", f.0, f.1.chars().take(90).collect::<String>());
        }
    }
    if let (Ok(repo), Ok(token)) = (
        std::env::var("NEXUS_LIVE_GH_REPO"),
        std::env::var("NEXUS_LIVE_GH_TOKEN"),
    ) {
        t.connector_configure(&kc, "github", &BTreeMap::from([("repo".into(), repo)]))
            .unwrap();
        t.connector_secret_set(&kc, "github", "token", token)
            .unwrap();
        let c = commands::check_in(&t, &kc, &super::net::Live, super::net::real_sleep, "github")
            .unwrap();
        println!("github check: {} · {}", c.state, c.detail);
        let r = commands::sync_in(
            &t,
            &kc,
            &super::net::Live,
            super::net::real_sleep,
            "github",
            "user",
        )
        .unwrap();
        println!(
            "github sync: {} · {}",
            r.status,
            r.detail.unwrap_or_default()
        );
        for f in t
            .facts("github")
            .unwrap()
            .into_iter()
            .filter(|f| f.0 != "commits.recent" && f.0 != "branches.names")
        {
            println!("  {} = {}", f.0, f.1.chars().take(110).collect::<String>());
        }
    }
    if let Ok(folder) = std::env::var("NEXUS_LIVE_DRIVE") {
        t.connector_configure(
            &kc,
            "drive_folder",
            &BTreeMap::from([("path".into(), folder)]),
        )
        .unwrap();
        let r = commands::sync_in(
            &t,
            &kc,
            &FakeHttp::default(),
            rec_sleep,
            "drive_folder",
            "user",
        )
        .unwrap();
        println!(
            "drive sync: {} · {}",
            r.status,
            r.detail.unwrap_or_default()
        );
        let docs = t.documents().unwrap();
        println!(
            "  {} documents copied; kinds: {:?}",
            docs.len(),
            docs.iter().map(|d| d.kind.as_str()).fold(
                BTreeMap::<&str, usize>::new(),
                |mut m, k| {
                    *m.entry(k).or_default() += 1;
                    m
                }
            )
        );
        if let Some(i) = t.context_list(Some("documents")).unwrap().first() {
            println!(
                "  Google files listed by name: {}",
                i.body.lines().filter(|l| l.starts_with("- ")).count()
            );
        }
    }
    let w = super::intelligence::website_report(&t).unwrap();
    for i in w
        .insights
        .iter()
        .filter(|i| i.group == "Deployment" || i.id == "site.sitemap")
    {
        println!("[{}] {} · {} · {}", i.kind, i.title, i.status, i.statement);
    }
    TEST_ALLOW_NET.with(|a| a.set(false));
}
