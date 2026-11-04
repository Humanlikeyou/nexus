/**
 * Canned Tauri command responses for the e2e suite. Sample data only: no real
 * names, paths, tokens or health records. Shapes follow src-console/types.ts.
 */
export const NOW = "2026-10-28T14:00:00+09:00";

const day = (d: string, o: Record<string, unknown> = {}) => ({
  day: d, source: "noop", totalSleepMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230,
  restingHr: 52, avgHrv: 64, recovery: 74, strain: 26, spo2Pct: 97, skinTempDevC: 0.3, respRateBpm: 14.8, ...o,
});
const RECOVERY = [62, 70, 58, 66, 81, 77, 54, 49, 68, 72, 80, 61, 66, 74];
const days = RECOVERY.map((recovery, i) =>
  day(new Date(Date.parse("2026-10-15T00:00:00Z") + i * 864e5).toISOString().slice(0, 10), {
    recovery, avgHrv: 55 + ((i * 7) % 14), strain: 12 + ((i * 11) % 40), restingHr: 50 + (i % 4),
  }),
);

const NOOP_READY = {
  source: "noop", status: "ready", generatedAt: "2026-10-28T07:30:00+09:00", deviceId: "sample-band", latestDay: "2026-10-28", dataAgeDays: 0,
  today: days[13], recentDays: days,
  latestNight: { day: "2026-10-28", startedAt: "2026-10-27T23:41:00+09:00", endedAt: "2026-10-28T07:13:00+09:00", durationMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230, awakeMin: 26, sessionCount: 2, restingHr: 51, avgHrv: 66 },
  latestSleep: null,
  battery: { percentage: 82, charging: false, observedAt: "2026-10-28T07:00:00+09:00" },
  insights: [
    { metric: "hrv", fact: "HRV is up 8%", context: "vs your 14-day baseline" },
    { metric: "sleep", fact: "Slept 7h 32m", context: "22 min more than average" },
  ],
  message: null,
};

const prio = (rank: number, title: string, sourceStatus: string) => ({
  rank, sourceKind: "project", sourceId: `p${rank}`, title, sourceStatus, score: 0.9 - rank / 10, confidence: 0.8, reasons: [], contextItemIds: [], actionId: null,
});
const interaction = (id: string, user: string, assistant: string, at: string) => ({
  id, conversationId: "e2e-conv", hermesSessionId: "e2e-session", runtime: "hermes", model: "sample-model", provider: "ollama", status: "completed",
  userMessage: user, assistantMessage: assistant, error: null, startedAt: at, completedAt: at, durationMs: 2100, contextItemIds: [], actionId: null, runtimeProfile: "nexus",
});
const crewRun = (id: string, name: string, status: string, task: string, result: string | null) => ({
  id, name, status, agents: ["writer"], started_at: "2026-10-28T13:30:00+09:00", task, result,
});

