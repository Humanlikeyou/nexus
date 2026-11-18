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

## Backend and wiring: Grok bot, not Claude Code
Taisun, 2026-11-18: "grok bot will be doing all of this. I m just letting you know in case you notice a change".
Claude Code does NOT build the 1118a or 1118b backend below. If those files or commands change in the tree, it's the Grok bot's work: don't revert or rewrite it, rebase onto it, and run the gates as usual.

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
