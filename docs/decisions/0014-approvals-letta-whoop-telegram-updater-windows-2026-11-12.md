# 0014: Approvals: Letta writes, WHOOP takeover, Telegram sends, updater, Home windows (Taisun, 2026-11-12, in session)

His words: "yes approve letta, yes WHOOP takeover, Telegram sends, the updater and the Hom" (cut off; read as "the Home windows", from the list he was answering).

Approved, to build in a fresh session, each with a security review, tests and his own steps where noted:
1. **Letta writes** (Settings › Memory edit/delete/keep, and session summaries if wanted). Do the planned Letta upgrade for CVE-2026-77937 first (`docs/ops/letta-upgrade-plan.md`: backup → pinned patched tag → checks; his step to run), then writes through a reviewed command with a backup before any delete. Muse recordings and `memory/` files are still never touched.
2. **WHOOP takeover** (design-1111s step 5): `whoop_status`, a graceful handover from the legacy health app (never kill -9), copy + verify the health store, single-owner lock, old copy read-only for 30 days and deleted only with his OK. This was previously "out of scope tonight"; it is now approved.
3. **Telegram sends** (design-1111s step 6): the relay with `{enabled, kinds}`, token + chat in the Keychain, preview never sends, Send Test asks first, still blocked by `NEXUS_OUTBOUND_DISABLED` and navy mode. Consider routing Telegram chat through Hermes (intelligence gap, handoff §12–13).
4. **Updater** (design-1111s step 7): needs a Developer ID + notarization and an updater signing key (his steps, secrets never in the repo); install order backup → wait for crews/Muse → stop → swap → start → health check → roll back on failure.
5. **Home windows** (design-1111w §8): new outbound hosts for the sites he opens himself (no background browsing), Tauri multi-webview (`unstable`), per-site profiles in app data, sign-in pause, nothing to chat/logs/Letta. Still honours `NEXUS_OUTBOUND_DISABLED` and navy mode.

Still frozen (0001): merge into `nexus-v2`, push, PR.
