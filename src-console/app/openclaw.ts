import type { OpenClawStatus } from "../types";
import type { Tone } from "../ui";

export interface OpenClawBadge { tone: Tone; label: string; subtitle: string }

/** Badge + subtitle for OpenClaw status. Older backends sent only `online`. */
export function openClawBadge(s: OpenClawStatus): OpenClawBadge {
  const status = s.status ?? (s.online ? "online" : "offline");
  if (status === "online") return { tone: "success", label: "Online", subtitle: s.model && s.model !== "unknown" ? s.model : "Gateway reachable" };
  if (status === "not_installed") return { tone: "neutral", label: "Not installed", subtitle: "No gateway or launchd job on this Mac" };
  if (status === "configured") return { tone: "warning", label: "No heartbeat", subtitle: "Configured, no recent heartbeat" };
  return { tone: "neutral", label: "Not connected", subtitle: "Not set up" };
}

/**
 * Neutral "Assistant" badge for Home and Chat. The snapshot carries no router mode, so the
 * OpenClaw status is the only backend signal: the badge shows only while that backend is
 * reachable (it is then in use) and is hidden otherwise (local mode, not installed, silent).
 */
export const ASSISTANT_TITLE = "Assistant";
export function assistantBadge(s: OpenClawStatus): OpenClawBadge | null {
  const b = openClawBadge(s);
  return b.label === "Online" ? { ...b, subtitle: b.subtitle === "Gateway reachable" ? "Backend reachable" : b.subtitle } : null;
}
