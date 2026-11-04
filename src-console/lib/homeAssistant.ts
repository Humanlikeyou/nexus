import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Home Assistant bridge for the console. Types mirror the camelCase structs in
 * src-tauri/src/commands/ha.rs (signatures confirmed by Rust Engineer, 2026-11-04).
 * The HA token never reaches the webview: every call goes through Rust.
 *
 * SWAP POINT: `tauriHaBackend` is the only place that names commands/events. If Rust
 * renames anything, change it here; tests and e2e swap in `createMockHaBackend()`.
 */

export interface HaEntity {
  entityId: string;
  state: string;
  friendlyName?: string | null;
  domain: string;
  lastChanged?: string | null;
  attributes: Record<string, unknown>;
}
export type HaConversationResult = { kind: "match"; speech: string } | { kind: "noMatch" };
/** Only for climate.set_temperature ({temperature}) and climate.set_hvac_mode ({hvacMode}); Rust rejects unknown fields. */
export interface HaServiceData { temperature?: number; hvacMode?: string }
export interface HaContainerStatus { running: boolean; status: string }
export interface HaStateChanged { entityId: string; newState: HaEntity | null; oldState: HaEntity | null; timeFired: string | null }
export type HaLiveState = "connecting" | "connected" | "authFailed" | "disconnected" | "stopped";

export interface HaBackend {
  states(): Promise<HaEntity[]>;
  callService(domain: string, service: string, entityId: string, data?: HaServiceData): Promise<void>;
  conversation(text: string): Promise<HaConversationResult>;
  containerStatus(): Promise<HaContainerStatus>;
  containerStart(): Promise<HaContainerStatus>;
  containerStop(): Promise<HaContainerStatus>;
  liveStart(): Promise<boolean>;
  liveStop(): Promise<boolean>;
  onStateChanged(handler: (change: HaStateChanged) => void): Promise<UnlistenFn>;
  onLiveStatus(handler: (state: HaLiveState) => void): Promise<UnlistenFn>;
}

export const HA_NAME_RE = /^[a-z0-9_]{1,64}$/;
export const HA_ENTITY_RE = /^[a-z0-9_]{1,64}\.[a-z0-9_]{1,128}$/;
export const HA_MAX_TEXT = 1000;
/** Client-side mirror of Rust's validation; Rust checks again. */
export const validServiceCall = (domain: string, service: string, entityId: string) =>
  HA_NAME_RE.test(domain) && HA_NAME_RE.test(service) && HA_ENTITY_RE.test(entityId);
export const HA_INVALID = "invalid_input: Invalid domain, service or entity id.";

export const HA_BAD_DATA = "invalid_input: Invalid or missing data for this Home Assistant service.";
export const HA_TEMP_MIN = 5;
export const HA_TEMP_MAX = 95;
/** Mirrors ha.rs's allowed hvac modes. */
export const HA_HVAC_MODES = ["off", "heat", "cool", "heat_cool", "auto", "dry", "fan_only"] as const;
export type HaHvacMode = (typeof HA_HVAC_MODES)[number];
export const isHvacMode = (v: unknown): v is HaHvacMode => typeof v === "string" && (HA_HVAC_MODES as readonly string[]).includes(v);
export const validTemperature = (t: unknown): t is number => typeof t === "number" && Number.isFinite(t) && t >= HA_TEMP_MIN && t <= HA_TEMP_MAX;

/** Client-side mirror of ha.rs: data only for the two climate services, exactly one relevant field. */
export function validServiceData(domain: string, service: string, data: HaServiceData | undefined): boolean {
  const keys = data ? Object.keys(data) : [];
  if (domain === "climate" && service === "set_temperature") return keys.length === 1 && keys[0] === "temperature" && validTemperature(data!.temperature);
  if (domain === "climate" && service === "set_hvac_mode") return keys.length === 1 && keys[0] === "hvacMode" && isHvacMode(data!.hvacMode);
  return data === undefined;
}

