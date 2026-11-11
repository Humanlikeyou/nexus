# HANDOFF-1111x: hidden voice-only dark look

Approved 21:47 JST, 11 Nov 2026. UI only, in `src-console` (plus e2e and this file). No src-tauri changes.

## Taisun's picks (these override the mockups and the original brief)

- Transition **C** (glitch, 1.4 s). Reduced motion is a **200 ms** crossfade of the palette and the orb colour.
- The voice phrase **"Amara, go dark."** turns it on. "Amara, back to normal." turns it off.
- Accent shifts off emerald to a **cool steel blue**, so red is only Amara's light. Contrast ≥ 4.5. Errors stay **amber**.
- The only exit besides voice is a **long-press on the red orb** (1.5 s, then "Back to normal?").
- **Nameless:** no visible label for the look anywhere.
- It does **not** persist. It resets to normal on app restart.

Typed chat uses the same intent path as voice (one write path, as in 1111v / 1111w). The phrase still has to match.

## 8 · Built (branch design-build-1111x)

**What was built**

- `src-console/navyMode/`: in-memory store (the live flag is never written to disk), phrase parser, Wired adapter for local TTS / local-only enforcement, session-only `<NavyRoot>`, Transition C glitch overlay, orb long-press, steel-blue + amber + red-lens tokens.
- Sample lines rotate and never name the look. Sample data is tagged. She speaks only when `openedBy === "voice"` and the adapter can speak; in live mode the adapter is UNWIRED (the caption still shows).
- Sample-mode triggers: type the phrase in chat, or set `localStorage.nexus.navyPreview` (`on` / `tear` / `flash` / `cut` / `holding` / `confirm`) for tests and captures. That key is a test hook, not persistence.
- Wired into `App.tsx` (`handleNavyPrompt` before window prompts; `useVoiceNavyPrompts` skips the first historical voice poll).

**Deviations**

- Typed chat shares the same write path as voice.
- Standard exit is an **800 ms** fade (200 ms if reduced motion), not a reverse glitch.
- `--accent` is the steel **fill** `#35617C` so on-accent text `#E8EEF8` stays 5.72:1. The cool steel `#8FB8D0` is `--accent-strong`, `--tone-accent-text`, and `--focus-ring` (8.86:1 on `#0B1220`). Success/healthy can stay mint; it is not the accent.
- Light-theme shell chrome (sidebar, toolbar, wordmark, search, status) is forced to navy so contrast holds.

**Backend needs (no src-tauri in this build)**

- `navyAdapter.speak(text)` — local TTS of the line when he asked by voice.
- `navyAdapter.localOnly()` — enforce that nothing leaves this Mac while the look is on.
- Both are UNWIRED in live mode and return sample values only when `nexus.sampleAdapters=1`.

**Gates** (worktree, before install): see `/workspace/nexus/build-1111x/README.md` for the commit, install time, pid, and rollback after the app lands.
