import type { ModuleId } from "../types";
import { Activity, BriefcaseBusiness, Bot, HeartPulse, MessagesSquare, Radio, Settings2, type LucideIcon } from "lucide-react";

export type SystemEntry = { id: ModuleId; label: string; signal: string; icon: LucideIcon };

/** Seven items in four groups, like macOS Settings (screen-specs Part 2). The first group is untitled
 *  (it sits under the wordmark). ⌘1–7 follow this order. */
export const SYSTEM_GROUPS: Array<{ label: string; systems: SystemEntry[] }> = [
  { label: "", systems: [
    { id: "terrance", label: "Home", signal: "AI", icon: Bot },
    { id: "chat", label: "Chat & Agents", signal: "CHAT", icon: MessagesSquare },
  ] },
  { label: "Body & Mind", systems: [
    { id: "health", label: "Fitness", signal: "NOOP", icon: HeartPulse },
    { id: "neuro", label: "Neuro", signal: "EEG", icon: Radio },
    { id: "mindbody", label: "Mind–Body", signal: "BODY", icon: Activity },
  ] },
  { label: "Work", systems: [
    { id: "business", label: "Business", signal: "WORK", icon: BriefcaseBusiness },
  ] },
  { label: "System", systems: [
    { id: "settings", label: "Settings", signal: "APP", icon: Settings2 },
  ] },
];

export const SYSTEMS = SYSTEM_GROUPS.flatMap((group) => group.systems);

/** A sub-view inside a host screen. `screen` is the (legacy) screen that renders it until the
 *  host builds the view natively; "main" is the host's own screen. */
export type SubView = { view: string; label: string; screen: ModuleId };

/** Sub-views per host. Interim: folded screens render as-is behind a SegmentedControl so nothing
 *  becomes unreachable; each host replaces its legacy screen as its spec (§5–§10) lands. */
export const HOST_VIEWS: Partial<Record<ModuleId, SubView[]>> = {
  chat: [{ view: "main", label: "Chat", screen: "chat" }, { view: "activity", label: "Activity", screen: "assistant" }, { view: "crews", label: "Crews", screen: "crews" }, { view: "voice", label: "Voice", screen: "voice" }],
  neuro: [{ view: "main", label: "Live", screen: "neuro" }, { view: "history", label: "History", screen: "sessions" }],
  business: [{ view: "main", label: "Work", screen: "business" }, { view: "revenue", label: "Revenue", screen: "revenue" }],
};

/** Folded ids stay valid ModuleIds (stored state, ⌘K, onNavigate) and resolve to their host. */
export const ROUTE_ALIASES: Partial<Record<ModuleId, { id: ModuleId; view: string }>> = {
  sessions: { id: "neuro", view: "history" },
  // Anchor alias (no sub-view): Home renders the Goals card and App scrolls to #goals.
  trajectory: { id: "terrance", view: "goals" },
  revenue: { id: "business", view: "revenue" },
  assistant: { id: "chat", view: "activity" },
  crews: { id: "chat", view: "crews" },
  // Anchor alias: Settings renders Devices inline (§9) and App scrolls to #devices.
  devices: { id: "settings", view: "devices" },
};

export type Route = { id: ModuleId; view: string };

export function resolveRoute(id: ModuleId, view = "main"): Route {
  const alias = ROUTE_ALIASES[id];
  if (alias) return { id: alias.id, view: alias.view };
  if (SYSTEMS.some((system) => system.id === id)) return { id, view };
  return { id: "terrance", view: "main" };
}

/** The screen that actually renders for a route (the host, or a folded legacy screen). */
export function screenFor(route: Route): ModuleId {
  return HOST_VIEWS[route.id]?.find((entry) => entry.view === route.view)?.screen ?? route.id;
}
