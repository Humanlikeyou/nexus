# 0066: Cross-domain intelligence, Phase 15 (Taisun, 2026-11-26/27, in session)

Brief: `docs/specs/cross-domain-intelligence-2026-11-26.md`.

## Taisun's answers

| Question | Answer |
|---|---|
| Where the UI goes (design override) | **Life + Enterprise:** a Briefings view in the Life host, and a Briefing section in Enterprise › Workspaces. No new top-level nav. |
| What counts as workload | **Personal records only** (Life dates, check-ins tagged work). Business data stays out of personal analysis (0053 unchanged). |
| Runtime | **An in-app timer plus catch-up.** Nothing runs while the app is closed, and the screen says so. |
| After the prompt is complete | "you can build and reinstall, as well as update the GitHub": logged below as the approval for that exact install and that exact push of `amara-integration`. |

## What was reused, not rebuilt

- Recommendations, evidence, outcomes and the status machine (`amara_store`, 0053), and the evidence resolver: model-free rules may cite NOOP daily rows; every other record is looked up and hashed.
- Domain readers: goals, roadmaps, habit days, Life dates, the week plan, check-ins, workout sessions (`workout::summary`), the daily table (WHOOP via NOOP), NEUROSENSE training records and trends, Muse session files (read-only), the money store's own summary line, and the business workspace (objectives, site changes, section counts, connector checks, website and customer reports).
- The weekly Life review (`weekly_review_read` + `life_week`). Phase 15 adds a cross-domain part below it; it does not build a second review.
- `life_week`'s goal-stalled observation: the cross-domain "behind pace" rule stays quiet while that one is open for the same goal.

## What landed

**Store v4** (`amara_store.rs`, additive): recommendation `evaluation_json`, `period_start`, `period_end` and `fingerprint`, plus an index.
- Same migration rule as v2 and v3: a file with rows is copied first (`<file>.v3-<ts>.bak`, 0600), and the change and version bump are one transaction. On failure the file stays v3. Rollback: `restore_backup` with the app closed.
- A `rules:cross_domain` recommendation is refused without all six parts: an observation; evidence (each with a role, quality and window) and the reporting period; an interpretation (`rationale`); a confidence with its basis, plus the limitations (`uncertainty`); a suggested action; and a measurable evaluation (metric, baseline or null, target, direction, unit, measure-after date).
- `create_unless_duplicate`: the duplicate check and the insert share one write transaction (6 parallel producers → 1 row). A finding with the same fingerprint is suppressed while one is open, within the rule's cooldown, and for 30 days after a dismissal.
- A new outcome, `measured` (`met`, `not_met`, `unknown`), records the metric re-read from the records. It never moves the status and never claims cause. Taisun's own "it helped / no difference / worse" stays the separate `evaluated` outcome.

**Briefings** (`commands/briefings.rs` and `briefings/`): `inputs`, `rules`, `summaries`, `evaluate`, `enterprise`, tests.

Personal rules (fixed thresholds, no model):

| Rule | Fires when | Measured by |
|---|---|---|
| Training volume rising | Logged sets this week ≥ 1.5× and ≥ 10 sets over the average of ≥ 2 earlier weeks | Sets in the next 7 days, at most the average × 1.1 |
| Sleep and energy down together / energy down, sleep steady | ≥ 3 nights and ≥ 3 check-in days in each of the two weeks; thresholds on sleep, recovery and energy | Average sleep / tagged check-ins |
| Workload up, training down | Life dates and work-tagged check-ins +2 or more, with fewer sessions than last week | Sessions next week, at least last week's |
| Commitments against the priority | ≥ 3 open Life dates in the next 7 days not linked to the week's priority, with its actions not done | Planned priority actions done by Sunday |
| Repeated obstacles | Same habit missed on the same weekday ≥ 3 of ≥ 3 marked; or a tag in ≥ 3 low check-ins that are at least half of its uses (28 days) | Done on that weekday / low check-ins with the tag |
| Goal behind pace | Progress ≥ 25 points behind elapsed time, 8–180 days left | Progress in 14 days |

Business rules (business file only, business evidence only):
- **Objectives against weekly execution:** approved objectives, no workspace work this week, and work recorded in the three weeks before. With no earlier work it stays unknown, never "idle".
- **Search interest against customer needs:** compares the top search queries with quiz concerns named by ≥ 20% of results. Search Console is deferred (0064), so today it reports **unavailable** and makes no recommendation.

