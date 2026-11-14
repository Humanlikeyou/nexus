# Design questions for the Nexus Design Lead

Claude Code does not act on these. Each says where it came from.

- **Follow-ups (Lists) approval copy** (from B08, LANE-T): every write to `~/.hermes/amara/*.json` now shows a native Allow/Deny dialog. A Deny returns an error starting `declined:` and a concurrent dialog `busy:`. `ListsScreen` only maps the existing `ERR_*` kinds, so these show raw. What copy (if any) should Lists show for "you declined" and "another approval is open"? Claude Code has not changed ListsScreen.
- **Money tab as-of date** (from W12b ledger): `ledgerAsOf(ledger)` in `src-console/business/liveLedger.ts` returns the rate's as-of date (or null). Where/how should the Money tab show it?

## W12c Chunk C: recordings loading and gaps (frontend-executor, 2026-11-11)
- Detail view: a 30 s block loads asynchronously, so the first paint of a window (and after a scrub into a new block) reads as "Needs wiring" for a moment before the trace appears. Wanted: a loading state for the lanes, or keep as is?
- Gaps in a recording (dropouts, > 0.5 s) arrive as NaN in lane values and in the focus/calm/band series. `linePath`/stack polygons in RecordingsPanels.tsx and the lane drawing do not break lines at NaN, and "avg focus" over a series with a gap shows NaN. Wanted: how gaps should draw (break the line) and be averaged (ignore). No code changed.

## R13-D crew approvals (frontend-executor, 2026-11-11)
- A run waiting for an approval has status `awaiting_approval` ("Awaiting approval", wait tone). The run page reuses the running copy ("Running… checking every 5 seconds.") and keeps the Stop button for it; a denied run ends `cancelled` (off tone). Wanted: dedicated copy for a run that is waiting for you, and a link from the run page to its approval?
- Approve now opens the native "Approve Action" dialog; a Deny or an error shows as the card's existing status note. Wanted: how an error or a declined dialog should look.
- **Take chips → source** (item E): chips now jump to an existing element id. Neuro (sessions), Enterprise (revenue / ledger rows) and Fitness recovery/effort have no stable element ids yet, so those chips stay inert. Add ids where you want a target (e.g. `neuro-last-session`, `ent-revenue`, `fitness-recovery`) and Claude Code will map them.

## design-1111v live wiring (backend, 2026-11-12)
- Enterprise Tasks and Content show a just-saved item with the subtitle "Just added · sample data" even when it was saved for real (live). Wanted: copy for a live "Just added" row (and removing the sample label there). Not changed (copy).
- Tasks and Content still say "venture link needs wiring" although tasks and ideas now carry a venture id (`venture` on `read_tasks` / `read_content_pipeline` rows). Wanted: how the venture should show on a task or card.
- Dates & deadlines "Edit" stays disabled ("Needs wiring · life_date_upsert"): `life_date_upsert` accepts an id (edit) now, but the Edit flow is a Design call.

## 1111a panels label any value "◇ Sample data" (blocks live wiring) — 2026-11-12
`TodaySections.tsx` (Goals hero + Milestones card :29 :51; Weekly review :164 :179 :190 :193 :198), `PageViews.tsx` (Patterns :33 :41 :46 :56; Personal money :84 :85 :103 :107 :112; Memory :163) render `<DataTag kind="sample" />` whenever the adapter returns a value, so a live value would be labelled sample. `life/DatesView.tsx` and `standalone/BackupView.tsx` already do it right (`{x.state === "sample" && <DataTag kind="sample" />}`). Ask: make these tags state-conditional the same way (no visual change in sample mode). Until then Claude keeps goals / patterns / weekly review / money / memory unwired; Milestones already show the real goals.json through the unwired fallback card.
Also for Goals: habits added through the one add path have no `source`, so their days can't be counted from Fitness/Neuro/check-ins. Either the add preview asks for a source, or the panel needs a manual tick (handoff §1 allows it only for `source = manual`).

