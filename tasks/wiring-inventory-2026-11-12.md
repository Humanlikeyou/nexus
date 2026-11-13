# Wiring inventory: "Needs wiring" in the LIVE app (inside Tauri, sample mode OFF)

Date 2026-11-12. Read-only audit of `src-console` against `src-tauri/src/lib.rs` `generate_handler!` (about 245 commands). Source of truth was the code, not the older `tasks/HANDOFF-FOR-REVIEW.md`. Nothing but this file was written.

## How the live app picks adapters (what is live vs unwired)

| Picker | Live in Tauri? | Methods still `UNWIRED` in the live adapter |
|---|---|---|
| `personalAdapter()` (`personal/adapters.ts:299`, `makeLivePersonal`) | partly | patterns, transcribe, money, memories, pendingMemories, editMemory, deleteMemory, decideMemory, weeklyReview, tryChange. LIVE: goals (`read_goals`), lifeAdmin (`life_date_list`), checkins (`checkin_list`), saveCheckIn (`checkin_save`) |
| `moneyAdapter()` (`money/adapters.ts:181`, `makeLiveMoney`) | partly | overview, take, followUps, routes (all `ready: UNWIRED`), addWatchOnly, importStatement, answer, undo, reconnect, disconnect. LIVE: accounts (`money_accounts_list`), fx (`fx_rate_get`) |
| `takesAdapter()` / `hrTakeAdapter()` | all live | only transient (store not loaded) or data-empty UNWIRED |
| `crewsAdapter()` (`crews/adapters.ts:51`) | NO, base is `unwiredCrews`; hooks layer live data over it inside `CrewsScreen` only: `useLiveApprovals` (approvals, decided), `useLiveRunAdapter` (runMetrics, steps, handoffs for the open run), `useLiveN8n` (n8n, only with key-file counts) | roster, nextSchedule (roster is not used by UI) |
| `crewToolsAdapter()` + `useLiveCrewTools` | live when crew service online | grant/audit until first read |
| `standaloneAdapter()` (`makeLiveStandalone`) | partly | serviceLogs, serviceAction, summary, restore, updates, setUpdatePrefs, relay, setRelay, relayPreview, whoop, whoopAction, pairSteps, profile, tiers, setupDownload, restorePreview, setupDone. LIVE: services (from `system_map_status`), secrets, setSecret, backup (with `last: null` always), backupNow, setBackupSettings, keyFile, makeKeyFile |
| `systemMapAdapter()` | live (`system_map_status`) | none |
| `hemisyncAdapter()` | fully live | none (tag only on a failed write) |
| `recordingsAdapter()` / `liveRecordings` | partly | removeConfirmed, liveAux. window/body/series/contact are live but UNWIRED for summary-only/missing raw or a cold block |
| `roadmapAdapter()` (`makeLiveRoadmaps`) | fully live | none (UNWIRED only before first read) |
| `businessAdapter(data)` -> `liveBusiness(ledger)` | live only if `<app data>/business/ledger.json` exists. **It does not exist on this Mac** (checked `~/Library/Application Support/com.taisun.nexus/`), so every business method is unwired and funds are `NEEDS_CONVERSION` | ventures, venture, fundingRequests, voteRules, taxFlags, revenueTargetUsd; funds shows 4 names with currency-conversion tag |
| `decisionsLog()` (`makeLiveDecisions`) | live (`decisions_*`) | none (wired flips after first read) |
| `addAdapter(kind)` (`makeLiveAdd`) | live (`add_*`) | none (tag only before first `add_history` read) |
| `homeWindowsAdapter()` | NO live adapter exists: `sampleAdaptersEnabled() ? sample : unwiredAdapter` | embedding, signedSites, signOut, comment, speak: ALL |
| `navyAdapter()` (`liveNavy`) | partly | speak (localOnly is live after `set_local_only`) |

