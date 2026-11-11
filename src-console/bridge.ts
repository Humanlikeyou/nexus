import { invoke } from "@tauri-apps/api/core";
import { EMPTY_DEVICE_ID } from "./lib/devices";
import type {
  CrewInfo,
  CrewRunSteps,
  CrewServiceHealth,
  CrewSnapshot,
  CrewToolInfo,
  FollowupItem,
  FollowupItemInput,
  FollowupItemPatch,
  FollowupLists,
  KbStatus,
  ActivityEntry,
  CorrectTerrenceContextItemInput,
  Goal,
  HermesRuntimeHealth,
  MuseDevice,
  MuseState,
  MutedTopic,
  NexusUnavailableSource,
  NoopHealthSnapshot,
  OpenClawStatus,
  OutboxStatus,
  PipelineItem,
  RecordedSession,
  RevenueSummary,
  RiskStatus,
  Strategy,
  SystemSnapshot,
  TaskItem,
  TerrenceCapability,
  TerrenceContextSnapshot,
  TerrenceContextItem,
  TerrenceContextItemQuery,
  TerrenceDecision,
  TerrenceInteraction,
  AmaraContextSection,
  AmaraTierInfo,
  AmaraTurnContext,
  ChatTurn,
  TerrenceAction,
  TerrenceAutomation,
  TerrenceAutomationRun,
  TerrencePrioritySnapshot,
  TerrenceProject,
  VoiceList,
  VoiceStatus,
  VoiceTurn,
} from "./types";
import type { AuxInfo, BandName, Contact, EegChannel, RecordingListItem } from "./neuro/recordings";
import type { SensorKey } from "./neuro/sessionSamples";

interface SnapshotRead<T> {
  unavailable: NexusUnavailableSource | null;
  value: T;
}

const SOURCE_LABELS: Record<string, string> = {
  get_terrence_runtime_health: "Amara runtime",
  list_terrence_capabilities: "Amara capabilities",
  get_terrence_context_snapshot: "Amara context",
  get_current_terrence_priority_snapshot: "Amara priorities",
  list_terrence_decisions: "Decision records",
  list_terrence_actions: "Action ledger",
  list_terrence_automations: "Automations",
  list_terrence_automation_runs: "Automation history",
  list_terrence_projects: "Project state",
  get_noop_health_snapshot: "NOOP health",
  get_muse_state: "Muse",
  list_sessions: "Sensor sessions",
  read_tasks: "Task source",
  read_content_pipeline: "Content pipeline",
  arm_get_revenue_summary: "Revenue summary",
  arm_list_strategies: "Revenue strategies",
  arm_get_risk_status: "Revenue risk",
  read_goals: "Legacy goals",
  get_outbox_status: "Delivery outbox",
  list_activity_entries: "Activity log",
  read_openclaw_status: "Assistant backend",
  crew_health: "Crew service",
  crew_list: "Crew runs",
  crew_tools: "Crew tools",
  kb_status: "Knowledge base",
};

function unavailableSource(command: string): NexusUnavailableSource {
  return { command, label: SOURCE_LABELS[command] ?? command };
}

async function read<T>(command: string, fallback: T, args?: Record<string, unknown>): Promise<SnapshotRead<T>> {
  try {
    return { unavailable: null, value: await invoke<T>(command, args) };
  } catch (error) {
    console.warn(`[console] ${command} unavailable`, error);
    return { unavailable: unavailableSource(command), value: fallback };
  }
}

const EMPTY_NOOP: NoopHealthSnapshot = {
  source: "noop",
  status: "unavailable",
  generatedAt: "",
  deviceId: EMPTY_DEVICE_ID,
  latestDay: null,
  dataAgeDays: null,
  today: null,
  recentDays: [],
  latestSleep: null,
  latestNight: null,
  battery: null,
  insights: [],
  message: "NOOP health data is not available yet.",
};

const EMPTY_KB: KbStatus = { state: "not_running", model: null, docs: null, detail: null };
const EMPTY_OPENCLAW: OpenClawStatus = { status: "offline", online: false, model: "unknown", gateway_url: "", last_heartbeat: null };

