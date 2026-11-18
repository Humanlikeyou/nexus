# Design notes, 2026-11-18 (decision 0033, Design items 1 and 2)

## 1. Light-theme contrast of `.m-warn b`
- Cause: `.m-warn b` used `--copper-text` (#8c6239) on the 10% copper tint over the light dialog surface (#ebe5dd as measured by axe): 4.28:1, needs 4.5:1.
- Fix: new token `--copper-text-on-tint` in `src-console/styles/tokens.css`. Light #835a33 (4.83:1 on #ebe5dd; at least 4.78:1 on #f2f2f7 and #f5f5f7 tints). Both dark blocks set it to `var(--d-copper-text)`, so dark is unchanged.
- `src-console/money/money.css`: `.m-warn b` uses the new token. `--copper-text` itself is untouched, so its other uses (page names, status glyphs, chips, bars) are unchanged.
- The e2e preview serves `dist/`, so run `npm run build` before Playwright or the old colour is measured.

## 2. Settings > General > Startup: Open at login
- New `src-console/settings/OpenAtLogin.tsx` (`StartupGroup`), used in `SettingsScreen.tsx` in place of the old Startup group (the "Set up Amara again" row is kept).
- Load calls `getLoginItemStatus` only. The toggle is disabled until status arrives, and off if the read fails. Checked = enabled or needs approval.
- On: `enableLoginItem` (the backend shows the native confirm). Off: `disableLoginItem`. The toggle is disabled while a call runs.
- Copy (footer of the group):
  - needs approval: "Allow Amara OS in System Settings > General > Login Items to finish."
  - `refused: not_in_applications`: "Move Amara OS to your Applications folder first, then try again."
  - unsupported: "Open at login needs macOS 13 or newer." (toggle disabled)
  - declined confirm: no message, stays off
  - anything else: "Couldn't change this setting. Try again." Raw text is never shown.
- Tests: `src-console/settings/OpenAtLogin.test.tsx` (9, bridge mocked); `SettingsScreen.test.tsx` switch order now includes "Open at login".
