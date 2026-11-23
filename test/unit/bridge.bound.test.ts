// Bridge functions backed by generated bindings (src-console/bindings.gen.ts): same command name,
// same payload keys, same resolve/reject behaviour as the old hand-written invoke (amara_turn included, at the bottom).
import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), Channel: class { onmessage: ((e: unknown) => void) | null = null } }));

import * as bridge from "../../src-console/bridge";
import { invokeBound } from "../../src-console/bridge";
import { BOUND_TABLE } from "../../src-console/boundTable";
import { amaraTurnCancel, getN8nStatus, loadKv, loadSystemSnapshot, recordScreenOpen, saveKv, screenOpenCounts } from "../../src-console/bridge";

beforeEach(() => { invoke.mockReset(); });
// What reaches the Rust side: JSON drops undefined keys, so absent optionals must not appear as null.
const wire = (p: unknown) => (p === undefined ? {} : JSON.parse(JSON.stringify(p)));
const sentWire = () => invoke.mock.calls.map((c) => [c[0], wire(c[1])]);
const sent = () => invoke.mock.calls.map((c) => c.slice(0, 2));

describe("bridge functions on generated bindings", () => {
  it("loadKv", async () => {
    invoke.mockResolvedValue("v");
    await expect(loadKv("k")).resolves.toBe("v");
    expect(sent()).toEqual([["load_kv", { key: "k" }]]);
    invoke.mockRejectedValue("reserved_key");
    await expect(loadKv("k")).rejects.toBe("reserved_key");
  });

  it("saveKv resolves with the raw backend value and rejects on error", async () => {
    invoke.mockResolvedValue(null);
    await expect(saveKv("k", "v")).resolves.toBeNull();
    expect(sent()).toEqual([["save_kv", { key: "k", value: "v" }]]);
    invoke.mockRejectedValue("disk full");
    await expect(saveKv("k", "v")).rejects.toBe("disk full");
  });

  it("recordScreenOpen swallows errors", async () => {
    invoke.mockResolvedValue(null);
    await recordScreenOpen("life");
    expect(sent()).toEqual([["screen_open_record", { screen: "life" }]]);
    invoke.mockRejectedValue("no backend");
    await expect(recordScreenOpen("life")).resolves.toBeUndefined();
  });

  it("screenOpenCounts sends weeks (null when omitted) and surfaces errors", async () => {
    invoke.mockResolvedValue([]);
    await screenOpenCounts(4);
    await screenOpenCounts();
    expect(sent()).toEqual([["screen_open_counts", { weeks: 4 }], ["screen_open_counts", { weeks: null }]]);
    invoke.mockRejectedValue("store_error");
    await expect(screenOpenCounts()).rejects.toBe("store_error");
  });

  it("getN8nStatus sends no args", async () => {
    invoke.mockResolvedValue({ connected: false, workflows: null, failures7d: null });
    await expect(getN8nStatus()).resolves.toEqual({ connected: false, workflows: null, failures7d: null });
    expect(sent()).toEqual([["n8n_status", undefined]]);
    invoke.mockRejectedValue("boom");
    await expect(getN8nStatus()).rejects.toBe("boom");
  });

  it("amaraTurnCancel sends turnId (camelCase)", async () => {
    invoke.mockResolvedValue(true);
    await expect(amaraTurnCancel("t1")).resolves.toBe(true);
    expect(sent()).toEqual([["amara_turn_cancel", { turnId: "t1" }]]);
    invoke.mockRejectedValue("x");
    await expect(amaraTurnCancel("t1")).rejects.toBe("x");
  });
});