## Class key
A = wireable now from existing data/commands. B = needs a new read-only backend command (named). C = blocked (reason named).
Rows marked **(tag bug)** are places where the live adapter IS live but the UI shows the "Needs wiring" tag anyway (hard-coded or wrong condition). Those are class A and the cheapest wins.
Rows marked **(cond.)** show wiring text only while a store has not loaded or data is absent; not a standing gap.

## Home (Home screen, `screens/TerranceScreen.tsx`)

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Home > Needs you today > Decisions row ("will show here / Needs wiring") | `inbox/NeedsYouToday.tsx:123-124` (reason text `inbox/needsYou.ts:19`) | `needs_you_list` "decisions" report, fallback `businessAdapter.fundingRequests/taxFlags` | unwired (no ledger.json) | `read_business_ledger` exists, file absent; `decisions_list` live | C: needs Taisun's ledger data (decision 0005 ledger file) |
| Home > Needs you today > Crew approvals row | `NeedsYouToday.tsx:123` (reason `needsYou.ts:19`) | `crewApprovals()` cache (client store) | unwired unless the Crews view was opened and crew service online (cache is filled only by `useLiveApprovals` in CrewsScreen). Unsure: not run | `crew_list_approvals` | A: prefetch the approvals cache on Home (unsure on the fallback path) |
| Home > Needs you today > To-dos / Dates / Check-in rows | `NeedsYouToday.tsx:123` | `needs_you_list` | live | `needs_you_list`, `followups_list`, `life_date_list`, `checkin_list` | (cond.) only if `needs_you_list` fails |
| Home > Sunday card "Your weekly review is ready" | `NeedsYouToday.tsx:150` (mounted `TerranceScreen.tsx:360`, Sundays only) | `personal.weeklyReview` | unwired | none (no weekly job or store); inputs exist: `checkin_list`, `noop_query`, `list_sessions`, `read_goals` | B: `weekly_review_read` + weekly job (board "Next session" item) |
| Home > Amara's take (card, `page="home"`) | `amaraTake/AmaraTakeCard.tsx:64,72` | `runTake` -> `liveDeps.facts` | live | `get_noop_health_snapshot`/`noop_query`, `home_brief`, `chat_with_router` | (cond.) shows "Needs wiring" when no facts today or router fails |
| Home > Heart-rate take | `hrTake/HrTakeCard.tsx:107` | `hrTake.status` | live | `home_hr_take_status`, `home_hr_take_maybe_refresh`, `hr_window` | (cond.) on command failure or invalid reply |
| Home > voice-opened windows (TradingView etc.) body | `homeWindows/HomeWindows.tsx:185`, `homeWindows/SampleBodies.tsx:128`, `homeWindows/useHomeWindows.ts:177` | `homeWindowsAdapter.embedding` | unwired (no live adapter) | none | C: webview embedding needs a new Tauri webview, outbound hosts and Taisun approval |
| Home > Lease-notice date preview (add a date by voice/chat) | `HomeWindows.tsx:87,108` | `addAdapter("date").parse/save` | live | `add_preview`, `add_save`, `add_undo` | A **(tag bug)**: line 108 `{!sample && <DataTag kind="wire" />}` shows the tag whenever sample is off, even though the adapter is live |

