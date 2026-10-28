# Amara Architecture Audit

Audited: 2026-08-21

This audit is based on the active Nexus worktree, the locally running services, and build/test results. It does not treat a UI, a mock, or an isolated command as proof of an end-to-end capability.

## Executive Conclusion

Nexus has several worthwhile foundations: a Tauri desktop shell, typed Rust commands, a robust local event/outbox pipeline, a local/API model router, identity material, a modular device layer, and a real Hermes agent runtime. It is not yet one coherent personal operating intelligence.

The immediate architectural problem is runtime fragmentation. The current product runs Hermes for Amara and Telegram, while Nexus is still primarily wired to OpenClaw. It also contains three separate model/chat paths and several unrelated stores for goals, context, sessions, and task-like data. The new console is a visually coherent monitoring surface, but it has removed the conversation and scheduler startup paths that previously connected much of the Amara logic to a user workflow.

Do not add more specialist agents, dashboards, or automations until one runtime, one interaction path, one context model, and one action record exist.

## A. Current Architecture

### Desktop and frontend

- Tauri 2 hosts the app in `src-tauri/`.
- The active entry point is `index.html -> src-console/main.tsx -> src-console/App.tsx`.
- `src-console` polls bounded Tauri commands every 15 seconds and renders seven system views: Amara, Health (NOOP), Muse, Business, ARM, Activity, and Trajectory.
- The console provides an in-app Amara conversation, action approval controls, and a read-only NOOP health view.
- The legacy `src/` application and its component tree are deleted in the current worktree, but most system modules remain under `src/terrance/system/`, `src/lib/`, and `src/hooks/`.

### Tauri/Rust backend

- `src-tauri/src/lib.rs` registers a broad set of direct commands for models, OpenClaw, Telegram, memory, goals, telemetry, device control, research, ARM, and CrewAI.
- The Rust backend starts a durable event outbox worker at app startup. This is the only confirmed background worker hosted by Tauri itself.
- ~~The app identity is still `Electron` / `com.taisun.electron`~~ — resolved: the app is now `Nexus` / `com.taisun.nexus` (80276f9).

### Persistence

The app currently persists state in several independent places:

- `~/Library/Application Support/com.taisun.nexus/events.db`: sensor/event records and outbox rows.
- `~/Library/Application Support/com.taisun.nexus/assistant.db`: chat sessions and chat messages.
- `~/Library/Application Support/com.taisun.nexus/goals.json`: structured goals.
- `~/Library/Application Support/com.taisun.nexus/terrance_context.json`: string-based active goals, a session summary, and `relevantMemory`.
- `~/Library/Application Support/com.taisun.nexus/session_logs/` or a repository-relative `memory/session_logs/`: session summaries.
- `~/Library/Application Support/com.taisun.nexus/activity_log/`: one JSON file per completed background activity.
- `~/.openclaw/`: OpenClaw configuration, markdown tasks/content pipeline, cron data, memory, and skills.
- `~/.hermes/`: the live Hermes configuration, sessions, Telegram runtime, cron jobs, and Amara model wrapper.
- The Docker stack is intended to add n8n, Letta, Weaviate, Verba, and Postgres, but Docker was unavailable during this audit.

### AI/model layer and conversation

- `llm_router.rs` routes direct calls to Ollama, Anthropic, or an OpenClaw-compatible gateway. Its local default is `qwen3.5`, which is not one of the currently installed Ollama models.
- `send_to_terrance` in `telegram.rs` is a separate direct Ollama caller. It bypasses identity, context, conversation history, usage routing, tool use, and action verification.
- The deleted legacy `TelegramChat` used `chat_with_router` and continuity functions. The active `src-console` UI has no replacement.
- Hermes is the actual current assistant runtime: its process is running and its latest gateway log shows a successful Telegram connection. The active local assistant model is `local-amara-agent`.
- OpenClaw is also still running. Nexus reads `~/.openclaw`, probes its port, and restarts `ai.openclaw.gateway`; it does not inspect or control Hermes.

### Memory and context

- `continuity.ts` builds a prompt from identity, profile, skills, strings in `TerranceContext`, and session summaries.
- Consolidation and reasoning functions can write to an n8n endpoint for Letta memory.
- The context model does not distinguish durable facts, preferences, goals, projects, commitments, events, inferred state, confidence, provenance, corrections, or deletion policy as required by the product constitution.

### Tools, actions, and automation

