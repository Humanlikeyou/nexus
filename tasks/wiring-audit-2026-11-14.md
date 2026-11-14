# Wiring audit, classes A and B (and C briefly), re-checked against current code

Date 2026-11-14, HEAD 8a05492 (branch amara-integration). Read-only audit; this is the only file written. Source of truth is the code. It re-checks every row of `tasks/wiring-inventory-2026-11-12.md` (plus its 2026-11-13 status table).

Status key: DONE (live now), OPEN-wireable (code only, work described), OPEN-data-gap (no data source exists), OPEN-gate (needs a decision or Taisun step), OPEN-Design (needs new markup; see `tasks/design-questions.md`).
Cond. rows (text only while a store has not loaded) are counted as DONE.

## Summary counts

| Class | DONE | OPEN-wireable | OPEN-data-gap | OPEN-gate | OPEN-Design | Total |
|---|---|---|---|---|---|---|
| A (16 in inventory) | 13 | 0 | 0 | 1 (habit day marks) | 2 (Dates Edit, Life take empty block) | 16 |
| B (12 in inventory) | 5 | 6 | 1 | 0 | 0 | 12 |
| C (inventory says 16, table actually lists 21) | 4 | 0 | 8 | 9 | 0 | 21 |
| Extra rows found by this audit (not in inventory) | 0 | 4 | 1 | 2 | 0 | 7 |

Remaining "Needs wiring" in `src-console` (non-test): 93 lines containing the string (standalone 30, screens 11, money 9, personal 9, takes 7, adds 7, homeWindows 5, hrTake 3, neuro 3, inbox 2, fitness 2, lib 2, life 2, ui 1; most are conditional error/fallback copy, comments, or disabled-button reasons). 84 lines render `<DataTag kind="wire">` (standalone 21, screens 19, personal 10, money 9, takes 8, homeWindows 4, amaraTake 2, ui 2, inbox 2, life 2, hrTake 1, neuro 1, fitness 1, adds 1, goals 1). 486 lines mention UNWIRED (crews 48, takes 46, screens 83, money 40, standalone 39, homeWindows 35, personal 31, neuro 31, goals 23, business 16, amaraTake 15, hrTake 14, adds 14, inbox 13, navyMode 12, decisions 10, lib 7, fitness 6); nearly all are adapter definitions or the `unwiredX` base objects, not standing gaps. Live adapters still returning unwired for a whole method: see the table of rows below (personal.money / transcribe / pendingMemories / decideMemory; money.take / followUps / answer / undo / reconnect / disconnect; standalone.serviceLogs / serviceAction / updates / setUpdatePrefs / whoop / whoopAction / pairSteps / profile / tiers / setupDownload / restorePreview / restore / setupDone; homeWindows.comment / speak; recordings.removeConfirmed / liveAux; business (all, no ledger.json); crews.roster / n8n; navy.speak).

## Rows wrong or stale in the inventory

