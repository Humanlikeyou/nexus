# Nexus

Local, private command center — Tauri 2 + React 19 (`src-console/`) + Rust (`src-tauri/`).
Covers Amara (AI runtime + OpenClaw), Muse EEG sessions, health signals read from NOOP, business/ARM and goals.

> Formerly named `electron`; the rename to **Nexus** (`com.taisun.nexus`, Rust lib `nexus_lib`) is complete. Existing app data in `com.taisun.electron` is copied over once on first launch, and old `electron-*` backups still restore.

## Quick start

```bash
npm ci
npm run tauri -- dev        # Vite on http://127.0.0.1:1420 + Tauri window
npm run test:neuro          # frontend neuro tests
cargo test --manifest-path src-tauri/Cargo.toml
```

## Docs

- [BUILD.md](BUILD.md) — prerequisites, build, troubleshooting
- [CLAUDE.md](CLAUDE.md) — architecture and conventions
- [STAGES.md](STAGES.md) — roadmap
- `docs/` — product verification and Amara architecture audit
- `stack/` — Docker stack (n8n, Postgres, Letta, Weaviate)

## Layout

`src-console/` live UI · `src/` legacy helpers · `src-tauri/` Rust backend · `config/` Amara identity & skills · `scripts/` dev helpers · `stack/` services
