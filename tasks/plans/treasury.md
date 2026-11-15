# Treasury plan (Step 1 of tasks/PROMPT-TREASURY-2026-11-15.md)

Status: plan only. No code written. Stop here for Taisun's approval.
Checked against HEAD 5ed6537 on branch amara-integration (the prompt was checked at 67617ed).
Hard rules read first (`.claude/rules/hard-rules.md`): no push/merge/PR, no secrets, `NEXUS_OUTBOUND_DISABLED=1`, no new outbound hosts, nav labels frozen (rule 4), design is out of scope (rule 10), never fabricate (rule 12).

## Prompt corrections

1. **The ledger is read-only and hand-written.** The prompt says "extend" `business_ledger.rs`. Its header says "read-only here ... Writes come later, behind approval" (`business_ledger.rs:1-5`). It has no writer, no sqlite, a 1 MiB cap (`:10`, `:232`) and strict validation that rejects negative amounts (`:176-182`). Treasury cannot append income rows to it. Plan below extends the ledger by reference, not by writing into the file.
2. **The "past HIGH security finding" on personal vs business money is not in `tasks/HANDOFF-FOR-REVIEW.md` or `docs/decisions/`.** I searched both. The only HIGH there is the Docker-logs leak (`HANDOFF-FOR-REVIEW.md:506,520`). The separation itself is real and pinned in code (see section 1). Taisun may mean an earlier review that is not in the repo.
3. **The guard test is narrower than the prompt implies.** The TS test only checks commands named `^(money|fx)_` (`test/commands.registered.test.ts:148`). The Rust test pins exactly 8 commands inside `money.rs` (`money.rs:2790-2815`, `assert_eq!(names.len(), 8)`) and a twin in `money_import_pick.rs:95-125`. A `treasury_*` command is **not covered today**. The plan adds coverage (Phase 2).
4. **The banned-word list is substring based and broad:** transfer, pay, trade, convert, withdraw, send, buy, sell, move, wire, remit, swap, sign, broadcast, order, deposit, exchange (`money.rs:2799-2816`). Names like `*_payout_*`, `*_orders_*`, `*_signups_*`, `*_paypal_*` would trip it. Provider names go in arguments, never in command names.
5. **The relay is health-only today.** `notify_relay.rs:37-41` fixes `KINDS` to 7 health kinds, `CONNECTED` to sleep and recovery. `notify_dispatch_due` builds events from `noop::health_snapshot` only (`:845-860`). A Treasury alert needs a new kind and a fresh native approval (`grew_since_approval`, `:105-109`, `:656-657`). It is not "reuse as is".
6. **Two FX stores already exist.** `ledger.json` `fx` (USD per 1 JPY, with as-of; `business_ledger.rs:14-19`) and `money/fx.json` (JPY per USD, ECB refresh; `money.rs:26`, `:1106`, `:1510-1523`). The prompt does not say which one Treasury uses. Question 5 covers it.
7. **`daily` table holds no money or attention column** (`daily_table.rs:498-503`). It is health, takes and follow-ups. `daily_range` is at `daily_table.rs:904`. Its tags/steps count work done, not hours.
8. **`<data_dir>/business/ledger.json` does not exist on this Mac yet** (`ls ~/Library/Application Support/com.taisun.nexus/business` returned no such directory). Enterprise > Finances shows "Tracking starts when a real source reports in" (`RevenueScreen.tsx:37`). The only venture id in the repo is the test fixture `taiyo-skin` (`business_ledger.rs:290`). Real ids come from Taisun's file.
9. Names that checked out: `backup_schedule.rs`, `needs_you.rs`, `notify_relay.rs`, `keychain.rs`, `commands/shared/{http,sqlite,time}.rs`, `daily_table.rs`, `src-console/bindings.gen.ts`, `bindings.rs`. Enterprise > Finances is route `finances` -> screen `revenue` (`src-console/app/routes.ts:52`).

