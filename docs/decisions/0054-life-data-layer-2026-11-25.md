# 0054: Life data layer (Taisun, 2026-11-25, in session)

His words: **"Upgrade the existing Trajectory/GoalTracker implementation into the backend foundation for the Life system. Do not build a second goal tracker."**

**Note:** the full approved Life specification is not in the repository (see `AMARA_PROJECT_HANDOFF.md` §4). This phase follows the prompt itself.

## One source of truth per record (reused, not duplicated)

| Record | Source of truth | Change in this phase |
|---|---|---|
| Goals | `goals.json` | Life fields added as an optional nested `life` object |
| Habits | `goals.json` (`category: "habit"`) | Life habit fields in the same object |
| Habit completions | `habits/days.json` | Read as done / not_done / **no_record** |
| Projects, project milestones, project next action | `terrence_projects` (`terrence_core.db`) | None; linkable from goals |
| Goal milestones | Roadmap steps (`goals/roadmaps.json`) | None |
| Tasks | `TASKS.md` (History/Undo) | None. A structured task store would be a second task system; that needs your decision |
| Dates and deadlines | `life/dates.json` (its own `goal_id` link) | None |
| Life areas | **new** `life/areas.json` | Areas didn't exist (the goal category is free text) |
| Goal-to-project, goal-to-habit links | **new** `life/links.json` | Each relationship stored once; both ends must exist |
| Activity | `amara.db` `activity` (0053) | New kinds: `goal.status_changed`, `link.added`, `link.removed`, `area.updated` |

## Goal and habit fields (`goals::LifeFields`)

- **Goals:** type (outcome, performance or learning), motivation, baseline, desired result, measurement, completion criteria, target date (the existing `deadline`), status, status reason, area, next action.
- **Habits:** frequency (times per day or week), preferred time, cue, minimum version, full version, reminders on/off (the time is the existing `remind_at`). Linked goals come from `life/links.json`; completion history from `habits/days.json`.
- **Status:** `active`, `paused`, `blocked` (needs a reason), `completed`, `archived`. When unset, it is read from the old `paused` flag, and the flag is kept in step. Every change is recorded as `goal.status_changed` with from and to.
- **Progress history** comes from the activity history (progress, completion and status changes).
- **A progress fraction** is computed only when a baseline is set and differs from the target: (current - baseline) / (target - baseline), unclamped. Otherwise there is none.

## Migration safety

- **No rewrite of `goals.json`.** The format stays a list of goals; `life` is optional and omitted when empty, so existing files load unchanged and older builds ignore the key.
- **Ids and values are never changed** by a Life update (tested).
- **A console save that omits `life` keeps it from disk**, exactly like `venture_id`. Only `life::update_in` changes it.
- **Rollback caveat:** if an older build saves `goals.json`, it drops the `life` objects (the goals themselves are kept). Install forward only, or restore `goals.json` from a backup.

## Tests (`life/tests.rs`, repository level, real files)

- Old goals load unchanged and keep their ids and values.
- Invalid values are refused and nothing is written.
- The five statuses are distinct and each change is recorded.
- Habit fields work, and missing days are `no_record`.
- Progress is never invented.
- Links are stored once, both ends must exist, and removal works.
- Areas are created, renamed and archived without touching goals.

All gates pass. **Not run:** e2e (no UI change) and the packaged app.

## Not done

- **Tauri commands for Life:** the Life interface phase adds them. Until then the repository is used only by its tests (`#[allow(dead_code)]` on the module).
- **Structured tasks:** your decision, above.
- **Cross-domain recommendations:** excluded by the prompt.
