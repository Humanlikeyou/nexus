import { invoke } from "@tauri-apps/api/core";
import type {
  OpenClawStatus,
  OpenClawHealth,
  CronJob,
  Skill,
  TaskItem,
  ContentPipelineItem,
  MuseDevice,
  MuseConnectionState,
  SystemInfo,
  NexusConfig,
  TelegramMessage,
  PackageInfo,
  Goal,
  RecordedSession,
  BrainSnapshot,
  CanonicalEvent,
  EventQuery,
  BleProofEvent,
  OutboxStatus,
  ArmStrategy,
  ArmDecision,
  ArmRiskStatus,
  ArmRevenueSummary,
  ArmConfig,
  ArmStrategyConfig,
  ReplayFileInfo,
  RouterResponse,
  LlmRouterConfig,
  CostEstimate,
  LlmUsageRecord,
  TerranceContext,
  SessionLogEntry,
  SessionInsights,
} from "./types";

// ============================================================
// System
// ============================================================

export const getSystemInfo = () => invoke<SystemInfo>("get_system_info");
export const healthCheck = () => invoke<string>("health_check");

// ============================================================
// OpenClaw / Amara
// ============================================================

export const readOpenClawStatus = () =>
  invoke<OpenClawStatus>("read_openclaw_status");

export const getOpenClawHealth = () =>
  invoke<OpenClawHealth>("get_openclaw_health");

export const restartOpenclaw = () =>
  invoke<string>("restart_openclaw");

export const readCronJobs = () => invoke<CronJob[]>("read_cron_jobs");

export const readSkills = () => invoke<Skill[]>("read_skills");

export const readTasks = (workspacePath?: string) =>
  invoke<TaskItem[]>("read_tasks", { workspacePath });

export const readContentPipeline = (workspacePath?: string) =>
  invoke<ContentPipelineItem[]>("read_content_pipeline", { workspacePath });

export const readDailyLog = (date?: string) =>
  invoke<string>("read_daily_log", { date });

// ============================================================
// Muse BLE
// ============================================================

export const scanMuse = () => invoke<MuseDevice[]>("scan_muse");

export const connectMuse = (address: string) =>
  invoke<MuseConnectionState>("connect_muse", { address });

export const disconnectMuse = () => invoke<void>("disconnect_muse");

export const startEegStream = () => invoke<void>("start_eeg_stream");

export const stopEegStream = () => invoke<void>("stop_eeg_stream");

export const getMuseState = () =>
  invoke<MuseConnectionState>("get_muse_state");

// ============================================================
// Telegram
// ============================================================

export const getConfig = () => invoke<NexusConfig>("get_config");

export const saveSettings = (config: NexusConfig) =>
  invoke<void>("save_settings", { config });

export const getTelegramMessages = (limit?: number) =>
  invoke<TelegramMessage[]>("get_telegram_messages", { limit });

export const sendTelegramMessage = (text: string) =>
  invoke<TelegramMessage>("send_telegram_message", { text });

export const testTelegramConnection = () =>
  invoke<string>("test_telegram_connection");

export const sendToTerrance = (text: string) =>
  invoke<string>("send_to_terrance", { text });

export const pollTelegramUpdates = (lastUpdateId: number) =>
  invoke<import("./types").TelegramUpdate[]>("poll_telegram_updates", { lastUpdateId });

// ============================================================
// Packager
// ============================================================

export const packItUp = () => invoke<PackageInfo>("pack_it_up");

export const unpackBackup = (archivePath: string) =>
  invoke<string[]>("unpack_backup", { archivePath });

/** Native file picker (tauri-plugin-dialog, already registered in lib.rs; capability dialog:default).
 *  Resolves the chosen path, or null when the user cancels. */
export const pickBackupArchive = async (): Promise<string | null> => {
  const picked = await invoke<string | string[] | null>("plugin:dialog|open", {
    options: {
      title: "Choose an Amara OS backup",
      multiple: false,
      directory: false,
      filters: [{ name: "Amara OS backup", extensions: ["gz", "tgz"] }],
    },
  });
  return Array.isArray(picked) ? (picked[0] ?? null) : picked;
};

export const getAppPaths = () =>
  invoke<Record<string, string>>("get_app_paths");

// ============================================================
// Goals
// ============================================================

export const readGoals = () => invoke<Goal[]>("read_goals");

export const saveGoals = (goals: Goal[]) =>
  invoke<void>("save_goals", { goals });

export const deleteGoal = (goalId: string) =>
  invoke<Goal[]>("delete_goal", { goalId });

// ============================================================
// Sessions
// ============================================================

export const saveSession = (session: RecordedSession, data: unknown) =>
  invoke<string>("save_session", { session, data });

export const listSessions = () =>
  invoke<RecordedSession[]>("list_sessions");

export const deleteSession = (sessionId: string) =>
  invoke<void>("delete_session", { sessionId });

// ============================================================
// Snapshots
// ============================================================

export const saveSnapshot = (snapshot: BrainSnapshot) =>
  invoke<string>("save_snapshot", { snapshot });

