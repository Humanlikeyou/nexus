# Terrance merge inventory

Goal by 2026-11-27: "Terrance merged or archived." This is the inventory for that decision:
which Tauri commands the live UI (`src-console/`) still calls, and which legacy files in
`src/` and `src-next/` it still loads.

Generated 2026-10-29 by `node scripts/command-inventory.mjs --markdown` (JSON without the flag).

**Method.**
- **Registered:** the `module::fn` entries of `generate_handler![...]` in `src-tauri/src/lib.rs`.
- **Live:** reachable from `src-console/main.tsx`'s import graph. This is an esbuild metafile bundle with the vite `@` / `@console` aliases, so it includes the `src/` modules the console pulls in (scheduler, continuity, rituals, selfDiagnosis, api.ts, …). Type-only imports and unused imports are erased by the TypeScript transform, so they don't make a file reachable. See (c) for why that matters.
- **Callers:** `invoke("name"` / `read("name"` calls in `src-console/`, `src/` and `src-next/`, tests excluded. Generic arguments, including ones with parens like `invoke<import("./types").X>(`, and line breaks are handled.
- **Status:** a command is *live* if any reachable file calls it, *legacy-only* if only unreachable files call it, and *no-callers* otherwise.

`test/commands.registered.test.ts` uses the same script. It fails if any frontend `invoke`/`read` names a command that isn't in `generate_handler!`.

## Summary

| | Before | After |
|---|---|---|
| Registered commands | 137 | 129 |
| Live | 129 | 129 |
| Legacy-only | 0 | 0 |
| No callers | 8 | 0 (unregistered) |

The 8 no-caller commands were removed from `generate_handler!` only. Their Rust `fn`s stay, so restoring one is a one-line change; the list is in a comment above `generate_handler!`. The 6 that nothing else in Rust calls carry `#[allow(dead_code)]`. None of them appears in `src-tauri/capabilities/` (it lists plugin permissions only), and none is used by the scheduler, rituals, selfDiagnosis, research or continuity paths.

## (c) Commands removed from generate_handler!

| Command | Reason |
|---|---|
| `openclaw::get_openclaw_sandbox` | Read the sandbox toggle for the old Amara page's OpenClaw card; src-console has no sandbox control. |
| `openclaw::toggle_openclaw_sandbox` | Write side of the same old-UI toggle; no caller anywhere in the frontend. |
| `terrence_actions::list_terrence_tools` | Tool catalogue for the old action UI; the console only lists/approves/retries/cancels actions. |
| `terrence_actions::request_terrence_action` | Actions are proposed by the Hermes runtime in Rust; the console never requests one over IPC. |
| `terrence_context::create_context_item` | Context items are created Rust-side; the console only lists, corrects and deletes them. |
| `terrence_priorities::record_terrence_decision` | Decisions are recorded by the runtime; no frontend caller. |
| `terrence_priorities::resolve_terrence_decision` | Called directly from `terrence_runtime.rs:1066`; the IPC entry point is unused. |
| `terrence_projects::upsert_terrence_project` | Called directly from `terrence_runtime.rs:1089`; the IPC entry point is unused. |

## (c) Archive candidates: unreachable files (for approval, nothing moved)

These 30 non-test files are not reachable from `main.tsx`. They're split by how safe a move is.
`src/terrance/system/*.test.ts` (3 files) test reachable modules and stay.

**Tier 1: no importers anywhere. Safe to archive as-is.**

