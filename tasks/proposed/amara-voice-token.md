# Amara Voice level feed: per-launch token (D9)

Patch: `tasks/proposed/amara-voice-token.diff` (level_feed.py + tests/test_level_feed.py). Nothing in `~/projects/amara-voice` was touched.

## What it does
- New env switch `AMARA_VOICE_REQUIRE_LAUNCH_TOKEN`, default off (feed behaves exactly as today).
- On: each connection must offer WebSocket subprotocol `amara-launch.<64 hex>` matching `<data dir>/run/launch_token` (mtime-cached, so a new app launch is picked up; `hmac.compare_digest`; missing/bad file = refuse, 403). Any `Origin` header is refused (already true today).
- Off or on, an offered `amara-launch.*` subprotocol is echoed back in the 101 (clients require it).
- Data dir: `$NEXUS_DATA_DIR` or `~/Library/Application Support/com.taisun.nexus`.

## Steps for Taisun (in this order; the order matters)
1. Back up: `cp ~/projects/amara-voice/level_feed.py ~/projects/_archive/level_feed.py.pre-token` and the same for `tests/test_level_feed.py`.
2. `cd ~/projects/amara-voice && patch -p1 --dry-run < ~/projects/nexus-amara/tasks/proposed/amara-voice-token.diff`, then without `--dry-run`.
3. `python3 -m unittest tests.test_level_feed` (expect 17 OK).
4. Restart amara-voice (LaunchAgent `com.thevisualdude.amara-voice`). The server now echoes the subprotocol but does not require it.
5. In Nexus set `LEVEL_SEND_LAUNCH_TOKEN = true` in `src-tauri/src/commands/voice_bridge.rs`, rebuild, confirm the level meter still moves.
6. Only then add `AMARA_VOICE_REQUIRE_LAUNCH_TOKEN=1` to the amara-voice environment and restart it.

Do not do step 5 before step 2 to 4: an unpatched server does not echo the subprotocol, and the app's websocket client then fails the handshake (the level feed goes quiet). That is why the app side is gated off.
