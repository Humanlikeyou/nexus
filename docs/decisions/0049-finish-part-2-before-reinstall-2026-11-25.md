# 0049: Finish part 2 of the 0045 brief, then reinstall once (Taisun, 2026-11-25, in session)

His words (verbatim): **"do it that should have been done first before reinstalling etc"**.

- Claude's reading: the remaining part 2 items should have been finished before the 152a549 install, because he had said "After you're done with everything commit and reinstall" (0045). So Claude finishes them, then commits and reinstalls once at the end.
- **Scope (all from the 0045 brief and its handoff §46):**
  1. The Nexus side of the Telegram inbox, and the timed check-in, habit and money questions.
  2. The Neuro brain rings and the heart-rate take graph, in Three.js (the approved mock-up gallery).
  3. The four Mind-Body charts.
  4. Money charts, Amara's advice with its assumptions, the exchange-rate form, and the last ¥ glyphs.
  5. Marking a habit done from chat, the Enterprise History list, and venture links on tasks and content.
- **Chart variants (Claude's pick, since he approved "the best markup"):** the A variants (2D-clean, drawn in Three.js) for Readiness, Sleep and Trends, and B (3D bars with a ratio dial) for Training Load. The existing charts stay as the fallback when WebGL is unavailable.
- **Not covered:** any push. The last push OK (0048) was for one push of amara-integration that has already happened. Publishing this work needs a new OK.

## Found while finishing: the brain ring had no data

- The chart lane found that real saved sessions carry no band data: `list_sessions` only attaches `samples` when a current `.bands.json` sidecar already exists, and nothing built one for the Neuro page.
- **Fix (frontend only):** `src-console/neuro/useSessionBands.ts` calls the existing `get_session_samples` for the newest 5 sessions, one at a time, and merges the series in.
  - That command reads the recording read-only and writes only the allowed derived `.bands.json` cache.
  - No Rust, no change to recordings.rs/replay.rs/sessions.rs, and no file in replays/sessions/muse is modified, moved or deleted (muse-recordings.md).
  - A failing read leaves the session as it was and is not retried in a loop.
- Because the first read of a long recording does real work, it runs off the UI thread in the Rust command and the ring appears once the series arrives.

## Outcome

- Gates on the final tree: tsc, lint, check:secrets pass; vitest 2907; neuro 21; src-tauri fmt and clippy clean, 1843 passed / 7 ignored; amara-router 131. Full e2e run alone: 800 passed / 12 skipped / 0 failed (before the Rust-only and Python security fixes).
- The security review (strong model) found no HIGH, 2 MEDIUM and 6 LOW; all but one optional LOW are fixed (handoff §47).
- Commit, then install. Pushing needs a new OK (not covered above).

## Install

- **Committed:** d284e4b. **Installed:** d284e4b, quit and reopened, confirmed running. Rollback: `~/Backups/Amara OS-backup-20261125b.app` (build 152a549).
- **Not pushed.** origin/amara-integration is at 152a549.
- **What happens next on its own:** the first timed question goes out at the default 22:00 JST if there is no check-in that day. Habit reminders go out at 20:00 or each habit's own time, and the money check-in on Sunday 19:00. All of it can be switched off in Settings › Check-ins on Telegram. Replies reach the app only after Taisun installs the Hermes tools (`~/projects/hermes-patches/nexus-inbox/README.md`).
