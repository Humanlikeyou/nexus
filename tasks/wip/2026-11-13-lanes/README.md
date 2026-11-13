# Parked lane work (2026-11-13 ~09:20 JST)

Four Sonnet lanes stopped mid-task on the account's weekly limit (resets Nov 18, 10:00 JST). This is their unfinished, **untested, unreviewed** work, saved so they can resume. It was removed from the tree so the tree stays green.

- `tracked.patch`: edits to existing files, against HEAD `ba9d45e`. Restore with `git apply tasks/wip/2026-11-13-lanes/tracked.patch`.
- `files/`: the new files, copied byte for byte. Restore with `cp -R tasks/wip/2026-11-13-lanes/files/. .`.

Which lane each file belongs to (from the lane briefs):
- N1 chat/voice adds: `src-console/adds/*`, `App.tsx`, `app/useConversation.ts`, `adds.rs`, `checkin.rs`, `llm_router.rs`(?)
- N2 nightly backups / restore drill: `backup_schedule.rs`, `backups.rs`, `goal_roadmap.rs` (busy check reuse)
- N3 class-A wiring: `money/ConnectSheet.tsx`, `MoneyView.tsx`, `money/adapters.ts`, `money/live.ts`, `money_import_pick.rs`, `money.rs`
- N4 Telegram relay: `notify_relay.rs`
- Shared: `lib.rs`, `mod.rs`, `test/commands.registered.test.ts`

The agents can be resumed (SendMessage) after the reset. They keep their context and will need to re-apply this first.
