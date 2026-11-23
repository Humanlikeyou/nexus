# Plan: one turn pipeline in Rust (`amara_turn`)

Design only: nothing here has been built or run. Sources: decision 0016 §2–3, REVIEW-2026-11-13 §2.2, §3.6, §4, §8.
Line numbers are from `amara-integration` as read on 2026-11-13. **[unverified]** marks claims I did not confirm in code.

## 0. What happens on a turn today (checked)
- Pre-route in the renderer. `sendHome` (`src-console/App.tsx:122-127`) tries adds (`useChatAdds`), then navy (`handleNavyPrompt`), then windows (`handleWindowPrompt`), then `sendToTerrence`. Inside it (`app/useConversation.ts:110`), HA runs `preRouteHome`, which calls the Rust `ha_conversation` (`lib/homeAssistant.ts:94,264`; `ha.rs:670`). The lens's `onAsk` uses the same `sendHome` (`App.tsx:178`).
- Context and compression. These run in parallel in the renderer (`useConversation.ts:120-123`): `prepare_amara_turn` (`terrence_runtime.rs:1234`) plus the TS guard (`amaraChat.ts:34`), and `compressTurns` (`amaraChat.ts:55`).
  - The compressor needs more than 6000 estimated tokens (`contextCompressor.ts:52`). It keeps a head of 3 and a tail of 6 (`:55,58`), splits into 5,000-char chunks (`:70`), and calls `chat_with_router` once per chunk *before* the send (`:260`).
  - That call passes the renderer's `buildRouterConfig(cfg)` (`:256`), so in hybrid/api mode the summary can be asked of Claude.
  - Its state is module-global (`_previousSummary`), and it is reset per conversation (`useConversation.ts:53,188`).
- History is the visible messages minus failed and local lines, with assistant turns guarded (`amaraChat.ts:48-52`). Rust re-guards it (`guard_history`, `terrence_runtime.rs:870`).
- The send is `interact_with_terrence` (`terrence_runtime.rs:1323`).
  - Navy gate at `:1363`, then the cached policy prefix (`:1376`, `system_prompt_for`, `terrence_runtime_bridge.rs:778`).
  - Clock and Hemi-Sync sections, then the context message (`:1383-1398`), then a router dry run for the tier (`:1419-1425`).
  - Navy re-check before the send (`:1429`), then `HermesTurnGuard`, then a biased `select!` with the navy arm first (`:1439-1455`, cf93fd0).
  - Action parse comes after (`:1504`, `extract_nexus_action_proposal` `:600`).
- Hermes HTTP sends `"stream": false` (`terrence_runtime_bridge.rs:256`) with `HERMES_TIMEOUT` = 30 min (`:34`). The key is read from `~/.hermes/.env` on each send with blocking `std::fs` inside async (`:805`). `X-Hermes-Session-Key` defaults to `nexus-chat` (`:31,660`). Requests are serialised by `self.turn` (`:814`).
- Retry and cancel. The renderer retries a router 503 "model loading" up to 3 times, at most 60 s each (`modelLoading.ts:10-13`, `useConversation.ts:136-147`). Stop only bumps a generation counter (`useConversation.ts:171-176`), and the Tauri call keeps running.
- Navy path. `navy_local_turn` (`llm_router.rs:562`) refuses on `needs_outside_or_tools` (`:519`) or a non-loopback base. It sends `system_prompt(amara_persona())` plus **only the new text**: no history, no context (`:578`). Claude replies are refused.
- Router streaming. `/v1/chat/completions` honours `stream` (`amara-router/src/server.rs:245,389-396`).
  - SSE format: the first chunk and the finish chunk carry `amara{tier,reason,routing_ms}` (`:584-616`). Errors arrive as `data: {"error":…}` (`:620-626`), then `[DONE]`. Route headers are set on every reply (`with_meta` `:466`).
  - A client disconnect drops the stream and logs `cancelled` (`:364,430`).
  - A cold deep model returns 503 with `Retry-After: 10` (`server.rs:198`, `engine.rs:45`).