| File | Reason |
|---|---|
| `src/build-stamp.d.ts` | Ambient types for `__BUILD_HASH__`/`__BUILD_TIME__`; nothing in src-console references them and tsconfig only includes src-console. |
| `src/context/ThemeContext.tsx` | Old UI theme provider; replaced by `src-console/design/useAppearance.ts`. |
| `src/hooks/useArm.ts` | Old Revenue page hook; Business › Revenue reads ARM through `bridge.ts`. |
| `src/hooks/useArtifactDetection.ts` | Old Neuro page; console uses `src-console/neuro`. |
| `src/hooks/useCalibration.ts` | Old Neuro calibration flow; no console screen uses it. |
| `src/hooks/useEegDisplay.ts` | Old EEG chart hook (ECharts era). |
| `src/hooks/useMuse.ts` | Old Muse hook; console uses `src-console/app/useMuseSession.ts`. |
| `src/hooks/useOpenClaw.ts` | Old Amara page OpenClaw card; console reads `read_openclaw_status` in bridge. |
| `src/hooks/useReplay.ts` | Old session replay UI. |
| `src/hooks/useScoreConfidence.ts` | Old neurofeedback score display. |
| `src/hooks/useSimpleSession.ts` | Old simple-session UI; replaced by `useMuseSession`. |
| `src/lib/chart-theme.ts` | ECharts theme; ECharts was removed from deps. |
| `src/lib/utils.ts` | Tailwind `cn()` helper; Tailwind was removed. |
| `src/terrance/system/contextCompressor.ts` | Not imported by continuity or anything else. |
| `src/terrance/system/openclawHealth.ts` | `pushHealthToN8n()` from the old Amara page; selfDiagnosis does health now. |

**Tier 2: imported only by other unreachable files. Archive together with their importers.**

| File | Reason |
|---|---|
| `src/hooks/useKeyboardShortcuts.ts` | Old shell shortcuts; App.tsx owns ⌘K / ⌘1–6. |
| `src/context/NexusContext.tsx` | Only used by `useKeyboardShortcuts.ts`. |
| `src/hooks/useNeurofeedbackSession.ts` | Old neurofeedback session runner; console uses `src-console/neuro/training.ts`. |
| `src/hooks/useAudioFeedback.ts` | Only used by `useNeurofeedbackSession.ts` and `actuators.ts`. |
| `src/lib/neurofeedback/actuators.ts` | Only used by `useNeurofeedbackSession.ts`. |
| `src/lib/neurofeedback/scoring.ts` | Only used by `useNeurofeedbackSession.ts`. |
| `src/hooks/useElectrode.ts` | Old electrode UI. |
| `src/lib/electrode/electrodePolicy.ts` | Only used by `useElectrode.ts`. |
| `src/lib/electrode/myndliftAdapter.ts` | Only used by `useElectrode.ts`. |
| `src/hooks/useGoals.ts` | Old goals hook; console reads `read_goals` in bridge. |
| `src/lib/events.ts` | Only used by `useGoals.ts`. |

**Tier 3: unreachable at runtime but still imported by live code. Don't move without a code change.**

| File | Reason |
|---|---|
| `src/terrance/system/telegramBridge.ts` | `scheduler.ts` imports `start/stopTelegramBridge` but only uses them in comments (bridge disabled: OpenClaw handles Telegram). The import must be removed first, or the build breaks. |
| `src/terrance/system/research.ts` | Only imported by `telegramBridge.ts`. Its command `web_search` stays registered (the `api.ts` wrapper is live). |

**Keep: type-only dependencies of live code. Not candidates.**

| File | Reason |
|---|---|
| `src/lib/eegRingBuffer.ts` | Types used by live `src/hooks/useSignalQuality.ts`. |
| `src/lib/neurofeedback/brainState.ts` | Types used by live `outputEngine.ts`, `policyEngine.ts` and `src-console/neuro/training.ts`. |

**Empty directories (no files):** `src-next/components`, `src-next/scene`, `src/styles`, `src/lib/whoop`,
`src/components/{arm,assistant,business,fitness,layout,neuro,settings,system,terrance,trajectory,ui}`.
`src-next/` contains no files at all.

`src-console/screens/TrajectoryScreen.tsx` was deleted on 2026-10-29 (Trajectory folded into the Home Goals card).

## (a) Commands and their callers (after the trim)

Registered: 129 · live: 129 · legacy-only: 0 · no callers: 0