// [bridge function, call args, expected command, expected payload (undefined = no payload)]
const TABLE: Array<[string, unknown[], string, unknown]> = [
["startCrew", ["n","t"], "crew_start", {name:"n",task:"t",agents:null,retryOf:null}],
["startCrew", ["n","t","r1"], "crew_start", {name:"n",task:"t",agents:null,retryOf:"r1"}],
["listCrewApprovals", ["pending"], "crew_list_approvals", {state:"pending"}],
["decideCrewApproval", ["a1",true], "crew_decide_approval", {id:"a1",approve:true}],
["getCrewStatus", ["c1"], "crew_status", {id:"c1"}],
["getCrewRunSteps", ["c1"], "crew_run_steps", {id:"c1"}],
["stopCrew", ["c1"], "crew_stop", {id:"c1"}],
["getCrewToolGrant", ["c1"], "crew_tool_grant_get", {crewId:"c1"}],
["setCrewToolGrant", ["c1",["bash"],"1h"], "crew_tool_grant_set", {crewId:"c1",tools:["bash"],duration:"1h"}],
["revokeCrewToolGrant", ["c1"], "crew_tool_grant_revoke", {crewId:"c1"}],
["getCrewToolAudit", ["c1"], "crew_tool_audit", {crewId:"c1",limit:50,before:null}],
["getCrewToolAudit", ["c1",10,99], "crew_tool_audit", {crewId:"c1",limit:10,before:99}],
["interactWithTerrence", [{message:"m",conversationId:"c",hermesSessionId:null}], "interact_with_terrence", {request:{message:"m",conversationId:"c",hermesSessionId:null}}],
["prepareAmaraTurn", ["hi"], "prepare_amara_turn", {message:"hi"}],
["recordAmaraInteraction", [{conversationId:"c",userMessage:"u",assistantMessage:"a",tier:null,durationMs:5}], "record_amara_interaction", {input:{conversationId:"c",userMessage:"u",assistantMessage:"a",tier:null,durationMs:5}}],
["listTerrenceInteractions", ["c"], "list_terrence_interactions", {conversationId:"c",limit:80}],
["migrateLegacyTerrenceContext", [], "migrate_legacy_terrence_context", undefined],
["listTerrenceContextItems", [], "list_context_items", {query:{limit:120}}],
["correctTerrenceContextItem", [{originalId:"o",replacement:{kind:"goal",title:"t"}}], "correct_context_item", {input:{originalId:"o",replacement:{kind:"goal",title:"t"}}}],
["deleteTerrenceContextItem", ["i"], "delete_context_item", {id:"i"}],
["approveTerrenceAction", ["i"], "approve_terrence_action", {id:"i"}],
["retryTerrenceAction", ["i"], "retry_terrence_action", {id:"i"}],
["cancelTerrenceAction", ["i"], "cancel_terrence_action", {id:"i"}],
["getReduceTransparency", [], "get_reduce_transparency", undefined],
["scanMuse", [], "scan_muse", undefined],
["connectMuse", ["AA:BB"], "connect_muse", {address:"AA:BB"}],
["disconnectMuse", [], "disconnect_muse", undefined],
["startEegStream", [], "start_eeg_stream", undefined],
["stopEegStream", [], "stop_eeg_stream", undefined],
["startEegRecording", [], "start_eeg_recording", undefined],
["stopEegRecording", [], "stop_eeg_recording", undefined],
["listRecordings", [], "list_recordings", undefined],
["readRecordingWindow", ["r",1,2], "read_recording_window", {id:"r",startSecs:1,spanSecs:2}],
["readRecordingWindow", ["r",1,2,300], "read_recording_window", {id:"r",startSecs:1,spanSecs:2,points:300}],
["readBodyWindow", ["r",1,2], "read_body_window", {id:"r",startSecs:1,spanSecs:2}],
["recordingSeries", ["r"], "recording_series", {id:"r"}],
["getSessionSamples", ["s"], "get_session_samples", {sessionId:"s"}],
["markRecordingAux", [{channel:"AF7"}], "mark_recording_aux", {aux:{channel:"AF7"}}],
["saveMuseSession", [{id:"s",file_path:"p"},{k:1}], "save_session", {session:{id:"s",file_path:"p"},data:{k:1}}],
["getAllowClaude", [], "get_allow_claude", undefined],
["setAllowClaude", [true], "set_allow_claude", {enabled:true}],
["getVoiceStatus", [], "voice_status", undefined],
["startVoice", [], "voice_start", undefined],
["stopVoice", [], "voice_stop", undefined],
["getVoiceTurns", [], "voice_recent_turns", {n:20}],
["getVoiceTurns", [5], "voice_recent_turns", {n:5}],
["listVoices", [], "voice_list_voices", undefined],
["setVoice", ["v"], "voice_set_voice", {name:"v"}],
["stopVoiceSpeaking", [], "voice_stop_speaking", undefined],
["listFollowups", [], "followups_list", undefined],
["recordExperimentStart", [3,null], "experiments_record_start", {followupId:3,pair:null}],
["listExperiments", [], "experiments_list", undefined],
["hemisyncRanking", [], "hemisync_ranking", undefined],
["addFollowupItem", [{text:"t"}], "followups_add_item", {text:"t"}],
["addFollowupItem", [{text:"t",due:"2026-11-20",next:"n"}], "followups_add_item", {text:"t",due:"2026-11-20",next:"n"}],
["updateFollowupItem", [4,{text:"x"}], "followups_update_item", {id:4,patch:{text:"x"}}],
["markFollowupDone", [4], "followups_mark_done", {id:4}],
["reopenFollowupItem", [4], "followups_reopen_item", {id:4}],
["deleteFollowupItem", [4], "followups_delete_item", {id:4}],
["addMutedTopic", ["t"], "followups_add_muted_topic", {topic:"t"}],
["addMutedTopic", ["t","2026-12-01"], "followups_add_muted_topic", {topic:"t",until:"2026-12-01"}],
["deleteMutedTopic", ["t"], "followups_delete_muted_topic", {topic:"t"}],
["getHomeBrief", [], "home_brief", undefined],
["getLiveHeartRate", [], "live_heart_rate", undefined],
["getRecentWorkouts", [], "noop_recent_workouts", {days:14}],
["getRecentWorkouts", [3], "noop_recent_workouts", {days:3}],
["getHypnogram", [], "noop_hypnogram", {day:null}],
["getHypnogram", ["2026-11-13"], "noop_hypnogram", {day:"2026-11-13"}],
["dailyRebuild", [], "daily_rebuild", undefined],
["dailyRange", ["a","b"], "daily_range", {from:"a",to:"b"}],
["goalProgressWeek", [], "goal_progress_week", {weekEnd:null}],
["goalProgressWeek", ["2026-11-14"], "goal_progress_week", {weekEnd:"2026-11-14"}],
];

