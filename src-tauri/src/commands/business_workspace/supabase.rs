//! Supabase, read-only (decision 0064). Taisun chose a database role that can only read.
//!
//! **The role** (`setup_sql`, applied by Taisun in the Supabase SQL editor): `nexus_reader`, login,
//! read-only by default, a statement timeout, no bypass of row security, SELECT on **non-identifying
//! columns only** (no names, emails, phone numbers, user ids, user agents, message text, addresses or
//! orders), and a row policy so it can see the rows of those tables. The check below refuses a role
//! that can write, bypass row security or read any listed personal column.
//!
//! **TLS:** the pooler's certificate chains to Supabase's own root CA, which isn't publicly trusted.
//! The connection trusts that one certificate only (built-in roots off) and verifies the host name.
//! Nothing falls back to an unverified connection.
//!
//! **What is copied:** first-party website analytics (page views, sessions, events, checkout steps),
//! pseudonymous only: session ids are stored as keyed hashes, referrers as host names, paths without
//! query strings. Keyset pagination by (created_at, id), 500 rows a page; a cursor per stream; rows are
//! upserted by source id, so a repeat sync adds nothing twice. Sessions are re-read 2 days back,
//! because their exit page and end time change after they start.
//!
//! **Customer information:** aggregates only, computed in the database. A group under 5 comes back
//! with no label and no count, and all such groups are stored as one suppressed row per metric.
//! No free text is read. Nothing is sent anywhere else.

use super::connectors::CheckResult;
use super::{err, now, Tenant};
use crate::commands::keychain::Keychain;
use rusqlite::params;
use std::path::Path;

pub(crate) const ROLE: &str = "nexus_reader";
pub(crate) const PAGE: i64 = 500;
pub(crate) const MIN_GROUP: i64 = 5;
const MAX_PAGES: usize = 2000;
const START_AT: &str = "1970-01-01T00:00:00.000000Z";
const START_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Columns the role may read, per table. The queries below use nothing else (a test checks).
pub(crate) const GRANTS: &[(&str, &[&str])] = &[
    (
        "page_views",
        &["id", "path", "referrer", "session_id", "created_at"],
    ),
    (
        "sessions",
        &[
            "id",
            "session_id",
            "entry_path",
            "exit_path",
            "time_spent_seconds",
            "bounced",
            "started_at",
            "ended_at",
            "created_at",
        ],
    ),
    (
        "analytics_events",
        &[
            "id",
            "event_name",
            "page",
            "quiz_step",
            "session_id",
            "created_at",
        ],
    ),
    (
        "checkout_events",
        &[
            "id",
            "event_type",
            "session_id",
            "cart_value",
            "cart_items",
            "created_at",
        ],
    ),
    (
        "quiz_results",
        &[
            "id",
            "concerns",
            "recommended_products",
            "secondary_type",
            "barrier_status",
            "sensitivity_index",
            "pigmentation_risk_level",
            "climate_compatibility",
            "oiliness_score",
            "dryness_score",
            "sensitivity_score",
            "barrier_strength_score",
            "pigmentation_risk_score",
            "acne_proneness_score",
            "texture_score",
            "resilience_score",
            "created_at",
        ],
    ),
    ("waitlist", &["id", "product", "created_at"]),
    ("contact_requests", &["id", "category", "created_at"]),
    ("customer_profiles", &["id", "created_at"]),
];

/// Personal columns the role must NOT be able to read; the check refuses a role that can.
pub(crate) const PERSONAL: &[(&str, &str)] = &[
    ("waitlist", "email"),
    ("waitlist", "first_name"),
    ("contact_requests", "email"),
    ("contact_requests", "name"),
    ("contact_requests", "message"),
    ("contact_requests", "order_number"),
    ("customer_profiles", "display_name"),
    ("customer_profiles", "phone"),
    ("customer_profiles", "user_id"),
    ("quiz_results", "email"),
    ("quiz_results", "user_id"),
    ("analytics_events", "user_email"),
    ("analytics_events", "user_id"),
    ("analytics_events", "event_data"),
    ("page_views", "user_id"),
    ("page_views", "user_agent"),
    ("sessions", "user_id"),
    ("checkout_events", "user_id"),
];