## 1. Current state (evidence)

**Business ledger.** `read_business_ledger` returns `Option<BusinessLedger>` (`business_ledger.rs:252`). `None` when the file is missing. Struct has `version`, `fx`, `funds`, `ventures`, `funding_requests`, `tax_flags`, `vote_rules`, `revenue_target_jpy` (`:140-150`). Venture = `id, name, stage, health, progress, next_step, note, target_jpy, detail` (`:91-101`). Path `data_dir/business/ledger.json` (`:210-212`). Amounts are integer yen. USD exists only with a hand-entered rate plus date (`:1-4`). Venture ids must be unique (`:200-205`). Ids are free text; only `taiyo-skin` appears in a fixture.
**Who reads it.** `needs_you.rs:9`, `adds.rs:10` (`ventures()` at `:268`, tasks tagged `#venture:<id>` at `adds.rs:6`), `src-console/business/liveLedger.ts`, `bridge.ts:234`, `takes/liveSuggest.ts:35`. Registered at `lib.rs:284`, `bindings.rs:84`, `bindings.gen.ts:39`, `boundTable.ts:25`.
**Finances screen.** Enterprise > Finances = `RevenueScreen.tsx` (design-owned). Read-only, USD only, links to Life > Personal money (`:37`).

**Personal vs business separation (already enforced).**
- Accounts carry `scope` of `personal` or `venture` (`money.rs:273`, `:283`). `balances_list_scoped_in` skips other scopes before opening any file (`money.rs:1326-1344`). `personal_statements_in` skips non-personal accounts (`:1420-1425`).
- `money_summary.rs:25` and `weekly_review.rs:11`: "Business ventures are never read here". Test at `weekly_review.rs:1058` (`a_poisoned_or_unreadable_venture_file_is_never_opened_by_the_personal_read`).
- Decision 0008: "venture accounts stay out of the personal total" (`docs/decisions/0008-money-sources-2026-11-12.md:10`).

**Scheduler.** `backup_schedule.rs`: 60 s tick (`:34`), 5-minute startup delay (`:29-30`), `slot_missed` = newest slot has no success since (`:66-73`), `prev_slot`/`next_run_after` in JST (`:45-63`), `decide()` returns Wait/Skip/Run with busy and battery skips, 3 attempts, 1 h retry (`:32-33`, `:131-156`). `start()` spawns the loop (`:358`). Settings and last-ok live in `backups.rs` (`record_run`, `record_attempt`, `:481-508`). Catch-up for roadmaps is decision 0018 (`goal_roadmap.rs:32,653,785`).

**Needs you.** `needs_you_list` (`needs_you.rs:311`) = pure-read pointers per source. `Pointer{source,id,title,due,reason,deep_link:Route{id,view},badge,quiet}` (`:31-41`; `Route` at `adds.rs:88`). Sources carry `ok | failed | unwired` (`:45`, `:100-116`). A failed source never hides others.

**Relay.** `notify_relay.rs`: Telegram, approved in decision 0014 #3. Pins a hash of the approved chat and the approved kinds (`:62-67`). Token and chat id in Keychain. Kill switch checked before the post. Prose is model text validated against placeholders (`validate_prose` `:380`; `RelayEvent` `:242`). Settings command `notify_settings_set` (`:784`) runs the native approval via `approval_gate::require_approval` (`:801-808`).

**Keychain.** `keychain.rs`: fixed allow-list `SECRETS` of 10 names (`:19-30`), `is_allowed` (`:50`), `secrets_status` returns `set: bool` only (`:117-123`, `:168`), `secret_set` (`:221`) with first-set approval (`:176`). No Treasury entry. CSV-only needs none.

