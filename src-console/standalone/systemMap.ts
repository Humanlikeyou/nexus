import { UNWIRED, sample, type Wired } from "../lib/sampleAdapters";

/**
 * System map (decision 0011): the CONTRACT between the backend (`system_map_status`, live
 * implementation below the line) and Design (the map at the top of Settings › Services).
 * Status: ok = steady green, broken = blinking red (static ring with reduced motion),
 * off = steady grey (off on purpose), unknown = not checked yet / no probe for it.
 */

export type MapNodeKind = "app" | "backend" | "service" | "device" | "store" | "external";
export type MapNode = { id: string; label: string; kind: MapNodeKind };
export type PathStatus = "ok" | "broken" | "off" | "unknown";

export type MapPath = {
  id: string;
  from: string;              // MapNode id
  to: string;                // MapNode id
  carries: string;           // plain English, e.g. "Chat messages and replies"
  status: PathStatus;
  lastOkAt: string | null;   // ISO; null = never seen working (or not tracked)
  reason: string | null;     // plain English for broken/off ("Hermes isn't answering on this Mac"); never a secret or path
  fix: string | null;        // the one usual step ("Open Terminal and run: hermes gateway restart")
};

export type SystemMap = { nodes: MapNode[]; paths: MapPath[]; checkedAt: string };

export interface SystemMapAdapter {
  map(): Wired<SystemMap>;
  /** Re-check now. The screen calls it every 10 s while the map is visible, and stops when it isn't. */
  refresh(): Promise<void>;
}

export const unwiredSystemMap: SystemMapAdapter = { map: () => UNWIRED, refresh: async () => undefined };

/** Mock mode only: the real topology with sample statuses. */
export function makeSampleSystemMap(): SystemMapAdapter {
  const n = (id: string, label: string, kind: MapNodeKind): MapNode => ({ id, label, kind });
  const nodes = [
    n("app", "Amara OS", "app"), n("router", "Amara Router", "backend"), n("ollama", "Local models", "backend"),
    n("hermes", "Hermes (Chat)", "backend"), n("noop", "Health sync (NOOP)", "store"), n("muse", "Muse headset", "device"),
    n("crew", "Crews", "service"), n("n8n", "n8n", "service"), n("postgres", "Postgres", "store"), n("letta", "Letta memory", "store"),
    n("verba", "Knowledge base", "service"), n("ha", "Home Assistant", "service"), n("voice", "Amara Voice", "service"),
    n("telegram", "Telegram", "external"), n("backups", "Backups", "store"),
  ];
  const p = (id: string, from: string, to: string, carries: string, status: PathStatus, reason: string | null = null, fix: string | null = null): MapPath =>
    ({ id, from, to, carries, status, lastOkAt: status === "ok" ? "2026-11-12T10:00:00+09:00" : null, reason, fix });
  const paths = [
    p("app-router", "app", "router", "Every model request", "ok"), p("router-ollama", "router", "ollama", "Prompts to the local models", "ok"),
    p("app-hermes", "app", "hermes", "Chat messages and replies", "ok"), p("hermes-router", "hermes", "router", "Amara's thinking", "ok"),
    p("noop-app", "noop", "app", "Heart rate, sleep, recovery", "ok"), p("muse-app", "muse", "app", "EEG while a session records", "unknown", "Not connected right now", "Turn the headset on and press Connect in Neuro"),
    p("app-crew", "app", "crew", "Crew runs and approvals", "broken", "The crew service isn't answering", "Open Docker and start the stack"),
    p("crew-router", "crew", "router", "Crew model calls", "unknown"), p("n8n-postgres", "n8n", "postgres", "Workflow data", "ok"),
    p("crew-n8n", "crew", "n8n", "Knowledge and memory lookups", "ok"), p("n8n-verba", "n8n", "verba", "Knowledge base queries", "ok"),
    p("app-letta", "app", "letta", "Long-term memory reads", "ok"), p("app-ha", "app", "ha", "Smart home status", "ok"),
    p("voice-hermes", "voice", "hermes", "Spoken requests", "ok"),
    p("app-telegram", "app", "telegram", "Notifications to your phone", "off", "Sends are paused (outbound is off)", null),
    p("app-backups", "app", "backups", "Your backups", "ok"),
  ];
  return { map: () => sample({ nodes, paths, checkedAt: "2026-11-12T10:00:00+09:00" }), refresh: async () => undefined };
}
