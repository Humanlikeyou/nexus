# 0064: TaiyoSkin read-only integrations and intelligence, Phase 12 (Taisun, 2026-11-26, in session)

Brief: `docs/specs/taiyoskin-readonly-integrations-2026-11-26.md`. The inspection findings are in that file: no credentials were assumed, no row contents were read, and no key value was printed.

## Taisun's answers

| Question | Answer |
|---|---|
| Hard rule 3: new outbound hosts | **"Approve all incl. Google"** for manual, user-started syncs only, blocked while outbound is disabled |
| Supabase access | **Read-only database role** (one new Rust dependency, a Postgres client) |
| GA4 and Search Console | **Defer**; use the site's own first-party analytics |
| Customer data | **Aggregates only, local AI**: no names, emails or free text stored; groups under 5 suppressed; interpretation on the local model only |

### Hosts in use (`net.rs`, pinned by a test)

- **HTTPS:** `taiyoskin.com`, `www.taiyoskin.com` and `api.github.com`.
- **Postgres:** the Supabase pooler for the Tokyo region (`aws-<n>-ap-northeast-1.pooler.supabase.com:5432`). **Flag:** the question named `hghluspjxlfhymjmlmcx.supabase.co`, and that direct host is IPv6-only. The pooler is Supabase's own entry point to the same project, and the CLI's linked project uses it. Taisun should confirm this host explicitly.
- **Google hosts:** approved but unused, because GA4 and Search Console are deferred.

Every request first checks `outbound_disabled()`, so nothing is sent while outbound is off.

## What landed

**Workspace store v2** (`business_workspace.rs`, the migration has a backup and a rollback-safe transaction):
- the connector table is rebuilt for the new registry, and `hostinger` becomes `website`;
- every registered connector is added to every business;
- new tenant tables (tenant pin and triggers): `connector_check`, `source_cursor`, `site_page_view`, `site_session`, `site_event`, `site_checkout`, `aggregate_snapshot`, `site_fact`, `business_salt` and `insight_note`.

**Registry** (`connectors.rs`): Supabase (read-only database), Website (Hostinger), GitHub, Google Analytics 4 (deferred), Search Console (deferred), Instagram (deferred), the website source on this Mac, and the Google Drive folder on this Mac.
- A network connector is **connected only after an explicit check passed**, stored with its time.
- An authentication failure shows as `auth_required`, and a deferred connector shows its reason.
- A changed setting or key clears the last check.

| Connector | How it reads | Pagination, rate limits, de-duplication | Privacy |
|---|---|---|---|
| **Supabase** (`supabase.rs`) | Postgres over TLS pinned to Supabase's CA (built-in roots off, host name verified) as `nexus_reader.<ref>`. The check refuses a role that can write, bypass row security, or read any listed personal column | Keyset pagination by (created_at, id), 500 rows a page, a cursor per stream; upsert by source id; sessions re-read 2 days back | Column-level grants on non-identifying columns only. Paths without query strings, referrers as host names, session ids as keyed hashes. Customer data is aggregated **in SQL**, and a group under 5 comes back with no label or count. No free text; orders are not granted |
| **Website** (`website.rs`) | `GET /`, `/robots.txt`, `/sitemap.xml` | Retries on 429/5xx (`Retry-After` capped at 30 s, else 1, 2, 4 s); facts upserted by key | Public pages only |
| **GitHub** (`website.rs`) | Repository, branches and commits with a read-only token | `Link: rel="next"` pages; a far rate-limit reset stops the sync and says when | Token only in the Keychain and in the request header |
| **Drive folder** (`drive_folder.rs`) | Real files in the synced folder, at most 4 levels deep, 300 files of 10 MB | By path and hash; approved, draft and archived copies are never changed | Google Docs/Sheets/Slides stubs hold the account email and are never stored; they're listed by name only |

**Setup SQL** (`business_supabase_setup_sql`) creates the role. It contains no secret: the password is a placeholder Taisun replaces in the SQL editor. It's shown in Integrations with a Copy button.

**Intelligence** (`intelligence.rs`, Enterprise › Workspaces › Intelligence):
- Every insight is labelled **measured**, **calculated**, **AI interpretation** or **unverified hypothesis**, with its n, time window and sources.
- **Website report:** page views, top pages, traffic sources by host, entry and exit pages, bounce rate, median time, product page views, events, checkout steps, and the share of sessions that reached checkout. Search queries, impressions, clicks, positions, indexed pages and GA4 show as **unavailable**, with the reason. Deployment: sitemap URLs ("listed is not indexed"), the live last-modified date, the latest commit, and the gap between them.
- **Customer report:** totals; quiz concerns, recommended products, waitlist interest, skin profile and contact topics. Groups under 5 are hidden, and fewer than 20 people means "too few to tell". It also lists products seen in checkout steps, and content-gap **hypotheses**: a rule that matches concern words against page addresses.
- **Ask Amara to interpret:** local model only (`LocalOnly`, persona off), given only measured and calculated insights. Each note must cite existing insight ids, and invented ids are dropped.

**Separation:** business data never reads ARM, Treasury, money, personal stores or chat context, and the cloud tier is never used (source pins). HTTP exists only in `net.rs` and Postgres only in `supabase.rs`.

## Verification

- **Rust:** 34 workspace tests (15 new for Phase 12) using fake database and HTTP sources. They cover:
  - pagination, cursors and no duplicates;
  - no personal detail in the stored file;
  - small-group suppression with no label;
  - refused roles, auth states and outbound refusal;
  - unapproved hosts and redirects;
  - `Retry-After` and rate-limit stops;
  - website facts written once;
  - GitHub `Link` paging and 401 → `auth_required`;
  - Drive files, Google stubs and symlinks;
  - the v1→v2 migration with its backup;
  - reports unavailable before a sync, labels and n, suppressed and too-few states;
  - note validation.
- **Live, into a temp data folder** (`tests12::live`, opt-in):
  - Website: connected; Hostinger, last-modified 2026-08-09, 25 sitemap URLs.
  - GitHub (`gh`'s token, read-only): connected; 21 branches and 300 commits read; latest `main` commit 2026-03-26.
  - Drive "Taiyo Skin" folder: 43 files copied (10 PDF, 22 text, 11 images); **123 Google Docs/Sheets are listed by name only.**
- **Not run:**
  - a live Supabase check or sync: the role, its password and the CA certificate don't exist yet;
  - Amara's interpretation against the live router;
  - an independent security review;
  - a packaged-app launch and an install.

## Taisun's steps to turn Supabase on

1. Supabase › Database settings › SSL configuration: download the CA certificate.
2. Integrations › Supabase › Show the setup SQL. Replace the password placeholder with a long random one, then run it in the SQL editor.
3. Enter the pooler host (`aws-1-ap-northeast-1.pooler.supabase.com`) and the project reference, choose the certificate, and store the password in the Keychain.
4. Check connection, then Sync now. Outbound must be allowed in Settings for that session.

## Not done

- GA4, Search Console and Google Docs text (needs Google OAuth).
- Instagram.
- Any publishing or write. No connector writes outside the app.