**Shared.** `shared/http.rs` (`HttpSpec`, `builder`, loopback and https-only flags `:8-56`), `shared/sqlite.rs` (`open_private_wal` `:31`: 0600, WAL, busy timeout), `shared/time.rs` (`jst()`, `now_jst()`, `day_key()` `:12-38`), `shared/task.rs` (`blocking`, `JoinErr`). Also `local_store.rs` (`read_json`, `write_json_atomic`, `new_id`, `clean_text`, `:15-91`).

**Daily table.** `daily.db` with `daily`, `daily_goal`, `meta` (`daily_table.rs:498-507`). `daily_range(from,to)` command at `:904`.

**Typed bridge.** `bindings.rs` `builder()` lists commands; a staleness test fails if `bindings.gen.ts` differs (`bindings.rs:1-8`). i64 exports as `number` (`:10-14`). New commands must also be in `commands/mod.rs`, `lib.rs generate_handler!`, `test/commands.registered.test.ts` (`rules/rust.md`), and `boundTable.ts` / `test/unit/bridge.bound.test.ts`.

**Backups.** The packager copies the data dir except `EXCLUDED_SECRET_FILES`, name fragments (token, secret, password...) and key suffixes (`backups.rs:33-62`). `money/` and `business/` are not excluded, so they are backed up today.

## 2. (a) Data model

### Principle
One ledger. `business/ledger.json` stays the registry of ventures, funds, targets and the dated FX rate. Treasury adds **income facts** keyed to a venture id from that file. It does not copy venture fields, targets or FX into a second place.

### Tables (sqlite, amounts as integer minor units)
- `streams(id TEXT PK, venture_id TEXT NOT NULL, name, type, status, setup_date, notes, created_at)`.
  - `type` in `product_sales | digital_product | investment | affiliate | other`.
  - `status` in `active | paused | closed`.