- Letta. `memory_read::list_from` (`memory_read.rs:207`) uses GET only, loopback, a 3 s/8 s timeout (`:38-39`) and a Keychain bearer (`:78`). The turn path has no Letta reference (review §3.6).
- Existing pieces this plan reuses:
  - No `tauri::ipc::Channel` exists anywhere in `src-tauri/src`.
  - reqwest has only the `json` feature (`Cargo.toml:24`). `Response::chunk()` already works without the `stream` feature (`llm_router.rs:436`, `memory_read.rs:198`).
  - There is no `regex` and no `tokio-util` crate.

## 1. The command
```
amara_turn(input: AmaraTurnInput, on_event: tauri::ipc::Channel<TurnEvent>) -> Result<TurnId, String>
amara_turn_cancel(turn_id: String) -> bool
AmaraTurnInput { conversation_id, text, surface: "typed"|"voice"|"lens"|"window", deep: bool, ask_context: Option<…> }
TurnEvent (serde tag="type"):
  status { phase: "routing"|"context"|"loading_model"|"sending"|"tools", tier?: "fast"|"deep"|"claude", sections?: [tag] }
  delta  { text }                       // visible text only, never the <nexus_action> tag
  action { kind: "nexus_action"|"add_preview"|"navy"|"window_open"|"home", payload }  // proposal, never executed
  done   { interaction_id, text, tier?, path: "router"|"hermes"|"navy"|"home"|"add", duration_ms, sections_used: [tag] }
  error  { code, message }              // fixed copy only (same strings as today)
```
The command returns the `turn_id` at once and runs the turn on a spawned task. Every turn ends with exactly one `done` or `error`. Steps, in order:

1. **Validate.** Empty input or more than `MAX_MESSAGE_CHARS` (`terrence_runtime.rs:16`) gives an `error`. Same rules as `:1345-1356`.
2. **Pre-route**, no model involved. Each hit persists an interaction exactly as today and emits `action` + `done`.
   - Adds: confirm or cancel a pending preview, else `detect_add_intent`, then `adds::add_preview` (`adds.rs:887`).
     - This needs a hand-written Rust port of `adds/intent.ts:10-34`, because there is no regex crate.
     - The pending state moves per conversation into Rust (today `chatAdds.ts:26`, 2 min `PENDING_MS` `:14`).
     - `add_save` (`adds.rs:906`) runs only on a confirm turn.
   - Navy: a port of `navyMode/parse.ts:8`. It emits `action{navy,on}`, and the renderer still calls `enterNavy` (`navyMode/store.ts:61`) so the confirmation animation stays.
   - Windows: `parseWindowPrompt` (`homeWindows/catalog.ts:93`) is stateless, so it moves to Rust and emits `action{window_open}`. `resolveVoiceTarget` (`homeWindows/intents.ts:30`) reads live window state, so it **stays in the renderer** before the call. (This departs from the brief; see Q4.)
   - HA (typed/lens only, skipped when deep): `looks_like_home_command` port, then `ha_conversation`. `looksLikeHomeCommand` uses renderer-held entity names (`homeAssistant.ts:241`). Whether `ha.rs` already caches names is **[unverified]**; otherwise the input carries `ha_names` (at most 200, each at most 64 chars).
   - `surface: "voice"` stops here. amara-voice already answered through Hermes itself, so a voice turn only runs pre-route (it replaces the three `useVoice*Prompts` hooks, `App.tsx:119-121`).
3. **Context.** This runs `prepare_amara_turn`'s body (`:1235-1263`), clock (`:839`), Hemi-Sync (`:1383`) and the Letta section (§4), all in `spawn_blocking`, and emits `status{context, sections}`. It goes through `nexus_context_message` (`:819`), which already re-guards and escapes. The TS guard pass is no longer needed.
4. **History.** Read from persisted interactions for the conversation (`list_terrence_interactions` logic, `:1758`), drop failed ones, take the last N, then `guard_history`. If a cached summary exists (§5), use head 3 + summary note (assistant role, `amaraChat.ts:60-61` reason) + tail.
5. **Path choice** (§2), then **send** (§2/§3). On a router 503 with `Retry-After`: emit `status{loading_model}` and retry up to 3 times, clamped 0.5–60 s. These are the same constants, moved from `modelLoading.ts:10-13`. Cancel works during the wait.
6. **Action parse.** At the end, run `extract_nexus_action_proposal` on the full raw text, then keep today's `create_terrence_action` block (`:1505-1560`). Emit `action{nexus_action}` and then `done` with the authoritative visible text; the renderer replaces any streamed text with it.
   - While streaming, a `TagHoldback` filter holds text from any `<` that could start `<nexus_action>` until it can rule the tag out. That way no part of the protocol reaches a `delta`.