/** Modes offered for an entity: ha.rs's list ∩ attributes.hvac_modes when HA lists them. */
export function hvacModesFor(e: HaEntity): HaHvacMode[] {
  const listed = e.attributes.hvac_modes;
  return Array.isArray(listed) ? HA_HVAC_MODES.filter((m) => listed.includes(m)) : [...HA_HVAC_MODES];
}

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : null);
/** Setpoint range and step for a climate entity, always inside 5–95. */
export function climateRange(e: HaEntity): { min: number; max: number; step: number } {
  const min = Math.max(HA_TEMP_MIN, num(e.attributes.min_temp) ?? HA_TEMP_MIN);
  const max = Math.min(HA_TEMP_MAX, num(e.attributes.max_temp) ?? HA_TEMP_MAX);
  const step = num(e.attributes.target_temp_step) ?? 0.5;
  return min <= max ? { min, max, step: step > 0 ? step : 0.5 } : { min: HA_TEMP_MIN, max: HA_TEMP_MAX, step: 0.5 };
}
export const clampTemperature = (t: number, range: { min: number; max: number }) => Math.min(range.max, Math.max(range.min, Math.round(t * 10) / 10));
export const targetTemperature = (e: HaEntity) => num(e.attributes.temperature);
export const HVAC_LABEL: Record<HaHvacMode, string> = { off: "Off", heat: "Heat", cool: "Cool", heat_cool: "Heat and cool", auto: "Auto", dry: "Dry", fan_only: "Fan only" };

export const HA_STATE_CHANGED_EVENT = "ha://state_changed";
export const HA_LIVE_STATUS_EVENT = "ha://live_status";

export const tauriHaBackend: HaBackend = {
  states: () => invoke<HaEntity[]>("ha_states"),
  callService: (domain, service, entityId, data) => {
    if (!validServiceCall(domain, service, entityId)) return Promise.reject(HA_INVALID);
    if (!validServiceData(domain, service, data)) return Promise.reject(HA_BAD_DATA);
    return invoke<void>("ha_call_service", data ? { domain, service, entityId, data } : { domain, service, entityId });
  },
  conversation: (text) => invoke<HaConversationResult>("ha_conversation", { text }),
  containerStatus: () => invoke<HaContainerStatus>("ha_container_status"),
  containerStart: () => invoke<HaContainerStatus>("ha_container_start"),
  containerStop: () => invoke<HaContainerStatus>("ha_container_stop"),
  liveStart: () => invoke<boolean>("ha_live_start"),
  liveStop: () => invoke<boolean>("ha_live_stop"),
  onStateChanged: (handler) => listen<HaStateChanged>(HA_STATE_CHANGED_EVENT, (e) => handler(e.payload)),
  onLiveStatus: (handler) => listen<{ state: HaLiveState }>(HA_LIVE_STATUS_EVENT, (e) => handler(e.payload.state)),
};

let backend: HaBackend = tauriHaBackend;
export const haBackend = (): HaBackend => backend;
/** Tests only. Returns a restore function. */
export function setHaBackend(next: HaBackend): () => void { const prev = backend; backend = next; return () => { backend = prev; }; }

// ─── error classification ────────────────────────────────────────────────────
/**
 * Rust returns "<kind>: <message>". The message is never shown; only fixed copy per kind.
 * MUST stay in sync with ERROR_KINDS in src-tauri/src/commands/ha.rs.
 */
export const HA_ERROR_KINDS = [
  "not_configured", "bad_url", "auth_rejected", "unreachable", "docker_unavailable", "already_running", "timed_out",
  "too_large", "not_allowed", "needs_approval", "declined", "domain_mismatch", "invalid_input", "upstream_error",
] as const;
export type HaErrorKind = (typeof HA_ERROR_KINDS)[number] | "backend_missing" | "unknown";

export const HA_ERROR_COPY: Record<HaErrorKind, string> = {
  not_configured: "Home Assistant isn't set up.",
  bad_url: "Home Assistant's address must be on this Mac (127.0.0.1).",
  auth_rejected: "Home Assistant rejected the token.",
  unreachable: "Home Assistant isn't reachable.",
  docker_unavailable: "Docker isn't available. Open Docker Desktop, then try again.",
  already_running: "A container action is already running.",
  timed_out: "That took too long. Try again.",
  too_large: "That request was too large.",
  not_allowed: "Not allowed from Nexus without confirmation.",
  needs_approval: "Not allowed from Nexus without confirmation.",
  declined: "Not changed.",
  domain_mismatch: "That service doesn't match this device.",
  invalid_input: "That value isn't valid for this device.",
  upstream_error: "Home Assistant returned an error.",
  backend_missing: "Home Assistant support isn't in this build.",
  unknown: "Something went wrong with Home Assistant.",
};