| Command | Status | Live callers | Legacy callers |
|---|---|---|---|
| `get_system_info` | live | `src/lib/api.ts` | — |
| `health_check` | live | `src/lib/api.ts` | — |
| `check_docker_service` | live | `src/lib/api.ts`, `src/terrance/system/selfDiagnosis.ts` | — |
| `restart_docker_service` | live | `src/lib/api.ts` | — |
| `restart_services` | live | `src/terrance/system/selfDiagnosis.ts` | — |
| `check_service_port` | live | `src/lib/api.ts`, `src/terrance/system/selfDiagnosis.ts` | — |
| `read_openclaw_status` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `read_cron_jobs` | live | `src/lib/api.ts` | — |
| `read_skills` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts`, `src/terrance/system/skillsEvolution.ts` | — |
| `read_tasks` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `read_content_pipeline` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `read_daily_log` | live | `src/lib/api.ts` | — |
| `get_openclaw_health` | live | `src/lib/api.ts`, `src/terrance/system/selfDiagnosis.ts` | `src/terrance/system/openclawHealth.ts` |
| `restart_openclaw` | live | `src/lib/api.ts` | — |
| `save_skill` | live | `src/lib/api.ts`, `src/terrance/system/skillsEvolution.ts` | — |
| `delete_skill` | live | `src/lib/api.ts` | — |
| `scan_muse` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `connect_muse` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `disconnect_muse` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `start_eeg_stream` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `stop_eeg_stream` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `get_muse_state` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `get_noop_health_snapshot` | live | `src-console/bridge.ts` | — |
| `get_config` | live | `src/lib/api.ts` | — |
| `save_settings` | live | `src/lib/api.ts` | — |
| `get_telegram_messages` | live | `src/lib/api.ts` | — |
| `send_telegram_message` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts`, `src/terrance/system/customJobs.ts`, `src/terrance/system/rituals.ts`, `src/terrance/system/selfDiagnosis.ts`, `src/terrance/system/subagents.ts` | `src/terrance/system/telegramBridge.ts` |
| `test_telegram_connection` | live | `src/lib/api.ts` | — |
| `send_to_terrance` | live | `src/lib/api.ts` | — |
| `poll_telegram_updates` | live | `src/lib/api.ts` | — |
| `pack_it_up` | live | `src/lib/api.ts` | — |
| `unpack_backup` | live | `src/lib/api.ts` | — |
| `get_app_paths` | live | `src/lib/api.ts` | — |
| `read_goals` | live | `src-console/bridge.ts`, `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `save_goals` | live | `src/lib/api.ts` | — |
| `delete_goal` | live | `src/lib/api.ts` | — |
| `save_session` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `list_sessions` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `delete_session` | live | `src/lib/api.ts` | — |
| `save_snapshot` | live | `src/lib/api.ts` | — |
| `list_snapshots` | live | `src/lib/api.ts` | — |
| `check_onboarding` | live | `src/lib/api.ts` | — |
| `complete_onboarding` | live | `src/lib/api.ts` | — |
| `insert_events` | live | `src/lib/api.ts` | — |
| `query_events` | live | `src/lib/api.ts` | — |
| `migrate_existing_data` | live | `src/lib/api.ts` | — |
| `set_proof_mode` | live | `src/lib/api.ts` | — |
| `get_proof_log` | live | `src/lib/api.ts` | — |
| `get_outbox_status` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `simulate_gateway_down` | live | `src/lib/api.ts` | — |
| `simulate_gateway_up` | live | `src/lib/api.ts` | — |
| `arm_list_strategies` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `arm_create_strategy` | live | `src/lib/api.ts` | — |
| `arm_update_strategy` | live | `src/lib/api.ts` | — |
| `arm_delete_strategy` | live | `src/lib/api.ts` | — |
| `arm_activate_strategy` | live | `src/lib/api.ts` | — |
| `arm_pause_strategy` | live | `src/lib/api.ts` | — |
| `arm_kill_strategy` | live | `src/lib/api.ts` | — |
| `arm_poll_revenue` | live | `src/lib/api.ts` | — |
| `arm_get_revenue_summary` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `arm_log_decision` | live | `src/lib/api.ts` | — |
| `arm_list_decisions` | live | `src/lib/api.ts` | — |
| `arm_update_decision_outcome` | live | `src/lib/api.ts` | — |
| `arm_get_risk_status` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `arm_set_global_limits` | live | `src/lib/api.ts` | — |
| `arm_kill_switch` | live | `src/lib/api.ts` | — |
| `arm_health_check` | live | `src/lib/api.ts` | — |
| `arm_poll_all` | live | `src/lib/api.ts` | — |
| `arm_get_config` | live | `src/lib/api.ts` | — |
| `arm_save_config` | live | `src/lib/api.ts` | — |
| `read_identity_file` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `read_profile_file` | live | `src/terrance/system/continuity.ts` | — |
| `load_kv` | live | `src/terrance/system/rituals.ts`, `src/terrance/system/selfDiagnosis.ts` | — |
| `save_kv` | live | `src/terrance/system/rituals.ts`, `src/terrance/system/selfDiagnosis.ts` | — |
| `load_terrance_context` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `save_terrance_context` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `store_session_log` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts`, `src/terrance/system/selfDiagnosis.ts` | `src/terrance/system/research.ts` |
| `list_session_logs` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts`, `src/terrance/system/skillsEvolution.ts` | — |
| `archive_old_session_logs` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `prune_context_memory` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts` | — |
| `load_custom_jobs` | live | `src/lib/api.ts`, `src/terrance/system/customJobs.ts` | — |
| `save_custom_jobs` | live | `src/lib/api.ts`, `src/terrance/system/customJobs.ts` | — |
| `get_session_insights` | live | `src/lib/api.ts` | — |
| `store_activity_entry` | live | `src/lib/api.ts`, `src/terrance/system/activityBus.ts` | — |
| `list_activity_entries` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `prune_activity_log` | live | `src/lib/api.ts` | — |
| `load_subagents` | live | `src/lib/api.ts`, `src/terrance/system/subagents.ts` | — |
| `save_subagents` | live | `src/lib/api.ts`, `src/terrance/system/subagents.ts` | — |
| `chat_with_router` | live | `src/lib/api.ts`, `src/terrance/system/continuity.ts`, `src/terrance/system/customJobs.ts`, `src/terrance/system/deliberation.ts`, `src/terrance/system/skillsEvolution.ts`, `src/terrance/system/subagents.ts` | `src/terrance/system/contextCompressor.ts`, `src/terrance/system/research.ts`, `src/terrance/system/telegramBridge.ts` |
| `get_cost_estimate` | live | `src/lib/api.ts` | — |
| `get_llm_usage` | live | `src/lib/api.ts` | — |
| `clear_llm_usage` | live | `src/lib/api.ts` | — |
| `list_terrence_actions` | live | `src-console/bridge.ts` | — |
| `approve_terrence_action` | live | `src-console/bridge.ts` | — |
| `retry_terrence_action` | live | `src-console/bridge.ts` | — |
| `cancel_terrence_action` | live | `src-console/bridge.ts` | — |
| `list_terrence_automations` | live | `src-console/bridge.ts` | — |
| `list_terrence_automation_runs` | live | `src-console/bridge.ts` | — |
| `list_context_items` | live | `src-console/bridge.ts` | — |
| `correct_context_item` | live | `src-console/bridge.ts` | — |
| `delete_context_item` | live | `src-console/bridge.ts` | — |
| `get_terrence_context_snapshot` | live | `src-console/bridge.ts` | — |
| `migrate_legacy_terrence_context` | live | `src-console/bridge.ts` | — |
| `get_current_terrence_priority_snapshot` | live | `src-console/bridge.ts` | — |
| `list_terrence_decisions` | live | `src-console/bridge.ts` | — |
| `list_terrence_projects` | live | `src-console/bridge.ts` | — |
| `get_terrence_runtime_health` | live | `src-console/bridge.ts` | — |
| `list_terrence_capabilities` | live | `src-console/bridge.ts` | — |
| `interact_with_terrence` | live | `src-console/bridge.ts` | — |
| `list_terrence_interactions` | live | `src-console/bridge.ts` | — |
| `create_chat_session` | live | `src/lib/api.ts` | — |
| `list_chat_sessions` | live | `src/lib/api.ts` | — |
| `save_chat_message` | live | `src/lib/api.ts` | — |
| `load_chat_history` | live | `src/lib/api.ts` | — |
| `delete_chat_session` | live | `src/lib/api.ts` | — |
| `web_search` | live | `src/lib/api.ts` | `src/terrance/system/research.ts` |
| `fetch_url` | live | `src/lib/api.ts` | `src/terrance/system/research.ts` |
| `start_eeg_recording` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `stop_eeg_recording` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `list_replays` | live | `src/lib/api.ts` | — |
| `start_replay` | live | `src/lib/api.ts` | — |
| `stop_replay` | live | `src/lib/api.ts` | — |
| `start_demo_eeg` | live | `src/lib/api.ts` | — |
| `stop_demo_eeg` | live | `src/lib/api.ts` | — |
| `crew_list` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `crew_start` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `crew_status` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `crew_stop` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |
| `crew_health` | live | `src-console/bridge.ts`, `src/lib/api.ts` | — |

