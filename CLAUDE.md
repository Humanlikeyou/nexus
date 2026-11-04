# Nexus — Project Brief for Claude Coders

> **Central Goal**: Nexus is a local, private command center — a Tauri 2 desktop app combining Amara (AI agent runtime; local Ollama; the OpenClaw gateway mode was removed in 3a663eb/e183ad6), neuro sessions (Muse EEG), health signals (read-only from NOOP), business/ARM data, and goals, with n8n automation behind it.

> **Naming (Decision 13, ec12efa / de94dfa):** the user-facing app is **Amara OS** (`productName`), the window and page title is "Amara OS — Command Center", and the assistant is **Amara**. Internal ids are unchanged: repo/npm/Cargo `nexus`, `nexus_lib`, `com.taisun.nexus`, Tauri command names, `terrance*` module ids, tables and file names.

> **Rename: done** (80276f9, f30acbb). `electron` → **Nexus**: npm package `nexus`, Cargo package `nexus`, Rust lib `nexus_lib` (`main.rs` calls `nexus_lib::run()`), `productName` `Nexus`, identifier `com.taisun.nexus`, favicon `public/nexus.svg`. Backups are `nexus-backup-*.tar.gz` (staging `nexus-pack-*`, config dir `nexus-config`); older `electron-pack`/`electron-config` backups still unpack. App data moves from `com.taisun.electron` to `com.taisun.nexus` via a one-time, non-destructive copy on first launch (`src-tauri/src/app_paths.rs`).

---

## Architecture Overview

```
nexus/
├── index.html                  # Loads /src-console/main.tsx (title "Amara OS — Command Center")
├── src-console/                # LIVE UI (React 19 + TS, plain CSS, lucide-react icons)
│   ├── App.tsx                 # Shell + navigation: Overview, Live, Sessions, Health, Trajectory,
│   │                           #   Business, Revenue, Devices, Activity, Settings
│   ├── bridge.ts               # Tauri invoke wrappers used by the console
│   ├── types.ts
│   ├── styles.css              # Console design system
│   ├── design/                 # ProductUI, SystemPrimitives, useAppearance
│   └── neuro/                  # fft, training, sessionFinalizer, useMuseTelemetry (+ *.test.mjs)
├── src/                        # LEGACY — not type-checked (tsconfig includes only src-console)
│   ├── context/, hooks/, lib/  # Remaining helpers (neurofeedback, electrode, llm-router, api.ts)
│   └── terrance/system/        # continuity.ts, rituals.ts, ... (openclawHealth.ts archived in 3154f28)
│                               # (App.tsx, main.tsx, components/*, styles/globals.css,
│                               #  whoop hooks/lib are deleted; empty dirs remain)
├── src-tauri/
│   ├── src/lib.rs              # Builder + generate_handler! (~136 commands)
│   ├── src/main.rs             # calls nexus_lib::run()
│   └── src/commands/           # 28 modules in mod.rs, incl.:
│       ├── noop.rs             # Read-only bridge to NOOP's local SQLite health store
│       ├── crew.rs             # CrewAI service client (http://127.0.0.1:8400)
│       ├── terrence_*.rs       # actions, automations, context, priorities, projects,
│       │                       #   runtime, runtime_bridge
│       ├── openclaw.rs, muse.rs, arm.rs, sessions.rs, telegram.rs, continuity.rs,
│       └── llm_router.rs, research.rs, kv.rs, goals, snapshots, replay, packager, outbox, ...
├── config/                     # terrance.identity.md, terrance.profile.md, skills/
├── scripts/                    # mock-gateway.mjs, outbox-status.sh
├── stack/                      # Docker stack; n8n workflows in integrations/n8n/
└── docs/                       # NEXUS_PRODUCT_VERIFICATION.md, TERRENCE_ARCHITECTURE_AUDIT.md
```

There is **no Whoop integration** any more: `whoop.rs`, `useWhoop*`, `src/lib/whoop/` and the fitness components are deleted. Health data is read from NOOP's database (`OpenWhoop/whoop.sqlite` under Application Support) by `noop.rs`; Nexus does not pair with the strap or compute metrics.
`commands/notifications.rs` still exists on disk but is no longer declared in `mod.rs`.

