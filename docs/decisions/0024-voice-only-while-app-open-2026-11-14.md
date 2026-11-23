# 0024: Voice only while the app is open; fewer fillers; chime bug (Taisun, 2026-11-14, in session)

His words, after applying the speak and dictate patches: "when i say hey amara it chimes and then immediatly chimes out. so i can say anything and i feel like the filller lines she gives after i speak are a bit redundant and slows down the actually message coming in and i only want voice to work while the app is open and stops working when its closed. fix these then next".

1. **Bug:** the wake chime is followed straight away by the end chime, so nothing gets heard. It started after the patches, and the dictate patch's hook in `Ears.next_utterance` is the first suspect. Fix it at the cause with a corrective patch.
2. **Filler lines** ("one sec…" style acknowledgements spoken before the answer) are redundant and slow down the real reply. Turn them off, through config if there is a switch, otherwise with a patch.
3. **Voice only while Amara OS is open.**
   - The app writes a heartbeat file while it runs.
   - Amara Voice listens for the wake word only while the heartbeat is fresh. Closing the app (or the app dying) makes voice go quiet within seconds, and it resumes when the app opens again.
   - The LaunchAgent itself is NOT unloaded or edited (hard rule 5). This mechanism needs no LaunchAgent change. If Taisun later wants the process itself stopped when the app closes, that's a separate LaunchAgent decision.

amara-voice is a live, non-git folder that agents never edit. Every fix there is a patch under `tasks/proposed/` that Taisun applies, as with the speak and dictate patches.
