# Board (read this first when resuming)

Branch `amara-integration`, local commits only, nothing pushed. Decisions: `docs/decisions/0001–0007`.
Full gates on HEAD (2026-11-13, after `8af876a`): src-tauri 769 / 5 ignored · router 126 · vitest 1710 / 9 skipped · neuro 21 · full e2e 754 passed / 12 skipped / 0 failed. Older: (2026-11-12, after `7de5b4b`): vitest 1523 / 9 skipped · neuro 21 · src-tauri 542 / 5 ignored · router 126 · crew-service 222 · tsc / lint / fmt / clippy / secrets clean · **full e2e 728 passed / 12 skipped / 0 failed**. Review gate reached: see `tasks/HANDOFF-FOR-REVIEW.md`.
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
| Install | see Installed |
