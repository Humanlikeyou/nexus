# Design 1118a/1118b coordination (Nexus Lead heads-up, 2026-11-18, pasted by Taisun)

Design is in mockup-only mode. Taisun: "don't build yet" until the full design package is ready. When building starts, Design Lead makes local commits on `amara-integration`, rebased onto HEAD, never rewriting ours, and never pushes.

## UI files Design will touch (avoid big refactors; keep changes small and note them here)
- Pages and components:
  - Life/Goals, Milestones (Path)
  - Neuro (Muse, Hemi-Sync)
  - Enterprise (Ventures, Trading, Finances, Content)
  - Agents, Smart home
  - Home (lens, cards, tab bar)
- Theme and motion tokens, plus global styles.
- The Amara lens component, and a new shared WebGL canvas for Home (1118b).

## Backend and wiring: Claude Code, waiting for Taisun's go
Taisun, 2026-11-18 (correcting an earlier line): "no sorry you will do the back in. sorry i will let you know when to start on that back end."
Claude Code builds the 1118a and 1118b backend below, but only after Taisun says start. Grok bot changes to the design files may still appear; don't revert them.

## The 1118a/1118b backend list (for reference)
Specs are in `HANDOFF-1118a.md` §8–10 on Design's box (`/workspace/nexus/design-1118a/`). **That file is not on this Mac yet**, so it must be copied into the repo before this work starts.

From 1118a:
- Amara's step plan per goal.
- A detector for stalled goals and better routes.
- A store for branch proposals; accept and decline go through the single write path, with Undo and History.
- A market and news feed. Trade proposals are stored as choices only: no orders are ever sent, and record-only needs a second confirm.
- Home Assistant device names, types and controls.
- Finances data.

From 1118b (later):
- An AnalyserNode hook on Amara's voice output.
- Amara state events: off, wake, listening, speaking.
- WHOOP heart rate and recovery passed to the UI.
- Daily data for the day rings.

## Open
- Get `HANDOFF-1118a.md` into the repo, under `tasks/` or `docs/`.
- New outbound hosts for market or news data are gated (hard rule 3). They need Taisun's logged decision before any are wired.
- Trading guardrails apply: no order or trade execution path. The command-name guard covers `trade` and `order`, so name proposal commands with "choice" or "proposal".

## UI touches by Claude Code since this note
- be64e9b: Settings › Startup "Open at login" toggle, and the `.m-warn b` light contrast token (`--copper-text-on-tint`).

## UI touches (Integrations, strap pointer, 2026-11-18)
- No markup, class or token change. Logic only, so the "strap" Needs-you source can render: `src-console/inbox/needsYou.ts` (`InboxSource` + `SOURCE_LABEL`/`OPEN_LABEL`/`UNWIRED_REASON` entries, `ORDER`), `src-console/lib/localStores.ts` (`NeedsYouReport.source` union), `src-console/inbox/NeedsYouToday.tsx` (one line in the hook: reads the `strap` report when the backend gave one). Design: please review the labels "Strap" and "Open in Mind–Body ›".

## UI touches (Fitness Recovery empty reason, 2026-11-18, decision 0035)
- Logic and copy only; no new class or token. `src-console/screens/FitnessRings.tsx` (`RecoveryCard` takes `hrOnly` and `onOpenNote`; `Pending` takes an optional `action`; reuses `LinkButton` / `.v-link`, imports `adds/v1111.css`), `src-console/screens/HealthScreen.tsx` (`useStrapHrOnly` reads the existing `needsYouList` and looks for pointer `strap-hr-only`; `HealthField` gets optional `onOpen`), `src-console/App.tsx` (passes `navigate`).
- Only when today's recovery is missing AND that pointer is present. Otherwise the normal pending copy. Never falls back to an older day.
- Copy: "No score today. Last night came through as heart rate only, so recovery couldn't be scored." + link "See the note ›" (opens Home › Needs you).
- Test: `src-console/screens/HealthScreen.hronly.test.tsx` (4, bridge mocked).

## UI touches (Stage G3/G6, 2026-11-18)
- Type only, no markup: `src-console/legacy-core/lib/types.ts` (`NoopQueryError` gains `{ kind: "pin_mismatch" }` and `{ kind: "pin_missing" }`, the new fixed refusal codes); `src-console/bindings.gen.ts` and `boundTable.ts` (generated/bound for the new commands). `MindBodyScreen.tsx` already shows its generic error state for any unknown kind, so no screen change. Design: if you want fixed copy for a pinned-binary refusal, ask in `tasks/design-questions.md`. About-screen copy for NOOP attribution is yours; the files ship in the bundle under `Resources/third-party/noop/`.
- `get_config` now always returns `telegram_bot_token: null` to the webview (no screen reads or writes that field; checked by grep). The Telegram token is set only through Settings > Notifications (`secret_set`) and shown as set/not set (`secrets_status`). Nothing for Design to change.

## UI touches (B1 Path backend, 2026-11-18)
- Logic only, no markup, class or token: new `src-console/design1118/branchesLive.ts` (+ `branchesLive.test.ts`), and `branchAdapter()` in `src-console/design1118/branches.ts` returns it inside Tauri (sample flag still wins). `boundTable.ts` / `bindings.gen.ts` gained the 8 `goal_route_*` / `goal_step_*` / `goal_branch_*` commands.
- For Design/Frontend: call `goal_route_get` for the Path (it returns ghosts and a server `percent`; `roadmap_get` hides ghosts). Complete and edit a step with `goal_step_complete` / `goal_step_edit` and Undo with `add_undo("route", id)`.

## UI touches (B2 watch-only proposals backend, 2026-11-18)
- Logic only, no markup, class or token: new `src-console/design1118/marketProposalsLive.ts` (+ test); `marketAdapter()` in `marketProposals.ts` returns it inside Tauri (sample flag still wins). `boundTable.ts` / `bindings.gen.ts` gained `watch_proposals_list`, `watch_proposal_record`, `watch_proposal_dismiss`, `market_feed_status`.
- Market line and news note stay "Needs wiring" (gated feeds). Toast text comes back from `watch_proposal_record` ("Recorded. Watch-only — no order was sent."). Undo: `add_undo("watch", id)`.
- Flaky (for Design): `test/e2e/sand-lens.spec.ts:28` [dark] "voice off: the sand keeps floating" failed once in the full suite on 2026-11-18 (12.3 s, under load). It passed 40/40 run alone with `--repeat-each=4`. It probably needs a deterministic wait on the lens frame instead of timing. Not changed by Claude Code.