Wording rules, pinned by tests: no causal words. Co-movement says "These moved in the same period. That does not show that one caused the other." WHOOP and Muse figures are described as measurements, "not a readiness, health or psychological assessment" and "not a measure of mood, ability or mental health".

Documents:
- **The daily personal briefing:** today and the week ahead (Life dates and the week's priority, with "No calendar is connected"), sleep and recovery against his own 28-day medians, today's habits, goals, training, neurotechnology, personal money (from the money store's own summary; unavailable when nothing is logged), and open suggestions.
- **The Enterprise briefing per business:** work recorded, approved objectives, what needs a decision (site changes, drafts), connector checks, website and customer figures, search against needs (unavailable), and open suggestions.
- **Summaries:** goal trajectory (ahead, on pace, behind, no deadline, not started, done, unknown), training (logged and WHOOP-detected kept separate), and neurotechnology (sessions, eligible training, medians, and 0059's trends with confidence).
- **The weekly Life review's cross-domain part:** built when it's opened.
- Stored at `<data>/briefings/daily/<day>.json` and `<data>/business/briefings/<business>/<day>.json` (0600, atomic, 60 days kept). Runtime state is in `<data>/briefings/runtime.json`.

**Runtime, exactly:**
- `briefings::start()` (from `lib.rs` setup) ticks every 60 s, starting 20 s after launch.
- The personal briefing is due at 07:00 JST and the Enterprise one at 07:30 JST. The first tick builds whatever is due and missing (trigger `catch_up`). Opening a briefing builds it when due (`opened`). "Rebuild now" is allowed once a minute (`manual`).
- **Nothing runs while Amara OS is closed.** No LaunchAgent or background service exists for this, and the app says so on every briefing.
- No language model is called anywhere in this phase.

**Commands (7):** `briefing_daily_read`, `briefing_daily_refresh`, `briefing_enterprise_read`, `briefing_enterprise_refresh`, `briefing_week`, `briefing_runtime`, `briefing_recommendations`. Registered in `lib.rs`, `bindings.rs` and `boundTable.ts`; bindings regenerated.

**UI** (design override above; existing primitives and class names only):
- Life › **Briefings** (`life/BriefingsView.tsx`).
- The weekly review's "Across your domains" (`life/CrossDomainWeek.tsx`).
- Workspaces › **Briefing** (`business/workspace/BusinessBriefingView.tsx`).
- Shared parts: `briefings/BriefingParts.tsx` and `briefingApi.ts`.
- `RecommendationCard` now shows how a suggestion is measured, its reporting period, the interpretation, the confidence, the limitations, and any measurement. It accepts business-scoped calls.
- The pinned Life tab lists in `routes.test.ts` and `CommandPalette.test.tsx` now include Briefings.

## Verification

- **Rust:**
  - 14 briefing tests: each rule on its own; summaries and missing sources; the full personal flow on real files (6 complete recommendations with resolved evidence, then 6 suppressed on a rebuild); a dismissal respected; a due evaluation measured once and the status unchanged; the week view; Enterprise isolation; the business rules alone; the timer (before the slot, catch-up, once a day); opening before and after the slot; and the honesty pin.
  - 6 new store tests: the v3 → v4 migration with a backup, and a failed v4 changing nothing; the six parts and the evaluation shape; suppression while open, after a dismissal and within the cooldown; parallel producers; and measurement.
  - The v2→v3 and v1 migration tests were updated for the v4 step.
- **vitest:** 5 new UI tests.
- **e2e:** `test/e2e/briefings.spec.ts` (3 tests × 2 themes, axe), plus life-ui and workspaces: 38 passed.
- **Full gates and the full e2e suite:** results are in the commit message.
- **Not run:** a briefing on Taisun's real data (needs the installed app), any WHOOP daily-table case beyond fixtures, and the search-against-needs rule on real search data (none exists).

## Open for Taisun

1. Thresholds are first guesses: review them after a few weeks of real briefings.
2. Should briefings ever be sent (Telegram)? Not built; nothing is sent.
3. A real background service, if briefings must run with the app closed, would need a LaunchAgent: a hard-rule exception.
