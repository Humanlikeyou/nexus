# 0051: The shared Amara inference layer (Taisun, 2026-11-25, in session)

His words: **"Build the unified Amara intelligence and inference layer by extending Nexus's existing AI architecture ... Do not create a second model router ... Start with the shared interface, router enforcement and tests. Then migrate existing model call sites incrementally ... Stop after the shared intelligence interface is functioning."**

**Order.** This is Phase 4 of the handoff table (`AMARA_PROJECT_HANDOFF.md` §5), taken ahead of Phases 2 and 3 because his prompt asked for it next. Phase 2 (data architecture) and Phase 3 (execution integrity) are still open. Nothing here changes a data store or adds an executor for state-changing tools.

## What changed

**Router (`amara-router/`)**
- **Default-deny for the cloud.** A request with no `X-Amara-Allow-Claude` header used to count as allowed (`engine.rs` `gate`). Now only an explicit `1` opens the cloud. Hermes, crews and Telegram send no header, so they never reach Claude. Today this changes nothing in practice, because the cloud tier is off in the live config (`anthropic_enabled = false`). Test: `tests/http.rs` `cloud_only_when_enabled_asked_and_allowed` failed before the fix and passes after it.
- **`POST /amara/capabilities`.** Per tier (`fast`, `deep`, `claude`): available, local, tools, json_schema, thinking, vision, context_length and num_ctx. The values come from Ollama's `/api/show` for the configured model, never guessed. The reply carries tier names only, no model names. Claude is listed as available only when the router itself could call it. Its tools and json_schema are reported `false`, because the Anthropic path forwards text only.
- The fallback order is unchanged and only ever goes down a tier: claude → deep → fast, and deep → fast. **No local failure ever falls back to the cloud.**

**Nexus (`src-tauri/src/commands/inference/`)**
- `mod.rs` is the interface:
  - **Request:** `InferenceRequest` with purpose, `ModelClass` (Fast | Deep | Cloud) and `ProviderPolicy` (LocalOnly by default | CloudIfAuthorized), plus persona, instruction, retrieved context and messages (system, user, assistant and tool roles, with tool calls). It also carries `OutputSpec` (Text | Json with a schema), tools, `InferenceParams` (temperature, top_p, max_tokens, stop, seed), `Limits` (an overall timeout and retries) and a `CancelToken`.
  - **Result:** `InferenceOutcome` or `InferenceError`. Errors carry fixed user text: the same router-down and Ollama-down wording chat has always used.
- **Provider policy, in Rust:**
  - Fast and Deep always send `llm_router::local_tier_headers()` or `local_deep_headers()` (Allow-Claude 0), whatever the gate says.
  - Cloud needs CloudIfAuthorized **and** the existing `ClaudeGate`: Allow Claude on, a cloud compute mode, under the daily cap, and `NEXUS_OUTBOUND_DISABLED` unset. The navy local-only switch must also be off.
  - A denied request sends nothing. A Claude-tier reply to a local request is refused. A base URL that isn't loopback sends nothing.
- **Structured output:**
  - The runtime is asked for `response_format: json_schema`, which the router turns into Ollama's grammar-constrained `format`.
  - The reply is validated here regardless (`schema.rs`, no new dependency). A schema keyword the checker doesn't enforce is refused before sending.
  - An invalid reply gets one corrective turn, giving the JSON path and rule but never the value.
- **Retries** cover only these failures: router unreachable, 5xx, an empty reply, and invalid output. There is a short backoff, and everything stays under the overall deadline. An empty reply is retried at temperature ≥ 0.3 (see the probe below).
- **`tools.rs` is the tool authorization layer.** A proposed call runs only if all four hold:
  - it was offered in the request;
  - the caller's `ToolGrant` names it;
  - it is read-only, or the grant allows state changes (off for everyone until Phase 3);
  - its arguments validate against the tool's schema.

  Denied calls are not run, and the model is told "not permitted". `run_with_tools` is the application-controlled loop, with a step limit. Only granted tools are offered. When tools are offered, a fixed line, `TOOL_USE_HINT`, is added to the system prompt (see the probe below).
