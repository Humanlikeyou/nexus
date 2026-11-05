# Amara Router

One local endpoint for Amara. It speaks the OpenAI chat API and the Ollama API, and sends
each request to one of two Ollama models:

- **fast**: resident, answers most things (`gemma4:latest` today, 8B)
- **deep**: loaded on demand when asked for (`qwen3.8-uncensored:latest` today)

Model names live in `config.toml` only. Nothing is hard-coded.

```
Hermes ─────┬─► 127.0.0.1:8787 amara-router ─► 127.0.0.1:11434 Ollama
Nexus app ──┘        (routes, strips <think>, logs)
```

## Routing (first match wins)

| # | Rule | Tier | `reason` |
|---|------|------|----------|
| 0 | `/claude`, `X-Amara-Route: claude`, model `amara-claude`, `X-Amara-Mode: api`, or `hybrid` + an escalation tag | claude if the cloud gate is open, else deep | `slash_claude`, `mode=api`, … (`; claude_blocked=<why>`) |
| 1 | `/deep`, `X-Amara-Route: deep`, model `amara-deep`, a trigger phrase, or tag `heavy`/`research`/`code`/`long` | deep | `slash_deep`, `header_deep`, `model_deep`, `trigger:<phrase>`, `tag=<t>` |
| 2 | `/fast`, `X-Amara-Route: fast`, model `amara-fast` | fast | `slash_fast`, `header_fast`, `model_fast` |
| 3 | Only with `auto_deep` on: latest user message over `token_threshold` (~4 chars/token), or it has a ```` ``` ```` code block | deep | `over_token_threshold`, `code_block` |
| 4 | With `classifier` on: EASY/HARD classifier on the fast model. Off: plain fast | HARD → deep, else fast | `classifier_hard`, `classifier_easy`, `classifier_unclear`, `classifier_timeout`, `classifier_error`; off: `default` |

The live config and `config.example.toml` set `auto_deep = false` and `classifier = false`, so deep is manual: `/deep`, a
trigger phrase, a route header, the `amara-deep` alias or an escalation tag. A Claude ask that the gate blocks also goes deep.
Everything else goes fast. Both settings default to true if the keys are missing from `config.toml`.

- `/deep`, `/fast`, `/claude` and trigger phrases are stripped from the message before it is forwarded.
- Only the latest user message is routed on. The full history and the system prompt are passed through unchanged.
- When on, the classifier sends the first `classifier_max_chars` (400) characters with temperature 0,
  `num_predict` 3, `think: false` and a JSON-schema enum (`EASY`|`HARD`), and gives up after
  `classifier_timeout_ms` (2000). Timeout or error = fast.
- Fallback when a model fails to answer: deep → fast; claude → deep → fast. Never to the network.

### Classifier speed

The classifier is off. When on, it added 0.40–0.84 s to short questions with fast warm, and
about 1.0 s near 400 characters. That was measured on 2026-10-31 on the previous fast model,
`gemma4-12b-uncensored`. It has not been measured on `gemma4:latest`.

## Route metadata for clients

Every chat reply carries the decision, streamed or not, and so do errors returned after a tier
was chosen (the 502 when every tier failed, the 503 when Ollama is down or loading). Errors
before routing (a 400 for bad JSON) have none.

- Headers (set before the body, so they are on streamed replies too; after a fallback they name
  the tier and model that actually answered):
  - `X-Amara-Route: fast|deep|claude`
  - `X-Amara-Model: <model id>`, e.g. `gemma4:latest` or the Anthropic model
  - `X-Amara-Route-Reason: <reason>`, e.g. `slash_deep; deep_failed`
  - `X-Amara-Routing-Ms: <ms>`
  - `X-Amara-Tier`: older alias of `X-Amara-Route`, kept for existing clients.
- `model` in the JSON body is the real model id that answered, not the virtual name the caller
  sent (`amara`, `amara:latest`, …): on the OpenAI `chat.completion` object and on every
  `chat.completion.chunk`, and on the Ollama object and every NDJSON line.
- JSON: `"amara": {"tier": "fast", "reason": "default", "routing_ms": 0}`
  - OpenAI non-streamed: top-level `amara` on the `chat.completion` object.
  - OpenAI streamed: on the **first** chunk (role chunk) and the **last** chunk (the one with
    `finish_reason`), before `data: [DONE]`.
  - Ollama: on the final `done: true` object (streamed or not).

## Endpoints

| Method | Path | Notes |
|--------|------|-------|
| POST | `/v1/chat/completions` | OpenAI. `stream` true/false, `stream_options.include_usage`, `tools`, `response_format`, `max_tokens`, `temperature`, `stop`, … |
| GET | `/v1/models` | `amara`, `amara-fast`, `amara-deep` |
| POST | `/api/chat` | Ollama. Streams by default, like Ollama. `tools`, `format`, `options`, `think` pass through |
| GET | `/api/tags` | Lists `amara:latest`, `amara-fast:latest`, `amara-deep:latest` (both `name` and `model` keys) |
| POST | `/api/show` | Minimal model info, `capabilities: ["completion","tools"]` |
| GET | `/api/version` | Ollama's version (proxied) |
| POST | `/api/pull` | Succeeds at once for the three router models; 404 for anything else |
| GET | `/health` | `status` ok/degraded, Ollama up/down, which models are loaded |
| POST | `/amara/route` | Dry run: `{"text": "..."}` or a chat body → `{tier, reason, routing_ms, model, stripped}`. Never generates, never returns content |

Home Assistant doesn't use the router today (see [below](#home-assistant-does-not-use-the-router)). These
Ollama endpoints stay for compatibility. HA's Ollama integration calls exactly `GET /api/tags` (setup, config flow, model
list), `POST /api/chat` (`stream: true`, `options.num_ctx`, `keep_alive`, `tools`, optional
`think`/`format`) and `POST /api/pull` when the chosen model is not in the tag list. It does
not call `/api/show` or `/api/version`; those exist for other Ollama clients.

Requests may also send `X-Amara-Route`, `X-Amara-Tags`, `X-Amara-Mode`, `X-Amara-Allow-Claude`.

### What the router overrides

The router owns model loading, so on every forwarded request it sets `model`, `keep_alive`
(fast `-1`, deep `"5m"`), `options.num_ctx` (`fast_num_ctx` / `deep_num_ctx`) and, when the
caller did not set it, `think` (`fast_think = false`). Callers asking for another context size
or keep-alive (HA's Ollama integration sends `num_ctx` 8192 and `keep_alive -1`) therefore never force the
resident model to reload. `<think>…</think>` blocks are removed from content, also while
streaming (tags split across chunks are held back), and Ollama's separate `thinking` field is dropped.

## Loading and memory

- Fast stays resident (`keep_alive = -1`), warmed at start (`warm_fast_on_start`).
- Deep loads on demand and unloads after 5 idle minutes (`deep_keep_alive = "5m"`).
- Fast and deep don't fit in memory together, so the router swaps them by hand. A deep request while
  deep isn't resident waits for fast replies to end, unloads fast, confirms it left, loads deep
  (about 10 s) and answers on deep in the same request. The load runs on its own, so a client
  disconnecting doesn't stop it, and several requests share one load. Past `deep_load_timeout_ms`
  (90 s) the reply comes from fast with reason `...; deep_unavailable`, one line is logged, fast
  comes back, and deep requests go straight to fast for 60 s. Prompt evaluation after the load has
  `first_token_timeout_resident_ms`; a deep that stays silent past it also falls to fast.
- A fast request while deep is loading or answering waits, up to `fast_wait_ms` (120 s), then gets
  a 503 with `Retry-After: 10`. It never cancels deep. The first fast request after that unloads an
  idle deep by hand. If none comes, fast is re-warmed once deep's keep-alive ends
  (`rewarm_fast_after_deep`, checked every `poll_secs`).
- amara-voice's keep_warm turn ("Voice mode is starting up...") is answered "ready." by the router
  while deep is busy or resident (reason `...; warm_ping_skipped`), so it can't evict deep.
- `fast_num_ctx` defaults to 65536, because Hermes refuses a window under 64,000 tokens.
  `deep_num_ctx` defaults to 32768, because the deep model's KV cache grows fast. Both are sent
  as `options.num_ctx` on every Ollama call, and Ollama silently cuts longer prompts.
- `deep_think` defaults to false. Hidden thinking on deep ran 280 s with no reply.
- First-byte timeout: if the target model is not resident per `/api/ps` (or `/api/ps` does
  not answer), Ollama must send a byte within `first_token_timeout_ms` (20 s); if it is
  resident, within `first_token_timeout_resident_ms` (90 s, since prompt evaluation streams
  nothing). Otherwise the reply is HTTP 503 "Model loading, try again shortly." with
  `Retry-After: 10`, on both APIs, streamed or not; the load continues in the background.
- Before the classifier runs, the router checks `/api/ps` (`residency_check_timeout_ms`,
  default 1000; a model swap makes it take 340-620 ms). If `/api/ps` does not answer in time,
  the classifier is skipped and a *hard-looking* prompt goes deep (`ps_unavailable_hard_to_deep`),
  anything else fast (`classifier_skipped_ps_unavailable`). If fast is not loaded, the
  classifier is skipped: a hard-looking prompt goes deep (`fast_not_resident_hard_to_deep`,
  loading deep if needed, since a load is needed either way), anything else fast (and fast is loaded). Hard-looking (`route::looks_hard`): a ```
  code block, a reasoning phrase (explain why/how, step by step, prove, analyze/analyse,
  compare, debug, design, architecture, trade-off/tradeoff, as a word start, any case), two or
  more `?`, two or more numbered/bulleted lines, or over 300 estimated tokens.
