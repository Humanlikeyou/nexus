# 0012: Rebuild and reinstall Amara OS (Taisun, 2026-11-12, in session)

His words: "OK, you have my OK to rebuild and reinstall, etc."

Scope applied: build the app from `amara-integration` (HEAD in `~/projects/nexus-amara`) with `APPLE_SIGNING_IDENTITY=- npx tauri build --bundles app`, quit the running app, move the installed copy to `~/Backups/` as the rollback, install the new build with `ditto`. Taisun launches it himself.

Not covered (still frozen by 0001): merging into `nexus-v2`, any push, PR. "etc." is not read as an OK for those.

Second OK, 2026-11-12 ~18:30 JST: "when you're done with all that you have my OK to update and reinstall the app", then clarified: "once you're done wiring everything up and doing the goals thing … then you can rebuild and reinstall." So the next install waits until the wiring and decision 0013 (goals) are done. A bundle built at 2616099 was NOT installed.
