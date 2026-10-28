import type { OpenClawStatus } from "../types";
import type { Tone } from "../ui";

export interface OpenClawBadge { tone: Tone; label: string; subtitle: string }

/** Badge + subtitle for OpenClaw rows on Home and Chat & Agents. Older backends sent only `online`. */
export function openClawBadge(s: OpenClawStatus): OpenClawBadge {
  const status = s.status ?? (s.online ? "online" : "offline");
  if (status === "online") return { tone: "success", label: "Online", subtitle: s.model && s.model !== "unknown" ? s.model : "Gateway reachable" };
  if (status === "configured") return { tone: "warning", label: "No heartbeat", subtitle: "Configured, no recent heartbeat" };
  return { tone: "neutral", label: "Not connected", subtitle: "Not set up" };
}
