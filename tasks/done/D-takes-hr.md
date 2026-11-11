# D-takes-hr: take memory (1111t §11 step 1) + Home heart-rate take (1111u §4, §8 steps 1-3)

Approved: docs/decisions/0006. Design's `Wired<T>` shapes unchanged.

## Decisions
- **TakeLog = Rust JSONL** (`{app data}/takes/take_log.jsonl`, `src-tauri/src/commands/take_log.rs`), not kv. kv `save_kv` overwrites the whole file, so two writers (TS daily take, Rust Home HR take) doing read-modify-write lose entries; Rust must append anyway. One process mutex; append = one line; forget = temp file + rename.
- **forget** blanks `text`/`cited`/`memoryLine`/`hr` and sets `forgottenAt` (Design §2 shape); the row stays; history hides it.
- **Migration**: `take_log_migrate` copies kv `amara_take_<page>` records, read-only on kv (old keys never touched). Id `<page>-<generatedAt>` = the id generate.ts uses, so migrate/append never duplicate.
- **Retention**: 365 days computed in `withinRetention`; no deletion job. Summary into Memory is NOT built (TODO in HANDOFF-1111t §11; no live Letta writes).
- **HR generation lives in Rust** (`home_hr_take.rs`) so the 30-min gate, per-page lock and TakeLog append are one atomic backend step (§4). Calls `chat_with_router(force_provider "light", local_only true)`; Claude-tier reply rejected; 45 s timeout; number/causal/medical validator and template are Rust ports of `hrTextOk`/`hrTemplate`/`hrMemoryLine` (pinned by tests). Lock is a tokio mutex held across the call: the second caller waits, then returns the first take.
- usual = same clock window over previous 14 days, waking hours (06-22 Tokyo), days with >=10 covered minutes, >=7 such days, 25th-75th percentile of per-minute means. Stale = last sample > 10 min. Take needs >= 10 of 15 minutes.
- `settings().autoDone` shows Design's default true; `setAutoDone` stays unwired (nothing detects yet).

## Acceptance criteria and evidence
- [x] hr_window bucketing, coverage, usual percentile (+ <7 days, thin day, waking hours), stale source, real sqlite read: 6 Rust tests in home_hr_take.rs
- [x] within 30 min same take (no model call, nothing written); 30+ min new take with memory line: test
- [x] blocked (not_enough, source_down, no data) writes nothing, clock not restarted, later success writes: test
- [x] two concurrent calls -> one take, one model call: test
- [x] invented number / cloud reply / model down -> template, tier "template"; validator rejects invented numbers, causal/medical words: Rust + TS tests
- [x] TakeLog append/idempotent/concurrent/forget/migration/damaged line: 6 Rust tests in take_log.rs; history filter/retention/memory line/live adapter/forget/generate append: src-console/takes/takeLog.test.ts
- [x] commands registered (lib.rs next to noop entries): hr_window, home_hr_take_status, home_hr_take_maybe_refresh, take_log_append/list/forget/migrate

## Gates (run 11 Nov)
- cargo fmt --check clean; clippy --all-targets -D warnings clean; cargo test src-tauri: 489 passed / 5 ignored (includes unit + integration targets)
- npx tsc --noEmit clean; npm run lint clean
- npm run test:unit: 1474 passed / 9 skipped (before final lint-only edit; src-console/takes rerun 25 passed)
- test/commands.registered.test.ts: 5 passed
- check:secrets, test:neuro, e2e: not run. No live app / real router / real NOOP run.

## Design files touched (logic only, no markup/class/copy)
- src-console/takes/adapters.ts (live adapter replaces unwired for 5 methods; takesAdapter() returns it outside sample mode)
- src-console/takes/TakeExtras.tsx, HistorySheet.tsx (one `useTakeLogVersion()` re-render hook line each)
- src-console/hrTake/adapters.ts (`liveHrTake` invokes the two commands through `liveOf`)
- docs/design/HANDOFF-1111t.md (TODO note)
New: takes/takeLog.ts, takes/takeLog.test.ts. Edited my own: amaraTake/generate.ts (optional `log` dep), amaraTake/live.ts.
Rust: new take_log.rs, home_hr_take.rs; noop.rs (3 pub(crate) read helpers); commands/mod.rs; lib.rs.
