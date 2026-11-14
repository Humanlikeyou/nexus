import { Channel, invoke } from "@tauri-apps/api/core";
import { commands as bound } from "./bindings.gen";
import { EMPTY_DEVICE_ID } from "./lib/devices";
import type {
  CrewApprovalRow,
  CrewAuditApiRow,
  CrewInfo,
  CrewRunSteps,
  CrewServiceHealth,
  CrewSnapshot,
  CrewToolGrantRow,
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
  N8nStatusRow,
  MuseState,
  MutedTopic,
  NexusUnavailableSource,
  NoopHealthSnapshot,
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
  TurnEvent,
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
  get_noop_health_snapshot: "Health sync",
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
  crew_health: "Crew service",
  crew_list: "Crew runs",
  crew_tools: "Crew tools",
  kb_status: "Knowledge base",
};

function unavailableSource(command: string): NexusUnavailableSource {
  return { command, label: SOURCE_LABELS[command] ?? command };
}

/** Generated bindings type Rust `()` as `null`; the runtime value is unchanged, so callers keep `Promise<void>`. */
// Where a bound call ends in `as Promise<T>`: the generated type is looser than the hand-written one for
// harmless reasons (Rust `String` vs a TS literal union, `serde_json::Value`, `f64` typed `number | null`
// by specta for NaN). A direct assertion still fails to compile if the two shapes stop overlapping.
// Rust `Option` args are typed `T | null` by the bindings, but the old hand-written calls OMITTED absent
// optionals from the payload. `opt()` yields undefined (JSON drops it) so the wire payload is unchanged.
const opt = <T = never>(v?: T | null): T | null => (v ?? undefined) as T | null;
const asVoid = (p: Promise<null>): Promise<void> => p as Promise<unknown> as Promise<void>;

async function read<T>(command: string, fallback: T, call: () => Promise<T>): Promise<SnapshotRead<T>> {
  try {
    return { unavailable: null, value: await call() };
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
  message: "Health data is not available yet.",
};

const EMPTY_KB: KbStatus = { state: "not_running", model: null, docs: null, detail: null };
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
  crew: { health: "offline", runs: [], tools: [] },
  kb: EMPTY_KB,
  ledger: null,
};

