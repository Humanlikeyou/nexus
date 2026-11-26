# 0059: NEUROSENSE intelligence, Phase 8 (Taisun, 2026-11-26, in session)

Brief: `docs/specs/neurosense-intelligence-2026-11-26.md` (his "prompt 9").

Claude inspected the policy engine, actuators, audio feedback and session controls, then proposed six steps
(8a–8f) and five open questions. Taisun answered: **"Up to you. and Run"**. So the choices below are
Claude's, made under that delegation, and each one can be reversed.

## Choices made under the delegation

1. **FFT pipelines are not unified.** Training keeps `computeBands` (legacy-core) and display keeps
   `neuro/fft.ts`. Unifying them would change every past number. Instead, every target feature names its
   method, and the long-term summaries compare a session only with sessions made by the same training
   method version. Nothing historical is recomputed.
2. **Design boundary lifted narrowly** (as in 0055 and 0058):
   - New Phase 8 views and panels are Claude's: protocol settings, training records, trends and the
     post-session comparison.
   - The existing Design-owned screens keep their labels for now. "Focus" and "Calm" appear on about ten of
     them (Neuro, Recordings, Sessions, Hemi-Sync, Neuro trends, Muse journey, takes), and renaming them on
     only one screen would make the app inconsistent. The app-wide rename to neutral feature names
     ("beta/theta", "alpha/beta") is written up in `tasks/design-questions.md` for the Design team.
   - All the new Phase 8 surfaces, and Amara's reports, use the neutral names. The brief forbids
     presenting band power as attention or relaxation.
   - The protocol descriptions and coaching text in `protocols.ts` are reworded so they make no claims
     about mental state or learning.
   - Internal field names (`avg_focus`, `avg_relaxation`, `calm_pct`) stay unchanged for compatibility.
3. **No HRV.** No connected sensor gives beat-to-beat RR intervals. HRV biofeedback shows "not
   available", and nothing computes HRV from EEG or from PPG heart-rate estimates (a test enforces this).
   PPG heart rate stays an estimate, shown only when the PPG stream is fresh and its pulse peak is clear.
4. **Hemi-Sync** stays limited to audio Taisun imports himself from his own folders (the existing import).
   Nothing is downloaded or streamed.
5. **Order:** 8a → 8f, one local commit each, gates green before each commit, then stop for review
   (hard rule 11).

## Steps and acceptance criteria

| Step | Brief items | Acceptance |
|---|---|---|
| 8a | 1, 2 | Versioned protocols with explicit target-feature definitions (band, Hz, direction, method, reference); per-protocol settings (duration, target reward rate, sound, visual feedback, volume) persisted and validated; neutral wording; defaults leave training behaviour unchanged |
| 8b | 3, 4, 5 | The session progress record holds the protocol version, target definitions, settings, every threshold change with its reason, held time by reason, and time-in-target on valid data only; old session files are untouched; existing safety gates pinned by tests |
| 8c | 6, 7 | Volume, smoother ramps, a distinct "paused" state that never sounds like reward, a mute that really silences; visual feedback optional. The lens light already gave visual feedback, so it stays **on** by default and can be switched off (keeping existing work, rather than the "off by default" first proposed). The default sound stays the tone, which the training screen always played |
| 8d | 8 | An optional post-session rest recording (`post_session`, kept out of the baseline), compared with the same day's pre-meditation calibration and with the personal baseline; worded as a measured change, never as improvement |
| 8e | 9, 10, biofeedback | Hemi-Sync playback controls (play, pause, seek, volume, in order) linked to the session; biofeedback kept apart from neurofeedback; sensors checked, never assumed |
| 8f | Long-term + Amara | Per-protocol trends only from validated sessions (same method, enough valid data, minimum count) with confidence; Amara's local-only `nexus_neuro` section gets a structured training report (what changed, the comparison with history, supporting measurements, what is uncertain, optional training to try) and no raw EEG |

Every step must also hold:
- all gates pass, and the e2e suite runs for UI changes;
- `AUX_PROVEN = false` and the electrode lock are unchanged;
- no electrical stimulation and no imagined-speech decoding;
- recordings are untouched;
- no new outbound hosts;
- a missing sensor reads "not available", never 0;
- the security review covers the `neuro/**` and `sessions.rs` changes;
- hardware checks are reported as not run.

## Results

Filled in as each step lands.
