# PLAN-0055: Life UI (design boundary lifted for the Life host, decision 0055)

Start here in a fresh session. Read decisions 0053, 0054 and 0055 first. Commit by explicit path; gates plus `npm run test:e2e` (light and dark) before each commit.

## Read first (existing code to extend, not replace)

- **The Life host:** `src-console/life/LifeScreen.tsx`, `DatesView.tsx`, `WeekReviewExtras.tsx`, `timeline.ts`, and their tests.
- **Routes:** `src-console/app/routes.ts`. Today `life` has these views: `goals`, `weekly-review`, `dates`, `money`. `trajectory` and `terrance:goals` redirect to `life/goals`.
- **Goals UI:** `src-console/goals/`, `src-console/design1118/GoalPath.tsx` (roadmaps), and the habits UI inside Goals.
- **Backend:** `life_*` commands (0055), `recommendation_*`, `activity_list`, `audit_trail` (0053), `read_goals` and `save_goals`, `habit_mark` and `habit_days`, `life_date_*`, the weekly review commands (`weekly_review.rs`), roadmap commands (`goal_roadmap.rs`), project commands (`terrence_projects.rs`), and `read_tasks` (TASKS.md).

## Destinations (keep the `life` id; keep the existing views working)

| # | Label | View id | Built from |
|---|---|---|---|
| 1 | Overview | `overview` (new; becomes the host's first view) | Read-only summary (below) |
| 2 | Goals & Projects | `goals` (existing view, relabelled) | Goals tree: area → goal → linked projects and habits, using `life_links`; roadmap steps as milestones; tasks from `read_tasks` |
| 3 | Habits & Routines | `habits` (new; the habit UI moves here from Goals) | `habit_mark` (record today); a week calendar and trends from `life_habit_history` (**no_record shown as unknown, never as missed**) |
| 4 | Weekly Review | `weekly-review` (existing) | `weekly_review.rs` plus the rules below |
| 5 | Dates & Deadlines | `dates` (existing) | `life_date_*`; a range picker beyond 12 months (the backend `list_in` takes `today`; add from/to) |

- **Personal money** stays an existing view (no new money system). Overview shows money only when the money store has entries.
- `VIEW_REDIRECTS`: keep `trajectory` → `life/goals`; add nothing that breaks old links.

## Overview items and their only sources

| Item | Source |
|---|---|
| Current focus | The goal with `life.status = active` and the nearest deadline, or the weekly priority if set |
| Primary weekly priority | Weekly planning (below) |
| Today's planned actions | Each goal's `life.nextAction` plus today's tasks from TASKS.md |
| Habits due today | Habits with `life.frequency` and not paused, archived or completed; checked against today's `habit_days` |
| Goal progress | `life_progress`; **the fraction only when it is non-null**, otherwise "current / target unit" as text |
| Next milestones | The next undone roadmap step per goal |
| Upcoming deadlines | `life_date_list` plus goal deadlines |
| One Amara insight | The newest `recommendation_list(personal, status=proposed or shown)` whose evidence is all `measured` or `self_report` and not dismissed; otherwise nothing |

## Weekly Review rules

- **Reporting dates:** Monday 00:00 to Sunday 24:00 JST, the same week the existing `weekly_review.rs` computes. Show the dates.
- **Compare with targets:** habit done-days against `frequency.times`. A goal's change comes from `goal.progressed` activity inside the week.
- **Missing versus zero:** a day with no record counts as unknown and is listed separately. A goal with no activity rows is "no data", not "0".
- **Progressed or stalled:** progressed if there's a progress row this week; stalled if active, with no progress for 14 days, and a deadline within 90 days.
- **Other modules,** only where an authorized source exists: NOOP sleep and workouts (existing `daily.db`), and money totals (only with money entries). Neural data appears as summaries only.
- **Observations** are recommendations created with evidence (`recommendation_create`; the resolver enforces real records). At most 3 suggested next actions a week.

## Recommendation controls

| Control | Call |
|---|---|
| Accept | `recommendation_respond(accepted)` |
| Modify | `recommendation_respond(modified, {changed fields})`, after an inline edit of the suggested action |
| Dismiss | `recommendation_respond(dismissed)` |
| Explain | `recommendation_get`: evidence list with role, quality and window, then rationale and uncertainty |
| Record outcome | `recommendation_record_outcome(completed or evaluated)` |

## Proactive support (`proactive.rs` already runs timed questions)

- Respect user settings plus quiet hours; reuse the existing proactive settings, and add quiet hours only if none exist.
- At most N nudges a day (setting; default 2).
- Only recommendations whose evidence quality is not `estimate` or `partial`.
- **Never resurface a dismissed recommendation,** or one with the same kind and target dismissed within 30 days. Expired is never shown again.
- Each shown nudge records `shown` once. The status machine stops repeats: shown can't be shown again.

## Tests

- **vitest** per view, with real adapters mocked at the invoke boundary.
- **Playwright e2e:** every button, form and navigation action, in light and dark: record a habit, edit Life fields, add and remove a link, the date range, weekly planning edit, and each recommendation control.
- **Backend:** add a weekly-planning store only if no existing one fits (check `weekly_review.rs` first).