/** Keep only the typed fields (defence in depth; the Rust command already returns just these). */
function pickOpenClaw(s: OpenClawStatus): OpenClawStatus {
  const status = s?.status === "online" || s?.status === "configured" || s?.status === "not_installed" || s?.status === "offline" ? s.status : s?.online ? "online" : "offline";
  return { status, online: status === "online", model: s?.model ?? "unknown", gateway_url: s?.gateway_url ?? "", last_heartbeat: s?.last_heartbeat ?? null };
}

const EMPTY_REVENUE: RevenueSummary = {
  total_revenue: 0,
  total_expenses: 0,
  net_income: 0,
  today_revenue: 0,
  today_expenses: 0,
  week_revenue: 0,
  month_revenue: 0,
  by_strategy: [],
};

const EMPTY_RISK: RiskStatus = {
  global_capital_deployed: 0,
  global_capital_limit: 0,
  global_daily_pnl: 0,
  global_daily_loss_cap: 0,
  kill_switch_active: false,
  active_strategies: 0,
  strategies_at_risk: [],
  risk_index: 0,
};

const EMPTY_CONTEXT: TerrenceContextSnapshot = {
  generatedAt: "",
  priorities: [],
  activeProjects: [],
  commitments: [],
  recentObservations: [],
};

const EMPTY_PRIORITY_SNAPSHOT: TerrencePrioritySnapshot = {
  id: "",
  generatedAt: "",
  items: [],
};

export const INITIAL_SNAPSHOT: SystemSnapshot = {
  refreshedAt: Date.now(),
  unavailableSources: [],
  hermes: {
    runtime: "hermes",
    status: "offline",
    model: "unknown",
    provider: "unknown",
    gatewayRunning: false,
    telegramStatus: "unknown",
    activeSchedules: 0,
    configuredToolsets: [],
    checkedAt: "",
    detail: null,
  },
  capabilities: [],
  context: EMPTY_CONTEXT,
  prioritySnapshot: EMPTY_PRIORITY_SNAPSHOT,
  decisions: [],
  actions: [],
  automations: [],
  automationRuns: [],
  projects: [],
  noop: EMPTY_NOOP,
  muse: { connected: false, device_name: null, streaming: false },
  sessions: [],
  tasks: [],
  pipeline: [],
  revenue: EMPTY_REVENUE,
  strategies: [],
  risk: EMPTY_RISK,
  goals: [],
  outbox: { gateway_online: false, queue_depth: 0, last_delivered_at: null, last_error: null, total_delivered: 0 },
  activity: [],
  openclaw: EMPTY_OPENCLAW,
  crew: { health: "offline", runs: [], tools: [] },
  kb: EMPTY_KB,
  ledger: null,
};

