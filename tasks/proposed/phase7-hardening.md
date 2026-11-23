# Stage D / Phase 7: harden in place (build plan, read-only research)

Sources: HANDOVER-FINAL Phase 7 (`tasks/HANDOVER-FINAL-2026-11-12.md:322-343`), hard rules (`.claude/rules/hard-rules.md`), "Runtime and security" DoD (handover `:139-146`).
**`standalone-app-plan.md` does not exist anywhere in the repo** (searched whole tree). §2.3-2.5/§7 are known only from the handover summary at `:324`. Not verified: any Health-sync listener (none found in `src-tauri/src`; treat as "no server" until Taisun confirms).
Rust rule applies to every lane: loopback only, no new outbound hosts, never log secrets (`.claude/rules/rust.md`).

## 1. Per-launch token

### Mechanism (one change point)
- App client construction is already funnelled: `shared/http.rs:46` `builder(spec)` and `loopback_http.rs:14` `loopback_client()`. Add `HttpSpec.token: Option<TokenScope>`; `builder` does `.default_headers(...)` (`x-amara-launch: <tok>`) only when the spec says so. Do NOT put it on a global default: Hermes/HA/Letta/n8n/Weaviate must never receive Amara's token (they are not ours; leaking it to a third-party log is a risk).
- Unconverted call sites build bare `reqwest::Client::new()` and must move to the builder first (lane B0): `crew.rs:168,205,234,260,282,328,345,480`, `crew_grants.rs:195,212,230,250`, `n8n_status.rs:83`, `outbox.rs:181,312`, `telegram.rs:494,621` (the `Client::new` source-pin tests at `telegram.rs:822-836`, `crew.rs:510`, `arm.rs:1172`, `money.rs:2848` must be updated in the same commit, not loosened). Off-Mac clients (`telegram.rs:152,367,423`, `research.rs:33,185`, `money.rs:1235`, `notify_relay.rs:468`) are NOT given the token.
- Mint: new `src-tauri/src/commands/launch_token.rs` at app start: 32 random bytes (OsRng), hex; dir `<data_dir>/run/` created 0700, file `run/launch_token` written 0600 via `app_paths::private_open_options` (`app_paths.rs:167`); atomic write (tmp + rename); rewritten every launch; removed on quit (token file only, never user data). Held in memory as `Zeroizing<String>`; never in IPC, kv, logs.
- Sidecars learn it with no LaunchAgent edits by **reading the file per request (mtime-cached, re-stat each request)**, so a router started by launchd before the app picks up the new token the moment the app mints it. Until the app has run, the file is absent: server must fail closed (401) except `/health`.
- Host/Origin: reuse the router's pattern (`amara-router/src/server.rs:66-109`: Host must be loopback, any `Origin` header refused). Port the same two checks to crew-service.

