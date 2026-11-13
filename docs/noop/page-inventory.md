# NOOP / Lucid page inventory (decision 0017)

Read-only inventory, 2026-11-13. Nothing built, run or changed. Sources: `~/projects/noop-lucid-current` (branch `lucid/v12`, read by file; no app run), Amara `src-console/`, `src-tauri/src/commands/noop.rs` and `noop_query.rs`.
Paths under `Strand/` are in the NOOP repo. "Line" is the `ScreenScaffold(title:)` call site or the cited definition. Where only a file is given, the line was not captured: unverified.
`docs/noop/` did not exist and no noop-merge-map was found in `docs/` (checked: `docs/` holds only architecture, decisions, design, lessons-learned, ops, release, research, plus two inventory files). Related: `docs/terrance-merge-inventory.md` (not read in full).

## How to read this

- NOOP Mac shell: `Strand/App/RootView.swift:4` `NavItem` enum, 31 sidebar destinations, grouped Body / Insights / Data & App (group names unverified beyond `MoreSectionPrefs.swift:30-40`, which names Insights, Body, Data, App for iOS More). Mac-only extra: `Strand/MenuBar/MenuBarContent.swift`.
- iOS shell (`StrandiOS/App/RootTabView.swift`), widgets, Live Activities, Siri/App Intents, HealthKit bridge, Apple Watch app and complications: dropped (listed once at the end of each area where relevant).
- Amara today, Fitness: `app/routes.ts:45` Mind-Body tabs Overview / Fitness / Neuro / Neuro history / Patterns. Fitness = `screens/HealthScreen.tsx` (hero, `FitnessRings.tsx`, `SleepCard.tsx`, `FitnessTrends.tsx` "Last 14 days", "Last night", "More trends" x7, "Insights", battery). Patterns/Overview = `screens/MindBodyScreen.tsx` (range chart, driver scatter, strain bars).
- Amara today, Settings tabs (`routes.ts:49`): General, Services, Devices (`standalone/WhoopPanel.tsx`), Notifications (`standalone/NotificationsView.tsx`, Telegram relay only), Backup, Voice, Memory. WHOOP group = `settings/NoopSettingsGroup.tsx`.
- Data Amara can read now (verified in Rust):
  - `noop_query` allowlist `noop_query.rs:13`: `health_snapshot` (days<=120), `metric_series` (keys: recovery, hrv, rhr, resp_rate, spo2, strain, sleep_total_min, steps, active_kcal; optional source, days, from/to day, limit), `data_freshness`, `sleep_summary`, `workout_summary`.
  - Direct read-only SQLite in `noop.rs`: `get_noop_health_snapshot` (tables `dailyMetric`, `sleepSession`, `battery`), `live_heart_rate` (`hrSample`), `noop_recent_workouts` (`workout`, incl. zonesJSON, distance, steps), `noop_hypnogram` (`sleepSession.stagesJSON`).
  - `DailyMetric` fields Amara reads: totalSleepMin, efficiency, deep/rem/light min, restingHr, avgHrv, recovery, strain, spo2Pct, skinTempDevC, respRateBpm.
  - Nothing else in NOOP's store is read (no stress, HRV beat stream, journal, lift log, lab markers, hydration, devices, coach messages, Oura tables).
