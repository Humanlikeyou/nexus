// ============================================================
// Terrance / OpenClaw types
// ============================================================

export interface OpenClawStatus {
  online: boolean;
  model: string;
  gateway_url: string;
  last_heartbeat: string | null;
  heartbeat_data: Record<string, unknown> | null;
  config: Record<string, unknown> | null;
}

export interface OpenClawChannels {
  telegram: string; // "connected" | "disconnected" | "unknown"
  whatsapp: string; // "connected" | "not_linked" | "unknown"
}

export interface OpenClawHealth {
  status: "healthy" | "degraded" | "offline";
  version: string | null;
  gateway: boolean;
  rpc: boolean;
  memory_search: "configured" | "missing";
  channels: OpenClawChannels;
  timestamp: string;
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

// ============================================================
// Business / TaiyoSkin types
// ============================================================

export interface TaskItem {
  title: string;
  status: "active" | "backlog" | "waiting" | "done";
  priority: string | null;
  tags: string[];
}

export interface ContentPipelineItem {
  title: string;
  stage: "ideation" | "drafting" | "review" | "scheduled" | "published";
  content_type: string;
  notes: string | null;
}

// ============================================================
// Muse / Neurofeedback types
// ============================================================

export interface MuseDevice {
  name: string;
  address: string;
  rssi: number | null;
}

export interface EegSample {
  timestamp: number;
  channel: "TP9" | "AF7" | "AF8" | "TP10";
  samples: number[];
}

export interface MuseConnectionState {
  connected: boolean;
  device_name: string | null;
  streaming: boolean;
}

export type BleFlowState = "idle" | "scanning" | "discovered" | "connecting" | "connected" | "streaming";

export type DataSource = "live" | "replay" | "demo";

/** Structured BLE state payload from Rust — always { state, data_source } */
export interface BleStatePayload {
  state: BleFlowState;
  data_source: DataSource;
}

export type ReplayMode = "off" | "recording" | "replaying" | "demo";

export interface ReplayFileInfo {
  filename: string;
  size_bytes: number;
  modified: string;
  duration_secs: number | null;
}

export interface FrequencyBands {
  delta: number;
  theta: number;
  alpha: number;
  beta: number;
  gamma: number;
}

export interface MuseSession {
  id: string;
  start: string;
  end: string | null;
  duration_secs: number;
  avg_focus: number;
  avg_relaxation: number;
}

// ============================================================
// Whoop / Fitness types
// ============================================================

export interface WhoopDevice {
  name: string;
  address: string;
  rssi: number | null;
}

export interface HeartRateData {
  timestamp: number;
  heart_rate: number;
  rr_interval: number | null;
  source: "custom" | "standard";
}

export interface WhoopConnectionState {
  connected: boolean;
  device_name: string | null;
  streaming: boolean;
}

export interface WhoopSession {
  id: string;
  start: string;
  end: string | null;
  duration_secs: number;
  avg_hr: number;
  max_hr: number;
  avg_hrv: number | null;
}

// Whoop Analytics — ported from OpenWhoop algorithms
export interface StressScore {
  score: number;          // 0-10 (0 = very relaxed, 10 = max stress)
  mode: number;           // most frequent RR interval (ms)
  variationRange: number; // max - min RR (ms)
  sampleCount: number;    // how many HR samples used
  ready: boolean;         // true when enough data collected (≥120 samples)
}

export interface StrainMetrics {
  strain: number;         // 0-21 scale (TRIMP-style accumulation)
  maxHR: number;          // peak HR in session
  avgHR: number;          // mean HR in session
  durationSecs: number;   // total seconds of HR data
  hrZones: [number, number, number, number, number]; // seconds in zone 1-5
}

export interface RecoveryMetrics {
  score: number;          // 0-100 (67+ green, 34-66 yellow, <34 red)
  hrvBaseline: number;    // RMSSD used for score
  restingHR: number;      // lowest stable HR
  hrVariability: number;  // coefficient of variation
  ready: boolean;         // true when enough data for meaningful score
}

export interface SleepStage {
  name: "Awake" | "Light" | "REM" | "Deep (SWS)";
  hours: number;
  color: string;
}

export interface SleepMetrics {
  totalHours: number;
  score: number;          // 0-100
  stages: SleepStage[];
  avgHR: number;
  minHR: number;
  avgHRV: number;
  ready: boolean;         // true when sleep data available
}

// ============================================================
// Trajectory / Goals types
// ============================================================

export interface Goal {
  id: string;
  title: string;
  category: "revenue" | "health" | "project" | "personal";
  target: number;
  current: number;
  unit: string;
  deadline: string | null;
  created_at: string;
  updated_at: string;
}

export interface WellnessScore {
  overall: number;
  recovery: number | null;
  focus: number | null;
  productivity: number | null;
}

// ============================================================
// System types
// ============================================================

export interface SystemInfo {
  os: string;
  arch: string;
  hostname: string;
  uptime_secs: number;
  timestamp: number;
}

// ============================================================
// Telegram types
// ============================================================

export interface TelegramMessage {
  id: number;
  from_name: string;
  text: string;
  date: number;
  is_outgoing: boolean;
}

// ============================================================
// BLE Proof Mode
// ============================================================

export interface BleProofEvent {
  timestamp: number;
  device: "muse" | "whoop";
  event_type: string;
  detail: string;
}

// ============================================================
// Outbox / Gateway Pipeline
// ============================================================

export interface OutboxStatus {
  gateway_online: boolean;
  queue_depth: number;
  last_delivered_at: string | null;
  last_error: string | null;
  total_delivered: number;
}

export interface GatewayConfig {
  gateway_url: string | null;
  gateway_auth_token: string | null;
  gateway_batch_size: number | null;
  gateway_retry_base_secs: number | null;
  gateway_retry_max_secs: number | null;
}

// ============================================================
// Settings / Config types
// ============================================================

export interface NexusConfig {
  telegram_bot_token: string | null;
  telegram_chat_id: string | null;
  openclaw_path: string | null;
  tasks_path: string | null;
  pipeline_path: string | null;
  gateway_url: string | null;
  gateway_auth_token: string | null;
  gateway_batch_size: number | null;
  gateway_retry_base_secs: number | null;
  gateway_retry_max_secs: number | null;
  n8n_webhook_secret: string | null;
  preferred_muse_address: string | null;
  preferred_whoop_address: string | null;
  // LLM Router
  compute_mode: string | null;
  llm_ollama_model: string | null;
  llm_anthropic_model: string | null;
  llm_token_threshold: number | null;
  llm_daily_cap_usd: number | null;
  llm_per_task_cap_usd: number | null;
  llm_show_cost_estimate: boolean | null;
  anthropic_api_key: string | null;
}

// ============================================================
// LLM Router
// ============================================================

export type ComputeMode = "local" | "hybrid" | "api";

export interface LlmRouterConfig {
  mode: ComputeMode;
  ollama_model: string;
  anthropic_model: string;
  token_threshold: number;
  daily_cap_usd: number | null;
  per_task_cap_usd: number | null;
  show_cost_estimate: boolean;
}

export interface RouterResponse {
  content: string;
  provider: "ollama" | "anthropic";
  model: string;
  latency_ms: number;
  prompt_tokens: number;
  completion_tokens: number;
  estimated_cost_usd: number;
  routing_reason: string;
}

export interface LlmUsageRecord {
  id: string;
  timestamp: number;
  provider: "ollama" | "anthropic";
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  estimated_cost_usd: number;
  latency_ms: number;
  routing_reason: string;
}

export interface CostEstimate {
  provider: string;
  model: string;
  estimated_input_tokens: number;
  estimated_cost_usd: number;
  routing_reason: string;
}

export interface PackageInfo {
  path: string;
  size_mb: number;
  includes: string[];
}

// ============================================================
// Chat History (SQLite-persisted)
// ============================================================

export interface ChatSession {
  id: string;
  title: string;
  created_at: number;
  updated_at: number;
  message_count: number;
}

export interface PersistedChatMessage {
  id: number;
  session_id: string;
  role: "user" | "terrance";
  text: string;
  ts: number;
  provider?: string | null;
  model?: string | null;
  latency_ms?: number | null;
  cost_usd?: number | null;
}

// ============================================================
// Notifications
// ============================================================

export interface AppNotification {
  id: string;
  type: "info" | "success" | "warning" | "error";
  title: string;
  message: string;
  timestamp: number;
  read: boolean;
  source: string;
}

// ============================================================
// Toast
// ============================================================

export interface ToastOptions {
  type: "success" | "error" | "info" | "warning";
  title: string;
  description?: string;
  duration?: number;
  action?: { label: string; onClick: () => void };
}

export interface ToastItem extends ToastOptions {
  id: string;
  timestamp: number;
}

// ============================================================
// Command Palette
// ============================================================

export interface CommandItem {
  id: string;
  label: string;
  icon?: string;
  shortcut?: string;
  action: () => void;
  category: "Navigation" | "Actions" | "Devices";
}

// ============================================================
// EEG Display Config
// ============================================================

export interface EegDisplayConfig {
  /** Time window in seconds (1-30) */
  windowSecs: number;
  /** Target display FPS (1-30) */
  displayFps: number;
  /** Y-axis range in µV */
  yRange: number;
  /** Which channels to show */
  visibleChannels: EegChannel[];
  /** Auto-scale Y-axis to data */
  autoScale: boolean;
}

export type EegChannel = "TP9" | "AF7" | "AF8" | "TP10";

export const EEG_CHANNELS: EegChannel[] = ["TP9", "AF7", "AF8", "TP10"];

export const DEFAULT_DISPLAY_CONFIG: EegDisplayConfig = {
  windowSecs: 5,
  displayFps: 15,
  yRange: 200,
  visibleChannels: ["TP9", "AF7", "AF8", "TP10"],
  autoScale: true,
};

// ============================================================
// FFT / Frequency Analysis
// ============================================================

export interface FrequencyBandPowers {
  delta: number;
  theta: number;
  alpha: number;
  beta: number;
  gamma: number;
  smr: number;
  lowBeta: number;
  highBeta: number;
  alphaPeak: number;
  focusScore: number;
  relaxationScore: number;
  /** Alpha/Theta ratio — higher = more relaxed/meditative */
  alphaTheta: number;
  /** Beta/Alpha ratio — higher = more engaged/focused */
  betaAlpha: number;
}

// ============================================================
// Signal Quality (shared types)
// ============================================================

export type ChannelQuality = "good" | "fair" | "poor";
export type OverallQuality = "good" | "fair" | "poor" | "frozen";

// ============================================================
// Artifact Detection
// ============================================================

export type ArtifactType = "blink" | "jaw_clench" | "motion";

export interface ArtifactEvent {
  type: ArtifactType;
  timestamp: number;
  channel?: string;
  amplitude?: number;
}

// ============================================================
// Score Confidence
// ============================================================

export interface ScoreConfidenceState {
  confidence: number;
  displayFocus: number;
  displayRelax: number;
  frozen: boolean;
}

// ============================================================
// Simple Session Modes
// ============================================================

export type SessionModeId = "calm" | "focus" | "neutral";

export interface SessionMode {
  id: SessionModeId;
  label: string;
  description: string;
  icon: string;
  primaryMetric: "relaxationScore" | "focusScore" | "alphaTheta";
  defaultDurationMin: number;
}

export type SimpleSessionPhase = "idle" | "warmup" | "running" | "cooldown" | "complete";

export interface SessionTimelineEntry {
  timestamp: number;
  bands: FrequencyBandPowers;
  focusScore: number;
  relaxationScore: number;
  confidence: number;
  quality: OverallQuality;
}

export interface SimpleSessionSummary {
  mode: SessionModeId;
  durationSecs: number;
  avgPrimary: number;
  avgFocus: number;
  avgRelax: number;
  avgConfidence: number;
  timeline: SessionTimelineEntry[];
  startedAt: string;
  endedAt: string;
}

// ============================================================
// Electrode Pathway (Stage 1 Scaffold)
// ============================================================

export type DeviceSource = "muse" | "myndlift" | "replay" | "demo";

/**
 * Electrode operating mode.
 * "off"     — no stimulation, no read (default safe state)
 * "read"    — passive reading only (EEG acquisition)
 * "output"  — stimulation enabled (LOCKED OFF until Stage 2+ safety review)
 * "unknown" — device connected but mode not yet determined
 */
export type ElectrodeMode = "off" | "read" | "output" | "unknown";

export interface ElectrodeDeviceInfo {
  id: string;
  name: string;
  rssi?: number;
}

/**
 * Device telemetry — battery, firmware, hardware info.
 * Matches patterns from Muse control response (hn/sn/bp/fw fields)
 * and Whoop device metadata.
 */
export interface DeviceTelemetry {
  batteryPct: number | null;       // 0-100, null if unknown
  firmwareVersion: string | null;
  serialNumber: string | null;
  hardwareVersion: string | null;
  /** Muse channels: TP9, AF7, AF8, TP10, AUX. Others may differ. */
  channels: string[];
  sampleRate: number;              // Hz (Muse = 256)
}

export interface ElectrodeState {
  source: DeviceSource;
  connected: boolean;
  streaming: boolean;
  mode: ElectrodeMode;
  status: string;
  lastError?: string;
  telemetry: DeviceTelemetry | null;
}

// ============================================================
// Neurofeedback Output (Closed-Loop Feedback)
// ============================================================

export type FeedbackAction = "reward" | "inhibit" | "neutral";

export interface FeedbackSignal {
  action: FeedbackAction;
  intensity: number;      // 0-1
  gated: boolean;         // true = suppressed by safety gate
  gateReason?: string;    // why it was gated
  policyDetail?: string;  // which policy rule fired
}

export interface GateResult {
  allowed: boolean;
  holdState: boolean;     // true = keep last output, don't change
  reason: string;
}

export interface PolicyDecision {
  action: FeedbackAction;
  rewardIntensity: number;  // 0-1
  inhibitIntensity: number; // 0-1
  detail: string;           // human-readable rule that fired
}

// ============================================================
// Neurofeedback Protocol & Session types
// ============================================================

export interface NfProtocol {
  id: string;
  name: string;
  description: string;
  icon: string;
  rewardBands: Array<{ label: string; minHz: number; maxHz: number }>;
  inhibitBands: Array<{ label: string; minHz: number; maxHz: number }>;
  audioMode: "soundscape" | "tone" | "silent";
  eyesClosed: boolean;
  defaultDurationMin: number;
  category: "focus" | "calm" | "performance" | "relaxation";
  /** Bands that trigger suppression when drifting upward (soft inhibit on trend) */
  suppressOnDrift?: Array<{ label: string; minHz: number; maxHz: number }>;
  /** Coupling reward — reward ratio stability between two bands */
  couplingReward?: { bandA: string; bandB: string; rewardStability: boolean };
  /** Bands monitored but neither rewarded nor inhibited */
  passthroughBands?: string[];
  /** Per-protocol safety gate overrides (stricter thresholds) */
  gateOverrides?: { maxArtifactRate?: number; minConfidence?: number };
  /** Reward stability (low variance) rather than just magnitude */
  rewardStability?: boolean;
  /** Session prompt displayed during countdown / coaching cue */
  sessionPrompt?: string;
  /** Coaching cues shown at intervals during session */
  coachingCues?: string[];
  /** Recommended progression order (1-based) */
  progressionOrder?: number;
}

/** Training state output — tracks protocol progression maturity */
export type TrainingState = "warming" | "approaching" | "in_zone" | "sustaining" | "drifting";

/** Gamma artifact safeguard — gamma spikes above this are treated as muscle artifact */
export const GAMMA_ARTIFACT_THRESHOLD = 80; // µV² — typical gamma artifact floor

export type BrainState = "calm" | "neutral" | "active";

export interface StateTimelineEntry {
  timestamp: number;
  state: BrainState;
  inZone: boolean;
  score: number;
}

export type SessionPhase =
  | "idle"
  | "selecting"
  | "calibrating"
  | "countdown"
  | "running"
  | "paused"
  | "complete";

export interface BrainSnapshot {
  id: string;
  timestamp: string;
  fatigue: number;
  engagement: number;
  alphaPeak: number;
  betaBalance: number;
  focusScore: number;
  relaxationScore: number;
}

// ============================================================
// Session Recording
// ============================================================

export interface RecordedSession {
  id: string;
  device_type: "muse" | "whoop";
  start: string;
  end: string;
  duration_secs: number;
  data_points: number;
  file_path: string;
  summary: {
    avg_focus?: number;
    avg_relaxation?: number;
    avg_hr?: number;
    max_hr?: number;
    avg_hrv?: number;
  };
}

export interface NfSession extends RecordedSession {
  protocol_id: string;
  protocol_name: string;
  total_score: number;
  time_in_zone_pct: number;
  state_timeline: StateTimelineEntry[];
  avg_alpha_peak: number | null;
  band_averages: Record<string, number>;
}

// ============================================================
// Canonical Event Store
// ============================================================

export interface CanonicalEvent {
  id?: string;
  session_id?: string;
  timestamp: string;
  source: string;
  metric: string;
  value: number;
  unit?: string;
  confidence?: number;
  tags?: string[];
  raw_ref?: string;
  created_at?: string;
  schema_version?: number;
}

export interface EventQuery {
  source?: string;
  metric?: string;
  session_id?: string;
  from?: string;
  to?: string;
  limit?: number;
}

// ============================================================
// Navigation
// ============================================================

export type PageId =
  | "terrance"
  | "business"
  | "assistant"
  | "neuro"
  | "fitness"
  | "trajectory"
  | "revenue"
  | "settings";

// ============================================================
// ARM / Autonomous Revenue Module
// ============================================================

export type ArmStrategyType =
  | "api_service"
  | "content_syndication"
  | "data_packaging"
  | "saas_micro"
  | "affiliate";

export type ArmStrategyStatus = "draft" | "testing" | "active" | "paused" | "killed";

export type ArmHealthStatus = "healthy" | "degraded" | "down" | "unknown";

export interface ArmStrategyConfig {
  worker_url: string;
  health_endpoint: string;
  revenue_endpoint: string;
  pause_endpoint?: string;
  kill_endpoint?: string;
  auth_key?: string;
  pricing?: Record<string, number>;
  limits?: Record<string, number>;
  metadata?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface ArmStrategy {
  id: string;
  name: string;
  type: ArmStrategyType;
  status: ArmStrategyStatus;
  config: ArmStrategyConfig;
  capital_limit: number;
  daily_loss_cap: number;
  total_revenue: number;
  total_expenses: number;
  last_health: string | null;
  health_status: ArmHealthStatus;
  created_at: string;
  updated_at: string;
}

export type ArmDecisionAction =
  | "activate"
  | "pause"
  | "kill"
  | "adjust_limit"
  | "deploy"
  | "scale"
  | "rollback";

export interface ArmDecision {
  id: string;
  strategy_id: string;
  action: ArmDecisionAction;
  reasoning: string;
  context: Record<string, unknown>;
  outcome: "positive" | "negative" | "neutral" | null;
  outcome_notes: string | null;
  timestamp: string;
}

export interface ArmRiskStatus {
  global_capital_deployed: number;
  global_capital_limit: number;
  global_daily_pnl: number;
  global_daily_loss_cap: number;
  kill_switch_active: boolean;
  active_strategies: number;
  strategies_at_risk: string[];
  risk_index: number;
}

export interface ArmRevenueSummary {
  total_revenue: number;
  total_expenses: number;
  net_income: number;
  today_revenue: number;
  today_expenses: number;
  week_revenue: number;
  month_revenue: number;
  by_strategy: ArmStrategyRevenue[];
}

export interface ArmStrategyRevenue {
  strategy_id: string;
  strategy_name: string;
  revenue: number;
  expenses: number;
  net: number;
}

export interface ArmConfig {
  wallet_address: string | null;
  global_capital_limit: number;
  global_daily_loss_cap: number;
  poll_interval_secs: number;
  kill_switch_active: boolean;
  worker_api_keys: Array<{ name: string; key: string }>;
}

// ============================================================
// Terrance Continuity
// ============================================================

export interface TerranceContext {
  activeGoals: string[];
  lastSessionSummary: string | null;
  lastSessionTimestamp: number | null;
  relevantMemory: string[];
  lastConsolidationTimestamp: number | null;
  lastReasoningTimestamp: number | null;
}

export interface SessionLogEntry {
  id: string;
  timestamp: number;
  messageCount: number;
  summary: string;
  provider: string;
  model: string;
}
