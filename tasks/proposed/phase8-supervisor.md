# Stage E / Phase 8: services supervisor (build plan, read-only research, 2026-11-14)

Sources: HANDOVER-FINAL Phase 8 (`tasks/HANDOVER-FINAL-2026-11-12.md:340-357`), `.claude/rules/`, `docs/design/HANDOFF-1111s.md:24` (step 4), HANDOFF-FOR-REVIEW §21-23, `tasks/proposed/phase7-hardening.md`. Environment read 2026-11-14 (read-only `docker ps/inspect`, `launchctl list`, plist keys, compose diff). No `.env` opened. `standalone-app-plan.md` does not exist; §3 is known only from the handover text.

**Correction (parent, after the research):** finding 0.2 below was read before 2104461. The live `~/projects/nexus/stack/docker-compose.yml` is clean again (restored from `~/projects/_archive/docker-compose.yml.pre-crew-token-20261114`; `git status` clean; 0 mount lines; `stack-crew-1` created 2026-10-30, 0 mounts). The rule stays: the supervisor never runs `compose up`/`--build`.

## 0. Findings that change the design
1. **Name collision.** `service_restart(id)` (`commands/service_control.rs:126`) and `service_logs(id)` (`commands/service_logs.rs:361`) exist, with TS callers `src-console/standalone/liveAdapter.ts:259-261, 165-170`. Evolve in place: the supervisor takes over the internals; `service_restart` keeps its signature and native approval and gains new ids; `service_logs` stays as an alias of `service_log_tail`. Never register a second `service_restart`.
2. (Superseded, see correction.) The supervisor uses `docker start/stop/restart <name>` only, never `compose up`.
3. **Compose project name collision.** Neither compose sets `name:`; the project is `stack` in both trees (`stack-<svc>-1`). The live dir has `data/` and `.env`; `~/projects/nexus-amara/stack` has neither. TAISUN-STEPS §9.9 (`cd ~/projects/nexus-amara/stack && docker compose build crew && up -d crew`, `TAISUN-STEPS.md:204`) would point project `stack` at a tree with no `.env`/`data/`. Fix the step: build crew through the live dir.
4. **amara-voice is not loaded in launchd** (plist present: RunAtLoad, KeepAlive). Supervisor cannot kickstart an unloaded job; bootstrap is forbidden. Voice start would be `./amara start|stop` in `~/projects/amara-voice`: read the script first. Until then voice is observe-only.
5. **Hermes and router run under launchd** (`ai.hermes.gateway`, `com.taisun.amara-router`, KeepAlive, ThrottleInterval 30). A supervisor restart must not race launchd (grace rule, §3).
6. **Not in compose:** `homeassistant` (`ghcr.io/home-assistant/home-assistant:2026.10.4`, 127.0.0.1:8123) and `freqtrade-live` (`freqtradeorg/freqtrade:stable_freqai`, 127.0.0.1:8082->8080) are plain `docker run` containers; a stopped `freqtrade` and a leftover `a7f1d68c3eee_nexus5etest-postgres-1` (Created) exist. Only `docker start/stop/restart <name>`; never `docker run`.
7. Open WebUI: LaunchAgent `com.thevisualdude.open-webui`, port 3000: "separate", never acted on.
8. Ollama: Ollama.app running, `com.ollama.ollama` loaded not running. Map only (Phase 9 owns runtime).

## 1. Compose re-diff (live `~/projects/nexus` vs repo)
One hunk, `crew` only; postgres, n8n, letta 0.16.8, weaviate 1.40.1 (anonymous on, `:137`), verba identical.

| Setting | Live | Repo |
|---|---|---|
| `CREW_MAX_CONCURRENT` | default 2 | default 1 |
| LLM path | `MINIMAX_*`, `OLLAMA_BASE_URL=host.docker.internal:11434`, `CREW_OLLAMA_MODEL` | `CREW_ROUTER_URL=http://host.docker.internal:8787/v1`, `CREW_ROUTER_KEY`, `CREW_DEEP_WAIT_MAX_SECONDS=300`, `NEXUS_OUTBOUND_DISABLED` passthrough |