- **Optional, OFF by default:** on a 32 GB M1 Pro macOS lets the GPU wire roughly two thirds of
  RAM, so fast and deep (≈16 GB + KV) may not both fit and Ollama
  evicts fast while deep runs. You can raise the limit until the next reboot with
  `sudo sysctl iogpu.wired_limit_mb=26000`. The router never does this. It leaves ~6 GB for
  macOS, Docker (~5 GB) and the voice pipeline (~3 GB), so expect swapping; undo with
  `sudo sysctl iogpu.wired_limit_mb=0` or a reboot.

## Filler line (off by default)

With `[filler] enabled = true`, a streamed deep or claude answer starts with `text` plus a
blank line, sent at once, so a voice pipeline has something to say while the deep model
loads. Non-streamed replies never get it, so stored chat history stays clean.

## Cloud (Anthropic): off by default

`[cloud] anthropic_enabled = false`. When it is `true` the router logs a warning at start.
Claude is used only when all of these hold:

1. the request asks for it (rule 0),
2. `anthropic_enabled = true`,
3. `NEXUS_OUTBOUND_DISABLED` is not set (1/true/yes/on) in the router's environment,
4. the caller did not send `X-Amara-Allow-Claude: 0` (the Nexus app sends 0 unless
   Settings › Assistant › Allow Claude is on, the mode is api/hybrid, its own `NEXUS_OUTBOUND_DISABLED`
   is unset and the daily cap is not spent),
