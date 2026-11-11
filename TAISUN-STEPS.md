# Taisun's steps: Amara Router, Hermes and the voice bridge

Sections 1 to 3 and 7 are already done. They were approved by Taisun and applied on 2026-11-04, so don't run them again.
Use section 4 to check the setup and section 5 to roll it back. Section 7 has its own roll back. Section 8 records the current status and order of work. Full details: `amara-router/README.md`.
No secret values belong in this file.

## 1. Done: backups

Taken before the changes. `cp -p` kept the original file dates.

| Backup | Holds |
|---|---|
| `~/.hermes/config.yaml.backup-before-amara-router-20261104152031` | Hermes before the router |
| `~/.hermes/config.yaml.backup-before-hermes-edits-20261104152924` | The same bytes as the file above |
| `~/.config/amara-router/config.toml.backup-20261104152924` | Router config with `fast_num_ctx` 16384, `deep_num_ctx` 8192 |
| `~/ai.hermes.gateway.plist.backup-20261104152031` | Hermes LaunchAgent (never edited) |

The two Hermes backups are identical. Both go back to Hermes calling Ollama directly. Neither one undoes only the tuning.

## 2. Done: router

LaunchAgent `com.taisun.amara-router` runs on `127.0.0.1:8787`, with `NEXUS_OUTBOUND_DISABLED=1` and `Umask` 63.
The binary is `~/.local/bin/amara-router` and the config is `~/.config/amara-router/config.toml`.

```toml
[models]
fast_num_ctx = 65536
deep_num_ctx = 32768
deep_think = false
```

Claude stays off: `[cloud] anthropic_enabled = false`, and no secrets file.

On a reinstall, create the log before loading the LaunchAgent. This sets the folder to 0700 and creates the log at 0600. An existing log is left as it is.

```bash
mkdir -p ~/Library/Logs/amara-router
chmod 700 ~/Library/Logs/amara-router
L=~/Library/Logs/amara-router/router.log
[ -e "$L" ] || [ -L "$L" ] || install -m 600 /dev/null "$L"
```

## 3. Done: Hermes

`~/.hermes/config.yaml` now has this `model:` block:

```yaml
model:
  default: amara            # auto routing; amara-fast / amara-deep pin a tier
  provider: custom
  base_url: http://127.0.0.1:8787/v1
  api_mode: chat_completions
  context_length: 65536
  ollama_num_ctx: 65536
  max_tokens: 4096
```

Two other keys changed:

```yaml
agent:
  api_max_retries: 4        # Hermes default is 3
platform_toolsets:
  api_server: [cronjob, file, memory, session_search, skills, terminal, todo, web]
```

`SOUL.md` is unchanged.

Why these values:

- Keep the `/v1`. Hermes uses the OpenAI API, and Nexus detects the router by this URL.
- Hermes refuses a window under 64,000 tokens (`MINIMUM_CONTEXT_LENGTH`), so `ollama_num_ctx: 16384` can't be used.
- `context_length` must match `fast_num_ctx`. Ollama silently cuts any prompt longer than `num_ctx`, so
  `context_length: 131072` would tell Hermes it has far more room than the router gives it.
- Deep stays at 32768 to save memory, and Hermes doesn't check that window. A long session routed to deep can still be cut.

## 4. Verify

```bash
curl -s 127.0.0.1:8787/health                          # status ok, Ollama up, cloud off
curl -s 127.0.0.1:11434/api/ps                         # context_length 65536 on the fast model
plutil -p ~/Library/LaunchAgents/com.taisun.amara-router.plist | grep -E 'NEXUS_OUTBOUND_DISABLED|Umask'
grep -E '^(fast_num_ctx|deep_num_ctx|deep_think)' ~/.config/amara-router/config.toml
grep -A7 '^model:' ~/.hermes/config.yaml
tail -f ~/Library/Logs/amara-router/routes.jsonl       # then send Amara a message: one line per model call
```

