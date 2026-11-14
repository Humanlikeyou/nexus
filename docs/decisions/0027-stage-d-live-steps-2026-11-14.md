# 0027: Stage D live steps, done by the agent (Taisun, 2026-11-14, in session)

Question: after Stage D wave 2 (a1d1cb5), who does the Taisun steps, and should the app be installed?

His words: **"Approved. You do these yourself Token enforcement: install and launch the new app once, then set require_launch_token = true and restart the router. Then apply the crew compose diff. 2. Voice token: apply tasks/proposed/amara-voice-token.diff. After that, the token gets switched on in the app. 3. Weaviate: log a decision first. [...] 4. Keychain moves: log each move you want in docs/decisions/. Installl and next"**

This is a one-time OK for these exact actions, overriding for them only the rules "never edit ~/projects/amara-voice", "~/projects/nexus is read-only for agents" and "router restart is a Taisun step":

1. Voice: back up and apply `tasks/proposed/amara-voice-token.diff` to `~/projects/amara-voice`, run its tests, restart the amara-voice LaunchAgent (kickstart only, no plist change), then set `LEVEL_SEND_LAUNCH_TOKEN = true` in the app. The server switch `AMARA_VOICE_REQUIRE_LAUNCH_TOKEN` stays off (it needs a LaunchAgent environment edit, which this OK does not cover).
2. Install the app built from the commit that carries step 1, launch it once so it mints the token.
3. Router: build from `amara-router/` at that commit, back up and replace `~/.local/bin/amara-router` (README install steps), set `[security] require_launch_token = true` in `~/.config/amara-router/config.toml`, restart with `launchctl kickstart -k`. Rollback: the backup binary and the config line back to false.
4. Crew: apply `tasks/proposed/crew-launch-token-compose.diff` to `~/projects/nexus/stack/docker-compose.yml` (backup first), `docker compose up -d crew` (no image rebuild). Known limit: the live crew image builds from `~/projects/nexus` (nexus-v2), whose `main.py` predates the launch-token credential, so the new env and mount are inert until amara-integration lands there (merges stay frozen, decision 0001).
5. Weaviate and Keychain moves: logged in 0028 and 0029.
