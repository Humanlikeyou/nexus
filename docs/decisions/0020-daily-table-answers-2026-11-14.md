# 0020: Daily table answers (Taisun, 2026-11-14, in session)

Questions: `tasks/HANDOFF-FOR-REVIEW.md` §16 (source map `tasks/proposed/daily-table-sources.md`). Package 3.

His answers, verbatim where short:
1. and 2. **Takes shown / rated helpful: "get it recorded, use your best judgement".** Neither is recorded today, so package 3 starts by recording them locally. Assistant's reading:
   - "shown": the take was on screen, once per take per day.
   - "helpful": the take's existing helpful / worse verdict, stored in Rust instead of only in the renderer.
   - Recording only, with no new UI. Any new control is a Design question.
3. **Follow-ups per goal: "best judgement based on project needs".** Assistant's call:
   - Use Amara's own follow-ups store in the app data dir, and do not read `~/.hermes` (this prompt's constraint).
   - Count per goal where an item names a goal, and keep an "unlinked" count otherwise.
   - Never guess a goal.
4. **Calm: "best one".** Assistant's call: `calm_pct`, the definition patterns and the weekly review already use, so "calm" means one thing across the app. Hemi-Sync's `avg_relaxation` stays a Hemi-Sync detail.
5. **Several check-ins in a day: "mean"** for mood and energy.
6. **Hemi-Sync and Muse minutes: "both".** Two columns, never summed.
7. **Re-derive NOOP workout days in JST: "no".** Keep NOOP's own day as stored.

Still frozen (0001): merge into `nexus-v2`, push, PR.