export async function loadSystemSnapshot(): Promise<SystemSnapshot> {
  const [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, openclaw, crewHealth, crewRuns, crewTools, kb, ledger] =
    await Promise.all([
      read<HermesRuntimeHealth>("get_terrence_runtime_health", INITIAL_SNAPSHOT.hermes),
      read<TerrenceCapability[]>("list_terrence_capabilities", []),
      read<TerrenceContextSnapshot>("get_terrence_context_snapshot", EMPTY_CONTEXT),
      read<TerrencePrioritySnapshot>("get_current_terrence_priority_snapshot", EMPTY_PRIORITY_SNAPSHOT),
      read<TerrenceDecision[]>("list_terrence_decisions", []),
      read<TerrenceAction[]>("list_terrence_actions", []),
      read<TerrenceAutomation[]>("list_terrence_automations", []),
      read<TerrenceAutomationRun[]>("list_terrence_automation_runs", []),
      read<TerrenceProject[]>("list_terrence_projects", []),
      read<NoopHealthSnapshot>("get_noop_health_snapshot", EMPTY_NOOP),
      read<MuseState>("get_muse_state", INITIAL_SNAPSHOT.muse),
      read<RecordedSession[]>("list_sessions", []),
      read<TaskItem[]>("read_tasks", [], { workspacePath: null }),
      read<PipelineItem[]>("read_content_pipeline", [], { workspacePath: null }),
      read<RevenueSummary>("arm_get_revenue_summary", EMPTY_REVENUE, { from: null, to: null }),
      read<Strategy[]>("arm_list_strategies", []),
      read<RiskStatus>("arm_get_risk_status", EMPTY_RISK),
      read<Goal[]>("read_goals", []),
      read<OutboxStatus>("get_outbox_status", INITIAL_SNAPSHOT.outbox),
      read<ActivityEntry[]>("list_activity_entries", [], { limit: 30 }),
      read<OpenClawStatus>("read_openclaw_status", EMPTY_OPENCLAW),
      read<string>("crew_health", "offline"),
      read<CrewInfo[]>("crew_list", []),
      read<CrewToolInfo[]>("crew_tools", []),
      read<KbStatus>("kb_status", EMPTY_KB),
      read<SystemSnapshot["ledger"]>("read_business_ledger", null),
    ]);

  const crew = crewSnapshot(crewHealth.value, crewRuns.value, crewTools.value);
  // crew_list can only succeed when the service is configured and up; its
  // failure is expected (not a broken source) otherwise.
  if (crew.health !== "online") crewRuns.unavailable = null;
  const reads = [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, openclaw, crewHealth, crewRuns, crewTools, kb, ledger];

  return {
    refreshedAt: Date.now(),
    unavailableSources: reads.flatMap((read) => read.unavailable ? [read.unavailable] : []),
    hermes: hermes.value,
    capabilities: capabilities.value,
    context: context.value,
    prioritySnapshot: prioritySnapshot.value,
    decisions: decisions.value,
    actions: actions.value,
    automations: automations.value,
    automationRuns: automationRuns.value,
    projects: projects.value,
    noop: noop.value,
    muse: muse.value,
    sessions: sessions.value,
    tasks: tasks.value,
    pipeline: pipeline.value,
    revenue: revenue.value,
    strategies: strategies.value,
    risk: risk.value,
    goals: goals.value,
    outbox: outbox.value,
    activity: activity.value,
    openclaw: pickOpenClaw(openclaw.value),
    crew,
    kb: pickKb(kb.value),
    ledger: ledger.value,
  };
}

/** crew_health → three UI states; "degraded" (reachable but unhealthy) counts as offline. */
export function crewSnapshot(health: string, runs: CrewInfo[], tools: CrewToolInfo[] = []): CrewSnapshot {
  const state: CrewServiceHealth = health === "not_configured" ? "not_configured" : health === "operational" ? "online" : "offline";
  const safeTools = Array.isArray(tools)
    ? tools.filter((t) => t && /^[a-z0-9_]{1,40}$/.test(String(t.name))).map((t) => ({ name: t.name, allowed: t.allowed === true }))
    : [];
  return { health: state, runs: state === "online" ? runs : [], tools: safeTools };
}

/** Only the four known states and plain numbers/model ids reach the UI. */
export function pickKb(k: Partial<KbStatus> | null | undefined): KbStatus {
  const state = k?.state === "running" || k?.state === "empty" || k?.state === "not_running" || k?.state === "error" ? k.state : "error";
  const model = typeof k?.model === "string" && /^[A-Za-z0-9.:_-]{1,64}$/.test(k.model) ? k.model : null;
  const docs = typeof k?.docs === "number" && Number.isFinite(k.docs) && k.docs >= 0 ? Math.floor(k.docs) : null;
  return { state, model, docs, detail: null };
}

export const startCrew = (name: string, task: string) => invoke<CrewInfo>("crew_start", { name, task, agents: null });
export const getCrewStatus = (id: string) => invoke<CrewInfo>("crew_status", { id });
export const getCrewRunSteps = (id: string) => invoke<CrewRunSteps>("crew_run_steps", { id });
export const stopCrew = (id: string) => invoke<string>("crew_stop", { id });

export const interactWithTerrence = (request: {
  message: string;
  conversationId: string;
  hermesSessionId: string | null;
  /** Full visible history (guarded, compressed when long). */
  history?: ChatTurn[];
  /** Composer Deep toggle → /deep through the router. */
  deep?: boolean;
  /** Guarded sections from prepareAmaraTurn. */
  context?: AmaraContextSection[];
  contextItemIds?: string[];
}) => invoke<TerrenceInteraction>("interact_with_terrence", { request });

