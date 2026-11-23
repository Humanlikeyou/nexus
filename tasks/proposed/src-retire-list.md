# src/ retire list and now-unused commands (candidates only, nothing deleted)

Status: PROPOSED. Written 2026-11-14 at typed-bridge step 5. Removal of anything below is BLOCKED until Taisun logs it in `docs/decisions/` (hard rule 8; no destructive action without his OK; the Rust removals also need security review).

## What happened to src/

An import-graph walk (tsc `--listFiles` over `src-console` plus every vitest file in `src-console`, `test`; cross-checked with an esbuild metafile from `src-console/main.tsx`) found 33 non-test `src/` files reachable from the console. The plan's list was right, plus four it did not name directly: `lib/eegRingBuffer`, `lib/neurofeedback/brainState` (type-only imports), `terrance/system/research` and `terrance/system/telegramBridge` (imported by the scheduler / each other, tree-shaken at runtime but still in the type graph). All 33 plus their 20 test files moved to `src-console/legacy-core/{lib,hooks,terrance/system}` (same sub-folders). Nothing in `src-console`, `test` imports from `src/` any more.

## Unreachable src/ files (11 remain)

| File | Why it is dead (importers) |
|---|---|
| src/context/NexusContext.tsx | only importer is src/hooks/useKeyboardShortcuts.ts (itself dead) |
| src/hooks/useKeyboardShortcuts.ts | no importer |
| src/hooks/useNeurofeedbackSession.ts | no importer |
| src/hooks/useElectrode.ts | only importer is its own test src/hooks/useElectrode.test.ts |
| src/hooks/useElectrode.test.ts | tests dead code; still runs (vitest includes `src/**`) |
| src/hooks/useGoals.ts | only importer is its own test src/hooks/useGoals.test.ts |
| src/hooks/useGoals.test.ts | tests dead code; still runs |
| src/lib/electrode/myndliftAdapter.ts | only importers are useElectrode.ts and its test |
| src/lib/events.ts | only importer is useGoals.ts |
| src/lib/neurofeedback/actuators.ts | only importer is useNeurofeedbackSession.ts |
| src/lib/neurofeedback/scoring.ts | only importer is useNeurofeedbackSession.ts |

Notes for whoever retires them:
- Their imports of moved modules were rewritten to point at `src-console/legacy-core/...` so the two remaining tests keep running (vitest count unchanged). They are NOT type-checked (tsconfig includes only `src-console`) or linted.
- Retiring `useElectrode.test.ts` and `useGoals.test.ts` lowers the vitest count by those files' tests; that needs the written reason the verification rules ask for.
- After they go, `@` in vite.config.ts / vitest.config.ts / tsconfig `paths` (`@/*` -> `src/*`) and the `src/**` entry in vitest `include` are unused and can be dropped, and `scripts/command-inventory.mjs` FRONTEND_DIRS can lose `src` / `src-next`.
- CLAUDE.md still says src/ is unused; Taisun's uncommitted edit, not touched here. It is now true of what remains in src/ (and false before this step).

## Registered Rust commands that nothing in src-console calls (candidates)

Method: a command counts as called when its name appears as a string literal, or `bound.<camel>` / `commands.<camel>` appears, in any non-test, non-generated, non-`boundTable.ts` file under `src-console`.