## Life (Goals, Weekly review, Dates and deadlines, Personal money; Patterns lives under Mind-Body)

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Life > Goals > habits and milestones | `personal/TodaySections.tsx:31,35,40-41` (WIRE_NOTE `:19`) | `personal.goals` | live | `read_goals`, `save_goals` | (cond.) only if `read_goals` fails. Note: habit day marks are always "none" ("No data source linked yet"), which is not tagged but is unfinished: A to compute from `checkin_list`/`noop_query`/sessions |
| Life > Goals > Check-in card, Recent entries | `TodaySections.tsx:105,117,139-140` | `personal.checkins`, `saveCheckIn` | live | `checkin_list`, `checkin_save` | (cond.) |
| Life > Goals > Check-in > mic button | `TodaySections.tsx:110`, `adds/Composer.tsx:139` | `personal.transcribe` | unwired | `voice_*` commands exist (voice bridge); no one-shot dictation command. Unsure | B (unsure): `dictate_once` reusing the voice bridge STT, on-device only |
| Life > Weekly review > whole page | `TodaySections.tsx:171,173` | `personal.weeklyReview` | unwired | none (see Home row) | B: `weekly_review_read` + job |
| Life > Weekly review > "Try it this week" | `TodaySections.tsx:167` | `personal.tryChange` | unwired | `followups_add_item` could store the reminder. Unreachable today because the review is unwired | A once review exists (uses `followups_add_item`) |
| Life > Dates & deadlines > Next 12 months | `life/DatesView.tsx:89,92` | `personal.lifeAdmin` | live | `life_date_list` | (cond.) |
| Life > Dates & deadlines > Upcoming > Edit button (personal/residence rows) | `DatesView.tsx:108` | none (`OffButton`, reason "life_date_upsert") | n/a, button is disabled | `life_date_upsert` registered; `life_date_delete` too | A for Edit (upsert exists). Delete is C (no-delete rule), do not add |
| Life > Dates > Amara take "Today's one thing" empty block | `takes/TakeExtras.tsx:33`, via `AmaraTake page="life"` (`DatesView.tsx:90`, `TodaySections.tsx:32,172`) | `takes.today("life")` | live, but `SUGGEST_PAGES` (`takes/suggestions.ts:26`) lacks "life" so `today` is always UNWIRED. Likely, not run | take log, answer store | A **(likely tag bug)**: add life to suggestions or hide the empty block when the page has no catalog entry |
| Life > Personal money > hero detail, "Bills, subscriptions and spending" | `personal/PageViews.tsx:89,97` (WIRE_NOTE `:15`) | `personal.money` | unwired | none for bills/subscriptions/categories. `money_import_statement` stores statements but nothing derives bills | B: `money_summary_read` (bills due, subscriptions, month spend from imported statements) |
| Life > Personal money > Connected accounts card tag | `money/MoneyView.tsx:55` | `money.accounts` | live | `money_accounts_list` | A **(tag bug)**: `w.state === "sample" ? sample : <DataTag kind="wire"/>` shows "Needs wiring" for live too |
| Life > Personal money > account row "USD" for yen/BTC | `MoneyView.tsx:33` | `money.accounts` (`usd == null`) | live | `fx_rate_get`, `fx_rate_set_manual`, `fx_rate_refresh` (ECB host approved, decision 0008) | A: set or refresh the rate (needs a stored rate; none on this Mac) and map usd; crypto BTC/ETH USD stays B (no price source) |
| Life > Personal money > Overview tiles/table ("Crypto", rows USD) | `MoneyView.tsx:94,103` | `money.overview` | unwired (always, live adapter never sets it) | accounts + fx live; balance history missing | A for totals/cash/investments split from live accounts+fx; B `money_balances_list` for 30-day deltas, buffer months, savings rate (named on the board) |
| Life > Personal money > Money take | `MoneyView.tsx:149` | `money.take` | unwired | `chat_with_router` + accounts | B: `money_facts_read` (take facts) after overview exists |
| Life > Personal money > Follow-ups | `MoneyView.tsx` (FollowUps) | `money.followUps` | unwired | `followups_*` is a different store | B (unsure what a "money follow-up" is) |
| Life > Personal money > Connect sheet routes (Chime, Fidelity, Japanese bank, Ledger) | `money/ConnectSheet.tsx:40,58` | `money.routes()` `ready: UNWIRED` for all four | unwired | JP bank: `money_import_statement`, `money_account_add` (SMBC and Yucho CSV approved, 0008). Ledger: `money_watch_add`, `money_balance_set_manual` (manual, approved). Chime/Fidelity: none | A for jp_bank and ledger (flip `ready`, wire `importStatement`, `addWatchOnly`). C for Chime/Fidelity (Plaid not signed up, Taisun: not in first version, decision 0008 item 3) |
| Life > Personal money > Disconnect / reconnect / answer / undo | `MoneyView.tsx:37`, ConnectSheet | `money.disconnect/reconnect/answer/undo` | unwired | no remove command registered | C: disconnect/removal is a delete (no-delete rule), reconnect needs Plaid. answer/undo: B with the money take |

