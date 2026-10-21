# Terrance Build Stages

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
- Deployed to /Applications/Electron.app (rename to Nexus / `com.taisun.nexus` planned, in progress)

**Result:** Moved from cloud dependency to controlled local runtime. The "I own my machine" stage.

---

## Stage 1 — Core Brain (Chat + Routing)

**Objective:** Make Terrance think properly.

**Completed:**
- LLM Router (model selection logic)
- Identity injection system
- Provider routing (MiniMax / Ollama / fallback chain)
- SQLite persistence for chat
- Session summarization
- Provider escalation ordering fix
- Routing reason transparency

**Result:** Terrance can think consistently and switch engines intelligently. The cognitive layer.

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
- Terrance identity canonical definition
- 67 Tauri commands across 19 Rust modules (at the time; working tree now ~136 commands across 28 modules)
- OpenClaw health monitor + Telegram alerting

**Result:** Terrance gained long-term memory (Letta), semantic knowledge retrieval (Weaviate), workflow routing (n8n), and observability (health monitor). The "external brain" stage.

---

## Stage 4 — Activation (Complete)

**Objective:** Make it alive.

**Completed:**
- Continuity system (TS + Rust) — `src/terrance/system/continuity.ts` + `src-tauri/src/commands/continuity.rs`
- session_logs structure — per-session JSON summaries in `memory/session_logs/`
- memory.write pipeline — posts to n8n ingress webhook
- RAG operational — Weaviate + Verba
- LLM router implemented — local/hybrid/api modes with budget caps
- Identity prepended to prompts — `config/terrance.identity.md` always injected
- Automatic memory capture — TelegramChat idle timer (2min, 4+ msgs), silent background save via `summarizeAndStoreSessionSilent()`, cleanup save on unmount
- Scheduled consolidation — `scheduler.ts` runs every 5min, consolidation every 6h, summarizes last 24h of sessions via LLM router, posts to n8n
- Autonomous reasoning loops — `runReasoningLoop()` every 12h, goal-driven reflection, pattern recognition, generates proactive briefs, writes to n8n memory
- Gateway authentication — n8n ingress validates `X-Nexus-Secret` header against `$env.NEXUS_SECRET`, client sends secret from NexusConfig, optional open mode when unconfigured

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
- Auto-heal: restart OpenClaw (launchctl), restart Docker containers
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

**Phase 5E — CrewAI Integration** (started — Rust client only)
- Python FastAPI service with CrewAI in Docker
- [x] `src-tauri/src/commands/crew.rs` — crew_list/start/status/stop/health against http://127.0.0.1:8400 (WIP commit 4a33ff6)
- [ ] Python FastAPI CrewAI service (`stack/crew-service/` — not created; `stack/install-crewai.sh` exists)
- [ ] Crew monitoring UI (planned in `src-console/`; the old CrewDashboard.tsx plan targeted the deleted `src/components`)

*Approach: Built on existing TypeScript/Rust stack first (5A-5D), CrewAI Python service last (5E).*
*Local model: qwen3.5 (9.7B). OpenClaw primary: MiniMax-M2.5, fallback: ollama/qwen3.5.*

This stage turns Terrance from semi-autonomous to fully autonomous.

---

## Stage 5½ — Console Rebuild (In Progress — WIP commit 4a33ff6)

- Live UI moved to `src-console/` (old `src/App.tsx` + `src/components/*` deleted; Tailwind/Radix/ECharts/framer-motion removed)
- Whoop BLE integration removed (`whoop.rs`); health is read-only from NOOP via `noop.rs`
- Terrence runtime modules: `terrence_actions/automations/context/priorities/projects/runtime/runtime_bridge.rs`
- Rename electron → Nexus (`com.taisun.nexus`) planned; partially applied

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
