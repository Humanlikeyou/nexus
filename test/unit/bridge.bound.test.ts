// Bridge functions backed by generated bindings (src-console/bindings.gen.ts): same command name,
// same payload keys, same resolve/reject behaviour as the old hand-written invoke. Not covered here:
// amara_turn (Channel, stays on invoke until step 3).
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), Channel: class {} }));

import * as bridge from "../../src-console/bridge";
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