## Mind-Body (Overview, Fitness, Neuro, Neuro history, Patterns)

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Mind-Body > Patterns > Findings | `personal/PageViews.tsx:42,44` | `personal.patterns` | unwired | none (no job). Inputs exist: `noop_query`, `list_sessions`, `checkin_list` | B: `patterns_read` + weekly compute job |
| Mind-Body > Patterns > Follow-through | `takes/FollowThrough.tsx:27`, `takes/WeekTakes.tsx:19,33` | `takes.outcomes/week/goals` | live | take events, take log | (cond.) until `computeViews` finishes (30 s retry) |
| Mind-Body > Fitness > Recovery, Effort rings | `screens/FitnessRings.tsx:93,116,151` (model `fitnessRingsModel.ts:59,93`) | model state "unwired" when no score ever | n/a (reads NOOP snapshot) | `get_noop_health_snapshot`, `noop_query` | (cond.) data, not wiring; shows when the strap never sent a nightly score |
| Mind-Body > Fitness > Effort > Zone minutes | `FitnessRings.tsx:136`, `fitnessRingsModel.ts:90` (`zones = UNWIRED` unless sample) | zones | unwired always | `hr_window`, `live_heart_rate`, `noop_query` (`metric_series`). Unsure whether per-day HR history exists in NOOP | B (unsure): `hr_zone_minutes(day)` |
| Mind-Body > Neuro > Live signal note "AUX automatic detection" | `neuro/LiveSignal.tsx:34`, fed by `screens/NeuroScreen.tsx:132` | `recordings.liveAux` | unwired always | `mark_recording_aux` exists | C: `AUX_PROVEN = false` (`neuro/training.ts:36`); eyes-closed test pending |
| Mind-Body > Neuro history > Recordings list, filter | `screens/RecordingsPanels.tsx:30,33` | `recordings.list` | live (always) | `list_recordings`, `list_sessions` | none |
| Mind-Body > Neuro history > Keep raw recordings | `RecordingsPanels.tsx:74,91` | `recordings.removeConfirmed` | unwired always | none | C: no-delete rule (Muse recordings are sacred, decision 0010). Tag at `:74` is shown for live too: A to drop the tag only, keep removal blocked |
| Mind-Body > Neuro history > one recording: Raw EEG, Heart and motion, Contact | `RecordingsPanels.tsx:168-201` | `recordings.window/body/contact` | live | `read_recording_window`, `read_body_window` | (cond.) cold block, or session saved before raw recording ("summary-only") |
| Mind-Body > Neuro history > Band power, Focus and calm timelines | `RecordingsPanels.tsx:215-219`, `screens/NeuroTrends.tsx:74` | `recordings.series` | live | `recording_series`, `get_session_samples` | (cond.) cold block or summary-only |
| Mind-Body > Neuro > Hemi-Sync (import, link, rate) | `screens/HemisyncSection.tsx:67,237` | `hemisync.*` | fully live | `hemisync_*` | (cond.) only on failed write |

