# 0057: NEUROSENSE reliability, Phase 7 (Taisun, 2026-11-26, in session)

His brief (verbatim, saved as `docs/specs/neurosense-reliability-2026-11-26.md`): audit and improve the Muse/NEUROSENSE pipeline
without replacing it, with the ten priorities listed there. Standing constraints were kept: recordings are sacred
(`.claude/rules/muse-recordings.md`), `AUX_PROVEN = false`, electrode output locked, and no second Bluetooth service.

## What the audit found (verified in the code before changing it)

| # | Finding | Where |
|---|---|---|
| 1 | Sample rate is never measured: 256 Hz is assumed in about 8 places. One host-clock stamp per 12-sample packet, taken on arrival | `muse.rs`, `replay.rs`, `fft.ts` |
| 2 | The EEG packet counter was ignored, so lost or repeated BLE packets were invisible. The recorder also dropped the counter | `muse.rs` `parse_eeg_packet` |
| 3 | A stream that died (15 s with no data, or the BLE stream ending) exited silently while the app still said "Streaming" | `muse.rs` stream task |
| 4 | A second `start_eeg_stream` could leave two tasks emitting every packet twice | `muse.rs` |
| 5 | Replay and demo used the live `muse-eeg` event with no source, could run alongside live, and demo could be recorded unlabelled. Sessions were always saved as `live_muse` | `replay.rs`, `useMuseSession.ts` |
| 6 | The feedback gate was called with fixed inputs (confidence 1, quality "good", not frozen), so its confidence and frozen rules never ran. A held (frozen) gate returned the last signal, which could be a reward | `training.ts`, `outputEngine.ts` |
| 7 | No frozen-signal detection on the live path (a repeating buffer passed every check) | `telemetryStore.ts` |
| 8 | A failed UI emit dropped the sample from the recording, and write errors were swallowed | `muse.rs`, `replay.rs` |
| 9 | Session files were written with `create_new` then `write_all`, so a crash could leave a truncated file | `sessions.rs` |
| 10 | Misleading fallbacks: 0 % bands from incomplete channels (meters and sparklines), unknown sleep stages drawn as 0 min, a real 0–100 score of 1 shown as 100, and "Stage order … isn't sent yet" although Fitness › Sleep reads it | `NeuroScreen.tsx`, `bandHistory.ts`, `MindBodyScreen.tsx`, `HealthScreen.tsx`, `sessionSamples.ts` |
| 11 | Sleep: "core" counted as light in the hypnogram but dropped from the totals | `noop.rs` |
| 12 | NOOP scores sleep stages itself (its own "Sleep staging V2" recipe), so they are NOOP's estimate, not WHOOP's official staging. The UI did not say so | `SleepDetailPanel.tsx`, `MindBodyScreen.tsx` |

## What changed

**Acquisition (`muse.rs`)**
- Each EEG/AUX packet's 16-bit counter is read (`packet_sequence`, `SeqTracker`). A repeat, or a packet up to 64 behind, is dropped, never emitted or recorded twice. A larger step back is a headset counter restart and is kept. Gaps and restarts are counted per channel (`muse_stream_continuity`).
- Every `muse-eeg` sample carries `source` (`live`, `replay` or `demo`) and `sequence`.
- One source claim owns `muse-eeg`, and a stream generation guard makes a stale task exit. A second live start is a no-op. Live is refused while a replay or demo runs, and replay/demo are refused while live streams.
- A stream that gets no data for 15 s, goes silent for 30 s, or loses its BLE stream ends itself. It emits `muse-stream-ended {reason}` and the state returns to Connected; the app shows a plain message.
- Samples are recorded before the UI emit, so a failed emit no longer cuts a hole in the raw recording.

**Recording (`replay.rs`, new files only; no existing file is touched)**
- Header: `data_source`, `sample_rate_basis: "nominal"`, `timestamp_basis: "host_receive_ms_per_packet"`, `samples_per_packet`, `eeg_decoder`, `app_version`.
- Sample lines: `seq`.
- Trailer `stats`: per channel the packets, samples, **measured rate** (from what was written; null under 2 s), missed and duplicate packets; plus `write_errors` and `other_source_dropped`.
- A recording keeps one source: demo during a live recording is never written into it, and PPG/motion never go into a demo or replay recording.
- Readers parse JSON values, so older readers and all existing format-1/2 files are unaffected.

**Persistence (`sessions.rs`)**: a session file is written to a private temp file, synced, then hard-linked into place. It is whole or absent, and never over an existing session.