5. `ANTHROPIC_API_KEY` is in the 0600 `secrets_file`.

Otherwise the request goes deep, with `claude_blocked=<why>` in the reason.

## Log

`~/Library/Logs/amara-router/routes.jsonl` (0600), one line per request:

```json
{"ts":"2026-10-31T09:26:54.803Z","api":"openai","stream":false,"requested":"fast","route":"fast","reason":"default","model":"gemma4:latest","routing_ms":0,"latency_ms":5058,"prompt_tokens":31,"completion_tokens":56,"status":"ok"}
```

No message content is ever logged. Errors add `"status":"error"` and `http_status`.

## Install (LaunchAgent)

```bash
cd ~/projects/nexus-amara/amara-router    # path moves after the nexus merge
cargo build --release
cargo test

# 1. Binary
mkdir -p ~/.local/bin
rm -f ~/.local/bin/amara-router && cp target/release/amara-router ~/.local/bin/amara-router && codesign --force --sign - ~/.local/bin/amara-router

# 2. Config (0600). Check model names first:  ollama list
mkdir -p ~/.config/amara-router && chmod 700 ~/.config/amara-router
cp config.example.toml ~/.config/amara-router/config.toml
chmod 600 ~/.config/amara-router/config.toml
~/.local/bin/amara-router --config ~/.config/amara-router/config.toml --check

# 3. Port must be free
lsof -nP -iTCP:8787 -sTCP:LISTEN          # no output = free

# 4. LaunchAgent
mkdir -p ~/Library/Logs/amara-router
chmod 700 ~/Library/Logs/amara-router
L=~/Library/Logs/amara-router/router.log
[ -e "$L" ] || [ -L "$L" ] || install -m 600 /dev/null "$L"   # never touches an existing log
sed "s#__HOME__#$HOME#g" launchd/com.taisun.amara-router.plist > ~/Library/LaunchAgents/com.taisun.amara-router.plist
plutil -lint ~/Library/LaunchAgents/com.taisun.amara-router.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.taisun.amara-router.plist

# 5. Check
curl -s 127.0.0.1:8787/health
tail -f ~/Library/Logs/amara-router/router.log
```

