import type { CrewToolInfo, KbStatus } from "../src/lib/types";
export type { CrewToolInfo, KbState, KbStatus } from "../src/lib/types";

export type ModuleId = "landing" | "terrance" | "chat" | "health" | "neuro" | "sessions" | "devices" | "business" | "revenue" | "assistant" | "crews" | "trajectory" | "settings" | "voice" | "lists" | "mindbody" | "smarthome";

export interface HermesRuntimeHealth {
  runtime: string;
  status: string;
  model: string;
  provider: string;
  gatewayRunning: boolean;
  telegramStatus: string;
  activeSchedules: number;
  configuredToolsets: string[];
  checkedAt: string;
  detail: string | null;
  /** Hermes's model.base_url is the Amara Router, so Fast/Deep can be known. */
  routedViaRouter?: boolean;
}

/** Fast/Deep as the Amara Router decided it (router names). Absent = unknown, no badge. */
export interface AmaraTierInfo {
  tier: "fast" | "deep" | "claude";
  reason: string;
  /** hermes_metadata | router_log | router_dry_run | router_reply */
  source: string;
}

export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
}

export interface AmaraContextSection {
  tag: string;
  text: string;
}

export interface AmaraTurnContext {
  sections: AmaraContextSection[];
  itemIds: string[];
}

export interface TerrenceCapability {
  name: string;
  status: string;
  description: string;
}

export interface TerrenceInteraction {
  id: string;
  conversationId: string;
  hermesSessionId: string | null;
  runtime: string;
  model: string;
  provider: string;
  status: string;
  userMessage: string;
  assistantMessage: string | null;
  error: string | null;
  startedAt: string;
  completedAt: string;
  durationMs: number;
  contextItemIds: string[];
  actionId: string | null;
  runtimeProfile: string;
  tier?: AmaraTierInfo | null;
  /** Who answered, e.g. "home" (Home Assistant). Absent when unset. */
  source?: string;
}

export interface TerrenceTool {
  id: string;
  label: string;
  description: string;
  category: string;
  requiresApproval: boolean;
  parameters: Array<{ name: string; description: string; required: boolean }>;
}

export interface TerrenceAction {
  id: string;
  toolId: string;
  title: string;
  status: "requested" | "planning" | "awaiting_approval" | "approved" | "executing" | "succeeded" | "failed" | "cancelled" | "needs_attention";
  requiresApproval: boolean;
  parameters: Record<string, unknown>;
  conversationId: string | null;
  contextItemIds: string[];
  requestedAt: string;
  updatedAt: string;
  approvedAt: string | null;
  startedAt: string | null;
  completedAt: string | null;
  result: string | null;
  error: string | null;
  retryOfActionId: string | null;
  retryAllowed: boolean;
  summary: string;
}

export interface TerrenceContextItem {
  id: string;
  kind: "identity" | "preference" | "goal" | "project" | "commitment" | "observation" | "inference" | "entity" | "episode" | "pattern";
  title: string;
  content: string | null;
  status: "active" | "blocked" | "paused" | "completed" | "archived" | "historical" | "superseded";
  confidence: number;
  provenanceSource: string;
  provenanceRef: string | null;
  createdAt: string;
  updatedAt: string;
  occurredAt: string | null;
  supersedesId: string | null;
  deletedAt: string | null;
  metadata: Record<string, unknown>;
}

export interface TerrenceContextItemQuery {
  kinds?: TerrenceContextItem["kind"][];
  statuses?: TerrenceContextItem["status"][];
  includeDeleted?: boolean;
  limit?: number;
}

export interface TerrenceContextItemReplacement {
  kind: TerrenceContextItem["kind"];
  title: string;
  content: string | null;
  status: TerrenceContextItem["status"];
  confidence: number;
  provenanceSource: string;
  provenanceRef: string | null;
  occurredAt: string | null;
  metadata: Record<string, unknown>;
}

export interface CorrectTerrenceContextItemInput {
  originalId: string;
  replacement: TerrenceContextItemReplacement;
}

export interface TerrenceContextSnapshot {
  generatedAt: string;
  priorities: TerrenceContextItem[];
  activeProjects: TerrenceContextItem[];
  commitments: TerrenceContextItem[];
  recentObservations: TerrenceContextItem[];
}

export interface TerrencePriority {
  rank: number;
  sourceKind: "context" | "action" | "project";
  sourceId: string;
  title: string;
  sourceStatus: string;
  score: number;
  confidence: number;
  reasons: string[];
  contextItemIds: string[];
  actionId: string | null;
}

export interface TerrencePrioritySnapshot {
  id: string;
  generatedAt: string;
  items: TerrencePriority[];
}