- External capability is represented as many direct Tauri commands, OpenClaw functions, Hermes tools, and n8n workflows. There is no central tool registry or action router.
- The Rust event outbox tracks telemetry delivery to a `/events` gateway and verifies acknowledgements. It does not represent Amara actions or their approval/verification state.
- The TypeScript scheduler includes daily briefs, health checks, memory work, research, jobs, rituals, and subagents. There is no active `startScheduler()` call in the current entry point, so these routines do not start in the active console build. Even if started from a UI, they stop when the app closes.
- Hermes has its own persistent gateway cron scheduler, which is distinct from the Nexus scheduler.

### Projects, tasks, integrations, and devices

- Goals are structured JSON, but project state is not modeled.
- The console reads tasks and content from OpenClaw markdown files. The parser generates hardcoded TaiyoSkin placeholder tasks when no task source is present.
- The Rust backend provides Muse and a read-only NOOP health bridge. NOOP is the exclusive WHOOP source.
- Research, CrewAI, ARM, Telegram, n8n, and local shell controls exist as separate integrations, not as a unified Amara capability layer.

## B. Current User Flow

### Active build

1. Nexus launches Tauri and starts the Rust event outbox worker.
2. The webview loads `src-console/main.tsx` and mounts the console.
3. The console polls direct commands for OpenClaw health, files, goals, sensors, ARM, and activity state every 15 seconds.
4. The user can inspect those values, converse with Amara, and approve prepared Mac actions.
5. The runtime persists interaction and action records for inspection and follow-through.

### Actual Amara path outside Nexus

1. Telegram reaches the Hermes gateway.
2. Hermes assembles its own context, calls `local-amara-agent` through Ollama, selects Hermes tools, and returns a Telegram response.
3. Hermes writes its own sessions, memory, and cron state under `~/.hermes`.
4. Nexus neither receives this interaction nor makes it inspectable.

## C. What Is Worth Preserving

- Tauri is an appropriate local-first desktop foundation.
- The event/outbox transaction and acknowledgement behavior in `events.rs` and `outbox.rs` is strong. It is a good model for future action records because it preserves delivery status rather than assuming success.
- The Rust LLM routing policy has explicit mode and cost behavior, plus 19 passing unit tests.
- The identity/profile material is useful source content, although it needs to be separated from runtime-specific claims and rebalanced around Taisun rather than one business.
- Hermes is an active, local agentic runtime with Telegram and tool calling. It should be integrated, not duplicated.
- The Muse and read-only NOOP integrations are separated from the main UI and can remain contextual providers.
- The console visual language can remain as a presentation direction after it is connected to real Amara state and actions.

## D. Incomplete or Placeholder Functionality

- The active console has no conversation, planning, approval, memory inspection, or action execution workflow.
- The active console uses a read fallback that silently converts command failures into empty/default data. It cannot distinguish a missing integration from a genuine empty state.
- Task parsing supplies fabricated TaiyoSkin work when the source file does not exist.
- The scheduler, Telegram bridge, subagents, reasoning loops, rituals, and custom jobs are not started by the active app entry point.
- The Docker memory/RAG stack is not available while Docker is stopped.
- The model router's default local model does not match the installed local model inventory.
- The existing "gateway" provider is OpenClaw-specific; it is not a Hermes adapter.
- There is no user-facing action status contract covering requested, planning, awaiting permission, executing, succeeded, failed, and needs attention.
- There is no structured project model or canonical prioritization engine.

## E. Duplication and Competing Sources of Truth

| Concern | Competing implementations |
| --- | --- |
| Agent runtime | Hermes/Amara and OpenClaw/Terrance both run locally; Nexus only observes OpenClaw. |
| Model invocation | `chat_with_router`, `send_to_terrance`, Hermes inference, and OpenClaw gateway calls. |
| Conversation persistence | `assistant.db`, session logs, `terrance_context.json`, Hermes sessions, and possibly Letta. |
| Memory | `relevantMemory` strings, session logs, OpenClaw memory, Hermes memory, Letta/Weaviate. |
| Goals/tasks | `goals.json`, string `activeGoals`, OpenClaw `TASKS.md`, custom jobs, and Hermes cron. |
| UI/type layer | the new `src-console` types and bridge duplicate the legacy `src/lib/types.ts` and `src/lib/api.ts` boundary. |
| Background work | frontend TypeScript scheduler, Hermes cron, OpenClaw cron, n8n workflows, and the Rust outbox worker. |

