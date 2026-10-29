# Build Guide — Nexus

> App: shown as **Amara OS** (`productName`; window title "Amara OS — Command Center"), internally `Nexus` (`com.taisun.nexus`), npm package `nexus`, Rust lib `nexus_lib`. Formerly `electron` (rename done in 80276f9).

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Node.js | 22 (`.nvmrc`; ≥ 22.6 needed for `test:neuro`) | `nvm use` picks it up |
| npm | >= 10 (tested: v11.8.0) | Lockfile is `package-lock.json` — use `npm`, not pnpm |
| Rust | stable (tested: 1.93.0, `stable-aarch64-apple-darwin`) | No `rust-toolchain.toml`; `rustup default stable` |
| Tauri CLI | ^2.10.0 | Installed as a devDependency — invoked via `npm run tauri` |

### Install Rust (if missing)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

### Install Node dependencies
```bash
npm ci
```

On Linux (CI / Ubuntu) you also need Tauri's system dependencies:
```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev \
  patchelf libssl-dev pkg-config
```

---

## Development

```bash
# Start Vite dev server + Tauri window (hot-reload)
npm run tauri -- dev
```

Vite serves at `http://127.0.0.1:1420` (`strictPort: true`) from `index.html` → `src-console/main.tsx`. Rust changes trigger an automatic recompile.

---

## Production Build

Run these in order:

```bash
# 1. Type-check TypeScript (tsconfig includes src-console only)
npx tsc --noEmit

# 2. Build frontend (tsc + vite bundle → dist/)
npm run build

# 3. Build Rust backend only (no bundle)
(cd src-tauri && cargo build --release)

# 4. Full Tauri bundle (dmg + .app on macOS)
npm run tauri -- build
# or, ad-hoc signed for local use:
npm run bundle:local
```

Output lands in `src-tauri/target/release/bundle/`.

---

## Tests & helper scripts

```bash
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests
npm run test:neuro          # src-console/neuro/*.test.mjs (node --experimental-strip-types --test)
npm run mock-gateway        # mock OpenClaw gateway (also :partial, :no-ack, :fail)
npm run outbox-status       # scripts/outbox-status.sh
```

CI (`.github/workflows/ci.yml`) uses Node 22 from `.nvmrc`: `npm ci`, `npx tsc --noEmit`, `npm run test:neuro`, `npm run build`; Rust on Linux (`cargo fmt --check`, `cargo clippy`, `cargo test --locked`) and macOS (`cargo build --locked`, `cargo test --locked`). `release.yml` builds a draft macOS release on `v*` tags.

## Local runtime

- **LLM:** the default is local Ollama with `gemma4:latest` (`ollama pull gemma4`). Set `compute_mode` to `local`, because `gateway` routes LLM jobs to OpenClaw on :18789.
- **QA launches:** `NEXUS_OUTBOUND_DISABLED=1 npm run tauri -- dev` (also `true`/`yes`) blocks Telegram sends and outbox delivery for that session. Without it, a launch with `telegram_chat_id` configured sends real messages.
- **Outbox:** pending items older than 24h are marked `expired` and are never sent.

## App data & backups

App data lives in `~/Library/Application Support/com.taisun.nexus`. On first launch after the rename, Nexus copies `~/Library/Application Support/com.taisun.electron` into it once (marker file; never deletes or overwrites existing files). The old directory is left in place.

Backups (`pack_it_up`) are written to `~/Desktop/nexus-backup-<timestamp>.tar.gz` (staging dir `nexus-pack-*`, config in `nexus-config`). Restore also accepts legacy `electron-pack*` / `electron-config` backups.

Activity log: `activity_log/` keeps 30 days (max 5000 files). The prune job (every 7 days) archives older entries into `activity_log/archive/YYYY-MM.jsonl.gz` after verifying them, so expect a one-time archive of about 16.6k files on the first prune after updating.

---

## Troubleshooting

### `Command plugin:event|listen not allowed by ACL`
Tauri 2.x enforces capability declarations. Check `src-tauri/capabilities/default.json` and ensure the required permission string is listed (e.g. `core:event:allow-listen`). See [Tauri capability docs](https://v2.tauri.app/security/capabilities/).

### `port already in use` (1420)
Another Vite instance is running (`strictPort` means Vite will not fall back to another port). Kill it:
```bash
lsof -ti :1420 | xargs kill -9
```

### `cargo could not find Cargo.toml`
Cargo must be run from inside `src-tauri/`, not the repo root:
```bash
# Wrong
cargo build

# Correct
(cd src-tauri && cargo build)
# or
cargo build --manifest-path src-tauri/Cargo.toml
```

### `npm run tauri -- build` fails on macOS with signing errors
Add `--no-bundle` to skip bundling/code-signing for local testing (produces only the binary in `src-tauri/target/release/`):
```bash
npm run tauri -- build --no-bundle
```