In Amara OS › Chat & Agents › Chat, a reply shows a Fast/Deep badge and the Deep toggle is enabled.

Hermes may answer `503 runtime_draining` for up to a minute after a restart. Wait and try again.

## 5. Roll back

This returns Hermes to Ollama directly and removes the router:

```bash
cp -p ~/.hermes/config.yaml.backup-before-amara-router-20261104152031 ~/.hermes/config.yaml
launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway
launchctl bootout gui/$(id -u)/com.taisun.amara-router
rm ~/Library/LaunchAgents/com.taisun.amara-router.plist
cp -p ~/.config/amara-router/config.toml.backup-20261104152924 ~/.config/amara-router/config.toml
```

- The restored file also removes `agent.api_max_retries` and `platform_toolsets.api_server`, and sets `context_length` and
  `ollama_num_ctx` back to 131072. The first reply can be slow while Ollama reloads the model at that size.
- Never restore the router config alone. With Hermes on the router, 16384 cuts its prompts silently.
- To undo only the tuning, edit the values by hand and keep `context_length`, `ollama_num_ctx` and `fast_num_ctx` equal
  and at 64,000 or more. Then restart both:

```bash
launchctl kickstart -k gui/$(id -u)/com.taisun.amara-router
launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway
```

The Hermes plist was never edited, so `~/ai.hermes.gateway.plist.backup-20261104152031` is needed only if it changes.

## 6. Voice bridge

The bridge shows Amara Voice's live state and sound level in Amara OS. Nothing here changes Hermes or the router.

- **State:** Amara OS reads `~/projects/amara-voice/logs/state` and checks for the `amara_voice.py` process. It emits
  `amara-voice://state` on each change and every 5 seconds.
- **Level:** Amara OS connects to `ws://127.0.0.1:8791/level`, published by `level_feed.py` in amara-voice at about 30 Hz.
  It emits `amara-voice://level`. Frames carry level and state only, never audio. The feed binds to loopback only.
- **Port:** 8791 on both sides. To move it, set `AMARA_LEVEL_PORT` for amara-voice and `NEXUS_AMARA_VOICE_LEVEL_PORT` for
  Amara OS to the same value.

**Start.** There is nothing to start by hand. The bridge starts when the Voice panel or a live Amara lens opens, and stops
when the last one closes. If Amara Voice isn't running, the state reads as unavailable and no level events arrive.

To start Amara Voice itself, use Start in Chat & Agents › Voice. Or run it by hand:

```bash
cd ~/projects/amara-voice && ~/projects/ming-mac/.venv/bin/python -u amara_voice.py
```

`./amara start` works only after `./amara install`, which sets up LaunchAgent `com.thevisualdude.amara-voice`.
As of 2026-11-04 that LaunchAgent isn't installed and Amara Voice isn't running.

**Verify.** With Amara Voice running:

```bash
lsof -nP -iTCP:8791 -sTCP:LISTEN    # one listener on 127.0.0.1
cat ~/projects/amara-voice/logs/state
```

**Roll back.** Start Amara Voice with `AMARA_LEVEL_FEED=0` in its process environment. Don't put it in `.env`: `level_feed.py`
reads the variable before `.env` loads. The voice loop keeps running, the level meter stays at zero, and state events
continue. The bridge stores nothing, so there is no data to restore. The code is in commits `f7eea69` and `f5a2e25`.

## 7. Done: Hermes route patch

`hermes-amara-passthrough.patch` is applied to `~/.hermes/hermes-agent` (base `bf0203a794`). The gateway was restarted after it
was applied, so it is live. The changes are uncommitted edits in that checkout.

Hermes's reply now carries what the router reported: the `X-Amara-*` headers, the real `model`, and `amara` metadata.
Nexus reads these for the Fast/Deep badge. The badge has not been checked in the app with the patch applied.
Details: `~/projects/hermes-patches/README.md`.