Restart after a config change: `launchctl kickstart -k gui/$(id -u)/com.taisun.amara-router`.
Remove: `launchctl bootout gui/$(id -u)/com.taisun.amara-router && rm ~/Library/LaunchAgents/com.taisun.amara-router.plist`.

For a dev run, put a 0600 `config.toml` next to this README (git-ignored) and run
`cargo run --release` (the default config path is `./config.toml`, or set `AMARA_ROUTER_CONFIG`).

### Secrets file (only for LAN or cloud)

```bash
umask 077
cat > ~/.config/amara-router/secrets.env <<'EOF'
AMARA_ROUTER_API_KEY=<openssl rand -hex 32>
# ANTHROPIC_API_KEY=...
EOF
chmod 600 ~/.config/amara-router/secrets.env
```

Then set `secrets_file = "~/.config/amara-router/secrets.env"` in config.toml. The router
refuses to start if the file is readable by group or others.

## Home Assistant does not use the router

Nothing to set up. Home Assistant is not a router client, and it needs no LLM.

Checked read-only on HA 2026.10.4, 2026-11-04: its integrations are `sun`, `analytics`, `go2rtc`,
`backup`, `shopping_list`, `radio_browser` and `google_translate`, with no Ollama or other LLM
integration. Its only assist pipeline uses `conversation.home_assistant`, the built-in sentence matcher.

Amara Voice (`amara-voice/ha.py`) and Nexus chat (`src-tauri/src/commands/ha.rs`) call HA themselves.
HA either handles a home command with its matcher, or answers `no_intent_match`, and then the text goes
to Hermes, which calls the router. So free-form text reaches the router through Hermes, never through HA.

### If HA ever gets an LLM conversation agent

That is a new decision, not a setup step. `http://127.0.0.1:8787` will not work: HA runs in Docker
(`amara-voice/homeassistant/docker-compose.yml`), where `127.0.0.1` is the container itself.
`host.docker.internal:8787` does reach the listener, but the router answers 403 because the Host isn't
loopback. The supported way is the LAN listener: `lan_bind` plus the secrets file, with
`http://host.docker.internal:8788` as the URL and `AMARA_ROUTER_API_KEY` in the Ollama integration's
API key field. That opens a port beyond loopback, so get it approved first.

## Point Hermes at the router

Done on 2026-11-04, so don't repeat it. Hermes now calls the router, and the old direct Ollama setup is in the backups below.
Hermes must use the OpenAI path `http://127.0.0.1:8787/v1` (with `/v1`), not bare `:8787`; only Ollama API clients use the bare URL.

`~/.hermes/config.yaml` has this `model:` block:

```yaml
model:
  default: amara                        # the router decides; amara-fast / amara-deep to pin a tier
  provider: custom
  base_url: http://127.0.0.1:8787/v1
  api_mode: chat_completions
  context_length: 65536
  ollama_num_ctx: 65536
  max_tokens: 4096
```

It also sets `agent.api_max_retries: 4` and `platform_toolsets.api_server` to cronjob, file, memory, session_search,
skills, terminal, todo and web. `tools.tool_search.enabled` is `"off"`, so the 8B fast model gets direct tools.
`SOUL.md` is unchanged.

### Why context_length and ollama_num_ctx are 65536

