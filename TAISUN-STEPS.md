# Taisun's manual steps: Amara Router + Hermes

Only Taisun runs these. Nothing here has been applied. Full details: `amara-router/README.md`.
State checked 2026-11-04: no router installed yet (`~/.local/bin/amara-router`, `~/.config/amara-router/` and the
router plist are absent). Hermes runs as LaunchAgent `ai.hermes.gateway` with `model.base_url: http://localhost:11434/v1`.
No secret values belong in this file.

## 1. Back up first

```bash
TS=$(date +%Y%m%d%H%M%S)
cp -p ~/.hermes/config.yaml ~/.hermes/config.yaml.backup-before-amara-router-$TS
cp -p ~/Library/LaunchAgents/ai.hermes.gateway.plist ~/ai.hermes.gateway.plist.backup-$TS
# Only if a router plist already exists from an earlier try:
[ -f ~/Library/LaunchAgents/com.taisun.amara-router.plist ] && cp -p ~/Library/LaunchAgents/com.taisun.amara-router.plist ~/com.taisun.amara-router.plist.backup-$TS
ls -l ~/.hermes/config.yaml.backup-before-amara-router-* ~/*.backup-$TS
```

## 2. Install and load the router LaunchAgent

```bash
cd ~/projects/nexus-amara/amara-router
cargo build --release && cargo test
mkdir -p ~/.local/bin && cp target/release/amara-router ~/.local/bin/amara-router
mkdir -p ~/.config/amara-router && chmod 700 ~/.config/amara-router
cp config.example.toml ~/.config/amara-router/config.toml && chmod 600 ~/.config/amara-router/config.toml
ollama list    # model names in config.toml must match (fast/deep under [models])
~/.local/bin/amara-router --config ~/.config/amara-router/config.toml --check
lsof -nP -iTCP:8787 -sTCP:LISTEN    # no output = port free
mkdir -p ~/Library/Logs/amara-router
sed "s#__HOME__#$HOME#g" launchd/com.taisun.amara-router.plist > ~/Library/LaunchAgents/com.taisun.amara-router.plist
plutil -lint ~/Library/LaunchAgents/com.taisun.amara-router.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.taisun.amara-router.plist
curl -s 127.0.0.1:8787/health
```

Claude stays off: leave `[cloud] anthropic_enabled = false` and skip the secrets file unless you decide otherwise.

## 3. Point Hermes at the router

Edit only the `model:` block in `~/.hermes/config.yaml`:

```yaml
model:
  default: amara            # auto routing; amara-fast / amara-deep pin a tier
  provider: custom
  base_url: http://127.0.0.1:8787/v1
```

Keep the `/v1`: Hermes uses the OpenAI API, and Nexus detects the router by this URL. Leave the other keys as they are.
Restart Hermes: `launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway`

## 4. Verify

```bash
curl -s 127.0.0.1:8787/health                         # status ok, Ollama up
tail -f ~/Library/Logs/amara-router/routes.jsonl       # then send Amara a message: one line per model call
```

In Amara OS › Chat & Agents › Chat, a reply now shows a Fast/Deep badge and the Deep toggle is enabled.

## 5. Roll back

```bash
cp -p ~/.hermes/config.yaml.backup-before-amara-router-<TS> ~/.hermes/config.yaml
launchctl kickstart -k gui/$(id -u)/ai.hermes.gateway
launchctl bootout gui/$(id -u)/com.taisun.amara-router
rm ~/Library/LaunchAgents/com.taisun.amara-router.plist   # or restore your backup copy of it
```

The Hermes plist was never edited; restore `~/ai.hermes.gateway.plist.backup-<TS>` only if it changed.
