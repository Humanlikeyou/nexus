import { invokeBound, pluginDialogOpen } from "../../bridge";
import type {
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

export const getSystemInfo = () => invokeBound<SystemInfo>("get_system_info");
export const healthCheck = () => invokeBound<string>("health_check");

// ============================================================
// Skills / tasks / content pipeline
// ============================================================

export const readSkills = () => invokeBound<Skill[]>("read_skills");

export const readTasks = (workspacePath?: string) =>
  invokeBound<TaskItem[]>("read_tasks", { workspacePath });

export const readContentPipeline = (workspacePath?: string) =>
  invokeBound<ContentPipelineItem[]>("read_content_pipeline", { workspacePath });

// ============================================================
// Muse BLE
// ============================================================

export const scanMuse = () => invokeBound<MuseDevice[]>("scan_muse");

export const connectMuse = (address: string) =>
  invokeBound<MuseConnectionState>("connect_muse", { address });

export const disconnectMuse = () => invokeBound<void>("disconnect_muse");

export const startEegStream = () => invokeBound<void>("start_eeg_stream");

export const stopEegStream = () => invokeBound<void>("stop_eeg_stream");

export const getMuseState = () =>
  invokeBound<MuseConnectionState>("get_muse_state");

// ============================================================
// Telegram
// ============================================================

export const getConfig = () => invokeBound<NexusConfig>("get_config");

export const saveSettings = (config: NexusConfig) =>
  invokeBound<void>("save_settings", { config });

export const getTelegramMessages = (limit?: number) =>
  invokeBound<TelegramMessage[]>("get_telegram_messages", { limit });

export const sendTelegramMessage = (text: string) =>
  invokeBound<TelegramMessage>("send_telegram_message", { text });

export const testTelegramConnection = () =>
  invokeBound<string>("test_telegram_connection");

export const sendToTerrance = (text: string) =>
  invokeBound<string>("send_to_terrance", { text });

export const pollTelegramUpdates = (lastUpdateId: number) =>
  invokeBound<import("./types").TelegramUpdate[]>("poll_telegram_updates", { lastUpdateId });

// ============================================================
// Packager
// ============================================================

export const packItUp = () => invokeBound<PackageInfo>("pack_it_up");

export const unpackBackup = (archivePath: string) =>
  invokeBound<string[]>("unpack_backup", { archivePath });

/** Native file picker (tauri-plugin-dialog, already registered in lib.rs; capability dialog:default).
 *  Resolves the chosen path, or null when the user cancels. */
export const pickBackupArchive = async (): Promise<string | null> => {
  const picked = await pluginDialogOpen<string | string[] | null>({
    title: "Choose an Amara OS backup",
    multiple: false,
    directory: false,
    filters: [{ name: "Amara OS backup", extensions: ["gz", "tgz"] }],
  });
  return Array.isArray(picked) ? (picked[0] ?? null) : picked;
};

export const getAppPaths = () =>
  invokeBound<Record<string, string>>("get_app_paths");

// ============================================================
// Goals
// ============================================================

export const readGoals = () => invokeBound<Goal[]>("read_goals");

export const saveGoals = (goals: Goal[]) =>
  invokeBound<void>("save_goals", { goals });

export const deleteGoal = (goalId: string) =>
  invokeBound<Goal[]>("delete_goal", { goalId });

// ============================================================
// Sessions
// ============================================================

export const saveSession = (session: RecordedSession, data: unknown) =>
  invokeBound<string>("save_session", { session, data });

export const listSessions = () =>
  invokeBound<RecordedSession[]>("list_sessions");

// ============================================================
// Snapshots
// ============================================================

export const saveSnapshot = (snapshot: BrainSnapshot) =>
  invokeBound<string>("save_snapshot", { snapshot });

export const listSnapshots = () =>
  invokeBound<BrainSnapshot[]>("list_snapshots");

// ============================================================
// Event Store
// ============================================================

export const insertEvents = (events: CanonicalEvent[]) =>
  invokeBound<number>("insert_events", { events });

export const queryEvents = (query: EventQuery) =>
  invokeBound<CanonicalEvent[]>("query_events", { query });

export const migrateExistingData = () =>
  invokeBound<string>("migrate_existing_data");

// ============================================================
// Chat History (SQLite-persisted)
// ============================================================

export const createChatSession = (title: string) =>
  invokeBound<import("./types").ChatSession>("create_chat_session", { title });

export const listChatSessions = () =>
  invokeBound<import("./types").ChatSession[]>("list_chat_sessions");

export const saveChatMessage = (message: import("./types").PersistedChatMessage) =>
  invokeBound<number>("save_chat_message", { message });

export const loadChatHistory = (sessionId: string, limit?: number) =>
  invokeBound<import("./types").PersistedChatMessage[]>("load_chat_history", { sessionId, limit });

export const deleteChatSession = (sessionId: string) =>
  invokeBound<void>("delete_chat_session", { sessionId });

// ============================================================
// Onboarding
// ============================================================

export const checkOnboarding = () => invokeBound<boolean>("check_onboarding");

export const completeOnboarding = () => invokeBound<void>("complete_onboarding");

// ============================================================
// ARM — Autonomous Revenue Module
// ============================================================

// Strategy CRUD
export const armListStrategies = () =>
  invokeBound<ArmStrategy[]>("arm_list_strategies");

export const armCreateStrategy = (
  name: string,
  strategyType: string,
  config: ArmStrategyConfig,
  capitalLimit: number,
  dailyLossCap: number,
) =>
  invokeBound<ArmStrategy>("arm_create_strategy", {
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
  invokeBound<ArmStrategy>("arm_update_strategy", {
    id,
    name,
    config,
    capitalLimit,
    dailyLossCap,
  });

export const armDeleteStrategy = (id: string) =>
  invokeBound<void>("arm_delete_strategy", { id });

// Status management
export const armActivateStrategy = (id: string) =>
  invokeBound<ArmStrategy>("arm_activate_strategy", { id });

export const armPauseStrategy = (id: string, reasoning: string) =>
  invokeBound<ArmStrategy>("arm_pause_strategy", { id, reasoning });

export const armKillStrategy = (id: string, reasoning: string) =>
  invokeBound<ArmStrategy>("arm_kill_strategy", { id, reasoning });

// Revenue
export const armPollRevenue = (strategyId: string) =>
  invokeBound<number>("arm_poll_revenue", { strategyId });

export const armGetRevenueSummary = (from?: string, to?: string) =>
  invokeBound<ArmRevenueSummary>("arm_get_revenue_summary", { from, to });

// Decisions
export const armLogDecision = (
  strategyId: string,
  action: string,
  reasoning: string,
  context?: Record<string, unknown>,
) =>
  invokeBound<ArmDecision>("arm_log_decision", { strategyId, action, reasoning, context });

export const armListDecisions = (strategyId?: string, limit?: number) =>
  invokeBound<ArmDecision[]>("arm_list_decisions", { strategyId, limit });

export const armUpdateDecisionOutcome = (id: string, outcome: string, notes?: string) =>
  invokeBound<ArmDecision>("arm_update_decision_outcome", { id, outcome, notes });

// Risk
export const armGetRiskStatus = () =>
  invokeBound<ArmRiskStatus>("arm_get_risk_status");

export const armSetGlobalLimits = (capitalLimit: number, dailyLossCap: number) =>
  invokeBound<void>("arm_set_global_limits", { capitalLimit, dailyLossCap });

export const armKillSwitch = (active: boolean, reasoning: string) =>
  invokeBound<void>("arm_kill_switch", { active, reasoning });

// Health + Polling
export const armHealthCheck = (strategyId: string) =>
  invokeBound<{ status: string; uptime_secs?: number; requests_24h?: number; error_rate?: number }>(
    "arm_health_check",
    { strategyId },
  );

export const armPollAll = () => invokeBound<ArmStrategy[]>("arm_poll_all");

// Config
export const armGetConfig = () => invokeBound<ArmConfig>("arm_get_config");

export const armSaveConfig = (config: ArmConfig) =>
  invokeBound<void>("arm_save_config", { config });

// ============================================================
// BLE Proof Mode
// ============================================================

export const setProofMode = (enabled: boolean) =>
  invokeBound<void>("set_proof_mode", { enabled });

export const getProofLog = () =>
  invokeBound<BleProofEvent[]>("get_proof_log");

// ============================================================
// Outbox / Gateway Pipeline
// ============================================================

export const getOutboxStatus = () =>
  invokeBound<OutboxStatus>("get_outbox_status");

export const simulateGatewayDown = () =>
  invokeBound<void>("simulate_gateway_down");

export const simulateGatewayUp = () =>
  invokeBound<void>("simulate_gateway_up");

// ============================================================
// Replay / Demo
// ============================================================

export const startEegRecording = () =>
  invokeBound<string>("start_eeg_recording");

export const stopEegRecording = () =>
  invokeBound<string>("stop_eeg_recording");

export const listReplays = () =>
  invokeBound<ReplayFileInfo[]>("list_replays");

export const startReplayFile = (filename: string, speed: number) =>
  invokeBound<void>("start_replay", { filename, speed });

export const stopReplay = () => invokeBound<void>("stop_replay");

export const startDemoEeg = (mode: string, speed: number) =>
  invokeBound<void>("start_demo_eeg", { mode, speed });

export const stopDemoEeg = () => invokeBound<void>("stop_demo_eeg");

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
  invokeBound<RouterResponse>("chat_with_router", {
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
  invokeBound<CostEstimate>("get_cost_estimate", { text, config, tags });

export const getLlmUsage = () =>
  invokeBound<LlmUsageRecord[]>("get_llm_usage");

export const clearLlmUsage = () =>
  invokeBound<void>("clear_llm_usage");

// ============================================================
// Amara Continuity
// ============================================================

export const readIdentityFile = () =>
  invokeBound<string>("read_identity_file");

export const loadTerranceContext = () =>
  invokeBound<TerranceContext>("load_terrance_context");

export const saveTerranceContext = (context: TerranceContext) =>
  invokeBound<void>("save_terrance_context", { context });

export const storeSessionLog = (entry: SessionLogEntry) =>
  invokeBound<void>("store_session_log", { entry });

export const listSessionLogs = () =>
  invokeBound<SessionLogEntry[]>("list_session_logs");

export const archiveOldSessionLogs = (daysThreshold: number) =>
  invokeBound<number>("archive_old_session_logs", { daysThreshold });

export const pruneContextMemory = (maxItems: number) =>
  invokeBound<number>("prune_context_memory", { maxItems });

export const getSessionInsights = () =>
  invokeBound<string>("get_session_insights").then((raw) => JSON.parse(raw) as SessionInsights);

export const loadCustomJobs = () =>
  invokeBound<string>("load_custom_jobs").then((raw) => JSON.parse(raw));

export const saveCustomJobs = (data: string) =>
  invokeBound<void>("save_custom_jobs", { data });

export const saveSkill = (name: string, content: string) =>
  invokeBound<void>("save_skill", { name, content });

export const deleteSkill = (name: string) =>
  invokeBound<boolean>("delete_skill", { name });

// ============================================================
// System — Docker / Port checks
// ============================================================

export const checkDockerService = (name: string) =>
  invokeBound<string>("check_docker_service", { name });

export const restartDockerService = (name: string) =>
  invokeBound<string>("restart_docker_service", { name });

export const checkServicePort = (host: string, port: number) =>
  invokeBound<string>("check_service_port", { host, port });

// ============================================================
// Research
// ============================================================

export const webSearch = (query: string, maxResults?: number) =>
  invokeBound<import("./types").ResearchResult[]>("web_search", { query, maxResults });

export const fetchUrl = (url: string, maxChars?: number) =>
  invokeBound<string>("fetch_url", { url, maxChars });

// ============================================================
// Activity Log
// ============================================================

export const storeActivityEntry = (entry: import("./types").ActivityEntry) =>
  invokeBound<void>("store_activity_entry", { entry });

export const listActivityEntries = (limit?: number) =>
  invokeBound<import("./types").ActivityEntry[]>("list_activity_entries", { limit });

export const pruneActivityLog = (days: number) =>
  invokeBound<number>("prune_activity_log", { days });
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
  invokeBound<ActivityArchiveReport>("archive_activity_log", opts);

// ============================================================
// Subagents
// ============================================================

export const loadSubagents = () => invokeBound<string>("load_subagents");

export const saveSubagents = (data: string) =>
  invokeBound<void>("save_subagents", { data });

// ============================================================
// CrewAI
// ============================================================

export const crewList = () =>
  invokeBound<import("./types").CrewInfo[]>("crew_list");

export const crewStart = (name: string, task: string, agents?: string[]) =>
  invokeBound<import("./types").CrewInfo>("crew_start", { name, task, agents });

export const crewStatus = (id: string) =>
  invokeBound<import("./types").CrewInfo>("crew_status", { id });

export const crewStop = (id: string) =>
  invokeBound<string>("crew_stop", { id });

export const crewHealth = () =>
  invokeBound<string>("crew_health");