- `income_events(id TEXT PK, stream_id FK, date TEXT, amount_minor INTEGER, currency TEXT, source TEXT, raw_ref TEXT, dedupe_key TEXT NOT NULL, import_id, UNIQUE(stream_id, dedupe_key))`.
  - Refunds and fees are negative rows (the ledger's "no negatives" rule is for targets, not for income facts).
- `imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)`. Feeds the failed-import and stale-data flags.
- `attention_log(stream_id, day, minutes, PRIMARY KEY(stream_id, day, source))`. Minutes, not hours, to avoid float drift.
- Phase 3 (only if Taisun keeps contacts): `contacts`, `interactions`, `segments`. Phase 4: `sequences`, `sequence_results`.

### Venture id mapping
- `streams.venture_id` must equal a `ventures[].id` in `ledger.json`. Checked at `treasury_stream_add` by `read_ledger_in`. Unknown id returns `unknown_venture` (never auto-created).
- If `ledger.json` is missing (today's state), stream creation returns `ledger_missing` and the UI shows "Needs wiring". We do not invent a venture.
- A removed venture leaves its streams orphaned. Reads mark them `venture_missing`; nothing is deleted.
- The `#venture:<id>` tag already used by tasks (`adds.rs`) can later join tagged roadmap steps to streams (question 1).

### Storage: recommend one sqlite file `<data_dir>/business/treasury.db`
Opened with `shared/sqlite::open_private_wal`. Reasons:
1. Idempotent import is a database rule: `INSERT OR IGNORE` on `UNIQUE(stream_id, dedupe_key)`, in one transaction per file. A JSON rewrite can lose or double rows on a crash.
2. Order-level CSVs will pass the ledger's 1 MiB cap (`business_ledger.rs:10`). Raising that cap would loosen a reviewed guard.
3. The reads (monthly per stream, trailing 3 months, hours join) are range queries and group-bys.
4. It keeps `ledger.json` hand-authored and untouched, so no new writer appears in a file Taisun edits by hand.
Cost: the file is not human-readable. Mitigation: a read command that exports a summary.
Rejected: writing into `ledger.json` (needs a ledger writer and approval flow that does not exist; would also change the `BusinessLedger` binding) and `money/` jsonl (that is the personal store).

### Currency and FX rules (fixed in the data model)
- Amounts stay in their own currency. No stored converted total.
- Per-currency totals are always safe. A cross-currency total or a per-hour figure is computed only when a rate and its `as_of` exist. Source of the rate: the ledger's `fx` (hand-entered, dated). Treasury never calls ECB (that host belongs to `money.rs`).
- Today the ledger rate covers JPY<->USD only. Any other pair has no rate, so those totals stay `null` with reason `needs_rate`.
- A rate older than the stale limit (question) is shown with its date and flagged.
- Never `0` for missing. Types: `Option<i64>` / `null`. Income per attention hour = `null` when hours are 0 or absent (`no_hours`), when income is `null`, or when FX is needed and missing.
- Never render "¥" (design rule); the UI gets a currency code.

### Keeping personal and business reads separate
1. Separate file and folder: `business/treasury.db`. It never touches `money/`.
2. Treasury code imports nothing from `money.rs`, `money_summary.rs` or personal stores. A source-pin test asserts it (same technique as `decisions_log.rs:330`).
3. The personal summary and weekly review never import `treasury`. A test pins that too, beside `weekly_review.rs:1058`.
4. Every Treasury read takes no account scope; it only knows venture streams. There is no "all money" command.
5. A `venture` account in `money/` is never read for income. Income comes only by explicit import.
6. Attention log is per stream. Personal hours are never mixed in.
7. A security review checks the above each phase.

## 3. (b) Reuse table

| Component | Path | How Treasury uses it |
|---|---|---|
| Business ledger reader | `src-tauri/src/commands/business_ledger.rs` (`read_ledger_in` `:215`) | Validate venture ids, read `fx` and `as_of`. Read-only. |
| Enterprise > Finances | `src-console/screens/RevenueScreen.tsx`, `src-console/business/liveLedger.ts` | Treasury values arrive through the business adapter (live adapter only: `{state:"live",value}`). Markup is Design's. |
| Nightly scheduler + catch-up | `commands/backup_schedule.rs` (`slot_missed` `:66`, `prev_slot` `:45`, `decide` `:131`, `start` `:358`) | Same pattern: 60 s tick, due when the last-run is older than the newest slot, skip when busy or on low battery, 3 tries. Reuse the `pub(crate)` slot helpers; add a Treasury `start()` called from `setup`. Last-run stored in `treasury.db` `meta`, not in backup settings. |
| Needs you | `commands/needs_you.rs` (`Pointer` `:31`, `report`, `build` `:118`) | Add one source `treasury` that lists open flags as pointers. Deep link to Enterprise > Finances. Pure read. |
| Telegram relay + approvals | `commands/notify_relay.rs` (`KINDS` `:37`, `grew_since_approval` `:105`, `notify_settings_set` `:784`) | Add one kind, `treasury`. Turning it on re-runs the native approval. Message = the Rust-built plain line with no amounts, names or venture detail. No model prose. |
| Keychain | `commands/keychain.rs` (`SECRETS` `:19`, `secrets_status` `:168`) | Not used for CSV-only. A future provider read key is added to the allow-list under [GATE]. Commands return set / not set. |
| `shared/sqlite` | `commands/shared/sqlite.rs:31` | Open `treasury.db` (0600, WAL). |
| `shared/time` | `commands/shared/time.rs` | JST day keys for dates, slots, "14 days with no income". |
| `shared/http` | `commands/shared/http.rs` | Not used with CSV-only. [GATE] for a provider read. |
| `shared/task` | `commands/shared/task.rs` (`blocking`, `JoinErr`) | Run sqlite and file work off the async thread. |
| `local_store` | `commands/local_store.rs` (`new_id`, `clean_text`, `write_json_atomic`) | IDs, text cleaning, small settings file. |
| CSV file picker | `commands/money_import_pick.rs` (native dialog, no renderer path) | Copy the pattern: Rust opens the dialog, so the renderer never supplies a path. A new `treasury_import_pick`, not a shared command. |
| Daily table | `commands/daily_table.rs` (`daily_range` `:904`) | Read-only join for the attention view: steps done per goal per day. Not for income. Treasury keeps its own tables. |
| Typed bridge | `src-tauri/src/bindings.rs`, `src-console/bindings.gen.ts`, `boundTable.ts` | Every command is bound; staleness test must pass (`UPDATE_BINDINGS=1` to regenerate). |
| Guard tests | `test/commands.registered.test.ts:148`, `money.rs:2790` | Extend the TS regex to `^(money|fx|treasury)_` and copy the Rust scan with the same banned list. |
| Local model tiers | `llm_router.rs`, `model_tiers.rs` | Light tier drafts sequences; deep tier writes the monthly review. No Claude tier. |
| Take validation idea | `take_log.rs`, `take_events.rs` | Same rule for the monthly review: every number must match ledger rows or the review is hidden. |

## 4. (c) Outside calls of the finished module

With CSV-only imports the list is empty. Nothing in Treasury makes an HTTP call. All reads and writes are local files and the local sqlite file.

- Local model calls go to the existing loopback tiers (not an outside host).
- The relay alert uses the **already approved** Telegram host through `notify_relay.rs`. That is an existing path, not a new host, but it does send a message off the Mac. Plain line only, no amounts or names. Needs the re-approval described above.

Gated future calls. **Not to be built** without a logged decision:
- [GATE] Email provider read-only API sync (sends, bounces, unsubscribes, purchases). New host. Needs the provider named (question 3).
- [GATE] Payment-platform read API (Stripe, Shopify, PayPal read). New host and a Keychain secret.
- [GATE] FX rate source for non-JPY/USD pairs (for example ECB, which `money.rs` already calls for personal money). Treasury would be a second caller.
- [GATE] Brokerage read API. New host and secret. Investment data stays manual CSV until then.
- [GATE] Amara sending marketing email herself. Out of scope; needs changed outbound rules.
A fixture-backed interface test for the provider read is allowed (as in the prompt). The README section of the module lists "none" until a gate opens.

## 5. Phases 2 to 5

Command names avoid every banned substring (section "Prompt corrections" 4). All return `Result<T, String>` with fixed error codes that never echo input. All registered in `commands/mod.rs`, `lib.rs`, `bindings.rs`, `test/commands.registered.test.ts`.
Each phase ends with all gates (`rules/verification.md`), a security review, a local commit by explicit path, board + handoff update. Full e2e only if the UI is touched.

### Phase 2: ledger, CSV import, attention log, reads
Files: new `src-tauri/src/commands/treasury.rs` (or `treasury/` folder), `commands/mod.rs`, `lib.rs`, `bindings.rs`; TS: `src-console/business/treasury.ts` (live adapter), `boundTable.ts`; fixtures `src-tauri/tests` or inline temp-dir fixtures.
Commands:
| Name | Args | Returns |
|---|---|---|
| `treasury_stream_add` | `venture_id, name, stream_type, setup_date?, notes?` | `Stream` |
| `treasury_stream_update` | `id, status?, name?, notes?` | `Stream` |
| `treasury_streams_list` | none | `Vec<Stream>` (with `venture_missing` flag) |
| `treasury_import_pick` | `stream_id, adapter_id, mapping?` | `ImportResult{added, skipped, failed, import_id}` (opens native dialog) |
| `treasury_import_preview` | same, dry run | `ImportPreview{rows_seen, would_add, would_skip, errors}` |
| `treasury_attention_set` | `stream_id, day, minutes` | `()` |
| `treasury_attention_list` | `stream_id?, from, to` | `Vec<AttentionRow>` |
| `treasury_income_month` | `from_month, to_month` | `Vec<StreamMonth{stream_id, currency, month, total_minor: Option<i64>}>` |
| `treasury_income_trend` | `stream_id?` | `Vec<TrendRow{stream_id, currency, months:[Option<i64>;3], avg_minor: Option<i64>}>` |
| `treasury_per_hour` | `from, to` | `Vec<PerHour{stream_id, minutes: Option<i64>, income_minor: Option<i64>, per_hour_minor: Option<i64>, reason: Option<String>}>` |
| `treasury_fx_status` | none | `{rate: Option<FxRate>, stale: bool}` (reads the ledger `fx`) |
Adapters: trait `CsvAdapter{id, detect(header)->bool, parse(row)->Result<EventDraft>}`. Registry starts with the **generic mapper only** (Taisun maps columns). Stripe, Shopify, PayPal and brokerage adapters are written only from the real header rows he supplies (question 6). No column names from memory.
Tests (fixtures and temp dirs only):
- **Import twice adds nothing:** import fixture A, count rows, import A again, rows unchanged, `skipped == row_count`, `added == 0`. Also a file with overlapping rows adds only the new ones. Same-date duplicate real sales stay distinct (dedupe key includes a row ref, not just date+amount; the key recipe is fixed per adapter and tested).
- Crash mid-import leaves no partial import (transaction test).
- **Null not 0:** no events -> `total_minor: None`; no hours -> `per_hour_minor: None, reason "no_hours"`; zero hours entered -> `None`; missing FX -> `None, "needs_rate"`.
- **FX rule:** mixed JPY+USD with no rate -> no combined total; with a rate and as-of -> total plus as-of returned; stale rate flagged; never a made-up rate.
- Refund rows reduce the month; a month with only refunds is negative, not hidden.
- Unknown venture id rejected; missing ledger returns `ledger_missing`.
- Bad CSV (wrong header, a symlink, over-size, wrong encoding) fails with a fixed code and records a failed import row.
- **Guardrail coverage:** source scan of `treasury*.rs` for banned substrings in every `#[tauri::command]` name; TS test regex extended to `treasury_`; test that no `reqwest`, `http://`, `https://`, `shared::http` or `TcpStream` appears in the module's production source; test that the module never references `money::` or `money_summary`.
- Bindings staleness test passes; `commands.registered` lists every new name.
- Adapter tests: live adapter returns `{state:"live"}` only with real rows, else `unwired`/empty state. Sample data only behind `nexus.sampleAdapters=1`, tagged sample.
Design Lead: the Treasury dashboard, empty states, copy, any new tab or nav entry, charts and tokens. Claude Code stops at the typed bridge and adapter.

### Phase 3: contacts or counts (per question 2)
If contacts stay in the provider (recommended): table `segment_counts(stream_id, as_of, stage, segment, count)`. Commands: `treasury_counts_import_pick(stream_id)` -> `ImportResult`; `treasury_counts_list(stream_id)` -> `Vec<CountRow>`. No names, no emails ever read into memory beyond parsing: the importer reads only stage/segment columns and counts. Test: a fixture with fake emails produces counts and no email string anywhere in the db file (byte scan of `treasury.db`).
If contacts live in Amara: separate file `business/contacts.db` (not in `treasury.db`), tables as in the prompt. Commands: `treasury_contacts_import_pick`, `treasury_contacts_count` (counts only), `treasury_segment_save/list/run` (run returns **counts**, never rows, to the renderer by default; row export is a separate approved command). Stages `lead|waitlist|first_purchase|repeat|lapsed`. Needs the backup decision, a security review, and an `EXCLUDED_SECRET_FILES` entry if kept out of backups (`backups.rs:33`).
Tests: import twice idempotent; contacts never appear in any `log!`/`println!`/`eprintln!` (source scan); no contact string in chat context, `amara_turn`, `terrence_context`, Letta or take paths (source scan on those modules for `treasury`/`contacts`); consent fields required; no cloud tier call.
Design Lead: segment screen and copy.

### Phase 4: sequence drafts, checklists, provider-results import
Commands: `treasury_sequence_draft(kind, venture_id)` -> `Draft{kind, emails:[{subject, body}], checklist:[...], status}`; `treasury_sequence_approve(id)` -> `Draft`; `treasury_sequence_list()` -> `Vec<Draft>`; `treasury_results_import_pick(sequence_id)` -> `ImportResult` (sends, bounces, unsubscribes, purchases after a sequence; built from his real provider export header, question 6/3).
Kinds: `welcome` (3 emails), `post_purchase`, `reorder`, `win_back`.
Rules enforced in code: every draft has an unsubscribe placeholder and a mailing-address placeholder; draft refused if either is missing (test). Checklist always includes "only contacts with recorded consent; honour opt-outs at once". Local light tier only; the draft prompt contains no contact data (test). Copy needs one approval per template, stored as `approved_at`. A changed draft becomes `draft` again.
[GATE] provider API sync: interface + fixture test only. [GATE] Amara sending: not built.
Design Lead: sequence editor screen, copy tone of the checklist.

### Phase 5: exception monitor and monthly review
Commands: `treasury_flags_list` -> `Vec<Flag{id, kind, stream_id, since, detail}>`; `treasury_flags_ack(id)`; `treasury_monitor_run_now` -> `RunResult` (manual run, same code path); `treasury_review_read(month)` -> `Option<Review>`; `treasury_review_build(month)` -> `Review`; `treasury_monitor_settings_get/set` (stale-days N, thresholds).
Scheduler: nightly job in the style of `backup_schedule.rs` with catch-up. Runs `decide()`-like checks (busy, battery).
Flags (and only these): income down more than 30% vs trailing average; failed import; bounce/unsubscribe spike in imported results; a sequence with zero conversions after 30 days; no income for 14+ days; **stale data** (no import for N days).
- Stale data is its own flag, shown first. A stream with no import for N days is never reported as healthy. With no flags and fresh data the status is "ok as of <last import date>", not a bare "all fine".
- No flags -> no notification. Flags -> a Needs-you pointer and, if the relay kind is approved, one plain relay line. De-duplicate: one alert per flag until it clears.
- Voice only if a local speak path already works (navy `speak` is still unwired, `HANDOFF-FOR-REVIEW.md:120`), so none now.
Review: deep local tier writes one page. Every number is parsed out and matched to ledger rows; any mismatch hides the review (test with a tampered number). Missing months stay "no data", not 0.
Tests: each flag fires at the threshold edge and not below; catch-up fires once after a slept night; no flags sends nothing; stale flag fires with CSV silence; relay text has no amounts (source scan); relay approval pins the new kind; flag survives restart; review validator rejects a wrong or invented number.
Design Lead: flag cards, review page, where stale vs ok shows.

## 6. Risks

1. **Personal data in backups.** `business/` is copied into every backup today (`backups.rs:33-62` does not exclude it). Income and, if kept, contacts would sit in each archive and in any off-Mac copy Taisun makes. Mitigation: counts only by default; if contacts exist, a separate `contacts.db` with a logged in/out of backups decision and a restore test. Never log row content.
2. **Contacts in chat context, logs, Letta, takes.** Easy to leak through a "summarise my customers" path. Mitigation: contacts never enter any model prompt; source-scan tests on `amara_turn`, `terrence_context`, memory writers, `unified_log` and take code; segment runs return counts only.
3. **Consent and law.** US CAN-SPAM is not the only rule. Japan requires opt-in for marketing email, and EU customers bring GDPR (consent, access and erasure rights). This is a flag for Taisun, not legal advice. Provider-run sequences and recorded consent reduce risk but do not remove it. Consider a short check by a qualified person before the first send. The app stores `consent_status` and `consent_timestamp`, and drafts always carry unsubscribe and address placeholders.
4. **Stale data read as "all fine".** CSV only works if he imports. Silence is the most likely failure. Mitigation: stale-data flag, "as of <date>" on every number, and no all-clear wording.
5. **Wrong numbers.** Duplicate or missing rows distort income. Mitigation: import-twice test, import log, refund rows kept, review validation.
6. **FX misread.** Mixing currencies silently. Mitigation: no cross-currency total without a dated rate; rate age flagged.
7. **Relay leaks.** Income level or flags sent to Telegram. Mitigation: plain line with no amounts or names; re-approval; kill switch honoured.
8. **Guard gap.** `treasury_*` is outside the existing money guard. Mitigation: extend tests in Phase 2 before any command lands.
9. **One committer.** No code while another lane has uncommitted work in this worktree.
10. **Second FX store.** Two rate sources can disagree. Mitigation: Treasury reads only the ledger rate.

## 7. (d) Questions for Taisun

Answers must be logged in `docs/decisions/` (next number is 0032) before any build. Recommended default for each is below. If he does not answer, the default is **not** assumed for a build; Phase 2 waits.

1. **Attention hours source.** Quick manual log, tagged roadmap steps, or both? Default: **manual log only** (minutes per stream per day). Roadmap steps carry no time (`daily_table.rs:498-507`), so tagged steps would be guesses.
2. **Customer contacts in Amara?** Default: **no**. Counts and segments only; names and emails stay in the provider.
3. **Email provider.** Which one runs the sequences? Default: none chosen yet; Phase 4 builds provider-neutral drafts and checklists and waits for his pick before a results adapter.
4. **Placement.** Under Enterprise (new section beside Finances) or a new "Treasury" tab? New nav label needs his logged decision (hard rule 4) and Design Lead's layout. Default: **under Enterprise**, no new nav label.
5. **Currencies and FX rule.** Which currencies (JPY and USD only, or more)? Default: **JPY and USD**, keep each in its own currency, combine only with the hand-entered dated rate in `ledger.json`, no automatic refresh. A third currency waits for a [GATE] rate source.
6. **Real export files.** One header row plus two scrubbed sample rows from each source he uses (Stripe, Shopify, PayPal, brokerage, and the provider's results export). Default: ship the **generic column mapper only** until he supplies them.
7. **N days for stale data.** Default: **8 days** for streams he imports weekly (one missed week plus a day); 35 days for monthly sources such as a brokerage statement (matches the 35-day rule in `money_summary.rs`). Per-stream override.
8. **Contacts in or out of backups** (only if Q2 is "yes"). Default: **out of backups** (separate file, excluded), with a note that restore will not bring them back.
9. **Relay for Treasury flags.** Add a `treasury` alert kind to Telegram (plain line, no amounts, fresh approval)? Default: **Needs-you only** until he approves the new kind.
10. **Where is the real `ledger.json`?** It does not exist on this Mac. He must create it (with venture ids such as the skincare brand's id) before streams can be added. Default: Phase 2 ships a documented minimal example in the test fixture only; he supplies the real file.
11. **Rate choice.** Use the ledger `fx` and ignore `money/fx.json`? Default: **yes**, ledger only.
12. **Raw rows kept?** Store the raw CSV line reference (`raw_ref`) only as a row number and file hash, not the line text. Default: **yes**, so no customer detail enters the db.
13. **Refunds and fees.** Count as negative income in the same stream? Default: **yes**.
14. **Legal check.** Does he want a short review of consent rules (Japan opt-in, GDPR, CAN-SPAM) before the first provider send? Default: **yes, before turning the welcome sequence on**.

## 8. Order of work after approval
Phase 2 first and alone. One phase per run. Stop and report after each (`PROMPT` build order). Nothing is pushed, merged or opened as a PR. The prompt file `tasks/PROMPT-TREASURY-2026-11-15.md` is untouched.
