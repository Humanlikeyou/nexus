# 0060: one EEG band-power pipeline (Taisun, 2026-11-26, in session)

Taisun: **"i think it better to unify them FFT pipelines"** (open since 0057, re-raised in 0059).

## What differed (checked in the code before changing it)

| | `neuro/fft.ts` (display, 0058 calibration) and Rust sidecar bands | `legacy-core` `computeBands` (training, plain-recording summary) and Rust sidecar focus/relaxation |
|---|---|---|
| Window | last 256 samples (1 s), mean removed, Hann | Welch over up to 512 samples (2 s, 50% overlap) when available, else one 256-sample window; mean removed by the caller only |
| Band edges | delta 1–4, theta 4–8, alpha 8–13, beta 13–30, gamma 30–45 Hz | delta 1.5–4, gamma 30–50 Hz |
| Edge bins | half-open: each 1 Hz bin is counted in one band | inclusive at both ends: the 4, 8, 13 and 30 Hz bins are counted in **both** neighbouring bands, so relative powers overlap |
| Electrodes | power averaged across electrodes, then made relative | relative power per electrode, then averaged |

## Decision

`neuro/fft.ts` is the one pipeline, because the personal baseline (0058) is built on it and its edges count every bin once.

Everything that computes band power uses it now:
- the display,
- the pre-meditation and post-session calibration,
- neurofeedback training,
- the plain-recording summary,
- the Rust recording sidecars (bands, and the focus/relaxation series).

**Method:** one 256-sample (1 s) Hann window, mean removed, 1 Hz bins counted once. Bands are delta 1–4, theta 4–8, alpha 8–13, beta 13–30 and gamma 30–45 Hz, with SMR 12–15, low beta 15–20 and high beta 20–30 Hz as sub-bands. Power is averaged across electrodes before it is made relative to the 1–45 Hz total.

## What this changes, and what it does not

- **New numbers differ from old ones.** Training values, protocol thresholds and the focus/relaxation scores of new sessions are computed the new way. The training method version becomes `training-v3-0060`. Long-term trends (0059) compare only sessions of the same method, so old and new sessions are never mixed in one trend.
- **Nothing stored is rewritten.** Session files keep the numbers they were saved with, the calibration history is untouched, and replays are never touched. Recording sidecars are caches: `SIDECAR_VERSION` goes up, so each sidecar is rebuilt from its replay with the unified method the next time that recording is opened. The rebuild writes a temp file and renames it over the cache, as before.
- **Training is a little less smoothed.** It used to average three overlapping 1 s windows (Welch, 2 s). It now uses one 1 s window at each 0.5 s tick, the same as the baseline it is compared with.
- **Not part of this decision:**
  - The jaw and line-noise detectors (`legacy-core/lib/fft.ts`) are artifact detectors, not features, and stay as they are.
  - The gamma artifact threshold (80, compared against a percentage) still does not match its units. It is a separate open question.

## Results

- **Front end:** `neuro/fft.ts` gains `bandFeatures` and `BAND_EDGES_HZ`.
  - `computeBands` now delegates to it.
  - Training and the plain-recording summary call it directly.
  - `TRAINING_METHOD` is `training-v3-0060`, and its `fft` and `bands_hz` fields name the pipeline.
  - The target-feature edges in `protocolConfig.ts` follow it.
  - The protocol-calibration report saved with each session takes its method version from `TRAINING_METHOD` (it was a hard-coded `training-v2-0057`).
- **Rust:** `recordings_dsp::focus_relaxation` uses half-open bins, `band_power_inclusive` is removed, and `SIDECAR_VERSION` is now 2.
- **Parity fixture:** regenerated with `MUSE_PARITY_UPDATE=1`, the documented way. Bands and quality are unchanged for all five inputs; only focus changed, on three inputs:

  | Input | Focus before | Focus after |
  |---|---|---|
  | pure 10 Hz alpha | 15 | 52 |
  | 50 Hz hum | 16 | 54 |
  | noise | 0 | 5 |

  The old inclusive 8 Hz bin put alpha's spectral leakage into theta, which pulled focus down for an alpha signal. This is the distortion the unification removes.
- **Tests:** `unifiedFft.test.ts`, six tests:
  - display and training agree;
  - the five bands add up to 100%;
  - `computeBands` equals `bandFeatures`;
  - an alpha signal no longer leaks into theta;
  - the edges are stated once;
  - an incomplete or non-finite window gives zeros.

  `reliability.test.ts` now pins the new version.
- **Gates:** tsc, lint and check:secrets pass; vitest 2997 passed / 9 skipped; neuro 21; src-tauri fmt and clippy clean, 2016 passed / 9 ignored; amara-router 133.
- **Security review** (nexus-security, read-only). Nothing writes, renames or deletes a replay or a session file. The sidecar rebuild is lazy, happening only when a recording is opened, and is skipped for the recording in progress. `cached_session_samples` stays read-only. No panic or division by zero on empty, short, NaN or all-zero input. Safety gates, `AUX_PROVEN` and the electrode lock are untouched.
  - **MEDIUM, fixed:** the Rust sidecar still averaged per-electrode focus and relaxation scores. It now uses `focus_relaxation_channels`, which averages the AF7 and AF8 power spectra and then takes the scores, as `bandFeatures` does. Three two-electrode pairs were added to the parity fixture, and Rust matches the TypeScript on all of them.
  - **LOW, fixed:** a doc comment gave the wrong alpha peak (10) for an empty window; the code returns 8.
  - **Noted, unchanged:** the unused `useFFT` hook still averages the AF7 and AF8 waveforms. It has no caller.
- **Final gates:** tsc, lint and check:secrets pass; vitest 2997 passed / 9 skipped, run alone. One full-gates run overlapped e2e, and a Smart home slider timing test failed under that load; run alone it passes. Neuro 21; src-tauri fmt and clippy clean, 2016 passed / 9 ignored; amara-router 133. e2e: 820 passed / 12 skipped / 0 failed.
- **Not run:** a live Muse session with the new pipeline; opening an old recording in the app to see its sidecar rebuilt.
