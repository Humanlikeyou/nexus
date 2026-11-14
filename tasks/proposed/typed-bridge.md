# Proposal: generated typed Tauri bridge (Stage B)

Status: PROPOSED, nothing implemented. Researched 2026-11-14 (crates.io / docs.rs / GitHub fetched that day). Needs Taisun's OK for new dependencies (hard rule: no installs without OK; build is `--offline`).

## 1. What exists today (measured in this repo)

- Rust: 282 `#[tauri::command]` attributes (`grep -rE '^\s*#\[tauri::command' src-tauri/src`); 276 entries in `generate_handler![` (src-tauri/src/lib.rs:206-519). The 6-command gap is not yet explained (cfg-gated, helpers, or unregistered): first task in commit 1 is to list it.
- Tauri 2.11.6 locked (src-tauri/Cargo.lock); `tauri = { version = "2", features = ["tray-icon","macos-private-api","unstable"] }` (src-tauri/Cargo.toml:16). JS `@tauri-apps/api` 2.11.1.
- TS side: `src-console/bridge.ts` has 71 `invoke<...>(` call sites (it also has a `read(...)` helper and `Channel`, bridge.ts:1).
- Outside bridge.ts: 223 non-test `invoke` call sites in 32 files, plus 11 in tests. Biggest: src/lib/api.ts 102, src/terrance/system/continuity.ts 29, src-console/standalone/liveAdapter.ts 13, src-console/lib/homeAssistant.ts 8, subagents.ts 6, skillsEvolution.ts 6, selfDiagnosis.ts 7, research.ts 4, customJobs.ts 4, telegramBridge.ts 4, takeLog.ts 4, liveSuggest.ts 4, haConfig.ts 4; the rest 1-3 each (rituals, deliberation, roadmapNight, amaraTake/live, takes/events, NoopSettingsGroup, hrTake/adapters, and 1-each in 9 more). Exact list: `grep -rEc 'invoke[<(]' src-console src` (src-next/ no longer exists).
- `src/` modules still reachable from src-console (non-test importers):
  - `src/lib/types` <- src-console/types.ts (5 imports), settings/BackupSection, settings/NoopSettingsGroup, neuro/{training,recordingSummary,goals}, app/amaraChat, amaraTake/live
  - `src/lib/api` <- settings/BackupSection, amaraTake/live (this is the 102-invoke file)
  - `src/lib/llm-router` <- amaraTake/live
  - `src/lib/neurofeedback/*`, `lineNoise`, `artifactDetection`, `electrode/electrodePolicy` <- neuro/training, neuro/useTraining, screens/NeuroFlow
  - `src/hooks/{useFFT,useSignalQuality,useAudioFeedback}` <- neuro/*, screens/NeuroFlow
  - `src/terrance/system/{scheduler,continuity,deliberation,contextCompressor,injectionGuard,selfDiagnosis,approvalErrors}` <- screens/TerranceScreen, hooks/useSchedulerLifecycle, hooks/useIdleAutoSave, app/useConversation, app/amaraChat, App.tsx, lib/approval, settings/BackupSection
  - Transitive (pulled in by the above, not imported directly): rituals, research, subagents, skillsEvolution, customJobs, telegramBridge, roadmapNight, activityBus, events.ts, eegRingBuffer, fft. `node scripts/command-inventory.mjs` gives the authoritative reachable set (esbuild metafile from src-console/main.tsx).
  - Not reachable (candidates to retire, list only): src/context/NexusContext.tsx, src/hooks/{useElectrode,useGoals,useKeyboardShortcuts,useNeurofeedbackSession}.ts and their tests. Confirm with the inventory's `legacyFiles` table.
- Existing drift guard: `scripts/command-inventory.mjs` (145 lines) parses `generate_handler![...]` with string slicing and calls regex `invoke|read<...>("name"` across src-console/src; `test/commands.registered.test.ts` (118 lines) fails if a called name is not registered. It checks NAMES ONLY: no arg names, no arg/return types, and dynamic names (`invoke(variable)`) are invisible.
- Hard Rust cases the tool must handle: `amara_turn(app: AppHandle, registry: State<'_, TurnRegistry>, input: AmaraTurnInput, surface: String, channel: Channel<TurnEvent>) -> Result<String, String>` (amara_turn.rs:1750-1756); 197 `rename_all` uses in src-tauri/src; about 818 lines in src-tauri/src/commands with `u64/i64/usize/u128` in a type position (regex, over-counts; matters for BigInt, see risks).

## 2. Ecosystem today (verified 2026-11-14)

| Option | Latest | Maturity | Tauri 2 | Fits our patterns |
|---|---|---|---|---|
| tauri-specta | 2.0.0-rc.29 (2026-11-13); rc.28 2026-11-11; rc.26 2026-10-21; rc.25 2026-05-08; max stable is 1.0.2 (Tauri 1 only) (https://crates.io/api/v1/crates/tauri-specta) | Release candidate for 2+ years but actively released; API still moves between rcs | Yes. rc.29 deps: `tauri = "2"` with feature `specta`, `specta = "=2.0.0-rc.28"` (exact pin), `specta-typescript 0.0.15`, `specta-serde 0.0.15` (https://docs.rs/crate/tauri-specta/2.0.0-rc.29/source/Cargo.toml). Our tauri 2.11.6 has a `specta` feature (~/.cargo/registry tauri-2.11.6/Cargo.toml:122) | Async + `Result<T,E>` yes (error handling mode, promise rejects by default). `Channel<T>` yes as a direct argument/return only, typed `Channel<T>` (https://docs.rs/tauri-specta/2.0.0-rc.29/tauri_specta/). `rename_all` on commands: configured per command on both attributes. serde on types via specta-serde (separate `_Serialize`/`_Deserialize` aliases when shapes differ). AppHandle/State: not documented on that page, but they are tauri-injected params, not serialised; NOT verified, test in commit 1 |
| specta (+specta-typescript) | specta 2.0.0-rc.28 (2026-11-05); specta-typescript 0.0.15 | rc | n/a (type engine used by tauri-specta/taurpc) | Needed by tauri-specta. Pre-1.0 specta-typescript |
| TauRPC | 0.8.2 (2026-07-03) (https://crates.io/api/v1/crates/taurpc) | 0.x | Built on tauri 2.10.2 + `specta =2.0.0-rc.25` (https://docs.rs/crate/taurpc/0.8.2/source/Cargo.toml). Its pins would clash with a tauri-specta/specta rc.28 | Replaces `#[tauri::command]` with a trait macro: rewrite of all 282 commands. Rejected |
| ts-rs | 12.0.1 (2026-01-31) (https://crates.io/api/v1/crates/ts-rs) | Stable | Tauri-agnostic | Types only, no command names/args. `serde rename_all` supported (https://github.com/Aleph-Alpha/ts-rs); export via generated `cargo test`. Would not stop name/arg drift. Rejected as the main approach |
| Tauri's own | none | | | v2.tauri.app "Calling Rust" says commands are type safe on the Rust side only and shows hand-written TS types; no generator (https://v2.tauri.app/develop/calling-rust/) |

Caveat: tauri-specta's own repo workspace patches tauri to a git fork (raw Cargo.toml, fetched), so rc.29 may rely on unreleased tauri changes for some features. Whether rc.29 compiles against crates.io tauri 2.11.6 is NOT verified (nothing was built).

## 3. Offline availability

`ls ~/.cargo/registry/src/*/ | grep -iE 'specta|ts-rs|taurpc'` returns NOTHING (0 matches). Every candidate would be a download. For tauri-specta that is, from crates.io: `tauri-specta 2.0.0-rc.29`, `tauri-specta-macros 2.0.0-rc.29`, `specta 2.0.0-rc.28`, `specta-macros`, `specta-typescript 0.0.15`, `specta-serde 0.0.15`, `specta-util 0.0.15`, plus small transitive crates (heck 0.5, paste, thiserror 2 if not already present). Exact set is known only after `cargo fetch`/`cargo tree`. No npm packages are needed (the generated file imports only `@tauri-apps/api/core`, already installed). Hard rule 3 (no new outbound hosts) concerns the app at runtime, not crates.io at build time, but this still needs Taisun's OK.

## 4. Recommendation: tauri-specta 2.0.0-rc.29, bindings generated by a Rust test, committed

Dependencies (src-tauri/Cargo.toml):
```
tauri-specta = { version = "=2.0.0-rc.29", features = ["derive", "typescript"] }
specta = { version = "=2.0.0-rc.28", features = ["derive"] }   # same exact pin tauri-specta uses
specta-typescript = "=0.0.15"
tauri = { ..., features = [..., "specta"] }                      # existing line plus the feature
```
Pin exactly: rc releases break APIs. Do not use `^`.

Generation: a `#[test] fn export_bindings()` in src-tauri (runs under the existing `cargo test --manifest-path src-tauri/Cargo.toml`, so no build.rs, no runtime cost, no debug-only export in the app) calls `Builder::<tauri::Wry>::new().commands(collect_commands![...]).export(Typescript::default(), "../src-console/bindings.gen.ts")`. Output is CHECKED IN. Gate: a second test (or `npm test` step) regenerates to a temp path and fails if it differs from the committed file ("bindings stale: run cargo test export_bindings"); vitest/tsc then fail on any call site that no longer type-checks. This keeps the generator out of `build.rs` and works offline once vendored in the registry.