export async function loadSystemSnapshot(): Promise<SystemSnapshot> {
  const [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, crewHealth, crewRuns, crewTools, kb, ledger] =
    await Promise.all([
      read<HermesRuntimeHealth>("get_terrence_runtime_health", INITIAL_SNAPSHOT.hermes, () => bound.getTerrenceRuntimeHealth()),
      read<TerrenceCapability[]>("list_terrence_capabilities", [], () => bound.listTerrenceCapabilities()),
      read<TerrenceContextSnapshot>("get_terrence_context_snapshot", EMPTY_CONTEXT, () => bound.getTerrenceContextSnapshot() as Promise<TerrenceContextSnapshot>),
      read<TerrencePrioritySnapshot>("get_current_terrence_priority_snapshot", EMPTY_PRIORITY_SNAPSHOT, () => bound.getCurrentTerrencePrioritySnapshot() as Promise<TerrencePrioritySnapshot>),
      read<TerrenceDecision[]>("list_terrence_decisions", [], () => bound.listTerrenceDecisions(opt()) as Promise<TerrenceDecision[]>),
      read<TerrenceAction[]>("list_terrence_actions", [], () => bound.listTerrenceActions(opt()) as Promise<TerrenceAction[]>),
      read<TerrenceAutomation[]>("list_terrence_automations", [], () => bound.listTerrenceAutomations(opt()) as Promise<TerrenceAutomation[]>),
      read<TerrenceAutomationRun[]>("list_terrence_automation_runs", [], () => bound.listTerrenceAutomationRuns(opt(), opt()) as Promise<TerrenceAutomationRun[]>),
      read<TerrenceProject[]>("list_terrence_projects", [], () => bound.listTerrenceProjects(opt()) as Promise<TerrenceProject[]>),
      read<NoopHealthSnapshot>("get_noop_health_snapshot", EMPTY_NOOP, () => bound.getNoopHealthSnapshot() as Promise<NoopHealthSnapshot>),
      read<MuseState>("get_muse_state", INITIAL_SNAPSHOT.muse, () => bound.getMuseState()),
      read<RecordedSession[]>("list_sessions", [], () => bound.listSessions()),
      read<TaskItem[]>("read_tasks", [], () => bound.readTasks(null)),
      read<PipelineItem[]>("read_content_pipeline", [], () => bound.readContentPipeline(null)),
      read<RevenueSummary>("arm_get_revenue_summary", EMPTY_REVENUE, () => bound.armGetRevenueSummary(null, null) as Promise<RevenueSummary>),
      read<Strategy[]>("arm_list_strategies", [], () => bound.armListStrategies() as Promise<Strategy[]>),
      read<RiskStatus>("arm_get_risk_status", EMPTY_RISK, () => bound.armGetRiskStatus() as Promise<RiskStatus>),
      read<Goal[]>("read_goals", [], () => bound.readGoals() as Promise<Goal[]>),
      read<OutboxStatus>("get_outbox_status", INITIAL_SNAPSHOT.outbox, () => bound.getOutboxStatus()),
      read<ActivityEntry[]>("list_activity_entries", [], () => bound.listActivityEntries(30)),
      read<string>("crew_health", "offline", () => bound.crewHealth()),
      read<CrewInfo[]>("crew_list", [], () => bound.crewList()),
      read<CrewToolInfo[]>("crew_tools", [], () => bound.crewTools()),
      read<KbStatus>("kb_status", EMPTY_KB, () => bound.kbStatus()),
      read<SystemSnapshot["ledger"]>("read_business_ledger", null, () => bound.readBusinessLedger() as Promise<SystemSnapshot["ledger"]>),
    ]);

  const crew = crewSnapshot(crewHealth.value, crewRuns.value, crewTools.value);
  // crew_list can only succeed when the service is configured and up; its
  // failure is expected (not a broken source) otherwise.
  if (crew.health !== "online") crewRuns.unavailable = null;
  const reads = [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, crewHealth, crewRuns, crewTools, kb, ledger];

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

export const startCrew = (name: string, task: string, retryOf: string | null = null): Promise<CrewInfo> => bound.crewStart(name, task, null, retryOf);
export const listCrewApprovals = (state: "pending" | "decided"): Promise<CrewApprovalRow[]> => bound.crewListApprovals(state);
/** Approve opens the native dialog first; deny does not. Errors carry a "declined:" etc. prefix. */
export const decideCrewApproval = (id: string, approve: boolean): Promise<CrewApprovalRow> => bound.crewDecideApproval(id, approve);
export const getCrewStatus = (id: string): Promise<CrewInfo> => bound.crewStatus(id);
export const getCrewRunSteps = (id: string): Promise<CrewRunSteps> => bound.crewRunSteps(id);
export const stopCrew = (id: string): Promise<string> => bound.crewStop(id);
export type ScreenWeekCount = { weekStart: string; screen: string; count: number };
/** Local screen-open counter (screen id + time only). Recording is silent without a backend (browser/e2e). */
export const recordScreenOpen = async (screen: string): Promise<void> => {
  try { await bound.screenOpenRecord(screen); } catch { /* no backend: nothing to record */ }
};
/** Counts per screen per JST week. Errors reach the caller: an empty list must mean "no opens", never "store failed". */
export const screenOpenCounts = (weeks?: number): Promise<ScreenWeekCount[]> => bound.screenOpenCounts(weeks ?? null);
/** R13-F: read-only n8n probe; never errors (connected:false when it is down). */
export const getN8nStatus = (): Promise<N8nStatusRow> => bound.n8nStatus();
/** 1110i. Turning on opens the native dialog first and resolves only after crew-service confirms. */
export const getCrewToolGrant = (crewId: string): Promise<CrewToolGrantRow> => bound.crewToolGrantGet(crewId) as Promise<CrewToolGrantRow>;
export const setCrewToolGrant = (crewId: string, tools: string[], duration: "run" | "1h" | "24h"): Promise<CrewToolGrantRow> => bound.crewToolGrantSet(crewId, tools, duration) as Promise<CrewToolGrantRow>;
export const revokeCrewToolGrant = (crewId: string): Promise<CrewToolGrantRow> => bound.crewToolGrantRevoke(crewId) as Promise<CrewToolGrantRow>;
export const getCrewToolAudit = (crewId: string, limit = 50, before: number | null = null): Promise<CrewAuditApiRow[]> => bound.crewToolAudit(crewId, limit, before) as Promise<CrewAuditApiRow[]>;

/** Turn pipeline (flag nexus.turnPipeline). Resolves with the turn id at once; events arrive on `onEvent`. */
export const amaraTurn = (
  input: { conversationId: string; text: string; deep?: boolean; haNames?: string[] },
  surface: "typed" | "voice" | "lens" | "window",
  onEvent: (event: TurnEvent) => void,
) => {
  const channel = new Channel<TurnEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("amara_turn", { input, surface, channel });
};
export const amaraTurnCancel = (turnId: string): Promise<boolean> => bound.amaraTurnCancel(turnId);

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
}): Promise<TerrenceInteraction> => bound.interactWithTerrence(request) as Promise<TerrenceInteraction>;

