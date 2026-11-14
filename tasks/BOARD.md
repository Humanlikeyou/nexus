# Board (read this first when resuming)

Branch `amara-integration`, local commits only, nothing pushed (decision 0001). Decisions: `docs/decisions/0001–0021`.
**Gates on HEAD `bda3c15` (2026-11-14):**
- src-tauri 982 passed / 6 ignored · amara-router 126 · vitest 1899 / 9 skipped · neuro 21.
- fmt / clippy -D warnings / tsc / lint / check:secrets clean.
- **Full e2e 784 passed / 12 skipped / 0 failed.**
- Rust was last run at 0daa808, with no Rust change since.
- takes-1111t:80 [light] is a real Design contrast bug (4.28:1). It fails in some full runs; never weaken it.

Installed: 8bd95d0 (2026-11-14): Stages A–C + voice heartbeat.

### Handover phases (0–17)
Plan of record: `tasks/HANDOVER-FINAL-2026-11-12.md`. Taisun put it on disk on 2026-11-14; it is untracked and his file, so don't commit it without his OK. Evidence is a commit or a decision.

| phase | handover scope | status | evidence / what's left |
|---|---|---|---|
| 0 | Resume, verify, land in-flight crew work, re-baseline | done | crew 7de5b4b 32f1d8c; re-baselined each run (header above) |
| 1 | 1111x navy: localOnly incl. Hermes, local speak | partial | done: 6215a12 0dd3996 cf93fd0; speak() app side built (Stage C, inert). Left: Taisun applies tasks/proposed/amara-voice-speak.diff (+ navy-gate diff); Q1 open |
| 2 | 1111w Jarvis windows backend | partial | done: c024eeb 710863c 2c4a8a8 (cap 3/LRU/Undo, pop-outs, per-site profiles, home_window_open). Left: Amara's comment on a window, blocked-embed/offline signals, verify sign-in pause and Life take facts; live sign-in (Taisun) |
| 3 | 1111v leftovers: chat/voice adds, on-device transcription | partial | done: 16b78d8, Rust adds in amara_turn d6c44c0, dictate_once app side + Composer mic (Stage C, inert). Left: Taisun applies tasks/proposed/amara-voice-dictate.diff |
| 4 | 1111a personal-OS adapters | partial | done: check-ins/dates 97eddbb, goals and roadmaps 36d4378 bce7ebf, memory list b438574, Letta writes 8af876a (0015), patterns and weekly review 8af876a f45829a. Stage A (this commit): patterns and the weekly review on daily_range, generic patterns (BH), goal progress, experiment loop, Hemi-Sync ranking. Left: habits data source; verify the 22:00 reminder; UI for the new readouts (Design) |
| 5 | 1111t step 6 Money (read-only) | partial | done: spike 3be01d3, backend 7b448bb, accounts/fx UI 9b95d68, balances b438574, rows 1c93d5e, bank CSV picker a783a5f. Gated: JPY→USD rule, ledger data, aggregators |
| 6 | Backups: nightly, restore, drill on a copy | done | 6507382 1c93d5e a783a5f de30ebe; `docs/release/restore-drill.md` |
| 7 | Harden in place | partial | wave 1 988a464, wave 2 (this commit): launch token, outbound setting, logs, Keychain-first readers, router-only bridge client, voice/Weaviate plumbing. Gated: chmod, Keychain moves, Weaviate switch-over, token enforcement, Hermes key rotation |
| 8 | Services supervisor | todo | gated: LaunchAgent retirement, crew rebuild |
| 9 | Bundled runtime + first run | todo | gated: light-tier model name, router restart |
| 10 | NOOP merge 0–3 + WHOOP ownership | todo | 0017 plan, 7e9895c inventory. Gated: takeover on hardware |
| 11 | Telegram relay of NOOP notifications | partial | relay settings, preview and approvals a783a5f; daily_table 5994114 for the number check. Gated: bot token, first send |
| 12 | NOOP 4–5: secrets, Lucid cues, attribution, 14-day parity report | todo | Fitness tabs e265648 (0017) are related but not this phase. Gated: retire NOOP.app, old-store delete |
| 13 | Open at login | todo | gated: mechanism |
| 14 | Signing, notarization, DMG | blocked | Apple Developer ID (Taisun) |
| 15 | Auto-update | todo | gated: manifest host, updater key |
| 16 | Migration test | todo | gated: fresh-account run (Taisun) |
| 17 | Final e2e, parity matrix, readiness report | todo | n/a |