7. **Persist** a `TerrenceInteraction`, the same struct and directory (`:354`), with `runtime: "amara-router-direct"` on the router path. This keeps `list_terrence_interactions` and the renderer's history loader (`useConversation.ts:54-57`) working unchanged.
8. **Post-turn.** A background summary job (§5), detached from the turn.

## 2. Router direct by default, Hermes by rule
`fn needs_hermes(text, toggles) -> Option<&'static str>` uses an explicit list, no classifier. It runs on `strip_route_prefix(text)` (`bridge.rs:208`), lower-cased and split into words like `needs_outside_or_tools` (`llm_router.rs:519-546`).
- `/hermes` prefix (stripped before send), or the per-turn `tools` toggle (a Design ask, §6).
- Vault: `vault`, `obsidian`, `my notes`, `in my notes`, `note about`, `notes on` (`vault_search`/`vault_read`, per CLAUDE.md).
- Follow-ups: `follow-up`, `follow up`, `followup`, `mark … done`, `i did it`, `done with`, `mute`, `stop reminding`, `stop asking` (`item_done`/`mute_topic`).
- Web: `search`, `look up`, `look it up`, `google`, `news`, `weather`, `http:`, `https:`, `www.`. Hermes has web tools (`terrence_runtime.rs:1359` comment), and the direct path has none.
- **Not** Hermes: the `<nexus_action>` local actions (open URL or app, volume, mute, reveal). The policy text already prepares those on any path (`:635`).
- Collision: "mute" means both a follow-up and volume. If both "volume" and "mute" appear, go direct; otherwise Hermes. The rule table is one `const` with its own tests.

**Navy overrides everything.** If `telegram::local_only_on()` (`telegram.rs:234`) is true, the turn calls `llm_router::navy_chat` unchanged (`llm_router.rs:588`) through the existing `navy_local_interaction` (`terrence_runtime.rs:1268`). That means the same refusal copy, the light tier, no history and no tools. The new code only wraps it to emit `done{path:"navy"}`. Navy is re-checked:
- before the path choice
- right before each send (as `:1429`)
- through a biased `select!` arm `navy_switched_on_since(epoch)` placed first (`telegram.rs:270`), on **both** paths

On the direct path, navy flipping on mid-turn drops the request even though the target is loopback, because the router could have chosen the Claude tier.

**Identity on the direct path.** The system message is `llm_router::system_prompt(&amara_persona())` (`llm_router.rs:404`, `SOUL.md` capped at 64 KiB, or the fallback persona `:385-401`) + `"\n\n"` + `nexus_runtime_system_instruction()` (`terrence_runtime.rs:630`, or the legacy variant for a conversation that has history, as `:1376-1382`).
- It is built once per conversation, in `spawn_blocking` (because `amara_persona` uses `std::fs`), and cached with `system_prompt_for` under the key `direct:<conversation_id>`.
- Per-turn sections follow as a second system message, as in `chat_body` (`bridge.rs:236-256`).
- No new identity text is added. The existing test `nexus_adds_no_identity_of_its_own` (`:2037`) still covers the policy.

**Identity on the Hermes path** is unchanged: the policy-only cached prefix, with Hermes adding `SOUL.md`.

**Session key.** The direct path sends **no** `X-Hermes-Session-Key` and no Hermes session id, since the router has no use for them.
- The Hermes path keeps `session_key()` (`bridge.rs:188`, default `nexus-chat`, overridable with `NEXUS_HERMES_SESSION_KEY`) and the same cached prefix, so the Hermes session hash stays stable.
- Continuity across paths comes from step 4. Direct-path turns are persisted and so appear in the `messages` history that Hermes receives. Hermes's own long-term memory sees only the turns routed to it **[unverified how Hermes merges body history with its session store]**.
- `hermes_session_id` is still stored from Hermes replies only.