No caller at all (9):
- `hr_window` (home_hr_take)
- `life_date_delete`, `life_date_preview` (life_dates)
- `followups_session_edits` (followups)
- `fx_rate_set_manual` (commands/money.rs; named in a test)
- `backup_prune_plan`, `restore_preview` (commands/backup_schedule.rs; named in a test; Taisun's gates on restore/prune apply)
- `notify_dispatch_due` (notify_relay)
- `key_file_unlock` (key_file)

Called only from dead exports of `src-console/legacy-core/lib/api.ts` (50; these were bound only so that file could stop using raw invoke; deleting the dead exports first would let them be unbound):
- `get_system_info` (only via `api.ts` export `getSystemInfo`, which nothing imports)
- `save_settings` (only via `api.ts` export `saveSettings`, which nothing imports)
- `get_telegram_messages` (only via `api.ts` export `getTelegramMessages`, which nothing imports)
- `test_telegram_connection` (only via `api.ts` export `testTelegramConnection`, which nothing imports)
- `send_to_terrance` (only via `api.ts` export `sendToTerrance`, which nothing imports)
- `get_app_paths` (only via `api.ts` export `getAppPaths`, which nothing imports)
- `save_snapshot` (only via `api.ts` export `saveSnapshot`, which nothing imports)
- `list_snapshots` (only via `api.ts` export `listSnapshots`, which nothing imports)
- `insert_events` (only via `api.ts` export `insertEvents`, which nothing imports)
- `query_events` (only via `api.ts` export `queryEvents`, which nothing imports)
- `migrate_existing_data` (only via `api.ts` export `migrateExistingData`, which nothing imports)
- `create_chat_session` (only via `api.ts` export `createChatSession`, which nothing imports)
- `list_chat_sessions` (only via `api.ts` export `listChatSessions`, which nothing imports)
- `save_chat_message` (only via `api.ts` export `saveChatMessage`, which nothing imports)
- `load_chat_history` (only via `api.ts` export `loadChatHistory`, which nothing imports)
- `delete_chat_session` (only via `api.ts` export `deleteChatSession`, which nothing imports)
- `check_onboarding` (only via `api.ts` export `checkOnboarding`, which nothing imports)
- `complete_onboarding` (only via `api.ts` export `completeOnboarding`, which nothing imports)
- `arm_create_strategy` (only via `api.ts` export `armCreateStrategy`, which nothing imports)
- `arm_update_strategy` (only via `api.ts` export `armUpdateStrategy`, which nothing imports)
- `arm_delete_strategy` (only via `api.ts` export `armDeleteStrategy`, which nothing imports)
- `arm_activate_strategy` (only via `api.ts` export `armActivateStrategy`, which nothing imports)
- `arm_pause_strategy` (only via `api.ts` export `armPauseStrategy`, which nothing imports)
- `arm_kill_strategy` (only via `api.ts` export `armKillStrategy`, which nothing imports)
- `arm_poll_revenue` (only via `api.ts` export `armPollRevenue`, which nothing imports)
- `arm_log_decision` (only via `api.ts` export `armLogDecision`, which nothing imports)
- `arm_list_decisions` (only via `api.ts` export `armListDecisions`, which nothing imports)
- `arm_update_decision_outcome` (only via `api.ts` export `armUpdateDecisionOutcome`, which nothing imports)
- `arm_set_global_limits` (only via `api.ts` export `armSetGlobalLimits`, which nothing imports)
- `arm_kill_switch` (only via `api.ts` export `armKillSwitch`, which nothing imports)
- `arm_health_check` (only via `api.ts` export `armHealthCheck`, which nothing imports)
- `arm_poll_all` (only via `api.ts` export `armPollAll`, which nothing imports)
- `arm_get_config` (only via `api.ts` export `armGetConfig`, which nothing imports)
- `arm_save_config` (only via `api.ts` export `armSaveConfig`, which nothing imports)
- `set_proof_mode` (only via `api.ts` export `setProofMode`, which nothing imports)
- `get_proof_log` (only via `api.ts` export `getProofLog`, which nothing imports)
- `simulate_gateway_down` (only via `api.ts` export `simulateGatewayDown`, which nothing imports)
- `simulate_gateway_up` (only via `api.ts` export `simulateGatewayUp`, which nothing imports)
- `list_replays` (only via `api.ts` export `listReplays`, which nothing imports)
- `start_replay` (only via `api.ts` export `startReplayFile`, which nothing imports)
- `stop_replay` (only via `api.ts` export `stopReplay`, which nothing imports)
- `start_demo_eeg` (only via `api.ts` export `startDemoEeg`, which nothing imports)
- `stop_demo_eeg` (only via `api.ts` export `stopDemoEeg`, which nothing imports)
- `get_cost_estimate` (only via `api.ts` export `getCostEstimate`, which nothing imports)
- `get_llm_usage` (only via `api.ts` export `getLlmUsage`, which nothing imports)
- `clear_llm_usage` (only via `api.ts` export `clearLlmUsage`, which nothing imports)
- `get_session_insights` (only via `api.ts` export `getSessionInsights`, which nothing imports)
- `delete_skill` (only via `api.ts` export `deleteSkill`, which nothing imports)
- `restart_docker_service` (only via `api.ts` export `restartDockerService`, which nothing imports)
- `prune_activity_log` (only via `api.ts` export `pruneActivityLog`, which nothing imports)

Do not remove any of these Rust commands without Taisun's logged OK. The Muse/recordings rules and the backup/restore gates in `.claude/rules/` apply to the ones that touch them.