/** Background context for a turn, to run through the injection guard before it is sent. */
export const prepareAmaraTurn = (message: string) => invoke<AmaraTurnContext>("prepare_amara_turn", { message });

/** Persist an exchange made outside Hermes (/deliberate via the router). */
export const recordAmaraInteraction = (input: {
  conversationId: string;
  userMessage: string;
  assistantMessage: string;
  tier: AmaraTierInfo | null;
  durationMs: number;
  /** ^[a-z0-9_-]{1,32}$, e.g. "home"; Rust rejects others with invalid_source. */
  source?: string | null;
}) => invoke<TerrenceInteraction>("record_amara_interaction", { input });

export const listTerrenceInteractions = (conversationId: string, limit = 80) =>
  invoke<TerrenceInteraction[]>("list_terrence_interactions", { conversationId, limit });

export const migrateLegacyTerrenceContext = () => invoke<unknown>("migrate_legacy_terrence_context");
export const listTerrenceContextItems = (query: TerrenceContextItemQuery = { limit: 120 }) =>
  invoke<TerrenceContextItem[]>("list_context_items", { query });
export const correctTerrenceContextItem = (input: CorrectTerrenceContextItemInput) =>
  invoke<TerrenceContextItem>("correct_context_item", { input });
export const deleteTerrenceContextItem = (id: string) =>
  invoke<void>("delete_context_item", { id });
export const approveTerrenceAction = (id: string) => invoke<TerrenceAction>("approve_terrence_action", { id });
export const retryTerrenceAction = (id: string) => invoke<TerrenceAction>("retry_terrence_action", { id });
export const cancelTerrenceAction = (id: string) => invoke<TerrenceAction>("cancel_terrence_action", { id });

export const getReduceTransparency = () => invoke<boolean>("get_reduce_transparency");
export const scanMuse = () => invoke<MuseDevice[]>("scan_muse");
export const connectMuse = (address: string) => invoke<MuseState>("connect_muse", { address });
export const disconnectMuse = () => invoke<void>("disconnect_muse");
export const startEegStream = () => invoke<void>("start_eeg_stream");
export const stopEegStream = () => invoke<void>("stop_eeg_stream");
export const startEegRecording = () => invoke<string>("start_eeg_recording");
export const stopEegRecording = () => invoke<string>("stop_eeg_recording");
/** Muse recordings (commands/recordings.rs, replay.rs). Read-only: there is no delete command. */
export interface RecordingListItemIpc extends Omit<RecordingListItem, "id" | "replayFile"> {
  id: string;
  sessionId: string | null;
  replayFile: string | null;
}
/** Gaps in a recording come back as `null`. */
export type Gapped = Array<number | null>;
export interface RecordingWindowIpc {
  startSecs: number; endSecs: number; sampleRate: 256;
  lanes: Array<{ channel: EegChannel; contact: Contact; values: Gapped }>;
}
export interface BodyWindowIpc {
  startSecs: number; endSecs: number;
  heart: { ppgRate: 64; values: Gapped; bpm: number | null; estimate: true } | null;
  motion: { rate: 52; values: Gapped; events: Array<{ atSecs: number; label: string }> } | null;
}
export interface SessionSeriesIpc { stepSecs: number; bands: Record<BandName, Gapped>; focus: Gapped; calm: Gapped }
export interface SessionSamplesIpc extends SessionSeriesIpc {
  sensors?: Partial<Record<SensorKey, Partial<Record<BandName, Gapped>>>>;
  contact?: Partial<Record<SensorKey, number>>;
}
export const listRecordings = () => invoke<RecordingListItemIpc[]>("list_recordings");
export const readRecordingWindow = (id: string, startSecs: number, spanSecs: number, points?: number) =>
  invoke<RecordingWindowIpc>("read_recording_window", { id, startSecs, spanSecs, points });
export const readBodyWindow = (id: string, startSecs: number, spanSecs: number) =>
  invoke<BodyWindowIpc>("read_body_window", { id, startSecs, spanSecs });