- **Capabilities:** `capabilities()` reads the router endpoint. A router that predates it gives `None` (unknown), not "no".
- **Logging and usage, in one place.**
  - One log line per call: purpose, class, result, attempts, latency and token counts. It never includes a prompt, a reply or schema text.
  - One `llm_usage.json` record per call that used tokens. Failed attempts count, cost uses the same price table as chat, and `routing_reason` is `inference:<purpose>:<result>; <router reason>`.
  - `append_usage_record` now takes a lock, so concurrent writers no longer drop each other's records.

**First migrated call sites:** `llm_router::local_complete` and `local_deep_complete` now go through the layer. Their 9 production callers are unchanged: notify relay, content pipeline, treasury review and sequences, turn memory, goal roadmap, goal route and watchlist.
- **Preserved:** the same headers, persona and instruction, the same 600 s limit, no retry, and the same fixed error text.
- **Changed:**
  - An empty reply is now an error; it used to come back as `Ok("")`.
  - These calls are now logged and counted in usage, at cost 0 on the local tiers.

## Live capability probe (2026-11-25, this Mac)

- **Model.** Router fast = `gemma4:latest`: 8.0B parameters, Q4_K_M, 42 blocks, per-layer embeddings, an audio encoder and a 131072 context. That metadata matches **Gemma 4 E4B**. There is no `gemma4:e4b` tag locally, so this is inferred from metadata, not from the tag name.
- **Capabilities.** Ollama 0.40.14 reports `completion, vision, audio, tools, thinking`.

Results through the router (fast route, Allow-Claude 0):

| Check | Result |
|---|---|
| Plain text | 5/5 |
| `json_schema` output valid against the schema | 5/5 |
| `json_object` mode, checked against the same schema | 0/5. The JSON parses but breaks the enum (`"neutral"`). **Plain JSON mode is not enough; validate.** |
| Structured tool call (no system prompt) | 5/5 at temperature 0; 4/5 at 0.7 |
| Tool call with Amara's persona and identity guard, no hint | **0/10** |
| The same, plus `TOOL_USE_HINT` | 9/10 |
| Turn after a tool result, temperature 0 | **empty reply, 3/3** |
| The same, default temperature | uses the result |
| Interface live test (`inference::tests::live_*`): text, JSON, tool round | 5/5 runs, first attempt each time, about 0.6–1.2 s per call |

Against the old router binary, `/amara/capabilities` gave `None` (unknown). Since the install below it reports real values.

## Not changed

- There is still one router. `chat_with_router`, the Hermes chat path, `router_stream.rs`, `navy_chat` and every renderer call site are unchanged. They migrate one at a time next.
- There is no Tauri command for the layer yet: no new IPC surface and no capability change.
- No data stores, schemas or migrations.

## Router install (done 2026-11-25, at Taisun's request: "You run this code")

- `cargo build --release`, then the README swap into `~/.local/bin/amara-router`. The old binary is backed up at `~/Backups/amara-router-backup-20261125`.
- **The first try failed.** The README steps (`rm`, `cp`, then `codesign --force --sign -` in place, then `launchctl kickstart -k`) made launchd refuse the binary with `OS_REASON_CODESIGNING`, though the same file ran from the shell. The router was down for about a minute until the backup was restored.
- **The second try worked:** copy to `~/.local/bin/.amara-router.new`, `xattr -c`, sign, `codesign --verify --strict`, `--check`, then `mv` over the live path and kickstart. It came up after launchd's 30 s throttle. The signing identifier is now `.amara-router`, taken from the temporary name. That's harmless, but the README steps should switch to this order and pass `--identifier amara-router`.
- **Verified live:**
  - `/health` ok and `/v1/chat/completions` 200, so Hermes keeps working.
  - `/amara/capabilities` (with the launch token): fast = tools, json_schema, thinking, vision, context 131072, num_ctx 65536; deep = tools, json_schema, thinking, context 262144, num_ctx 32768; claude unavailable.
  - A `/claude` dry run with no header goes to deep (blocked).
  - The live interface test passes, now with capabilities detected.
- The `/amara/*` paths need `X-Amara-Launch-Token`, which is existing behaviour. The app's client already sends it.

## Your steps (not done by Claude)

1. Install a Nexus build that includes this commit, so the migrated background calls use the layer. Until then the installed app keeps the old code.
