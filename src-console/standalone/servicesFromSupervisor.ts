import type { ReasonCode, ServiceEvent as SupEvent, ServiceId, ServiceState as SupState, ServiceStatus } from "../bindings.gen";
import type { Service, ServiceEvent, ServiceGroup, ServiceState, ServicesOverview } from "./adapters";

/**
 * Settings > Services rows from the Phase 8 supervisor (`services_status`, `service_events`). Every word shown
 * comes from the tables below (fixed text per state and reason code); the only numbers are the real restart
 * counters and real timestamps. No uptime, version, count or latency is ever made up.
 */
type Static = { group: ServiceGroup; name: string; what: string; builtIn?: boolean };
const STATIC: Record<ServiceId, Static> = {
  "docker": { group: "core", name: "Background engine", what: "Docker. Runs the containers below, hidden. You never need to open it." },
  "model-runtime": { group: "core", name: "Model runtime", what: "Local AI on this Mac" },
  "amara-router": { group: "core", name: "Router", what: "Picks the fast or deep model for each request" },
  "hermes-gateway": { group: "core", name: "Assistant (Hermes)", what: "Amara's chat, tools and Telegram gateway" },
  "amara-voice": { group: "core", name: "Voice", what: "Listening and speaking" },
  "n8n": { group: "automation", name: "Automations (n8n)", what: "Runs your workflows" },
  "crew-service": { group: "automation", name: "Crews", what: "Runs your crews" },
  "postgres": { group: "memory", name: "Database (Postgres)", what: "Memory, automations, Terrance" },
  "letta": { group: "memory", name: "Memory (Letta)", what: "What Amara remembers" },
  "weaviate": { group: "memory", name: "Knowledge search", what: "Weaviate and Verba" },
  "health-reader": { group: "health", name: "Health sync · reader", what: "Built into Amara · reads your WHOOP history on this Mac", builtIn: true },
  "health-sync": { group: "health", name: "Health sync", what: "Built into Amara · talks to your WHOOP 4 strap", builtIn: true },
  "homeassistant": { group: "health", name: "Home Assistant", what: "Smart home" },
  "freqtrade": { group: "health", name: "Freqtrade", what: "Money / ARM trading bot" },
  "open-webui": { group: "side", name: "Open WebUI", what: "Separate chat window. Not part of Amara; Amara just watches it." },
};

/** Fixed sentence per reason code, shown when a service is not simply healthy. */
export const REASON_TEXT: Record<ReasonCode, string> = {
  not_checked: "Not checked yet.",
  ok: "Answering normally.",
  slow: "Slow to answer. It still works.",
  unreachable: "Not answering.",
  timeout: "Took too long to answer.",
  bad_status: "Answered with an error.",
  container_down: "The container is not running.",
  container_stopped: "The container is stopped.",
  container_starting: "The container is starting.",
  unhealthy: "The container says it is unhealthy.",
  missing_container: "The container is missing.",
  bad_bind: "It is open to the network. Amara will not start it that way.",
  docker_unavailable: "The background engine is not running.",
  not_running: "It is not running.",
  no_store: "No data found yet.",
  restarting: "Amara is restarting it.",
  restart_failed: "The restart did not work.",
};

const STATE: Record<SupState, ServiceState> = {
  unknown: "waiting", starting: "waiting", healthy: "healthy", degraded: "degraded", restarting: "restarting",
  needs_you: "failed", stopped: "stopped",
  // The UI has no "separate" pill (design-questions 2026-11-14): shown as healthy with a fixed meta.
  separate: "healthy",
};

const META: Record<SupState, string> = {
  unknown: "Not checked yet", starting: "Starting", healthy: "Answering", degraded: "Slow to answer", restarting: "Restarting",
  needs_you: "Needs you", stopped: "Stopped", separate: "Separate app · watched only",
};

/** A reason is only worth showing in place of the description when the service is not fine. */
const showReason = (s: SupState) => s === "degraded" || s === "restarting" || s === "needs_you" || s === "stopped" || s === "starting";

const clock = (ms: number) => {
  if (!Number.isFinite(ms) || ms <= 0) return "";
  return new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
};

const STATES = new Set<string>(Object.keys(STATE));
export const isSupStatus = (v: unknown): v is ServiceStatus => {
  const o = v as Record<string, unknown> | null;
  return !!o && typeof o.id === "string" && o.id in STATIC && typeof o.state === "string" && STATES.has(o.state)
    && typeof o.reasonCode === "string" && typeof o.attempt === "number" && typeof o.maxAttempts === "number" && typeof o.checkedMs === "number";
};

export function serviceFromSupervisor(s: ServiceStatus): Service {
  const st = STATIC[s.id];
  const reason = REASON_TEXT[s.reasonCode];
  const out: Service = {
    id: s.id, group: st.group, name: st.name,
    what: showReason(s.state) && reason ? reason : st.what,
    state: STATE[s.state],
    meta: META[s.state],
  };
  if (st.builtIn) out.builtIn = true;
  if (s.state === "restarting") {
    out.restartAttempt = s.attempt; out.restartMax = s.maxAttempts;
    out.meta = `try ${s.attempt} of ${s.maxAttempts}`;
  }
  if (s.state === "stopped" && s.owner === "user") out.meta = "by you";
  if (s.state === "restarting" || s.state === "stopped" || s.state === "needs_you" || s.state === "unknown" || s.state === "starting") out.reasonOwner = s.owner;
  return out;
}

export function servicesFromSupervisor(rows: ServiceStatus[]): ServicesOverview {
  const checked = rows.reduce((m, r) => Math.max(m, r.checkedMs), 0);
  return { services: rows.map(serviceFromSupervisor), checked: clock(checked) };
}

/** `service_events` rows to the logs dialog list. The backend text is fixed; nothing is added. */
export function eventsFromSupervisor(rows: SupEvent[]): ServiceEvent[] {
  return rows.filter((e) => e && typeof e.text === "string" && typeof e.tsMs === "number").map((e) => ({
    at: clock(e.tsMs),
    level: e.kind === "gave_up" || e.kind === "restart_failed" || e.state === "needs_you" ? "fail" as const
      : e.state === "degraded" || e.state === "restarting" || e.kind === "restart_requested" ? "warn" as const : "ok" as const,
    text: e.text,
  }));
}