Single source of command list: `collect_commands![...]` must replace or mirror `generate_handler!`. Use `builder.invoke_handler()` as the handler so there is one list. During migration keep `generate_handler!` and add a Rust test asserting both lists are equal.

Coexistence with bridge.ts: bridge.ts keeps its public functions and hand-written types. Migrate one function at a time so its body becomes `commands.name(args)` (generated, returns `Promise<T>` or `Result` per ErrorHandlingMode; choose throw/reject to preserve today's `invoke` behaviour). Generated file is imported ONLY from bridge.ts and the new modules; no component imports it directly. Where today's hand-written TS type differs from the generated one, the compiler reports it, which is the point.

Channel/events: `amara_turn` takes `Channel<TurnEvent>` directly (allowed). `TurnEvent` needs `#[derive(specta::Type)]`; generated binding is `channel: Channel<TurnEvent>` using the `@tauri-apps/api/core` Channel class bridge.ts already imports. Event emits (`app.emit`) stay untyped until a later pass (`collect_events!` needs a typed struct per event); out of scope for this proposal.

Required Rust changes: every command gets `#[specta::specta]`; every arg/return type gets `#[derive(specta::Type)]` (deriving next to `Serialize/Deserialize`; specta-serde honours `rename_all`, `tag`, `flatten`, `default`, `skip`). Mechanical but touches 282 fns and every referenced struct/enum.

Risks:
1. rc status and exact pins; an update is an explicit task. tauri-specta rc.29 is 1 day old (2026-11-13); prefer waiting for rc.30 or proving rc.29 in commit 1.
2. BigInt: specta-typescript refuses `u64/i64/usize/i128` by default (webview would truncate). The grep shows many such fields. Options per field: change to `u32`/`f64`, or `Builder::dangerously_cast_bigints_to_number()` (acceptable if values are timestamps/sizes below 2^53). Decide in commit 1 after counting real hits in command signatures and payload types.
3. Types from other crates or `serde_json::Value`: specta supports `Value`; foreign types need `specta` features or wrapper types.
4. Compile time: derive macros on ~hundreds of types add modest compile time; measure in commit 1 (not measured now).
5. Per-command `rename_all` for argument names must match what the JS side already sends (camelCase by default in Tauri); generated code encodes this, so mismatches surface as diff, not runtime error.
6. Two command lists during migration (see above).

Migration order (each a small local commit on amara-integration; no push):
1. Add deps, `#[specta::specta]` + `Type` on 5 low-risk commands (e.g. kv, onboarding, n8n_status, screen_opens, checkin), export test + staleness test, generated file with those 5 only. Prove compile with `--offline` (needs deps vendored first, see Decision). Also include one State/AppHandle command to resolve that open question.
2. Per-module batches of ~20-30 commands (src-tauri/src/commands/*.rs), each batch regenerating the file; BigInt decisions logged.
3. `amara_turn` + `TurnEvent` (the Channel case) alone in its own commit.
4. Switch bridge.ts functions to generated calls batch by batch (71 sites), then migrate the 223 outside sites: src/lib/api.ts (102) first by routing through bridge.ts, then continuity.ts (29), liveAdapter.ts (13), homeAssistant.ts (8), the rest.
5. Move still-used src/ modules into src-console (listed in section 1) with import rewrites (`@/` alias and `../src/`), one directory per commit; then LIST (do not delete) the unreachable src/ files for Taisun.
6. Retire `scripts/command-inventory.mjs` call-name checks once nothing calls `invoke` outside the generated file (add a vitest/ESLint rule: `invoke(` allowed only in bindings.gen.ts). Keep the inventory script's "live/legacy" report as a backup until src/ is gone, then keep only the registered-vs-bound test.

## 5. Fallback if Taisun declines new dependencies

Extend `scripts/command-inventory.mjs` and the test, no crates:
- Parse each `#[tauri::command] fn name(args) -> ret` in src-tauri/src/commands/*.rs (regex/brace matching; drop `app`, `State`, `AppHandle`, `Window` params; apply `rename_all` camelCase) and emit `src-console/commands.gen.ts` with `export type CommandName = "..." | ...` plus `export const COMMAND_ARGS = { name: ["argA","argB"] }` (argument NAMES only).
- bridge.ts `invoke<T>(name: CommandName, args)` and a vitest that compares object-literal keys at each call site against `COMMAND_ARGS`.
Limits: no argument or return TYPES (T stays hand-written), regex Rust parsing breaks on macros/multiline generics/cfg-gated fns, `rename_all` per command is guessed, and nested type drift is invisible. It catches typos, renamed/removed commands, and missing/extra argument names, which is most real breakage, at roughly a day's work.

## Decision needed from Taisun

Approve (or decline, then fallback in section 5), fetched from crates.io only, pinned exact:
1. `tauri-specta = "=2.0.0-rc.29"` (features `derive`, `typescript`) and its `tauri-specta-macros =2.0.0-rc.29`
2. `specta = "=2.0.0-rc.28"` (feature `derive`)
3. `specta-typescript = "=0.0.15"`, plus transitive `specta-serde`/`specta-util 0.0.15`, `specta-macros`, and any of heck/paste/thiserror not already in Cargo.lock
4. Enabling feature `specta` on the existing `tauri` dependency
5. npm: none.
Also: OK to run one online `cargo fetch` (or vendor them) so the `--offline` build can resolve them.
