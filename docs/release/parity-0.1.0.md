# Parity matrix 0.1.0 (Phase 17)

Base commit: `07453c2` (refreshed 2026-11-19; first written at `71d8d7e`, then updated for every commit since `4ce7a8d`). Evidence is a commit hash from
`git log`, a test file that exists in the repo, or a TAISUN check.

**Source of the rows.** The handover (`tasks/HANDOVER-FINAL-2026-11-12.md`) points at
`standalone-app-plan.md` section 4 ("capability inventory"). That file is **not in the repo and not found
anywhere under `~/projects`** (searched by name and by content). The rows below are therefore built from
the handover's own definition of done (section c) and its service and feature lists, not from section 4.
If Taisun supplies `standalone-app-plan.md`, diff its section 4 against this table; any row it has that
this one lacks is **not verified**.

Status: `pass` = automated test or recorded commit evidence in this repo; `fail`; `waiting-on-Taisun` =
needs live hardware, a login, a key, a decision or his sign-off. "Gates" = `.claude/skills/run-gates/gates.sh`
(see READINESS-0.1.0.md for this run's lines).

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
| 10 | Health store: NOOP read-only, daily table, parity report | `5994114`; `test/e2e/fitness-0017.spec.ts`; `bddb46f` (G1 `noop_parity_report`, G2 relay number check). G3 pinned read path + freeze report `46674a5` (`noop_query.rs`, `noop_freeze.rs` tests; fail-closed pin). Real NOOP store never read by tests. Q4 (what parity means after takeover) open | pass (tests); waiting-on-Taisun (live parity run, pin digest logged, Q4) |
| 11 | WHOOP 4 ownership (single writer) | Read-only freeze report and a gated copy (`noop_freeze_copy`, `FREEZE_COPY_ENABLED = false`) built in `46674a5`. Takeover (G4) and cutover (G5) not built; need hardware and his go | waiting-on-Taisun (G4/G5) |
| 12 | Health sync sidecars bundled and signed | `prepare-sidecars.sh`, `files{}` in `tauri.conf.json` (checked by `test/scripts/stageH.test.ts`); signed path not run (no Developer ID) | waiting-on-Taisun |
| 13 | Takes: memory, daily suggestion, answers, outcomes | `8f6eba5`, `65663f3`, `bda3c15`; `test/e2e/takes-1111t.spec.ts`, `takes-done-fold.spec.ts` | pass (see e2e line for the light-theme contrast issue) |
| 14 | Home heart-rate take | `8f6eba5`, `c3a15a6`; `test/e2e/hr-take-1111u.spec.ts` | pass |
| 15 | Life, Enterprise, Needs you today, decisions log, adds | `97eddbb`, `16b78d8`; `test/e2e/life-enterprise-1111v.spec.ts` | pass |
| 16 | Goals, roadmaps, check-ins, patterns, weekly review | `36d4378`, `bce7ebf`, `8af876a`, `f45829a`, `d6c44c0`; `test/e2e/goals-1112g.spec.ts`. Habits data source and 22:00 reminder not verified | pass (built parts); habits waiting-on-Taisun |
| 17 | Money (read-only), JP bank CSV, FX | `7b448bb`, `a783a5f`, `9b95d68`; `test/e2e/takes-1111t.spec.ts`. JPY to USD rule, ledger data, aggregators gated | pass (read-only); waiting-on-Taisun (gates) |
| 18 | Treasury | Phase 2 `3092346`; Phases 3-4 `22523f0` (customer counts, Gmail sequence drafts, results import; no Gmail API); Phase 5 `2ae105a` (exception monitor, monthly review). Rust tests in `treasury_counts.rs`, `treasury_sequences.rs`, `treasury_monitor.rs`, `treasury_review.rs`. Real inputs (provider exports, customer data, 77-vs-7 stale rule) not supplied; never run live | pass (built, tests); waiting-on-Taisun (inputs, "77 vs 7") |
| 19 | Jarvis windows (3 cap, LRU, Undo, per-site profiles) | `c024eeb`, `710863c`, `2c4a8a8`; `test/e2e/home-windows-1111w.spec.ts`. Live sign-in is a TAISUN check | pass (automated); waiting-on-Taisun (live) |
| 20 | Telegram relay of NOOP notifications | `a783a5f`, `bddb46f` (G2: events from `daily_range`, numerals checked, timer `notify_relay::start`). Timer inert; first real send needs the bot, Keychain token, chat and Send Test. Token moves to the Keychain through `keychain_move_one` (`46674a5`); old plaintext copies and backups still need a rotation | pass (tests); waiting-on-Taisun (relay bot, live send, Telegram token rotation) |
| 21 | Nightly backups, 14 kept, 0600 in 0700 | `6507382`, `1c93d5e`, `a783a5f`, `de30ebe`; `docs/release/restore-drill.md`; `scripts/migration-probe.sh` | pass |
| 22 | Restore on a copy (counts and hashes match) | `docs/release/restore-drill.md` (3,294 of 3,294 hashes, 2026-11-13); synthetic probe this run (`migration_probe_tests`) | pass |
| 23 | Migration to a fresh account | `scripts/migration-probe.sh` (automated part pass); `docs/release/migration-test.md` Part B not run | waiting-on-Taisun (migration test) |
| 24 | No path hard-coded to this Mac | `test/scripts/noAbsolutePaths.test.ts` (new this run). Caveat: some runtime paths are `~/projects/...` fallbacks (`ha.rs`, `kb_status.rs`, `noop_query.rs`, `continuity.rs`, `openclaw.rs`, `backup_schedule.rs` BUNDLE_REPO); they are home-relative, not absolute, and work only where those folders exist | pass (no absolute path); residual note |
| 25 | Services supervisor (probe, restart max 3, stop on quit) | `2642629` (15 services, observe-only); act paths behind `SUPERVISOR_ACT_ENABLED=false`; tests in `src-tauri/src/commands/supervisor/` and `src-console/standalone/*.test.ts` | pass (observe); waiting-on-Taisun (act on, LaunchAgent retirement) |
| 26 | Loopback only, per-launch token, Host/Origin checks | `988a464`, `a1d1cb5`, `a33e623` (router enforcing); crew mount reverted `2104461`. Letta and n8n do not check Host/Origin (known residual) | partial |
| 27 | Keychain for secrets; no secret in file, log, backup, IPC | `889bd19`, `a1d1cb5`; `46674a5` G6 (`keychain_migrate.rs`: Oura, coach, Telegram moves; `get_config` returns a null token; `telegram.rs` reads Keychain first). `npm run check:secrets` ok. Each move needs his own approval click and no screen calls it yet | pass (scans, tests); waiting-on-Taisun (Keychain approvals, Telegram rotation last) |
| 28 | Cloud AI off by default; outbound as a setting | `988a464` (Info.plist env removed, device-bound setting), `outbound.rs` | pass (tests); hostile-prompt live check waiting-on-Taisun |
| 29 | Unified logs rotated, redacted, 0600 | `988a464` | pass (tests) |
| 30 | Data dir 0700 / files 0600 | `data_harden.rs` built; one-time chmod needs his OK | waiting-on-Taisun |
| 31 | Model tiers by RAM, models downloaded into Application Support, no Ollama.app | `11c7883`, `bcd1385`, `9a4e84b`; model names `0030` | pass (built, gated download); live first run waiting-on-Taisun |
| 32 | First run (welcome, restore or fresh, tier, services, keys) | `11c7883`; `test/e2e/standalone-1111s.spec.ts`. A clean-account run is Part B of migration-test.md | waiting-on-Taisun |
| 33 | Open at login | `71d8d7e` (`login_item.rs`, SMAppService, default off); Startup toggle not wired (Design); not registered live | waiting-on-Taisun |
| 34 | Signed, notarized, stapled DMG | `71d8d7e` `scripts/release-local.sh`, Entitlements.plist; unsigned ad-hoc DMG exists. No Developer ID. Stage H questions: `tasks/proposed/stage-h-gates.md` | waiting-on-Taisun (Stage H inputs, Apple cert) |
| 35 | Auto-update with signed manifest, rollback | `71d8d7e` (`updater.rs`, `update_install.rs`, fake-host tests: bad signature refused, rollback, backup before swap); `UPDATER_SWITCH=false`, placeholder pubkey, no host | waiting-on-Taisun (inert) |
| 36 | OpenClaw removed from code, UI, packaging | `889bd19` | pass |
| 37 | Open WebUI as a side tool outside the DMG | Supervisor lists it (`registry.rs` label); not bundled (DMG scan: 20 files, no Open WebUI) | pass |
| 38 | NOOP attribution in NOTICE and bundle | `46674a5`: NOTICE, ATTRIBUTION.md and PolyForm LICENSE in `src-tauri/resources/third-party/noop/`, mapped into `Resources/third-party/noop/`; `test/config/attribution.test.ts` pins their SHA-256. About-screen copy is Design's, not done | pass (files and test); About copy waiting on Design |
| 39 | Lucid cues from the Mac, Oura/Hevy/coach secrets, retire NOOP.app | Secrets: moves built (row 27); Hevy has no stored key to move. Lucid cues and NOOP.app retirement (needs 2-week parity) not built | waiting-on-Taisun (Lucid, retirement) |
| 40 | Navy/voice speak and dictate | App side built (Stage C, inert); needs `tasks/proposed/amara-voice-speak.diff` and `amara-voice-dictate.diff` applied by Taisun | waiting-on-Taisun |
| 41 | Typed bridge, one registered command list | `c640d1c`..`7e01b5a`; `test/commands.registered.test.ts`, `test/unit/bridge.bound.test.ts` | pass |
| 42 | Accessibility: axe serious/critical = 0, both themes | `test/e2e/a11y.spec.ts` in the full e2e run (see READINESS e2e line) | see READINESS |
| 43 | Fitness: Recovery card says why there is no score | `ed4551c` (decision 0035); `src-console/screens/HealthScreen.hronly.test.tsx` (4) | pass |
| 44 | Needs you: warn when a night came through as heart rate only | `dd80bbf` (decision 0034; `strap_night.rs` counts only, `needs_you.rs` tests). NOOP-side strap clock fix is a branch in the NOOP repo (out of bounds): not verified here, building and installing it is his step | pass (Amara side); waiting-on-Taisun (NOOP build) |
| 45 | Path: routes, steps, branch suggestions, History and Undo (1118a B1) | `21ef607` (`goal_route.rs` tests, `design1118/branchesLive.test.ts`). Step edit and Yes now call the logged `goal_step_edit` / `goal_step_complete` (see HANDOFF section 40; `test/unit/roadmapLive.test.ts`); `test/e2e/goals-1112g.spec.ts` | pass; "Not yet" stays on the unlogged legacy command; no Undo control on the page (Design) |
| 46 | Watch-only market proposals (1118a B2) | `2ac4d64` (`watch_proposals.rs`; `design1118/marketProposalsLive.test.ts`). Records only, sends nothing; no live feed | pass; waiting-on-Taisun (feeds) |
| 47 | Smart home device list and one write command (1118a B3) | `16b04d0`, `1d44a2b` (blinds and covers need a native approval, no Undo; `ha_devices.rs` tests against a fake HA). Page still reads the older `ha_states` | pass (backend); Design wiring pending |
| 48 | Finances, venture tracking, Content stages (1118a B4) and goal-venture link, saved drafts (0037) | `fa5198f`, `dd62dab` (`enterprise_views.rs`, `content_pipeline.rs`, `goal_venture.rs`, `content_drafts.rs`; `design1118/enterpriseLive.test.ts`). Yen stays NEEDS_CONVERSION without a dated rate | pass (backend); Design wiring pending |
| 49 | Home lens hooks (1118b) | `c313b11` (`hooks/useAmaraLensState.test.ts` 5, `design1118/homeVitals.test.ts` 5). No screen uses them yet | pass (hooks); Design wiring pending |
| 50 | Sand-lens e2e stability | `test/e2e/sand-lens.spec.ts:28` [dark] failed once under load, passed 40/40 alone (repeat-each=4). Not fixed: the wait is Design's (design-1118-coordination.md); the test is unchanged | open (Design) |