## (b) Every file in src/ and src-next/

| File | Reachable from main.tsx |
|---|---|
| `src/build-stamp.d.ts` | no |
| `src/context/NexusContext.tsx` | no |
| `src/context/ThemeContext.tsx` | no |
| `src/hooks/useArm.ts` | no |
| `src/hooks/useArtifactDetection.ts` | no |
| `src/hooks/useAudioFeedback.ts` | no |
| `src/hooks/useCalibration.ts` | no |
| `src/hooks/useEegDisplay.ts` | no |
| `src/hooks/useElectrode.ts` | no |
| `src/hooks/useFFT.ts` | yes |
| `src/hooks/useGoals.ts` | no |
| `src/hooks/useKeyboardShortcuts.ts` | no |
| `src/hooks/useMuse.ts` | no |
| `src/hooks/useNeurofeedbackSession.ts` | no |
| `src/hooks/useOpenClaw.ts` | no |
| `src/hooks/useReplay.ts` | no |
| `src/hooks/useScoreConfidence.ts` | no |
| `src/hooks/useSignalQuality.ts` | yes |
| `src/hooks/useSimpleSession.ts` | no |
| `src/lib/api.ts` | yes |
| `src/lib/artifactDetection.ts` | yes |
| `src/lib/chart-theme.ts` | no |
| `src/lib/eegRingBuffer.ts` | no |
| `src/lib/electrode/electrodePolicy.ts` | no |
| `src/lib/electrode/myndliftAdapter.ts` | no |
| `src/lib/events.ts` | no |
| `src/lib/fft.ts` | yes |
| `src/lib/llm-router.ts` | yes |
| `src/lib/neurofeedback/actuators.ts` | no |
| `src/lib/neurofeedback/brainState.ts` | no |
| `src/lib/neurofeedback/outputEngine.ts` | yes |
| `src/lib/neurofeedback/policyEngine.ts` | yes |
| `src/lib/neurofeedback/protocols.ts` | yes |
| `src/lib/neurofeedback/safetyGate.ts` | yes |
| `src/lib/neurofeedback/scoring.ts` | no |
| `src/lib/types.ts` | yes |
| `src/lib/utils.ts` | no |
| `src/terrance/system/activityBus.ts` | yes |
| `src/terrance/system/approvalErrors.ts` | yes |
| `src/terrance/system/backoff.ts` | yes |
| `src/terrance/system/contextCompressor.ts` | no |
| `src/terrance/system/continuity.ts` | yes |
| `src/terrance/system/cronParser.ts` | yes |
| `src/terrance/system/customJobs.ts` | yes |
| `src/terrance/system/deliberation.ts` | yes |
| `src/terrance/system/injectionGuard.ts` | yes |
| `src/terrance/system/openclawHealth.ts` | no |
| `src/terrance/system/research.ts` | no |
| `src/terrance/system/rituals.ts` | yes |
| `src/terrance/system/scheduler.health.test.ts` | no (test file) |
| `src/terrance/system/scheduler.nextRuns.test.ts` | no (test file) |
| `src/terrance/system/scheduler.ts` | yes |
| `src/terrance/system/selfDiagnosis.autoheal.test.ts` | no (test file) |
| `src/terrance/system/selfDiagnosis.ts` | yes |
| `src/terrance/system/skillsEvolution.ts` | yes |
| `src/terrance/system/subagents.ts` | yes |
| `src/terrance/system/telegramBridge.ts` | no |
