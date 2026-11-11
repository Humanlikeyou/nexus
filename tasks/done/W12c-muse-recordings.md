# W12c (+12a Muse samples/score scale): Muse recordings wiring
- **Owner:** rust-executor (A, B), frontend-executor (C). security-reviewer on every chunk. Plan by architect, 2026-11-11.
- **URGENT for Taisun:** a delete path already exists: `delete_session` (sessions.rs:327, registered lib.rs:255, used by legacy src/lib/api.ts:162) deletes the first session file whose name *contains* the id (an empty id → the first file). Not changed; his call (keep / harden / unregister).

## Replay line formats (new files; old files parse as format 1)
- Header (written lazily on first line, lists only what subscribed): `{"kind":"header","format":2,"sample_rate":256,"channels":["TP9","AF7","AF8","TP10","AUX"],"ppg":["ambient","infrared","red"],"motion":["accelerometer","gyroscope"],"start_ts":ms,"session_id":"uuid"}`. Old: no format/ppg/motion → 1/[]/[].
- EEG/AUX: unchanged `{"kind":"sample","dt_ms","timestamp","channel","samples":[12]}`; only `Aux("AUX")`, never AUX_L.
- PPG: `{"kind":"ppg","source":"infrared|red|ambient","timestamp":ms,"samples":[6]}`. Motion: `{"kind":"motion","sensor":"accelerometer|gyroscope","timestamp":ms,"samples":[[x,y,z]x3]}` (g, deg/s).
- Trailer (may be missing after crash): `{"kind":"trailer","end_ts":ms,"aux":{"detected","placement":"occipital","detectedBy":"signal-quality","detectedAt"}|null}`.

## Chunk A: recording (muse.rs, replay.rs, lib.rs)
SUBSCRIBED state set in subscribe_optional_streams; emit_optional async → maybe_record_sample(AUX) / maybe_record_ppg / maybe_record_motion; lazy header via PENDING_HEADER + write_line; `mark_recording_aux(AuxInfo)` command (open file only); stop writes header-if-missing, trailer, flush.
## Chunk B: readers (new recordings.rs; sessions.rs)
resolve_replay(id) via validate_name/validate_file_name/safe_join, File::open read-only; parse_replay index with 1 s byte offsets, LRU 4; gap > 0.5 s → NaN. Commands: list_recordings (retention computed, 90 d, never acted on), read_recording_window (≤60 s, high-pass 0.5 Hz, bucketed), read_body_window, recording_series (Rust ports of relativeChannelBandPowers + assessChannel; focus/calm from neurofeedback.timeline else computed; sidecar `<file>.bands.json` via temp+rename, never while recording), get_session_samples (5 s steps; list_sessions attaches if sidecar exists). Score scale 0–100 canonical: save_session writes summary.score_scale=100; read normalises legacy once; disk never rewritten.
## Chunk C: frontend
recordingsCache.ts (30 s blocks, LRU 8, dedupe, prefetch), liveRecordings(sessions, now, cache) synchronous over cache (cold block → UNWIRED), useRecordingsAdapter hook (one-line swaps in SessionsScreen.tsx:13, NeuroScreen.tsx:126, logic only), auxDetect.ts (5 consecutive good 1 s steps, SD 1.5–40 µV) run in fit stage → markRecordingAux + data.aux. removeConfirmed stays UNWIRED. AUX_PROVEN untouched.
## Tests
Parser old/new/truncated/header-only/legacy data:[…]; record formats + lazy header + AUX_L not recorded; AUX fixtures on/off; retention 89/90/91 d injected clock; readers leave bytes/len/mtime unchanged (0444 fixture); safe_join rejects ../ and symlinks; Rust↔TS parity fixtures; no-delete test (only allow-listed delete_session); vitest cache/adapter.
## Decisions for Taisun
delete_session; AUX detection outside fit stage; sidecar location (replays/ vs replays/.cache/); sequence field on PPG/motion; adopt score_scale 100; delete path stays unwired.

