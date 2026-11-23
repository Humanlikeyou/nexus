---
paths:
  - "src-tauri/src/commands/muse.rs"
  - "src-tauri/src/commands/replay.rs"
  - "src-tauri/src/commands/sessions.rs"
  - "src-tauri/src/commands/recordings*.rs"
  - "src-console/neuro/**"
---
# Muse recordings are sacred

- Never purge, move, rewrite, compact or delete any file in `~/Library/Application Support/com.taisun.nexus/{replays,sessions,muse}/` without Taisun's explicit confirmation **for those exact files**. That includes the 10 raw 4-channel captures since 17 Feb (e.g. `eeg_20260217_100521`, `eeg_20260305_105204`) and the 18 Feb legacy session.
- Retention is 90 days, **confirm-only**: compute `expiring`, never act on it. No sweeper, no startup purge, no timer, no migration touching existing files.
- New sidecar caches (`<file>.bands.json`) are fine. Modifying a replay is not. Readers must open replays read-only.
- `remove_confirmed_recordings` must not exist until Taisun confirms the delete path may exist (logged in `docs/decisions/`). Until then `removeConfirmed()` stays unwired.
- Tests use temp dirs and fixtures only; never the real app-support folder.
- Any change here needs the **security-reviewer** agent.