export const listSnapshots = () =>
  invoke<BrainSnapshot[]>("list_snapshots");

// ============================================================
// Event Store
// ============================================================

export const insertEvents = (events: CanonicalEvent[]) =>
  invoke<number>("insert_events", { events });

export const queryEvents = (query: EventQuery) =>
  invoke<CanonicalEvent[]>("query_events", { query });

export const migrateExistingData = () =>
  invoke<string>("migrate_existing_data");

// ============================================================
// Chat History (SQLite-persisted)
// ============================================================

export const createChatSession = (title: string) =>
  invoke<import("./types").ChatSession>("create_chat_session", { title });

export const listChatSessions = () =>
  invoke<import("./types").ChatSession[]>("list_chat_sessions");

export const saveChatMessage = (message: import("./types").PersistedChatMessage) =>
  invoke<number>("save_chat_message", { message });

export const loadChatHistory = (sessionId: string, limit?: number) =>
  invoke<import("./types").PersistedChatMessage[]>("load_chat_history", { sessionId, limit });

export const deleteChatSession = (sessionId: string) =>
  invoke<void>("delete_chat_session", { sessionId });

// ============================================================
// Onboarding
// ============================================================

export const checkOnboarding = () => invoke<boolean>("check_onboarding");

export const completeOnboarding = () => invoke<void>("complete_onboarding");

// ============================================================
// ARM — Autonomous Revenue Module
// ============================================================

// Strategy CRUD
export const armListStrategies = () =>
  invoke<ArmStrategy[]>("arm_list_strategies");

export const armCreateStrategy = (
  name: string,
  strategyType: string,
  config: ArmStrategyConfig,
  capitalLimit: number,
  dailyLossCap: number,
) =>
  invoke<ArmStrategy>("arm_create_strategy", {
    name,
    strategyType,
    config,
    capitalLimit,
    dailyLossCap,
  });

export const armUpdateStrategy = (
  id: string,
  name?: string,
  config?: ArmStrategyConfig,
  capitalLimit?: number,
  dailyLossCap?: number,
) =>
  invoke<ArmStrategy>("arm_update_strategy", {
    id,
    name,
    config,
    capitalLimit,
    dailyLossCap,
  });

export const armDeleteStrategy = (id: string) =>
  invoke<void>("arm_delete_strategy", { id });

// Status management
export const armActivateStrategy = (id: string) =>
  invoke<ArmStrategy>("arm_activate_strategy", { id });

export const armPauseStrategy = (id: string, reasoning: string) =>
  invoke<ArmStrategy>("arm_pause_strategy", { id, reasoning });

export const armKillStrategy = (id: string, reasoning: string) =>
  invoke<ArmStrategy>("arm_kill_strategy", { id, reasoning });

// Revenue
export const armPollRevenue = (strategyId: string) =>
  invoke<number>("arm_poll_revenue", { strategyId });

export const armGetRevenueSummary = (from?: string, to?: string) =>
  invoke<ArmRevenueSummary>("arm_get_revenue_summary", { from, to });

// Decisions
export const armLogDecision = (
  strategyId: string,
  action: string,
  reasoning: string,
  context?: Record<string, unknown>,
) =>
  invoke<ArmDecision>("arm_log_decision", { strategyId, action, reasoning, context });

export const armListDecisions = (strategyId?: string, limit?: number) =>
  invoke<ArmDecision[]>("arm_list_decisions", { strategyId, limit });

export const armUpdateDecisionOutcome = (id: string, outcome: string, notes?: string) =>
  invoke<ArmDecision>("arm_update_decision_outcome", { id, outcome, notes });

// Risk
export const armGetRiskStatus = () =>
  invoke<ArmRiskStatus>("arm_get_risk_status");

export const armSetGlobalLimits = (capitalLimit: number, dailyLossCap: number) =>
  invoke<void>("arm_set_global_limits", { capitalLimit, dailyLossCap });

export const armKillSwitch = (active: boolean, reasoning: string) =>
  invoke<void>("arm_kill_switch", { active, reasoning });

// Health + Polling
export const armHealthCheck = (strategyId: string) =>
  invoke<{ status: string; uptime_secs?: number; requests_24h?: number; error_rate?: number }>(
    "arm_health_check",
    { strategyId },
  );

export const armPollAll = () => invoke<ArmStrategy[]>("arm_poll_all");

// Config
export const armGetConfig = () => invoke<ArmConfig>("arm_get_config");

export const armSaveConfig = (config: ArmConfig) =>
  invoke<void>("arm_save_config", { config });

// ============================================================
// BLE Proof Mode
// ============================================================

export const setProofMode = (enabled: boolean) =>
  invoke<void>("set_proof_mode", { enabled });

export const getProofLog = () =>
  invoke<BleProofEvent[]>("get_proof_log");

// ============================================================
// Outbox / Gateway Pipeline
// ============================================================

export const getOutboxStatus = () =>
  invoke<OutboxStatus>("get_outbox_status");

export const simulateGatewayDown = () =>
  invoke<void>("simulate_gateway_down");