const KIND_SET = new Set<string>(HA_ERROR_KINDS);

export function classifyHaError(error: unknown): HaErrorKind {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  const prefix = /^([a-z_]+):/.exec(text)?.[1];
  if (prefix && KIND_SET.has(prefix)) return prefix as HaErrorKind;
  // Primary path is the kind prefix above. Substrings are only a fallback for un-prefixed text.
  if (/not configured/i.test(text)) return "not_configured";
  if (/HA_URL must be/i.test(text)) return "bad_url";
  if (/rejected the token/i.test(text)) return "auth_rejected";
  if (/not reachable/i.test(text)) return "unreachable";
  if (/Docker is not available/i.test(text)) return "docker_unavailable";
  if (/already running/i.test(text)) return "already_running";
  if (/timed out/i.test(text)) return "timed_out";
  if (/empty or too long/i.test(text)) return "too_large";
  if (/Invalid domain, service or entity id/i.test(text)) return "invalid_input";
  if (/returned an error|unexpected reply|container command failed/i.test(text)) return "upstream_error";
  if (/command .*not found|unknown command|not mocked/i.test(text)) return "backend_missing";
  return "unknown";
}

export const haErrorCopy = (error: unknown) => HA_ERROR_COPY[classifyHaError(error)];
/** Allowlist refusals (not allowed / needs native approval / user declined the native confirm). */
export const isServiceRefusal = (kind: HaErrorKind) => kind === "not_allowed" || kind === "needs_approval" || kind === "declined";

export type HaInactiveKind = "not_configured" | "auth" | "unreachable" | "backend_missing";
/** Which setup screen a states() failure should show; null means a plain retry state. */
export function inactiveKindFor(kind: HaErrorKind): HaInactiveKind | null {
  if (kind === "not_configured" || kind === "bad_url") return "not_configured";
  if (kind === "auth_rejected") return "auth";
  if (kind === "unreachable") return "unreachable";
  if (kind === "backend_missing") return "backend_missing";
  return null;
}

export function containerErrorCopy(error: unknown): string {
  const kind = classifyHaError(error);
  return kind === "docker_unavailable" || kind === "already_running" || kind === "timed_out" ? HA_ERROR_COPY[kind] : "The container command failed.";
}

// ─── entities ────────────────────────────────────────────────────────────────
/** Domains with a simple on/off switch, and the service domain that toggles them. */
const TOGGLE_DOMAINS = new Set(["light", "switch", "fan", "input_boolean", "automation"]);

export const isToggleable = (e: HaEntity) => TOGGLE_DOMAINS.has(e.domain) && (e.state === "on" || e.state === "off");
export const entityName = (e: HaEntity) => e.friendlyName?.trim() || e.entityId;
export const toggleService = (on: boolean) => (on ? "turn_on" : "turn_off");

/** Hidden: HA's own bookkeeping entities, which aren't devices. */
const HIDDEN_DOMAINS = new Set(["sun", "zone", "person", "update", "conversation", "tts", "stt", "wake_word", "event", "todo", "weather"]);
export const isDevice = (e: HaEntity) => !HIDDEN_DOMAINS.has(e.domain);

export function sortDevices(list: HaEntity[]): HaEntity[] {
  return list.filter(isDevice).sort((a, b) => a.domain.localeCompare(b.domain) || entityName(a).localeCompare(entityName(b)));
}

/** Applies one live state_changed event; a null newState removes the entity. */
export function applyStateChange(list: HaEntity[], change: HaStateChanged): HaEntity[] {
  const rest = list.filter((e) => e.entityId !== change.entityId);
  return change.newState ? sortDevices([...rest, change.newState]) : rest;
}

const DOMAIN_LABEL: Record<string, string> = { light: "Light", switch: "Switch", fan: "Fan", input_boolean: "Helper", automation: "Automation", sensor: "Sensor", binary_sensor: "Sensor", climate: "Climate", media_player: "Media", cover: "Cover", lock: "Lock", scene: "Scene", script: "Script" };
export const domainLabel = (d: string) => DOMAIN_LABEL[d] ?? d.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());

