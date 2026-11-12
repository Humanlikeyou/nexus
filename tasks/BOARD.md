# Board (read this first when resuming)

Branch `amara-integration`, local commits only, nothing pushed. Decisions: `docs/decisions/0001–0007`.
Full gates on HEAD (2026-11-12, after `7de5b4b`): vitest 1523 / 9 skipped · neuro 21 · src-tauri 542 / 5 ignored · router 126 · crew-service 222 · tsc / lint / fmt / clippy / secrets clean · **full e2e 728 passed / 12 skipped / 0 failed**. Review gate reached: see `tasks/HANDOFF-FOR-REVIEW.md`.
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
- done 779b590 / a972a7e: Neuro redesign + Hemi-Sync (decision 0010)
- done 47b89cb / 79d76da: system map in Settings › Services (decision 0011)
- Taisun: rebuild + install, then: Import the Hemi-Sync collection, try one track with the Muse, check the map live

## Installed
- 73f53a5 installed 2026-11-12 22:21 JST (rollback: ~/Backups/Amara OS-backup-20261112c.app)

## Next session
- Intelligence gap A/B (Hermes vs Telegram vs command center); route Telegram through Hermes; slim the chat context
- Personal money section from docs/research/personal-money-best-practices-2026-11-12.md
- Wire StepQuestion into the takes; Life goal facts for goal-tied encouragement
- Muse LOW follow-ups (see handoff §12); life-enterprise-1111v:166 light contrast; re-run the commit security review (429)
- Decision 0013: habit tracker research + design, goal roadmaps (deep, overnight), step questions in takes
- money_balances_list read command; weekly review + patterns jobs; habit data sources

## Out of scope tonight (Taisun)
NOOP merge, live Telegram relay send path, standalone phases (bundling, signing, notarization, DMG, updater, WHOOP takeover), Letta upgrade apply.