### Tech Stack
| Layer | Technology |
|-------|-----------|
| Desktop shell | Tauri 2 (Rust backend + WebView frontend) |
| Frontend | React 19, TypeScript, Vite 6, plain CSS (`src-console/styles.css`), lucide-react |
| Rust backend | Tauri commands, `tokio`, `reqwest`, `rusqlite`, `dirs`, `serde_json` |
| Automation | n8n (self-hosted, Docker stack) |
| LLM | Amara Router (`amara-router/`, LaunchAgent `com.taisun.amara-router`, `127.0.0.1:8787`) in front of local Ollama: fast `gemma4-12b-uncensored`, deep `qwen3.8-uncensored`, optional Claude. Nexus default compute mode `amara` |
| AI gateway | OpenClaw (Node.js, port 18789), **not used by the router**: compute mode `gateway` was removed (3a663eb, e183ad6), and a saved `gateway` maps to local Ollama. Status/health/restart code (`openclaw.rs`, `restart_services`) and OpenClaw UI status remain |
| Biometrics | Muse EEG via BLE; health via NOOP (read-only SQLite) |
| Agents | CrewAI service (`stack/crew-service`, Docker, 127.0.0.1:8400, local Ollama gemma4) via `crew.rs`; UI in Chat & Agents › Crews |
| Trading | ARM system via local HTTP |
| Notifications | Telegram Bot API |

Removed from deps: Tailwind, Radix UI, ECharts, framer-motion.

---

## Development Commands

Run from the repo root.

```bash
npm run tauri -- dev          # Vite (127.0.0.1:1420, strictPort) + Tauri window
npm run dev                   # Frontend only
npm test                      # Gates: check:secrets, tsc, vitest, test:neuro, cargo test
npm run check:secrets         # Tracked files only: no .env (except .env.example/.env.test), no key patterns; prints file:line + pattern, never the value. Fake fixtures go in .secrets-allow
npx tsc --noEmit              # Type-check (src-console only)
npm run test:neuro            # node --test src-console/neuro/*.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build        # Production bundle
npm run bundle:local          # Ad-hoc signed local bundle (APPLE_SIGNING_IDENTITY=-)
npm run mock-gateway          # also :partial, :no-ack, :fail
npm run outbox-status
```

---

## Runtime behaviour (2026-10-29)

