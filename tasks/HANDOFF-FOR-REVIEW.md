# Handoff for review — Claude Code, 2026-11-11 → 2026-11-12

For Taisun, Nexus Lead and Nexus Design Lead. Branch `amara-integration` in `~/projects/nexus-amara`, **local commits only. Nothing pushed, merged or PR'd** (`docs/decisions/0001`; the local pre-push hook refuses without `NEXUS_PUSH_OK=1`, decision 0007 #9). Range: `cd5e4cc..HEAD` (HEAD = this commit, after `7de5b4b`).

Taisun's 7 uncommitted docs (BUILD, CLAUDE, NEXT_SESSION_PROMPT, README, STAGES, TAISUN-STEPS, amara-router/README) are **still uncommitted and intact**; earlier index-only hunks left his lines unstaged. `tasks/claude-goal-prompt-1111.md` and `docs/architecture/local-multi-agent-design.md` are his and untracked.

Design's own commits in the range (never touched): `11a9d6d` 1110g, `d1f2168` 1110f, `c1fcd9a` 1110i, `046eedc` 1111a, `3ca4ea2` 1111r, `2efc057` `1a90cbb` `07f7ca6` `70e90a8` 1111s, `e7d2ee4` 1111t, `accf8f6` 1111u, `c888c52` 1111v, `554fe28` `b306208` `8a2e7a0` 1111w, `66b7272` `bffe1e4` 1111x.

## 1. Gates on HEAD (2026-11-12, first full combined run since `021ddea`)

| Gate | Result | Baseline cd5e4cc |
|---|---|---|
| tsc, lint | clean | clean |
| vitest | **1523 passed / 9 skipped** | 1140 / 9 |
| test:neuro | 21 | 21 |
| check:secrets | ok (821 tracked files) | ok |
| src-tauri fmt / clippy -D warnings / test | clean / clean / **542 passed, 5 ignored** | 418 / 5 |
| amara-router fmt / clippy / test | clean / clean / **126** | 112 |
| crew-service (Docker test image, `--network none`) | **222 passed** | 56 at 5f4eca9 |
| **Full e2e** (`npm run test:e2e`, port 4179) | **728 passed, 12 skipped, 0 failed, 0 flaky** (3.0 min) | not run before |

No counts dropped. **Not run:** WebKit (not configured), Linux CI, anything live: the app was not relaunched, the router and the crew container were not rebuilt, no headset, no live Postgres for the new crew tables (they are created at crew startup after the rebuild).

## 2. Commits by area (mine; Design's listed above)

