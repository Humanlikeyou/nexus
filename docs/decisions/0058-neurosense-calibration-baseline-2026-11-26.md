# 0058: NEUROSENSE calibration and personal baseline (Taisun, 2026-11-26, in session)

Brief: `docs/specs/neurosense-calibration-baseline-2026-11-26.md`.

| Question | His answer |
|---|---|
| Neuro screens are Design-owned (hard rule 10). The calibration UI? | **Override for calibration:** a new Neuro › Calibration view only, in the existing style. Every other Neuro screen stays Design-owned |
| A keyword-gated "neuro measurements" section in Amara's chat context? | **Yes, keyword-gated.** Local only: neural data never goes to the cloud tier (0053) |

## What was built

**A. Pre-meditation calibration** (`src-console/neuro/calibration.ts`, `CalibrationView.tsx`, route `mindbody` › `neuro/calibration`, header button "Calibrate (optional)")
- Connection and channel check: every electrode good or fair, the source live, not frozen. Replay and demo can't begin. The view can start the EEG stream (no session, no recording) and stops it when it closes, if it started it.
- Eyes-open, then eyes-closed resting recordings, each 20 s to 5 min (default 60 s).
- Epochs are non-overlapping 1 s, never the same samples twice. An epoch is rejected (with its reason) if a channel is stale, frozen, poor or flat, over 150 µV, or shows a blink, jaw or movement artifact.
- Features per condition: median relative band power, alpha peak frequency (spectral peak between 7 and 13 Hz with a prominence check, refined between bins; "not estimable" unless a quarter of the epochs show one), variability (MAD of log total power) and per-channel contact.
- Report: counts, reasons, features, and the comparison with the history (robust z and within/above/below the usual range). Stopping early keeps what was recorded. Skip is always offered.

**B. Protocol calibration, kept separate** (`TrainingSession.protocolCalibration()`)
- Records the protocol's thresholds from its own session baseline, the threshold scale, baseline ticks used and held by reason, the eyes-closed/open alpha ratio and `TRAINING_METHOD`.
- It goes into the session summary and is saved as a `protocol` report linked to the session id.
- It never enters the personal baseline.

**C. Personal baseline** (`src-tauri/src/commands/neuro_baseline.rs`)
- History in `<data>/neuro/calibrations.json` (0600, atomic). It holds features and counts only; the validated shape has no room for raw samples. When full (2000 reports) it refuses, and never drops history.
- Conditions are kept apart (eyes open, eyes closed). Method `baseline-v1-median-mad`, recomputed from the history on read, so it is reproducible.
- High-quality only: a live pre-meditation report, at least 20 accepted epochs and 60 % accepted, every channel at least 50 % contact. Lower-quality reports are kept and shown, never used.
- Statistics: median and robust SD (MAD × 1.4826) over the newest 20 qualifying recordings, plus alpha reactivity (eyes closed over eyes open).
- Confidence: none under 3 recordings, low 3–5, medium 6–11, high 12 or more, with an uncertainty sentence.
- A new report is compared with the baseline as it stood before it.
- Commands: `neuro_calibration_save`, `neuro_calibration_list`, `neuro_baseline_get`.

**Amara** (`terrence_runtime.rs`): a `nexus_neuro` guarded section (17th allowed tag), included only when the message mentions calibration, baseline, EEG, Neuro, Muse, alpha or band power, **and** Allow Claude is off. It carries the latest report's counts, quality and numbers, and the baseline with its confidence. It says these are signal measurements, not emotion, attention, stress or relaxation. No raw EEG.

**Wording:** every feature is named as a signal measure (for example "Alpha power (8–13 Hz)"), and the report and baseline carry the "not a measure of emotion, attention, stress or relaxation" note.

**Not built:** imagined-speech decoding (per the brief). Calibration does not change protocol thresholds; those still come from the session's own baseline, so the two stay separate.

## Not run (hardware-dependent)

- A real calibration on the headset: real contact checks, epoch rejection on real blinks and movement, a real alpha peak.
- A recorded session's protocol report saved in the app.
- The packaged app.

## Security review (required by the recordings rule for `src-console/neuro/**`)

The read-only review (`nexus-security`) confirmed: the report shape has no sample array, the condition fields are typed with `deny_unknown_fields` and bounded, the store refuses when full, it never touches `replays/`, `sessions/` or `muse/`, and the not-a-state wording holds. Fixed:
- **HIGH:** the cloud exclusion was checked only where the section was built. Now `nexus_context_message`, where every turn's context is assembled, drops any `NEURAL_TAGS` section while Allow Claude is on, whoever supplied it. Test: `a_neural_section_never_goes_out_while_the_cloud_is_allowed`.
- **MEDIUM:** the protocol block is capped at 3 KB, and its `protocolId` must be a short slug (it reaches Amara's context). Each report must fit the store budget (1000 reports × 6 KB), so "full" is reported honestly.
- **MEDIUM:** Calibration's "Start the headset stream" now claims the stream only when nothing was streaming, so closing the view never stops a stream a session started. Test: `never claims (or later stops) a stream that was already running`.

Left as noted:
- `source` on a report is supplied by the renderer, so a direct IPC call could label a forged report as live (an integrity issue only; it stays on this Mac).
- `read_json`'s lstat-then-read is shared by every local store.
- The keyword gate is broad ("alpha", "muse"), but local-only.

Not verified: whether the Telegram, n8n or Hermes request paths ever take these sections. The callers found that turn sections into a turn's context (`terrence_runtime.rs`, `amara_turn.rs` and `inference/mod.rs`) all go through `nexus_context_message`, which now applies the drop.