**Before `hermes update`,** revert the patch. After the update, run `git apply --check`, then re-apply.

**Verify.** Run `pytest tests/agent/test_amara_route.py` in `~/.hermes/hermes-agent`, then send one real turn with `curl -i` to `:8642`.

**Roll back.**

```bash
cd ~/.hermes/hermes-agent
git apply -R ~/projects/hermes-patches/hermes-amara-passthrough.patch
launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway
```

Hermes then sends no route data, and Nexus goes back to `routes.jsonl` and the `/amara/route` dry run for the badge.
Section 5 doesn't remove this patch.

## 8. Status and order

- The local tracking ref `origin/nexus-v2` is at `62ce254` (read 2026-11-06, no fetch run). `nexus-v2` is ahead of it by design: nothing is pushed until the project is near 90%. `amara-integration` has no branch on origin.
- Order of what's left is in `NEXT_SESSION_PROMPT.md`. Home control comes after the live checks. The Muse eyes-closed test and key rotation are last, then the push.
- Lid-awake is installed (AC power only). Its scripts are in `~/projects/hermes-patches/lid-awake`.
- The rotation runbook `stack/ROTATION-RUNBOOK.md` stays out of git on purpose. It's in `.git/info/exclude`, along with `memory/session_logs/`.

## 9. Steps from 2026-11-11 (Claude Code, amara-integration)

Nothing here has been applied. Run each only when you want it. Merges and pushes stay frozen (`docs/decisions/0001`).

1. **Router: install the fixes and switch the fast model** (`bb6669a`, decision 0004).
   ```bash
   cp ~/.config/amara-router/config.toml ~/.config/amara-router/config.toml.backup-$(date +%Y%m%d%H%M%S)
   # In config.toml [models]: fast = "gemma4-12b-uncensored:latest"
   cd ~/projects/nexus-amara/amara-router && cargo build --release
   # install the binary the way amara-router/README.md › Install describes, then:
   launchctl kickstart -k gui/$(id -u)/com.taisun.amara-router
   curl -s http://127.0.0.1:8787/health
   ```
   New optional keys (defaults are fine): `deep_grace_secs = 60` (must stay ≤ `fast_wait_ms`/1000), `allow_remote_ollama = false`. Rollback: restore the backup and kickstart again. Watch `router.log` for the next `-9` exit (`amara-router/STABILITY.md`).
2. **GitHub secret name** (B14): confirm the repo secret is `APPLE_API_KEY_P8` (633cbb9 renamed it from `APPLE_API_PRIVATE_KEY`). A release now fails if `APPLE_API_KEY` is set without it.
3. **Hermes route patch** (B20): `~/projects/hermes-patches/hermes-amara-passthrough.patch` matches the live edits in `~/.hermes/hermes-agent` file for file (base `bf0203a794`, checked 2026-11-11). Run `pytest tests/agent/test_amara_route.py` there. Optional: `git init` `~/projects/hermes-patches` so patches are versioned.
4. **Amara Voice** (B18): checked 2026-11-11: LaunchAgent loaded, `amara_voice.py` running, 8791 listening. No step needed.
5. **Follow-ups dialog** (B08): every Lists edit that writes `~/.hermes/amara/*.json` now asks Allow/Deny. If that's too many dialogs, say which you prefer: one approval per session, or the app keeps its own copy and syncs.
6. **Hermes key** (B06): `API_SERVER_KEY` is 24 bytes and containers can reach `:8642` (they get 401). Rotate it to ≥32 bytes in the final rotation batch, after everything else.
7. **README nav names** (B19): `git apply tasks/proposed/README-nav.patch` fixes the Pages line in your uncommitted README edit.
8. **Guard hook backstop** (optional): Claude Code's Bash guard is a pattern check. A real backstop would be a `pre-push` hook in the shared `.git` or `chflags uchg` on old recordings; both would also affect you, so they're your call.
