# Nexus

Local, private command center — Tauri 2 + React 19 (`src-console/`) + Rust (`src-tauri/`).
Covers Terrence (AI runtime + OpenClaw), Muse EEG sessions, health signals read from NOOP, business/ARM and goals.

> Rename from `electron` to **Nexus** (`com.taisun.nexus`) is planned and partly applied; some internal names still say `electron`.

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
- `docs/` — product verification and Terrence architecture audit
- `stack/` — Docker stack (n8n, Postgres, Letta, Weaviate)

## Layout

`src-console/` live UI · `src/` legacy helpers · `src-tauri/` Rust backend · `config/` Terrence identity & skills · `scripts/` dev helpers · `stack/` services