**Direct send.** `POST {router_base}/v1/chat/completions` with `stream: true`, `model: "amara"`, and headers from `router_headers` with the Claude gate (`llm_router.rs:307`, `claude_gate` `:247`).
- The base comes from `router_base(None, saved)` only, never from the renderer, as in `navy_chat` (`:590`).
- `outgoing_message(text, deep, true)` (`bridge.rs:223`) keeps `/deep` working.
- The gate config is `RouterConfig::default()`, the same as `deep_complete` (`:676`). Mode `amara` never asks for Claude. Whether to honour the saved hybrid/api mode is Q2.
- The tier comes from the first SSE `amara` chunk (falling back to the `x-amara-route` header) and is emitted as `status{tier}`. The dry run (`bridge.rs:687`) is not needed on this path.
- Usage is appended like `chat_with_router` (`:650-662`).

**Hermes send.** This is `bridge.request` (`bridge.rs:799`) unchanged and non-streamed, with `status{tools}` emitted first. The `.env` read moves to `tokio::fs`. `HermesTurnGuard` (`terrence_runtime.rs:1439`) stays on this path only.

## 3. Cancel and timeouts
- A turn registry lives on a new managed state, `TurnRegistry` (a `Mutex<HashMap<TurnId, Arc<Notify>>>`, so no new crate).
- The task runs `select! { biased; navy, cancel.notified(), send }`. Cancel drops the send future, and with it the reqwest connection. The router then logs `cancelled` (`server.rs:364`).
- A failed `Channel::send` (window gone) counts as a cancel.
- One active turn per conversation: a new `amara_turn` cancels the previous one (today a stale reply is only ignored, `useConversation.ts:96-97`).
- The renderer's `cancelPending` calls `amara_turn_cancel`. This needs no markup change.
- What cancel does *not* stop:
  - Hermes keeps running a turn whose connection was dropped (cf93fd0 message).
  - Whether the router or Ollama stops generating on disconnect is **[unverified]**. `engine.rs:397` says a model *load* continues on purpose.
  - A cancelled turn persists nothing and emits `error{code:"cancelled"}`.

| Timeout | Value | Where |
|---|---|---|
| `ROUTER_CONNECT_TIMEOUT` | 3 s | new, shared client builder (L0) |
| `ROUTER_FIRST_BYTE_TIMEOUT` | 600 s (today's `ask_router`, `llm_router.rs:490`) | direct send, covers a deep load |
| `ROUTER_STREAM_IDLE_TIMEOUT` | 120 s between chunks | new; a stalled stream ends with `error` |
| `ROUTER_TURN_CEILING` | 15 min | new hard cap on the direct path **(proposed; Q3)** |
| `HERMES_TIMEOUT` | 30 min, unchanged (`bridge.rs:34`) | tool turns only; cancel is the real exit |
| `ROUTER_DRY_RUN_TIMEOUT` | 10 s (`bridge.rs:35`) | Hermes path only |
| Letta `REQUEST_TIMEOUT` / `CALL_TIMEOUT` | 3 s / 8 s (`memory_read.rs:38-39`) | reused |
| Model-loading waits | 3 retries × 0.5–60 s (`modelLoading.ts:10-13`) | moved to Rust |
| Summary job | `local_complete` → `ask_router` 600 s, plus a 5 min job cap (new) | background only |

## 4. Letta blocks, read-only
- `turn_memory::letta_section(conversation_id)` calls `memory_read::list_from(LETTA_BASE, &key)` (`memory_read.rs:207`) with the Keychain key read in `spawn_blocking` (as `:232`). The key is zeroized; it is never logged and never put in an event.
- Keep blocks whose label is `human` or `persona`. Each is already capped at `MAX_VALUE_CHARS` = 2000 (`:36`).
  - If more than one agent has these labels, take the agent named `Amara` (the test fixture name, `memory_read.rs:369`). Otherwise inject none and log `letta_ambiguous` **[unverified: live agent names]**.
- The blocks are emitted as the section `letta_memory` (added to `ALLOWED_SECTION_TAGS`, `terrence_runtime.rs:784`). They go through `guarded_section`, which already truncates to 4,000 chars, scans and escapes. They sit in the **per-turn context**, not the cached prefix, so the Hermes session hash doesn't change.
- Cache: per conversation, in memory, filled on the first turn and kept for the conversation's life (a new conversation re-reads).
- Fail-soft: any `MemoryError` (Off/NoKey/Unauthorized/BadResponse) caches an empty value for that conversation, logs only the code, and the turn goes on. No retry until the next conversation.
- No writes. Only `list_from` is called, and that module's `GET`-only test (`memory_read.rs:4-5`) still holds.
- Letta is loopback, so `NEXUS_OUTBOUND_DISABLED` doesn't apply (`memory_read.rs:13-14`). In navy it is not read at all, because the navy path sends no context.

## 5. Compression after the turn, in the background
- A port of `contextCompressor.ts` (thresholds 6000 tokens at 4 chars per token, head 3, tail 6, 5,000-char chunks, iterative summary, the same prompt text `:241-252`) to `turn_memory.rs`.
- The summary call is `llm_router::local_complete` (`llm_router.rs:708`): fast tier, Claude forced off, a Claude-tier reply refused. Today's path can reach Claude in hybrid/api (`contextCompressor.ts:256-265`), so this is stricter.
- Trigger: after `done`, when the history estimate is over the threshold and no job is running for that conversation. Never during a turn, and never in navy.
- Cache: `{summary, summarized_count, built_at}` per conversation, in memory plus `app_data/turn_summaries/<sha256(conversation_id)>.json`, with mode 0600 written through `app_paths::write_private`.
  - The summary passes `injection_guard::scan` (`terrence_runtime.rs:897`) before it is stored and again when used.
- If no summary is ready, a long history is cut to its last messages so the estimate stays under the router's `fast_num_ctx` (65,536 tokens per CLAUDE.md) **[unverified that the router doesn't cut it itself]**. That replaces the blocking extra call.
- The renderer's `resetCompressor`/`compressTurns` calls go away in L4. `contextCompressor.ts` stays in place (legacy `src/`, don't add to it).