/** Background context for a turn, to run through the injection guard before it is sent. */
export const prepareAmaraTurn = (message: string): Promise<AmaraTurnContext> => bound.prepareAmaraTurn(message);

/** Persist an exchange made outside Hermes (/deliberate via the router). */
export const recordAmaraInteraction = (input: {
  conversationId: string;
  userMessage: string;
  assistantMessage: string;
  tier: AmaraTierInfo | null;
  durationMs: number;
  /** ^[a-z0-9_-]{1,32}$, e.g. "home"; Rust rejects others with invalid_source. */
  source?: string | null;
}): Promise<TerrenceInteraction> => bound.recordAmaraInteraction(input) as Promise<TerrenceInteraction>;

export const listTerrenceInteractions = (conversationId: string, limit = 80): Promise<TerrenceInteraction[]> =>
  bound.listTerrenceInteractions(conversationId, limit) as Promise<TerrenceInteraction[]>;

export const migrateLegacyTerrenceContext = (): Promise<unknown> => bound.migrateLegacyTerrenceContext();
export const listTerrenceContextItems = (query: TerrenceContextItemQuery = { limit: 120 }): Promise<TerrenceContextItem[]> =>
  bound.listContextItems(query) as Promise<TerrenceContextItem[]>;
export const correctTerrenceContextItem = (input: CorrectTerrenceContextItemInput): Promise<TerrenceContextItem> =>
  bound.correctContextItem(input as Parameters<typeof bound.correctContextItem>[0]) as Promise<TerrenceContextItem>;
export const deleteTerrenceContextItem = (id: string): Promise<void> =>
  asVoid(bound.deleteContextItem(id));
export const approveTerrenceAction = (id: string): Promise<TerrenceAction> => bound.approveTerrenceAction(id) as Promise<TerrenceAction>;
export const retryTerrenceAction = (id: string): Promise<TerrenceAction> => bound.retryTerrenceAction(id) as Promise<TerrenceAction>;
export const cancelTerrenceAction = (id: string): Promise<TerrenceAction> => bound.cancelTerrenceAction(id) as Promise<TerrenceAction>;

export const getReduceTransparency = (): Promise<boolean> => bound.getReduceTransparency();
export const scanMuse = (): Promise<MuseDevice[]> => bound.scanMuse();
export const connectMuse = (address: string): Promise<MuseState> => bound.connectMuse(address);
export const disconnectMuse = (): Promise<void> => asVoid(bound.disconnectMuse());
export const startEegStream = (): Promise<void> => asVoid(bound.startEegStream());
export const stopEegStream = (): Promise<void> => asVoid(bound.stopEegStream());
export const startEegRecording = (): Promise<string> => bound.startEegRecording();
export const stopEegRecording = (): Promise<string> => bound.stopEegRecording();
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
export const listRecordings = (): Promise<RecordingListItemIpc[]> => bound.listRecordings() as Promise<RecordingListItemIpc[]>;
export const readRecordingWindow = (id: string, startSecs: number, spanSecs: number, points?: number): Promise<RecordingWindowIpc> =>
  bound.readRecordingWindow(id, startSecs, spanSecs, opt(points)) as Promise<RecordingWindowIpc>;
export const readBodyWindow = (id: string, startSecs: number, spanSecs: number): Promise<BodyWindowIpc> =>
  bound.readBodyWindow(id, startSecs, spanSecs) as Promise<BodyWindowIpc>;