const SNAPSHOT_READS: Array<[string, unknown]> = [
  ["get_terrence_runtime_health", undefined], ["list_terrence_capabilities", undefined], ["get_terrence_context_snapshot", undefined],
  ["get_current_terrence_priority_snapshot", undefined], ["list_terrence_decisions", undefined], ["list_terrence_actions", undefined],
  ["list_terrence_automations", undefined], ["list_terrence_automation_runs", undefined],
  ["list_terrence_projects", undefined], ["get_noop_health_snapshot", undefined], ["get_muse_state", undefined], ["list_sessions", undefined],
  ["read_tasks", { workspacePath: null }], ["read_content_pipeline", { workspacePath: null }], ["arm_get_revenue_summary", { from: null, to: null }],
  ["arm_list_strategies", undefined], ["arm_get_risk_status", undefined], ["read_goals", undefined], ["get_outbox_status", undefined],
  ["list_activity_entries", { limit: 30 }], ["crew_health", undefined], ["crew_list", undefined], ["crew_tools", undefined], ["kb_status", undefined],
  ["read_business_ledger", undefined],
];

describe("table: every bound bridge function sends the expected command and payload", () => {
  it.each(TABLE)("%s -> %s", async (fn, args, command, payload) => {
    const call = (bridge as unknown as Record<string, (...a: unknown[]) => Promise<unknown>>)[fn];
    expect(typeof call).toBe("function");
    invoke.mockResolvedValue(null);
    await call(...args);
    expect(sentWire()).toEqual([[command, wire(payload)]]);
    // a rejection reaches the caller unchanged
    invoke.mockRejectedValue("boom");
    await expect(call(...args)).rejects.toBe("boom");
  });

  it("table covers at least 60 distinct bridge functions", () => {
    const fns = new Set(TABLE.map((r) => r[0]));
    expect(fns.size).toBeGreaterThanOrEqual(60);
  });

  it("loadSystemSnapshot reads each source through the bindings with the same payload keys", async () => {
    invoke.mockRejectedValue("offline");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await loadSystemSnapshot();
    warn.mockRestore();
    expect(sentWire()).toEqual(SNAPSHOT_READS.map(([c, p]) => [c, wire(p)]));
  });
});

