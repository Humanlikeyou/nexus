# 0041: Waves approved, voice restart, blind Undo, Neuro waves, clean and install (Taisun, 2026-11-19, in session)

His words (verbatim): **"approved waves make sure with everynew goal a new wave is entangled. You restart amara voice. No show home, yes add undo, yes working now is good. dont forget to update the same waves on the neuro page also, for the wave the frequency should match the neuro section form the most recent neuro scan. but it should be smmooth and looping until a new session is done  install"**, then **"also you finish and run 9 yourself"** and **"then install"**.

1. **Milestones waves: approved** (b9c0298, from the screenshots). Every goal gets its own string; past the first four, none repeats (da0c560).
2. **Restart amara-voice:** Claude ran `~/projects/amara-voice/amara start`. That bootstraps the LaunchAgent `com.thevisualdude.amara-voice`, already installed in `~/Library/LaunchAgents` since 2026-11-04. Hard rule 5 ("don't load LaunchAgents") is lifted for this one action by his direct instruction. Nothing was installed or edited; voice was not running at the time. The level feed listens on 127.0.0.1:8791 with the decision 0040 code.
3. **Smart home take when Home Assistant is not connected: don't show it** (kept as built in 9d7e549).
4. **Blind Undo: yes.** Undo of a blind move sends the reverse move (the previous position, or close after open and back) through `ha_device_set`, which asks for the native approval again. Scenes still can't be undone.
5. **"Working now" (8b0478d): approved as built,** queued actions included.
6. **Neuro page waves:** the same vibrating-string look. The frequencies match the most recent Muse session's measured bands. The waves loop smoothly and stay the same until a new session finishes.
7. **Item 9: Claude runs it.** `cargo clean --profile dev` in src-tauri and amara-router. Then build and install the app (backup of the current app kept).