## 6. Migration: backend first, behind the current UI
1. **L0–L3** land Rust with no caller. `interact_with_terrence` and `prepare_amara_turn` stay, untouched in behaviour.
2. **L4** switches `useConversation.sendToTerrence` to `amara_turn` behind a localStorage flag, `nexus.turnPipeline=1` (default off).
   - With the flag off, nothing changes.
   - With it on, deltas are buffered in the hook and the reply appears on `done`, the same as now.
   - `loadingModel` is driven by `status{loading_model}`. Stop calls `amara_turn_cancel`. `postLocal` lines come from `action{add_preview}`.
   - All of this is logic in `src-console/app/` and `App.tsx`'s `sendHome` body (no markup).
3. After Taisun has used it with the flag on: flip the default, then remove the renderer pre-route, compress and guard calls. Remove `interact_with_terrence` only after one release.
4. **Design asks** (to `tasks/design-questions.md`, not built by us):
   - Streaming text in the bubble: render `delta` as it arrives.
   - Stop's state while streaming: today's Cancel exists; whether it changes is Design's call.
   - A tier and timing line from `done{tier,duration_ms}` (review §4.5).
   - "Used:" chips from `done.sections_used`.
   - A "tools" toggle or a Hermes indicator (`status{tools}`).
   - How a cancelled partial reply is shown.

## 7. Tests to add
Rust (`cargo test`, mocks only, no live services):
- Rule table: each vault, follow-up and web phrase routes to Hermes; plain chat, volume and mute route direct; `/hermes` is stripped.
- SSE parser: deltas, the `amara` tier in the first and last chunk, `data: {"error"}` becomes `error`, `[DONE]`, split chunks across reads, UTF-8 split mid-char, a body cap.
- `TagHoldback`: a protocol tag split over chunks never appears in any `delta`, and a lone `<` is released.
- Cancel: the mock stream is dropped (its connection-closed flag is set), there is no persist, exactly one `error{cancelled}`, and a second turn cancels the first.
- Timeouts: a stalled mock stream reaches `ROUTER_STREAM_IDLE_TIMEOUT` (paused tokio clock).
- Navy:
  - with local-only on, nothing reaches the Hermes or router mocks except `navy_chat`'s path
  - flipping it on mid-stream drops the direct send (biased arm first, source-order test like `:1871-1896`)
  - the re-check sits right before both sends