export const simulateGatewayUp = () =>
  invoke<void>("simulate_gateway_up");

// ============================================================
// Replay / Demo
// ============================================================

export const startEegRecording = () =>
  invoke<string>("start_eeg_recording");

export const stopEegRecording = () =>
  invoke<string>("stop_eeg_recording");

export const listReplays = () =>
  invoke<ReplayFileInfo[]>("list_replays");

export const startReplayFile = (filename: string, speed: number) =>
  invoke<void>("start_replay", { filename, speed });

export const stopReplay = () => invoke<void>("stop_replay");

export const startDemoEeg = (mode: string, speed: number) =>
  invoke<void>("start_demo_eeg", { mode, speed });

export const stopDemoEeg = () => invoke<void>("stop_demo_eeg");

// ============================================================
// LLM Router
// ============================================================

export const chatWithRouter = (
  text: string,
  config: LlmRouterConfig,
  tags: string[],
  forceProvider?: string,
  anthropicApiKey?: string,
) =>
  invoke<RouterResponse>("chat_with_router", {
    text,
    config,
    tags,
    forceProvider,
    anthropicApiKey,
  });

export const getCostEstimate = (
  text: string,
  config: LlmRouterConfig,
  tags: string[],
) =>
  invoke<CostEstimate>("get_cost_estimate", { text, config, tags });

export const getLlmUsage = () =>
  invoke<LlmUsageRecord[]>("get_llm_usage");

export const clearLlmUsage = () =>
  invoke<void>("clear_llm_usage");

// ============================================================
// Amara Continuity
// ============================================================

export const readIdentityFile = () =>
  invoke<string>("read_identity_file");

export const loadTerranceContext = () =>
  invoke<TerranceContext>("load_terrance_context");

export const saveTerranceContext = (context: TerranceContext) =>
  invoke<void>("save_terrance_context", { context });

export const storeSessionLog = (entry: SessionLogEntry) =>
  invoke<void>("store_session_log", { entry });

export const listSessionLogs = () =>
  invoke<SessionLogEntry[]>("list_session_logs");

export const archiveOldSessionLogs = (daysThreshold: number) =>
  invoke<number>("archive_old_session_logs", { daysThreshold });

export const pruneContextMemory = (maxItems: number) =>
  invoke<number>("prune_context_memory", { maxItems });

export const getSessionInsights = () =>
  invoke<string>("get_session_insights").then((raw) => JSON.parse(raw) as SessionInsights);

export const loadCustomJobs = () =>
  invoke<string>("load_custom_jobs").then((raw) => JSON.parse(raw));

export const saveCustomJobs = (data: string) =>
  invoke<void>("save_custom_jobs", { data });

export const saveSkill = (name: string, content: string) =>
  invoke<void>("save_skill", { name, content });

export const deleteSkill = (name: string) =>
  invoke<boolean>("delete_skill", { name });

// ============================================================
// System — Docker / Port checks
// ============================================================

export const checkDockerService = (name: string) =>
  invoke<string>("check_docker_service", { name });

export const restartDockerService = (name: string) =>
  invoke<string>("restart_docker_service", { name });

export const checkServicePort = (host: string, port: number) =>
  invoke<string>("check_service_port", { host, port });

// ============================================================
// Research
// ============================================================

export const webSearch = (query: string, maxResults?: number) =>
  invoke<import("./types").ResearchResult[]>("web_search", { query, maxResults });

export const fetchUrl = (url: string, maxChars?: number) =>
  invoke<string>("fetch_url", { url, maxChars });

// ============================================================
// Activity Log
// ============================================================

export const storeActivityEntry = (entry: import("./types").ActivityEntry) =>
  invoke<void>("store_activity_entry", { entry });

export const listActivityEntries = (limit?: number) =>
  invoke<import("./types").ActivityEntry[]>("list_activity_entries", { limit });

export const pruneActivityLog = (days: number) =>
  invoke<number>("prune_activity_log", { days });
export interface ActivityArchiveReport {
  dryRun: boolean;
  scanned: number;
  selected: number;
  archived: number;
  skipped: number;
  kept: number;
  months: [string, number][];
}
export const archiveActivityLog = (opts: { keepDays?: number; maxFiles?: number; dryRun?: boolean } = {}) =>
  invoke<ActivityArchiveReport>("archive_activity_log", opts);

// ============================================================
// Subagents
// ============================================================

export const loadSubagents = () => invoke<string>("load_subagents");

export const saveSubagents = (data: string) =>
  invoke<void>("save_subagents", { data });

// ============================================================
// CrewAI
// ============================================================

export const crewList = () =>
  invoke<import("./types").CrewInfo[]>("crew_list");

export const crewStart = (name: string, task: string, agents?: string[]) =>
  invoke<import("./types").CrewInfo>("crew_start", { name, task, agents });

export const crewStatus = (id: string) =>
  invoke<import("./types").CrewInfo>("crew_status", { id });

export const crewStop = (id: string) =>
  invoke<string>("crew_stop", { id });

export const crewHealth = () =>
  invoke<string>("crew_health");