1. Home > Crew approvals prefetch: not an open row. `needs_you_list` already returns the crew source (`src-tauri/src/commands/needs_you.rs:236-243`, from `crew_list_approvals`), and the hook prefers it (`inbox/NeedsYouToday.tsx` `fromAgg("crew")`). `crewApprovals()` (`NeedsYouToday.tsx:18`) is only the fallback if `needs_you_list` fails, and returns UNWIRED outside sample mode on purpose.
2. Home windows: inventory says "NO live adapter exists". Wrong now: `homeWindows/adapters.ts:73-82` has `liveAdapter` (embedding live, signedSites, signOut); `homeWindowsAdapter()` picks it when `liveAvailable()` (adapters.ts:85). Landed in c024eeb (+710863c, 2c4a8a8). Only `comment` and `speak` are still unwired (adapters.ts:81-82).
3. Settings > Notifications relay: inventory says C/unwired. Now live: `standalone/liveAdapter.ts:198-210` (`relay`, `setRelay`, `relayPreview`, `relaySendTest`, a783a5f). Still blocked at send time by `NEXUS_OUTBOUND_DISABLED` and navy mode, by design.
4. Settings > Signed-in sites: live (`home_window_sites` / `home_window_sign_out`, `homeWindows/adapters.ts:74-80`). The "Needs wiring" copy at `SignedInSites.tsx:39` shows only when unwired.
5. Settings > Memory: reads, edit, delete (clears the block) and undo are live (`personal/adapters.ts:394-399`, memory_list b438574, edits 8af876a, decision 0014 #1). Inventory said "no Rust command reads Letta".
6. Weekly review, Patterns: live (`personal/adapters.ts:392-393` via `patterns_read` / `weekly_review_read`; 8af876a, d6c44c0). There is no weekly job: both compute on read.
7. Counts: inventory C count "16" is 21 actual rows; the 12 in B includes `memory_list`, `weekly_review_read`, `patterns_read`, `money_balances_list` and `backup_verify_last`, all now built.
8. Home > Sunday card: new tag bug, not in inventory. See row X1.
9. Mind-Body > Neuro history > Keep raw recordings tag: already fixed (`RecordingsPanels.tsx:74` calls `wiredTag(state)` and `state` is the live `retention()` result, so no tag when live). Removal itself stays blocked (decision 0010).
10. Several copy strings are now stale but are Design's: `CrewsScreen.tsx:299` ("Next scheduled run per crew needs wiring": next run is live for the tile, per-crew run times are not), `NotificationsView.tsx:47` ("once the relay is built"), `TodaySections.tsx:134` mic note, `DatesView.tsx:92` "(HANDOFF-1111a)".
11. `restore_preview` and `backup_prune_plan` are registered in `lib.rs:329-330` but are not bound in `bindings.gen.ts` (the only `restore_preview` mentions are `test/commands.registered.test.ts:86`). They are not callable from the renderer yet.

## Class A rows

| # | Area | Item | Status | Evidence | Work needed | Files | Design file | Size |
|---|---|---|---|---|---|---|---|---|
| A1 | Home | Crew approvals prefetch | DONE | `needs_you.rs:236-243`; fallback only `NeedsYouToday.tsx:18` | none | n/a | n | n/a |
| A2 | Home | Lease-notice tag (`HomeWindows.tsx`) | DONE | `HomeWindows.tsx:83,110` tag is now `unwired &&` (set from the parse result), 4e5ad29 | none | n/a | n | n/a |
| A3 | Life | Dates Edit button | OPEN-Design | `life/DatesView.tsx:108` OffButton; `life_date_upsert` ready (`lib.rs:250`, no delete); design-questions 2026-11-13 | edit form (title, lane, date, reminder) needs new markup; submit maps 1:1 to `life_date_upsert` | `life/DatesView.tsx` + new form | y | M |
| A4 | Life | Today's one thing empty block (life page) | OPEN-Design | `takes/suggestions.ts:26` `SUGGEST_PAGES` has no "life"; `takes/TakeExtras.tsx:34` | add catalog copy for "life" or hide the block; catalog copy is Design's | `takes/suggestions.ts`, `takes/TakeExtras.tsx` | y | S |
| A5 | Life | Personal money: yen USD from stored rate | DONE | `money/live.ts:44-45` (`mapAccount`, rate from `fx_rate_get` / `money_balances_list.fx`); 9b95d68, 1c93d5e | none. BTC/ETH USD stays data-gap (row B-extra M-D) | n/a | n | n/a |
| A6 | Life | Personal money overview tiles / totals | DONE | `money/live.ts:143` + `money/overview.ts`; b438574 | buffer months and savings rate still null (`overview.ts:100-101`), see B3 | n/a | n | n/a |
| A7 | Life | Connect routes jp_bank + ledger | DONE | `money/live.ts:107-142` (`money_import_pick`, `money_account_add`, `money_watch_add`, `money_balance_set_manual`); 1af2316, a783a5f | "Balance now" field removed by parent (Design, design-questions 2026-11-13) | n/a | n | n/a |
| A8 | Life | Weekly review "Try it this week" | DONE | `personal/adapters.ts:404`, `tryChangeWith`; f45829a (+ `experiments_record_start`) | none. Experiment results have no markup (design-questions 2026-11-14) | n/a | n | n/a |
| A9 | Life | Connected accounts tag | DONE | `money/MoneyView.tsx:55` `wiredTag(w)`; 4e5ad29 | none | n/a | n | n/a |
| A10 | Mind-Body | Recordings retention tag | DONE | `screens/RecordingsPanels.tsx:74` | none | n/a | n | n/a |
| A11 | Agents | Schedules "Next run" | DONE | `crews/liveSchedule.ts:11-36` (`list_terrence_automations`); 1af2316 | none | n/a | n | n/a |
| A12 | Agents | Runs table metrics tag | DONE | `screens/CrewsScreen.tsx:255-264` no tag, columns present; 4e5ad29 | none | n/a | n | n/a |
| A13 | Settings | Services hero tag | DONE | `standalone/ServicesView.tsx:66` tag only in the `!v` branch | none | n/a | n | n/a |
| A14 | Settings | Sidebar services summary | DONE | `standalone/liveAdapter.ts:159` (`summaryFromServices`); 1af2316. `updateReady` stays false (updater not built, `servicesSummary.ts:7`) | none | n/a | n | n/a |
| A15 | Settings | Backup last (time, size, checksum) | DONE | `standalone/liveAdapter.ts:70-80` (`mapLast`), `verifyLast`; `backup_verify_last`; 1c93d5e | none | n/a | n | n/a |
| A16 | Life | Habit day marks (Goals) | OPEN-gate | `personal/adapters.ts:271` days always "none", note "No data source..."; daily table exists (5994114) but what counts as a met day is undecided | Decision needed from Taisun: the rule per habit (check-in tag, NOOP metric, session) before computing marks from `daily_*` | `personal/adapters.ts` (+ a Rust read if the daily table is used) | n | M |

## Class B rows

| # | Area | Item | Status | Evidence | Work needed | Files | Design file | Size |
|---|---|---|---|---|---|---|---|---|
| B1 | Home / Life | `weekly_review_read` | DONE | `lib.rs:260`, `personal/adapters.ts:393`; 8af876a, d6c44c0 | none (computes on open of the app, no job) | n/a | n | n/a |
| B2 | Mind-Body | `patterns_read` | DONE | `lib.rs:259`, `personal/adapters.ts:392` | none | n/a | n | n/a |
| B3 | Life | `money_summary_read`: bills, subscriptions, month spend (`personal.money`, buffer months, savings rate) | OPEN-wireable (partial data-gap) | `personal/adapters.ts:96` `money: () => UNWIRED` (live adapter never overrides it); `PageViews.tsx:89,97`; `money/overview.ts:100-101` | New read-only Rust command over stored statement rows (`money_import_pick` already stores them): bills due, recurring, month spend. Needs a rule for "recurring" (state it in code, show nothing under 2 months of data). Then map in `makeLivePersonal` and feed `overview.bufferMonths/savingsRate`. Data exists only for the Japanese bank CSV account | new `src-tauri/src/commands/money_summary.rs` (or money.rs hunk), `money/overview.ts`, `personal/adapters.ts` | y (logic-only in `PageViews.tsx` none expected) | L |
| B4 | Life | `money_balances_list` | DONE | `money/live.ts:79,85-97`; b438574 | none | n/a | n | n/a |
| B5 | Life | `money_facts_read` + money take (`money.take`, `answer`, `undo`) | OPEN-wireable | `money/live.ts` spreads `unwiredMoney`; `MoneyView.tsx:132,153` | Facts command from accounts + overview (+ B3 spend), take text via router like `amaraTake/generate.ts`, suggestions from the reviewed list (money rules text in `MoneyView.tsx:223`). Depends on B3 for spend facts; accounts-only take is possible first | new Rust fact read, `money/live.ts`, maybe `amaraTake/` | n | L |
| B6 | Life | `dictate_once` (check-in mic, `personal.transcribe`) | OPEN-wireable (unsure) | `personal/adapters.ts:95,154` (live uses base unwired); mic button `adds/Composer.tsx:139` is `disabled` | On-device one-shot STT command reusing the voice bridge (`~/projects/amara-voice`, live, non-git: propose a patch under `tasks/proposed/`, do not edit). Enabling the mic button is logic-only. No audio stored. Mic permission (TCC) is a Taisun step | new `src-tauri/src/commands/dictate.rs`, `tasks/proposed/`, `personal/adapters.ts` (one hunk), `adds/Composer.tsx` | y (logic-only: remove `disabled`; copy untouched) | L |
| B7 | Mind-Body | `hr_zone_minutes` (Fitness Effort zones) | OPEN-data-gap | `screens/fitnessRingsModel.ts:90` `zones = UNWIRED`; `FitnessRings.tsx:136` says NOOP stores the day's Effort score only | No per-day HR history in NOOP. Only `hr_window` (recent minutes) exists. Needs a stored HR stream (Muse/WHOOP live HR logger) first | n/a | n/a | n/a | L |
| B8 | Settings | `whoop_status` (read-only: last sync, owner, gaps, store) | OPEN-wireable | no `whoop_status` in `lib.rs`; `WhoopPanel.tsx:30,53,70-90` falls back to `data.noop` battery and `generatedAt`; decision 0014 #2 approves the takeover work | Read-only status command (last sync from NOOP, owner from a lock file if present, gaps from NOOP) and `whoop()` in `liveAdapter.ts`. Takeover actions are separate (X6) | new `src-tauri/src/commands/whoop_status.rs`, `standalone/liveAdapter.ts` | n (adapter only; `WhoopPanel.tsx` unchanged) | M |
| B9 | Settings | `service_restart(id)` | OPEN-wireable | `standalone/liveAdapter.ts` has no `serviceAction`; `lib.rs:213-215` `check_docker_service`, `restart_docker_service`, `restart_services` exist | Map system-map node ids to an allowlist of docker/launchd names; loopback only; native approval dialog like other writers; adapter `serviceAction` | new `src-tauri/src/commands/service_control.rs`, `standalone/liveAdapter.ts` | n | M |
| B10 | Settings | `service_logs(id)` | OPEN-wireable | `standalone/adapters.ts:137` `serviceLogs: () => UNWIRED`; no command | Read-only log tail per allowlisted service with secret redaction (reuse the redaction used for logs elsewhere) | new `src-tauri/src/commands/service_logs.rs`, `standalone/liveAdapter.ts` | n | M |
| B11 | Settings | `backup_verify_last` | DONE | `lib.rs:328`, `liveAdapter.ts:79-92`; 1c93d5e | none | n/a | n | n/a |
| B12 | Settings | `memory_list` | DONE | `lib.rs:501`, `personal/adapters.ts:394`; b438574 | none | n/a | n | n/a |

## Class C rows (brief)

| # | Area | Item | Status | Evidence | Note |
|---|---|---|---|---|---|
| C1 | Home | Needs you > Decisions row | OPEN-data-gap | `needs_you.rs:191-192` returns "unwired" when `read_business_ledger` is None; `inbox/needsYou.ts:19` | needs Taisun's `business/ledger.json` (decision 0005) |
| C2 | Home | Home windows webview | DONE | c024eeb, `homeWindows/adapters.ts:73` | live; hosts approved by 0014 #5 |
| C3 | Life | Chime / Fidelity | OPEN-gate | `money/adapters.ts:75-76` `ready: UNWIRED` | Plaid not signed up (decision 0008 #3) |
| C4 | Life | Disconnect / reconnect | OPEN-gate | `money/live.ts` leaves them unwired | no-delete rule; reconnect needs Plaid |
| C5 | Mind-Body | `recordings.liveAux` | OPEN-gate | `neuro/training.ts:36` `AUX_PROVEN = false`; `recordings.ts:145` | eyes-closed test pending, Taisun |
| C6 | Mind-Body | `recordings.removeConfirmed` | OPEN-gate | `neuro/recordings.ts:144`; decision 0010 | no-delete, Muse recordings are sacred |
| C7 | Enterprise | Ventures card | OPEN-data-gap | `business/adapters.ts:25`; `liveBusiness` needs ledger | ledger.json absent |
| C8 | Enterprise | Overview Decisions link card | OPEN-data-gap | same | same |
| C9 | Enterprise | Revenue target tile | OPEN-data-gap | same | same |
| C10 | Enterprise | Venture page (checklist, milestones, budget, next steps) | OPEN-data-gap | same | same |
| C11 | Enterprise | Decisions > Waiting, vote rules | OPEN-data-gap | `EnterprisePages.tsx:55`; decisions log itself is live (`decisions/log.ts`) | ledger.json |
| C12 | Enterprise | Finances > Venture funds | OPEN-data-gap | `business/adapters.ts:26` NEEDS_CONVERSION; `RevenueScreen.tsx:40` | ledger.json; once present, convert with `fx_rate_get` (S) |
| C13 | Enterprise | "Draft with crew" and next-step buttons | OPEN-gate | `BusinessPanels.tsx:158,160` | asks-before-sending flow and copy: Taisun plus Design |
| C14 | Agents | n8n tile and card | OPEN-data-gap | `crews/adapters.ts:28`, `CrewsScreen.tsx:236,302` | n8n API key file in app data absent |
| C15 | Settings | Updates | OPEN-gate | `standalone/adapters.ts:137-139`; no updater crate in `Cargo.toml` | decision 0014 #4 approved; needs Developer ID, notarization, updater signing key (Taisun's steps) |
| C16 | Settings | WHOOP actions (sync, buzz, forget, take over, lucid, pairing) | OPEN-gate | `WhoopPanel.tsx:25,59,102` | decision 0014 #2 approved; takeover touches the real strap and store (graceful handover, copy + verify, lock): Taisun's steps, no kill -9 |
| C17 | Settings | Notifications relay | DONE | `liveAdapter.ts:198-210`; a783a5f | live; send blocked by outbound switch and navy mode |
| C18 | Settings | Signed-in sites | DONE | `homeWindows/adapters.ts:74-80` | live |
| C19 | Settings | Restore from a Backup | OPEN-gate | `standalone/adapters.ts:138` `restore: never`; `unpack_backup` in `bindings.gen.ts:296` | overwrites data; needs Taisun go-ahead. Preview part is wireable (X4) |
| C20 | Settings | Memory writes | DONE | `personal/adapters.ts:398-400`; 8af876a | live with approval dialog and backup |
| C21 | Settings | Run setup again (first-run sheet) | OPEN-gate | `standalone/FirstRun.tsx`, `App.tsx:115` auto-open gate; `get_system_info` has only os, arch, hostname, uptime (`system.rs:46-52`) | tiers need a signed model manifest, downloads are outbound (standalone phases, Taisun); `setupDone` would auto-open the sheet at launch (decision). Profile alone: X5 |

## Extra rows found by this audit

| # | Area | Item | Status | Evidence | Work needed | Files | Design file | Size |
|---|---|---|---|---|---|---|---|---|
| X1 | Home | Sunday card shows "Sample data" tag on a LIVE weekly review, and "1 change to try" is hard-coded | OPEN-wireable | `inbox/NeedsYouToday.tsx:150` `{v ? <>... <DataTag kind="sample" /> ...` | tag only when `wr.state === "sample"`; show "1 change to try" only if the review has a suggestion (`weeklyReviewFrom` already carries it). Logic-only | `inbox/NeedsYouToday.tsx` (+ test) | y (logic-only) | S |
| X2 | Home | Home windows `comment` (local light-tier comment from on-screen text) | OPEN-gate | `homeWindows/adapters.ts:81` | reading the page text of a signed-in site into a model conflicts with 0014 #5 "nothing to chat/logs/Letta": decision needed | n/a | n | M |
| X3 | Navy / Home | `speak` (navy mode and Home windows TTS) | OPEN-wireable (unsure) | `homeWindows/adapters.ts:82`, `navyMode/store.ts:45`; no TTS command in `lib.rs` | local TTS command via the voice bridge (propose patch for `~/projects/amara-voice` under `tasks/proposed/`, do not edit); then `navyAdapter().speak` and `liveAdapter.speak`. Shares the voice work with B6 | new Rust command file, `navyMode/` adapter, `homeWindows/adapters.ts` | n | M |
| X4 | Settings | Restore preview (`standalone.restorePreview`) | OPEN-wireable | `restore_preview` registered `lib.rs:330`, `backup_schedule.rs:648`, not bound in `bindings.gen.ts`; `standalone/adapters.ts:142` unwired; UI `FirstRun.tsx:31,60` | `#[specta::specta]` + specta::Type on `RestorePreview`/`VerifyReport`/`RestoreGroup`/`Reenter`, bind, boundTable row, bridge fn; map to the UI `RestorePreview` (`standalone/adapters.ts:98`: madeOn, version, contents, keyFileFound) in `liveAdapter.ts`. Needs a chosen backup file id (from `backup_list`). Preview only, restore stays gated (C19) | `src-tauri/src/commands/backup_schedule.rs`, `bindings.rs`, `bindings.gen.ts`, `boundTable.ts`, `bridge.ts`, `standalone/liveAdapter.ts` | n | M |
| X5 | Settings | First-run `profile` (RAM, chip, FileVault) | OPEN-wireable | `standalone/adapters.ts:142`; `get_system_info` lacks these | new `system_profile` (hw.memsize, chip name, FileVault state); adapter only. Tiers/download stay gated | `src-tauri/src/commands/system.rs` hunk or new file, `standalone/liveAdapter.ts` | n | S |
| X6 | Settings | Memory "Amara wants to remember" (`pendingMemories`, `decideMemory`) | OPEN-data-gap | `personal/PageViews.tsx:158,180-182` | no source of pending memories (Letta has no queue); define one first | n/a | n/a | n/a |
| X7 | Life | Money take answer/undo/followUps | OPEN-gate | `money/live.ts` unwired; `MoneyView.tsx:207` | what a "money follow-up" is has no definition; decide with B5 | n/a | n | M |
| X8 | Life | BTC/ETH USD values | OPEN-data-gap | `money/live.ts:44` comment "BTC/ETH have no price source" | no approved price host (decision 0008); Taisun could allow one, else typed balances only | n/a | n | M |

## Hard-rule notes for any work below

Every new command needs `#[specta::specta]`, `specta::Type`, a `collect_commands!` line in `src-tauri/src/bindings.rs`, a `boundTable.ts` row, a bridge function or `invokeBound` use, and `UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml bindings`. Shared files (`lib.rs`, `commands/mod.rs`, `bridge.ts`, `boundTable.ts`, `bindings.rs`, `bindings.gen.ts`, `test/commands.registered.test.ts`) take small Edit hunks only, so lanes below must serialize those hunks (do them in one place at merge time, or let each lane add its own non-adjacent lines).

## Proposed work order for OPEN-wireable items (file-disjoint lanes)

Lanes are disjoint on feature files. They all touch the shared registration files above with one small hunk each; run them in parallel but have the parent apply or rebase those hunks, or give each lane a distinct hunk region and regenerate bindings once at the end.

| Lane | Items | Feature files (exclusive) | Design files (logic-only) | Size | Order |
|---|---|---|---|---|---|
| W1 Money reads | B3 (`money_summary_read`), then B5 (`money_facts_read`, money take) | `src-tauri/src/commands/money_summary.rs` (new) and money.rs read hunk, `src-console/money/live.ts`, `money/overview.ts`, `personal/adapters.ts` (the `money` method only) | none expected | L | B3 first, B5 after |
| W2 Settings services and system | B9 (`service_restart`), B10 (`service_logs`), B8 (`whoop_status` read-only), X4 (restore preview), X5 (`system_profile`) | new `service_control.rs`, `service_logs.rs`, `whoop_status.rs`, `system_profile.rs`; `backup_schedule.rs` (specta derives only); `src-console/standalone/liveAdapter.ts` (single owner of this file) | none | L | X5 and X4 first (smallest), then B10, B9, B8 |
| W3 Home inbox tag | X1 | `src-console/inbox/NeedsYouToday.tsx` and its test | `NeedsYouToday.tsx` (logic-only condition) | S | any time; independent |
| W4 Voice | B6 (`dictate_once`), X3 (`speak` for navy + Home windows) | new `dictate.rs`/`tts.rs`, `tasks/proposed/` patch for `~/projects/amara-voice` (never edited), `navyMode/*`, `homeWindows/adapters.ts`, `adds/Composer.tsx` | `Composer.tsx` (remove `disabled`; copy unchanged) | L | after W1 if it needs the one-line `transcribe` hook in `personal/adapters.ts` (W1 owns that file; do that single hunk after W1 lands) |

Not wireable now, for the parent to decide or ask Taisun: A3, A4 (Design), A16 (habit rule), B7 and C7-C12, C14 (data gaps: ledger.json, HR stream, n8n key file), C3, C4, C5, C6, C13, C15, C16, C19, C21, X2, X7 (gates), X6, X8 (data gaps).

Not run: nothing was executed (no gates, no app). All findings are from reading code, `git log` and the decision files.