/// The SQL Taisun runs once in the Supabase SQL editor. The password placeholder is his to replace;
/// the app never sees the SQL editor or sets a password.
pub(crate) fn setup_sql() -> String {
    let mut s = String::from(
        "-- Amara OS read-only role (decision 0064). Run once in the Supabase SQL editor.\n\
         -- Replace the password below with a long random one, then store it in Amara OS (Keychain).\n\
         create role nexus_reader with login password 'REPLACE-WITH-A-LONG-RANDOM-PASSWORD'\n  \
         nosuperuser nocreatedb nocreaterole noinherit noreplication nobypassrls;\n\
         alter role nexus_reader set default_transaction_read_only = on;\n\
         alter role nexus_reader set statement_timeout = '20s';\n\
         alter role nexus_reader set idle_in_transaction_session_timeout = '30s';\n\
         grant usage on schema public to nexus_reader;\n",
    );
    for (t, cols) in GRANTS {
        s.push_str(&format!(
            "grant select ({}) on public.{t} to nexus_reader;\n",
            cols.join(", ")
        ));
        s.push_str(&format!(
            "create policy amara_os_read_only on public.{t} for select to nexus_reader using (true);\n"
        ));
    }
    s.push_str(
        "\n-- To revoke later:\n\
         -- drop owned by nexus_reader; drop role nexus_reader;\n",
    );
    s
}

/// The Supabase CA certificate: a PEM file inside the home folder with exactly one certificate.
pub(crate) fn read_ca(path: &Path) -> Result<Vec<u8>, String> {
    super::connectors::local_path_ok(path)?;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| "the certificate file isn't there".to_string())?;
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return Err("the certificate must be a regular file under 16 KB".into());
    }
    let pem = std::fs::read(path).map_err(|_| "the certificate can't be read".to_string())?;
    let text = String::from_utf8_lossy(&pem);
    if text.matches("-----BEGIN CERTIFICATE-----").count() != 1 {
        return Err("the file must hold exactly one PEM certificate".into());
    }
    native_tls::Certificate::from_pem(&pem)
        .map_err(|_| "that isn't a valid certificate".to_string())?;
    Ok(pem)
}

