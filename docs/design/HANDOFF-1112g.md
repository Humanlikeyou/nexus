# HANDOFF 1112g: Life › Goals habit tracker and roadmaps (Design Lead)

Decision 0013. No commit, no live app, no new tokens, contract (`roadmap.ts`) untouched.

## What changed
- `src-console/personal/TodaySections.tsx` (GoalsSection only): Today strip card, habit rows with streak + 12-week heatmap, full-width Milestones card with a roadmap under each milestone. Unwired keeps the goals card and shows one honest roadmap note.
- New in `src-console/goals/`: `GoalRoadmap.tsx`, `HabitTracker.tsx` (TodayStrip, HabitRow, Heatmap, streakOf), `StepQuestion.tsx`, `goals.css`, `goals.test.tsx`.
- `test/e2e/goals-1112g.spec.ts`: sample + unwired, both themes, axe serious/critical = 0.

## Patterns picked (research; summarised from these sources, not re-fetched in this session)
1. James Clear, Atomic Habits: "never miss twice" and "make it easy". Used as a quiet "Missed yesterday" line, only when yesterday is a known miss.
2. Streaks (iOS app): today-first, a small set of habits visible at once. Used for the Today strip.
3. Loop Habit Tracker: calendar-style history and a score instead of a punishing streak. Used for the 12-week heatmap, "No streak yet" rather than 0.
4. Habitify: check-in prompts and progress per habit. Informs one question a day (StepQuestion).
5. GitHub contribution graph: weeks as columns. Used for the heatmap layout.
Rejected: confetti, loud streak-loss warnings, leaderboards (not calm). Marks never rely on colour alone (filled / half / hollow ring).

## Honesty rules applied
- Habits only have 7 known days and all are "none" until a source exists: heatmap is 84 empty cells plus "Starts counting once a source is linked". Streak shows "No streak yet" (never 0). No hand ticks.
- Sample tag only when the adapter state is "sample" (tested).
- Roadmap % is the roadmap's own `pct`. Only after he answers Yes/edits in this session do we recompute with the contract's own weight formula until the store re-reads. In sample mode the adapter is static, so this local overlay is what shows the tick.
- Save on an unwired store says "Not saved"; failures use plain sentences, never raw errors.

## States covered
unwired, sample, live, empty (Draft a roadmap tonight), drafting, suggested (Accept, edit), accepted, all done; smaller-first-move with "Use this" (editStep); Yes / Not yet per open step.

## Notes for others
- StepQuestion is exported but NOT wired into AmaraTake (out of scope today).
- Backend: roadmaps are keyed by milestone id from `adapter.goals()` (ms1...). When the live store keys by goals.json id, map it in `GoalsSection`.
- In the unwired e2e the roadmap note appears twice inside the Life region (another hidden Life view mounts GoalsSection); the spec uses `.first()`. Worth a look.
- Not checked: real-Mac visual check of the heatmap at width, no screenshots taken (no live app per brief).