crew-service drift: live `main.py` 627 lines vs repo 1960; repo adds `crew_safety.py`, `crew_tasks.py`, `crew_memory.py`, `crew_grants.py` (+tests), Dockerfile (+git, `CREW_CONTAINER=1`), requirements (`crewai==1.15.28` vs live `crewai[anthropic]==1.15.28` + `anthropic==0.73.0`). Live lacks `/crews/runs/{id}/steps`, approvals, tasks, memory, grants, launch-token credential, router LLM path. Image `stack-crew` built 2026-10-30 from live sources.
Consequences: crew is a pinned container (`docker restart stack-crew-1` restarts OLD code); `/health` 200 is not feature-complete, so the row carries the probe only. The §9.9 rebuild is a Taisun step through the live dir, and needs the router to accept Host `host.docker.internal` (`phase7-hardening.md:44-45`, not verified).

## 2. Inventory
| Service (UI id) | Runs today | Port | Health probe | Adopt probe | Verbs allowed | Existing code |
|---|---|---|---|---|---|---|
| amara-router | LaunchAgent `com.taisun.amara-router` | 8787 | `GET /health` (`system_map.rs:642-666`) | health 200 | restart: `launchctl kickstart -k gui/<uid>/com.taisun.amara-router`; no stop/start | `llm_router.rs:273`, `router_stream.rs:249` |
| hermes-gateway | LaunchAgent `ai.hermes.gateway` | 8642 | TCP only (HTTP /health not verified) | TCP | restart: kickstart -k; never edit/read `~/.hermes` here | `terrence_runtime_bridge.rs` |
| amara-voice | not loaded | 8791 ws | TCP 8791 + `voice.rs:158` state word, heartbeat | TCP/state | observe-only first | `voice.rs`, `voice_heartbeat.rs`, `voice_bridge.rs` |
| crew-service | `stack-crew-1` | 8400 | `GET /health` (`crew.rs:538`) | inspect + /health | docker start/stop/restart; never compose up | `crew.rs`, `crew_grants.rs`, `service_control.rs:44` |
| n8n | `stack-n8n-1` 2.44.5 | 5678 | `GET /healthz` (`n8n_status.rs:69-71`) | same | docker verbs | `n8n_status.rs` |
| letta | `stack-letta-1` 0.16.8 | 8283 | TCP (`system_map.rs:24`) | TCP | docker verbs; never change image (0015) | `memory_read.rs` |
| postgres | `stack-postgres-1` | 5432 | docker health via inspect | same | docker verbs; dependency order | none |
| weaviate (+verba) | `stack-weaviate-1`, `stack-verba-1` | 8080, 8001 | Verba `/api/health` (`kb_status.rs:145`), Weaviate via `kb_status.rs` | same | docker verbs; row = worst of two | `kb_status.rs` |
| homeassistant | `docker run` container `homeassistant` | 8123 | docker state + TCP (`system_map.rs:21`) | inspect | docker start/stop/restart | `ha.rs:656,668` |
| freqtrade | `freqtrade-live` | 8082 | TCP | inspect | restart/start only; stop gated | none |
| health-reader / health-sync | built_in | none | in-process (`whoop_status.rs:128`) | n/a | none | `whoop_status.rs` |
| open-webui | LaunchAgent | 3000 | TCP | TCP | none ever (`separate`) | none |
| docker | Docker Desktop | n/a | `docker info` (bounded) | same | background start `/usr/bin/open -g -a Docker` | `ha.rs:656` |
| model-runtime | Ollama.app | 11434 | router `/health` ollama word | n/a | observe-only | `system_map.rs` |

Stop semantics: LaunchAgent jobs have no stop (bootout = unload, forbidden); docker stop is fine; kickstart only for loaded jobs.