- **Amara Router** (1c769c3, ce2b612, 6d32a9c): `amara-router/` is a separate Rust service on `127.0.0.1:8787` (OpenAI + Ollama APIs; optional LAN listener `lan_bind`, e.g. `:8788`, with a bearer key). It is the only place that picks a model: **fast** (resident, `gemma4-12b-uncensored:latest`) or **deep** (on demand, `qwen3.8-uncensored:latest`), via slash commands, `X-Amara-*` headers, trigger phrases, a token threshold, then an EASY/HARD classifier. Model names live only in its `config.toml` (`~/.config/amara-router/config.toml`, 0600; template `config.example.toml`). It runs as LaunchAgent `com.taisun.amara-router` (`amara-router/launchd/`), logs to `~/Library/Logs/amara-router/router.log` and `routes.jsonl` (no content). Hermes and Nexus point at it; Home Assistant does not (it uses its built-in matcher, no LLM); install and pointing steps are in `amara-router/README.md`, and Taisun's manual steps in `TAISUN-STEPS.md`.
- **Nexus `llm_router.rs` is a thin client** (1c769c3): default compute mode `amara` (router URL `amara_router_url`, loopback only, default `http://127.0.0.1:8787`). `local` and a saved `gateway` behave like `amara`; `hybrid`/`api` ask the router for Claude. `DEFAULT_OLLAMA_MODEL = "gemma4:latest"` is still the Telegram chat model: Telegram is **gemma4-only** (576e5a2) with no fallback, and fails cleanly with a logged reason.
- **Amara tiers** (caaa71c, 97441b7, 1c769c3): Gemma (fast, Nexus calls it `light`), Qwen (deep) and Claude. Amara is **one identity across all tiers**: every call gets `~/.hermes/SOUL.md` plus `IDENTITY_GUARD` ("You are Amara, and only Amara ... never name a model or AI company as yourself"), and responses and usage carry the tier only (`light|deep|claude`), never a model or provider name. **Claude** needs all of: Settings › Assistant › **Allow Claude** on (saved `allow_claude`; off by default; the row says "This sends data off the Mac"), `NEXUS_OUTBOUND_DISABLED` unset, mode api/hybrid and the daily cap not spent (else Nexus sends `X-Amara-Allow-Claude: 0`), plus the router's own `[cloud] anthropic_enabled = true` and `ANTHROPIC_API_KEY` in its 0600 secrets file (off by default). Fallback claude→deep→fast, never to the network. Callers: the renderer scheduler (`continuity.ts`, `research.ts`), Chat's `/deliberate` and the context compressor (below).
- **Amara Chat → Hermes over HTTP** (`terrence_runtime_bridge.rs`). The old bridge spawned `python -m hermes_cli.nexus_runtime`, which only existed in Hermes fork commit 3683f75c07, so every Chat send failed. Chat now POSTs to Hermes's OpenAI API at `127.0.0.1:8642` (`API_SERVER_PORT` may move it) with `Authorization: Bearer <API_SERVER_KEY>` read from `~/.hermes/.env` on each send (never logged) and `X-Hermes-Session-Key: nexus-chat` (`NEXUS_HERMES_SESSION_KEY` overrides it for tests). The full visible history is sent in `messages`. Hermes (`SOUL.md`) is the only identity: Nexus's system message is policy + context only and is fixed per conversation (Hermes hashes it into its session id). Context comes from `prepare_amara_turn` and passes `injectionGuard.ts` in the renderer, and long histories pass `contextCompressor.ts` (restored from the archive; summarized through the router in chunks below its deep threshold, then guarded). Actions use the inline `<nexus_action>` protocol only. `/deliberate <topic>` runs `deliberation.ts` through `chat_with_router` with `deep: true` and is saved with `record_amara_interaction`.
- **Fast/Deep badge**: shown only when the router decided the tier. The Hermes route patch is applied (`~/projects/hermes-patches/`; revert it before `hermes update`), so Hermes's reply carries the router's `X-Amara-Route`, `X-Amara-Model` and `X-Amara-Route-Reason` headers, the real `model`, and `amara` metadata. `route_info` in `terrence_runtime_bridge.rs` reads those headers into `interaction.route`, and `routeInfoFrom` in `routeInfo.ts` uses that first. A reply with no route falls back to `interaction.tier`: `amara` metadata, then the router's `routes.jsonl` lines in the turn window, then a `/amara/route` dry run. The last two run only when Hermes's `model.base_url` is the router (`routedViaRouter` in runtime health). Otherwise there is no badge and the composer's Deep toggle is disabled. The badge has not been checked in the app with the patch applied. How to point Hermes at the router: `amara-router/README.md`.
- **Chat bridge** (6206c38): the broken `hermes_cli.nexus_runtime` bridge noted in 84e57f1 is fixed (see above). Chat is routed through Hermes, and Hermes reaches the router at `http://127.0.0.1:8787/v1` (`model.base_url`, applied 2026-11-04). `context_length` and `ollama_num_ctx` are 65536 and must equal the router's `fast_num_ctx`: Hermes refuses a window under 64,000, and Ollama silently cuts longer prompts.
- **Voice panel** (6399560): Chat & Agents › Voice drives Amara Voice (`~/projects/amara-voice`, its own process): `commands/voice.rs` (`voice_status`, `voice_start`/`voice_stop`, `voice_recent_turns` from `logs/turns.jsonl`, `voice_list_voices`/`voice_set_voice`). Shows live state, start/stop, voice picker, recent turns with route badges and latencies, and an inactive state when amara-voice isn't installed.
- **Voice bridge** (f7eea69, f5a2e25): `commands/voice_bridge.rs` emits `amara-voice://state` (from `logs/state` and the process check, on change plus a 5 s heartbeat) and `amara-voice://level` (from `ws://127.0.0.1:8791/level`, published by amara-voice's `level_feed.py`). `voice_bridge_start`/`voice_bridge_stop` are ref-counted by `useAmaraVoiceBridge` in the Voice panel and the live Amara lens. Ports: `AMARA_LEVEL_PORT` (amara-voice) and `NEXUS_AMARA_VOICE_LEVEL_PORT` (Nexus) must match; `AMARA_LEVEL_FEED=0` turns the feed off. Steps: `TAISUN-STEPS.md`.
- **Mind-Body page** (f835631): `src-console/screens/MindBodyScreen.tsx` reads NOOP data through `commands/noop_query.rs` (`noop_query`, `get_noop_settings`, `set_noop_settings`). QA on live NOOP data is still open, so Stage 5½ stays open.
- **`NEXUS_OUTBOUND_DISABLED`** (14cdf7c): set to `1`/`true`/`yes`/`on` (trimmed, any case) at launch to stop all outbound sends for that session. `send_telegram_message` refuses, the outbox pauses delivery to the gateway (`Delivery paused`), and Activity shows "delivery paused". Reads and health checks still work. Use it for QA launches, because a normal launch with `telegram_chat_id` set **will** send.
- **Background throttling off**: the main window sets `"backgroundThrottling": "disabled"` (macOS 14+). The scheduler (daily summary, prune, rituals) runs in the renderer, and WebKit used to suspend its timers when the window was hidden or the screen was off (live check 2026-10-29: lid closed, scheduler frozen for 10+ min). Guarded by `test/config/tauriConf.test.ts`.
- **`suppressed` status** (8041224): an activity outcome meaning "deliberately not sent" (for example a ritual refused by the `NEXUS_OUTBOUND_DISABLED` kill switch). It is neutral, neither success nor failure, and shows as "Suppressed" in Activity. `terrence_automations.rs` also uses `suppressed` as a run and delivery status.
- **Outbox expiry** (46fa421): pending outbox items older than `OUTBOX_EXPIRY_HOURS = 24` are marked `expired`. They are kept, never sent and not deleted, and the delivery card shows "Expired".
- **Activity-log retention** (6a076f8, be271f5): `archive_activity_log` keeps 30 days (`ACTIVITY_KEEP_DAYS`) as files, capped at 5000 files (`ACTIVITY_MAX_FILES`). Older entries move into `activity_log/archive/YYYY-MM.jsonl.gz` (UTC month). Each archive is written via a temp file and verified before the sources are removed, and unparsable files are left in place. It runs from `pruneMemory()` in the prune job (every 7 days) and failures are non-fatal. `list_activity_entries` reads only the newest files.
- **OpenClaw is optional** (71bf687, 0a23bcd, 4c55ba7, 514de9b, 95bd46a, da68b7d). The router never calls it: gateway mode was removed (3a663eb Rust, e183ad6 frontend) and a saved or forced `gateway` routes to local Ollama. Self-diagnosis never flags or restarts a down OpenClaw (e183ad6). When OpenClaw isn't installed (no plist or binary: nothing is spawned; plist present but no launchd job: still not installed), `read_openclaw_status` returns `not_installed`, `get_openclaw_health` returns `installed:false`, and `restart_services`/`restart_openclaw` return `unavailable: openclaw not installed` with no dialog.  and `pack_it_up` records `openclaw not installed` in `skipped`. The UI shows a neutral "Assistant" badge on Home and Chat only while the gateway is reachable, and `jobFailureReason` maps OpenClaw errors to "Assistant backend not reachable".
- **Prune fix** (c90435a, be271f5): from 2026-07-06 the weekly prune never ran. `runReasoningLoop` swallowed errors, so reasoning logged "Skipped" with no backoff and held the single heavy-job slot every tick. Reasoning and prune failures now count as failures with backoff and a reason (`scheduler.pruneStarvation.test.ts`). The first prune after launch archives about 16.6k old activity files.

## Services & Infrastructure

| Service | Location | Notes |
|---------|----------|-------|
| OpenClaw AI gateway (optional) | `127.0.0.1:18789` | Not used by the router (gateway mode removed). Installed = LaunchAgent plist `~/Library/LaunchAgents/ai.openclaw.gateway.plist` + binary (`/opt/homebrew/bin/openclaw` or `/usr/local/bin/openclaw`) + a loaded launchd job. On this Mac it is currently shut down (job not loaded), so status reads `not_installed` |
| n8n automation | Docker: `stack-n8n-1` | Backed by `stack-postgres-1`, db `n8n_db`, user `terrance` |
| Postgres | Docker: `stack-postgres-1` | `psql -U terrance -d n8n_db` |
| Amara integrations | `stack/integrations/` (in this repo) | n8n workflow JSON files in `stack/integrations/n8n/` |

### OpenClaw Health Check Logic (Rust)
`src-tauri/src/commands/openclaw.rs` — `check_memory_search()`:
1. Reads `~/.openclaw/openclaw.json` — looks for `modelPath` or `localModel` keys
2. **If absent**, probes `~/.node-llama-cpp/models/` for any `.gguf` file as fallback
3. Returns `"operational"` if found, `"missing"` if not

This was needed because `openclaw memory status --deep` downloads the model but doesn't write `modelPath` to the config file.

---

## n8n Workflows

### openclaw_health_monitor.json
- **Location**: `stack/integrations/n8n/openclaw_health_monitor.json`
- **Workflow ID**: `WxCNKfPXqcn6YY4Q` (the old `tAKyEzsax7BoTM7t` no longer exists in n8n)
- **Trigger**: Webhook POST. Its old sender, `pushHealthToN8n()` in `openclawHealth.ts`, is archived (3154f28), so nothing in the app calls this webhook now
- **Flow**: webhook → extract status → switch (healthy/degraded/offline) → 3 conditional branches → Telegram alert → respond
- **Telegram credentials**: Bot token and Chat ID are hardcoded in the Code node (real values set Feb 2026)
- **Status**: Inactive (checked 2026-10-30). Keep it off: the "Send Telegram Alert" Code node calls api.telegram.org directly and ignores `NEXUS_OUTBOUND_DISABLED`.

### Ingress workflow (`stack/n8n/workflows/eYdwlwZKTMV89z2J.json`)
- General Amara AI agent ingress workflow. The only export; `stack/integrations/n8n/terrance_ingress_workflow.json` is a pointer stub (tests: `INGRESS_WORKFLOW_PATH` in `test/n8n/workflowPath.ts`)
- **Live workflow ID**: `eYdwlwZKTMV89z2J` ("Terrance Ingress Gateway"), the only active workflow (checked 2026-10-30 with `n8n list:workflow --active=true`). `sMtd9mnMhXid8MQn` and `laKQPqbtCQcWT3XL` share the name but are inactive copies; they are not the live Ingress.
- **Crew read-only key** (export `stack/n8n/workflows/eYdwlwZKTMV89z2J.json`). Live n8n runs the *previous* crew webhook (n8n Header Auth, so a bad or missing key gets 403; checked 2026-10-30 with `NEXUS_LIVE_CREW=1`). The version below is **not yet imported**: a second webhook `terrance/ingress-crew` has no n8n auth, because n8n's Header Auth compares with `!==` and answers 403. Instead its **Crew Auth** Code node compares SHA-256(`X-Nexus-Secret`) with a rendered hash in constant time, then drops the header. A missing or wrong key gets **401**. A valid key may call only `kb.query` (payload reduced to query/labels/document_filter) and `memory.query`; anything else gets **403**. The export holds the placeholder `__CREW_KEY_SHA256__`, which never matches (fail closed). `stack/scripts/n8n_render_crew_auth.sh` writes the importable copy to the git-ignored `stack/n8n/rendered/` (0600): an empty key renders a random hash (revoked), and a key equal to `NEXUS_SECRET` or under 32 bytes is refused. The old `nxCrewIngress1` credential is no longer created.
- **Crew key file**: `CREW_INGRESS_SECRET` is its own random value in `stack/.env` (not derived from `NEXUS_SECRET`). `scripts/sync-crew-env.sh` copies it alone into `stack/.env.crew` (git-ignored, untracked, umask 077, 0600), which the crew container loads with `env_file` (never `.env`). It refuses a reused or short key, and clearing the key writes an empty one (revoked).
- **Crew tools**: `stack/crew-service/main.py` keeps a `CrewTool` registry (15s timeout, 4000-char output cap). A tool reaches an agent only if it is both registered and named in `CREW_ALLOWED_TOOLS`; empty means no tools. The live value is `kb_query,memory_query`, and both register only when the crew key is set.
- **crew_tools** (app) reads only `stack/crew-tools.conf` (0600, owner-only, no symlink, ≤4 KiB, written by `scripts/sync-crew-env.sh`) and never opens `.env.crew`.
- **Ingress kb.query errors**: connection refused, DNS failure or timeout returns `ok:false` "knowledge base not running". A Verba 200 with `{error}` returns "knowledge base is empty" (empty-index gRPC error) or "knowledge base query failed". The raw text goes to the n8n log only (`console.error`), never the response.
- **Verba / knowledge base**: compose healthcheck on `/api/health`; the generator defaults to `OLLAMA_MODEL=gemma4:latest` (the live container still has `qwen3.5` until it is recreated) and embeddings use `nomic-embed-text`. The KB is **empty** (never ingested). The ingest plan is `stack/kb/INGEST-PLAN.md`. `scripts/kb-ingest.sh` defaults to a dry run; `--apply` waits for Taisun's OK.

---

## UI Design System

The live design system is `src-console/styles.css` + `src-console/design/`. The former `src/styles/globals.css`, `src/lib/chart-theme.ts` (ECharts) and Sidebar icon rail are legacy/deleted; their history is kept in the session log below. See also `TERRANCE_UI_DOCTRINE.md`.

---

## Completed Work (Session Log — historical)

> Entries below describe the pre-`src-console` UI (`src/App.tsx`, `src/components/*`, Whoop). Many referenced files are now deleted.

### OpenClaw Integration
- [x] Updated OpenClaw from v2026.2.6-3 → v2026.2.22
- [x] Built Tauri command `get_openclaw_health()` — checks process, HTTP, config, memory search
- [x] Fixed `check_memory_search()` to probe `~/.node-llama-cpp/models/*.gguf` (config key absent bug)
- [x] Built `OpenClawStatusCard.tsx` — real-time health display with restart/kill controls
- [x] Built `openclawHealth.ts` — `pushHealthToN8n()` sends health payload to n8n webhook
- [x] Wired `OpenClawStatusCard` into Amara page in App.tsx (2-col grid with GatewayStatus)
- [x] Fixed duplicate import bug in App.tsx (TS2300 → 0 errors)

### n8n Health Monitor Workflow
- [x] Created `openclaw_health_monitor.json` with 7 nodes
- [x] Set real Telegram Bot Token + Chat ID in Code node
- [x] Re-imported workflow, resolved FK constraint on activation, workflow is ACTIVE
- [x] n8n workflow sends Telegram alerts on healthy/degraded/offline status changes

### UI Modernization (Apple Vision Pro aesthetic)
- [x] **globals.css** — Richer OKLch palette, enhanced glass classes, ambient body gradient mesh, shimmer/glow-pulse/gradient-shift animations
- [x] **Sidebar** — Complete icon rail rewrite (64px→220px framer-motion, Radix Tooltips, glow on active)
- [x] **Shell** — Ambient glow orbs (cyan top-left, violet bottom-right), `p-6→p-7`, z-index layering
- [x] **Header** — `glass-subtle`, larger typography (`text-xl`), `px-7 py-5`, `rounded-xl` buttons
- [x] **Typography** — Bulk bump: `text-[10px]→text-xs`, `text-[11px]→text-[13px]`, h3 `text-sm→text-base`
- [x] **Spacing** — `glass p-5→p-6`, `glass p-4→p-5`, `space-y-4→space-y-5`
- [x] **Badges** — `px-1.5 py-0.5 rounded` → `px-2 py-0.5 rounded-md font-medium`
- [x] **Progress bars** — `h-1.5→h-2` across GoalTracker, SessionModePicker, RiskPanel, etc.
- [x] **ECharts** — All 9 charts use `CHART_THEME` constants (axis 11px, tooltip 13px/r12, gauge 32px detail)
- [x] **Dialogs/Modals** — All Radix overlays use `glass-elevated`
- [x] **Settings inputs** — `py-2 rounded-lg` → `py-2.5 rounded-xl`
- [x] **App.tsx grids** — `gap-4→gap-5`, `space-y-6→space-y-7`

### LLM Router (Session Feb 2026)
- [x] `src/lib/types.ts` — Added `ComputeMode`, `LlmRouterConfig`, `RouterResponse`, `LlmUsageRecord`, `CostEstimate`; 8 new `NexusConfig` fields
- [x] `src-tauri/src/commands/llm_router.rs` — Full routing module: `decide_provider()`, `call_ollama()`, `call_anthropic()`, 4 Tauri commands, 16 unit tests
- [x] `src-tauri/src/commands/mod.rs` — Added `pub mod llm_router`
- [x] `src-tauri/src/lib.rs` — Registered 4 commands: `chat_with_router`, `get_cost_estimate`, `get_llm_usage`, `clear_llm_usage`
- [x] `src-tauri/src/commands/telegram.rs` — Added 8 `#[serde(default)]` LLM router fields to `NexusConfig` struct
- [x] `src/lib/llm-router.ts` — TypeScript helpers: `estimateTokens`, `estimateCostUsd`, `formatCost`, `formatProvider`, `buildRouterConfig`, `aggregateUsage`
- [x] `src/lib/api.ts` — Added `chatWithRouter`, `getCostEstimate`, `getLlmUsage`, `clearLlmUsage` wrappers
- [x] `src/components/settings/SettingsPage.tsx` — Compute Mode section: mode selector, model inputs, API key, budget caps, cost toggle, usage summary
- [x] `src/components/terrance/TelegramChat.tsx` — Wired to `chatWithRouter`; shows provider badge + cost estimate per message

### Amara Continuity Architecture (Session Feb 2026)
- [x] `config/terrance.identity.md` — Canonical identity: operating principles, communication style, decision priorities, tone constraints, mission alignment. Prepended to ALL model calls regardless of provider.
- [x] `src/terrance/system/continuity.ts` — `buildSystemPrompt()` (identity + context primer), `summarizeAndStoreSession()` (post-session memory), `loadContext()` / `saveContext()`, `INFERENCE_PARAMS` (shared temperature/top_p/repeat_penalty)
- [x] `src-tauri/src/commands/continuity.rs` — `read_identity_file`, `load_terrance_context`, `save_terrance_context`, `store_session_log`, `list_session_logs`
- [x] `memory/session_logs/` — Directory for per-session JSON summaries (runtime directory, not tracked in git)
- [x] `~/Library/.../terrance_context.json` — Runtime context store: active goals + last session summary
- [x] `src/components/terrance/TelegramChat.tsx` — Identity injected at send time; "Save to memory" button triggers `summarizeAndStoreSession()`; provider badge + cost shown per message

**Continuity guarantees:**
- Identity file is always prepended regardless of mode (local/hybrid/api)
- Context (goals + last summary) loaded from external storage before every session
- Session summaries stored to `memory/session_logs/` after each conversation
- Fallback identity inline if file read fails — Amara never loses its identity
- Routing never alters the identity prompt — only the compute engine changes

---

### OpenClaw Model Routing (Session Feb 25 2026)
- [x] OpenClaw primary switched to `google/gemini-2.0-flash` (temporary — until Anthropic credits topped up)
- [x] Fallback chain: `ollama/qwen2.5:7b → ollama/qwen2.5:32b → anthropic/claude-haiku → anthropic/claude-sonnet`
- [x] `OLLAMA_API_KEY=ollama-local` added to LaunchAgent plist
- [x] `models.json` fixed — was storing literal string "OLLAMA_API_KEY" instead of value
- [x] Sessions cache cleared (was locked on deleted deepseek-r1:32b)

### Visual Redesign: Tactical Command Interface (Session Mar 2026)
- [x] **globals.css** — Complete rewrite: removed glass morphism, OKLch colors → hex, near-black base (#0B0F14), `.panel`/`.panel-primary`/`.panel-elevated` replace `.glass` classes, new utility classes (`.heading-section`, `.metric-value`, `.metric-label`, `.badge-status`, `.btn-kill`, `.subsystem-inactive`, `.terminal-readout`), reduced border-radius (max 6px), removed light mode, restrained animations
- [x] **chart-theme.ts** — Thinner gauges (progressWidth: 6), desaturated colors (#3D8B6E emerald, #8B7A3D amber), darker tooltip/axis styling
- [x] **Shell.tsx** — Removed vignette overlay div, reduced padding (p-6→px-4 py-3)
- [x] **Header.tsx** — Removed glass-subtle, smaller typography (text-xs), mono uppercase tracked headings, tighter layout
- [x] **Sidebar.tsx** — Removed glass class, dark bg (#0A0E13), narrower (56px collapsed/200px expanded), mono uppercase labels, reduced border-radius
- [x] **Amara page** — Full redesign: SYSTEM CORE dominant full-width panel at top, 3-col secondary modules (Token/Gateway/Skills), 2-col tertiary (OpenClaw/Schedule), full-width chat. All components use `.panel`/`.heading-section`/`.metric-value`/`.metric-label`
- [x] **Business page** — TaskBoard, ContentPipeline, RevenueCard: glass→panel, reduced padding, mono fonts, tighter spacing
- [x] **Assistant page** — CalendarView/EmailSummary use `.subsystem-inactive` (dimmed, dashed border), DailyLog uses `.terminal-readout`
- [x] **Fitness page** — RecoveryScore/SleepChart/StrainChart/HrvTrend: glass→panel, thinner gauge arcs via CHART_THEME
- [x] **Trajectory page** — WellnessScore/GoalTracker: glass→panel
- [x] **ARM/Revenue page** — All 7 arm components: glass→panel (bulk sed)
- [x] **Neuro page** — All 24 neuro components: glass→panel (bulk sed)
- [x] **Settings page** — All sections: glass→panel (bulk sed)
- [x] **TypeScript** — Zero errors (`npx tsc --noEmit` clean)

**Design language**: Near-black graphite base, 1px steel borders, desaturated emerald accent, ALL CAPS tracked headings, JetBrains Mono for metrics, thin instrumentation gauges, no drop shadows, no blur, no pastels, no rounded-xl. Panels feel mounted, not floating.

### Contrast Calibration Pass (Session Mar 2026)
- [x] **globals.css** — Lifted all CSS custom properties: card #0F1318→#131921, foreground #C8CCD4→#E0E3E9, muted-foreground #5C6370→#737D8C, accent #3D8B6E→#4A9E7E (+15% saturation), border #1E2530→#263040 (higher opacity), panel-primary glow strengthened, heading color #5C6370→#8B95A3, metric-value #F0F2F5 (brightest tier), metric-label #454D58→#6B7585, all semantic colors lifted ~15%
- [x] **chart-theme.ts** — Axis labels rgba(92,99,112)→rgba(139,149,163), splitLine rgba(30,37,48)→rgba(38,48,64), tooltip text lifted to match new foreground, chart colors aligned to new accent (#4A9E7E, #A3923D, #4A80A3)
- [x] **Amara page** — All 6 components manually calibrated: StatusCard (model text /90→100%, heartbeat /70→/85), TokenUsage (chart bar colors updated to new accent), SkillsList (skill names /70→/85, badges /70→100%), GatewayStatus (metric values /50→/75, destructive /60→/80), CronSchedule (job names /70→/85, borders /20→/35), TelegramChat (comprehensive overhaul: timestamps /20→/40, meta /15→/35, bot icons /60→/80, message borders /10→/18, input bg #080B0F→#0C1018, placeholder /20→/35, send button /70→/85, typing dots /30→/45)
- [x] **Header.tsx** — Page title /80→/90, description /50→/65, buttons /50→/65, hover states /70→/80, header border /40→/55
- [x] **Sidebar.tsx** — Sidebar bg #0A0E13→#0C1018, borders /20→/35 and /40→/55, title /70→/85, subtitle /40→/55, version /25→/40, BLE dots /20→/35, tooltip arrow fill #121820→#172030
- [x] **System-wide bulk uplift** — All remaining components (~50 files): text-muted-foreground (/15→/30, /20→/35, /25→/45, /30→/50, /40→/60), text-foreground (/50→/70, /60→/80, /70→/85), border-border (/10→/18, /12→/20, /15→/25, /20→/35, /30→/45), semantic colors (text-accent/destructive/success/warning /50→/70 and /60→/80), semantic bg (/8→/12, /10→/15), semantic borders (accent/warning/destructive /15→/25, success /20→/30)
- [x] **Fitness charts** — StrainChart: axis labels → CHART_THEME refs, splitLine → CHART_THEME. WellnessScore: radar axis colors aligned to calibrated palette, chart trace #0891b2→#4A80A3
- [x] **TypeScript** — Zero errors (`npx tsc --noEmit` clean)

**Calibration principle**: 3-layer depth system (background #0B0F14 → panels #131921 → primary #161E2A), typography legibility tiers (headings ~#8B95A3, metrics #F0F2F5, secondary 70-85%, tertiary minimum 50%), accent +15% saturation, all borders ≥ /18 opacity, no text below /30 opacity.

## Pending / Future Work

- [x] **Rename to Nexus** — done in 80276f9 (names) and f30acbb (app-data migration)
- [x] **Clean up after the src-console migration** (WIP snapshot commit 4a33ff6): the empty legacy dirs (`src/components/*`, `src/styles`, `src-next/`, `src-console/system/`) are gone
- [ ] **Legacy `src/`** — decide what remains (not type-checked or imported by the entry point)
- [x] **`notifications.rs`** — removed (d6fd108)
- [x] **CI** — Node 22 via `.nvmrc`, `test:neuro`, `cargo test --locked`, fmt/clippy (non-blocking), macOS job; tag-triggered draft release in `release.yml`
- [x] **CrewAI service** — running on 127.0.0.1:8400; Crews UI (9e36a1e); bearer token (dc7e5b2), see STAGES.md Phase 5E
- [x] **n8n webhook auth**: the Ingress webhook uses Header Auth, see STAGES.md Stage 4
- [ ] **End-to-end Telegram alert test**, **OpenClaw auto-restart** (from earlier list)

---

## Key Patterns & Conventions

### Adding a New Tauri Command
1. Write `#[tauri::command] pub async fn my_command(...) -> Result<T, String>` in `src-tauri/src/commands/`
2. Declare `pub mod` in `commands/mod.rs` and add to `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`
3. Add TypeScript wrapper in `src-console/bridge.ts` using `invoke<T>('my_command', { args })`
4. Use in components with `useEffect` + `useState`

### Adding a New Page
Add an entry to the nav list in `src-console/App.tsx` and render its view there.

### n8n HTTP Calls from Code Nodes
n8n 2.x task runner sandbox — **no `fetch`, no `process`**. Use:
```js
const response = await helpers.httpRequest({
  url: 'http://host/path',
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ key: value }),
  json: true,
});
```

---

## Important File Locations

| File | Purpose |
|------|---------|
| `src-console/App.tsx` | Live UI shell + navigation |
| `src-console/bridge.ts` | Tauri invoke wrappers |
| `src-console/styles.css` | Design system |
| `src-tauri/src/lib.rs` | Command registration (`generate_handler!`) |
| `src-tauri/src/commands/mod.rs` | Command module declarations |
| `src-tauri/src/commands/noop.rs` | NOOP health bridge (read-only) |
| `src-tauri/src/commands/openclaw.rs` | OpenClaw health check logic |
| `~/.openclaw/openclaw.json` | OpenClaw runtime config (read for status/health only) |
| `stack/integrations/n8n/` | n8n workflow JSON files |
