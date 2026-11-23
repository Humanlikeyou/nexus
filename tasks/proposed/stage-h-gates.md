# Proposed decision note: Stage H gates (Phases 13, 14, 15)

Status: PROPOSED by Claude Code, 2026-11-18. Not a decision. Only Taisun's own words go into `docs/decisions/` (next free number is 0033). Each question below has a recommended answer so a one-word reply is enough.

## Phase 13: Open at login

**Q13.1. Which mechanism?**
- A (recommended): `SMAppService.mainApp`, the modern login item. It shows in System Settings > General > Login Items, writes no LaunchAgent plist, and macOS may ask you to allow it once. Built, off by default.
- B: the old LaunchAgent plist (`tauri_plugin_autostart`). Not built any more (the plugin and its `autostart:default` permission were removed). Choosing B means re-adding the plugin and a plist in `~/Library/LaunchAgents`, which your rules forbid without approval.

**Q13.2. Confirm that the toggle (Settings > General > Startup) is Design's, and that it stays off until you turn it on yourself.**
Backend commands `login_item_status`, `login_item_enable`, `login_item_disable` exist. Nothing calls them yet.

## Phase 14: Signing, notarization, DMG

**Q14.1. Apple Developer ID Application certificate on this Mac.** `security find-identity -v -p codesigning` shows 0 today. You install it (Xcode > Settings > Accounts, or Keychain Access import). Claude never sees the certificate or its password.

**Q14.2. Notary credentials.** You run once, yourself: `xcrun notarytool store-credentials nexus-notary` (it asks for your Apple ID, team ID and an app-specific password, or an App Store Connect API key). They live in the login Keychain only. The `.p8` is never committed.

**Q14.3. B14: GitHub secret name `APPLE_API_KEY_P8`?** Only matters if CI releases. CI release needs a push, which is frozen (decision 0001). Recommended: local script only for now (`scripts/release-local.sh`).

**Q14.4. Version bump.** Version is still `0.1.0` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`. What version is the first signed release (suggest `0.2.0`)? Log it.

**Q14.5. Entitlements.** Built minimal: Bluetooth only (Muse). No sandbox, no `get-task-allow`, no `allow-jit` (WebKit's JIT runs in Apple's WebContent process). Confirm, or say if you want JIT added after a failed live test. First signed run must be checked with the real Muse and the bundled Ollama (hardened runtime can block an unsigned or differently signed helper).

**Q14.6. Architecture.** arm64 only (recommended, matches the sidecars), or also universal?

## Phase 15: Auto-update

**Q15.1. Where is the update manifest hosted?** A folder you control, or one private HTTPS URL. It becomes the only update host and must also be named in the `plugins.updater.endpoints` list. Updates are outbound, so they also respect `NEXUS_OUTBOUND_DISABLED`, Local only and Settings > Outbound.

**Q15.2. The updater key.** You generate it yourself: `npx tauri signer generate -w <path outside the repo>`. You keep the private key offline plus in your Keychain, and keep a backup (losing it means installed copies can never update). You give Claude only the PUBLIC key text to paste into `plugins.updater.pubkey`. Claude never sees or stores the private key.

**Q15.3. Flip the switch.** `UPDATER_SWITCH` in `src-tauri/src/commands/updater.rs` stays `false` until you say yes, logged. Also needs the real pubkey and endpoints.

**Q15.4. Defaults.** `auto_update` and `check_daily` are stored with default on (your earlier answer) but inert until configured. Confirm "on" is still what you want once updates are live.

**Q15.5. Real swap host.** The install order (backup, wait for crews and Muse, stop sidecars, swap, start, health check, rollback) is built and tested with a fake host. The real host that replaces the `.app` is not built; `update_install_now` refuses. Approve building it after Phase 14 works end to end (needs a vN to vN+1 test you run).

## What Claude will not do without these
Install or load a LaunchAgent, register a login item, replace `/Applications/Amara OS.app`, generate or store a real updater private key, add an outbound host, push, tag or bump the version.