// Every command bound in step 4 (the callers outside bridge.ts): name, args object, and that the same payload reaches invoke.
const CALLER_COMMANDS: Array<[string, Record<string, unknown> | undefined]> = [
  ["get_noop_settings", undefined],
  ["set_noop_settings", {settings: "settings-v"}],
  ["sync_dock_icon", undefined],
  ["chat_with_router", {text: "text-v", config: "config-v", tags: "tags-v", forceProvider: "forceProvider-v", anthropicApiKey: "anthropicApiKey-v", deep: "deep-v", localOnly: "localOnly-v"}],
  ["roadmap_get", {goalId: "goalId-v"}],
  ["roadmap_todays_question", {today: "today-v"}],
  ["roadmap_request", {goalId: "goalId-v"}],
  ["system_map_status", undefined],
  ["notify_preview", {kind: "kind-v"}],
  ["secrets_status", undefined],
  ["key_file_status", undefined],
  ["backup_settings_get", undefined],
  ["backup_list", undefined],
  ["notify_settings_get", undefined],
  ["noop_parity_report", {days: "days-v"}],
  ["noop_binary_status", undefined],
  ["noop_freeze_report", undefined],
  ["noop_freeze_copy", undefined],
  ["keychain_move_review", undefined],
  ["keychain_move_one", {name: "name-v"}],
  ["secret_set", {name: "name-v", value: "value-v"}],
  ["backup_now", undefined],
  ["backup_settings_set", {patch: "patch-v"}],
  ["notify_settings_set", {patch: "patch-v"}],
  ["notify_send_test", undefined],
  ["key_file_make", {passphrase: "passphrase-v", replace: "replace-v"}],
  ["backup_verify_last", undefined],
  ["take_events_list", undefined],
  ["take_events_append", {event: "event-v"}],
  ["take_log_migrate", undefined],
  ["take_log_list", undefined],
  ["take_log_forget", {id: "id-v"}],
  ["take_log_append", {entry: "entry-v"}],
  ["noop_recent_workouts", {days: "days-v"}],
  ["ha_states", undefined],
  ["home_hr_take_status", undefined],
  ["home_hr_take_maybe_refresh", {trigger: "trigger-v"}],
  ["home_window_open", {id: "id-v", kind: "kind-v", target: "target-v"}],
  ["home_window_close", {id: "id-v"}],
  ["home_window_focus", {id: "id-v", grab: "grab-v"}],
  ["home_window_pop_out", {id: "id-v"}],
  ["home_window_layout", {rects: "rects-v"}],
  ["home_window_preflight", undefined],
  ["home_window_sites", undefined],
  ["home_window_sign_out", {site: "site-v"}],
  ["home_window_undo", undefined],
  ["ha_get_config", undefined],
  ["ha_set_config", {url: "url-v", token: "token-v"}],
  ["ha_clear_token", undefined],
  ["ha_test_connection", {url: "url-v", token: "token-v"}],
  ["noop_query", {tool: "tool-v", args: "args-v"}],
  ["noop_query_batch", {calls: "calls-v"}],
  ["ha_call_service", {domain: "domain-v", service: "service-v", entityId: "entityId-v", data: "data-v"}],
  ["ha_devices_list", undefined],
  ["ha_device_set", {entityId: "entityId-v", service: "service-v", value: "value-v", via: "via-v"}],
  ["ha_conversation", {text: "text-v"}],
  ["ha_container_status", undefined],
  ["ha_container_start", undefined],
  ["ha_container_stop", undefined],
  ["ha_live_start", undefined],
  ["ha_live_stop", undefined],
  ["needs_you_list", undefined],
  ["agents_now", undefined],
  ["hemisync_library", undefined],
  ["hemisync_sessions", undefined],
  ["hemisync_stats", undefined],
  ["hemisync_import", {folder: "folder-v"}],
  ["set_local_only", {on: "on-v"}],
  ["outbound_get", undefined],
  ["outbound_set", {enabled: "enabled-v"}],
  ["login_item_status", undefined],
  ["login_item_enable", undefined],
  ["login_item_disable", undefined],
  ["update_status", undefined],
  ["update_check", undefined],
  ["update_install_now", undefined],
  ["update_set_prefs", {patch: "patch-v"}],
  ["voice_speak_ready", undefined],
  ["voice_speak_local", {text: "text-v"}],
  ["voice_dictate_ready", undefined],
  ["dictate_once", {purpose: "purpose-v", maxSecs: "maxSecs-v"}],
  ["life_date_list", undefined],
  ["checkin_list", {limit: "limit-v"}],
  ["patterns_read", undefined],
  ["weekly_review_read", undefined],
  ["memory_list", undefined],
  ["checkin_save", {draft: "draft-v"}],
  ["decisions_list", undefined],
  ["decisions_append", {draft: "draft-v"}],
  ["decisions_undo", {id: "id-v"}],
  ["decisions_add_outcome", {id: "id-v", outcome: "outcome-v"}],
  ["money_accounts_list", undefined],
  ["fx_rate_get", undefined],
  ["money_balances_list", undefined],
  ["money_import_pick", undefined],
  ["treasury_stream_add", {ventureId: "ventureId-v", name: "name-v", streamType: "streamType-v", setupDate: null, notes: null}],
  ["treasury_stream_update", {id: "id-v", status: null, name: null, notes: null}],
  ["treasury_streams_list", undefined],
  ["treasury_import_pick", {streamId: "streamId-v", adapterId: "adapterId-v", mapping: null}],
  ["treasury_import_preview", {streamId: "streamId-v", adapterId: "adapterId-v", mapping: null}],
  ["treasury_attention_set", {streamId: "streamId-v", day: "day-v", minutes: "minutes-v"}],
  ["treasury_attention_list", {streamId: null, from: "from-v", to: "to-v"}],
  ["treasury_income_month", {fromMonth: "fromMonth-v", toMonth: "toMonth-v"}],
  ["treasury_income_trend", {streamId: null}],
  ["treasury_per_hour", {from: "from-v", to: "to-v"}],
  ["treasury_fx_status", undefined],
  ["finances_view", undefined],
  ["venture_tracking_list", undefined],
  ["content_stage_set", {title: "title-v", from: "from-v", to: "to-v", postedByMe: "postedByMe-v", via: null}],
  ["content_draft_propose", {title: "title-v"}],
  ["content_draft_save", {title: "title-v", text: "text-v", via: null}],
  ["content_draft_get", {title: "title-v"}],
  ["goal_venture_set", {goalId: "goalId-v", ventureId: null, via: null}],
  ["treasury_counts_import_pick", {streamId: "streamId-v", mapping: "mapping-v"}],
  ["treasury_counts_list", {streamId: "streamId-v"}],
  ["treasury_sequence_draft", {kind: "kind-v", ventureId: "ventureId-v"}],
  ["treasury_sequence_list", undefined],
  ["treasury_sequence_update", {id: "id-v", emails: "emails-v"}],
  ["treasury_sequence_approve", {id: "id-v"}],
  ["treasury_results_import_pick", {sequenceId: "sequenceId-v", mapping: "mapping-v"}],
  ["treasury_results_list", {sequenceId: null}],
  ["money_account_add", {draft: "draft-v"}],
  ["money_watch_add", {accountId: "accountId-v", value: "value-v"}],
  ["money_balance_set_manual", {accountId: "accountId-v", nativeAmount: "nativeAmount-v", asOf: "asOf-v"}],
  ["add_history", {kind: "kind-v"}],
  ["add_preview", {kind: "kind-v", text: "text-v"}],
  ["add_save", {kind: "kind-v", preview: "preview-v", via: "via-v"}],
  ["add_undo", {kind: "kind-v", id: "id-v"}],
  ["voice_bridge_start", undefined],
  ["voice_bridge_stop", undefined],
  ["get_system_info", undefined],
  ["health_check", undefined],
  ["read_skills", undefined],
  ["get_config", undefined],
  ["save_settings", {config: "config-v"}],
  ["get_telegram_messages", {limit: "limit-v"}],
  ["send_telegram_message", {text: "text-v"}],
  ["test_telegram_connection", undefined],
  ["send_to_terrance", {text: "text-v"}],
  ["poll_telegram_updates", {lastUpdateId: "lastUpdateId-v"}],
  ["pack_it_up", undefined],
  ["unpack_backup", {archivePath: "archivePath-v"}],
  ["restore_backup", {file: "file-v"}],
  ["get_app_paths", undefined],
  ["save_goals", {goals: "goals-v"}],
  ["delete_goal", {goalId: "goalId-v"}],
  ["save_snapshot", {snapshot: "snapshot-v"}],
  ["list_snapshots", undefined],
  ["insert_events", {events: "events-v"}],
  ["query_events", {query: "query-v"}],
  ["migrate_existing_data", undefined],
  ["create_chat_session", {title: "title-v"}],
  ["list_chat_sessions", undefined],
  ["save_chat_message", {message: "message-v"}],
  ["load_chat_history", {sessionId: "sessionId-v", limit: "limit-v"}],
  ["delete_chat_session", {sessionId: "sessionId-v"}],
  ["check_onboarding", undefined],
  ["complete_onboarding", undefined],
  ["arm_create_strategy", {name: "name-v", strategyType: "strategyType-v", config: "config-v", capitalLimit: "capitalLimit-v", dailyLossCap: "dailyLossCap-v"}],
  ["arm_update_strategy", {id: "id-v", name: "name-v", config: "config-v", capitalLimit: "capitalLimit-v", dailyLossCap: "dailyLossCap-v"}],
  ["arm_delete_strategy", {id: "id-v"}],
  ["arm_activate_strategy", {id: "id-v"}],
  ["arm_pause_strategy", {id: "id-v", reasoning: "reasoning-v"}],
  ["arm_kill_strategy", {id: "id-v", reasoning: "reasoning-v"}],
  ["arm_poll_revenue", {strategyId: "strategyId-v"}],
  ["arm_log_decision", {strategyId: "strategyId-v", action: "action-v", reasoning: "reasoning-v", context: "context-v"}],
  ["arm_list_decisions", {strategyId: "strategyId-v", limit: "limit-v"}],
  ["arm_update_decision_outcome", {id: "id-v", outcome: "outcome-v", notes: "notes-v"}],
  ["arm_set_global_limits", {capitalLimit: "capitalLimit-v", dailyLossCap: "dailyLossCap-v"}],
  ["arm_kill_switch", {active: "active-v", reasoning: "reasoning-v"}],
  ["arm_health_check", {strategyId: "strategyId-v"}],
  ["arm_poll_all", undefined],
  ["arm_get_config", undefined],
  ["arm_save_config", {config: "config-v"}],
  ["set_proof_mode", {enabled: "enabled-v"}],
  ["get_proof_log", undefined],
  ["simulate_gateway_down", undefined],
  ["simulate_gateway_up", undefined],
  ["list_replays", undefined],
  ["start_replay", {filename: "filename-v", speed: "speed-v"}],
  ["stop_replay", undefined],
  ["start_demo_eeg", {mode: "mode-v", speed: "speed-v"}],
  ["stop_demo_eeg", undefined],
  ["get_cost_estimate", {text: "text-v", config: "config-v", tags: "tags-v", deep: "deep-v"}],
  ["get_llm_usage", undefined],
  ["clear_llm_usage", undefined],
  ["read_identity_file", undefined],
  ["load_terrance_context", undefined],
  ["save_terrance_context", {context: "context-v"}],
  ["store_session_log", {entry: "entry-v"}],
  ["list_session_logs", undefined],
  ["archive_old_session_logs", {daysThreshold: "daysThreshold-v"}],
  ["prune_context_memory", {maxItems: "maxItems-v"}],
  ["get_session_insights", undefined],
  ["load_custom_jobs", undefined],
  ["save_custom_jobs", {data: "data-v"}],
  ["save_skill", {name: "name-v", content: "content-v"}],
  ["delete_skill", {name: "name-v"}],
  ["check_docker_service", {name: "name-v"}],
  ["restart_docker_service", {name: "name-v"}],
  ["check_service_port", {host: "host-v", port: "port-v"}],
  ["web_search", {query: "query-v", maxResults: "maxResults-v"}],
  ["fetch_url", {url: "url-v", maxChars: "maxChars-v"}],
  ["store_activity_entry", {entry: "entry-v"}],
  ["prune_activity_log", {days: "days-v"}],
  ["archive_activity_log", {keepDays: "keepDays-v", maxFiles: "maxFiles-v", dryRun: "dryRun-v"}],
  ["load_subagents", undefined],
  ["save_subagents", {data: "data-v"}],
  ["read_profile_file", undefined],
  ["roadmap_catchup_status", undefined],
  ["roadmap_draft_pending", {catchUp: "catchUp-v"}],
  ["goal_route_get", {goalId: "goalId-v"}],
  ["goal_step_edit", {goalId: "goalId-v", stepId: "stepId-v", title: "title-v", size: "size-v", via: "via-v"}],
  ["goal_step_complete", {goalId: "goalId-v", stepId: "stepId-v", via: "via-v"}],
  ["goal_branch_open", {goalId: "goalId-v"}],
  ["goal_branch_accept", {suggestionId: "suggestionId-v", via: "via-v"}],
  ["goal_branch_decline", {suggestionId: "suggestionId-v", via: "via-v"}],
  ["goal_route_settings_get", undefined],
  ["goal_route_settings_set", {stagnationDays: "stagnationDays-v"}],
  ["watch_proposals_list", {includeSample: "includeSample-v"}],
  ["watch_proposal_record", {id: "id-v", via: "via-v"}],
  ["watch_proposal_dismiss", {id: "id-v", via: "via-v"}],
  ["market_feed_status", undefined],
  ["watchlist_get", undefined],
  ["watchlist_research_now", undefined],
  ["watchlist_drop", {ticker: "ticker-v", via: "via-v"}],
  ["restart_services", {services: "services-v", deadlineMs: "deadlineMs-v"}],
];

