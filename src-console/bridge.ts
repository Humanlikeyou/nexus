import { invoke } from "@tauri-apps/api/core";
import type {
  ActivityEntry,
  CorrectTerrenceContextItemInput,
  Goal,
  HermesRuntimeHealth,
  MuseDevice,
  MuseState,
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
  TerrenceAction,
  TerrenceAutomation,
  TerrenceAutomationRun,
  TerrencePrioritySnapshot,
  TerrenceProject,
} from "./types";

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
  read_openclaw_status: "OpenClaw",
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
  deviceId: "my-whoop",
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

const EMPTY_OPENCLAW: OpenClawStatus = { status: "offline", online: false, model: "unknown", gateway_url: "", last_heartbeat: null };

/** Keep only the typed fields (defence in depth; the Rust command already returns just these). */
function pickOpenClaw(s: OpenClawStatus): OpenClawStatus {
  const status = s?.status === "online" || s?.status === "configured" || s?.status === "offline" ? s.status : s?.online ? "online" : "offline";
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
};

export async function loadSystemSnapshot(): Promise<SystemSnapshot> {
  const [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, openclaw] =
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
    ]);

  const reads = [hermes, capabilities, context, prioritySnapshot, decisions, actions, automations, automationRuns, projects, noop, muse, sessions, tasks, pipeline, revenue, strategies, risk, goals, outbox, activity, openclaw];

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
  };
}

export const interactWithTerrence = (request: {
  message: string;
  conversationId: string;
  hermesSessionId: string | null;
}) => invoke<TerrenceInteraction>("interact_with_terrence", { request });

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

export const scanMuse = () => invoke<MuseDevice[]>("scan_muse");
export const connectMuse = (address: string) => invoke<MuseState>("connect_muse", { address });
export const disconnectMuse = () => invoke<void>("disconnect_muse");
export const startEegStream = () => invoke<void>("start_eeg_stream");
export const stopEegStream = () => invoke<void>("stop_eeg_stream");
export const startEegRecording = () => invoke<string>("start_eeg_recording");
export const stopEegRecording = () => invoke<string>("stop_eeg_recording");
export const saveMuseSession = (session: RecordedSession & { file_path: string }, data: unknown) =>
  invoke<string>("save_session", { session, data });
