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

| Step | Commit | What landed |
|---|---|---|
| 9a | `9797e55` | Built-in calisthenics library (`calisthenics-v1-0062`, 34 exercises in push, pull, dip, legs, core and skill families) plus custom exercises; templates; the session state machine; the deterministic estimate, set tracking, rest, completion and volume; store under `<data>/workout/` |
| 9b | `486bbe5` | The 15 fully-calisthenics fitness takes run as templates on the same engine (`source: amara`, `sourceRef: take:<id>`). EMOM, AMRAP, Tabata, cardio and unstated counts are not guessed |
| 9c | `d78bed2` | Mind-Body › Fitness › Workout: library, details, builder with reordering and the engine's estimate, player (start, pause, resume, log, skip set or exercise, end), summary, history; one "Workout" button on Fitness |
| 9d | (this commit) | `workout.finished` in the shared activity history (`workout:` refs resolve). A completed Amara workout marks that day's take offer done (never over your own answer). Life's weekly review gets a Workouts card. History links an overlapping NOOP-detected workout (read-only). Amara gets a keyword-gated `nexus_workouts` section with sources |

**Security review** (nexus-security, read-only). No HIGH findings.
- **MEDIUM, fixed:** the screen could save a template as `source: amara` with any reference and plan it with any offer id, then mark an unrelated take done. Now the save command only makes user templates and refuses to overwrite Amara's. An offer id must be `fitness-<date>-<that take>` both when a session is planned and when the take is marked done.
- **LOW–MEDIUM, fixed:** names allowed line breaks into Amara's section. Now no control characters are allowed in any text, and the section flattens whitespace and caps the names.
- **LOW, fixed:** the totals saturate instead of overflowing.
- **LOW, open:** the 8 MB store cap is reached long before 5000 very large sessions. Typical sessions are small; revisit if it ever nears the cap.
- **Passed:** atomic, locked, capped writes with read-back; no path is built from input; no delete path; NOOP is read-only and never opened in tests; no new hosts.

**Verification:**
- **Gates:** tsc, lint and check:secrets pass; vitest 3006 passed / 9 skipped; neuro 21; src-tauri fmt and clippy clean, 2048 passed / 9 ignored (workout 25); amara-router 133.
- **e2e:** 820 passed / 12 skipped / 0 failed on 9d. This ran before the Rust-only review fixes. On 9c, at load average 44, 5 specs timed out; they passed 28/28 at lower load.

**Not run:**
- a real workout in the app;
- a take marked done by a real workout;
- the NOOP link against real detected workouts;
- the packaged app.