- **No new outbound hosts:**
  - every URL the pipeline builds parses through `loopback_base` (`llm_router.rs:273`)
  - a source scan of the new files finds no `http` literal other than through `LETTA_BASE`, `HERMES_API_BASE` and `router_base`
  - the shared client has redirects off and no proxy
  - a renderer-supplied router URL is ignored
- Outbound gate: `NEXUS_OUTBOUND_DISABLED` → `X-Amara-Allow-Claude: 0` on the direct path (as `:1083-1102`). Re-checked right before the send, not just at the start.
- **Nothing secret in events:** serialise every `TurnEvent` from a run with a fake API key, fake Letta key and fake SOUL text. Assert none of them, no `Bearer`, no model id (`gemma`, `qwen`), and no system or policy text appears.
- Error events carry only the fixed strings.
- Identity: the direct body's first system message starts with the SOUL text and contains `IDENTITY_GUARD`. The prefix is stable across turns of one conversation, and there is no `X-Hermes-Session-Key` on router requests.
- Letta: Off, NoKey and 401 lead to a turn with no `letta_memory` and no error. The second turn makes no second GET. A hostile block value is redacted and escaped.
- Summary: the job never runs mid-turn, goes through `local_complete`, a Claude-tier reply is refused, and the cache file is 0600.
- Pre-route parity: one fixture `test/fixtures/turn-intents.json` (add/navy/window/HA phrases → expected kind) read by both the vitest suites for `adds/intent.ts`, `navyMode/parse.ts` and `homeWindows/catalog.ts` and the Rust ports.
- Registration: `amara_turn` and `amara_turn_cancel` appear in `test/commands.registered.test.ts`.

Renderer (vitest): with the flag off, `useConversation` behaves exactly as now (existing tests unchanged). With it on, a mocked Channel `done` adds one assistant message, Stop calls `amara_turn_cancel`, and `status{loading_model}` sets `loadingModel`.

## 8. Lanes (Sonnet; one lane, one agent, orchestrator commits)
Order: **L0 → (L1 ∥ L3) → L2 → L4**. No sqlite opener is needed: the pipeline stores JSON interactions (`terrence_runtime.rs:354`) and summary files. The shared opener with `busy_timeout` stays a separate lane (review §2.3).

Gates for every Rust lane: `cargo fmt --check`, `cargo clippy --offline --all-targets -- -D warnings`, `cargo test --offline` (src-tauri; counts must not drop from the baseline), `npm run check:secrets`. No new crates and no reqwest feature changes.

- **L0: shared loopback HTTP client.**
  - Files: new `src-tauri/src/commands/loopback_http.rs` (`loopback_client(connect_timeout)`: no redirect, `no_proxy`; moves `loopback_base` here and re-exports it), `commands/mod.rs`, plus call-site swaps in `llm_router.rs:298`, `terrence_runtime_bridge.rs:603` and `memory_read.rs:162`.
  - Do not touch: `ha.rs:274` (a second `loopback_base` copy; separate task), every other client site, and behaviour.
  - Gate extra: the existing `router_clients_skip_proxies…` test (`llm_router.rs:1000`) is updated to point at the new home.
  - The bridge client gains `no_proxy` (it lacks one today, `bridge.rs:603-606`): a security-review point.
- **L1: router streaming client.**
  - Files: new `src-tauri/src/commands/router_stream.rs` (SSE parser over `Response::chunk()`, `TagHoldback`, idle and first-byte timeouts, a `StreamTransport` trait plus mock), `commands/mod.rs`.
  - Do not touch: `amara-router/**` (it already streams), `llm_router.rs` beyond `pub(crate)` on `router_headers`/`claude_gate`/`system_prompt`/`amara_persona`/`router_base`.
- **L3: turn memory.**
  - Files: new `src-tauri/src/commands/turn_memory.rs` (Letta section and cache, summary port, summary cache), `memory_read.rs` (only `pub(crate)` on `list_from`/`read_key`), `commands/mod.rs`.
  - Do not touch: `memory_write.rs`, any Letta write, `src/terrance/**`, `memory/`.