export function stateLabel(e: HaEntity): string {
  if (e.state === "unavailable") return "Unavailable";
  if (e.state === "unknown") return "Unknown";
  const unit = typeof e.attributes.unit_of_measurement === "string" ? ` ${e.attributes.unit_of_measurement}` : "";
  if (e.state === "on" || e.state === "off") return e.state === "on" ? "On" : "Off";
  return `${e.state.replace(/_/g, " ")}${unit}`;
}

// ─── Home replies across reload ──────────────────────────────────────────────
/**
 * record_amara_interaction has no source field (and filters tier), so the "Home" badge is kept by
 * remembering saved interaction ids locally. Ids only; no message text. Drop once Rust adds `source`.
 */
export const HOME_REPLY_IDS_KEY = "nexus.ha.homeReplyIds";
const MAX_HOME_IDS = 500;
export function homeReplyIds(): Set<string> {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(HOME_REPLY_IDS_KEY) ?? "[]");
    return new Set(Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === "string") : []);
  } catch { return new Set(); }
}
export function rememberHomeReply(interactionId: string) {
  const ids = [...homeReplyIds().add(interactionId)].slice(-MAX_HOME_IDS);
  window.localStorage.setItem(HOME_REPLY_IDS_KEY, JSON.stringify(ids));
}

// ─── chat pre-route ──────────────────────────────────────────────────────────
/**
 * Every chat message goes to HA's conversation agent first. NoMatch, any error
 * (not configured, unreachable, backend missing) or an empty reply falls through
 * to the normal Hermes path, so chat never breaks because of HA.
 */
export async function preRouteHome(text: string): Promise<{ handled: true; speech: string } | { handled: false }> {
  const trimmed = text.trim();
  if (!trimmed || trimmed.length > HA_MAX_TEXT) return { handled: false };
  try {
    const result = await backend.conversation(trimmed);
    if (result?.kind === "match" && typeof result.speech === "string" && result.speech.trim()) return { handled: true, speech: result.speech.trim() };
  } catch (error) {
    console.warn(`[ha] pre-route skipped: ${classifyHaError(error)}`);
  }
  return { handled: false };
}

// ─── mocks (tests, e2e fixtures, until Rust lands) ───────────────────────────
export const mockEntity = (over: Partial<HaEntity> & { entityId: string }): HaEntity => ({
  state: "off", friendlyName: null, lastChanged: "2026-11-04T03:00:00Z", attributes: {}, domain: over.entityId.split(".")[0], ...over,
});

export const MOCK_ENTITIES: HaEntity[] = [
  mockEntity({ entityId: "light.desk_lamp", friendlyName: "Desk lamp", state: "on" }),
  mockEntity({ entityId: "switch.kettle", friendlyName: "Kettle", state: "off" }),
  mockEntity({ entityId: "sensor.office_temperature", friendlyName: "Office temperature", state: "21.5", attributes: { unit_of_measurement: "°C" } }),
  mockEntity({ entityId: "sun.sun", friendlyName: "Sun", state: "above_horizon" }),
];

export function createMockHaBackend(over: Partial<HaBackend> = {}): HaBackend & { emit: (c: HaStateChanged) => void; emitLive: (s: HaLiveState) => void } {
  const stateHandlers = new Set<(c: HaStateChanged) => void>();
  const liveHandlers = new Set<(s: HaLiveState) => void>();
  let running = true;
  return {
    states: async () => MOCK_ENTITIES.map((e) => ({ ...e })),
    callService: async () => {},
    conversation: async () => ({ kind: "noMatch" }),
    containerStatus: async () => ({ running, status: running ? "Up 3 hours" : "Exited (0) 1 minute ago" }),
    containerStart: async () => { running = true; return { running, status: "Up 1 second" }; },
    containerStop: async () => { running = false; return { running, status: "Exited (0) 1 second ago" }; },
    liveStart: async () => true,
    liveStop: async () => true,
    onStateChanged: async (h) => { stateHandlers.add(h); return () => { stateHandlers.delete(h); }; },
    onLiveStatus: async (h) => { liveHandlers.add(h); return () => { liveHandlers.delete(h); }; },
    emit: (c) => stateHandlers.forEach((h) => h(c)),
    emitLive: (s) => liveHandlers.forEach((h) => h(s)),
    ...over,
  };
}
