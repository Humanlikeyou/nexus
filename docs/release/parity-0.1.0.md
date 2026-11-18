# Parity matrix 0.1.0 (Phase 17)

Base commit: `71d8d7e` (+ this run's uncommitted Phase 16/17 files). Evidence is a commit hash from
`git log`, a test file that exists in the repo, or a TAISUN check.

**Source of the rows.** The handover (`tasks/HANDOVER-FINAL-2026-11-12.md`) points at
`standalone-app-plan.md` section 4 ("capability inventory"). That file is **not in the repo and not found
anywhere under `~/projects`** (searched by name and by content). The rows below are therefore built from
the handover's own definition of done (section c) and its service and feature lists, not from section 4.
If Taisun supplies `standalone-app-plan.md`, diff its section 4 against this table; any row it has that
this one lacks is **not verified**.

Status: `pass` = automated test or recorded commit evidence in this repo; `fail`; `waiting-on-Taisun` =
needs live hardware, a login, a key, a decision or his sign-off. "Gates" = `.claude/skills/run-gates/gates.sh`
(all green this run, see READINESS-0.1.0.md).

| # | Capability | Evidence (commit / test / check) | Status |
|---|---|---|---|
| 1 | Chat, light and deep tier through the router | Router `amara-router` tests (131 pass); `5e8ead7` amara_turn; `test/e2e/chat-amara.spec.ts`, `home-chat.spec.ts`. TAISUN: live light/deep reply | pass (automated); live waiting-on-Taisun |
| 2 | Navy mode (go dark, local only, incl. Hermes chat) | `6215a12`, `0dd3996`, `cf93fd0`; `test/e2e/navy-1111x.spec.ts`. Hermes web tools not covered (Q1 open); voice side needs `tasks/proposed/amara-voice-navy-gate.diff` applied | pass (app side); waiting-on-Taisun (voice patch, Q1) |
| 3 | Crews: roster, approvals, read-only tools, signed n8n trigger | `7de5b4b`, `021ddea`, `32f1d8c`; crew-service Docker tests 230 passed (this run, `--network none`). Live crew image rebuild is a TAISUN step | pass (tests); live waiting-on-Taisun |
| 4 | n8n ingress, `kb.query`, crew read-only key | `stack/n8n/workflows/eYdwlwZKTMV89z2J.json`; `tasks/proposed/kb-parity.mjs`; `test/scripts/kbParity.test.ts`. TAISUN: workflow count and names, `kb.query` hits on the new Mac (migration-test.md Part B) | pass (tests); waiting-on-Taisun (live) |
| 5 | Letta memory read, edit, undo (stays 0.16.8) | `8af876a` (0015); `58374e6` upgrade plan only. PATCH not run live (needs pg_dump of `letta_db` first) | pass (tests); waiting-on-Taisun (live write) |
| 6 | Hermes gateway and tools | Supervisor probes `2642629`; turn pipeline `5e8ead7`. Hermes lives in `~/.hermes` and was not touched | pass (probe/tests); live waiting-on-Taisun |
| 7 | Home Assistant (smart home) | `test/e2e/smart-home.spec.ts`, `ha-settings.spec.ts`; HA token to Keychain-first reader `a1d1cb5` | pass (automated); live waiting-on-Taisun |
| 8 | Muse EEG: live, replay, recordings | `test/e2e/recordings.spec.ts`, `neuro-flow.spec.ts`, `neuro-1112n.spec.ts`; `npm run test:neuro` 21 pass. Live BLE and a known-file replay: TAISUN | pass (automated); waiting-on-Taisun (hardware) |
| 9 | Muse recordings never deleted, no delete path | `a34a224` (delete_session removed), `e182d9f`; backups copy Muse files byte-for-byte (`de30ebe`). Count check in READINESS-0.1.0.md | pass |
| 10 | Health store: NOOP read-only, daily table | `5994114`; `test/e2e/fitness-0017.spec.ts`; `bddb46f` parity report (pre-takeover meaning, Q4 open) | pass (tests); waiting-on-Taisun (live parity run, Q4) |
| 11 | WHOOP 4 ownership (single writer) | Not built (needs takeover on real hardware) | waiting-on-Taisun (phase 10 gate) |
| 12 | Health sync sidecars bundled and signed | `prepare-sidecars.sh`, `files{}` in `tauri.conf.json` (checked by `test/scripts/stageH.test.ts`); signed path not run (no Developer ID) | waiting-on-Taisun |
| 13 | Takes: memory, daily suggestion, answers, outcomes | `8f6eba5`, `65663f3`, `bda3c15`; `test/e2e/takes-1111t.spec.ts`, `takes-done-fold.spec.ts` | pass (see e2e line for the light-theme contrast issue) |
| 14 | Home heart-rate take | `8f6eba5`, `c3a15a6`; `test/e2e/hr-take-1111u.spec.ts` | pass |
| 15 | Life, Enterprise, Needs you today, decisions log, adds | `97eddbb`, `16b78d8`; `test/e2e/life-enterprise-1111v.spec.ts` | pass |
| 16 | Goals, roadmaps, check-ins, patterns, weekly review | `36d4378`, `bce7ebf`, `8af876a`, `f45829a`, `d6c44c0`; `test/e2e/goals-1112g.spec.ts`. Habits data source and 22:00 reminder not verified | pass (built parts); habits waiting-on-Taisun |
| 17 | Money (read-only), JP bank CSV, FX | `7b448bb`, `a783a5f`, `9b95d68`; `test/e2e/takes-1111t.spec.ts`. JPY to USD rule, ledger data, aggregators gated | pass (read-only); waiting-on-Taisun (gates) |
| 18 | Treasury | Phase 2 `3092346` (decision 0032). Phases 3-5 not built | partial: phase 2 pass; 3-5 waiting-on-Taisun |
| 19 | Jarvis windows (3 cap, LRU, Undo, per-site profiles) | `c024eeb`, `710863c`, `2c4a8a8`; `test/e2e/home-windows-1111w.spec.ts`. Live sign-in is a TAISUN check | pass (automated); waiting-on-Taisun (live) |
| 20 | Telegram relay of NOOP notifications | `a783a5f`, `bddb46f` (G2 number check). Timer inert; first real send needs token, chat and Send Test | pass (tests); waiting-on-Taisun (live send) |
| 21 | Nightly backups, 14 kept, 0600 in 0700 | `6507382`, `1c93d5e`, `a783a5f`, `de30ebe`; `docs/release/restore-drill.md`; `scripts/migration-probe.sh` | pass |
| 22 | Restore on a copy (counts and hashes match) | `docs/release/restore-drill.md` (3,294 of 3,294 hashes, 2026-11-13); synthetic probe this run (`migration_probe_tests`) | pass |
| 23 | Migration to a fresh account | `scripts/migration-probe.sh` (automated part pass); `docs/release/migration-test.md` Part B | waiting-on-Taisun |
| 24 | No path hard-coded to this Mac | `test/scripts/noAbsolutePaths.test.ts` (new this run). Caveat: some runtime paths are `~/projects/...` fallbacks (`ha.rs`, `kb_status.rs`, `noop_query.rs`, `continuity.rs`, `openclaw.rs`, `backup_schedule.rs` BUNDLE_REPO); they are home-relative, not absolute, and work only where those folders exist | pass (no absolute path); residual note |
| 25 | Services supervisor (probe, restart max 3, stop on quit) | `2642629` (15 services, observe-only); act paths behind `SUPERVISOR_ACT_ENABLED=false`; tests in `src-tauri/src/commands/supervisor/` and `src-console/standalone/*.test.ts` | pass (observe); waiting-on-Taisun (act on, LaunchAgent retirement) |
| 26 | Loopback only, per-launch token, Host/Origin checks | `988a464`, `a1d1cb5`, `a33e623` (router enforcing); crew mount reverted `2104461`. Letta and n8n do not check Host/Origin (known residual) | partial |
| 27 | Keychain for secrets; no secret in file, log, backup, IPC | `889bd19`, `a1d1cb5`; `npm run check:secrets` ok; sentinel scan of DMG (READINESS) ; Keychain moves need his approval in the UI (decision 0028) | pass (scans); waiting-on-Taisun (moves) |
| 28 | Cloud AI off by default; outbound as a setting | `988a464` (Info.plist env removed, device-bound setting), `outbound.rs` | pass (tests); hostile-prompt live check waiting-on-Taisun |
| 29 | Unified logs rotated, redacted, 0600 | `988a464` | pass (tests) |
| 30 | Data dir 0700 / files 0600 | `data_harden.rs` built; one-time chmod needs his OK | waiting-on-Taisun |
| 31 | Model tiers by RAM, models downloaded into Application Support, no Ollama.app | `11c7883`, `bcd1385`, `9a4e84b`; model names `0030` | pass (built, gated download); live first run waiting-on-Taisun |
| 32 | First run (welcome, restore or fresh, tier, services, keys) | `11c7883`; `test/e2e/standalone-1111s.spec.ts`. A clean-account run is Part B of migration-test.md | waiting-on-Taisun |
| 33 | Open at login | `71d8d7e` (`login_item.rs`, SMAppService, default off); Startup toggle not wired (Design); not registered live | waiting-on-Taisun |
| 34 | Signed, notarized, stapled DMG | `71d8d7e` `scripts/release-local.sh`, Entitlements.plist; unsigned ad-hoc DMG exists. No Developer ID | waiting-on-Taisun (blocked: Apple cert) |
| 35 | Auto-update with signed manifest, rollback | `71d8d7e` (`updater.rs`, `update_install.rs`, fake-host tests: bad signature refused, rollback, backup before swap); `UPDATER_SWITCH=false`, placeholder pubkey, no host | waiting-on-Taisun (inert) |
| 36 | OpenClaw removed from code, UI, packaging | `889bd19` | pass |
| 37 | Open WebUI as a side tool outside the DMG | Supervisor lists it (`registry.rs` label); not bundled (DMG scan: 20 files, no Open WebUI) | pass |
| 38 | NOOP attribution in About and NOTICE | No `NOTICE` file in the repo and no PolyForm text in code or UI (`rg -i polyform` hits only tasks docs). Phase 12 item, not built | fail (not built) |
| 39 | Lucid cues from the Mac, Oura/Hevy/coach secrets, retire NOOP.app | Phase 12 not built | waiting-on-Taisun |
| 40 | Navy/voice speak and dictate | App side built (Stage C, inert); needs `tasks/proposed/amara-voice-speak.diff` and `amara-voice-dictate.diff` applied by Taisun | waiting-on-Taisun |
| 41 | Typed bridge, one registered command list | `c640d1c`..`7e01b5a`; `test/commands.registered.test.ts`, `test/unit/bridge.bound.test.ts` | pass |
| 42 | Accessibility: axe serious/critical = 0, both themes | `test/e2e/a11y.spec.ts` in the full e2e run (see READINESS e2e line) | see READINESS |