## Evidence (rust-executor, chunks A + B, 2026-11-11)
- Scope change (Taisun, docs/decisions/0003): `delete_session` removed (fn in sessions.rs, lib.rs handler, wrapper in src/lib/api.ts). `tests/no_new_delete_path.rs` allow-lists nothing.
- Chunk A: replay.rs lazy header (format 2, StreamSet), `maybe_record_ppg/motion`, AUX recorded and AUX_L not, trailer, `mark_recording_aux`; muse.rs SUBSCRIBED state, async `emit_optional`.
- Chunk B: new recordings.rs + recordings_dsp.rs (ports of fft.ts, lineNoise, assessChannel, computeBands scores, headMotion); list_recordings, read_recording_window, read_body_window, recording_series, get_session_samples; `.bands.json` sidecar (temp + rename, never for the open file); list_sessions attaches samples when a sidecar exists; save_session writes summary.score_scale=100, legacy read normalised.
- Failing before: `score_scale_100_marks_a_new_file_that_is_never_rescaled` failed with guard disabled (left (Some(40.0), Some(60.0)), right (Some(0.4), Some(0.6))), passes with it.
- Gates: cargo fmt --check clean; clippy --all-targets -D warnings clean; cargo test src-tauri: lib 441 passed / 5 ignored (was 426), all integration targets ok (new: muse_recording_flow 1, no_new_delete_path 2); tsc clean; eslint clean; vitest 1290 passed / 9 skipped (new: museParity 1, registry test +1).
- Not run: security-reviewer agent, test:neuro, test:e2e, check:secrets, amara-router tests, any run against the real app-support folder or a real recording/headset.

## Evidence (frontend-executor, chunk C, 2026-11-11)
- New: neuro/recordingsCache.ts (30 s blocks, LRU 8, in-flight de-dupe, failures remembered until retry(), prefetch within 6 s of block end when the recording has the next block, reply-shape checks), neuro/useRecordingsAdapter.ts (useSyncExternalStore; refreshes the list when the saved-session count changes), neuro/auxDetect.ts (pure; 0.5 Hz high-pass, robust SD 1.5-40 uV, rail check, 5 consecutive good 1 s steps, line noise tolerated).
- Edited: neuro/recordings.ts (liveRecordings(sessions, now, cache?) synchronous over the cache: cold/failed block = UNWIRED; list()/retention() fall back to recordingFromSession until the list loads or when the reader list is empty; gaps become NaN; removeConfirmed and liveAux stay UNWIRED), app/useMuseSession.ts (AUX detection every 1 s while recording and the guided stage is "fit" -> markRecordingAux + data.aux in the save snapshot; no detection for "Recording only" because it has no fit stage).
- Design-owned screens touched (logic only, markup/classes/copy identical): screens/SessionsScreen.tsx (adapter line -> useRecordingsAdapter(data.sessions, !given), unused useMemo import dropped), screens/NeuroScreen.tsx (hook call placed above the early return at the top of the component; liveAux line reads it). bridge.ts and src-tauri untouched. AUX_PROVEN untouched.
- Tests: neuro/recordingsCache.test.ts (6), neuro/auxDetect.test.ts (4), neuro/recordingsLive.test.ts (8, incl. mocked invoke never receiving remove/delete and only the 4 read commands).
- Found by e2e: the mock returns [] for unmocked list_* commands, which hid the sessions in recordings.spec "detail"; fixed by falling back to sessions when the reader list is empty (plus a test).
- Gates: tsc clean; eslint clean; vitest 1309 passed / 9 skipped (was 1290/9); test:neuro see report; e2e recordings.spec + neuro-flow.spec + mind-body.spec 34 passed.
- Not run: security-reviewer agent, real headset/recording, cargo, check:secrets.
- Questions appended to tasks/design-questions.md (loading flash, NaN gaps).
