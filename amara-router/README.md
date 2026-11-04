# Amara Router

One local endpoint for Amara. It speaks the OpenAI chat API and the Ollama API, and sends
each request to one of two Ollama models:

- **fast**: resident, answers most things (`gemma4-12b-uncensored:latest` today)
- **deep**: loaded on demand for hard requests (`qwen3.8-uncensored:latest` today)

Model names live in `config.toml` only. Nothing is hard-coded.

```
Home Assistant ─┐
Hermes ─────────┼─► 127.0.0.1:8787 amara-router ─► 127.0.0.1:11434 Ollama
Nexus app ──────┘        (routes, strips <think>, logs)
```

## Routing (first match wins)

| # | Rule | Tier | `reason` |
|---|------|------|----------|
| 0 | `/claude`, `X-Amara-Route: claude`, model `amara-claude`, `X-Amara-Mode: api`, or `hybrid` + an escalation tag | claude if the cloud gate is open, else deep | `slash_claude`, `mode=api`, … (`; claude_blocked=<why>`) |
| 1 | `/deep`, `X-Amara-Route: deep`, model `amara-deep`, a trigger phrase, or tag `heavy`/`research`/`code`/`long` | deep | `slash_deep`, `header_deep`, `model_deep`, `trigger:<phrase>`, `tag=<t>` |
| 2 | `/fast`, `X-Amara-Route: fast`, model `amara-fast` | fast | `slash_fast`, `header_fast`, `model_fast` |
| 3 | Latest user message over `token_threshold` (~4 chars/token), or it has a ```` ``` ```` code block | deep | `over_token_threshold`, `code_block` |
| 4 | EASY/HARD classifier on the fast model | HARD → deep, else fast | `classifier_hard`, `classifier_easy`, `classifier_unclear`, `classifier_timeout`, `classifier_error` |

- `/deep`, `/fast`, `/claude` and trigger phrases are stripped from the message before it is forwarded.
- Only the latest user message is routed on. The full history and the system prompt are passed through unchanged.
- The classifier sends the first `classifier_max_chars` (400) characters with temperature 0,
  `num_predict` 3, `think: false` and a JSON-schema enum (`EASY`|`HARD`), and gives up after
  `classifier_timeout_ms` (2000). Timeout or error = fast.
- Fallback when a model fails to answer: deep → fast; claude → deep → fast. Never to the network.

### Classifier speed on this Mac (measured 2026-10-31)

Prompt evaluation on gemma4-12b runs at about 85 tokens/s, and Gemma's sliding-window
attention stops Ollama reusing the cached prompt prefix. So every prompt token costs about
11 ms. The classifier prompt is kept to about 50 tokens. Measured routing overhead for short
questions with fast warm: 0.40–0.84 s (typically ~0.8 s). Messages near 400 characters take
about 1.0 s. If that is too slow, set `classifier = false` (then rule 4 is plain fast) or
use `/deep` and trigger phrases.

## Route metadata for clients

Every chat reply carries the decision:

- Headers: `X-Amara-Tier: fast|deep|claude`, `X-Amara-Route-Reason: <reason>`, `X-Amara-Routing-Ms: <ms>`
  (set before the body, so they are on streamed replies too; after a fallback they name the tier that answered).
- JSON: `"amara": {"tier": "fast", "reason": "classifier_easy", "routing_ms": 795}`
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

Home Assistant's Ollama integration calls exactly `GET /api/tags` (setup, config flow, model
list), `POST /api/chat` (`stream: true`, `options.num_ctx`, `keep_alive`, `tools`, optional
`think`/`format`) and `POST /api/pull` when the chosen model is not in the tag list. It does
not call `/api/show` or `/api/version`; those exist for other Ollama clients.

Requests may also send `X-Amara-Route`, `X-Amara-Tags`, `X-Amara-Mode`, `X-Amara-Allow-Claude`.

### What the router overrides

The router owns model loading, so on every forwarded request it sets `model`, `keep_alive`
(fast `-1`, deep `"5m"`), `options.num_ctx` (`fast_num_ctx` / `deep_num_ctx`) and, when the
caller did not set it, `think` (`fast_think = false`). Callers asking for another context size
or keep-alive (Home Assistant sends `num_ctx` 8192 and `keep_alive -1`) therefore never force the
resident model to reload. `<think>…</think>` blocks are removed from content, also while
streaming (tags split across chunks are held back), and Ollama's separate `thinking` field is dropped.

## Loading and memory

- Fast stays resident (`keep_alive = -1`), warmed at start (`warm_fast_on_start`).
- Deep loads on demand and unloads after 5 idle minutes (`deep_keep_alive = "5m"`).
- After a deep answer the router polls `/api/ps` every `poll_secs`. Once deep has unloaded,
  it reloads fast if Ollama had evicted it (`rewarm_fast_after_deep`).
- **Optional, OFF by default:** on a 32 GB M1 Pro macOS lets the GPU wire roughly two thirds of
  RAM, so fast (≈8.5 GB at 131k context) and deep (≈16 GB + KV) may not both fit and Ollama
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
{"ts":"2026-10-31T09:26:54.803Z","api":"openai","stream":false,"requested":"fast","route":"fast","reason":"classifier_easy","model":"gemma4-12b-uncensored:latest","routing_ms":795,"latency_ms":5058,"prompt_tokens":31,"completion_tokens":56,"status":"ok"}
```

