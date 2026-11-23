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

## UI touches (B4 Finances, venture tracking, Content backend, 2026-11-18)
- Logic only, no markup, class or token: new `src-console/design1118/enterpriseLive.ts` (+ test). No screen edited. `boundTable.ts` / `bindings.gen.ts` gained `finances_view`, `venture_tracking_list`, `content_stage_set`, `content_draft_propose`.
- For Design/Frontend: `enterpriseAdapter()` gives `finances()`, `fx()` (with `asOf` and `stale`), `funds()`, `sources()`, `ventures()`, `draft(title)`, `setStage(...)`, `undoStage(id)`. Content "Draft with Amara" is still the `OffButton` in `EnterprisePages.tsx`; swap it for `draft()` when you build it. Path goals have no venture tag, so Ventures cannot show linked goals yet.

## UI touches (B4 gaps: goal-venture link, saved drafts, 2026-11-19, decision 0037)
- Logic only, no markup, class or token: `src-console/design1118/enterpriseLive.ts` (+ test) gained `saveDraft`, `savedDraft`, `undoDraft`, `linkGoal`, `undoGoalLink`; `ventures()[i].goals` now carries each venture's goals (server percent, N of M steps). `boundTable.ts` / `bindings.gen.ts` gained `content_draft_save`, `content_draft_get`, `goal_venture_set`; the `Goal` type gained optional `venture_id`. No screen edited. Design: wire "Draft with Amara" save and the Ventures goal list / goal venture picker when you build them.

## UI touches (Roadmap writes logged, 2026-11-19)
- Logic only, no markup, class or token: `src-console/goals/roadmapLive.ts` (+ `test/unit/roadmapLive.test.ts`). `editStep` now calls `goal_step_edit` and `answerStep("yes")` calls `goal_step_complete` (both write History, via "typed"). "Not yet" stays on the legacy `roadmap_answer_step` (no new equivalent; it only bumps notYetCount, unlogged). `RoadmapAdapter` interface unchanged; the live adapter also exposes `undoStepWrite(goalId, stepId)` (calls `add_undo("route", id)`).
- Design: there is no Undo affordance on Path/Goals step edit or Yes. If you want one (8 s toast), call `undoStepWrite` on the live adapter. A Yes on an already-done step is now refused by the backend (shows as unwired) instead of being a no-op.

## UI touches (Watchlist feeds, 2026-11-19, decision 0038)
- Logic only, no markup, class or token: `src-console/design1118/marketProposalsLive.ts` (+ test) only. The live adapter now returns `LiveMarket` (still a `MarketProposalAdapter`) with `watchlist()`, `refresh()`, `researchNow()`, `dropPick(ticker)`, `undoWatch(historyId)`. `marketAdapter()` is unchanged (its declared type is still the base interface; narrow with a cast or export the live type when you build the panels). `boundTable.ts` / `bindings.gen.ts` gained `watchlist_get`, `watchlist_research_now`, `watchlist_drop`; `market_feed_status` now returns the real state (`not_connected | blocked | connected | stale | error`).
- `market()` (sparkline points) and `newsNote()` stay "Needs wiring": no price history or news text is stored, so nothing is invented.
- Needs you gets one new source `watchlist` (fixed text "Want new picks?", link `enterprise` / `ventures`). It only appears 14 days after the picks were made (or the last research), and never while outbound is off.

## UI touches (Claude Design wiring, 0039)
Wiring only, per decision 0039; no new token or colour. New small classes: none (reuses `d18-*`). Existing Grok design commits are built on, not rewritten.
- Path: `src-console/design1118/GoalPath.tsx` (step Yes / edit now show the 8 s "Step done. / Step updated." toast with Undo via `undoStepWrite`; branch accept/decline and its Undo were already wired through `branchAdapter()`). Percent ring still shows "N of M" until the server percent exists (unchanged).
- Trading: `design1118/Trading.tsx` (record/dismiss show the backend toast + Undo via `undoWatch`; new `WatchlistCard`: No price yet, open/closed/stale, Research now with rate_limited/blocked/already_running, Drop + Undo), `design1118/marketProposals.ts` (`ProposalChoice` gains optional `toast`, `historyId`), `design1118/marketProposalsLive.ts` (`recordChoice` returns them), `screens/BusinessPanels.tsx` (mounts `WatchlistCard`, `VentureGoalsCard`).
- Smart home: new `design1118/LiveHomeDevices.tsx` (ha_devices_list / ha_device_set, rooms, scenes and covers say "Asks for your approval on this Mac. Can't be undone.", status-only has no control, Undo when `undoable`); `screens/SmartHomeScreen.tsx` mounts it in Tauri when the sample flag is off (Sample controls unchanged).
- Enterprise: new `design1118/EnterpriseWiring.tsx` (`LiveFinancesCard` fx as-of + stale note, funds, sources; `VentureGoalsCard` linked goals N of M + server percent, goal picker, Undo; `ContentItemTools` stage move + Undo, "Did you post it?" to postedByMe, Draft with Amara shown as plain text "Model draft", save + Undo). Mounted from `screens/RevenueScreen.tsx`, `screens/BusinessPanels.tsx`, `screens/EnterprisePages.tsx` (Tauri only; otherwise the old disabled "Needs wiring" button).
- Treasury: new `design1118/TreasuryView.tsx`; `app/routes.ts` (new Enterprise tab "Treasury", between Finances and Tasks; existing labels untouched), `screens/BusinessScreen.tsx`; `app/routes.test.ts` and the ⌘K palette snapshot gained the new entry.
- Needs you: `inbox/needsYou.ts`, `lib/localStores.ts`, `inbox/NeedsYouToday.tsx` accept the `watchlist` source (label "Watchlist", "Open in Ventures ›"). Strap already rendered.
- Home lens: left as is (SandLens reads are not a provable no-visual-change swap; the sand-lens e2e is flaky under load).
- Tests: `design1118/__tests__/wiring.test.tsx` (9, bridges mocked).