- **Dev system:** `0cc6d7e` agents/rules/guard hook/skills/decisions/board; hook hardening `5f3a579` `19b3bc8` `b4b251d`. Pre-push hook installed locally (not versioned).
- **Section 11 backlog (all 20 + route-motion flake):** `bb6669a` router (deep warm, auto_deep off, remote-Ollama flag, cloud needs bearer, phrase strip), `7a1f47e` `8b0456f` follow-ups gate + app fixes, `47e95a1` nav focus + e2e + unit tests, `9609615` CI pin + notarization guard, `b869bf0` Hermes 8642 note, `bb2d574` README nav patch (proposed; his README). Router SIGKILL cause **not proven** (`amara-router/STABILITY.md`). Item 17 not run on Linux.
- **Section 12 wiring:** `8d96417` `534d19f` take backend (local only), `a34a224` `e182d9f` Muse recordings (no delete path), `153c7ce` workouts/hypnogram/voice stop-speaking, `8b6265e` `89317c9` Business ledger. Nutrition: **no data source** (gap, nothing invented).
- **Decisions 0007 (A):** pre-push hook; `a2a7ac5` save_session refuses a duplicate id (fail-before/pass-after); `021ddea` MiniMax removed, concurrency 1, read-only tools by default, batch approvals, retries 2; `a6fd349` follow-ups one approval per session with the edit list; recording delete path: none (`removeConfirmed` no-op); `58374e6` Letta plan + inventory (plan/list only); `b9e9efe` router bearer from Keychain.
- **B liveBusiness crash:** `89317c9`: a missing file, a non-object (incl. `[]`) or a ledger without ventures/funds is unwired, never throws; fail-before/pass-after unit test incl. `[]`. Design's e2e mock (`70e90a8`) kept. Still "Needs wiring" until the ledger file exists.
- **1111u Home HR take:** `8f6eba5` (15-min query, 30-min rule in the backend, take store, trigger on open/focus).
- **1111x navy localOnly:** `6215a12` (cloud tiers off + outbound blocked while on, tested).
- **Section 13 crews:** A `5f4eca9`, B `1b57e9e`, D `021ddea`, **C/E/F + 1110i `7de5b4b`** (below).
- **D HANDOFF wiring:** 1110f `8d96417` `534d19f`; 1110i `7de5b4b`; 1111s secrets status/set + n8n passphrase key file `889bd19` `65ffe02` `149cd68`; 1111t take log `8f6eba5`, steps 2–5 `65663f3`; 1111v backend `97eddbb`.
- **E take → source:** `84333aa` (Neuro/Enterprise chips stay inert: no source ids on those pages; nothing invented).
- **F OpenClaw cleanup:** `889bd19` (not-installed default with tests; packager no longer expects ~/.openclaw; `gateway` alias kept; tokens untouched; backup at ~/Applications-backup untouched).

## 3. Crew lane `7de5b4b` and its security review

Built: R13-C task contract and loop (retry limit 2; no success without evidence; verification runner executes nothing unless BOTH `CREW_VERIFY_EXEC=1` and `CREW_VERIFY_NO_NETWORK=1`, set nowhere). R13-E capped, redacted run summaries (no-op or local file writer; **no Letta writer**: `select_writer("letta")` refuses) and skill proposals as data only through Needs you. R13-F `POST /crews/trigger` (HMAC-SHA256 over `ts.body`, 5 min skew, replay refused, 403 on any failure, own secret) and read-only loopback `n8n_status`. 1110i Files/terminal switch per decision 0007 #8.