// ─── The source, behind a trait (tests use a fake) ───────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SourceError {
    /// Wrong role or password: `auth_required`.
    Auth(String),
    Other(String),
}
impl SourceError {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Auth(s) | Self::Other(s) => s.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cursor {
    pub at: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageView {
    pub id: String,
    pub path: Option<String>,
    pub referrer: Option<String>,
    pub session_id: Option<String>,
    pub at: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Session {
    pub id: String,
    pub session_id: Option<String>,
    pub entry_path: Option<String>,
    pub exit_path: Option<String>,
    pub time_spent_s: Option<i64>,
    pub bounced: Option<bool>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub at: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub id: String,
    pub event_name: Option<String>,
    pub page: Option<String>,
    pub quiz_step: Option<i64>,
    pub session_id: Option<String>,
    pub at: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Checkout {
    pub id: String,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub cart_value: Option<f64>,
    /// Product identifiers and quantities only (extracted in the database).
    pub products: String,
    pub at: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Aggregate {
    pub metric: String,
    /// None = a group under 5 (no label leaves the database).
    pub dimension: Option<String>,
    pub value: Option<f64>,
    pub n: Option<i64>,
    pub source_query: String,
}

pub(crate) trait Source {
    /// Who we are signed in as and that the role is read-only and sees no personal columns.
    fn check(&self) -> Result<String, SourceError>;
    fn page_views(&self, after: &Cursor, limit: i64) -> Result<Vec<PageView>, SourceError>;
    fn sessions(&self, after: &Cursor, limit: i64) -> Result<Vec<Session>, SourceError>;
    fn events(&self, after: &Cursor, limit: i64) -> Result<Vec<Event>, SourceError>;
    fn checkouts(&self, after: &Cursor, limit: i64) -> Result<Vec<Checkout>, SourceError>;
    fn aggregates(&self) -> Result<Vec<Aggregate>, SourceError>;
}

// ─── Postgres ────────────────────────────────────────────────────────────────

pub(crate) struct Pg {
    client: tokio_postgres::Client,
}

fn pg_err(e: tokio_postgres::Error) -> SourceError {
    let code = e.code().map(|c| c.code().to_string());
    match code.as_deref() {
        Some("28P01") | Some("28000") => {
            SourceError::Auth("The database refused the role or password.".into())
        }
        Some("42501") => SourceError::Other(
            "The role isn't allowed to read that (permission denied). Run the setup SQL again."
                .into(),
        ),
        Some("57014") => SourceError::Other("A query took too long and was stopped.".into()),
        Some(c) => SourceError::Other(format!("The database returned an error ({c}).")),
        None => {
            let t = e.to_string();
            if t.contains("certificate") || t.contains("tls") || t.contains("TLS") {
                SourceError::Other("The secure connection failed: the server's certificate didn't match the Supabase CA you chose.".into())
            } else if t.contains("password authentication")
                || t.contains("Tenant or user not found")
            {
                SourceError::Auth("The database refused the role or password.".into())
            } else {
                SourceError::Other("Couldn't connect to the database.".into())
            }
        }
    }
}

impl Pg {
    /// Connects with TLS pinned to the CA, as `nexus_reader.<project ref>`.
    pub(crate) fn connect(
        host: &str,
        project_ref: &str,
        ca_pem: &[u8],
        password: &str,
    ) -> Result<Self, SourceError> {
        if !super::net::supabase_pooler_ok(host) {
            return Err(SourceError::Other(
                "That pooler host isn't approved.".into(),
            ));
        }
        super::net::gate().map_err(SourceError::Other)?;
        let cert = native_tls::Certificate::from_pem(ca_pem)
            .map_err(|_| SourceError::Other("The CA certificate isn't valid.".into()))?;
        let tls = native_tls::TlsConnector::builder()
            .add_root_certificate(cert)
            .disable_built_in_roots(true)
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|_| SourceError::Other("Couldn't set up the secure connection.".into()))?;
        let tls = postgres_native_tls::MakeTlsConnector::new(tls);
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(host)
            .port(5432)
            .user(format!("{ROLE}.{project_ref}"))
            .password(password)
            .dbname("postgres")
            .application_name("amara-os-readonly")
            .ssl_mode(tokio_postgres::config::SslMode::Require)
            .connect_timeout(std::time::Duration::from_secs(15));
        let client = tauri::async_runtime::block_on(async move {
            let (client, connection) = cfg.connect(tls).await.map_err(pg_err)?;
            tauri::async_runtime::spawn(async move {
                let _ = connection.await;
            });
            client
                .batch_execute("SET TIME ZONE 'UTC'; SET default_transaction_read_only = on; SET statement_timeout = '20s';")
                .await
                .map_err(pg_err)?;
            Ok::<_, SourceError>(client)
        })?;
        Ok(Self { client })
    }

    fn rows(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>, SourceError> {
        tauri::async_runtime::block_on(self.client.query(sql, params)).map_err(pg_err)
    }
}

const TS: &str = r#"to_char(created_at, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')"#;
fn keyset(table: &str, cols: &str) -> String {
    format!(
        "SELECT id::text, {cols}, {TS} FROM public.{table}
         WHERE created_at IS NOT NULL AND (created_at, id) > ($1::text::timestamptz, $2::text::uuid)
         ORDER BY created_at, id LIMIT $3"
    )
}

impl Source for Pg {
    fn check(&self) -> Result<String, SourceError> {
        let r = self.rows(
            "SELECT current_user::text,
                    current_setting('transaction_read_only'),
                    (SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user),
                    has_table_privilege(current_user, 'public.page_views', 'INSERT, UPDATE, DELETE, TRUNCATE')",
            &[],
        )?;
        let row = r
            .first()
            .ok_or(SourceError::Other("The database sent no answer.".into()))?;
        let (user, ro, super_, writes): (String, String, Option<bool>, bool) =
            (row.get(0), row.get(1), row.get(2), row.get(3));
        if user != ROLE {
            return Err(SourceError::Other(format!(
                "Signed in as {user}, not {ROLE}; refusing. Use the read-only role only."
            )));
        }
        if ro != "on" || writes || super_.unwrap_or(true) {
            return Err(SourceError::Other(
                "This role can write or bypass row security; refusing. Run the setup SQL again."
                    .into(),
            ));
        }
        for (t, c) in PERSONAL {
            let r = self.rows(
                "SELECT COALESCE((SELECT has_column_privilege(current_user, ('public.' || $1)::regclass, $2::text, 'SELECT')
                                   WHERE to_regclass('public.' || $1) IS NOT NULL
                                     AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2)), false)",
                &[t, c],
            )?;
            if r.first().map(|x| x.get::<_, bool>(0)).unwrap_or(true) {
                return Err(SourceError::Other(format!("The role can read the personal column {t}.{c}; refusing. Use the setup SQL's column list.")));
            }
        }
        Ok(format!(
            "Signed in as {ROLE}: read-only, no personal columns (checked just now)."
        ))
    }

    fn page_views(&self, after: &Cursor, limit: i64) -> Result<Vec<PageView>, SourceError> {
        Ok(self
            .rows(
                &keyset("page_views", "path, referrer, session_id"),
                &[&after.at, &after.id, &limit],
            )?
            .iter()
            .map(|r| PageView {
                id: r.get(0),
                path: r.get(1),
                referrer: r.get(2),
                session_id: r.get(3),
                at: r.get(4),
            })
            .collect())
    }
    fn sessions(&self, after: &Cursor, limit: i64) -> Result<Vec<Session>, SourceError> {
        let sql = keyset(
            "sessions",
            r#"session_id, entry_path, exit_path, time_spent_seconds::int8, bounced,
               to_char(started_at, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'), to_char(ended_at, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')"#,
        );
        Ok(self
            .rows(&sql, &[&after.at, &after.id, &limit])?
            .iter()
            .map(|r| Session {
                id: r.get(0),
                session_id: r.get(1),
                entry_path: r.get(2),
                exit_path: r.get(3),
                time_spent_s: r.get(4),
                bounced: r.get(5),
                started_at: r.get(6),
                ended_at: r.get(7),
                at: r.get(8),
            })
            .collect())
    }
    fn events(&self, after: &Cursor, limit: i64) -> Result<Vec<Event>, SourceError> {
        Ok(self
            .rows(
                &keyset(
                    "analytics_events",
                    "event_name, page, quiz_step::int8, session_id",
                ),
                &[&after.at, &after.id, &limit],
            )?
            .iter()
            .map(|r| Event {
                id: r.get(0),
                event_name: r.get(1),
                page: r.get(2),
                quiz_step: r.get(3),
                session_id: r.get(4),
                at: r.get(5),
            })
            .collect())
    }
    fn checkouts(&self, after: &Cursor, limit: i64) -> Result<Vec<Checkout>, SourceError> {
        let cols = "event_type, session_id, cart_value::float8,
            (SELECT COALESCE(jsonb_agg(jsonb_build_object(
                     'product', left(COALESCE(e->>'slug', e->>'id', e->>'productId', e->>'name'), 80),
                     'quantity', CASE WHEN (e->>'quantity') ~ '^[0-9]{1,4}$' THEN (e->>'quantity')::int END)), '[]'::jsonb)::text
             FROM jsonb_array_elements(CASE WHEN jsonb_typeof(cart_items) = 'array' THEN cart_items ELSE '[]'::jsonb END) e)";
        Ok(self
            .rows(
                &keyset("checkout_events", cols),
                &[&after.at, &after.id, &limit],
            )?
            .iter()
            .map(|r| Checkout {
                id: r.get(0),
                event_type: r.get(1),
                session_id: r.get(2),
                cart_value: r.get(3),
                products: r.get(4),
                at: r.get(5),
            })
            .collect())
    }
    fn aggregates(&self) -> Result<Vec<Aggregate>, SourceError> {
        // Groups under MIN_GROUP come back with no label and no count.
        let g = |metric: &str, expr: &str, from: &str| {
            format!(
                "SELECT '{metric}', CASE WHEN count(*) >= {MIN_GROUP} THEN d END, CASE WHEN count(*) >= {MIN_GROUP} THEN count(*)::float8 END,
                        CASE WHEN count(*) >= {MIN_GROUP} THEN count(*) END, '{from}:{metric}'
                 FROM (SELECT {expr} AS d FROM {from}) x WHERE d IS NOT NULL AND d <> '' GROUP BY d"
            )
        };
        let total = |metric: &str, table: &str| {
            format!("SELECT '{metric}', 'all', count(*)::float8, count(*), '{table}:count' FROM public.{table}")
        };
        let mut parts = vec![
            total("quiz.total", "quiz_results"),
            total("waitlist.total", "waitlist"),
            total("contact.total", "contact_requests"),
            total("profiles.total", "customer_profiles"),
            g("quiz.concern", "unnest(concerns)", "public.quiz_results"),
            g(
                "quiz.recommended_product",
                "unnest(recommended_products)",
                "public.quiz_results",
            ),
            g("quiz.skin_type", "secondary_type", "public.quiz_results"),
            g(
                "quiz.barrier_status",
                "barrier_status",
                "public.quiz_results",
            ),
            g(
                "quiz.sensitivity",
                "sensitivity_index",
                "public.quiz_results",
            ),
            g(
                "quiz.pigmentation_risk",
                "pigmentation_risk_level",
                "public.quiz_results",
            ),
            g(
                "quiz.climate",
                "climate_compatibility",
                "public.quiz_results",
            ),
            g("waitlist.product", "product", "public.waitlist"),
            g("contact.category", "category", "public.contact_requests"),
        ];
        for s in [
            "oiliness",
            "dryness",
            "sensitivity",
            "barrier_strength",
            "pigmentation_risk",
            "acne_proneness",
            "texture",
            "resilience",
        ] {
            parts.push(format!(
                "SELECT 'quiz.avg_score', '{s}', CASE WHEN count({s}_score) >= {MIN_GROUP} THEN avg({s}_score)::float8 END,
                        CASE WHEN count({s}_score) >= {MIN_GROUP} THEN count({s}_score) END, 'public.quiz_results:avg_{s}_score'
                 FROM public.quiz_results"
            ));
        }
        let sql = parts.join("\nUNION ALL\n");
        // One statement reads one consistent snapshot, and the session is read-only.
        let rows = self.rows(&sql, &[])?;
        Ok(rows
            .iter()
            .map(|r| Aggregate {
                metric: r.get(0),
                dimension: r.get(1),
                value: r.get(2),
                n: r.get(3),
                source_query: r.get(4),
            })
            .collect())
    }
}

// ─── Local writes ────────────────────────────────────────────────────────────

/// A path without query string or fragment, at most 300 characters.
pub(crate) fn clean_path(p: Option<&str>) -> Option<String> {
    let p = p?.split(['?', '#']).next()?.trim();
    if p.is_empty() {
        return None;
    }
    let p = if let Some(rest) = p
        .strip_prefix("https://")
        .or_else(|| p.strip_prefix("http://"))
    {
        rest.split_once('/')
            .map(|(_, r)| format!("/{r}"))
            .unwrap_or_else(|| "/".into())
    } else {
        p.to_string()
    };
    Some(p.chars().take(300).collect())
}

/// The referrer's host name only (lowercase, no "www."), or None for a direct visit.
pub(crate) fn referrer_host(r: Option<&str>) -> Option<String> {
    let r = r?.trim();
    let rest = r
        .strip_prefix("https://")
        .or_else(|| r.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    (!host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
    .then_some(host)
}

impl Tenant {
    fn salt(&self) -> Result<String, String> {
        let b = self.id().as_str();
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO business_salt (business_id, salt) VALUES (?1, ?2)",
                params![
                    b,
                    uuid::Uuid::new_v4().simple().to_string()
                        + &uuid::Uuid::new_v4().simple().to_string()
                ],
            )
            .map_err(err)?;
        self.conn()
            .query_row(
                "SELECT salt FROM business_salt WHERE business_id = ?1",
                params![b],
                |r| r.get(0),
            )
            .map_err(err)
    }

    pub(crate) fn session_key(&self, salt: &str, sid: Option<&str>) -> Option<String> {
        sid.filter(|s| !s.is_empty())
            .map(|s| super::sha256_hex(format!("{salt}:{s}").as_bytes())[..24].to_string())
    }

    pub(crate) fn cursor(&self, connector_id: &str, stream: &str) -> Result<Cursor, String> {
        Ok(self
            .conn()
            .query_row(
                "SELECT cursor_at, cursor_id FROM source_cursor WHERE business_id = ?1 AND connector_id = ?2 AND stream = ?3",
                params![self.id().as_str(), connector_id, stream],
                |r| Ok(Cursor { at: r.get(0)?, id: r.get(1)? }),
            )
            .ok()
            .unwrap_or(Cursor { at: START_AT.into(), id: START_ID.into() }))
    }

    fn save_cursor(&self, connector_id: &str, stream: &str, c: &Cursor) -> Result<(), String> {
        self.conn()
            .execute(
                "INSERT INTO source_cursor (business_id, connector_id, stream, cursor_at, cursor_id, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (business_id, connector_id, stream) DO UPDATE SET cursor_at = excluded.cursor_at, cursor_id = excluded.cursor_id, updated_at = excluded.updated_at",
                params![self.id().as_str(), connector_id, stream, c.at, c.id, now()],
            )
            .map_err(err)?;
        Ok(())
    }
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct Counts {
    pub seen: i64,
    pub changed: i64,
}

/// Pages through one stream from its cursor, writing each page in one transaction.
fn pull<R>(
    t: &Tenant,
    stream: &str,
    start: Cursor,
    fetch: impl Fn(&Cursor) -> Result<Vec<R>, SourceError>,
    key: impl Fn(&R) -> Cursor,
    write: impl Fn(&rusqlite::Transaction, &R) -> Result<usize, String>,
) -> Result<Counts, SourceError> {
    let mut c = Counts::default();
    let mut cur = start;
    for _ in 0..MAX_PAGES {
        let rows = fetch(&cur)?;
        if rows.is_empty() {
            break;
        }
        let tx = rusqlite::Transaction::new_unchecked(
            t.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| SourceError::Other(err(e)))?;
        for r in &rows {
            c.seen += 1;
            c.changed += write(&tx, r).map_err(SourceError::Other)? as i64;
        }
        tx.commit().map_err(|e| SourceError::Other(err(e)))?;
        let full = rows.len() as i64 >= PAGE;
        cur = key(rows.last().expect("non-empty"));
        if !full {
            break;
        }
    }
    // The cursor only ever moves forward.
    let saved = t.cursor("supabase", stream).map_err(SourceError::Other)?;
    if (cur.at.as_str(), cur.id.as_str()) > (saved.at.as_str(), saved.id.as_str()) {
        t.save_cursor("supabase", stream, &cur)
            .map_err(SourceError::Other)?;
    }
    Ok(c)
}

/// Two days earlier than `at` (sessions are re-read so their exit and end are current).
fn lookback(c: &Cursor) -> Cursor {
    match chrono::DateTime::parse_from_rfc3339(&c.at) {
        Ok(d) if c.at != START_AT => Cursor {
            at: (d - chrono::Duration::days(2))
                .with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%S%.6fZ")
                .to_string(),
            id: START_ID.into(),
        },
        _ => Cursor {
            at: START_AT.into(),
            id: START_ID.into(),
        },
    }
}

/// Upsert counting only real changes.
fn upsert(
    tx: &rusqlite::Transaction,
    sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> Result<usize, String> {
    tx.execute(sql, p).map_err(err)
}

/// One full sync: the four analytics streams, then the customer aggregates.
pub(crate) fn sync_with(
    t: &Tenant,
    src: &dyn Source,
    run_id: &str,
) -> Result<(i64, i64, i64, String), String> {
    src.check().map_err(|e| e.text())?;
    let b = t.id().as_str().to_string();
    let salt = t.salt()?;
    let mut seen = 0;
    let mut changed = 0;
    let mut parts = Vec::new();

    let c = pull(
        t,
        "page_views",
        t.cursor("supabase", "page_views")?,
        |cur| src.page_views(cur, PAGE),
        |r| Cursor { at: r.at.clone(), id: r.id.clone() },
        |tx, r| {
            upsert(
                tx,
                "INSERT INTO site_page_view (business_id, source_id, path, referrer_host, session_key, at, run_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (business_id, source_id) DO UPDATE SET path = excluded.path, referrer_host = excluded.referrer_host,
                     session_key = excluded.session_key, at = excluded.at, run_id = excluded.run_id
                 WHERE site_page_view.path IS NOT excluded.path OR site_page_view.referrer_host IS NOT excluded.referrer_host
                     OR site_page_view.session_key IS NOT excluded.session_key OR site_page_view.at IS NOT excluded.at",
                &[&b, &r.id, &clean_path(r.path.as_deref()).unwrap_or_else(|| "/".into()), &referrer_host(r.referrer.as_deref()), &t.session_key(&salt, r.session_id.as_deref()), &r.at, &run_id],
            )
        },
    )
    .map_err(|e| e.text())?;
    seen += c.seen;
    changed += c.changed;
    parts.push(format!("{} page views", c.seen));

    let c = pull(
        t,
        "sessions",
        lookback(&t.cursor("supabase", "sessions")?),
        |cur| src.sessions(cur, PAGE),
        |r| Cursor { at: r.at.clone(), id: r.id.clone() },
        |tx, r| {
            upsert(
                tx,
                "INSERT INTO site_session (business_id, source_id, session_key, entry_path, exit_path, time_spent_s, bounced, started_at, ended_at, run_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (business_id, source_id) DO UPDATE SET session_key = excluded.session_key, entry_path = excluded.entry_path,
                     exit_path = excluded.exit_path, time_spent_s = excluded.time_spent_s, bounced = excluded.bounced,
                     started_at = excluded.started_at, ended_at = excluded.ended_at, run_id = excluded.run_id
                 WHERE site_session.exit_path IS NOT excluded.exit_path OR site_session.ended_at IS NOT excluded.ended_at
                     OR site_session.time_spent_s IS NOT excluded.time_spent_s OR site_session.bounced IS NOT excluded.bounced
                     OR site_session.entry_path IS NOT excluded.entry_path",
                &[
                    &b,
                    &r.id,
                    &t.session_key(&salt, r.session_id.as_deref()),
                    &clean_path(r.entry_path.as_deref()),
                    &clean_path(r.exit_path.as_deref()),
                    &r.time_spent_s,
                    &r.bounced.map(|x| x as i64),
                    &r.started_at.clone().unwrap_or_else(|| r.at.clone()),
                    &r.ended_at,
                    &run_id,
                ],
            )
        },
    )
    .map_err(|e| e.text())?;
    seen += c.seen;
    changed += c.changed;
    parts.push(format!("{} sessions", c.seen));

    let c = pull(
        t,
        "analytics_events",
        t.cursor("supabase", "analytics_events")?,
        |cur| src.events(cur, PAGE),
        |r| Cursor { at: r.at.clone(), id: r.id.clone() },
        |tx, r| {
            upsert(
                tx,
                "INSERT INTO site_event (business_id, source_id, event_name, page, quiz_step, session_key, at, run_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (business_id, source_id) DO NOTHING",
                &[&b, &r.id, &r.event_name.clone().unwrap_or_else(|| "unnamed".into()).chars().take(80).collect::<String>(), &clean_path(r.page.as_deref()), &r.quiz_step, &t.session_key(&salt, r.session_id.as_deref()), &r.at, &run_id],
            )
        },
    )
    .map_err(|e| e.text())?;
    seen += c.seen;
    changed += c.changed;
    parts.push(format!("{} events", c.seen));

    let c = pull(
        t,
        "checkout_events",
        t.cursor("supabase", "checkout_events")?,
        |cur| src.checkouts(cur, PAGE),
        |r| Cursor { at: r.at.clone(), id: r.id.clone() },
        |tx, r| {
            let products = serde_json::from_str::<serde_json::Value>(&r.products).map(|v| v.to_string()).unwrap_or_else(|_| "[]".into());
            upsert(
                tx,
                "INSERT INTO site_checkout (business_id, source_id, event_type, session_key, cart_value, products, at, run_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (business_id, source_id) DO NOTHING",
                &[&b, &r.id, &r.event_type.clone().unwrap_or_else(|| "unknown".into()).chars().take(80).collect::<String>(), &t.session_key(&salt, r.session_id.as_deref()), &r.cart_value, &products, &r.at, &run_id],
            )
        },
    )
    .map_err(|e| e.text())?;
    seen += c.seen;
    changed += c.changed;
    parts.push(format!("{} checkout steps", c.seen));

    let aggs = src.aggregates().map_err(|e| e.text())?;
    let stored = store_aggregates(t, &aggs, run_id)?;
    parts.push(format!("{stored} customer aggregates"));
    seen += aggs.len() as i64;

    Ok((
        seen,
        changed,
        0,
        format!("{} read; {changed} new or changed rows", parts.join(", ")),
    ))
}

/// Replaces this connector's aggregates in one transaction. Every group without a label (under 5)
/// becomes one suppressed row per metric; a labelled count under 5 is suppressed here as well.
pub(crate) fn store_aggregates(
    t: &Tenant,
    aggs: &[Aggregate],
    run_id: &str,
) -> Result<usize, String> {
    let b = t.id().as_str();
    let at = now();
    let tx =
        rusqlite::Transaction::new_unchecked(t.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    tx.execute(
        "DELETE FROM aggregate_snapshot WHERE business_id = ?1 AND connector_id = 'supabase'",
        params![b],
    )
    .map_err(err)?;
    let mut n = 0;
    let mut suppressed: Vec<(String, String)> = Vec::new();
    for a in aggs {
        let total = a.dimension.as_deref() == Some("all");
        let small = a.n.is_none_or(|n| n < MIN_GROUP);
        match (&a.dimension, small && !total) {
            (Some(d), false) => {
                tx.execute(
                    "INSERT INTO aggregate_snapshot (business_id, connector_id, metric, dimension, value, n, suppressed, source_query, run_id, captured_at)
                     VALUES (?1, 'supabase', ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8)
                     ON CONFLICT (business_id, connector_id, metric, dimension) DO UPDATE SET value = excluded.value, n = excluded.n",
                    params![b, a.metric, d.chars().take(120).collect::<String>(), a.value, a.n, a.source_query, run_id, at],
                )
                .map_err(err)?;
                n += 1;
            }
            _ => {
                if !suppressed.iter().any(|(m, _)| *m == a.metric) {
                    suppressed.push((a.metric.clone(), a.source_query.clone()));
                }
            }
        }
    }
    for (m, q) in suppressed {
        tx.execute(
            "INSERT OR IGNORE INTO aggregate_snapshot (business_id, connector_id, metric, dimension, value, n, suppressed, source_query, run_id, captured_at)
             VALUES (?1, 'supabase', ?2, '(groups under 5)', NULL, NULL, 1, ?3, ?4, ?5)",
            params![b, m, q, run_id, at],
        )
        .map_err(err)?;
        n += 1;
    }
    tx.commit().map_err(err)?;
    Ok(n)
}

fn live_source(t: &Tenant, kc: &dyn Keychain) -> Result<Pg, SourceError> {
    let host = t
        .connector_config_value("supabase", "pooler_host")
        .map_err(SourceError::Other)?
        .ok_or(SourceError::Other("Set the pooler host first.".into()))?;
    let project = t
        .connector_config_value("supabase", "project_ref")
        .map_err(SourceError::Other)?
        .ok_or(SourceError::Other(
            "Set the project reference first.".into(),
        ))?;
    let ca_path = t
        .connector_config_value("supabase", "ca_cert")
        .map_err(SourceError::Other)?
        .ok_or(SourceError::Other(
            "Choose the Supabase CA certificate first.".into(),
        ))?;
    let ca = read_ca(Path::new(&ca_path)).map_err(SourceError::Other)?;
    let pw = t
        .secret(kc, "supabase", "db_password")
        .map_err(SourceError::Auth)?;
    Pg::connect(&host, &project, &ca, &pw)
}

/// Check connection: connect, then confirm the role is read-only and sees no personal column.
pub(crate) fn check_live(t: &Tenant, kc: &dyn Keychain) -> Result<CheckResult, String> {
    let r = live_source(t, kc).and_then(|pg| pg.check());
    record(t, r)
}

pub(crate) fn record(t: &Tenant, r: Result<String, SourceError>) -> Result<CheckResult, String> {
    let (outcome, detail) = match r {
        Ok(d) => ("ok", d),
        Err(SourceError::Auth(d)) => ("auth", d),
        Err(SourceError::Other(d)) => ("error", d),
    };
    t.record_check("supabase", outcome, &detail)?;
    t.last_check("supabase")?
        .ok_or_else(|| "check vanished".into())
}

pub(crate) fn sync_live(
    t: &Tenant,
    kc: &dyn Keychain,
    actor: &str,
) -> Result<super::connectors::BusinessSyncRun, String> {
    t.require(actor, "sync_connectors")?;
    let run_id = t.begin_run("supabase")?;
    let outcome = match live_source(t, kc) {
        Ok(pg) => sync_with(t, &pg, &run_id),
        Err(e) => {
            let _ = record(t, Err(e.clone()));
            Err(e.text())
        }
    };
    if outcome.is_ok() {
        let _ = t.record_check(
            "supabase",
            "ok",
            "Signed in as nexus_reader: read-only, no personal columns (checked at the last sync).",
        );
    }
    t.finish_run(&run_id, "Supabase", outcome, actor)
}