export const recordingSeries = (id: string): Promise<SessionSeriesIpc> => bound.recordingSeries(id);
export const getSessionSamples = (sessionId: string): Promise<SessionSamplesIpc | null> => bound.getSessionSamples(sessionId);
/** Tells the open recording where the back-of-head electrode was found; written to its trailer. */
export const markRecordingAux = (aux: AuxInfo): Promise<void> => asVoid(bound.markRecordingAux(aux));
export const saveMuseSession = (session: RecordedSession & { file_path: string }, data: unknown): Promise<string> =>
  bound.saveSession(session, data as Parameters<typeof bound.saveSession>[1]);

/** Settings › "Allow Claude": only this flag crosses the bridge (no keys). */
export const getAllowClaude = (): Promise<boolean> => bound.getAllowClaude();
export const setAllowClaude = (enabled: boolean): Promise<boolean> => bound.setAllowClaude(enabled);

/** Amara Voice (commands/voice.rs). */
export const getVoiceStatus = (): Promise<VoiceStatus> => bound.voiceStatus() as Promise<VoiceStatus>;
export const startVoice = (): Promise<VoiceStatus> => bound.voiceStart() as Promise<VoiceStatus>;
export const stopVoice = (): Promise<VoiceStatus> => bound.voiceStop() as Promise<VoiceStatus>;
export const getVoiceTurns = (n = 20): Promise<VoiceTurn[]> => bound.voiceRecentTurns(n) as Promise<VoiceTurn[]>;
export const listVoices = (): Promise<VoiceList> => bound.voiceListVoices();
export const setVoice = (name: string): Promise<VoiceList> => bound.voiceSetVoice(name);
/** Interrupt the current spoken reply only (SIGUSR1). false = nothing sent (not running / loop not patched). */
export const stopVoiceSpeaking = (): Promise<boolean> => bound.voiceStopSpeaking();

/** Amara's follow-up lists (commands/followups.rs). Errors are the bare kinds in lib/followups.ts. */
export const listFollowups = (): Promise<FollowupLists> => bound.followupsList();
/** Experiment loop (commands/experiments.rs). Errors reach the caller. */
export interface ExperimentRow { followupId: number; startDay: string; measure: string | null; pair: string | null }
export const recordExperimentStart = (followupId: number, pair: string | null): Promise<ExperimentRow> => bound.experimentsRecordStart(followupId, pair);
export const listExperiments = (): Promise<ExperimentRow[]> => bound.experimentsList();
/** Hemi-Sync track ranking (read-only). Errors reach the caller. */
export interface HemiRankedTrack { rank: number; trackId: string; n: number; mean: number; sessionIds: string[] }
export interface HemiUnrankedTrack { trackId: string; n: number; sessionIds: string[]; reason: string }
export interface HemiMeasureRanking { ranked: HemiRankedTrack[]; notEnoughSessions: HemiUnrankedTrack[] }
export interface HemiTrackRanking { minSessions: number; focusField: string; calmField: string; focus: HemiMeasureRanking; calm: HemiMeasureRanking; rating: HemiMeasureRanking }
export const hemisyncRanking = (): Promise<HemiTrackRanking> => bound.hemisyncRanking() as Promise<HemiTrackRanking>;
export const addFollowupItem = (input: FollowupItemInput): Promise<FollowupItem> => bound.followupsAddItem(input.text, opt(input.due), opt(input.next));
export const updateFollowupItem = (id: number, patch: FollowupItemPatch): Promise<FollowupItem> => bound.followupsUpdateItem(id, patch);
export const markFollowupDone = (id: number): Promise<FollowupItem> => bound.followupsMarkDone(id);
export const reopenFollowupItem = (id: number): Promise<FollowupItem> => bound.followupsReopenItem(id);
export const deleteFollowupItem = (id: number): Promise<void> => asVoid(bound.followupsDeleteItem(id));
export const addMutedTopic = (topic: string, until?: string): Promise<MutedTopic> => bound.followupsAddMutedTopic(topic, until ? until : opt());
export const deleteMutedTopic = (topic: string): Promise<void> => asVoid(bound.followupsDeleteMutedTopic(topic));

/** Small app state on this Mac (commands/kv.rs). */
export const loadKv = (key: string): Promise<string | null> => bound.loadKv(key);
/** Generated bindings type Rust `()` as `null`; the runtime value is unchanged, so callers keep `Promise<void>`. */
export const saveKv = (key: string, value: string): Promise<void> => bound.saveKv(key, value) as Promise<unknown> as Promise<void>;