## 1111s Backup live (backend 2026-11-12)
- `backup_settings_*`, `backup_now`, `backup_list` are live. No nightly timer and no keep-14 prune exist (prune needs Taisun's OK, hard rule 7), so the "Every night" toggle and "keeps the last 14" are stored preferences only; the hero keeps its Needs wiring tag because `last` is null. Wanted: copy that says nightly/keep aren't running yet, or keep as is until the scheduler lands.
- `backups[].version` is "" (the archive name has no app version; the dialog shows "—"); `last` needs a stored checksum verify (not built); `BackupRun` done/total/left are "" (progress events carry phase + pct only).
- Files ≥ 50 MB (large Muse replays) are skipped and listed in the result, so a backup doesn't hold big raw EEG captures. Taisun's call whether backups should include them.

## takes-1111t money e2e: light-theme contrast (2026-11-12)
`test/e2e/takes-1111t.spec.ts:80` fails in light theme only, 3/3, on `0b2a312`, HEAD and the working tree alike (dark passes): axe `color-contrast` on `div[role="alert"] > b` in the Connect-an-account sheet after the seed phrase is refused. It passed in the 0b2a312 full run earlier the same day, so it is likely date/time-dependent content. Design-owned styling (money/ConnectSheet); not changed by Claude.

## 2026-11-13: light-theme contrast failure on the seed-phrase alert (real, not a flake)

`takes-1111t.spec.ts:80` [light] fails the axe check `color-contrast: div[role="alert"] > b`. Measured with axe on a stable page at 0, 300 and 1500 ms after the alert shows: **4.28:1 in light every time** (needs 4.5:1), 5.99:1 in dark.

The element is `.m-warn b` (`src-console/money/money.css:28`): `color: var(--copper-text)` (light `#8c6239`, `tokens.css:38`) on the `.m-warn` background `color-mix(copper-text 10%, transparent)` over the sheet.

Earlier sessions called this a flake because it sometimes passed. Today's probe shows a stable 4.28. Design's call, for example a darker light-theme copper for text on tinted fills, or no tint behind the bold line. Claude Code hasn't touched the CSS or tokens, and the test stays as it is.

## 2026-11-13: Life › Dates "Edit" needs an edit form (wiring blocked on Design)

`life/DatesView.tsx:108` renders Edit as an `OffButton` ("Needs wiring · life_date_upsert"). The backend `life_date_upsert(draft)` is ready (no delete). Wiring needs an edit sheet or inline form (title, lane, date, reminder), which is new markup. Design's call. Once a form exists, its submit maps 1:1 to `life_date_upsert`.

## 2026-11-13: Ledger watch-only "balance now" field (wiring ready, field is Design's)

`addWatchOnly(chain, value, nickname, balance)` saves an optional typed balance via `money_balance_set_manual`. Lane N3 added an input for it to `ConnectSheet.tsx`; the parent removed it because new markup is Design's. If wanted, a field like "Balance now, in BTC/ETH (optional, you type it)" under Nickname passes a number (or null) as the 4th argument.

## 2026-11-14: Take rating control (backend ready, no control calls it)

Decision 0020 asks to record whether a take was helpful. The backend and `recordTakeRating(page, takeId, helpful)` exist; no control calls it. Design: add a helpful / not helpful control on the take card (or say where).

## 2026-11-14: "This experiment can't be tracked" (Try it this week)

When "Try it this week" adds the follow-up but `experiments_record_start` fails, the experiment is not tracked and no result will appear in the weekly review. The reminder text no longer says so (a copy change is Design's call; the earlier suffix "(the result won't be tracked: couldn't save the experiment)" was removed). Design: whether to tell the person, and if so where and in what words (reminder line, a quiet note, or nothing).

## 2026-11-14: Weekly review "Tried this week" results (backend and adapter ready, no markup)

`weekly_review_read` now returns `experiments` (results of "Try it this week" experiments whose 7 days ended this week: label "better" / "worse" / "no clear difference", before and after means, the days behind each). `weeklyReviewFrom` passes it through as `WeeklyReview.experiments`; no screen shows it. Design: where in the weekly review a result goes, and how "no clear difference" and its reasons (not enough days, no measure named, not significant) read.

## 2026-11-14: Goal progress readout (backend and bridge ready, no adapter method, no markup)

`goal_progress_week(weekEnd?)` (bridge: `goalProgressWeek`) returns, from the daily table and published patterns only: roadmap steps finished per goal this week and the week before; goals with no step finished in the last 21 days (last finished day, or null for never); and "what the days before finished steps had in common" (only causes with a published generic finding that beat chance; otherwise `nothing_stands_out_yet`). Every item lists its `daily_goal:` / `daily:` refs. No adapter method exists for it, so nothing is wired. Design: where it shows (Goals screen, weekly review, or both), and how the three parts and the empty "nothing stands out yet" state read.

## C3 (2026-11-14): check-in reminder delivery and habit sources
1. The 22:00 JST check-in "reminder" today is only a quiet Needs-you pointer (`needs_you.rs` `CHECKIN_REMINDER`, row "Not done yet today · reminder at 22:00 JST"), shown all day while no check-in exists. Nothing fires at 22:00: the app has no notification plugin or toast surface. Ask: (a) accept the pointer as the reminder and show it only from 22:00 JST (logic-only, Claude can do it on a yes), or (b) a real surface (toast/banner/OS notification) when the app is open at 22:00, fired once per day, only if no check-in today. (b) needs new markup or a plugin decision.
2. Habits: still blocked on the 2026-11-12 entry above (no `source`/metric on habits). Mapping is clear once a source exists (check-ins -> `mood_mean` rows; Workouts -> `workout_min`; Muse -> `muse_min`; Hemi-Sync -> `hemisync_min`; sleep -> `sleep_total_min`, all in `daily_range`), but the add preview must ask for the source and metric first.

## 2026-11-14 (W1): Money take copy and money follow-ups (copy missing; plumbing built, take stays "Needs wiring")

The take catalog (`amaraTake/catalog.json`) has no `money` entries and `TakePage` has no `money`; HANDOFF-1111t §9.4 lists the suggestion order but not the take or follow-up wording, and the only money copy in code is sample text (`SAMPLE_MONEY_TEXT`). No copy was written. Built: `money_facts_read` (sourced facts, `partial` when any personal account is stale or unknown) and `money/facts.ts` (`moneyTakeGate`, `checkMoneyText`: banned words, no yen sign, NFA line, numbers only from sourced facts, no suggestion while partial). `take()`, `answer`, `undo` and `followUps()` stay unwired. Ask: (1) the take wording and which page it lives on (own `money` page in the catalog, or inside the `life` take, as the UI does today); (2) what a behaviour-only money follow-up is (saving rate over 3 paydays, annual fee estimate, buffer months, adherence) and its copy; (3) whether suggestions without a connected Fidelity/Chime account (no fund fees, no brokerage) can exist at all, since only a Japanese bank CSV and manual balances are wired.

## 2026-11-14 (W1): Live "Bills, subscriptions and spending" card is mostly empty

`personal.money` is now live only when this month's spend is real and a stored yen rate exists: it fills "Spent in <month>" (USD at the stored rate, with the rate and date in the note) and leaves Subscriptions, Bills due, Left after bills as "Needs wiring" and the Bills table, Subscriptions list and category list empty (a statement has no due dates or categories). `money_summary_read` also returns recurring debits (same description and amount in 2+ months), but they are not shown: rent and loans recur too, so calling them subscriptions would be wrong. Ask: a layout for the live variant (hide empty tables?), and whether a "Recurring payments" list (label, usual amount, last date) should exist.

## 2026-11-14 (W2): Settings copy that no longer matches live data

Wired live with no markup change, but four static strings now sit next to real values. (1) Services logs dialog: the "Sample data" tag shows whenever logs exist, including live ones, and the plain-words list is empty because the backend only has a redacted raw tail (no events are invented). (2) Services restart: a declined approval, `unsupported` (LaunchAgent services and containers without a verified name) or `needs_you:` (3 failed tries) all come back as the one note "restart needs wiring"; the UI needs distinct wording and a "Needs you" state for the third failure. (3) Devices: "Last sync" and "Recovery, sleep, effort" show fixed "Up to date" / "Current" badges, and the live card says "Not connected" because Amara has no strap link until the takeover stage; it needs a read-only variant of those rows. (4) First run: the FileVault and "about 20 minutes" lines keep a "Sample data" tag when the profile is live, and the restore preview has no file picker (it previews the newest backup), no "Amara version" source (shown empty) and no place for the "re-enter these keys" list the backend returns.

## 2026-11-14 · W4 · Composer mic copy once dictation is live
`src-console/adds/Composer.tsx` mic button is now enabled by logic (`disabled={!micReady || listening || busy}`) only when the patched Amara Voice announces dictation. Its aria-label and title still read "Dictate on this Mac (needs wiring: on-device transcription)" / "Needs wiring · voice is transcribed on this Mac only", which is wrong once enabled. Copy was left unchanged (no copy edits allowed). Needs from Design: an enabled-state label and title, plus a "listening" state (the button is disabled while recording, with no visual cue). The Check-in mic (`personal/TodaySections.tsx`) already had an enabled button and a fallback note, so it needs nothing.

## 2026-11-14 (D5): Settings toggle for "Send data off this Mac" (backend and bridge ready, no markup)

Outbound (Telegram sends, outbox delivery, research, notify relay, the Claude tier) is now OFF by default and no longer baked into `Info.plist`; it is enabled only from Settings. Built: commands `outbound_get` / `outbound_set(enabled)` (bridge: `getOutboundAllowed`, `setOutboundAllowed`). Turning it on opens the native approval dialog and the call rejects with `declined:` (Deny), `busy:` (dialog already open) or `refused:`; turning it off is immediate and never asks. No screen calls them, so today a person cannot turn outbound on from the UI. Ask Design: (1) where the toggle lives (Settings > Assistant next to "Allow Claude", or a Privacy/Network row); (2) its label and plain-words explanation, including that it is off by default; (3) the states it must show beyond on/off: forced off by Local only (navy), forced off by the `NEXUS_OUTBOUND_DISABLED` environment variable (the saved choice can read "on" while the app still sends nothing; the backend has no command for the effective state yet, say if you want one), and "approval declined"; (4) whether the first-run flow should mention it.

## 2026-11-14 (D7): "Lock down the data folder" action (backend built, gated, no markup)

Commands `data_dir_harden_status` (dry run: counts of folders/files that would become 0700/0600, plus skipped protected folders, symlinks and not-ours entries; no paths) and `data_dir_harden` (applies it after a native approval; Deny changes nothing) exist and are bound in `boundTable.ts`. `data_dir_harden` is hard-disabled by the constant `DATA_HARDEN_ENABLED = false` (Taisun said "not yet"), so today it returns `disabled:`. `replays/`, `sessions/` and `muse/` are never touched. No screen calls either. Ask Design: where the action lives (Settings > Privacy?), the explainer copy (what changes, what is never touched, nothing deleted), how the dry-run counts are shown, and the disabled/declined/done states.