Shared pieces (REVIEW §8):
- shared helpers f6fad35
- screen-open counter eac9c0d
- daily table 5994114
- turn pipeline L0–L4 (cb6da06…bb7efff, flag off)

Status: `todo` · `active` (lane running) · `done <hash>` · `blocked` · `waiting` (Taisun).

## Dev system
| id | item | status |
|---|---|---|
| S00 | .claude/ agents, rules, guard hook, skills; decisions; tasks | done 0cc6d7e (+ 5f3a579 19b3bc8 b4b251d) |
| S01 | Local pre-push hook (NEXUS_PUSH_OK=1) in ~/projects/nexus/.git/hooks | done (installed, not versioned; decision 0007 #9) |

## Section 11 fix backlog
All 20 items + route-motion flake: done or proposed (see HANDOFF §2). B19 README nav: patch in `tasks/proposed/`.

## Tonight's decisions (0007), work order A–G
| item | status |
|---|---|
| A pre-push hook | done (S01) |
| A save_session refuses duplicate id | done a2a7ac5 |
| A MiniMax removed, concurrency 1, read-only tools, batch approval, retries 2 | done 021ddea |
| A follow-ups: one approval per session with edit list | done a6fd349 |
| A recording delete path: none (removeConfirmed no-op) | done (e182d9f, a34a224 tests) |
| A Letta upgrade plan / n8n + container inventory | done 58374e6 (plan/list only) |
| A router bearer from Keychain | done b9e9efe |
| B liveBusiness crash | done 89317c9 |
| 1111u Home heart-rate take (card C) | done 8f6eba5 |
| 1111x navy localOnly | done 6215a12 (Hermes web tools not covered: open question) |
| C R13-D approvals | done 021ddea |
| C R13-C / R13-E / R13-F + 1110i enforcement | done 7de5b4b (security review re-run: 5 fixes with tests) |
| D 1110f take live wiring | done 8d96417, 534d19f |
| D 1111a personal-OS adapters | blocked: Design sample-tag fix (tasks/design-questions.md); life dates + check-ins live |
| D 1111s secrets status/set, n8n key file | done 889bd19 + 65ffe02 + 149cd68 (backups step 3 done 6507382 + 9ceaadd 0b2a312 d44a4d7) |
| D 1111t step 1 take log | done 8f6eba5 |
| D 1111t steps 2–5 | done 65663f3 |
| D 1111t step 6 money (spike first; no sign-ups) | backend done 7b448bb (spike 3be01d3, decision 0008); UI wiring waits on Design; real bank samples last |
| 1111v backend (decisions log, dates, adds, check-in, needs-you) | done 97eddbb |
| 1111w windows (webviews, profiles, home_window_open, Life take facts) | blocked: needs Taisun's OK for browsing new hosts |
| E take chips → source | done 84333aa (Neuro/Enterprise inert: no ids) |
| F OpenClaw code cleanup | done 889bd19 |
| G full e2e + gates | done on HEAD 2026-11-12 (728 e2e passed, 0 failed) |

## Section 12 / 13 status
12a done except nutrition (no source) · 12b Business done, Crews steps/handoffs/metrics/approvals done, n8n/roster/nextSchedule pending R13-F · 12c done · 12d done · 13 A B C D E F done (7de5b4b). n8n tile live when n8n is up + optional key file.

## Follow-ups
- `scripts/sync-crew-env.sh` must carry `CREW_TRIGGER_SECRET` before the trigger is enabled (today it is dropped: fails closed).

## Next
- Review 2026-11-13: `tasks/REVIEW-2026-11-13.md` (fix list, data use, Amara interface; §7 recommendations waiting on Taisun, §8 build order for shared pieces). Not committed.
- done 779b590 / a972a7e: Neuro redesign + Hemi-Sync (decision 0010)
- done 47b89cb / 79d76da: system map in Settings › Services (decision 0011)
- Taisun: rebuild + install, then: Import the Hemi-Sync collection, try one track with the Muse, check the map live

## Installed
- 8bd95d0 installed 2026-11-14 (voice heartbeat + 0025), clean checkout build; app_alive heartbeat confirmed after launch (rollback: ~/Backups/Amara OS-backup-20261114e.app)
- cdc98cb installed 2026-11-14 (Taisun: "install when stage C is done"), clean checkout build (rollback: ~/Backups/Amara OS-backup-20261114d.app)
- 7e01b5a installed 2026-11-14 (Taisun: "install when stage B is done"): Stage B typed bridge + decision 0022, clean checkout build (rollback: ~/Backups/Amara OS-backup-20261114c.app)
- d6c44c0 installed 2026-11-14 (Taisun: "install then next"): Done-fold bda3c15 + Stage A, clean checkout build (rollback: ~/Backups/Amara OS-backup-20261114b.app). Dev target cargo-cleaned first (disk 98% → 92%; 54.7 GiB freed)
- 0daa808 installed 2026-11-14 01:36 JST (Taisun: "then install"), clean checkout build; menu bar checked after launch: one template "Λ" icon (rollback: ~/Backups/Amara OS-backup-20261114.app)
- 2c4a8a8 installed 2026-11-13 (Taisun: "instal"), clean checkout build (rollback: ~/Backups/Amara OS-backup-20261113c.app)
- 048aaa1 installed 2026-11-13 (Taisun: "start the reinstall"), clean checkout build; L2 amara_turn not included (under review) (rollback: ~/Backups/Amara OS-backup-20261113b.app)
- 0502d78 installed 2026-11-13 (Taisun: "reinstall now"), built from a clean detached checkout `~/projects/nexus-amara-install` (no uncommitted lane work) (rollback: ~/Backups/Amara OS-backup-20261113.app)
- 540f051 installed 2026-11-12 23:08 JST (rollback: ~/Backups/Amara OS-backup-20261112d.app)
- 73f53a5 installed 2026-11-12 22:21 JST (rollback: ~/Backups/Amara OS-backup-20261112c.app)

## Next session
- Decision 0014 approved: Letta writes (after the CVE upgrade), WHOOP takeover, Telegram sends, updater (needs Developer ID + updater key), Home windows: one lane each, security review each
- Wiring classes A/B from tasks/wiring-inventory-2026-11-12.md
- Intelligence gap A/B (Hermes vs Telegram vs command center); route Telegram through Hermes; slim the chat context
- Personal money section from docs/research/personal-money-best-practices-2026-11-12.md
- Wire StepQuestion into the takes; Life goal facts for goal-tied encouragement
- Muse LOW follow-ups (see handoff §12); life-enterprise-1111v:166 light contrast; re-run the commit security review (429)
- Decision 0013: habit tracker research + design, goal roadmaps (deep, overnight), step questions in takes
- money_balances_list read command; weekly review + patterns jobs; habit data sources

## Night of 2026-11-12 → 13 (Taisun asleep; autonomy granted "keep working")
| item | status |
|---|---|
| Heart-rate take back on Home + Today (Taisun: removal was a mistake) | done c3a15a6 (not installed) |
| memory_list (Settings › Memory read-only) + money_balances_list (money overview) | done b438574 (security review PASS) |
| Decision 0015: Letta writes on 0.16.8 (option A); plan recheck, DB is letta_db | done 7e075d6 |
| Letta password in Keychain | done (Taisun re-saved it 2026-11-13; live read check 200) |
| L1 Letta writes (edit / delete-as-clear / undo, backup first, approval dialog) | done 8af876a (PATCH not run live: Taisun, after pg_dump of letta_db) |
| L2 patterns_read + weekly_review_read | done 8af876a |
| L3 navy localOnly covers chat (fail-closed, Q1 still open) | done 0dd3996 + cf93fd0; voice loop: waiting (Taisun applies tasks/proposed/amara-voice-navy-gate.diff); speak() unwired (no local TTS entry) |
| L4 money account rows + backup_verify_last / Backup "last" | done 1c93d5e (sha256 per file in new backups) |
| takes-1111t:80 light contrast: REAL 4.28:1 on .m-warn b (not a flake) | waiting (Design, tasks/design-questions.md) |

## 2026-11-13 later
| item | status |
|---|---|
| Memory approval dialog: hidden format chars shown as ⟨U+XXXX⟩, refused in edits, cut count | done 420a179 + ba9d45e (automated review findings) |
| Navy: not confirmed while a Hermes turn is in flight; biased select | done cf93fd0 |
| Weekly review "Try it this week" → followups_add_item (approval) | done f45829a |
| Personal refresh: one bad reply no longer blanks every store | done cc17766 |
| Life › Dates Edit | waiting (Design: needs an edit form; tasks/design-questions.md) |
| N1 chat/voice adds | done 16b78d8 (security review fixed) |
| N2 nightly backups + restore drill, N3 jp_bank import picker, N4 Telegram relay (approvals, no send yet) | done a783a5f (3 security reviews fixed; drill: Muse 34/34 byte-identical) |
| Turn pipeline (0016/0019): L0 cb6da06, L1 80d9c04, L3 048aaa1, L2 5e8ead7, L4 bb7efff | done (flag nexus.turnPipeline OFF by default; 4 security reviews fixed). Rust adds port still a stub (renderer adds run first). Design asks: streaming text, tier line, Used: chips, Stop state |
| Roadmap catch-up when idle (0018) | done 29d112f |
| Backups redaction follow-ups | done de30ebe |
| NOOP/Lucid inventory (0017) + Fitness Workouts/Sleep detail/Trends | done 7e9895c, e265648 |
| Home never scrolls | done 582b073 |
| Home windows backend (Phase 2, 0014 #5) | done c024eeb (security review 3 HIGH fixed; residual: no WK content rule list; Letta/n8n don't check Host/Origin). Taisun live check: open Instagram, sign in, close, reopen, sign out |
| Installed | 048aaa1 (rollback ~/Backups/Amara OS-backup-20261113b.app); later commits not installed |
| e2e on HEAD cc17766 | 753 passed / 12 skipped / 1 failed (takes-1111t:80 light, the real 4.28:1 contrast, Design) |

## Out of scope tonight (Taisun)
NOOP merge, live Telegram relay send path, standalone phases (bundling, signing, notarization, DMG, updater, WHOOP takeover), Letta upgrade apply.

## 2026-11-14: shared pieces (prompt "READ FIRST… GOAL Build the shared pieces", from REVIEW-2026-11-13 §8)
| package | status |
|---|---|
| 1 Backups / N2 (nightly 03:00, prune plan, restore preview, drill) | done earlier: a783a5f + de30ebe; `docs/release/restore-drill.md` shows 3,294 / 3,294 hashes matching. No new work needed |
| 2 Shared Rust helpers (`commands/shared/`: http, sqlite, task, fsx, time) | done f6fad35 (HANDOFF §16) |
| 3 Daily table | done (this commit; HANDOFF §18). Take rating control: waiting on Design |
| 4 Turn pipeline design note | superseded: `tasks/plans/turn-pipeline.md` (decision 0019) was already built as L0–L4 (cb6da06, 80d9c04, 048aaa1, 5e8ead7, bb7efff) |
| 5 Screen-open counter | done eac9c0d (HANDOFF §17) |

## 2026-11-14: follow-ups (decision 0021)
| item | status |
|---|---|
| Timeouts on Telegram (15 s) and ARM worker (10 s) sends that had none | done (this commit) |
| Menu-bar icon: one template tray (Amara mark), duplicate config tray removed | done (this commit) |
| Install | done 0daa808 (see Installed) |

## 2026-11-14: goal prompt "take the app to the handover's definition of done", Stage A
| item | status |
|---|---|
| A1 Rust adds port inside amara_turn + check-in / follow-up / pattern context sections | done (this commit). [GATE] nexus.turnPipeline default stays OFF until Taisun tries it |
| A2 patterns + weekly review read daily_range (results proven identical on fixtures) | done (this commit) |
| A3 generic patterns: 43 pairs, lags 0–3, one BH correction, recent-window comparison | done (this commit) |
| A4 goal progress weekly readout (`goal_progress_week`) | done (this commit). UI: Design |
| A5 experiment loop (sidecar, 7-day before/after, in the weekly review) | done (this commit). UI: Design |
| A6 Hemi-Sync ranking (`hemisync_ranking`) + Hemi-Sync day as an A3 cause | done (this commit). UI: Design |
| Stage B typed bridge | next: write tasks/proposed/typed-bridge.md, stop for OK on dependencies |
| Decision 0022: check-in notes in chat context, experiment carry-over (28 days), "in common" at lags 1–3 | done (this commit) |
| Stage B plan `tasks/proposed/typed-bridge.md` | done (this commit). **Waiting on Taisun:** OK for new dependencies (tauri-specta =2.0.0-rc.29, specta =2.0.0-rc.28, specta-typescript =0.0.15, tauri `specta` feature, one online `cargo fetch`) or the no-dependency fallback |
| Stage B commit 1: specta deps (decision 0023) + 6 commands bound + staleness test + drift guard reads bindings.gen.ts | done (this commit) |
| Stage B batch 2+: bind the rest of bridge.ts by module, then the outside callers, then src/ moves | next |
| Stage B batch 2: all bridge.ts commands bound (94 total; amara_turn left for step 3); BigInt cast to number (table in bindings.rs); specta serde_json feature | done (this commit) |
| Stage B steps 3–4: amara_turn Channel bound; every src-console invoke() moved onto bridge (181 bound; invokeBound dispatcher + boundTable.ts) | done (this commit) |
| Stage B steps 5–6: 33 reachable src/ modules moved to src-console/legacy-core (git mv); their calls bound (255 total); invoke guard test; retire list in tasks/proposed/src-retire-list.md | done (this commit). **Stage B done** |
| Stage C: navy speak (C1), wiring audit (C2, tasks/wiring-audit-2026-11-14.md), Phase 4/5 check (C3), money summary/facts (W1), Settings services/logs/whoop status/system profile/restore preview (W2), Sunday card tag (W3), dictation + mic (W4) | done (this commit). Security review fixes in. Voice parts wait on Taisun's two patches |
| Stage D wave 1 (Phase 7): launch token (app/router/crew, enforcement off; /amara + /api only, decision 0026), outbound as a device-bound setting (Info.plist env removed), unified redacted logs, data-dir harden (gated off) | done (this commit). Next: D1/D4 keychain readers + migrate, D8 Weaviate, D9 voice token; router-only client for terrence_runtime_bridge before enforcement |
| Stage D wave 2 (Phase 7): D1 last bare clients (n8n, outbox, telegram Ollama), router-only HttpTransport (token only on router /amara + /api), D4 Keychain-first readers (HA, n8n, crew) + keychain_import_review + keychain_migrate_one (const off + approval, never deletes the file), D8 Weaviate bearer from Keychain + compose diff/parity fixture (switch-over gated), D9 voice level token (const off) + amara-voice patch | done (this commit). Security review: 0 HIGH, 3 MEDIUM + L1/L3 fixed; L2/L4/L5 open (handoff §23). Gates: src-tauri 1163/0/6, router 131, vitest 2290/9, neuro 21, e2e 783/12/1 (known takes-1111t:80 light) |