/* noop_query payloads in the real shapes (noop-shapes-1104), fake values. */
const noopRow = (d: string, i: number) => ({
  day: d, source: "my-whoop", totalSleepMin: 420 + ((i * 13) % 60), efficiency: 90, deepMin: 90, remMin: 100, lightMin: 230, disturbances: 3,
  restingHr: 50 + (i % 4), avgHrv: 56 + ((i * 7) % 14), recovery: 55 + ((i * 11) % 35), strain: 8 + ((i * 5) % 9), exerciseCount: 1,
  spo2Pct: 97, skinTempDevC: 0.2, respRateBpm: 14.6, steps: 8000, activeKcalEst: 450,
});
const noopDays = Array.from({ length: 29 }, (_, i) => noopRow(new Date(Date.parse("2026-09-30T00:00:00Z") + i * 864e5).toISOString().slice(0, 10), i));
export const NOOP_SNAPSHOT = {
  generatedAt: "2026-10-28T07:30:00+09:00", logicalToday: "2026-10-28",
  sources: { whoopImported: "my-whoop", noopComputed: "my-whoop-noop", appleHealth: null, nutrition: null, mood: null, journal: null },
  freshness: { latestHeartRateSample: null, latestDailyMetricDay: "2026-10-28", latestAppleHealthDay: null, dailyRows: 29, appleDailyRows: 0 },
  today: noopDays[28], recentDays: noopDays.slice(0, 28), appleDaily: [], latestSleep: null,
  latestNight: { day: "2026-10-28", startTs: 1, endTs: 2, start: "2026-10-27T23:41:00+09:00", end: "2026-10-28T07:13:00+09:00", sessionCount: 1, totalSleepMin: 452, inBedMin: 478, efficiency: 91, deepMin: 92, remMin: 104, lightMin: 230 },
  battery: null,
};
const NOOP_SERIES = {
  key: "hrv", requestedSource: "my-whoop", range: { from: "2026-09-30", to: "2026-10-28" },
  resolution: { candidates: [{ source: "my-whoop", key: "hrv" }], usedSources: ["my-whoop"] }, returned: 29,
  points: noopDays.map((d) => ({ day: d.day, key: "hrv", value: d.avgHrv, source: "my-whoop", sourceKey: "hrv" })),
};

export type Responses = Record<string, unknown>;

