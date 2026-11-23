# Stage F / Phase 9: bundled runtime, models, first run (build plan, read-only research, 2026-11-15)

Sources: HANDOVER-FINAL Phase 9 (`tasks/HANDOVER-FINAL-2026-11-12.md:358-368`), `docs/design/HANDOFF-1111s.md:28`, `src-console/standalone/{FirstRun.tsx,adapters.ts,liveAdapter.ts}`, `tauri.conf.json`, `system_profile.rs`, `keychain_migrate.rs`, `supervisor/{registry,runner}.rs`, amara-router config and src. `standalone-app-plan.md` does not exist.

## 0. Findings
1. Missing commands: `model_tiers`, `model_download`, `setup_done`, `setup_reset`, `services_start_all`. Present: `system_profile`, `keychain_import_review`. `onboarding.rs` (`onboarding.json`) is an older, separate flow: don't reuse it.
2. The router speaks Ollama native only (`/api/chat`, `/api/generate`, `/api/ps`, `/api/version`; keep_alive, think, num_ctx). llama-server would need a new router backend: deferred.
3. On this Mac `/usr/local/bin/ollama` → `/Applications/Ollama.app/Contents/Resources/ollama`; router `ollama_url` defaults to `127.0.0.1:11434`.
4. Installed models: `gemma4:latest` 9.6 GB, `gemma4-12b-uncensored:latest` 7.6 GB, `qwen3.8-uncensored:latest` 16 GB, `nomic-embed-text:latest` 274 MB. Live fast/deep = `gemma4:latest` / `qwen3.8-uncensored:latest` [GATE: keep].
5. `ring 0.17` and `sha2 0.10` are already direct deps: Ed25519 verify with no new crate.
6. No `externalBin`, no sidecar; Tauri needs every externalBin file at build time (`<name>-<triple>`), so entries stay out until binaries exist.
7. Supervisor has `model-runtime` and `amara-router` rows; act stays off.
8. Voice is Python (faster_whisper): a sidecar needs a freeze (Q3). Not built here.
9. Health sync reader is Phase 10's binary: slot reserved only.
10. Model download = a new outbound host [GATE].
11. FirstRun: `App.tsx:119-120` opens it when `setupDone()` is live and false; `liveAdapter.ts` wires only `profile`; buttons are inert.

## 2. Runtime choice
Recommended B: bundle the `ollama` binary as `externalBin`, `ollama serve` on 127.0.0.1:11434 only, adopt-first (if :11434 answers, start nothing: that is the Ollama.app fallback). Storage: existing `~/.ollama/models`; `OLLAMA_MODELS=<data_dir>/models` only for fresh installs. Binary: Taisun step (copy from Ollama.app or supply with version + SHA-256, logged in docs/decisions). Signed in Phase 14.

## 3. Sidecars
`binaries/amara-router` (buildable), `binaries/ollama` (Taisun), `binaries/amara-voice` (reserved, Q3), `binaries/noop-local-access` (Phase 10). Paths from `resource_dir()`/`current_exe()` and `app_paths::data_dir()`, no home path compiled in (grep test). Bundled router config `<data_dir>/router/config.toml`, never overwrites `~/.config/amara-router`. Supervisor: `Runtime::Sidecar`, `Verb::SidecarStart(id)` refused by `ActOffRunner`; probe before spawn (up = adopt; held but unhealthy = needs_you; spawn only on connection refused; loopback bind). Spawn via `tokio::process` in Rust; no shell capability.

## 4. Manifest, tiers, download
Manifest JSON `{schema:1, issued, tiers:[{id,name,minRamGb,models:[{role,name,bytes,sha256,url}]}]}` + detached Ed25519 signature, verified with `ring` against a compiled-in public key; fallback: SHA-256 pinned manifest constant (Q2). Ships inside the app; no remote fetch. Live names only. `model_tiers()` by `system_profile` RAM: available, recommended = highest available; `manifest_invalid` on any verify failure (never unverified data).
`model_download(tier)`: click-only; refused `outbound_blocked` when `telegram::outbound_disabled()` (env + navy); native approval naming host + size; host allow-list in the module [GATE]; resumable `.partial/<sha256>` with Range, full SHA-256 verify, atomic rename, `safe_join`; progress event `model://progress {role,state,pct,bytesDone,bytesTotal}` with no URL. Ollama store compatibility: Q1 (`/api/pull` vs blob layout). Tests: `NEXUS_OUTBOUND_DISABLED=1`, local 127.0.0.1 fixture server, host list injected in tests only.

## 5. Setup state, start-all, keychain
`setup_done()` from `<data_dir>/setup.json` (0600, atomic); `setup_complete(tier)` (extra command, needed to write the flag); `setup_reset()` deletes only `setup.json`. `services_start_all()`: with act off returns `skipped_act_off`, spawns nothing; with act on, one approval, sidecars only (runtime, router), never Docker/LaunchAgents; per-service `{id, outcome: adopted|started|skipped|failed}`. `keychain_import_review` reused read-only. FirstRun wiring logic-only (adapter calls from existing buttons; copy/markup → design-questions).

## 6. Parity check (Taisun live)
Quit Ollama.app; launch with the bundled runtime; check `/api/version`, model list, router health; chat light and deep; voice turn; one crew run (record live-compose direct vs router); KB query; cold-load times and RAM; reopen Ollama.app and confirm adopt, no double bind.

## 7. Lanes
F1 manifest + tiers (`model_manifest.rs`, `model_tiers.rs`, resources) · F2 download (`model_download.rs`, after F1) · F3 setup state (`setup_state.rs`) · F4 sidecar verbs + `services_start_all.rs` (registry/runner; externalBin entries only after the binary exists) · F5 TS wiring + all shared registration (`mod.rs`, `lib.rs`, `bindings.rs`, `bindings.gen.ts`, `boundTable.ts`, `bridge.ts`, `commands.registered.test.ts`) after F1-F4. F1-F4 hand over registration snippets and don't edit shared files.

## 8. Gates, Taisun steps, open questions
[GATE] light-tier name; download host; the ollama binary; running sidecars / flipping act; dropping Ollama.app and retiring LaunchAgents; Phase 14 signing.
Open: Q1 store (/api/pull vs blobs); Q2 Ed25519 key custody vs pinned hash; Q3 voice freeze + Whisper models; Q4 click alone vs outbound setting + approval; Q5 Intel?; Q6 first run on the existing Mac; Q7 crew parity before the router move.