### Per-service table
| Service | Owner / editable | Token feasible, where | App call sites | Host/Origin | /health open |
|---|---|---|---|---|---|
| amara-router :8787 | repo crate `amara-router/` (editable) | YES. `local_router` (`server.rs:66-109`) already has a bearer layer but only when `cloud.anthropic_enabled` (`:72-76`). Change: always require `x-amara-launch` = file token (new `launch_token.rs` in the crate, constant-time compare reuse `constant_time_eq`). Keep the existing cloud bearer as a second check. | `llm_router.rs:273`, `router_stream.rs:249`, `terrence_runtime_bridge.rs:605` (all via `loopback_client`); Hermes also calls the router (it is in `~/.hermes`, cannot send our header) | Already done (`:94-106`) | `GET /health` (`:83`) |
| router LAN listener | same crate, `lan_router` `server.rs:125` | Already bearer-only (`AMARA_ROUTER_API_KEY`, `config.rs:506-525`); leave as is. Not app-called. | none | n/a | no |
| crew-service :8400 | `stack/crew-service/main.py` (editable) | PARTLY DONE: static `X-Crew-Token` = `CREW_SERVICE_TOKEN` (`main.py:1588-1592`, `compose:207`), 401 otherwise. Per-launch variant: add a second accepted credential, `X-Amara-Launch`, validated against a bind-mounted file (see below). Static token stays valid until Taisun retires it (gate). | `crew.rs` (header const `:5`, token reader `:21`, `CREW_TOKEN_FILE :15`), `crew_grants.rs` | none today: add Host (loopback or `host.docker.internal`) + no-Origin dependency in `main.py` | `GET /health` (`main.py:1594`; compose healthcheck `:238` needs it open) |
| Health sync / NOOP reader | none found listening | n/a; if a listener is added later it must call `launch_token` verification. State "no server" in the lane report. | none found | n/a | n/a |
| Amara Voice level feed :8791 | `~/projects/amara-voice`, NOT editable: patch only to `tasks/proposed/*.diff` | Server is a ws publisher; app is the client (`voice_bridge.rs:6-7,23,33`). Patch: accept `?t=<token>` or `Sec-WebSocket-Protocol` carrying the file token, reject otherwise; add app side: append token to `level_url`. Depends on Taisun applying the patch. | `voice_bridge.rs` (`level_url`, test `:484`) | patch: reject Origin | no HTTP health |
| n8n :5678 | third party (container) | NO per-launch token. n8n has its own `X-N8N-API-KEY` (`n8n_status.rs:12-13,57`, file `n8n_api_key`). Leave; `/healthz` open. | `n8n_status.rs:54-90` | n/a | `/healthz` |
| Letta :8283 | third party, pinned 0.16.8 (decision 0015) | NO per-launch token; uses `letta_server_password` (Keychain, `keychain.rs:20`; read `memory_read.rs:79`) | `memory_read.rs:30,163`, `memory_write.rs` | n/a | n/a |
| Hermes :8642 | `~/.hermes`, NEVER edit | NO. Uses `API_SERVER_KEY` read from `~/.hermes/.env` per request (`terrence_runtime_bridge.rs:10,45`). Rotation = section 7. | `terrence_runtime_bridge.rs:31,598-605` | n/a | n/a |
| Home Assistant :8123 | third party | NO; HA long-lived token (`ha.rs:7-9`, `ha_token` Keychain name) | `ha.rs:339` | n/a | n/a |
| Weaviate :8080 / Verba :8001 | third party containers | Weaviate API key, not launch token: section 3 | `kb_status.rs:65,85,159` | n/a | n/a |

### Crew container learning the token (check compose)
Compose has no bind mount for runtime files today (`stack/docker-compose.yml:184-240`; crew reads `.env.crew` via `env_file` `:200-202`, `CREW_ROUTER_KEY` `:218`). Options: (a) add `volumes: - ${HOME}/Library/Application Support/com.taisun.nexus/run:/run/amara:ro` and have `main.py` read `/run/amara/launch_token` per request. This is a **compose edit on the live stack + `docker compose up -d crew` = Taisun step**. Build the code (main.py reads file if present, so it is inert without the mount) now; test with `--network none` Docker per hard rules. Crew to router: `CREW_ROUTER_URL=host.docker.internal:8787` (`:217`): the router would then see Host `host.docker.internal`, which `loopback_host` (`server.rs:112-122`) rejects. Needs an explicit allowlist entry (`host.docker.internal`) in the router plus the crew sending the launch header (crew `main.py:1081`), both gated on the restart step below.

### Buildable now vs gate
- Now (no live effect): `launch_token.rs`, `HttpSpec.token`, client conversions, router middleware (off unless the file exists = "optional mode"), crew `main.py` second credential, unit + probe tests on ephemeral ports.
- **GATE/Taisun**: router restart (decision 0004 / TAISUN-STEPS) to load the new binary; `docker compose up -d crew` with the new mount; switching the router/crew from optional to enforced (flip `require_launch_token`); applying the amara-voice patch. Order: app ships and mints, then router restarted, then enforced. Reverse order locks the app out of the router (`/v1/chat` 401).
- Hermes calls the router with no header. Enforcing token on `/v1/*` breaks Hermes. Decision needed from Taisun: keep Hermes working by (i) a second router credential (Hermes `base_url` key from `~/.hermes`, which we may not edit: patch in `~/projects/hermes-patches`), or (ii) leave the existing local bearer rule and enforce launch-token only on `/amara/*` and `/api/*`. Recommend (ii) first. Mark as **open question**.

