# Workout engine (Phase 9): brief

Saved 2026-11-26 from Taisun's prompt in session. This is Phase 9 in `AMARA_PROJECT_HANDOFF.md` §5. The full Workout specification is not in the repository (§4), so this brief is what Phase 9 follows. Phase 10, the holographic instructor, is separate.

## His prompt (verbatim)

> Create the new Workout capability as a distinct training system.
>
> Do not replace the existing Fitness page or WHOOP integration.
>
> Build the workout engine before adding 3D rendering.
>
> Implement:
>
> Exercise library:
> - Exercise identifier.
> - Name and description.
> - Instructions.
> - Target muscle groups.
> - Equipment requirements.
> - Difficulty.
> - Recommended repetitions or duration.
> - Form and safety guidance.
> - Demonstration asset reference.
>
> Workout templates:
> - Multiple exercises.
> - Exercise order.
> - Sets.
> - Repetitions.
> - Duration.
> - Rest periods.
> - Estimated total duration.
>
> Workout sessions:
> - Planned.
> - Active.
> - Paused.
> - Completed.
> - Partially completed.
> - Abandoned.
>
> Each session must record:
> - Start and end.
> - Exercises performed.
> - Sets completed.
> - Repetitions.
> - Timed holds.
> - Rest periods.
> - Completion state.
>
> Implement:
> - Exercise library browsing.
> - Exercise details.
> - Custom workout builder.
> - Editing exercise order.
> - Estimated duration.
> - Start, pause, resume, skip and end controls.
> - Workout summary.
> - Persistent history.
>
> Amara-recommended workouts and user-built workouts must use the same engine.
>
> Connect completed sessions to the shared activity history.
>
> Make workout data available to Life and Amara with appropriate provenance.
>
> Use deterministic code for duration estimates, set tracking, completion status and training-volume calculations.
>
> Do not implement a 3D model in this phase.

## What already exists (inspected 2026-11-26)

- **Fitness page** (`src-console/fitness/`, Design-owned): NOOP-detected workouts (`noop_recent_workouts`, `WorkoutsPanel.tsx`), trends and sleep.
- **NOOP** `whoop.sqlite`: 245 detected `workout` rows (sport, duration, HR, strain). NOOP also has `liftExercise`, `liftProgram`, `liftProgramItem`, `liftSession` and `liftSet` tables, all empty. Nexus never writes NOOP.
- **Take catalog** (`amaraTake/catalog.json`, 50 entries): fitness takes with progressions (for example the push-up ladder, levels 1 and up), triggers, contraindications and evidence levels. These are Amara's existing workout recommendations.
- **Shared activity history** (`amara_store`, decision 0053): the `workout.finished` kind already exists in the closed vocabulary, and there are recommendations with outcomes.
- **Life**: the weekly review has no workout field yet (handoff §48, item 6). `daily_table` derives `workout_min` from NOOP.

## Scope and acceptance criteria

Proposed after the inspection; they become decision 0062 once Taisun confirms.