## Enterprise (Overview, Decisions, Ventures, Finances, Tasks, Content)
Nearly everything below flips to live the moment `business/ledger.json` exists. The file is Taisun's data, not code.

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Enterprise > Overview > Ventures card | `screens/BusinessPanels.tsx:45,66,171` | `business.ventures` | unwired (no ledger) | `read_business_ledger` (file absent) | C: Taisun must author `ledger.json` (decision 0005) |
| Enterprise > Overview > Decisions link card | `BusinessPanels.tsx:94` | `business.fundingRequests` | unwired | ledger | C: ledger data |
| Enterprise > Overview > Revenue tile "Target" | `BusinessPanels.tsx:112` | `business.revenueTargetUsd` | unwired | ledger | C: ledger data |
| Enterprise > Overview > Content link card | `screens/BusinessScreen.tsx:21` | `data.pipeline` (not an adapter) | n/a | `read_content_pipeline` registered | (cond.) shown when pipeline is empty; data, not wiring |
| Enterprise > Ventures > venture page (checklist, milestones, budget, fund, next steps) | `BusinessPanels.tsx:134-160` | `business.venture` | unwired | ledger | C: ledger data |
| Enterprise > Ventures > "Draft with crew" and other next-step buttons | `BusinessPanels.tsx:158,160` | none (`OffButton`) | disabled | `crew_start` exists | C: crew draft that "asks before sending" needs a Taisun flow and Design copy; unsure |
| Enterprise > Decisions > Waiting (votes, tax flags), vote rules | `screens/EnterprisePages.tsx:55,92-95,113-124` | `business.fundingRequests/taxFlags/voteRules`, `decisionsLog.wired` | business unwired; log live | ledger; `decisions_append/list/undo/add_outcome` live | C: ledger data. Action buttons are real once ledger loads |
| Enterprise > Decisions > Decisions log | `EnterprisePages.tsx:130-131` | `decisionsLog.list` | live | `decisions_list` | (cond.) first read |
| Enterprise > Finances > Ledger funds table | `screens/RevenueScreen.tsx:40-50,67-68` | `business.funds` | unwired base: 4 named funds with "Needs wiring · currency conversion" | ledger balances (yen) plus a dated rate | C: ledger data. A for conversion: `fx_rate_get` exists but `liveBusiness` uses only the ledger's own fx |
| Enterprise > Tasks / Content / Ideas composers | `EnterprisePages.tsx:163,202`, `adds/Composer.tsx:63,142` | `addAdapter` | live | `add_*` | (cond.) before first `add_history` read |
| Enterprise > Content > Publish, Ask the accountant buttons | `EnterprisePages.tsx:124,219` | none | disabled by design | n/a | Not wiring: reason text is policy ("Amara doesn't post / contact anyone"). Leave |

## Agents (Agents, Crews)

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Agents > Crews > Crew approvals, Recently decided | `screens/CrewPanels.tsx:15,24,45`, `CrewsScreen.tsx:196-201` | `crews.approvals/decided` | live while crew service online (5 s poll) | `crew_list_approvals`, `crew_decide_approval` | (cond.) offline service |
| Agents > Crews > n8n tile and card | `CrewsScreen.tsx:236,302` | `crews.n8n` | live only when n8n answers AND counts known (needs API key file in app data; not present here) | `n8n_status` | C: n8n API key file (Taisun/key handling). Unsure the exact missing file |
| Agents > Crews > Schedules tile "Next run" | `CrewsScreen.tsx:237` (`nextSchedule` `:215`) | `crews.nextSchedule` | unwired always | `list_terrence_automations` has `nextRunAt` (`types.ts:209`); `CronJob.next_run` (`types.ts:255`) | A: map earliest active `nextRunAt` |
| Agents > Crews > Runs table: duration, tokens, cost columns | `CrewsScreen.tsx:257,262` | none (hard-coded "—" and tag) | n/a | `CrewInfo` has tokens, requests, cost_usd, start/end (used by `runDetail.ts` `mapMetrics`) | A **(tag bug)**: line 262 is an unconditional tag; the metrics already exist for the open run |
| Agents > Crews > Crews card (grant tag) | `CrewsScreen.tsx:267` | `crewTools.grant/audit` | live | `crew_tool_grant_get`, `crew_tool_audit` | (cond.) until first read |
| Agents > Crews > Tools panels | `screens/CrewToolsPanels.tsx:15,58,113,130` | `crewTools.*` | live | `crew_tool_grant_*`, `crew_tool_audit` | (cond.) |
| Agents > Crews > run page: Timeline, Handoffs, metrics | `CrewPanels.tsx:79-108` | `crews.steps/handoffs/runMetrics` | live for the open run | `crew_run_steps` | (cond.) run with no recorded steps/handoffs |
| Agents > Agents (chat), Activity | none found | none | none | none | no wiring text |

