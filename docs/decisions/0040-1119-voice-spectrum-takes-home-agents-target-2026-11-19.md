# 0040: 1119 handover items 1, 3, 4, 7, 9 (Taisun, 2026-11-19, in session)

Context: Claude checked the project against Taisun's 1119 handover prompt and reported, item by item, what was done and what was open. Item 1 (a real voice spectrum in the sand lens) could not be built in the app alone: Amara's speech is played by the separate amara-voice process, so the webview never holds the audio. Changing amara-voice was off limits under the earlier rules.

His words (verbatim): **"1. I'm giving you permission to change the voice in how you see fit to accomplish the goal 3. get it done 4. Get it done 7. Get it done 9.get it done"**

1. **Voice spectrum (item 1).** Claude may change `~/projects/amara-voice`, for this goal only. The approach Claude chose:
   - amara-voice measures the 2 to 3 strongest spectral peaks (80 to 4000 Hz) of each block it plays, after the volume gain. This is the same signal the speakers get.
   - The peaks travel in the existing loopback level frame (`ws://127.0.0.1:8791/level`) as an optional `peaks` field, sent only while speaking.
   - The app's voice bridge validates them, and the sand lens uses them in place of the loudness fallback. With no peaks, the lens behaves exactly as before.
   - No audio, no text and no new port or host. Every edited amara-voice file gets a dated `.bak` copy first (that folder is not a git repo).
2. **Take generators for Agents and Smart home (item 3):** build them.
3. **Live Smart home sliders and scenes through the ha module (item 4):** build them. Sample names stay until real devices are linked.
4. **Agents "who's working on what" live feed (item 7):** build it.
5. **Clean the debug and incremental files in `target/` (item 9):** approved. They are regenerable build caches, removed only when no build is running.

Everything else stands: local commits only, outbound off in tests, no Muse recordings, memory/ or ~/.hermes touched, and nothing is pushed.
