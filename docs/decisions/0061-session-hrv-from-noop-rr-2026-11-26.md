# 0061: session HRV from the WHOOP strap's RR intervals in NOOP (Taisun, 2026-11-26, in session)

Taisun asked whether NOOP provides HRV. It does: NOOP's `whoop.sqlite` has an `rrInterval` table of beat-to-beat intervals from the WHOOP strap. It holds about 6.8 million rows, the newest from this morning, and NOOP stores some beats on more than one source channel. Decision 0059's "no connected sensor gives beat-to-beat intervals" was wrong, and is corrected there. Taisun: "yes" to building session HRV from it.

## What was built

- **`noop_rr.rs`.** It opens NOOP's store strictly read-only (`noop::open_store`) and never writes to it. Health values are never logged. Tests use in-memory stores only, and a test build never opens the real store.
  - `noop_session_hrv(start, end)`: RMSSD for a session window, using method `rmssd-v1-0061`:
    - one source channel, the one with the most beats in the window, so no beat is counted twice;
    - rows NOOP marks `tsSuspect` are excluded;
    - intervals must be 300–2000 ms;
    - successive pairs must be at most 3 s apart and change by at most 20% (artifact and ectopic filter).
    - It is reported only with at least 2 minutes, 100 accepted pairs and 60% minute coverage. Otherwise it is "not available" with the reason, for example "The strap hasn't synced this session's beats to NOOP yet."
  - `noop_rr_status()`: whether NOOP holds RR data, the newest beat, and the beats in the 24 hours before it.
- **Neuro › Training › Session records** show each session's HRV, labelled "from the strap, not the EEG".
- **The biofeedback card** shows RR intervals and HRV as available when NOOP has them. It says plainly that they arrive when the strap syncs, not live, and that HRV is never worked out from EEG or a heart-rate estimate.
- **Amara's local-only `nexus_neuro` section** gets the latest session's HRV as biofeedback, separate from the EEG.
- **The HRV guard test** now forbids any HRV calculation in the console code. Showing the Rust result is allowed; fail-before cases are included.

## Limits

- **Not live.** RR intervals arrive when the strap syncs, so there is no in-session HRV feedback. HRV is a per-session figure, available after the sync.
- **NOOP's channel and suspect flags** are used as NOOP sets them. They have not been independently validated.
- **Not run:**
  - a real session's HRV in the app (no real session window was computed in this work);
  - the packaged app.

## Security review (nexus-security, read-only)

- **Passed:** the store is opened read-only, queries use bound parameters, row and window caps are in place, and no health values are logged. Tests never reach the real store (`cfg!(test)` returns before the path is resolved). The HRV line stays inside `nexus_neuro`, which is dropped twice whenever the cloud is allowed. No new hosts.
- **MEDIUM, fixed:** the narrowed guard regex missed real calculations, such as a typed `rmssd` declaration, an arrow function, `function rmssd`, and `hrv = rr.reduce(...)`. It now blocks any assignment to, function of, or computed property for an HRV-statistic name. All the missed cases are tested, and showing a value (`rmssdMs: number | null`, the word "(RMSSD)") is still allowed.
- **MEDIUM-LOW, fixed:** a corrupt session `duration_secs` could make `end_of` panic, and that path runs during Amara's context build. It now uses checked arithmetic, and an unrepresentable end reads as empty. Tested with `u64::MAX`.
- **LOW, fixed:** the records command opened NOOP's store once per record. It now opens it once per call (`session_hrv_many`).
- **LOW, accepted:** the renderer can ask for HRV over any window. Each call is bounded, the same as the existing `noop_query` exposure.

## Verification

- **Gates:** tsc, lint and check:secrets pass; vitest 2998 passed / 9 skipped; neuro 21; src-tauri fmt and clippy clean, 2025 passed / 9 ignored; amara-router 133.
- **e2e:** 820 passed / 12 skipped / 0 failed. This ran before the review fixes, which touched only Rust and a test file.
