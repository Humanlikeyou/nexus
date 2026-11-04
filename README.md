# Nexus (Amara OS)

Local, private command center — Tauri 2 + React 19 (`src-console/`) + Rust (`src-tauri/`).
The app is shown to users as **Amara OS** (assistant: Amara); the repo and internal ids remain `nexus`.
Covers Amara (one assistant identity; chat through Hermes, models picked by the local Amara Router on 127.0.0.1:8787: Gemma fast, Qwen deep, Claude only if Settings › Assistant › Allow Claude is on), the Voice panel (Chat & Agents › Voice), the Mind-Body page, Muse EEG sessions, health signals read from NOOP, business/ARM and goals.

> Formerly named `electron`; the rename to **Nexus** (`com.taisun.nexus`, Rust lib `nexus_lib`) is complete. Existing app data in `com.taisun.electron` is copied over once on first launch, and old `electron-*` backups still restore.

## Quick start

```bash
npm ci
npm run tauri -- dev        # Vite on http://127.0.0.1:1420 + Tauri window
NEXUS_OUTBOUND_DISABLED=1 npm run tauri -- dev   # QA launch: no Telegram/outbox sends
npm test                    # gates: check:secrets, tsc, vitest, test:neuro, cargo test
```

## Docs

- [BUILD.md](BUILD.md) — prerequisites, build, troubleshooting
- [CLAUDE.md](CLAUDE.md) — architecture and conventions
- [STAGES.md](STAGES.md) — roadmap
- `docs/` — product verification and Amara architecture audit
- [amara-router/README.md](amara-router/README.md) — Amara Router install and Hermes setup; [TAISUN-STEPS.md](TAISUN-STEPS.md) — manual steps to switch over
- `stack/` — Docker stack (n8n, Postgres, Letta, Weaviate, Verba, crew service); KB ingest plan in `stack/kb/INGEST-PLAN.md`

## Layout

`src-console/` live UI · `src/` legacy helpers · `src-tauri/` Rust backend · `config/` Amara identity & skills · `amara-router/` model router service · `scripts/` dev helpers · `stack/` services
