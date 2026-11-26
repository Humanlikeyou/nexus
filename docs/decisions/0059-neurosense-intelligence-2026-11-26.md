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
3. **No HRV** (*corrected 2026-11-26: this was wrong.* NOOP's `whoop.sqlite` has an `rrInterval` table of beat-to-beat intervals from the WHOOP strap, about 6.8 million rows and recent. They arrive when the strap syncs, not live. Using them for session HRV is proposed as a follow-up; until then the statement below stands in the app). As first written: no connected sensor gives beat-to-beat RR intervals. HRV biofeedback shows "not
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

| Step | Commit | What landed |
|---|---|---|
| 8a | `14e9101` | Protocol set `protocols-v1-0059`. `targetFeatures()` states each band's computed edges, measure, direction, reference and method, and flags declared ranges that differ (found: protocols declare delta 0.5–4 Hz, the FFT integrates 1.5–4 Hz; the declared Hz were never used). Per-protocol settings in `<data>/neuro/protocol_settings.json` (`neuro_protocol_settings_get/set`). Neutral protocol descriptions. Neuro › Training view |
| 8b | `3fecfe5` | Threshold log with reasons; `progress_record` (`progress-v1-0059`) in each session file's `data.neurofeedback`; `neuro_training_records` (read-only); safety gates pinned by tests |
| 8c | `b4a63fd` | Sound cues per state, a distinct held sound, volume, ramps from the current value. **Bug fixed:** muting only blocked later changes, so a playing tone kept sounding. The protocol's chosen sound is used (it was always the tone). End bell at the planned length (no auto-stop). The lens light can be switched off |
| 8d | `3006761` | Calibration kind `post_session` (never in the baseline), paired with the same day's pre-meditation recording (≤6 h, same source); changes measured in usual spreads |
| 8e | `a030003` | Hemi-Sync pause, resume, ±30 s, volume, next-in-wave offered; `biofeedback.ts` sensor availability; HRV and RR "not available"; HRV guard test |
| 8f | (this commit) | `neuro_training_trends`: per protocol and method, eligible sessions only (live, ≥70% usable, ≥3 min), at least 5 sessions; resting level per reward band, final threshold scale, usable signal; earlier vs later medians, Theil–Sen slope, confidence, calculation and uncertainty. Time in target is deliberately not trended (the adaptive threshold holds it near the target). Amara's local-only `nexus_neuro` section gets the training report and the latest before/after; keyword gate adds "meditat". Trends card in Neuro › Training |

**Security review** (nexus-security, read-only, over all six steps). No HIGH findings.
- **MEDIUM, fixed:** free-form strings from session files (protocol id, method, source, band and hold keys) could carry injected text into Amara's prompt. Now only slugs, version strings, the three known sources and short plain keys pass. Tested with newline and injection payloads.
- **MEDIUM, fixed:** the 4000-character section cap could cut off the honesty note, which came last. The note now comes first, and the training lines are kept within a 2400-character budget of whole lines.
- **LOW, fixed:** Theil–Sen is limited to the newest 100 sessions. The cards show fixed error text. Protocol ids are limited to 40 characters in both stores.
- **LOW, open:** the shared session reader (`sessions.rs`) reads each file with no size cap, and now runs on matching chat turns. Not changed here, because it is a recordings-rule file.
- **INFO, open (as in 0058):** a session's `data_source` is asserted by the renderer, so a compromised renderer could forge "live" records.
- **Passed:** no write, move or delete under sessions, replays or muse; `nexus_neuro` is still dropped whenever the cloud is allowed; no raw EEG anywhere; no new hosts; `AUX_PROVEN` false; safety gate and electrode code untouched; no HRV.

**Not run (needs hardware or Taisun):**
- a live Muse session with the new feedback sounds, held sound, volume and end bell;
- a real pre and post rest pair;
- the Hemi-Sync controls with a real track;
- PPG validation on a real headset;
- trends from real sessions (no session yet has a progress record);
- the packaged app.
