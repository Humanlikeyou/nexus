# Amara OS (repo `nexus`): brief for Claude coders

**Read first, they override this file:** `.claude/rules/hard-rules.md`, `design-boundary.md`, `muse-recordings.md`, `git-and-commits.md`, `verification.md`. Decisions are in `docs/decisions/` (0001–0016). Current work is in `tasks/BOARD.md`, and the latest status is in `tasks/HANDOFF-FOR-REVIEW.md`.

In short:
- Commit locally on `amara-integration` in `~/projects/nexus-amara`, by explicit path. No push, merge or PR (0001).
- No secrets.
- Run with `NEXUS_OUTBOUND_DISABLED=1`, and add no new outbound hosts.
- Design owns UI markup, CSS and tokens.
- Muse recordings are never deleted.

## What it is
Amara OS is a local, private Tauri 2 command center for Taisun. User-facing names are Amara OS (the app) and Amara (the assistant). Internal ids stay `nexus`, `nexus_lib`, `com.taisun.nexus`, plus the old `terrence*`/`terrance*` command and module names (they are the wire contract; new code uses `amara_*`).

Nav (⌘1–7): Home, Life, Mind–Body, Enterprise, Agents · System: Smart home, Settings (0009).

## Layout
| Path | What |
|---|---|
| `src-console/` | Live UI: React 19 + TS + Vite, plain CSS. `App.tsx` shell, `app/routes.ts` nav, per-area folders with `adapters.ts` (unwired/sample/live) and `live*.ts` |
| `src/` | Legacy, but ~10 `src-console` files still import from it (e.g. `src/terrance/system/contextCompressor.ts`). Don't add to it |
| `src-tauri/src/` | Rust backend. `lib.rs` registers ~248 commands; `commands/*.rs` has one module per area |
| `amara-router/` | Separate Rust service on 127.0.0.1:8787 that picks the model tier (fast `gemma4:latest`, deep `qwen3.8-uncensored`, Claude off by default). Config is in `~/.config/amara-router/config.toml` |
| `stack/` | Docker compose for n8n, Postgres, Letta 0.16.8, Weaviate/Verba and crew-service. **The live stack runs from `~/projects/nexus/stack`** (its `.env` is there; agents never read it) |
| `test/e2e/` | Playwright against a mocked Tauri (`fixtures/tauri-mock.ts`) |

## Runtime facts
- **Chat today:** app → Hermes `127.0.0.1:8642` (OpenAI API, key from `~/.hermes/.env`, never logged) → router → Ollama. No streaming, and Hermes timeout is 30 min. Decision 0016 changes the default to router-direct with Hermes only for tools; not built yet.
- **Navy mode:** a session-only local-only switch (`telegram::set_local_only`). Chat goes router-only with no tools, and outbound commands refuse. The voice loop is covered only after `tasks/proposed/amara-voice-navy-gate.diff` is applied.
- **Identity:** every tier gets `~/.hermes/SOUL.md` + `IDENTITY_GUARD`. Replies name the tier, never a model.
- **Voice:** `~/projects/amara-voice` is a separate process with its own LaunchAgent, and it calls Hermes itself. It's live and not under git, so **never edit it**. Propose patches instead.
- **Health:** read-only from NOOP's `whoop.sqlite` via a sandboxed CLI (`noop.rs`, `noop_query.rs`). Strain is 0–100.
- **Letta** (127.0.0.1:8283, Bearer from Keychain `com.taisun.nexus.letta_server_password`): `memory_list` reads; edit, clear and undo need a native approval and a local backup (0015). A `pg_dump` of `letta_db` comes before the first real delete.
- **Money:** read-only, and nothing moves money (0008). Yen gets no USD without a stored rate, and "¥" is never rendered.
- **Secrets** live in the Keychain only (`com.taisun.nexus.*`). Commands return set/not set.
- **App data:** `~/Library/Application Support/com.taisun.nexus/` (0700/0600 for new writes).

## Commands
```bash
npx tsc --noEmit && npm run lint && npx vitest run && npm run test:neuro && npm run check:secrets
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --offline --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --offline        # also amara-router/Cargo.toml
npx vite build && NEXUS_OUTBOUND_DISABLED=1 npm run test:e2e     # ~4 min; run only ONE playwright at a time
APPLE_SIGNING_IDENTITY=- npx tauri build --bundles app           # install only when Taisun asks
```
Use `--offline` for cargo, and add no new crates or npm packages. Commit messages go in a file (`git commit -F`). A hook blocks `--no-edit`, `-n` and any mention of push in a commit command.

## Adding a command
1. `#[tauri::command] pub async fn x(...) -> Result<T, String>` in `src-tauri/src/commands/<area>.rs`. Return fixed error codes only, never echo input or secrets, and use `spawn_blocking` for file or sqlite I/O.
2. `pub mod` in `commands/mod.rs` plus one line in `generate_handler!` (`lib.rs`). `test/commands.registered.test.ts` checks registration.
3. Call it from the area's live adapter (`invoke`). If there's no real data, keep it `UNWIRED` ("Needs wiring"). Sample data only behind `nexus.sampleAdapters=1`.
4. Add a test that fails before and passes after. Security-review anything that writes, reaches the network or touches approvals.

## n8n code nodes
No `fetch` or `process`: use `helpers.httpRequest({...})`.

History (the old UI, OpenClaw, session logs) is in `docs/history/CLAUDE-history-2026-11.md`. Move it there when this file is swapped in.