## Smart home
No `Needs wiring`, `DataTag kind="wire"` or UNWIRED path found in the Smart-home screen code (HA reads go through `ha_*` commands directly). No rows.

## Settings (General, Services, Devices, Notifications, Backup, Voice, Memory; Run setup again)

| Page > section | UI file:line | adapter.method | live adapter? | existing data source | class |
|---|---|---|---|---|---|
| Settings > General > Updates (version, notes, buttons, footer) | `standalone/UpdatesSection.tsx:17,26,31,42` | `standalone.updates/setUpdatePrefs` | unwired | none | C: updater/signing (Taisun: out of scope tonight; decision 0007 item) |
| Settings > Devices > WHOOP panel: status, last sync, live HR, gaps, store | `standalone/WhoopPanel.tsx:30,53,70,78,79,90` | `standalone.whoop` | unwired | battery and `generatedAt` already used as fallback from `data.noop`; `get_noop_health_snapshot`, `live_heart_rate`, `get_noop_settings` | B: `whoop_status` read-only (last sync, owner, gaps, store path). Owner/lock part touches takeover |
| Settings > Devices > WHOOP: sync, buzz, forget, take over, lucid, pairing steps | `WhoopPanel.tsx:25,102` | `standalone.whoopAction/pairSteps` | unwired | none | C: WHOOP takeover (Taisun out of scope tonight) |
| Settings > Services > hero note under groups | `standalone/ServicesView.tsx:97` | none | n/a | n/a | A **(tag bug)**: unconditional tag next to live services |
| Settings > Services > list | `ServicesView.tsx:66,68` | `standalone.services` | live (`system_map_status`) | `system_map_status` | (cond.) first read |
| Settings > Services > Restart / Start | `ServicesView.tsx:60` | `standalone.serviceAction` | unwired | `restart_services`, `restart_docker_service`, `check_docker_service` exist; id mapping from map nodes unsure | B: `service_restart(id)` with allowlist (writes, loopback only) |
| Settings > Services > Logs sheet | `ServicesView.tsx:81` | `standalone.serviceLogs` | unwired | none | B: `service_logs(id)` read-only with secret redaction |
| Sidebar footer "N services need attention / Update ready" | `shell/Sidebar.tsx:81`, `standalone/servicesSummary.ts` | `standalone.summary` | unwired (renders nothing, no visible tag) | derivable from live services list | A: derive attention count from `servicesFromMap` (updateReady stays C) |
| Settings > Services > System map | `standalone/SystemMapView.tsx:218-219` | `systemMap.map` | live | `system_map_status` | (cond.) first read |
| Settings > Services > Signed-in sites | `homeWindows/SignedInSites.tsx:30,33` | `homeWindows.signedSites/signOut` | unwired (no live adapter) | none | C: webview session store, outbound hosts, Taisun approval |
| Settings > Notifications > relay, preview, schedule | `standalone/NotificationsView.tsx:30,46,58` | `standalone.relay/setRelay/relayPreview` | unwired | Telegram commands exist (`send_telegram_message`, `save_settings`) | C: live Telegram relay send path (Taisun out of scope tonight; outbound) |
| Settings > Notifications > token/chat id rows | `NotificationsView.tsx:37,39` | `standalone.secrets` | live | `secrets_status` | (cond.) tag only when secrets read fails |
| Settings > Backup > "Last backup" row and hero detail | `standalone/BackupView.tsx:57,68` | `standalone.backup` (`last: null` always, `liveAdapter.ts:~108`) | live but `last` unset | `backup_list` gives newest `madeAt` and `sizeBytes` | A for time and size; B for checksum: `backup_verify_last` (no checksum command registered) |
| Settings > Backup > key file row, keys list | `BackupView.tsx:82,99` | `standalone.keyFile/secrets` | live | `key_file_status`, `secrets_status` | (cond.) |
| Settings > Backup > Restore from a Backup | `BackupView.tsx:110` | `standalone.restore` | unwired | `unpack_backup` registered | C (unsure): restore overwrites data; needs Taisun go-ahead and a preview (see restorePreview) |
| Settings > Memory > memories, "Amara wants to remember" | `personal/PageViews.tsx:167,177,190` (edit/delete/decide `:134,144,180-181`) | `personal.memories/pendingMemories` | unwired | none: no Rust command reads Letta (only `system_map.rs` mentions it) | B for reads: `memory_list` (read-only Letta loopback). Edit/delete/decide are C: Letta writes not approved (decisions 0007) and delete falls under no-delete rule |
| Settings > Run setup again (first-run sheet): disk, FileVault, model sizes, downloads, services, restore check, key list | `standalone/FirstRun.tsx:52-123`, auto-open gate `App.tsx:115` (`setupDone` unwired so it never auto-opens) | `standalone.profile/tiers/setupDownload/restorePreview/setupDone` | unwired | `get_system_info` registered (fields unsure) | C: standalone phases (bundling, signing, DMG, model_download; Taisun out of scope tonight). profile alone could be B (`get_system_info` mapping, unsure) |
| Settings > Voice | none found | none | none | `voice_*` | no wiring text |
| Navy mode "speak" | `navyMode/store.ts:45` | `navy.speak` | unwired | `voice_*` TTS path unclear | B (unsure): local speak command; not a visible "Needs wiring" string |