- Gap codes: **READ** = needs a new read command (data is in NOOP's store, schema unverified); **DESIGN** = needs Design Lead layout (data already readable); **SIDECAR** = needs Phase 10 sidecar/absorbed store, or NOOP-app-only state (UserDefaults, on-device compute); **DROPPED** = iOS/watchOS only or not wanted.
- "Amara today" names were read from code but not run in the app: all "Amara today" cells are unverified visually.

## 1. Fitness / health pages

| NOOP page/tab | Shows / controls (fields) | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Today (classic) | Charge/Effort/Rest hero rings, Key Metrics tiles, Synthesis, "Your cards", Stress through the day, HR, Synced from; customizable order/hide | `Screens/TodayView.swift:1461`, `DashboardCards.swift`, `HostedCards.swift` | `FitnessRings.tsx` (3 rings), `HealthScreen.tsx` hero | `health_snapshot`; `metric_series` recovery/strain/hrv/rhr | Rings: have. Key Metrics tiles + Synthesis + Stress strip: DESIGN. Layout customization: SIDECAR (app prefs) |
| Today (Liquid, experimental) | Scores as liquid, time-of-day sky, key-metrics window | `Liquid/LiquidTodayView.swift:117`, `LiquidSky.swift` | none | same as Today | DROPPED (prototype skin of Today; keep data, not the look). Ask Taisun |
| Intelligence | On-device charge/effort/rest model status, "Building from your strap", Recent, Charge model | `Screens/IntelligenceView.swift:30` | none | `health_snapshot` (scores only); model internals not readable | DESIGN for score history; model state needs SIDECAR |
| What Moves You (Insights hub) | n-of-1 effect size of behaviours on Charge; "How to read this" | `Screens/InsightsHubView.swift:41`, `Data/IntelligenceEngine.swift` | `MindBodyScreen.tsx` Patterns driver scatter (sleep/HRV/recovery vs sessions) is similar but not the same | `metric_series` (9 keys); journal/behaviour inputs not readable | READ (journal/behaviour tables) then DESIGN |
| Insights (journal + Q&A) | Journal log, custom questions, experiments (`noop.experiment.*`), mood | `Screens/InsightsView.swift:212`, `JournalLogCard.swift`, `Data/BehaviorStore.swift`, `MoodStore.swift`, `JournalCatalog.swift` | none | none (journal not in allowlist) | READ + DESIGN. Writes (logging) = SIDECAR |
| Sleep | Last-night result, stage breakdown, hypnogram, hours vs needed, consistency, sleep debt ledger, body clock dial, asleep duration, add a nap, dismissed spans, section order | `Screens/SleepView.swift:157`, `StagesCard.swift`, `SleepDebtLedgerCard.swift`, `HoursVsNeededCard.swift`, `ConsistencyCard.swift`, `BodyClockDialCard.swift`, `NightDetailCard.swift` | `SleepCard.tsx`, "Last night" card, hypnogram via `getHypnogram` (`bridge.ts:420`; render unverified) | `sleep_summary`, `noop_hypnogram`, `metric_series sleep_total_min`, dailyMetric stages | Core: have. Debt, consistency, needed, body clock: DESIGN (compute from series; "needed" not readable: unverified). Add nap/dismiss: SIDECAR |
| Trends | Charge, HRV, RHR, Effort over week/month/etc; export trends report | `Screens/TrendsView.swift:293`, `TrendsReportView.swift`, `System/TrendsReportRenderer.swift` | `FitnessTrends.tsx` (14 d), `HealthScreen` "More trends" (recovery, sleep, hrv, rhr, effort, skin, resp), `MindBodyScreen` range chart (Week..Year) | `metric_series` (up to 4000 d) | Have. Report export: DESIGN + READ-free (render client side) |
| Workouts | Every session, sport, duration, kcal, avg/max HR, strain, distance, steps, zones, auto-detected, manual add, dismissed | `Screens/WorkoutsView.swift:180`, `WorkoutDetailView.swift:74`, `ManualWorkoutSheet.swift`, `AutoWorkoutCard.swift`, `Data/WorkoutZones.swift` | none (used only by `takes/liveSuggest.ts:33`) | `noop_recent_workouts` (days), `workout_summary` | DESIGN. Manual add/auto-detect accept: SIDECAR |
| Workout detail | HR trace, zone minutes, route | `Screens/WorkoutDetailView.swift:74`, `GpsWorkoutRecorder.swift` | none | zones via `noop_recent_workouts`; HR trace via `hrSample` range (query exists at `noop.rs:1234`); route not readable | DESIGN; route = SIDECAR |
| Live Body Console | Live HR, strap trust, session controls | `Screens/LiveView.swift:111`, `LiveSessionRunner.swift` | Home live HR via `live_heart_rate` (`bridge.ts:408`); no Fitness page | `live_heart_rate` (hrSample, last 50) | DESIGN. Session controls (start workout, buzz) = SIDECAR |
| Live Session (beta, guarded workout) | HR band gated on Charge; buzz-only coaching | `Liquid/LiveSessionView.swift`, `Screens/LiveWorkoutView.swift` | none | live HR only | SIDECAR (strap control) |
| Health Monitor | Live vitals, Heart Rate, Fitness Age, VO2max, Vitality, Body Age | `Screens/HealthView.swift:27,1653,1662` | none | spo2, resp, rhr via series; VO2max/fitness age not in allowlist | READ (if stored) else SIDECAR (computed in app: unverified) |
| Stress | Autonomic load through day, sustained high stress, advanced HRV, daily 0-3 proxy, range chips | `Screens/StressView.swift:77`, `Data/StressDayCurve.swift`, `DaytimeStressMode.swift` | none | none (stress curve not in allowlist) | READ or SIDECAR (likely computed from HR/RR: unverified) |
| Explore (Metric Explorer) | Every catalog metric, one tap deep, by key+source | `Screens/MetricExplorerView.swift:505`, `Data/MetricCatalog.swift` | none | 9 of catalog keys via `metric_series`; catalog has Apple Health, Mi Band, nutrition, mood sources (count unverified) | DESIGN; extra keys need READ (allowlist widen) |
| Compare | Overlay signals, range, selected ids | `Screens/CompareView.swift:174` | none | the 9 keys | DESIGN |
| Day (Coupled) | One day, coupled signals | `Screens/CoupledView.swift:131` | none | partial | DESIGN (+READ for intraday) |
| Deep Timeline | Whole-day full-res HR, zoomable | `Screens/FullDayChartView.swift:87` | none | `hrSample` (full-day query not built; last-50 + range exist) | READ + DESIGN |
| HRV Reading | Snapshot HRV measurement | `Screens/HRVSnapshotView.swift:72` | none | avg HRV nightly only | SIDECAR (beat-to-beat) |
| Rhythm (experimental) | Last night, beat-to-beat scatter; consent-gated | `Screens/RhythmView.swift` | none | no RR data | SIDECAR |
| Lab Book | Bloods, BP, body numbers, markers, add a reading | `Screens/LabBookView.swift:622`, `MarkerEditorView.swift:54`, `Packages/WhoopStore/.../LabMarkerStore.swift` | none | none | READ + DESIGN; entry = SIDECAR. Private data: needs Taisun OK on exposure |
| Your Data, Fused | Multi-device per-metric source comparison and chosen source | `Screens/V5PillarHosts.swift:33`, `FusedRecordView.swift:334` | none | `data_freshness` (source coverage) | READ (per-source series) + DESIGN |
| Hydration | Fluid log, daily goal ring | `Screens/HydrationView.swift:50`, `Data/HydrationStore.swift` (UserDefaults `noop.hydrationEntries.*`) | none | none (UserDefaults) | SIDECAR |
| Caffeine log | Intakes, cutoff nudge | `Screens/CaffeineLogCard.swift`, `Data/CaffeineLog.swift` (`caffeine.intakes`) | none | none | SIDECAR |
| Breathe | Paced breathing, audio cues, biofeedback, resonance pace | `Screens/BreathingView.swift:182`, `BiofeedbackController.swift` | none (Neuro/Muse flows are separate) | none | DESIGN decision: keep? Strap haptics = SIDECAR |
| Interval Timer | Timed intervals with haptics | `Screens/IntervalTimerView.swift:94` | none | none | SIDECAR |
| Lift log / sessions | Program editor, sessions, exercises, sets, import | `Screens/LiftLogView.swift`, `LiftSessionView.swift:642`, `LiftProgramEditorSheet.swift`, `Packages/WhoopStore/.../LiftLogStore.swift` | none | none | READ (LiftLogStore) + DESIGN |
| Week in Review | Weekly digest | `Screens/WeeklyDigestView.swift:88` | none | derivable from series | DESIGN |
| Storage | Database size and prune | `Screens/StorageView.swift:21` | Backup tab (different) | none | SIDECAR |
| Test Centre | Guided capture tests, strap log, report | `Screens/TestCentreView.swift:164`, `System/GuidedCapture.swift` | none | none | DROPPED for Fitness (dev tool); lives with NOOP.app. Ask Taisun |
| NOOP Limitations / How NOOP works / Scoring guide / What's new / Updates inbox | Help and changelog pages | `Screens/NoopLimitationsView.swift:80`, `HowNoopWorksView.swift`, `ScoringGuideView.swift`, `WhatsNewView.swift`, `UpdatesInboxView.swift` | none | n/a | DROPPED (NOOP-app help); scoring guide worth a DESIGN "About scores" note: ask Taisun |
| Menu bar extra | Mac menu-bar glance | `MenuBar/MenuBarContent.swift` | none | n/a | DROPPED (Amara has its own shell). Unverified contents |
| iOS tab shell, widgets (Charge/Stress/HR/Coach brief), Live Activities, Siri shortcuts, quick actions | iPhone surfaces | `StrandiOS/**`, `StrandiOSWidgets/**` | n/a | n/a | DROPPED (iOS only) |
| Apple Watch app + complications (Glance, Live HR, Workout, Interval, Breathe) | Watch surfaces | `NOOPWatch/**`, `NOOPWatchComplications/**` | n/a | n/a | DROPPED (watchOS only) |

## 2. Lucid dream pages

Lucid has no standalone page in NOOP: it is one Settings card plus a controller. Verified at `Screens/SettingsView.swift:2877` (`LucidDreamSettingsCard`), `App/LucidDreamController.swift` (514 lines).

| NOOP page/tab | Shows / controls (fields) | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Lucid Dream Mode card: enable + tuning | Enable; start monitoring after onset (60-600 min, default 270); stable REM before cue (1-20, def 5); max cues per episode (1-4, def 2); delay between cues (30-300 s, def 60); stop on arousal; logging | `Screens/SettingsView.swift:2889`; `Data/LucidDreamSettingsStore.swift:25-34` | `WhoopPanel.tsx:89` one "Lucid Dream cues" toggle (`lucid_on/lucid_off` action; backend effect unverified) | none (UserDefaults `lucidDream.*`) | SIDECAR (settings read/write) + DESIGN |
| Lucid cue test | Fires the exact sleep cue on the strap | `SettingsView.swift:~2960` ("Lucid cue test") | none | n/a | SIDECAR (strap control); needs Taisun hardware gate |
| Mac notify on cue / keep Mac awake | Notify when band vibrates (def on); keep Mac awake overnight (def on) | `LucidDreamSettingsStore.swift:32-33`; `System/LucidDreamSystemHooks.swift` | none | n/a | SIDECAR |
| Pre-sleep training | Training minutes; once-a-minute buzz prompts, "Cue N of M" | `SettingsView.swift:~3022`; `Data/LucidDreamSettingsStore.swift` (`trainingMinutes`); `LucidDreamTrainingPlan` in StrandAnalytics (unverified location) | none | n/a | DESIGN + SIDECAR |
| Morning check-in | "I had a lucid dream", "A cue woke me up", "I remember a dream", Save | `SettingsView.swift:~3090`; `Data/LucidDreamJournal.swift` (`LucidCheckIn`) | none | none | READ (journal file) + DESIGN; save = SIDECAR |
| Your results (tally) | nights, cued nights, lucid cued vs uncued, woken by cue, recalled | `SettingsView.swift:~3124`; `LucidDreamJournal.swift` `LucidJournalTally` | none | none | READ + DESIGN |
| Night record | sessionID, sleepOnset, monitored, cueTimes | `LucidDreamJournal.swift:5` (`lucidDream.nightRecord` in UserDefaults per grep) | none | none | READ/SIDECAR |
| Lucid event log (JSONL) | event timeline: onset, REM onset, stable REM, cue #, haptic pattern, HR/movement before-after, stop reason, haptic error | `Data/LucidDreamEventLog.swift:5-28` | none | none (file in NOOP Application Support) | READ + DESIGN. Contains biometrics: handle per hard rule 2 style care |
| REM detector status | Live "monitoring / REM suspected / cued" state | `App/LucidDreamController.swift`; `StrandAnalytics/LucidDreamRemDetector.swift`, `LucidDreamPolicies.swift` | none | none | SIDECAR (runs in app) |

## 3. Strap / device

| NOOP page/tab | Shows / controls | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Devices | Paired bands, make active, rename, remove, restart, delete data; WHOOP 4/5/MG generations; reboot and battery-opcode probes | `Screens/DevicesView.swift:24`, `Data/DeviceRegistry.swift`, `WhoopStore/.../DeviceRegistryStore.swift` | `screens/DevicesScreen.tsx`, `standalone/WhoopPanel.tsx` (WHOOP 4, Sync, Who has the strap, Health store, Other devices) | battery via snapshot; device id; registry not readable | READ (registry) + DESIGN; actions (rename/remove/restart) = SIDECAR |
| Add Device wizard | Pair flow | `Screens/AddDeviceWizard.swift` | `WhoopPanel.tsx:101` "Pair your WHOOP 4" dialog | n/a | SIDECAR (Phase 10/11 strap takeover) |
| Strap settings card | Re-scan, Disconnect, strap name (rename BLE name), copy/save strap log, WHOOP model (`selectedWhoopModel`) | `SettingsView.swift:1410`, `BLE/WhoopModel.swift:36` | `WhoopPanel.tsx` (read-only status) | battery, last sync (`data_freshness`) | SIDECAR |
| Power saving | Mode on/off, kick-in battery %, pause HRV capture, low refresh (hourly sync) | `Screens/PowerSavingView.swift:24` | none | battery | SIDECAR |
| Sync settings | Keep screen on while syncing; sync behaviour; history gaps | `SettingsView.swift:1765,1796`; `Collect/Backfiller.swift` | `WhoopPanel.tsx:64-68` Sync card (last sync, live HR, gaps 14 d) | `data_freshness` | Status: have (wiring unverified). Controls: SIDECAR |
| HRV capture | Continuous HRV capture, overnight only, HRV window (Night / Deep sleep) | `SettingsView.swift:1797` (`hrv.window`) | none | none | SIDECAR |
| Recovery baseline | Recalibrate Charge baseline (re-learn ~4 nights) | `SettingsView.swift:1609`, `:~1614` button | none | n/a | SIDECAR |
| Alarms | Strap wake-alarm, wake time, weekdays, wind-down reminder, sleep need, lead minutes | `Screens/SmartAlarmView.swift:48`; `behavior.smartAlarm*`, `windDown.*` | none | none | SIDECAR + DESIGN |
| Automations | Double-tap action, wrist-off auto-lock, shortcuts, recent moments, zone coaching, stress nudge | `Screens/AutomationsView.swift:51`; `behavior.*` keys | none | none | SIDECAR |
| Mi Band | Heart & vitals, sleep, activity, wellbeing charts from import | `Screens/XiaomiBandView.swift:114`, `Data/XiaomiImporter.swift` | none | partly via `metric_series` `source` arg (xiaomi-band id: unverified) | DESIGN |
| Battery / charging | % and charging, predictive and low-battery alerts | `BLE/*`, `System/BatteryNotifier.swift` | `HealthScreen.tsx:139` BatteryStatus, `DevicesScreen.tsx:36` | `battery` table (48 h freshness rules in `noop.rs:20`) | Have |
| Experimental BLE probes (Puffin, Oura dumps, raw capture, GATT) | Developer tooling | `BLE/Puffin*.swift`, `Oura*Dump.swift`, `Screens/RawDataCollectorView.swift` | none | n/a | DROPPED (dev tooling; stays in NOOP.app/Test Centre) |

## 4. Integrations: Oura, Hevy, coach

| NOOP page/tab | Shows / controls | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Data Sources (hub) | Apple Health import, Mi Band import, Oura connect, Lifting log import, WHOOP gen table, HR broadcast | `Screens/DataSourcesView.swift:82` | none | `data_freshness` | DESIGN; imports = SIDECAR |
| Oura connect | OAuth connect, token store, sync result; needs `OuraSecrets.xcconfig` credentials | `DataSourcesView.swift:323`; `Strand/Oura/*` (OuraOAuth, OuraSyncCoordinator, OuraTokenStore) | none | `metric_series source` could carry an Oura id: unverified | SIDECAR. New outbound host (Oura API) conflicts with hard rule 3: needs Taisun OK |
| Oura ring live (BLE) + all-day HR toggle | `ouraAllDayLiveHREnabled` experimental | `SettingsView.swift:1410+~200`; `BLE/OuraLiveSource.swift` | none | none | SIDECAR |
| Hevy / Liftosaur import | CSV (Hevy) / JSON (Liftosaur) file import, volume estimate; no live Hevy API found | `DataSourcesView.swift:243,437`; `Screens/LiftProgramImportSheet.swift` | none | none | SIDECAR (file import); READ for imported sessions. I found no Hevy network integration in NOOP (grep: file import only) |
| Apple Health | Mac: import Health export zip; iOS-only live HealthKit read/write | `Screens/AppleHealthView.swift:196`, `Data/AppleHealthImport.swift`, `StrandiOS/Health/*` | none | dailyMetric includes merged sources | DESIGN for imported charts (Mac); HealthKit live = DROPPED (iOS) |
| Shortcuts health export/import | Shortcut-based sync (`noop.shortcutSync.*`) | `Data/ShortcutHealthExport.swift` | none | n/a | DROPPED (iOS Shortcuts) unverified for Mac |
| Coach (chat) | LLM chat on your data; launcher sheet; markdown; voice input (iOS) | `Screens/CoachView.swift:61`, `CoachLauncherSheet.swift`, `AI/AICoach.swift`, `WhoopStore/.../CoachMessageStore.swift` | Amara Agents / chat (own stack); `lib/noop.ts` feeds body_context | `noop.rs:1137` `runtime_health_brief` feeds assistant | DESIGN decision: does NOOP Coach history migrate? READ (CoachMessageStore). NOOP coach calls cloud providers (OpenAI, Anthropic, Gemini, Custom): conflicts with rule 3 |
| Coach settings | Let coach use my data; share patterns and Lab Book; send chart image to Gemini; instructions; Morning brief on + time | `Screens/CoachSettingsView.swift:40`; `System/CoachBriefScheduler.swift` (`coachBrief.*`) | none | n/a | SIDECAR settings; Taisun call on whether NOOP's coach survives |
| Data backup & sync | Folder bookmark, daily auto-backup, keep N, restore | `Screens/BackupSyncView.swift`, `Data/BackupSync.swift` (`backupSync.*`) | Settings > Backup (`standalone/BackupView.tsx`, `settings/BackupSection.tsx`) | n/a | Amara has own backup; NOOP-store backup = SIDECAR (Phase 10 copy-and-check) |
| Backup & restore (zip), CSV export | Export all data, import replaces | `SettingsView.swift:2153`, `Data/DataBackup.swift`, `CsvExport.swift` | none | n/a | SIDECAR |

## 5. Notifications

| NOOP page/tab | Shows / controls | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Notifications (wrist alerts) | Master enable, only when worn, quiet hours + start/end, per-app prefs by category, "Behaviour". Note NOOP says wrist delivery "isn't live yet" on Mac | `Screens/NotificationSettingsView.swift:13`; `Data/NotificationSettingsStore.swift:74-101` (`notif.*`) | `standalone/NotificationsView.tsx` is Telegram relay: different feature | none | SIDECAR + DESIGN (new section in Settings > Notifications) |
| Health alerts | Illness watch, battery alerts (+predictive), strain-target nudge | `Data/BehaviorStore.swift` (`behavior.illnessWatch` etc.), `System/IllnessNotifier.swift`, `BatteryNotifier.swift`, `StrainTargetNotifier.swift` | `NotificationsView.tsx` "What gets sent" chips (which alert types: unverified) | health_snapshot has insights/battery | Map to existing Telegram chips: DESIGN question |
| Inactivity nudge | Enable, threshold min, re-nudge, buzz loops, active hours | `Data/InactivityPrefs.swift:41-47` | none | none | SIDECAR |
| Stress check-in / auto nudge | Check-in, auto-nudge, quiet hours, resonance pace | `Screens/BiofeedbackPrefs.swift` (`biofeedback.*`) | none | none | SIDECAR |
| Wind-down | Enabled, sleep need, lead min, wake min, per-day wake | `System/WindDownNudge.swift` (`windDown.*`) | none | none | SIDECAR |
| Coach brief notification | Enabled, time | `System/CoachBriefScheduler.swift` | none | none | SIDECAR |
| Lucid cue Mac notification | notifyOnCue | `LucidDreamSettingsStore.swift:32` | none | none | SIDECAR |
| Live notifications (Lock Screen / Dynamic Island) | `liveActivity.*` | `SettingsView.swift:1571` | n/a | n/a | DROPPED (iOS) |
| Journal reminder | Today card | `SettingsView.swift:~1727`, `Screens/JournalReminderCard.swift` | none | none | SIDECAR |

## 6. Settings (Amara target: Settings tabs)

Amara today: WHOOP group has source (Health sync vs Direct Bluetooth), reader path, database path (`NoopSettingsGroup.tsx`, backed by `get/set_noop_settings`, `noop.rs`/`noop_query.rs:135-176`). That is the only NOOP setting Amara can write today.

| NOOP setting section | Fields / keys | NOOP source | Amara today | Data now? | Gap |
|---|---|---|---|---|---|
| Profile | Date of birth, sex, weight, height, waist, max HR, custom HR zones (+5 starts), day cycle, step calibration (+ coefficient/confidence), avatar | `SettingsView.swift:368-682`; `Data/Profile.swift:84-99` (`profile.*`) | none | none (UserDefaults) | SIDECAR + DESIGN. Read needed to avoid asking Taisun twice |
| Units | Body measurements, distance and pace, temperature, skin temp display, effort scale (0-100 vs 0-21) | `SettingsView.swift:933`; `Data/Units.swift:86-98` | Amara hardcodes Effort 0-100 clamp (`HealthScreen.tsx:71`) | none | DESIGN + SIDECAR |
| Appearance / language | Language, clock, theme, preset, chart colours, sleep chart, accent, trend charts style, app icon, bottom bar | `SettingsView.swift:1095`; `trend.chart.style`, `appIcon.alt` | `SettingsScreen.tsx:97` Theme, Reduce motion | n/a | DROPPED (Amara owns look); language/clock: ask Taisun |
| Strap | see Section 3 | `SettingsView.swift:1410` | partial | partial | see 3 |
| Live notifications | iOS | `SettingsView.swift:1571` | n/a | n/a | DROPPED |
| Recovery | Recalibrate | `SettingsView.swift:1609` | none | n/a | SIDECAR |
| Features | Hydration tracking, Auto-detect workouts, Journal reminder, Keep screen on during workout | `SettingsView.swift:1677` | none | n/a | SIDECAR (screen-on = DROPPED iOS-ish) |
| Sync | Keep screen on while syncing | `SettingsView.swift:1766` | none | n/a | SIDECAR |
| HRV | Continuous capture, overnight only, window | `SettingsView.swift:1797` | none | n/a | SIDECAR |
| Experimental toggles | Liquid Today, Live Sessions beta, Sleep staging V2, Motion-aware wake, Blood Oxygen strap estimate, Oura all-day HR | `SettingsView.swift:1883-2013` | none | n/a | SIDECAR; Liquid Today DROPPED |
| Sleep staging | V2 recipe on/off | `SettingsView.swift:1935` | none | n/a | SIDECAR |
| Diagnostics export | 24 h decoded sensor streams, reveal in Finder | `SettingsView.swift:2041` | Settings > General "About & Diagnostics" (different content) | n/a | DROPPED or SIDECAR: ask Taisun |
| Backup & restore | see Section 4 | `SettingsView.swift:2153` | Backup tab | n/a | see 4 |
| Test Centre | tests, scheduled debug export | `SettingsView.swift:1649` | none | n/a | DROPPED |
| About / terms / onboarding / onboarding wizard | Version, terms accepted (`noop.acceptedTerms*`), `noop.onboarded` | `SettingsView.swift:2344`, `App/ContentView.swift:7-11`, `Onboarding/OnboardingWizard.swift` | `SettingsScreen.tsx:120` About | n/a | DROPPED (NOOP onboarding retires with the app) |
| Lucid Dream Mode | see Section 2 | `SettingsView.swift:2883` | `WhoopPanel.tsx:89` toggle | n/a | see 2 |
| Layout prefs (Today/Sleep section order, hidden, key metrics, compare range) | `today.*`, `sleep.sectionOrder`, `compare.*` | `Data/TodayLayoutPrefs.swift`, `SleepLayoutPrefs.swift`, `KeyMetricPrefs.swift` | n/a | n/a | DROPPED (Design owns layout) |
| Haptics prefs | Per feature: breathing, intervals, lift rest, live session, workout | `Data/HapticPrefs.swift` | none | n/a | SIDECAR |
| Rescore / maintenance | `noop.rescore*` internal state | `System/RescoreBackgroundScheduler.swift`, `Data/RescoreBackgroundPolicy.swift` | none | n/a | DROPPED (internal) |
| Updates | `updates.autoCheck`, changelog | `System/UpdateChecker.swift` | Settings > General Updates section (`UpdatesSection.tsx`) | n/a | DROPPED (Amara updater; NOOP's checks an outbound host) |

Total distinct UserDefaults/AppStorage keys: `@AppStorage` unique keys 30 (grep); `UserDefaults` literal keys ~92 occurrences (grep counts, not deduped): unverified.

## Counts summary

Rows by group (a row can cover several screens): Fitness/health 28, Lucid 9, Strap/device 11, Integrations 9, Notifications 9, Settings 19. Total 85.
Gap tallies (rough, primary gap per row; unverified rounding):
- Data readable now and only needs DESIGN (plus light client math): about 9 (Today tiles/synthesis, Sleep extras, Trends report, Workouts, Workout detail, Live console, Compare, Week in Review, Mi Band).
- Already covered at least partly in Amara: 6 (rings, Sleep card, trends, battery, Devices status, WHOOP source settings).
- Needs a new READ command first: about 14 (journal/insights, stress, Lab Book, Lift log, Fused, Deep Timeline, Explore extra keys, Lucid journal/log/tally, device registry, Coach messages).
- Needs Phase 10 sidecar / app-owned state (settings writes, strap control, UserDefaults prefs): about 40.
- Dropped (iOS/watchOS/dev/prototype/NOOP-only): about 17.

## Top 10 to build first (biggest value on data Amara already reads)

1. Fitness > Workouts list + detail with zone minutes (`noop_recent_workouts`; commands already in `bridge.ts:415`, no screen).
2. Fitness > Sleep detail: hypnogram, stage breakdown, 14-night efficiency (`noop_hypnogram`, `sleep_summary`).
3. Fitness > Trends explorer: all 9 `metric_series` keys incl. spo2, steps, active_kcal, with week..year range (Explore + Compare basics).
4. Fitness > Today tiles: Key Metrics (RHR, HRV, resp, SpO2, skin temp, steps) from the snapshot.
5. Fitness > Live heart rate card/console (`live_heart_rate`, trace of 50 samples).
6. Fitness > Week in Review (derived from `metric_series`/`health_snapshot`, client side).
7. Fitness > Day view: one day's dailyMetric + sleep + workouts together (Coupled).
8. Fitness > Sleep consistency / sleep debt / hours vs needed (derived; confirm "needed" source first).
9. Fitness > Data freshness / sources panel (`data_freshness`), basis for the Fused view.
10. Settings > Devices: show NOOP source, store path and battery history (read-only; extends `NoopSettingsGroup`/`WhoopPanel`). Settings writes wait for Phase 10.

## Design asks (for Nexus Design Lead; not done by Claude Code)

1. Fitness sub-navigation: Fitness has no sub-tabs today (hero + cards). Need a structure for ~10 more pages (Workouts, Sleep, Trends, Stress, Live, Lucid, Lab Book, Lifting, Journal, Sources). Nav labels and shortcuts can't change without Taisun.
2. Lucid Dream area: where it lives (Fitness tab vs Neuro), training/check-in/results layout, event-timeline chart.
3. Settings sections for NOOP: Profile, Units, Strap, Sleep/HRV capture, Features, Coach, Notifications (wrist alerts), Lucid cues. Possibly under Settings > Devices and Notifications.
4. Workout list/detail, zone bars, route (if ever readable).
5. Stress day curve and HRV beat views, Rhythm scatter, Fused-record comparison, Lab Book chart.
6. Empty/"Needs wiring" states for every unreadable page (per design-boundary: show "Needs wiring", never invent numbers).
7. Questions to `tasks/design-questions.md`, not code.

## Questions for Taisun

1. Which of Test Centre, Liquid Today, Rhythm, Live Sessions (beta), Menu bar extra, NOOP help pages should be dropped vs kept?
2. Does NOOP's Coach (and its history) migrate, or does Amara's own assistant replace it? NOOP's coach calls cloud providers; hard rule 3 forbids new outbound hosts.
3. Oura: connect via Oura cloud OAuth (new outbound host) or BLE/file only? Is Oura in scope at all?
4. Hevy: file import only exists in NOOP; do you want a live Hevy API (new host) or just CSV import?
5. Lab Book, journal and mood are private. OK to surface them read-only in Amara?
6. Lucid cues: stay paused until strap takeover gate (your hardware check)? Should the Lucid toggle in WhoopPanel become the full card?
7. Should Phase 10 sidecars also own NOOP's UserDefaults settings (profile, units, notifications), or should Amara keep its own copy and only read?
8. Effort scale: keep 0-100 or add NOOP's 0-21 toggle?
9. Language and clock-format settings: wanted in Amara?

## What I could not verify

- Nothing was run: no NOOP app, no Amara app, no tests, no build. All "Amara today" and "renders" claims come from reading code only.
- Line numbers: given for `ScreenScaffold` calls and the cited definitions; "~" or file-only entries are approximate/unverified.
- Contents of most large views (Today 5980 lines, Sleep 3092, Workouts 2092, Metric Explorer 1895, Settings 3764): I read headings and keys by grep, not whole files. Card lists per screen are therefore incomplete.
- Whether `getHypnogram`, `getLiveHeartRate`, `getRecentWorkouts` are rendered anywhere in Amara's UI beyond `bridge.ts` and `takes/liveSuggest.ts`: not confirmed (grep found no screen callers).
- Whether `noop_query` tools `sleep_summary`, `workout_summary`, `data_freshness` have any TS consumers: grep found only the type list `src/lib/types.ts:633`.
- NOOP SQLite schema beyond tables named in `noop.rs` (dailyMetric, sleepSession, battery, hrSample, workout) and `Packages/WhoopStore/Sources` filenames (LiftLogStore, LabMarkerStore, CoachMessageStore, MetricSeriesStore, DeviceRegistryStore, etc.): table names and columns unverified.
- Which settings live in the SQLite store vs UserDefaults vs files (Lucid journal file path, Lucid event log location): unverified.
- Mac vs iOS availability of each screen: inferred from file location and `NavItem`; `Platform.swift` gating not checked.
- The Android app (`android/`) was not inspected.
- "What `WhoopPanel` lucid_on/lucid_off does" in Rust: not traced.
- Sidebar group assignments on the Mac (Body/Insights/Data & App): inferred from comments, not read from `RootView` body.
- Row and gap counts above are hand-tallied and approximate.