/** Every command the console reads on load, with sample values. */
export function responses(): Responses {
  return {
    get_terrence_runtime_health: { runtime: "hermes", status: "online", model: "sample-model", provider: "ollama", gatewayRunning: true, telegramStatus: "connected", activeSchedules: 3, configuredToolsets: ["web", "files"], checkedAt: NOW, detail: null },
    list_terrence_capabilities: [{ name: "web", status: "ready", description: "Web search" }, { name: "files", status: "ready", description: "Workspace files" }],
    get_terrence_context_snapshot: { generatedAt: NOW, priorities: [], activeProjects: [], commitments: [], recentObservations: [] },
    get_current_terrence_priority_snapshot: { id: "ps1", generatedAt: NOW, items: [prio(1, "Ship the sample release", "active"), prio(2, "Review the sleep pipeline", "active"), prio(3, "Plan next quarter", "blocked")] },
    list_terrence_decisions: [], list_terrence_actions: [], list_terrence_automations: [], list_terrence_automation_runs: [],
    list_terrence_projects: [{ id: "p1", title: "Sample project", status: "active", nextAction: "Write the next step" }],
    get_noop_health_snapshot: NOOP_READY,
    get_muse_state: { connected: false, device_name: null, streaming: false },
    list_sessions: [{ id: "s1", device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:20:00+09:00", duration_secs: 1200, data_points: 76800, summary: { avg_focus: 0.62, avg_relaxation: 0.71 }, body_context: { today: { recovery: 74, hrv: 64, rhr: 52, strain: 11, sleep_total_min: 452 }, latest_night: { day: "2026-10-27", total_sleep_min: 452, efficiency: 91, deep_min: 92, rem_min: 104, light_min: 230 } } }],
    read_tasks: [{ title: "Write release notes", status: "active", priority: "high", tags: ["docs"] }, { title: "Plan next quarter", status: "waiting", priority: null, tags: ["planning"] }, { title: "Restock samples", status: "backlog", priority: "medium", tags: [] }],
    read_content_pipeline: [{ title: "Launch note", stage: "drafting", content_type: "blog_post", notes: null }, { title: "Routine reel", stage: "ideation", content_type: "short_video", notes: null }],
    arm_get_revenue_summary: { total_revenue: 1820, total_expenses: 240, net_income: 1580, today_revenue: 120, today_expenses: 0, week_revenue: 640, month_revenue: 1820, by_strategy: [{ strategy_id: "s1", strategy_name: "Sample shop", revenue: 1500, expenses: 200, net: 1300 }, { strategy_id: "s2", strategy_name: "Sample API", revenue: 320, expenses: 40, net: 280 }] },
    arm_list_strategies: [{ id: "s1", name: "Sample shop", type: "e_commerce", status: "active", capital_limit: 5000, daily_loss_cap: 200, total_revenue: 1500, total_expenses: 200, last_health: null, health_status: "healthy" }],
    arm_get_risk_status: { global_capital_deployed: 2400, global_capital_limit: 6000, global_daily_pnl: -35, global_daily_loss_cap: 250, kill_switch_active: false, active_strategies: 1, strategies_at_risk: [], risk_index: 0.18 },
    read_goals: [{ id: "g1", title: "Run 300 km", category: "fitness", target: 300, current: 212, unit: "km", deadline: "2026-12-31" }, { id: "g2", title: "Daily meditation", category: "mind", target: 0, current: 14, unit: "days", deadline: null }],
    get_outbox_status: { gateway_online: true, queue_depth: 0, last_delivered_at: "2026-10-28T13:40:00+09:00", last_error: null, total_delivered: 41 },
    list_activity_entries: [
      { id: "a1", type: "mac_action", name: "open_app", status: "completed", startedAt: Date.parse("2026-10-28T13:38:00+09:00"), completedAt: Date.parse("2026-10-28T13:38:01+09:00"), durationMs: 1200, detail: "Opened Notes", outcome: null },
      { id: "a2", type: "agent_task", name: "daily_brief", status: "failed", startedAt: Date.parse("2026-10-28T07:30:00+09:00"), completedAt: Date.parse("2026-10-28T07:30:42+09:00"), durationMs: 42000, detail: "Delivery failed", outcome: null },
    ],
    read_openclaw_status: { status: "online", online: true, model: "sample-model", gateway_url: "http://127.0.0.1:18789", last_heartbeat: NOW },
    crew_health: "operational",
    crew_tools: [{ name: "kb_query", allowed: true }, { name: "memory_query", allowed: true }],
    get_allow_claude: false,
    set_allow_claude: true,
    kb_status: { state: "empty", model: "nomic-embed-text", docs: 0, detail: "knowledge base is empty" },
    crew_list: [crewRun("c1", "smoke", "completed", "Reply with the word OK", "OK"), crewRun("c2", "sample research", "failed", "List three sample facts", "LLM unavailable")],
    list_terrence_interactions: [
      interaction("i1", "What's on my plate this afternoon?", "Two things: finish the release notes, then review the sleep pipeline.", "2026-10-28T13:12:00+09:00"),
    ],
    list_context_items: [],
    // Tool-keyed: the mock returns __byTool[args.tool].
    noop_query: { __byTool: { health_snapshot: NOOP_SNAPSHOT, metric_series: NOOP_SERIES } },
    get_noop_settings: { whoop_source: "noop", binary_path: null, db_path: null },
    set_noop_settings: null,
    migrate_legacy_terrence_context: null,
    load_kv: null,
    save_kv: null,
    voice_status: { installed: true, running: true, pid: 4242, state: "listening", state_age_s: 2, voice: "neosoul_g", rss_mb: 3123.4, launchd: false, dir: "/Users/taisun/projects/amara-voice" },
    voice_list_voices: { voices: ["amara_ref", "neosoul", "neosoul_g"], current: "neosoul_g" },
    voice_recent_turns: [
      { time: "2026-10-28T13:05:00+09:00", heard: "Amara, what time is it?", text: "what time is it?", route: "local", reply: "It's one o five p.m.", asr_s: 0.4, total_s: 1.1, barged: null, verdict: "ok" },
      { time: "2026-10-28T13:06:00+09:00", heard: "The Aikido, the akiya in Utsunomiya.", text: "", route: "ignored", ignored: "hint-echo", reply: "", asr_s: 0.9, barged: null, verdict: "noise" },
      { time: "2026-10-28T13:07:00+09:00", heard: "Amara, how's the akiya search going?", text: "how's the akiya search going?", route: "hermes", reply: "Two new listings near Utsunomiya station came in this morning.", asr_s: 0.5, token_s: 2.1, first_audio_s: 5.4, total_s: 9.8, barged: null, verdict: "ok" },
    ],
  };
}