## Counts

Counting standing gaps only (rows classed A, B, C). `(cond.)` rows are transient/data-dependent and not counted. Rows that bundle several adapter methods count once.

- **A (wireable now): 16** — Home: crew approvals prefetch (unsure), Lease-notice tag; Life: Dates Edit, Life take empty block (likely), money account USD (fx), money overview totals, Connect routes jp_bank+ledger, tryChange (after review), Connected-accounts tag; Mind-Body: recordings retention tag only; Agents: nextSchedule, runs-table metrics tag; Settings: Services footer tag, Sidebar summary, Backup last (time/size).
- **B (needs new read-only command): 12** — `weekly_review_read`, `patterns_read`, `money_summary_read`, `money_balances_list`, `money_facts_read`, dictation (`dictate_once`, unsure), `hr_zone_minutes` (unsure), `whoop_status`, `service_restart`, `service_logs`, `backup_verify_last`, `memory_list` (+ `get_system_info` profile, unsure, and Navy speak, unsure, not counted separately).
- **C (blocked): 16** — Home: ledger Decisions row, Home windows webview; Life: Chime/Fidelity (Plaid), disconnect/reconnect (no-delete + Plaid); Mind-Body: AUX liveAux (`AUX_PROVEN`), removeConfirmed (no-delete rule); Enterprise: Ventures, Overview decisions card, Revenue target, venture detail, Decisions waiting, Finances funds (all ledger data from Taisun), Draft with crew; Agents: n8n key file; Settings: Updates (updater/signing), WHOOP actions (takeover), Notifications relay, Signed-in sites, Restore, Memory writes (Letta), Run setup again.

(Counts are of table rows, so they are approximate where a row mixes classes; the "unsure" markers above are the honest margin.)

## Recommended wiring order for class A (biggest visible win first)