## F. Architectural Debt

- The large uncommitted UI migration deletes approximately 15,794 lines across the legacy UI while adding an untracked `src-console` replacement. It currently builds, but it has not been committed as an intentional product boundary or verified end-to-end.
- Names are inconsistent: Amara/Terrance and OpenClaw/Hermes (Nexus/Electron was resolved by the rename). These make operating state and future migration decisions ambiguous.
- `src-tauri/tauri.conf.json` sets `csp` to `null` and the app has broad command exposure. Future tool execution needs narrower, explicit IPC boundaries.
- `NexusConfig` persists sensitive provider and integration configuration in an application JSON file. This needs a defined secret-storage policy before more external integrations are added.
- Identity text asserts behaviors such as continuous operation, Telegram processing, and comprehensive tracking that the active Nexus application does not currently guarantee.
- Hardcoded, source-relative storage attempts to put session logs in the repository; personal runtime data should not depend on a source checkout.
- The product constitution requires typed memory and action state, but the current core stores many important items as unproven strings.

## G. Broken or Miswired Flows

1. **Nexus does not represent the live assistant.** The actual assistant is Hermes, but Nexus monitors and restarts OpenClaw.
2. **The active UI removed its core interaction.** The console does not call the model router, Hermes, or the continuity layer.
3. **Proactivity is inactive from Nexus.** No active entry point starts `startScheduler()`, and a frontend scheduler would not survive app closure.
4. **Task data can be fake.** `read_tasks` returns fabricated TaiyoSkin tasks when its file is absent; this is displayed as current operational data.
5. **Model selection can fail or drift.** The router defaults to absent `qwen3.5`; a separate direct caller selects whatever model is first installed and omits Amara context.
6. **Failure state is obscured.** Console snapshot reads catch errors and render fallbacks, which can make an offline dependency look like valid zero/empty data.
7. **The Docker-backed intelligence stack is presently unavailable.** The stack smoke test could not connect to Docker.

## H. Missing Foundation

The missing foundation is a small Amara Core, not another screen or another autonomous agent. It needs four explicit contracts:

1. **Runtime adapter:** one selected agent runtime, initially Hermes, behind a stable interface for health, interaction, tool capability, and execution events.
2. **Canonical context store:** typed entities for facts, preferences, goals, projects, commitments, events, and inferences, including timestamps, provenance, confidence, correction, and deletion.
3. **Action registry:** every capability described as a tool/action with risk class, approval policy, lifecycle status, result, and verification evidence.
4. **Persistent orchestration host:** a background service that can continue safely when the Nexus window is closed, with the desktop app as an observer and approval surface.

## I. Recommended Target Architecture

```text
Nexus desktop UI
  -> Amara Core (Tauri commands / typed domain API)
      -> Context assembler <- canonical local state store
      -> Runtime router <- Hermes adapter (initial), future local/API adapters
      -> Tool router <- registered tools + approval policy
      -> Action ledger <- request, approval, execution, verification, outcome
      -> Automation service <- persistent scheduler / triggers
      -> Integrations <- calendar, files, browser, devices, business systems
```

The desktop UI should only display and operate on these typed Core contracts. It should not poll unrelated implementation-specific files or independently recreate model, memory, or scheduler behavior.

Hermes should be the initial runtime adapter because it is the working Telegram agent with tool calling. OpenClaw should be treated as a legacy integration until an explicit decision is made to retire or migrate it. Keep the existing Tauri event/outbox subsystem, but repurpose its delivery principles for an action ledger rather than using it as the definition of Amara activity.

## J. Staged Migration Plan

### Stage 0: Stabilize the active worktree

- **Objective:** Make the console migration intentional and remove false product claims before adding intelligence features.
- **Modules:** `src-console/`, `index.html`, `vite.config.ts`, `src-tauri/tauri.conf.json`, legacy source inventory, Docker stack status.
- **Dependencies:** None.
- **Risk:** Accidental loss of working legacy behavior if files are removed without an inventory.
- **Acceptance:** One documented active entry point; no fabricated task data; visible unavailable/error state; a clear decision to retain, migrate, or remove each deleted legacy capability.

### Stage 1: Unify the Amara runtime and interaction path

