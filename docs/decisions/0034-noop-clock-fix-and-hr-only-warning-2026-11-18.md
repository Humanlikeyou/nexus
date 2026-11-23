# 0034: Fix NOOP's reconnect after a strap power loss, and warn in Amara about heart-rate-only nights (Taisun, 2026-11-18, in session)

Context: on 2026-11-17 the strap battery died at 17:01. After charging, NOOP stayed on a heart-rate-only link (standard 0x180D/2A37) and only sent "Set Clock" at 12:15 the next day. So the strap recorded no motion, temperature or breathing overnight, NOOP found "NO-NIGHT", and 2026-11-18 has no sleep, HRV or recovery.

Claude offered two things: (1) fix NOOP so it sets the strap clock (and restores the custom data channels) right after a power loss, in the NOOP repo; (2) add a "Needs you" warning in Amara when a night came through as heart rate only.

His words: **"yes do it"**

Scope:
- NOOP: work on a new local branch off `lucid/v12` in `~/projects/noop-lucid-current`, with tests. No push. No change to the running NOOP app or its data. Building and installing the new NOOP is a Taisun step.
- Amara: a read-only check over the NOOP store (sample counts only, never values), shown as a Needs-you pointer.