1. **Tag-bug sweep (one small change set, removes the most "Needs wiring" text that is currently wrong on live pages):** `money/MoneyView.tsx:55` (Connected accounts), `standalone/ServicesView.tsx:97`, `screens/CrewsScreen.tsx:262`, `homeWindows/HomeWindows.tsx:108`, `screens/RecordingsPanels.tsx:74`, and Life take empty block (`takes/TakeExtras.tsx:33` with "life" page). Pure UI conditions, no backend.
2. **Settings > Backup "Last backup":** map newest `backup_list` row to `last` (time and size) in `standalone/liveAdapter.ts`; also clears `BackupView.tsx:57,68`. Show checksum as unknown until B.
3. **Crews > Schedules "Next run":** implement `nextSchedule` from `list_terrence_automations` earliest active `nextRunAt` (`crews/adapters.ts`, wire in CrewsScreen).
4. **Life > Personal money:** (a) flip `routes()` ready for jp_bank and ledger and wire `importStatement`/`addWatchOnly`/`money_balance_set_manual` in `money/live.ts` (decision 0008 approves these), (b) store an ECB or manual `fx_rate_*` so yen accounts show USD, (c) overview totals from live accounts+fx.
5. **Life > Dates Edit button:** wire `life_date_upsert` (`life/DatesView.tsx:108`). Do not add delete.
6. **Settings > Services summary:** derive `summary()` attention count from `servicesFromMap` so the sidebar footer shows real health.
7. **Home > Crew approvals row:** make `crewApprovals()` load from Home (small change in `useNeedsYou`; unsure of current fallback).
8. Habit day marks (Goals) from check-ins and NOOP data (not a wire tag, but the "No data source linked yet" note).

Biggest class B wins after that: `memory_list` (Settings > Memory), `weekly_review_read` and `patterns_read` (two whole Life/Mind-Body pages), then `service_logs`/`service_restart`, `whoop_status`, `backup_verify_last`.

## Status 2026-11-13 (class A sweep, lane N3)

| Class A row | Status 2026-11-13 |
|---|---|
| Tag-bug sweep (Connected accounts, Services footer, Crews runs table, Lease-notice, Recordings retention, Life take) | Landed earlier (9812b70, 4e5ad29). The Life take empty block (`takes/suggestions.ts` has no "life" page) is still open: it needs catalog copy, a Design call |
| Settings > Backup "Last backup" | Live (lane N2; checksum via the verify command, 1c93d5e) |
| Crews > Schedules "Next run" | Live (1af2316) |
| Settings > Services summary (sidebar footer) | Live (1af2316) |
| Personal money: account USD from stored rate, overview totals | Live (9b95d68, b438574, 1c93d5e) |
| Personal money > Connect sheet, Ledger (watch-only) | Live (1af2316). NEW today: optional "Balance now" you type, saved with `money_balance_set_manual` (today's date, JST). No price or chain lookup |
| Personal money > Connect sheet, Japanese bank | NEW, live: "Import a statement..." calls the Rust command `money_import_pick`. It opens the native open-file dialog (tauri-plugin-dialog), the renderer passes nothing and never sees a path. It returns only `{imported, skipped}` or a fixed code (`cancelled`, `bad_file`, `too_large`, `unrecognised_format`, `store_full`, `store_error`, `invalid_input`, `failed`). It parses first, then uses the one yen CSV account (made on first use, "Japanese bank"), then `money_import_statement` logic |
| Life > Weekly review > "Try it this week" | Live (f45829a, parent) |
| Life > Dates > Edit | Still blocked on Design (needs an edit form); `life_date_upsert` is ready |
| Home > Crew approvals prefetch | Open (unsure, touches the Home inbox fallback) |
| Habit day marks from check-ins/NOOP | Open: what counts as a "met" day is a decision for Taisun |
| Chime / Fidelity, disconnect / reconnect, all class C | Untouched by design (decision 0008 Plaid, no-delete rule) |
