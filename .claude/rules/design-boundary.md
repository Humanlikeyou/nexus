---
paths:
  - "src-console/**"
---
# Design boundary

Taisun's Nexus team (Nexus Lead + Nexus Design Lead) owns all design. Claude Code does **no** design work.

- Don't implement, restyle, rename or redesign UI, nav, labels, tokens, cards, charts or layout. Don't touch `src-console/styles/` or design tokens.
- Design-owned areas (not exhaustive): `src-console/styles/`, `src-console/ui/charts/`, Mind–Body / Fitness / Home / Agents / Crews / Business / Recordings / LiveSignal / Sessions screens and their CSS, `lib/sampleAdapters.ts`, `crews/adapters.ts`, `business/adapters.ts`, `neuro/recordings.ts` (interface + sample adapter), `app/routes.ts`, `PageSection` in Quiet.
- Allowed in those files: logic fixes that keep markup, class names and tokens identical; replacing an `unwired` return in a **live** adapter with `{ state: "live", value }` from real data. Note every such touch in the handoff.
- Never invent numbers. Unwired panels show "⚙ Needs wiring". Yen stays `NEEDS_CONVERSION` until a real rate + as-of date exists. Never render "¥".
- Visual/UX questions go to `tasks/design-questions.md`, not into code.
- Design commits on `amara-integration` are never reverted. `design-1110g` is never merged by Claude Code.