- **Objective:** Create the single path for conversation and runtime health, with Hermes as the first adapter.
- **Modules:** new Amara Core boundary in Tauri, Hermes adapter, model routing boundary, `src-console` conversation surface, old OpenClaw-specific commands.
- **Dependencies:** Hermes CLI/gateway interface and a stable local transport contract.
- **Risk:** Session continuity can be lost if the adapter creates unrelated Hermes sessions.
- **Acceptance:** A message sent from Nexus reaches Hermes/Amara, produces a typed response, records the selected runtime/model, and appears in a persisted interaction record. Nexus health reflects Hermes, not a stale parallel runtime.

### Stage 2: Establish canonical context and project state

- **Objective:** Replace scattered string/markdown state with typed local entities and evidence.
- **Modules:** `continuity.rs`, `goals.rs`, chat/session persistence, new context/project repositories, migration importer for existing files.
- **Dependencies:** Stage 1 interaction records.
- **Risk:** Data loss or invented facts during migration.
- **Acceptance:** Goals, projects, commitments, observations, and inferred state have IDs, provenance, timestamps, confidence, and correction/deletion behavior. The context assembler reads only this canonical store.

### Stage 3: Create the action and approval system

- **Objective:** Route real external capabilities through one observable safety boundary.
- **Modules:** new tool registry, action ledger, desktop approval surface, existing shell/browser/filesystem/device integrations.
- **Dependencies:** Stages 1 and 2.
- **Risk:** Unsafe or unreviewable actions.
- **Acceptance:** A tool invocation has a named permission level, requested/approved/executing/succeeded/failed/needs-attention state, user-visible evidence, and retry/rollback semantics where appropriate.

### Stage 4: Prioritization and planning loop

- **Objective:** Make Amara determine what matters and turn goals/projects into next actions.
- **Modules:** context assembler, priority service, decision records, project views.
- **Dependencies:** Canonical context and action ledger.
- **Risk:** Overconfident recommendations presented as facts.
- **Acceptance:** Amara can state its evidence, constraints, alternatives, recommendation, uncertainty, and follow-up outcome for a decision.

### Stage 5: Persistent automation and proactivity

- **Objective:** Move background work out of the frontend so it survives UI closure and remains selectively useful.
- **Modules:** persistent scheduler service, automation definitions, notification policy, action ledger.
- **Dependencies:** Stages 1 through 4.
- **Risk:** duplicate notices, action spam, and hidden failed automations.
- **Acceptance:** A bounded automation has trigger -> context -> evaluation -> action -> result -> memory, persists across app restarts, and is observable in Nexus.

### Stage 6: Integrate external domains deliberately

- **Objective:** Add calendar, email, files, finances, business data, research, and device signals only through the Core contracts.
- **Modules:** individual adapters and context importers.
- **Dependencies:** Stages 2, 3, and 5.
- **Risk:** duplicate data, secret leakage, and noisy low-value context.
- **Acceptance:** Each integration contributes typed observations or registered tools; it does not create its own isolated agent, memory, or scheduler.

### Stage 7: Sensor intelligence

- **Objective:** Use Muse, NOOP, and future health data as contextual signals, never diagnoses.
- **Modules:** existing device modules, context ingestion, interpretation policy.
- **Dependencies:** Canonical events/context and notification policy.
- **Risk:** misleading health claims or unsupported conclusions.
- **Acceptance:** Sensor information is source-tagged, confidence-aware, and used as context for recommendations with explicit uncertainty.

## K. Immediate Next Development Task

Implement **Stage 1: a narrow Hermes runtime adapter and a single typed interaction path**.

That task should not add broad automation or a new dashboard. It should introduce a small `TerrenceRuntime` contract with `getHealth`, `interact`, and `listCapabilities`, implement it for the already-running Hermes agent, and connect one minimal conversation surface in Nexus to that contract. Each interaction must persist as a typed record with runtime/model metadata and visible failure state.

This gives Nexus a relationship with the actual assistant the user uses, restores the product's central conversation loop, and creates the correct boundary for later context, actions, approvals, and automation without rewriting the rest of the app.

## Verification Performed

- `npm run build`: passed.
- `cargo test` in `src-tauri`: 19 passed, 0 failed. The coverage is limited to the LLM routing module; there are no end-to-end interaction, persistence, tool, or UI tests for the active console.
- Docker stack smoke test: could not connect because the Docker daemon is not running.
- Hermes gateway: current process is running; the latest gateway log reports a successful Telegram connection. The launch-agent status command also reported a stale service definition/previous exit status, so a future health adapter should report current process and recent successful connectivity separately.