**Signal gating (`telemetryStore.ts`, `training.ts`, `outputEngine.ts`)**
- The telemetry snapshot has `source`, `bandsValid` and per-channel `continuity` (packets, missed, repeated, `frozen` after 2 identical packets).
- Training holds with `not_live` for replay, demo or unlabelled data, and `frozen` for a frozen channel.
- The gate now gets real inputs: the worst electrode fit, and confidence = the share of the last 10 s with good contact on every electrode, so it recovers within seconds after a hold.
- A held gate returns a neutral signal, never the last reward.

**Traceability**
- The training summary has `valid_pct`, `training_ticks`, per-electrode `channel_fit_ticks`, `data_sources`, and `method` (`TRAINING_METHOD`: FFT, band edges, formulas, thresholds, version).
- The session data has the real `source`, `data_source`, `received_rate_hz` and `continuity`.

**Fallbacks and labels**
- Band meters and sparklines show a dash (not 0 %) until all four channels are complete.
- Unknown sleep stages are left out.
- `score()` honours `score_scale: 100`.
- The Mind–Body sleep note and Fitness › Sleep now say the stages are NOOP's estimate, not WHOOP's own staging, that Awake is in-bed minus asleep, and where the stage order is.

## Security review (required by the recordings rule) and what it changed

The read-only review (`nexus-security`) confirmed:
- No path can overwrite, truncate or delete an existing replay or session file.
- `hard_link` is atomic and no-clobber on APFS.
- Readers still read format-1/2 files.
- The lock order is consistent.
- AUX and electrode locks are unchanged, there is no new BLE service or outbound host, and nothing sensitive is logged.

It found one HIGH, two MEDIUM and four LOW issues, all fixed:
- **HIGH:** after a stream restart the headset counter starts again, and the tracker treated every packet as a repeat, so an open recording silently dropped live EEG. Now a step back of more than 64 is a counter restart (`Reset`, counted as `counter_resets`) and the data is kept.
- **MEDIUM:** a recording whose first lines were PPG/motion could later accept demo EEG under a `live` header. PPG/motion now fix the source as live, so the demo is refused.
- **MEDIUM and LOW:** there was a race between the live and replay/demo checks, and concurrent live starts could both pass. One claim (`claim_source` / `release_source`, one lock) now owns `muse-eeg`. A second live start while claimed is a no-op, and a stale live claim (not streaming, older than 30 s) never locks replay out. The telemetry store also clears its buffers when the source changes.
- **LOW:** the session save now fsyncs the folder after the link. Its temp file lives in `<data>/.tmp-writes`, outside `sessions/`. The `no_new_delete_path` guard (no delete call in the recordings files) still holds and was not loosened.
- **LOW:** an auto-ended stream now sends the halt command (best effort), as a normal stop does. The recording stays open until Taisun stops the session, so nothing is lost or auto-saved.
- **LOW:** the frozen flag is also passed to the feedback engine (defence in depth).

## Sleep data path

Per-night stage segments exist in NOOP's `sleepSession.stagesJSON` and are already connected through `noop_hypnogram` to Fitness › Sleep. Mind–Body shows the totals. Whether a given night has segments depends on NOOP: a dictionary-shaped value gives totals only. No real NOOP database was opened. Nexus computes no HR-based staging itself.

## Not done (left for a decision or a later phase)

- **Per-sample timestamps from the headset clock:** the Muse counter gives order and loss, not time. A device-time axis needs a clock model, which is a later phase.
- **The two FFT pipelines** (display `fft.ts` versus training `computeBands`) still differ in band edges and windowing. Both are now named in `TRAINING_METHOD`. Unifying them changes every historical number, so it waits for his call.
- **Calibration quality:** baselines are still not persisted and there is no minimum-quality check on a baseline.
- **Jaw detector:** inactive until its baseline exists.
- **Gamma artifact threshold** (80, compared against a percent) is still mismatched.
- **Auto-reconnect and battery telemetry:** not added.
- **Display changes:** the copy changes on the Design-owned Mind–Body and Fitness › Sleep screens are noted in `tasks/design-questions.md`.

## Not run (hardware-dependent)

- A live Muse session on the headset: real packet counters, gap and duplicate counts, measured rate, frozen detection on a real stuck stream, and `muse-stream-ended` on a real link drop or power-off.
- Reward audio during a live session, and its absence during replay or demo, in the real app.
- A recording made on the headset, and its trailer stats, read back.
- Sleep stages from Taisun's real NOOP database (the fixtures are synthetic).
- The packaged app.
