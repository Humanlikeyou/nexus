# Board (read this first when resuming)

Branch `amara-integration`, local commits only, nothing pushed. Decisions: `docs/decisions/0001–0007`.
Last full gates (after `021ddea`): vitest 1474 / 9 skipped · neuro 21 · src-tauri 507 / 5 ignored · router 126 · crew-service 109 · fmt / clippy / secrets clean. Full e2e: **not run yet** (step G).
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
| C R13-C / R13-E / R13-F + 1110i enforcement | built, UNCOMMITTED: awaiting security review (review was cut off by the usage limit) |
| D 1110f take live wiring | done 8d96417, 534d19f |
| D 1111a personal-OS adapters | todo |
| D 1111s secrets status/set, n8n key file | done 889bd19 + 65ffe02 + 149cd68 (backups step 3 not built) |
| D 1111t step 1 take log | done 8f6eba5 |
| D 1111t steps 2–5 | done 65663f3 |
| D 1111t step 6 money (spike first; no sign-ups) | todo |
| 1111v backend (decisions log, dates, adds, check-in, needs-you) | done 97eddbb |
| 1111w windows (webviews, profiles, home_window_open, Life take facts) | todo |
| E take chips → source | done 84333aa (Neuro/Enterprise inert: no ids) |
| F OpenClaw code cleanup | done 889bd19 |
| G full e2e + gates | todo (last) |

## Section 12 / 13 status
12a done except nutrition (no source) · 12b Business done, Crews steps/handoffs/metrics/approvals done, n8n/roster/nextSchedule pending R13-F · 12c done · 12d done · 13 A B D done, C E F active.

## Out of scope tonight (Taisun)
NOOP merge, live Telegram relay send path, standalone phases (bundling, signing, notarization, DMG, updater, WHOOP takeover), Letta upgrade apply.
