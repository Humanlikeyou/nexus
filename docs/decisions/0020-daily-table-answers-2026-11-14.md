# 0020: Daily table answers (Taisun, 2026-11-14, in session)

Questions: `tasks/HANDOFF-FOR-REVIEW.md` §16 (source map `tasks/proposed/daily-table-sources.md`). Package 3.

His answers, verbatim where short:
1. and 2. **Takes shown / rated helpful: "get it recorded, use your best judgement".** Neither is recorded today, so package 3 starts by recording them locally. Assistant's reading:
   - "shown": the take was on screen, once per take per day.
   - "helpful": no helpful/worse rating exists anywhere today (corrected after reading the code; the "verdict" in the takes code is an outcome check, not a rating). So the recording path is built now: a `take_rating` event and a bridge call. The button that lets Taisun rate a take is a Design request (`tasks/design-questions.md`).
   - Until a rating exists, "rated helpful" is NULL, never guessed. No new UI is added by Claude Code.
3. **Follow-ups per goal: "best judgement based on project needs".** Assistant's call, corrected 2026-11-14 after reading the code:
   - The app has no follow-ups store of its own. `followups.rs` already reads Amara's list at `~/.hermes/amara/open_items.json` for the Follow-ups screen.
   - The daily table reads that list through the same existing reader, read-only. It never writes it, and tests use temp dirs only. This is the same "read-only at runtime" treatment the table gives Muse sessions.
   - Follow-up items carry no goal id, and goals are never guessed. So the table records **follow-ups closed per day** as a total, and the per-goal follow-up cell stays NULL. Its source note says "follow-ups carry no goal id".
   - A goal link would need a new field in Hermes's file, which hard rule 5 doesn't allow; that's for Taisun to decide later.
4. **Calm: "best one".** Assistant's call: `calm_pct`, the definition patterns and the weekly review already use, so "calm" means one thing across the app. Hemi-Sync's `avg_relaxation` stays a Hemi-Sync detail.
5. **Several check-ins in a day: "mean"** for mood and energy.
6. **Hemi-Sync and Muse minutes: "both".** Two columns, never summed.
7. **Re-derive NOOP workout days in JST: "no".** Keep NOOP's own day as stored.

Still frozen (0001): merge into `nexus-v2`, push, PR.