- Hermes refuses a window under 64,000 tokens (`MINIMUM_CONTEXT_LENGTH` in `agent/model_metadata.py`). The old
  `ollama_num_ctx: 16384` can't be used.
- The router ignores `ollama_num_ctx` and sends `fast_num_ctx`, but Hermes still reads both keys for its own window.
- Keep `context_length`, `ollama_num_ctx` and `fast_num_ctx` equal. Ollama silently cuts any prompt longer than
  `num_ctx`, so `context_length: 131072` over a 16384 router window would lose text with no error.
- Deep stays at 32768, below the Hermes floor. Hermes doesn't check that window, so a long session routed to deep can be cut.

### Verify

```bash
curl -s 127.0.0.1:8787/health                       # status ok, Ollama up
curl -s 127.0.0.1:11434/api/ps                      # context_length 65536 on the fast model
tail -f ~/Library/Logs/amara-router/routes.jsonl    # one line per model call
```

Hermes may answer `503 runtime_draining` for up to a minute after a restart. Wait and try again.
Restart it with `launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway`.

### Roll back

Restore Hermes first, then the router config. Full commands: `TAISUN-STEPS.md` section 5.

- `~/.hermes/config.yaml.backup-before-amara-router-20261104152031` and
  `~/.hermes/config.yaml.backup-before-hermes-edits-20261104152924` are identical. Both return Hermes to Ollama directly,
  with `context_length` and `ollama_num_ctx` at 131072.
- `~/.config/amara-router/config.toml.backup-20261104152924` holds `fast_num_ctx` 16384 and `deep_num_ctx` 8192. Never
  restore it while Hermes uses the router: Ollama would cut Hermes's prompts silently.

### What changes once Hermes uses the router (checked in the Hermes source, read-only)

- **`/deep` reaches the router.** The API server passes the user's text to the agent
  unchanged; Hermes only *appends* its memory/plugin context after it
  (`agent/turn_context.py: compose_user_api_content`), so a leading `/deep`, `/fast` or
  `/claude` is still at the start of the last user message. The router strips it before the
  model sees it. Hermes's own transcript keeps the prefix.
- **`X-Amara-*` request headers are not forwarded.** Hermes builds its own OpenAI client; the
  only extra headers are static ones from `model.default_headers` / `model.extra_headers` in
  `config.yaml`. Setting `X-Amara-Route: fast` there would pin *every* Hermes call (Telegram
  included), so use `/deep` per message instead.
- **Tier metadata needs the Hermes route patch.** Stock Hermes writes its own `chat.completion`
  (`gateway/platforms/api_server_openai_routes.py`), without the router's headers or `amara`
  field. The patch in `~/projects/hermes-patches/` is applied, so replies carry them and Nexus
  uses them first. Revert it before `hermes update`. With no route on a reply, Nexus reads the
  route log for the turn's time window (`routes.jsonl`, `api: openai`, no content), then falls
  back to a `POST /amara/route` dry run of the same body.
- **The routing text includes Hermes's memory block.** Because Hermes appends recalled memory
  to the last user message, a short question with a large recall can cross `token_threshold`
  (2000 tokens) and go deep if `auto_deep` is on. The route log shows the tier that really answered.
- **Every model call is routed.** Hermes calls the model once per tool step, and each call
  routes on the same last user message. With the classifier off, routing adds no model call.
- **Telegram and every other Hermes channel go through the router too.** That is the point
  (one router decides), but it also means a deep answer on Telegram loads the deep model.

## Point the Nexus app at the router

Nothing to do: compute mode `amara` is the default and uses `http://127.0.0.1:8787`. To use
another loopback port, set `amara_router_url` in the Nexus config. Non-loopback URLs are ignored.

## Tests

`cargo test` runs unit tests plus end-to-end tests against a mocked Ollama and a mocked
Anthropic (no real model is loaded): each routing rule, classifier timeout/error fallback,
`<think>` stripping across chunks, both API formats streamed and not, tool calls, Ollama
down (clear 503, no crash), the JSONL log, fast re-warm, the filler line, LAN auth and the
cloud gate, the `/api/ps` residency check that skips the classifier, the immediate re-warm
after deep, `num_ctx` on every Ollama body, the first-token 503 and the startup pre-warm.