/** Home's brief (commands/home.rs). Null fields mean "not available": the page simply shows less. */
export interface HomeWeather { place: string | null; tempC: number | null; summary: string | null; highC: number | null; lowC: number | null }
export interface HomeBrief { greeting: string | null; weather: HomeWeather | null; updatedAt: string | null }
export const getHomeBrief = (): Promise<HomeBrief> => bound.homeBrief();
export interface HeartSample { bpm: number; at: number }
export interface LiveHeartRate { bpm: number; at: number; trace: HeartSample[] }
export const getLiveHeartRate = (): Promise<LiveHeartRate | null> => bound.liveHeartRate();
/** NOOP workouts (noop.rs): read-only, notes never cross. zoneMinutes = minutes in zones 1..5, or null. */
export interface NoopWorkout {
  day: string; startedAt: string; endedAt: string; sport: string;
  durationMin: number | null; energyKcal: number | null; avgHr: number | null; maxHr: number | null;
  strain: number | null; distanceM: number | null; steps: number | null; zoneMinutes: number[] | null;
}
export const getRecentWorkouts = (days = 14): Promise<NoopWorkout[]> => bound.noopRecentWorkouts(days) as Promise<NoopWorkout[]>;
/** One night's sleep stages (unix seconds, clipped to the night); null when the night has no segments. */
export type HypnogramStage = "awake" | "light" | "deep" | "rem";
export interface HypnogramSegment { start: number; end: number; stage: HypnogramStage }
export interface Hypnogram { day: string; startedAt: string; endedAt: string; segments: HypnogramSegment[] }
export const getHypnogram = (day?: string): Promise<Hypnogram | null> => bound.noopHypnogram(day ?? null) as Promise<Hypnogram | null>;

/** Daily table (commands/daily_table.rs): one row per JST day. null = no source rows, never 0. Errors reach the caller. */
export type DailySources = Record<string, string[]>;
export interface DailyRow {
  day: string; sleepTotalMin: number | null; hrvMs: number | null; recovery: number | null; strain: number | null;
  workoutMin: number | null; museMin: number | null; focusMean: number | null; calmMean: number | null;
  hemisyncMin: number | null; hemisyncRatingMean: number | null; moodMean: number | null; energyMean: number | null;
  checkinTags: string[] | null; takesShown: number | null; takesRated: number | null; takesHelpful: number | null;
  followupsClosed: number | null; sources: DailySources;
}
export interface DailyGoalRow { day: string; goalId: string; roadmapStepsDone: number | null; followupsClosed: number | null; sources: DailySources }
export interface DailyBuild { days: number; builtAtMs: number; partial: string[] }
export interface DailyRange { rows: DailyRow[]; goals: DailyGoalRow[]; builtAtMs: number }
export const dailyRebuild = (): Promise<DailyBuild> => bound.dailyRebuild();
export const dailyRange = (from: string, to: string): Promise<DailyRange> => bound.dailyRange(from, to);

/** Goal progress (commands/goal_progress.rs). Every count lists the `daily_goal:<day>:<goal id>` rows behind it; the "in common" readout quotes only published generic findings. Errors reach the caller. */
export interface GoalWeekWire { goalId: string; title: string | null; thisWeek: number; prevWeek: number; refs: string[] }
export interface StalledGoalWire { goalId: string; title: string; lastFinishedDay: string | null; daysSince: number | null; refs: string[] }
export interface CommonCauseWire {
  cause: string; effect: string; lagDays: number; beforeLagDays: number; findingPAdj: number; confidence: number; beforeDaysTested: number; beforeDaysCauseHigh: number;
  share: number; baseRate: number; pRaw: number; pAdj: number; causeHighBeforeDays: string[]; refs: string[];
}
export interface CommonGoalWire { goalId: string; title: string | null; finishedDays: number; beforeDays: string[]; causes: CommonCauseWire[] }
export interface GoalProgressWire {
  weekStart: string; weekEnd: string; prevWeekStart: string; goals: GoalWeekWire[]; stalled: StalledGoalWire[];
  inCommon: { state: "found" | "nothing_stands_out_yet"; goalsConsidered: number; goals: CommonGoalWire[]; skippedCauses: string[] };
}
export const goalProgressWeek = (weekEnd?: string): Promise<GoalProgressWire> => bound.goalProgressWeek(weekEnd ?? null) as Promise<GoalProgressWire>;
