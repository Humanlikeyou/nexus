import type { Service, ServiceGroup, ServiceState, ServicesOverview } from "./adapters";
import type { MapNode, PathStatus, SystemMap } from "./systemMap";

/**
 * Settings › Services rows derived from `system_map_status` only. A node's state comes from the paths that touch it:
 * all ok → healthy, any broken → failed, all off → stopped, anything else (unchecked, mixed ok/off) → waiting.
 * No uptime, version, count or log text is made up: `meta` carries only what the map itself says.
 */
const GROUP_BY_ID: Record<string, ServiceGroup> = {
  router: "core", ollama: "core", hermes: "core", voice: "core",
  crew: "automation", n8n: "automation",
  postgres: "memory", letta: "memory", verba: "memory",
  noop: "health", muse: "health", ha: "health",
  backups: "side",
};
const GROUP_BY_KIND: Partial<Record<MapNode["kind"], ServiceGroup>> = { backend: "core", service: "automation", store: "memory", device: "health" };

export function nodeState(statuses: PathStatus[]): ServiceState {
  if (statuses.length === 0) return "waiting";
  if (statuses.includes("broken")) return "failed";
  if (statuses.every((s) => s === "ok")) return "healthy";
  if (statuses.every((s) => s === "off")) return "stopped";
  return "waiting";
}

const clock = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
};

export function servicesFromMap(map: SystemMap): ServicesOverview {
  const services: Service[] = [];
  for (const n of map.nodes) {
    if (n.kind === "app" || n.kind === "external") continue;
    const paths = map.paths.filter((p) => p.from === n.id || p.to === n.id);
    const state = nodeState(paths.map((p) => p.status));
    const bad = paths.find((p) => p.status === "broken") ?? paths.find((p) => p.status === "off");
    const carries = paths.map((p) => p.carries).filter(Boolean)[0] ?? "";
    services.push({
      id: n.id, group: GROUP_BY_ID[n.id] ?? GROUP_BY_KIND[n.kind] ?? "side", name: n.label,
      what: (state === "failed" || state === "stopped") && bad?.reason ? bad.reason : carries,
      state,
      meta: state === "healthy" ? "Connected" : state === "waiting" ? "Not checked yet" : "",
    });
  }
  return { services, checked: clock(map.checkedAt) };
}