## 3. Module design
`src-tauri/src/commands/supervisor/`: `mod.rs` (types, commands), `registry.rs` (static table), `probe.rs` (probes behind a trait), `runner.rs` (docker/launchctl/script behind a trait, allowlisted verb enum), `machine.rs` (pure state machine + backoff, injected clock), `events.rs` (ring buffer 500), `lifecycle.rs` (startup, adoption, drain on quit). Reuse `docker_bin()` (`ha.rs:656`), `docker_restart` + `bounded()` (`system.rs:164`), `validate_container_name` (`system.rs:85`), `shared/http.rs`, `loopback_http.rs`, `service_logs::redact_line`, `approval_gate`, `shared/task.rs::blocking`, `unified_log.rs`.

Types (specta): `ServiceId` = the TS ids in `adapters.ts` (`docker, model-runtime, amara-router, hermes-gateway, amara-voice, n8n, crew-service, postgres, letta, weaviate, health-reader, health-sync, homeassistant, freqtrade, open-webui`; `servicesFromMap.ts` uses older `router, hermes, crew, verba`: the adapter maps). `ServiceState`: `unknown | starting | healthy | degraded | restarting | needs_you | stopped | separate` (TS mapping: unknown/starting → waiting, needs_you → failed, separate → healthy + meta, or a design question). `ServiceStatus { id, state, reason_code, since_ms, checked_ms, attempt, max_attempts, owner: user|supervisor|transition, adopted, can: {restart,start,stop} }`. Reasons are fixed codes + fixed text only.

Commands: `services_status()` (cached snapshot, never blocks), `service_events(since, id)` (memory ring + unified-log mirror, fixed strings), `service_log_tail(id)` (same redacted capped read as `service_logs`; sources router.log, Hermes `~/.hermes/logs/gateway.log` read-only, app.log; Docker ids stay `no_log`), `service_restart/start/stop(id)` (all through `approval_gate`), `services_summary()` (attention count), `supervisor_get()` / `supervisor_set_enabled(on)`.

Safety switches: **observe** always on (probes, adoption, events; only read-only `docker inspect/info`), first ship. **act** default OFF via `const SUPERVISOR_ACT_ENABLED = false` plus a persisted HMAC-tagged setting (pattern `outbound.rs`): gates auto-restart, start-on-launch, drain/stop on quit; flipping needs a decision entry. Manual verbs stay approval-gated one by one; first ship = restart only (existing Docker ids + kickstart for router/Hermes).

State machine: first probe Ok → healthy (adopted if pre-existing). Slow 3 of last 10 → degraded. Down x2 → restart attempt (act on, restartable) → restarting n/3; backoff 5 s, 30 s, 2 min; 4th failure → needs_you, no more automatic action until a manual restart (fold `service_control.rs` `MAX_TRIES` into the machine). `stopped` never auto-restarted; `separate` never acted on. launchd race: LaunchAgent ids wait 45 s of continuous Down before kickstart; docker ids 20 s.

Interval: one task, tick 10 s (5 s when not healthy), probes bounded 2 s, concurrent, no overlap; start in `lib.rs` setup after `launch_token::mint()` (`:152`), pattern `voice_heartbeat::start()` (`:188`).

Start on launch (act on): Docker (open -g, poll `docker info` ≤120 s) → Postgres → Letta, Weaviate → Verba, n8n → crew → HA, Freqtrade → router, Hermes (adopt or kickstart if loaded) → voice (after script check). Healthy = adopt; stopped container = `docker start`; missing container = needs_you (never `docker run`/`compose up`). Compose bring-up of missing stack services is a separate gated variant, live dir only, never `--build`/`pull`.

Quit (act on): only `started_by_app` services stop, reverse order, after drain; adopted services, Open WebUI, Ollama, Docker Desktop untouched; Freqtrade never stopped. Crew drain = poll active runs ≤60 s, then `docker stop` (SIGTERM). Hook `RunEvent::ExitRequested` + `prevent_exit()`, overall cap 30 s; keep `voice_heartbeat::stop()` and `launch_token::remove()` in Exit. Read the tray/close flow (`lib.rs:196`) first.