export const recordingSeries = (id: string) => invoke<SessionSeriesIpc>("recording_series", { id });
export const getSessionSamples = (sessionId: string) => invoke<SessionSamplesIpc | null>("get_session_samples", { sessionId });
/** Tells the open recording where the back-of-head electrode was found; written to its trailer. */
export const markRecordingAux = (aux: AuxInfo) => invoke<void>("mark_recording_aux", { aux });
export const saveMuseSession = (session: RecordedSession & { file_path: string }, data: unknown) =>
  invoke<string>("save_session", { session, data });

/** Settings › "Allow Claude": only this flag crosses the bridge (no keys). */
export const getAllowClaude = () => invoke<boolean>("get_allow_claude");
export const setAllowClaude = (enabled: boolean) => invoke<boolean>("set_allow_claude", { enabled });

/** Amara Voice (commands/voice.rs). */
export const getVoiceStatus = () => invoke<VoiceStatus>("voice_status");
export const startVoice = () => invoke<VoiceStatus>("voice_start");
export const stopVoice = () => invoke<VoiceStatus>("voice_stop");
export const getVoiceTurns = (n = 20) => invoke<VoiceTurn[]>("voice_recent_turns", { n });
export const listVoices = () => invoke<VoiceList>("voice_list_voices");
export const setVoice = (name: string) => invoke<VoiceList>("voice_set_voice", { name });
/** Interrupt the current spoken reply only (SIGUSR1). false = nothing sent (not running / loop not patched). */
export const stopVoiceSpeaking = () => invoke<boolean>("voice_stop_speaking");

/** Amara's follow-up lists (commands/followups.rs). Errors are the bare kinds in lib/followups.ts. */
export const listFollowups = () => invoke<FollowupLists>("followups_list");
export const addFollowupItem = (input: FollowupItemInput) => invoke<FollowupItem>("followups_add_item", { ...input });
export const updateFollowupItem = (id: number, patch: FollowupItemPatch) => invoke<FollowupItem>("followups_update_item", { id, patch });
export const markFollowupDone = (id: number) => invoke<FollowupItem>("followups_mark_done", { id });
export const reopenFollowupItem = (id: number) => invoke<FollowupItem>("followups_reopen_item", { id });
export const deleteFollowupItem = (id: number) => invoke<void>("followups_delete_item", { id });
export const addMutedTopic = (topic: string, until?: string) => invoke<MutedTopic>("followups_add_muted_topic", until ? { topic, until } : { topic });
export const deleteMutedTopic = (topic: string) => invoke<void>("followups_delete_muted_topic", { topic });

/** Small app state on this Mac (commands/kv.rs). */
export const loadKv = (key: string) => invoke<string | null>("load_kv", { key });
export const saveKv = (key: string, value: string) => invoke<void>("save_kv", { key, value });

/** Home's brief (commands/home.rs). Null fields mean "not available": the page simply shows less. */
export interface HomeWeather { place: string | null; tempC: number | null; summary: string | null; highC: number | null; lowC: number | null }
export interface HomeBrief { greeting: string | null; weather: HomeWeather | null; updatedAt: string | null }
export const getHomeBrief = () => invoke<HomeBrief>("home_brief");
export interface HeartSample { bpm: number; at: number }
export interface LiveHeartRate { bpm: number; at: number; trace: HeartSample[] }
export const getLiveHeartRate = () => invoke<LiveHeartRate | null>("live_heart_rate");
/** NOOP workouts (noop.rs): read-only, notes never cross. zoneMinutes = minutes in zones 1..5, or null. */
export interface NoopWorkout {
  day: string; startedAt: string; endedAt: string; sport: string;
  durationMin: number | null; energyKcal: number | null; avgHr: number | null; maxHr: number | null;
  strain: number | null; distanceM: number | null; steps: number | null; zoneMinutes: number[] | null;
}
export const getRecentWorkouts = (days = 14) => invoke<NoopWorkout[]>("noop_recent_workouts", { days });
/** One night's sleep stages (unix seconds, clipped to the night); null when the night has no segments. */
export type HypnogramStage = "awake" | "light" | "deep" | "rem";
export interface HypnogramSegment { start: number; end: number; stage: HypnogramStage }
export interface Hypnogram { day: string; startedAt: string; endedAt: string; segments: HypnogramSegment[] }
export const getHypnogram = (day?: string) => invoke<Hypnogram | null>("noop_hypnogram", { day: day ?? null });