No message content is ever logged. Errors add `"status":"error"` and `http_status`.

## Install (LaunchAgent)

```bash
cd ~/projects/nexus-amara/amara-router    # path moves after the nexus merge
cargo build --release
cargo test

# 1. Binary
mkdir -p ~/.local/bin
cp target/release/amara-router ~/.local/bin/amara-router

# 2. Config (0600). Check model names first:  ollama list
mkdir -p ~/.config/amara-router && chmod 700 ~/.config/amara-router
cp config.example.toml ~/.config/amara-router/config.toml
chmod 600 ~/.config/amara-router/config.toml
~/.local/bin/amara-router --config ~/.config/amara-router/config.toml --check

# 3. Port must be free
lsof -nP -iTCP:8787 -sTCP:LISTEN          # no output = free

# 4. LaunchAgent
mkdir -p ~/Library/Logs/amara-router
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

## Point Home Assistant at the router

If HA runs on this Mac (Docker/VM with host networking), use the loopback listener. If it
runs on another machine, enable the LAN listener (`lan_bind = "0.0.0.0:8788"` plus the
secrets file) and allow it through the macOS firewall. Every LAN request needs
`Authorization: Bearer <AMARA_ROUTER_API_KEY>`. Recent HA versions have an API key field on
the Ollama integration; if yours does not, HA cannot send the header, so keep HA on the
loopback listener or put a reverse proxy in front.

1. Back up first: Settings › System › Backups › Create backup.
2. Settings › Devices & services › Add integration › **Ollama**.
3. URL: `http://127.0.0.1:8787` (same host) or `http://<mac-lan-ip>:8788` (LAN). API key: the
   `AMARA_ROUTER_API_KEY` value if you use the LAN listener.
4. Add a conversation agent, model **`amara:latest`** (auto routing). It shows as
   "(downloaded)". `amara-fast:latest` forces fast (best for quick voice commands),
   `amara-deep:latest` forces deep.
5. Keep "Think before responding" off. Context window and keep-alive settings are
   overridden by the router (see above), so the defaults are fine.
6. Settings › Voice assistants: select the new agent for your assistant.
7. Test: Assist › "What's the capital of Japan?", then "/deep plan my week".

## Point Hermes at the router

Hermes today calls Ollama's OpenAI API directly (`~/.hermes/config.yaml`:
`model.base_url: http://localhost:11434/v1`, `model.default: gemma4-12b-uncensored`), so the
router never sees Amara's chat and Nexus can't show a Fast/Deep badge (it shows none).
Nothing below has been applied; Taisun applies it. Hermes must use the OpenAI path `http://127.0.0.1:8787/v1` (with `/v1`), not bare `:8787`; Home Assistant uses the bare URL.

1. Back up (keep the file name; it is how you roll back):

   ```bash
   cp -p ~/.hermes/config.yaml ~/.hermes/config.yaml.backup-before-amara-router-$(date +%Y%m%d%H%M%S)
   ls -l ~/.hermes/config.yaml.backup-before-amara-router-*
   ```

2. Install and start the router (see Install above) and check `curl -s 127.0.0.1:8787/health`.

3. Edit only the `model:` block in `~/.hermes/config.yaml`:

   ```yaml
   model:
     default: amara                        # auto routing; amara-fast / amara-deep to pin a tier
     provider: custom
     base_url: http://127.0.0.1:8787/v1
     api_mode: chat_completions
     context_length: 131072                # unchanged
     ollama_num_ctx: 8192                  # ignored by the router (it uses fast_num_ctx); keep equal
     max_tokens: 4096
   ```

4. Restart the Hermes gateway the way you normally do (Hermes desktop app, or stop/start
   `hermes gateway run`). Then send Amara a message and watch
   `tail -f ~/Library/Logs/amara-router/routes.jsonl`: one line per model call.

5. Roll back: copy the backup over `config.yaml` and restart Hermes.

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
- **Tier metadata does not come back through Hermes.** Hermes writes its own
  `chat.completion` (`gateway/platforms/api_server_openai_routes.py`), without the router's
  `X-Amara-Tier` header or `amara` field. Nexus therefore takes the tier from the router's route
  log for the turn's time window (`routes.jsonl`, `api: openai`, no content), and falls back
  to a `POST /amara/route` dry run of the same body. If Hermes ever forwards `amara`, Nexus
  uses that first.
- **The routing text includes Hermes's memory block.** Because Hermes appends recalled memory
  to the last user message, a short question with a large recall can cross `token_threshold`
  (2000 tokens) and go deep. The route log shows the tier that really answered.
- **Every model call is routed.** Hermes calls the model once per tool step, and each call
  routes on the same last user message, with ~0.8 s of classifier time when the classifier
  runs. If tool-heavy turns get slow, set `default: amara-fast` and ask for depth with `/deep`.
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