export interface TerrenceDecisionEvidence {
  detail: string;
  source: string | null;
  confidence: number | null;
}

export interface TerrenceDecisionAlternative {
  label: string;
  tradeoffs: string | null;
}

export interface TerrenceDecision {
  id: string;
  question: string;
  status: "open" | "decided" | "observing" | "closed" | "cancelled";
  constraints: string[];
  evidence: TerrenceDecisionEvidence[];
  alternatives: TerrenceDecisionAlternative[];
  recommendation: string;
  uncertainty: string | null;
  decision: string | null;
  outcome: string | null;
  conversationId: string | null;
  contextItemIds: string[];
  provenanceSource: string;
  createdAt: string;
  updatedAt: string;
  decidedAt: string | null;
  outcomeRecordedAt: string | null;
}

export interface TerrenceAutomation {
  id: string;
  source: string;
  sourceId: string;
  name: string;
  trigger: string;
  enabled: boolean;
  status: "active" | "paused" | "attention";
  nextRunAt: string | null;
  lastRunAt: string | null;
  lastStatus: string | null;
  lastError: string | null;
  updatedAt: string;
}

export interface TerrenceAutomationRun {
  id: string;
  automationId: string;
  status: "succeeded" | "failed" | "suppressed";
  triggeredAt: string;
  completedAt: string;
  result: string | null;
  error: string | null;
  deliveryStatus: "delivered" | "failed" | "suppressed" | "not_requested";
  deliveryTarget: string | null;
  provenanceSource: string;
}

export interface TerrenceProjectMilestone {
  title: string;
  status: "pending" | "active" | "blocked" | "completed";
  dueAt: string | null;
}

