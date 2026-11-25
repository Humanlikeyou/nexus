# NEUROSENSE reliability (Phase 7) — brief

Saved 2026-11-26 from Taisun's prompt in session. The full NEUROSENSE specification is still not in the repository
(`AMARA_PROJECT_HANDOFF.md` §4); this brief is what Phase 7 follows. Standing constraints: `.claude/rules/muse-recordings.md`
(recordings are sacred) and `AUX_PROVEN = false` (electrode output stays locked).

## His prompt (verbatim)

> Audit and improve the existing Muse/NEUROSENSE data pipeline without replacing the current implementation.
>
> Inspect:
> - Rust Muse BLE commands.
> - useMuse.
> - EEG ring buffer.
> - FFT processing.
> - Calibration.
> - Signal quality.
> - Artifact detection.
> - Score confidence.
> - Neurofeedback policy and safety gates.
> - Session recording.
> - Replay.
> - Event persistence.
> - WHOOP acquisition and analytics.
>
> Priorities:
>
> 1. Verify the actual sampling rate and timestamp handling.
> 2. Verify that recorded samples are not duplicated, skipped or inaccurately timestamped.
> 3. Preserve raw recordings.
> 4. Make recording metadata reproducible.
> 5. Track data source: live, replay or demo.
> 6. Track valid-data percentage and channel quality.
> 7. Ensure low-quality or frozen signals cannot generate reward feedback.
> 8. Remove misleading fallback values from user-facing measurements.
> 9. Distinguish actual sensor measurements from derived estimates.
> 10. Make every analytical result traceable to its calculation and source data.
>
> Investigate the existing WHOOP sleep-data path and connect it correctly if the recorded data is available.
>
> Do not represent the current HR-based sleep estimates as official WHOOP sleep staging.
>
> Do not change electrode/stimulation safety restrictions.
>
> Do not introduce a competing Bluetooth connection service.
>
> Add regression tests around signal gating, missing data, data-source labeling, recording and persistence.
>
> Report any hardware-dependent tests that cannot be run.
