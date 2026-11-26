# NEUROSENSE intelligence (Phase 8): brief

Saved 2026-11-26 from Taisun's prompt in session ("prompt 9"; Phase 8 in `AMARA_PROJECT_HANDOFF.md` §5).
The full NEUROSENSE specification is still not in the repository (§4). This brief is what Phase 8 follows.
Standing constraints: decisions 0057 and 0058, `.claude/rules/muse-recordings.md` (recordings are sacred),
`.claude/rules/design-boundary.md`, `AUX_PROVEN = false` (electrode output stays locked).

## His prompt (verbatim)

> Extend the existing neurofeedback and meditation capabilities using the approved NEUROSENSE data contracts.
>
> First inspect the existing neurofeedback policy engine, actuators, audio feedback and session controls.
>
> Implement incrementally:
>
> 1. Configurable neurofeedback protocols.
> 2. Explicit target-feature definitions.
> 3. Quality-gated feedback.
> 4. Auditable training rules.
> 5. Session-level progress records.
> 6. Audio feedback improvements.
> 7. Optional visual feedback.
> 8. Pre-session and post-session comparisons.
> 9. Meditation session organization and playback controls.
> 10. User-owned or authorized Hemi-Sync audio support.
>
> Keep biofeedback and neurofeedback distinct.
>
> Use available sensors only:
> - EEG from Muse.
> - IMU if supported and enabled.
> - PPG only after validating Muse 2 acquisition mode and signal quality.
> - Heart rate and RR intervals from an appropriate connected sensor.
>
> Do not calculate HRV from EEG or from ordinary heart-rate estimates.
>
> Do not assume sensor availability.
>
> Build longitudinal summaries using validated metrics and sufficient observations.
>
> Amara may explain:
> - What changed.
> - How it compares to personal history.
> - Which measurements support the observation.
> - What remains uncertain.
> - Which optional training may be worth testing.
>
> Do not claim that in-session metric changes prove cognitive improvement.
>
> Maintain all existing safety gates.
>
> Do not implement electrical stimulation.
>
> Do not implement imagined-speech decoding in this phase.

## Scope and acceptance criteria

Proposed after the inspection, pending Taisun's confirmation. They are recorded in decision 0059 once he confirms.
