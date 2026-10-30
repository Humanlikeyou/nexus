# Amara Build Stages

> The roadmap from local runtime to personal operating system.

---

## Stage 0 — Foundation (Local Sovereignty)

**Objective:** Own the hardware + runtime.

**Completed:**
- Installed Docker stack
- Set up Ollama
- Wired OpenClaw gateway
- Got MiniMax primary + Ollama fallback running
- Confirmed container health + resource usage
- Hardened .gitignore
- Set up private GitHub repo + CI
- Built Tauri desktop shell
- Deployed to /Applications/Electron.app (pre-rename name; now Nexus / `com.taisun.nexus`)

**Result:** Moved from cloud dependency to controlled local runtime. The "I own my machine" stage.

---

## Stage 1 — Core Brain (Chat + Routing)

**Objective:** Make Amara think properly.

**Completed:**
- LLM Router (model selection logic)
- Identity injection system
- Provider routing (MiniMax / Ollama / fallback chain)
- SQLite persistence for chat
- Session summarization
- Provider escalation ordering fix
- Routing reason transparency

**Result:** Amara can think consistently and switch engines intelligently. The cognitive layer.

---

## Stage 2 — Infrastructure & DevOps Hardening

**Objective:** Make it stable and real.

**Completed:**
- Initialized git + tagged v0.1-core
- Set CI passing (frontend + Rust)
- Eliminated fake UI data
- Fixed silent .catch() errors
- Added error toasts
- Removed misleading charts
- Reduced JS bundle from 1.7MB to 593kB
- Added 30s polling where required
- Fixed onboarding loop bug
- Fixed hardcoded path in Rust
- Confirmed 19/19 tests passing

**Result:** The system stopped pretending and started behaving honestly. This is when it stopped being a prototype.

---

## Stage 3 — Intelligence Stack (Memory + RAG + Gateway)

**Objective:** Externalize cognition.

**Docker services deployed:**
- n8n
- Letta (agent memory)
- Verba (RAG interface)
- Weaviate (vector DB)
- Postgres

**Built:**
- n8n ingress gateway
- 5 event types: health.ping, memory.write, memory.query, kb.ingest, kb.query
- Amara identity canonical definition
- 67 Tauri commands across 19 Rust modules (at the time; working tree now ~136 commands across 28 modules)
- OpenClaw health monitor + Telegram alerting

**Result:** Amara gained long-term memory (Letta), semantic knowledge retrieval (Weaviate), workflow routing (n8n), and observability (health monitor). The "external brain" stage.

---

## Stage 4 — Activation (Complete, except RAG content)

**Objective:** Make it alive.

**Completed:**
- Continuity system (TS + Rust) — `src/terrance/system/continuity.ts` + `src-tauri/src/commands/continuity.rs`
- session_logs structure — per-session JSON summaries in `memory/session_logs/`
- memory.write pipeline — posts to n8n ingress webhook
- RAG infrastructure — Weaviate + Verba up and healthy (compose healthcheck). **The knowledge base is empty**: it has never been ingested, so kb.query returns "knowledge base is empty" until the ingest plan (`stack/kb/INGEST-PLAN.md`) is applied
- LLM router implemented — local/hybrid/api modes with budget caps
- Identity prepended to prompts — `config/terrance.identity.md` always injected
- Automatic memory capture: `src-console/hooks/useIdleAutoSave.ts` saves after 10 min idle (`IDLE_AUTOSAVE_MS`) via `summarizeAndStoreSessionSilent()`, and also on new chat and on close. The old TelegramChat 2-min timer is gone with the legacy UI
- Scheduled consolidation — `scheduler.ts` runs every 5min, consolidation every 6h, summarizes last 24h of sessions via LLM router, posts to n8n
- Autonomous reasoning loops — `runReasoningLoop()` every 12h, goal-driven reflection, pattern recognition, generates proactive briefs, writes to n8n memory
- Gateway authentication — n8n ingress webhook uses Header Auth (credential `nxIngressSecret1`, header `X-Nexus-Secret`, value synced from `NEXUS_SECRET` by `stack/scripts/n8n_sync_credentials.sh`); client sends the secret from NexusConfig. There is no open mode: requests without a valid header get 403

- Crew read-only access: `terrance/ingress-crew` (kb.query + memory.query only; 401 bad key, 403 other actions) is in the export but **not yet imported** into live n8n

**Result:** The nervous system is alive. It remembers automatically, consolidates on schedule, reasons proactively, and authenticates its communications. It moves on its own.

---

## Stage 5 — Autonomy (In Progress)

**Objective:** Multi-agent autonomy.

**Phase 5A — Proactive Daily Summary** (COMPLETE)
- Scheduler extended with daily summary tick at configurable hour (default 7 AM)
- `generateDailySummary()` in continuity.ts: loads goals + sessions + memory → LLM generates morning brief → Telegram delivery
- `lastDailySummaryTimestamp` + `dailySummaryHour` added to TerranceContext (TS + Rust)
- CronSchedule.tsx updated to show real scheduler job status with next/last run times
- 20h cooldown prevents double-send across app restarts

**Phase 5B — Self-Diagnosis + Auto-Healing** (COMPLETE)
- Health check tick every 30min in scheduler
- New `selfDiagnosis.ts`: checks OpenClaw gateway, n8n Docker, Ollama port
- Auto-heal: restart Docker containers, and OpenClaw (launchctl) in `gateway` mode only. Never in local mode (9f0d8b7), and never when OpenClaw isn't installed (`unavailable:`, no dialog)
- 1h cooldown per service prevents heal spam
- Telegram alerts for failures that can't be auto-healed
- New Rust commands: `check_docker_service`, `restart_docker_service`, `check_service_port`

