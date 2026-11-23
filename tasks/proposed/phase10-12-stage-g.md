# Stage G / Phases 10–12: NOOP merge 0–3 + WHOOP ownership, Telegram relay, Lucid cues (plan, read-only research, 2026-11-15)

Sources: HANDOVER-FINAL `:369-419` (Phases 10–12), `:123-189` (definition of done), `docs/design/HANDOFF-1111s.md:25-26`, decisions 0014/0017/0020/0021/0025. Not found: `noop-merge-map.md`, and the goal prompt's Stage G line is not in the repo.

## Findings
- **Phase 10 is mostly unbuilt.** Only `whoop_status` exists (read-only: owner, storeFound, lastSampleAt, gaps14d). The owner is `amara` only via a `whoop-owner` marker that nothing writes; `legacy` comes from a 10-minute mtime heuristic. No take_over/sync/buzz/pair/forget, no lock, no `docs/noop/freeze.md`.
- **NOOP read path:** `noop_query.rs` runs `noop-local-access mcp` under `sandbox-exec (deny network*)`, with a 5-tool allowlist and 9 metric keys. The default binary is in `~/projects/noop-lucid-current/...` (it doesn't work if ~/projects is missing). There is no hash pin. The store is opened READ_ONLY and Rust never runs DDL.
- **Phase 11 is mostly built** (`notify_relay.rs`): Keychain token and chat id, model prose with placeholders and `validate_prose` (no digits), the `api.telegram.org` host pin, a ledger, the kill switch, Send Test with approval.
  - Gaps: only sleep and recovery are connected; there's no timer; the number check is against the NOOP snapshot, not `daily_range`; the plain fallback line's numbers aren't checked; DEEP/REM/RHR have no daily_range column.
  - A legacy plaintext `config.json` Telegram path is still in `telegram.rs` (Q7).
- **daily_range** (`daily_table.rs:904`): NOOP columns are sleep_total_min, hrv_ms, recovery and strain (0–100), with per-column sources and NULL = no data.
- **Lucid** = NOOP's Lucid Dream Mode (`~/projects/noop-lucid-current`, branch lucid/v12: LucidDreamController / RemDetector / Policies). A cue is a strap haptic plus a local notification. `bf90377c` stops stale HR counting toward a cue. Nothing Lucid is in Amara, and no Muse code is involved.
- **The NOOP repo has 6 local commits.** Licence: PolyForm Noncommercial 1.0.0, © 2026 NoopApp. The Info.plist Bluetooth text mentions Muse only.

## Design
- **Phase 10:**
  - P0 freeze tool: counts, checksums, v47, and Keychain item names only; the copy happens only with NOOP.app quit.
  - P1 sidecar `noop-local-access` with a SHA-256 pin, falling back to the ~/projects path.
  - P2 `noop-syncd` (Swift; status/sync_now/live_hr/battery/haptic_buzz; flock `whoop.lock`; refuses while NOOP.app runs; supervisor row act-off).
  - P3 SQLite backup-API copy to `<data>/health/whoop.sqlite`, verified by integrity_check, migrations, counts and max timestamps, done atomically. The old store is kept 30 days, and NOOP_DB_PATH is changed through the approval-gated settings.
  - `whoop_control.rs` commands fail closed (`locked_by_legacy` / `takeover_disabled`).
- **14-day parity report** (`noop_parity.rs`, `noop_parity_report(days≤31)`): daily_range vs the legacy store read through the same sandboxed CLI, compared per day and measure with tolerances. Days missing on one side are marked, never filled. The live report goes to `<data>/parity/` (0600), never the repo; the repo gets a fixture sample. Q4 decides what side-by-side means after the takeover.
- **Relay number check** (`relay_check.rs`):
  - Every number in the final text and the plain line must trace to the `daily_range` row for that day; a stale (>36 h) or NULL row blocks the prose.
  - Fallback is the plain line, and then nothing is sent.
  - DEEP/REM/RHR placeholders are dropped (Q5).
  - Other kinds connect only once their real source exists.
  - A timer runs `dispatch_due` only when outbound is allowed.
- **Phase 12:**
  - Oura/Hevy/coach secrets move via `keychain_migrate` (fake Keychain in tests).
  - `lucid.rs`: off by default, status and cue log, with the bf90377c rule as a mirrored Rust test and QA on a REM fixture.
  - NOTICE/ATTRIBUTION copied verbatim into bundle resources; the About copy is Design's.
  - NOOP.app retirement and old-store deletion are Taisun's only.

## Lanes
G1 parity (`noop_parity.rs`) · G2 relay check (`relay_check.rs` + `notify_relay.rs`) · G3 read path + freeze (`noop_query.rs`, `noop_freeze.rs`) → G4 WHOOP control + lock + data copy · G5 Swift `noop-syncd` + `lucid_cue` (NOOP repo, after its backup) · G6 lucid, secrets, attribution · G7 TS wiring + ALL shared registration (mod.rs, lib.rs, bindings, boundTable, bridge, adapters, registered test, tauri.conf.json). G1–G6 hand over snippets.

## Gates / Taisun steps / open
[GATE] WHOOP takeover on hardware; Telegram bot + token + first Send Test; NOOP.app retirement (14 d) + old-store deletion (30 d); secrets move per item; sidecar hashes/signing; running noop-syncd (act).
Taisun: git-bundle the NOOP repo; quit NOOP.app for the freeze/copy; supply/build the Swift binaries + log their hashes; run the takeover; make the bot; approve the secrets; watch a night of cues; parity sign-off.
Open: Q1 noop-syncd source location; Q2 takeover settings flag (yes, default off); Q3 real reports committed? (no); Q4 parity meaning after takeover; Q5 drop DEEP/REM/RHR (yes); Q6 where the Oura/Hevy/coach secrets live; Q7 migrate the legacy config.json Telegram token; Q8 haptic + notification vs audio.