export interface TerrenceProject {
  id: string;
  title: string;
  objective: string;
  currentState: string;
  status: "active" | "blocked" | "paused" | "completed" | "archived";
  milestones: TerrenceProjectMilestone[];
  nextAction: string | null;
  blockers: string[];
  dependencies: string[];
  contextItemIds: string[];
  provenanceSource: string;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface CronJob {
  name: string;
  schedule: string;
  next_run: string | null;
  last_run: string | null;
  enabled: boolean;
}

export interface Skill {
  name: string;
  status: string;
  description: string;
}

export interface TaskItem {
  title: string;
  status: string;
  priority: string | null;
  tags: string[];
}

export interface PipelineItem {
  title: string;
  stage: string;
  content_type: string;
  notes: string | null;
}

export interface MuseState {
  connected: boolean;
  device_name: string | null;
  streaming: boolean;
}

export interface MuseDevice {
  name: string;
  address: string;
  rssi: number | null;
}

export interface EegSample {
  timestamp: number;
  channel: string;
  samples: number[];
}

export interface RecordedSession {
  id: string;
  device_type: string;
  start: string;
  end: string;
  duration_secs: number;
  data_points: number;
  file_path?: string;
  summary: {
    avg_focus?: number | null;
    avg_relaxation?: number | null;
    avg_hr?: number | null;
    max_hr?: number | null;
    avg_hrv?: number | null;
    /** Muse training only; absent on sessions saved before 2026-10-28 (show "—"). */
    calm_pct?: number | null;
    time_in_target_pct?: number | null;
  };
  /** Minimised NOOP context captured at Muse session start; absent on older sessions. Parse with parseBodyContext. */
  body_context?: import("../src/lib/types").BodyContext | null;
}

export interface NoopDailyMetric {
  day: string;
  source: string;
  totalSleepMin: number | null;
  efficiency: number | null;
  deepMin: number | null;
  remMin: number | null;
  lightMin: number | null;
  restingHr: number | null;
  avgHrv: number | null;
  recovery: number | null;
  strain: number | null;
  spo2Pct: number | null;
  skinTempDevC: number | null;
  respRateBpm: number | null;
}

export interface NoopHealthInsight {
  metric: string;
  fact: string;
  context: string | null;
}

export interface NoopSleepSession {
  day: string;
  startedAt: string;
  endedAt: string;
  durationMin: number;
  efficiency: number | null;
  deepMin: number | null;
  remMin: number | null;
  lightMin: number | null;
  awakeMin: number | null;
  sessionCount: number;
  restingHr: number | null;
  avgHrv: number | null;
}

export interface NoopHealthSnapshot {
  source: "noop";
  status: "ready" | "stale" | "waiting_for_data" | "waiting_for_nightly_data" | "unavailable";
  generatedAt: string;
  deviceId: string;
  latestDay: string | null;
  dataAgeDays: number | null;
  today: NoopDailyMetric | null;
  recentDays: NoopDailyMetric[];
  latestSleep: NoopSleepSession | null;
  latestNight: NoopSleepSession | null;
  battery: { percentage: number; charging: boolean | null; observedAt: string } | null;
  insights: NoopHealthInsight[];
  message: string | null;
}

export interface Goal {
  id: string;
  title: string;
  category: string;
  target: number;
  current: number;
  unit: string;
  deadline: string | null;
}

export interface RevenueSummary {
  total_revenue: number;
  total_expenses: number;
  net_income: number;
  today_revenue: number;
  today_expenses: number;
  week_revenue: number;
  month_revenue: number;
  by_strategy: Array<{ strategy_id: string; strategy_name: string; revenue: number; expenses: number; net: number }>;
}

export interface Strategy {
  id: string;
  name: string;
  type: string;
  status: string;
  capital_limit: number;
  daily_loss_cap: number;
  total_revenue: number;
  total_expenses: number;
  last_health: string | null;
  health_status: string;
}

export interface RiskStatus {
  global_capital_deployed: number;
  global_capital_limit: number;
  global_daily_pnl: number;
  global_daily_loss_cap: number;
  kill_switch_active: boolean;
  active_strategies: number;
  strategies_at_risk: string[];
  risk_index: number;
}

export interface OutboxStatus {
  gateway_online: boolean;
  queue_depth: number;
  last_delivered_at: string | null;
  last_error: string | null;
  total_delivered: number;
  /** Items older than 24h: kept on disk, never sent. Older backends omit it. */
  expired?: number;
}

export interface ActivityEntry {
  id: string;
  type: string;
  name: string;
  status: string;
  startedAt: number;
  completedAt: number;
  durationMs: number;
  detail: string | null;
  outcome: string | null;
}

export interface SystemSnapshot {
  refreshedAt: number;
  unavailableSources: NexusUnavailableSource[];
  hermes: HermesRuntimeHealth;
  capabilities: TerrenceCapability[];
  context: TerrenceContextSnapshot;
  prioritySnapshot: TerrencePrioritySnapshot;
  decisions: TerrenceDecision[];
  actions: TerrenceAction[];
  automations: TerrenceAutomation[];
  automationRuns: TerrenceAutomationRun[];
  projects: TerrenceProject[];
  noop: NoopHealthSnapshot;
  muse: MuseState;
  sessions: RecordedSession[];
  tasks: TaskItem[];
  pipeline: PipelineItem[];
  revenue: RevenueSummary;
  strategies: Strategy[];
  risk: RiskStatus;
  goals: Goal[];
  outbox: OutboxStatus;
  activity: ActivityEntry[];
  /** CrewAI service (stack/crew-service via crew.rs). */
  crew: CrewSnapshot;
  /** Knowledge base (Verba + Weaviate) via kb_status. */
  kb: KbStatus;
  /** Business ledger (business/ledger.json, decision 0005); null when there is no file. */
  ledger: import("./business/liveLedger").BusinessLedger | null;
}

/** crew-service run states (crew_runs.status). */
export type CrewRunStatus = "running" | "completed" | "failed" | "stopped" | "interrupted";

/** One crew run (crew.rs CrewInfo). For failed runs `result` holds the sanitized error. */
export interface CrewInfo {
  id: string;
  name: string;
  status: CrewRunStatus | string;
  agents: string[];
  started_at: string | null;
  task: string | null;
  result: string | null;
  /** R13-B totals; absent on older rows. cost_usd is 0 for local runs and null when unknown. */
  finished_at?: string | null;
  tokens?: number | null;
  requests?: number | null;
  cost_usd?: number | null;
}

/** One timeline step of a run (crew.rs CrewStepRow); times are ms since the run's start. */
export interface CrewStepRow { id: number; agent: string; kind: "task" | "tool" | "llm" | string; label: string; start_ms: number; end_ms: number; status: "ok" | "failed" | "skipped" | string; error?: string | null }
export interface CrewHandoffRow { from: string; to: string; status: string }
export interface CrewRunSteps { steps: CrewStepRow[]; handoffs: CrewHandoffRow[] }

/** not_configured: no token in env or the app data dir; offline: unreachable or unhealthy. */
export type CrewServiceHealth = "not_configured" | "offline" | "online";

export interface CrewSnapshot {
  health: CrewServiceHealth;
  runs: CrewInfo[];
  /** crew_tools: the allowlist state of each known tool; [] = none enabled. */
  tools: CrewToolInfo[];
}

export interface NexusUnavailableSource {
  command: string;
  label: string;
}

/** Amara Voice (~/projects/amara-voice), via commands/voice.rs. */
export type VoiceState = "loading" | "idle" | "listening" | "heard" | "thinking" | "speaking" | "error" | "stopped" | "not_installed";

export interface VoiceStatus {
  installed: boolean;
  running: boolean;
  pid: number | null;
  state: VoiceState;
  /** Seconds since logs/state last changed. */
  state_age_s: number | null;
  voice: string;
  rss_mb: number | null;
  /** Installed as a LaunchAgent: start/stop go through `amara start|stop`. */
  launchd: boolean;
  dir: string;
}

/** One line of logs/turns.jsonl (amara_voice.log_turn). Fields are optional: older lines have fewer. */
export interface VoiceTurn {
  time?: string;
  heard?: string;
  text?: string;
  /** hermes = Amara answered; local = skills.py; repeat = "Sorry, say that again?"; ignored = not acted on. */
  route?: "hermes" | "local" | "repeat" | "ignored" | string;
  reply?: string;
  ignored?: string;
  verdict?: "ok" | "low" | "noise" | string;
  asr_s?: number | null;
  token_s?: number | null;
  first_audio_s?: number | null;
  total_s?: number | null;
  barged?: "name" | "stop" | null;
  follow_up?: boolean;
  error?: string | null;
}

export interface VoiceList {
  voices: string[];
  current: string;
}

/* ── NOOP query: contract types live in src/lib/types.ts (Integrations); re-exported, not duplicated. ── */
export type { NoopMetricKey, NoopQueryArgs, NoopQueryError, NoopQueryTool, NoopSettings, WhoopSource } from "../src/lib/types";
export { NOOP_QUERY_TOOLS } from "../src/lib/types";

export type { BodyContext } from "../src/lib/types";

/** UI-side body state, parsed from a session's body_context (see lib/noop.ts). */
export interface BodyState {
  captured_at: string | null;
  recovery: number | null;
  hrv_ms: number | null;
  resting_hr: number | null;
  sleep_minutes: number | null;
}

/* Real noop_query payloads (noop-shapes-1104; keys always present, values nullable). */
export interface NoopDayRow {
  day: string; source: string;
  totalSleepMin: number | null; efficiency: number | null; deepMin: number | null; remMin: number | null; lightMin: number | null;
  disturbances: number | null; restingHr: number | null; avgHrv: number | null; recovery: number | null; strain: number | null;
  exerciseCount: number | null; spo2Pct: number | null; skinTempDevC: number | null; respRateBpm: number | null;
  steps: number | null; activeKcalEst: number | null;
}
export interface NoopLatestNight {
  day: string; startTs: number; endTs: number; start: string; end: string; sessionCount: number;
  totalSleepMin: number; inBedMin: number; efficiency: number; deepMin: number; remMin: number; lightMin: number;
}
/** health_snapshot payload (also RecordedSession.body_context). Only the fields the UI reads are typed strictly. */
export interface NoopSnapshotPayload {
  generatedAt: string;
  logicalToday: string;
  sources: Record<string, string | null>;
  freshness: { latestHeartRateSample: { ts: number; iso: string; ageSeconds: number } | null; latestDailyMetricDay: string | null; latestAppleHealthDay: string | null; dailyRows: number; appleDailyRows: number };
  today?: NoopDayRow | null;
  recentDays: NoopDayRow[];
  appleDaily: unknown[];
  latestSleep?: unknown | null;
  latestNight?: NoopLatestNight | null;
  battery?: unknown | null;
}
export interface NoopSeriesPayload {
  key: import("../src/lib/types").NoopMetricKey;
  requestedSource: string;
  range: { from: string; to: string };
  resolution: { candidates: Array<{ source: string; key: string }>; usedSources: string[] };
  returned: number;
  points: Array<{ day: string; key: string; value: number; source: string; sourceKey: string }>;
}

/** UI-side metric point. */
export interface NoopMetricPoint { day: string; value: number | null }
export interface NoopMetricSeries { key: import("../src/lib/types").NoopMetricKey; points: NoopMetricPoint[] }

/** Amara's follow-up lists (commands/followups.rs). Dates are "YYYY-MM-DD" when the app wrote them. */
export interface FollowupItem {
  id: number;
  text: string;
  added: string | null;
  due: string | null;
  next: string | null;
  notes: string[];
  lastNudged: string | null;
  done: string | null;
}

export interface MutedTopic {
  topic: string;
  added: string | null;
  until: string | null;
  /** False once `until` has passed: Amara no longer treats it as muted. */
  active: boolean;
}

export interface FollowupLists {
  open: FollowupItem[];
  /** Closed in the last 30 days, newest first. */
  recentlyDone: FollowupItem[];
  muted: MutedTopic[];
}

export interface FollowupItemInput {
  text: string;
  due?: string;
  next?: string;
}

/** Absent stays as it is; `null` or "" clears `due` and `next`. */
export interface FollowupItemPatch {
  text?: string;
  due?: string | null;
  next?: string | null;
}
