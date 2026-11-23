# 0022: Stage A answers (Taisun, 2026-11-14, in session)

Questions: `tasks/HANDOFF-FOR-REVIEW.md` §19. His words: "install then next 1.sure 2.you decide 3.no 4.sure 5. you decide".

1. **Check-in notes may reach the chat context: "sure".** The `nexus_checkins` section may include the check-in free-text note. It goes through the same guarded path as everything else: capped per note and per section, injection-scanned, escaped, keyword-gated. Follow-up notes and muted topics stay out, because they weren't asked about.
2. **Experiment results that fall due in a week whose review was never opened: "you decide".**
   - Assistant's call: carry them over. A result appears in the first stored weekly review whose week is on or after its due day, unless an earlier stored review already listed it (de-duplicated by follow-up id).
   - Results more than 28 days past due are dropped, so a long absence doesn't produce a wall of stale results.
3. **Hide goals with no roadmap from "stalled": "no".** They keep showing as stalled, "never".
4. **"In common" looks further back: "sure".** It now uses the 1–3 days before each finished step. One BH correction still covers all candidates (each goal × finding × lag).
5. **Patterns use the table built overnight (up to 36 h old): "you decide".**
   - Assistant's call: keep it. Today is a partial day, and its means (sleep, focus, mood) would be misleading until the day ends.
   - The table rebuilds 10 min after launch and nightly after 05:30, so yesterday is always in.

Also in this message: **install** (done before these changes; see BOARD), then **next** (Stage B, typed bridge: plan first, stop for OK on dependencies).

Still frozen (0001): merge into `nexus-v2`, push, PR.
