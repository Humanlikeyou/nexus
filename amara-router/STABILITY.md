# Router stability notes (B01-B03, 2026-11-11)

Evidence: `~/Library/Logs/amara-router/router.log` (read-only, 14 starts 4-9 Nov) and `routes.jsonl`
(no content; 4-11 Nov). No prompt text is quoted here.

## What the logs show
- 290 "no first token in time; still loading" lines (fast 110, deep ~180), 142 of them in the last
  run alone; 23 "fast reply gave up waiting for deep to finish"; 2 "deep load failed" (one "fast stayed
  busy", one "warm deep: unreachable or timed out"); 9 "background warm failed".
- routes.jsonl: 169 deep and 52 fast requests ended `error`, mostly `/deep` and trigger-phrase asks.
- 13 clean "shutting down" (SIGTERM) against 14 starts. Panics `route.rs:132` (char boundary inside
  a curly apostrophe, `strip_command`) are in the log: already fixed in source (`str::get`), but the
  installed binary (`~/.local/bin/amara-router`) may predate it. Not verified.
- LaunchAgent last exit -9 = SIGKILL, not a Rust panic or a clean stop. Candidates: macOS memory
  pressure kill (fast + deep + apps on 32 GB), or a manual `kill -9` / `launchctl kickstart -k`.
  Not proven from the logs; no crash report was read.

## Cause (most likely, supported by the log pattern)
Fast and deep do not fit together, so every deep ask unloads fast and every fast request unloaded
deep (`restore_fast`), including the 30-minute home_brief cron. Each swap is a cold load (60-140 s
seen) so the next request times out waiting for a first byte, and a deep load queued behind a busy
fast gives "fast stayed busy". Before this change `auto_deep` also defaulted to true in code, so
long prompts and classifier-HARD verdicts triggered extra swaps.

## Fixes made
- B01: fast requests wait out `loading.deep_grace_secs` (default 60) after the last deep reply
  before unloading deep, and skip the unload when `/api/ps` shows loaded size_vram plus fast's last
  seen size_vram fits `loading.gpu_budget_gb`. A waiting fast request still gets a 503 +
  Retry-After at `fast_wait_ms`.
- B02: `auto_deep` default false; it also gates classifier HARD and the ps-unavailable/cold-fast
  heuristics.
- B09: `strip_phrase` no longer slices a lowercased copy.

## Needs Taisun (router not rebuilt or restarted by this work)
1. Rebuild/install and restart the LaunchAgent to pick up the fixes.
2. After restart, check `dmesg`/`log show --predicate 'eventMessage contains "amara-router"'` and
   Console crash reports around the next -9.
3. Consider `sudo sysctl iogpu.wired_limit_mb=26000` (see config.example.toml) so both models fit.
