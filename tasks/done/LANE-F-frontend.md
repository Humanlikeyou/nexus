# LANE-F: frontend logic + tests (B12, B15, B16, B21)
- **Owner:** frontend-executor. No visual changes.
## Objectives / acceptance criteria
- **B12** `src-console/hooks/useLiveHeartRate.ts:17` `beatSeconds` returns NaN when bpm missing → guard it. Playback timer (:48-54) pauses while the document is hidden and resumes when visible. Tests fail before.
- **B15** WebKit reduced-transparency check (`test/e2e/fixtures/tauri-mock.ts:44`) reads only its own fixture, so `glass-fallbacks.spec.ts:27` and `glass.spec.ts:91` always pass. Make it read the app's real state (the class/attribute/computed style the app actually applies). Show the spec would fail if the app's fallback broke.
- **B16** Unit tests for NeuroFlow.tsx, LandingScreen.tsx, useRouteNavigation.ts, MindBodyCharts, Quiet.
- **B21** route-motion.spec.ts:78 and :100 flakes: deterministic wait on the route transition end (an attribute/event the app sets when the transition finishes), not a longer timeout. If the app has no such signal, adding a data attribute is a logic change: keep markup/classes otherwise identical and note it.
## Verification
tsc, lint, vitest (baseline 1140), and the touched e2e specs (WebKit + chromium as configured), repeated (e.g. `--repeat-each=20`) for B21.
## Review: qa-reviewer.

## Evidence (frontend-executor, 2026-11-11)
- **B12**: already fixed in HEAD (beatSeconds null-guard, playback pause on hidden; tests in src-console/hooks/useLiveHeartRate.test.ts). Proof the tests bite: with guard + visibility handler temporarily removed, 2 of 9 fail (beatSeconds NaN, pause/resume); restored, 9/9. No code change.
- **B15**: test/e2e/fixtures/tauri-mock.ts `reducedTransparency` now also requires `appFallbackApplied` (the app's sidebar computed backdrop-filter none AND background == resolved --glass-solid). glass.spec.ts:91 test used reduceMotion:true, which alone drops blur and hid a broken fallback: now false. Added non-vacuity test (no reduction => false). Break test (solid fill removed from surfaces.css + shell.css, temporary, restored byte-identical): glass.spec:95 + glass-fallbacks:24 (Today, Fitness) 6/6 fail; restored: glass + glass-fallbacks 64/64 pass. Configured projects are light/dark Chromium only; WebKit not configured (not run).
- **B16**: test/unit/{useRouteNavigation,Quiet,MindBodyCharts,LandingScreen,NeuroFlow}.test.tsx, 33 tests. Unit total 1140 -> 1179 passed / 9 skipped.
- **B21**: real cause was app logic, not timing: focusHeading reported success after a no-op focus() (mid view transition) and never retried. useRouteNavigation.ts focusHeading now returns `document.activeElement === heading`, so the rAF loop retries. Spec waits on data-route-motion clearing (the app's existing transition-end signal; no new attribute). Before (`:78` + `:100`, --repeat-each=20, 2 projects, 4 workers): 79/80 (:100 failed once, Settings focus). Diagnostic spec under 8 workers failed repeatedly before, 120/120 after. After: 80/80; full route-motion.spec 20/20. Unit test "keeps trying..." fails without the fix.
- Gates: tsc clean; lint clean; test:unit 1179/9 skipped; e2e touched specs pass.