The security review, cut off last night, was re-run in full this session. Checked: no interpreter, socket or network program reachable from `shell`; per-program option grammar; every path inside the crew folder after symlinks; fs_write refuses symlinks and `.git`; git read-only and hardened; no out-of-sandbox writes; approve and turn-on go through the native dialog; no secrets in logs or audit. **Fixed** (each with a test: 5 failed with the fixes reverted, 222 passed with them):
1. Trigger replay bypass: the same signature with whitespace around it replayed (check used `strip()`, replay key didn't).
2. Runs started by the n8n trigger could claim/use Taisun's files/terminal grant. Now they never get one.
3. `rm -rf .` / `mv .` on the sandbox folder would destroy the git checkpoint. Refused.
4. `grep -R` (follows symlinks out of the folder) removed; `-r` stays.
5. Compose `environment:` blanked `CREW_TRIGGER_SECRET` over `env_file`, so the trigger could never be enabled. Removed.

Known, left: `diff -r` dereferences nested symlinks, but a crew cannot create one; `scripts/sync-crew-env.sh` rewrites `.env.crew` with only the Ingress key, so a hand-added trigger secret is dropped (fails closed) — teach the script before enabling the trigger.

Design-owned file touched: `CrewsScreen.tsx`, logic only (adapter hooks chained; no markup, class or token change).

## 4. Ops (list/plan only)

- `docs/ops/inventory-2026-11-11.md`: n8n duplicates `sMtd9mnMhXid8MQn`, `laKQPqbtCQcWT3XL` (inactive Ingress copies), OpenClaw monitor `WxCNKfPXqcn6YY4Q` (keep off), `envProbeStage5E1`; stale containers `freqtrade` (exited), `nexus5etest-postgres-1` (never started), an unnamed old openclaw-sandbox container. Nothing deleted, stopped or modified.
- `docs/ops/letta-upgrade-plan.md`: Letta 0.16.8 → first release fixing CVE-2026-77937 (target tag not verified: outbound off). Backup, migration notes, pin, check, rollback. **Not applied**; Letta data untouched.

## 5. Not started (left for the next session)

- **1111a** personal-OS adapters (goals, patterns, journal/mood, memory, life admin, weekly review).
- **1111t step 6 money:** research spike first; no sign-ups/aggregators; Rust seed guard; `money.test.ts` must keep failing on transfer/pay/trade/convert/withdraw; never invent an FX rate.
- **1111w Jarvis windows:** child webview + pop-out, per-site profile, `home_window_open`, local-only comments, Life take facts.
- **1111s backups step 3** (restore never automatic).
- Voice adds via on-device transcription; chat/voice call sites for the one add path.
- `scripts/sync-crew-env.sh` to carry `CREW_TRIGGER_SECRET`.

## 6. Taisun steps (folded from `tasks/active/TAISUN-NOTES-draft.md`; his TAISUN-STEPS.md not edited)

Rebuild the crew image (TAISUN-STEPS §9.9); startup then creates `crew_tasks`, `crew_tool_grants`, `crew_tool_audit` and widens `crew_approvals.kind`. Optional, all default off: `CREW_TRIGGER_SECRET` in `stack/.env.crew`, `~/crew-sandbox` + volume + `CREW_SANDBOX_ROOT` (run the container network-less before ever enabling shell), `CREW_MEMORY_WRITER=file` + `CREW_MEMORY_DIR`, `CREW_SKILLS_DIR`, app-data `n8n_api_key` (0600). Router rebuild/install and the fast-model switch (decision 0004). Hermes `API_SERVER_KEY` rotation (24 bytes → ≥32) in the final rotation batch. Confirm GitHub secret `APPLE_API_KEY_P8`.

## 7. Open questions for Taisun

1. Navy mode can't stop Hermes's own web tools: route chat to the local router while navy is on, or pause chat?
2. Follow-ups once-per-session approval OK as built?
3. `save_skill` etc. now write app-data `workspace/` (legacy ~/.openclaw is a read-only fallback). OK?
4. The n8n key file uses PBKDF2 (600k) + AES-256-GCM (ring) instead of Argon2id (offline build). OK?
5. 1110i: Design's `toolGrant.ts` default says "1 hour" vs decision "this run only" (backend enforces this-run-only; Design to align copy).
6. 1110i/R13-E mounts: `~/crew-sandbox` + `CREW_SANDBOX_ROOT`; where `CREW_SKILLS_DIR` / `CREW_MEMORY_DIR` map in app data.
7. A real shell for crews needs a network-less sandbox; until then shell stays file utilities only.
8. Then the NEXT_SESSION_PROMPT order: home control, real Muse session, voice with headphones, slow first Chat turn, calendar, eyes-closed test, key rotation, backup prune, push (each with his OK).

## 8. Since 1f84665 (2026-11-12, continued at Taisun's "keep working")

| Commit | What |
|---|---|
| `32f1d8c` | Crews: git control files refused in any case/Unicode spelling + same-inode check + `attr.tree` empty (a `.GIT/config` filter could run a command on the case-insensitive mount); triggered runs get no grant or request tool; `sync-crew-env.sh` carries `CREW_TRIGGER_SECRET` |
| `60a72e6` | Crews: run origin stored (`crew_runs.triggered`); a retry of a triggered run stays grant-free |
| `d6bee73` | Design question: 1111a panels tag live values "Sample" (blocks goals/patterns/weekly review/money/memory wiring) |
| `3be01d3` | Money research spike (`docs/research/money-spike-2026-11-12.md`) |
| `b8a3740` | Decision 0008: SMBC + Yucho CSV, ECB/Plaid hosts approved, no aggregator in v1, Ledger watch-only |
| `6507382` `9ceaadd` `0b2a312` `d44a4d7` | 1111s backup step 3: settings, Back Up Now, list (no restore UI, no prune, no nightly timer); secrets never in any archive (files + secret JSON fields); Desktop `pack_it_up` too (Taisun OK); a restore keeps live secrets only while their endpoints are unchanged |
| `60a47d8` | Decision 0009: sidebar Home, Life, Mind–Body, Enterprise, Agents · System: Smart home, Settings (⌘1–7 follow) |
| `3a3a935` | Voice test flake (wall clock) fixed with a frozen Date; proven with an injected stall |
| `7b448bb` | Money backend, read-only: seed guard (hardened after review), accounts/watch/manual balance, SMBC/Yucho CSV import (ASSUMED formats, synthetic fixtures; real samples last, per Taisun), ECB rate (kill switch + navy, no proxy, https only) |

Three automated commit reviews plus one agent security review ran on this work; every finding was fixed with a fail-before/pass-after test (one LOW, the shared check-then-reopen pattern in `local_store.rs`, is left and noted).

**Gates on HEAD:** src-tauri 590 passed / 5 ignored · vitest 1535 / 9 skipped · neuro 21 (not rerun since 1f84665: no neuro change) · router 126 (untouched) · crew-service 228 · fmt, clippy -D warnings, tsc, lint, check:secrets clean · full e2e on 0b2a312: 725 passed / 12 skipped / 3 failed → the 2 landing failures were the order change (fixed in d44a4d7, spec now 46/46); `route-motion.spec.ts:78` failed once under full-suite load and passes 3/3 alone (logged flaky, not loosened).

**Blocked / needs Taisun:** 1111w windows (browsing arbitrary sites = new outbound hosts; needs his OK + Tauri unstable multi-webview); voice adds (an amara-voice transcription endpoint patch he applies); money UI + 1111a live wiring wait on Design's sample-tag fix; money read commands (balances/transactions) not built yet. App rebuild/install to see the new sidebar is his step.

## 9. Neuro, Hemi-Sync and the system map (2026-11-12, late)

| Commit | What |
|---|---|
| `318aa17` `a972a7e` | Decision 0010 + Hemi-Sync backend: import (copy into app data; source read-only), library by wave, id-only `hemisync://` audio scheme (CSP `media-src` only), session links/finish/rating, local `<nexus_hemisync>` Amara section through the injection guard. Security review: 2 MEDIUM + 5 LOW fixed with tests. |
| `779b590` | Neuro redesigned in the Mind–Body language (Design Lead, HANDOFF-1112n) + Hemi-Sync UI: one Start (Recording only + track), rating sheet, wave of the last session open, 42-track checklist. Muse session id now exists from recording start (reviewed: no recording can be lost). |
| `bbe906f` `b00c3a6` `47b89cb` `79d76da` | Decision 0011 + system map at the top of Settings › Services: 16 paths, 11 read-only loopback probes, green / blinking red / grey / hollow, click for reason + documented fix, 10 s refresh while visible (HANDOFF-1112m). |

**Gates on HEAD:** src-tauri 648 passed / 5 ignored · vitest 1580 / 9 skipped · neuro 21 · fmt, clippy -D warnings, tsc, lint, check:secrets clean · full e2e 747 passed / 12 skipped / 1 failed (`takes-1111t.spec.ts:80` light-theme contrast in the money Connect sheet: fails on `0b2a312` too; logged in design-questions). `route-motion.spec.ts:63/:78` fail occasionally under full-suite load only (8/8 and 3/3 alone; logged, not loosened).
**Not run live:** real Hemi-Sync audio playback through `hemisync://` in WebKit, a real import of the Desktop collection, a Muse session with a track, and the system map against the real services. All need the app rebuilt and installed (Taisun's step); a live screenshot of the map is still owed (the Design Lead flagged some lines passing behind nodes in the straight-line layout).
