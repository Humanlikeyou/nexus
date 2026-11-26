# 0062: Workout engine, Phase 9 (Taisun, 2026-11-26, in session)

Brief: `docs/specs/workout-engine-2026-11-26.md`. Claude proposed steps 9a–9d and four choices. Taisun answered:
- **Library:** "No for fitness it should be calisthenics focused". The starter library is calisthenics: bodyweight exercises in progression families (push, pull, dip, legs, core and skills), using a pull-up bar, rings, parallettes or dip bars where needed. It is not general gym fitness.
- **Load:** "go". Sets may record load in kg (for example a weighted pull-up or dip), and training volume includes it.
- **Design:** "go". The design boundary is lifted for the new Workout view only, as for Neuro › Training in 0059. The Fitness page is unchanged apart from one button that opens the view.
- **Takes:** "auto". When a finished workout came from an Amara take, that take is marked done automatically.

## Steps and acceptance

| Step | Acceptance |
|---|---|
| 9a Engine (Rust) | **Library:** a built-in calisthenics library plus your own exercises. Every brief field is there: id, name, description, instructions, muscles, equipment, difficulty, recommended reps or hold, form and safety guidance, and a demonstration-asset reference. **Templates:** ordered items with sets, reps or a hold, optional load and rest. **Sessions:** planned → active ⇄ paused → completed, partially completed or abandoned; every transition is validated, and nothing changes after the end. **Each session records:** start and end, sets with reps, holds and load, rest between sets, skips and paused time. **Fixed calculations:** duration estimate, completion state and training volume, with tests. Stored in `<data>/workout/`, written atomically, locked, validated and capped |
| 9b One engine | Amara's fitness takes become templates in the same engine, with provenance (`source: amara`, and the take id) |
| 9c Screen | Mind-Body › Fitness › Workout: library browsing and details, a builder with reordering and a live estimate, start, pause, resume, skip and end controls, a summary and history |
| 9d Connections | `workout.finished` in the shared activity history. Workouts in Life's weekly review. A short, sourced workouts section for Amara. Linking to an overlapping NOOP-detected workout (link only; NOOP is never written). The take is marked done automatically |

For every step:
- the gates and e2e pass;
- a security review covers the data paths;
- no 3D;
- no new outbound hosts;
- the Fitness page and the NOOP integration are unchanged apart from the entry button.

## Results

Filled in as each step lands.
