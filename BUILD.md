# Build Guide — Nexus

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Node.js | >= 20 (tested: v25.6.0) | No `.nvmrc`; use latest LTS or current |
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

Vite serves at `http://localhost:1420`. Rust changes trigger an automatic recompile.

---

## Production Build

Run these in order:

```bash
# 1. Type-check TypeScript
npx tsc --noEmit

# 2. Build frontend (tsc + vite bundle → dist/)
npm run build

# 3. Build Rust backend only (no bundle)
(cd src-tauri && cargo build --release)

# 4. Full Tauri bundle (dmg + .app on macOS)
npm run tauri -- build
```

Output lands in `src-tauri/target/release/bundle/`.

---

## Troubleshooting

### `Command plugin:event|listen not allowed by ACL`
Tauri 2.x enforces capability declarations. Check `src-tauri/capabilities/default.json` and ensure the required permission string is listed (e.g. `core:event:allow-listen`). See [Tauri capability docs](https://v2.tauri.app/security/capabilities/).

### `port already in use` (1420)
Another Vite instance is running. Kill it:
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
Add `--no-bundle` to skip code-signing for local testing:
```bash
npm run tauri -- build --no-bundle
```