- **L2: the pipeline.**
  - Files: new `src-tauri/src/commands/amara_turn.rs` (command, cancel registry, rule table, pre-route ports, navy wrap), `terrence_runtime.rs` (`pub(crate)` on the helpers named in §1, plus `letta_memory` in `ALLOWED_SECTION_TAGS`; no logic change to `interact_with_terrence`), `lib.rs` (`generate_handler!`, `.manage(TurnRegistry)`), `commands/mod.rs`, `test/commands.registered.test.ts`, `test/fixtures/turn-intents.json`.
  - Do not touch: `telegram.rs` navy semantics, `adds.rs` logic, `ha.rs`, `terrence_runtime_bridge.rs` beyond the `tokio::fs` `.env` read, `src-console/**`.
  - Gate extra: security review (outbound, approvals path).
- **L4: renderer wiring behind the flag.**
  - Files: `src-console/app/useConversation.ts`, `src-console/bridge.ts`, `src-console/types.ts`, new `src-console/app/turnChannel.ts` (+ test), the `sendHome` body in `src-console/App.tsx:122-127` (logic only), and the parity reads in the existing intent tests.
  - Do not touch: `src-console/styles/**`, any JSX or markup or class names, `screens/**`, `homeWindows/HomeWindows.tsx`, `navyMode` UI, `src/**`.
  - Gates: `npx tsc --noEmit`, `npm run lint`, `npx vitest run`, `npm run test:neuro`, `npx vite build && NEXUS_OUTBOUND_DISABLED=1 npm run test:e2e` (one Playwright run only).

## 9. Risks
- **Hermes memory drift.** Hermes stops seeing most turns live, so its session memory thins out. Tool turns still get the full body history, but how Hermes uses it is **[unverified]**.
- **Rule misses.** A vault or follow-up question phrased without a trigger word goes direct, and Amara may say she can't look. The mitigations are the `/hermes` prefix and toggle, plus logging a rule-miss code (no content) when the direct reply mentions notes or follow-ups.
- **Intent port drift.** The Rust ports of the TS regexes can drift; the shared fixture limits this. The TS copies stay until the flag default flips.
- **Holdback lag.** `TagHoldback` adds visible lag around `<`.
- **Interaction listing.** It reads every interaction file on each call (`terrence_runtime.rs:1771-1792`). Calling it on every turn grows with all history; cache per conversation in L2, or move to sqlite (review §4.4).
- **Single-flight lost.** Router-direct turns are not serialised like Hermes turns (`bridge.rs:814`). Two windows could load deep twice **[router behaviour unverified]**.
- **Cancel stops the client, not generation.** It may not stop Ollama generating, so it frees the UI but maybe not the GPU.

## 10. Open questions for Taisun
1. **Hermes rule list.** Is the list in §2 right? Add or remove words, and should there be a visible "use tools" toggle (a Design ask)?
2. **Claude on the direct path.** Should it honour the saved hybrid/api mode (Claude possible), or stay `amara` (never Claude) the way Hermes chat behaves today? Whether Hermes sends `X-Amara-Allow-Claude` is **[unverified]**.
3. **Long direct turns.** Is a 15 min ceiling acceptable, or should only Stop end one?
4. **Window close/focus/undo.** These need live window state, so they stay in the renderer. Is that acceptable?
5. **Navy history.** Should navy turns get history and context (local-only data), or stay "new text only" as now (`llm_router.rs:578`)? This plan keeps it as now.
6. **Multiple Letta agents.** If several have human/persona blocks, which agent is Amara's?

## 11. Not verified
- Nothing was built, compiled or run.
- Not read: Hermes internals (`~/.hermes`, outside the repo) and amara-voice.
- Not checked:
  - whether Ollama aborts on disconnect
  - live Letta agent names
  - whether `ha.rs` caches entity names
  - whether `begin_hermes_conversation_action` (`terrence_actions.rs:1556`) suits router-direct turns (L2 must read it before reuse)
  - the router's own context cutting
  - the reqwest 0.12 `chunk()` behaviour on SSE (read in code, not tested here)
- Line numbers are as of reading and will move as lanes land.