## 2. Keychain migration tool [GATE per move]
Existing infra: `keychain.rs:13` prefix `com.taisun.nexus.`, allowlist `SECRETS` `:19-27` (names: `telegram_bot_token`, `telegram_chat_id`, `n8n_encryption_key`, `ha_token`, `freqtrade_api`, `coach_keys`, `letta_server_password`), commands `secrets_status`, `secret_set` (`:165,:195`, write-only, returns ok), UI `src-console/standalone/SecretSheet.tsx` plus `adapters.ts`. `needs_approval` (`:173`) already exists.
File-backed secret NAMES and readers (names only):
| Name | File | Reader |
|---|---|---|
| `HA_TOKEN` | `<data_dir>/.env` (fallback `~/projects/amara-voice/.env`) | `ha.rs:1011-1105` (`app_env_token`) |
| n8n API key | `<data_dir>/n8n_api_key` | `n8n_status.rs:12,89` |
| `CREW_SERVICE_TOKEN` | `<data_dir>/crew_service_token` (0600; env first) | `crew.rs:15,21-60` |
| config.json fields `gateway_auth_token`, `telegram_bot_token`, `telegram_chat_id` | app `config.json` | `app_paths.rs` (merge `:536-558`); telegram reader `telegram.rs` (not traced) |
| stack/.env: `POSTGRES_PASSWORD`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `CREW_SERVICE_TOKEN`, `CREW_ROUTER_KEY`, `NEXUS_SECRET`, `LETTA_SERVER_PASSWORD`, `LETTA_DB_PASSWORD` | `~/projects/nexus/stack/.env` (live) and `.env.crew` | consumed by compose, not the app: NOT movable by the app (compose needs env). Report-only; moving is a stack change = Taisun step. |
| router `AMARA_ROUTER_API_KEY`, `ANTHROPIC_API_KEY` | Keychain `com.taisun.amara-router` or 0600 `secrets_file` | `amara-router/src/config.rs:36-41,518` (already prefers Keychain) |
| Hermes `API_SERVER_KEY`, others | `~/.hermes/.env` | **NOT touched**; read-only by `terrence_runtime_bridge.rs` |
Design (new `keychain_migrate.rs`; Keychain trait reused for fakes):
- `keychain_import_review()` (already named in handover Phase 9): returns `[{name, source: file|keychain|both|none, file_mode_ok}]` only.
- Readers get one helper `secret_prefer_keychain(name, file_reader)`: Keychain first, then file. Convert `ha.rs`, `n8n_status.rs`, `crew.rs` readers; precedence tests with `FakeKeychain` (pattern at `notify_relay.rs:936`).
- `keychain_migrate_one(name)`: runs only from a click in SecretSheet, confirmed by a native dialog (as `secret_set`'s `chat_dialog_detail` `:185`). Steps: read file value into `Zeroizing`, `kc.set`, read back and constant-time compare, return `{ok, name}`. **Never deletes the file**; "remove the file copy" is a second, separate gated click (write 0-byte? no: leave file, show instruction). Never prints value; last-4 only if `tail4` already allowed.
- Never for `~/.hermes/.env`, `stack/.env`.

## 3. Weaviate/Verba auth [GATE]
- Today: `AUTHENTICATION_ANONYMOUS_ACCESS_ENABLED: "true"` (`stack/docker-compose.yml:137`); app reads `kb_status.rs:65,85,159` (no key); crew reaches KB via n8n ingress (`CREW_INGRESS_*` `:197,214`), not Weaviate directly. Verba env `:164-170`.
- Order (from handover step 3): (1) app: add optional `weaviate_api_key` to `SECRETS` (`keychain.rs:19`) and an `Authorization: Bearer` header on `kb_status.rs` calls and anywhere `kb.query` hits Weaviate (n8n workflow + verba are outside the repo: list, don't edit); (2) compose: `AUTHENTICATION_APIKEY_ENABLED`, `AUTHENTICATION_APIKEY_ALLOWED_KEYS`, `AUTHENTICATION_APIKEY_USERS`, then `ANONYMOUS_ACCESS_ENABLED=false`; Verba gets `WEAVIATE_API_KEY_VERBA`; (3) parity test: `kb.query` fixed 5-question set before and after returns the same top-k ids (run against a fixture instance, `--network none`).
- Buildable now: step 1 + a `kb_parity` test fixture. **GATE**: step 2 and the live restart.

## 4. NEXUS_OUTBOUND_DISABLED to a Settings default
- Baked at `src-tauri/Info.plist:9-12` (`LSEnvironment`), pinned by test `telegram.rs:847-856` (`finder_launches_have_the_kill_switch_on`). `tauri.conf.json` has no plist key (grep empty).
- Single reader chain: `telegram.rs:232-235` `outbound_disabled() = local_only_on() || parse_outbound_disabled(env)`. Callers: `notify_relay.rs:477,748`, `money.rs:1207,1524`, `home_windows.rs:389,1052`, `telegram.rs:339`, outbox. Navy: `local_only_on()` `telegram.rs:243`; `LOCAL_ONLY` AtomicBool stays untouched, so navy stays local-only.
- Change: add `OUTBOUND_ALLOWED: AtomicBool` (default false) loaded from a persisted Settings value at startup; `outbound_disabled() = local_only_on() || env_forces_on || !OUTBOUND_ALLOWED`. Env var, if set, still forces disabled (cannot be overridden from Settings). Add commands `outbound_get/set` (register in `commands/mod.rs`, `lib.rs generate_handler!`, `test/commands.registered.test.ts` per rust.md). UI toggle is a Design Lead file (design boundary): backend + adapter only, note in handoff. Remove the `LSEnvironment` block and replace the pin test with "Info.plist has no LSEnvironment; default is disabled". Router keeps its own env (LaunchAgent plist `amara-router/launchd/...plist:39`, not edited).
- Risk: Finder launch default stays OFF/disabled, so behaviour is unchanged by default.

## 5. Unified logs `~/Library/Logs/com.taisun.nexus/`
- Current sites: ~192 `println!/eprintln!` in `src-tauri/src` + `amara-router/src` (grep); router file log `~/Library/Logs/amara-router/router.log` via LaunchAgent (`launchd/...plist:29-32`); reader/redactor `service_logs.rs` (`redact_line`, only `router` source); muse gatt logs `muse.rs:134` (under data dir; **muse files are sacred, leave them**); `activity_log_dir` (`app_paths.rs:731`), `take_log.rs`, `decisions_log.rs` (domain records, not logs).
- Minimal design: new `unified_log.rs` with `log!(level, target, msg)`; writes `app-YYYY-MM-DD.log` 0600 (private_open_options), dir 0700; rotate at 5 MiB, keep 7 files; pass every message through the existing `service_logs::redact_line`; never takes prompt bodies/health values (rule). Convert `eprintln!` in lib.rs/commands to it incrementally (separate lane, mechanical). Migration: on first run copy `~/Library/Logs/amara-router/*` into `com.taisun.nexus/legacy/` (copy only, never delete). `service_logs` reads the new dir as well.

## 6. Data-dir chmod (build only) [GATE "not yet"]
New command `data_dir_harden(confirm: bool)` + `data_dir_harden_status()` (dry-run listing counts only). Walk `app_paths::data_dir()` without following symlinks, dirs to 0700, files to 0600, **skip `replays/`, `sessions/`, `muse/` entirely (hard rule 9)**, skip anything not owned by us, report counts. Only reachable from a UI click + native dialog. Do not run it. Test on a tempdir.

## 7. Hermes API_SERVER_KEY rotation [GATE, final rotation batch]
List only: key lives in `~/.hermes/.env` (24 bytes, target >=32), read per request at `terrence_runtime_bridge.rs:10,45`; Hermes restarts and any consumer of the key (the app, plus whatever else holds it in `~/.hermes`) must change together. Write the procedure into `TAISUN-STEPS.md`; no code, no edit. Rotation runs last with the other keys (hard rule 7).

## Lanes (file-disjoint) and order
| Lane | Files | Depends |
|---|---|---|
| D0 | `commands/launch_token.rs` (new), `shared/http.rs`, `loopback_http.rs`, `commands/mod.rs`, `lib.rs` | none |
| D1 | convert bare clients: `crew.rs`, `crew_grants.rs`, `n8n_status.rs`, `outbox.rs` (+pin tests) | D0 |
| D2 | `amara-router/src/{server.rs,config.rs,launch_token.rs}`, router tests | D0 file format only |
| D3 | `stack/crew-service/main.py`, `test_main.py`, compose diff as a proposal in `tasks/proposed/` | D0 format |
| D4 | `keychain.rs` additions, `keychain_migrate.rs`, `ha.rs`/`n8n_status.rs`/`crew.rs` readers (merge order after D1), SecretSheet adapter wiring | D1 |
| D5 | `telegram.rs`, `Info.plist`, new `outbound.rs` settings + registrations | none |
| D6 | `unified_log.rs`, `service_logs.rs`, mechanical `eprintln!` swaps | none (swaps last) |
| D7 | `data_dir_harden` in new `data_harden.rs` | none |
| D8 | `kb_status.rs`, Weaviate key plumbing, `tasks/proposed/` compose + parity fixture | D4 (Keychain name) |
| D9 | `tasks/proposed/amara-voice-token.diff`, `voice_bridge.rs` token param | D0 |
Order: D0, then {D2,D3,D5,D6,D7} parallel, D1, D4, D8, D9. D1 and D4 both touch `crew.rs`/`n8n_status.rs`: serialize. Registration files (`mod.rs`, `lib.rs`, `commands.registered.test.ts`) are shared: one owner (D0 pre-registers stubs, or a single integrator commit).

## Tests per item
- Token: router/crew probes on ephemeral ports: no header 401, wrong header 401, right header 200, `/health` 200 without; bad Host 403; Origin 403; token file absent = fail closed; file rotated between requests = new token accepted, old rejected; file mode 0600 and dir 0700 asserted; app client sends header only to loopback base (`is_loopback_url`) and never to off-Mac builders.
- Keychain: FakeKeychain precedence (Keychain over file, fallback when missing); migrate copies and verifies; no command return/log/error contains a sentinel value (extend pattern `notify_relay.rs:1198`).
- Sentinel: set `SENTINEL-<random>` as each secret, run readers/errors/unified logger, grep logs dir, kv, IPC return structs: absent.
- Outbound: default disabled with no env and no plist; env=1 forces on even if Settings enabled; Settings enable plus navy on = still disabled; `outbound_disabled` call sites unchanged.
- Navy: existing local-only tests (`telegram.rs:243`, `home_windows.rs:1927`) must remain green unmodified.
- Logs: rotation boundary, mode 0600/0700, redaction of bearer/keys, legacy copy leaves originals.
- Chmod: tempdir only; muse/replays/sessions untouched.
- Gates: tsc, lint, vitest, neuro, check:secrets, fmt/clippy/test for src-tauri and amara-router, crew tests in Docker `--network none`, e2e at phase end. Counts must not drop.

## Taisun steps / gates
1. Approve each Keychain move and each file-copy removal (SecretSheet clicks).
2. Router restart (decision 0004) to load token middleware; then flip enforcement.
3. `docker compose up -d crew` with the run-dir bind mount (compose edit on the live `~/projects/nexus/stack`).
4. Decide Hermes-to-router auth (open question above).
5. Apply the amara-voice patch (not editable here).
6. Weaviate API-key switch-over and live restart, after parity passes.
7. Click the data-dir chmod: "not yet".
8. Hermes `API_SERVER_KEY` rotation in the final rotation batch.
9. Confirm there is no Health-sync listener, and supply `standalone-app-plan.md` if the §2.3-2.5 detail matters.
