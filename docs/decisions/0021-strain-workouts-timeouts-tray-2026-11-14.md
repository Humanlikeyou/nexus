# 0021: Strain scale, workout window, take rating, timeouts, menu-bar icon (Taisun, 2026-11-14, in session)

His answers to the open questions after package 3 (HANDOFF §18):
1. **Strain scale 0–100: "yes".** The brief and patterns.rs stand. The daily table stores NOOP strain raw on that scale (the 7–11 values are test fixtures only).
2. **Workout minutes cover the last 90 days only: "yes"**, that's enough. The NOOP workout reader stays as it is.
3. **Take rating control goes to Design: "yes".** Logged in `tasks/design-questions.md` (2026-11-14).
4. **Timeouts on sends that had none: "sure".** These sites now use the shared builder with timeouts: telegram.rs (api.telegram.org: 15 s total, 5 s connect) and arm.rs (local ARM worker: 10 s, 3 s connect). Nothing else about those clients changed.

Also asked: **"fix the little icon at the top of my mac then install".**
- The menu-bar (tray) icon was the full-colour app icon, not marked as a template, so macOS showed a shrunken blob. `tauri.conf.json` also declared a second tray, which showed as a solid square.
- Now there is one tray, built in lib.rs from `icons/tray-template@2x.png` and marked as a template. That image is the Amara "Λ" mark only, black on transparent, 36 px = 18 pt @2x, cut from the app icon by colour (no new artwork), so macOS tints it for light and dark menu bars.
- Install was asked for in the same message.

Still frozen (0001): merge into `nexus-v2`, push, PR.