Loopback/tokens/no changes: every base through `is_loopback_url`/`loopback_base`; registry refuses non-loopback at construction; `docker inspect` HostIp `0.0.0.0`/`::` → needs_you `bad_bind`, never started. Launch token only via `amara_service_client` to router/crew/voice; third parties never get it. Source pin: no `kill`, `bootstrap`, `bootout`, `unload`, `load`, `submit`, `rm `, `compose up`, `--build`, `pull`. Verbs: enum `DockerStart/Stop/Restart/Inspect/Info`, `LaunchctlKickstartK`, `OpenDockerHidden`, `VoiceScript{start|stop}`; names from the registry, validated; labels are constants; `gui/<uid>` from `getuid`.

Logging: events/status in memory; one redacted unified-log line per action (id, verb, outcome code); no command output.

## 4. Lanes
| Lane | Files | Depends |
|---|---|---|
| S0 | `commands/mod.rs`, `lib.rs`, `test/commands.registered.test.ts`, `bindings.rs` + regen | first |
| S1 | `supervisor/{registry,machine,events}.rs` + tests | S0 |
| S2 | `supervisor/probe.rs` (+ small `pub(crate)` edits in `n8n_status.rs`, `kb_status.rs`, `crew.rs`, `system_map.rs`) | S1 |
| S3 | `supervisor/{runner,lifecycle}.rs` | S1, S2 |
| S4 | `service_control.rs`, `service_logs.rs` as thin wrappers (keep tests and codes) | S3 |
| S5 | TS: `src-console/standalone/{liveAdapter.ts, servicesFromMap.ts, servicesSummary.ts, systemMapLive.ts}` + tests, `boundTable.ts`, `bridge.ts` (logic only) | S0 bindings |
| S6 | `tasks/proposed/phase8-live-steps.md`, TAISUN-STEPS text fixes as proposed text, handoff | all |

New commands (mod.rs, lib.rs, registered test, specta, boundTable, bridge): `services_status`, `service_events`, `service_log_tail`, `service_start`, `service_stop`, `services_summary`, `supervisor_get`, `supervisor_set_enabled`. Kept: `service_restart`, `service_logs`, `restart_services`, `check_docker_service`, `restart_docker_service`, `check_service_port`.

Adapter: `services()` → `services_status` mapped; `serviceLogs(id)` → `service_log_tail` + `service_events`; `serviceAction(restart|start)`; `summary()` → `services_summary`; fall back to the system-map derivation when a command is missing (UNWIRED, never invented). `ServicesView.tsx` / `standalone.css` are Design files: no edits; visual needs → design-questions.

## 5. Tests
Fake traits + injected clock, no real process/Docker/launchctl/network: start order + adopt zero verbs + missing container needs_you; Down → grace → 3 attempts → needs_you, manual reset; quit drains, only started_by_app stop, cap; refuse non-loopback registry (`0.0.0.0`, LAN, `host.docker.internal`, userinfo) and inspect `HostIp 0.0.0.0`; act off = zero mutating verbs; tokens only to Amara loopback bases (recording mock); log tail sentinel/no_log/symlink/caps; source pins; table-driven machine; `service_control.rs`/`service_logs.rs` tests unmodified. TS mapping tests and a services live-adapter spec with mocked invoke. Baseline: src-tauri 1163/0/6, router 131, vitest 2290/9, neuro 21.

## 6. Gates and open questions
[GATE] flipping act; LaunchAgent retirement; crew image rebuild (§9.9) via the live dir; any `compose up`; voice `./amara` start/stop; Hermes key work.
Taisun steps: fix §9.9 text (project collision); crew rebuild after the router allows Host `host.docker.internal`; cold-boot check after a week of observe data; then decide act, then retirement.
Open: Hermes HTTP /health? `./amara` touches launchd? (and is voice unloaded on purpose, per 0024?); manual start/stop in first ship (recommend restart only); Freqtrade stop/dry-run; HA/Freqtrade into compose?; Docker Desktop missing or license prompt → fixed needs_you; `separate` as state or pill (design question); crew drain endpoint; per-service derived tokens (0027 amendment) before any crew token mount.