**Phase 5C — Memory Pruning / Archival** (COMPLETE)
- Archive session logs >30 days to `memory/archive/` via `archive_old_session_logs` Rust command
- LLM consolidation of old logs into monthly summary before archiving
- `prune_context_memory` trims relevantMemory to 20 items
- Weekly pruning tick in scheduler
- `pruneMemory()` orchestrates full workflow in continuity.ts

**Phase 5D — Background Research** (COMPLETE)
- Web search via reqwest: Brave Search API (primary) or DuckDuckGo HTML (fallback)
- `fetch_url` command: fetches + strips HTML to readable text
- `research.ts`: search → read top 3 → LLM synthesize → store as session log
- "/research [topic]" command in TelegramChat triggers background research
- New Rust module: `src-tauri/src/commands/research.rs`

**Phase 5E — CrewAI Integration** (running)
- Python FastAPI service with CrewAI in Docker on 127.0.0.1:8400. The LLM is local Ollama gemma4 (`CREW_OLLAMA_MODEL`, default `gemma4`); MiniMax is used only if a key is set.
- [x] `src-tauri/src/commands/crew.rs` — crew_list/start/status/stop/health against http://127.0.0.1:8400 (WIP commit 4a33ff6)
- [x] Python FastAPI CrewAI service in `stack/crew-service/` (a71e91f), running in Docker; `/health` answers ok
- [x] Service token (dc7e5b2): `crew.rs` reads it from the `CREW_SERVICE_TOKEN` env var, or else from `<app data dir>/crew_service_token`. That file must be mode 0600, and symlinks are refused. `scripts/sync-crew-token.sh` copies it from `stack/.env` (umask 077, prints only "written"/"unchanged", never the value). `crew_health` reports `not_configured` (no token), `offline`, `degraded` or `operational`.
- [x] Crews UI (9e36a1e): Chat & Agents › Crews has a service card (Online/Offline/Not configured), a runs list, a New Run form behind a confirm dialog, Stop behind a destructive dialog, 5s polling while a run is running, and a ⌘K entry

*Approach: Built on existing TypeScript/Rust stack first (5A-5D), CrewAI Python service last (5E).*
*Local model at the time: qwen3.5 (9.7B). OpenClaw primary: MiniMax-M2.5, fallback: ollama/qwen3.5. Since 68f2b49 the router defaults to local Ollama `gemma4:latest`.*

This stage turns Amara from semi-autonomous to fully autonomous.

**OpenClaw leftovers (follow-ups):**
- CLAUDE.md "OpenClaw Health Check Logic" still describes `check_memory_search()` reading `~/.openclaw/config.json`; the code reads `openclaw.json`
- `stack/integrations/n8n/openclaw_health_monitor.json` (inactive) and `src/terrance/system/openclawHealth.ts` still post OpenClaw health to n8n
- Settings still offers compute mode `gateway`, and `scripts/mock-gateway.mjs` mocks OpenClaw
- `get_openclaw_health` keeps the `status: "offline"` string for not installed; the typed signal is `installed:false`
- Gallery (`src-console/ui/gallery/Gallery.tsx`) still shows an "OpenClaw gateway" sample row
- Crews screen can't list tools: crew-service has no tools endpoint and `CrewSnapshot` has no tool data
- No KB status reaches the UI, so the KB empty/offline/error states have no surface yet

---

## Stage 5½ — Console Rebuild (In Progress — WIP commit 4a33ff6)

- Live UI moved to `src-console/` (old `src/App.tsx` + `src/components/*` deleted; Tailwind/Radix/ECharts/framer-motion removed)
- Whoop BLE integration removed (`whoop.rs`); health is read-only from NOOP via `noop.rs`
- Amara runtime modules: `terrence_actions/automations/context/priorities/projects/runtime/runtime_bridge.rs`
- [x] Rename electron → Nexus (`com.taisun.nexus`) done (80276f9), with one-time non-destructive app-data copy from `com.taisun.electron` (f30acbb)
- [x] User-facing name **Amara OS**, assistant **Amara**, title "Amara OS — Command Center" (de94dfa, ec12efa); internal ids stay `nexus`
- [x] LLM router defaults: local Ollama `gemma4:latest` (68f2b49); Telegram chat is gemma4-only with no fallback (576e5a2)
- [x] Neutral `Suppressed` activity status for sends refused by the kill switch (8041224)
- [x] `NEXUS_OUTBOUND_DISABLED=1` session kill switch for Telegram + outbox (14cdf7c)
- [x] Outbox: pending items older than 24h become `expired` (kept, never sent) (46fa421)
- [x] Activity-log retention: 30 days / 5000 files, older entries archived to monthly `.jsonl.gz` (6a076f8)
- [x] Prune fix: reasoning and prune failures now back off, so the weekly prune that had been stuck since 2026-07-06 runs again (c90435a, be271f5)

---

## Stage 6 — Full Cognitive System (Future)

**Planned:**
- Multi-agent orchestration — agents coordinating across domains
- Archival memory tier — long-term storage with retrieval ranking
- Knowledge ingestion pipelines — auto-ingest documents, articles, data
- Continuous learning — model fine-tuning on personal data
- Distributed execution logic — offload tasks across machines
- Self-prioritization of tasks — autonomous goal ranking
- Cross-domain reasoning — connect insights across neuro, business, health (NOOP), etc.

*Note: Secure authenticated gateway was delivered in Stage 4.*

At that point it stops being "AI app." It becomes a personal operating system.