describe("invokeBound: each caller command sends its name and payload, and rejections pass through", () => {
  it.each(CALLER_COMMANDS)("%s", async (command, args) => {
    invoke.mockResolvedValue(null);
    await invokeBound(command, args);
    expect(sentWire()).toEqual([[command, wire(args)]]);
    invoke.mockRejectedValue("boom");
    await expect(invokeBound(command, args)).rejects.toBe("boom");
  });

  it("an absent optional stays absent on the wire (not null)", async () => {
    invoke.mockResolvedValue(null);
    await invokeBound("home_window_focus", { id: "w1" });
    await invokeBound("hemisync_import");
    await invokeBound("chat_with_router", { text: "t", config: {}, tags: [], forceProvider: "light", localOnly: true });
    expect(sentWire()).toEqual([
      ["home_window_focus", { id: "w1" }],
      ["hemisync_import", {}],
      ["chat_with_router", { text: "t", config: {}, tags: [], forceProvider: "light", localOnly: true }],
    ]);
  });

  it("an explicit null is kept", async () => {
    invoke.mockResolvedValue(null);
    await invokeBound("list_terrence_automations", { limit: null });
    expect(sentWire()).toEqual([["list_terrence_automations", { limit: null }]]);
  });

  it("ha_call_service without data omits the key even in process", async () => {
    invoke.mockResolvedValue(null);
    await invokeBound("ha_call_service", { domain: "light", service: "turn_on", entityId: "light.x" });
    expect(invoke.mock.calls[0][1]).not.toHaveProperty("data");
  });

  it("an unknown command rejects without calling invoke", async () => {
    await expect(invokeBound("not_a_command")).rejects.toMatch(/not found/);
    await expect(invokeBound("toString")).rejects.toMatch(/not found/);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("the table has one row per command in bindings.gen.ts except amara_turn", async () => {
    const src = readFileSync("src-console/bindings.gen.ts", "utf8");
    const gen = [...src.matchAll(/__TAURI_INVOKE<[^>]*(?:<[^>]*>)?[^>]*>\("([a-z_0-9]+)"/g)].map((m) => m[1]).filter((c) => c !== "amara_turn").sort();
    expect(Object.keys(BOUND_TABLE).sort()).toEqual(gen);
  });
});

describe("amaraTurn (Channel) goes through the bindings", () => {
  it("sends input, surface and a channel whose messages reach the callback", async () => {
    invoke.mockResolvedValue("turn-1");
    const events: unknown[] = [];
    await expect(bridge.amaraTurn({ conversationId: "c", text: "hi", deep: true }, "typed", (e) => events.push(e))).resolves.toBe("turn-1");
    expect(invoke.mock.calls).toHaveLength(1);
    const [command, payload] = invoke.mock.calls[0];
    expect(command).toBe("amara_turn");
    expect(Object.keys(payload).sort()).toEqual(["channel", "input", "surface"]);
    expect(payload.input).toEqual({ conversationId: "c", text: "hi", deep: true });
    expect(payload.surface).toBe("typed");
    payload.channel.onmessage({ type: "delta", text: "a" });
    expect(events).toEqual([{ type: "delta", text: "a" }]);
  });
  it("a rejection reaches the caller", async () => {
    invoke.mockRejectedValue("busy");
    await expect(bridge.amaraTurn({ conversationId: "c", text: "x" }, "voice", () => {})).rejects.toBe("busy");
  });
});
