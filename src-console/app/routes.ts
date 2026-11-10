import type { ModuleId } from "../types";
import { Activity, BriefcaseBusiness, Bot, House, Settings2, Users, type LucideIcon } from "lucide-react";

export type SystemEntry = { id: ModuleId; label: string; signal: string; icon: LucideIcon };

/** Section anchors: a route whose view names a section of its host scrolls there (App). */
export const SECTION_ANCHORS: Partial<Record<ModuleId, string[]>> = {
  terrance: ["goals", "lists"],
  chat: ["activity"],
  settings: ["devices", "about", "home-assistant", "voice"],
};

/** Six items in three groups, like macOS Settings (screen-specs Part 2). The first group is untitled
 *  (it sits under the wordmark). ⌘1–6 follow this order. Mind–Body hosts Fitness and Neuro as sub-views. */
export const SYSTEM_GROUPS: Array<{ label: string; systems: SystemEntry[] }> = [
  { label: "", systems: [
    { id: "terrance", label: "Home", signal: "AI", icon: Bot },
    { id: "chat", label: "Agents", signal: "AGENTS", icon: Users },
    { id: "mindbody", label: "Mind–Body", signal: "BODY", icon: Activity },
  ] },
  { label: "Work", systems: [
    { id: "business", label: "Business", signal: "WORK", icon: BriefcaseBusiness },
  ] },
  { label: "System", systems: [
    { id: "settings", label: "Settings", signal: "APP", icon: Settings2 },
    { id: "smarthome", label: "Smart home", signal: "HA", icon: House },
  ] },
];

export const SYSTEMS = SYSTEM_GROUPS.flatMap((group) => group.systems);

/** A sub-view inside a host screen. `screen` is the (legacy) screen that renders it until the
 *  host builds the view natively; "main" is the host's own screen. */
export type SubView = { view: string; label: string; screen: ModuleId };

/** Sub-views per host. Interim: folded screens render as-is behind a SegmentedControl so nothing
 *  becomes unreachable; each host replaces its legacy screen as its spec (§5–§10) lands. */
export const HOST_VIEWS: Partial<Record<ModuleId, SubView[]>> = {
  // design-1110b B: chat lives on Home (under the lens), so this host is agents and crews only.
  // Activity is a section of Agents, Lists a section of Today, Voice a section of Settings.
  chat: [{ view: "main", label: "Agents", screen: "chat" }, { view: "crews", label: "Crews", screen: "crews" }],
  mindbody: [{ view: "main", label: "Overview", screen: "mindbody" }, { view: "fitness", label: "Fitness", screen: "health" }, { view: "neuro", label: "Neuro", screen: "neuro" }, { view: "neuro-history", label: "Neuro history", screen: "sessions" }],
  business: [{ view: "main", label: "Work", screen: "business" }, { view: "revenue", label: "Revenue", screen: "revenue" }],
};

/** Folded ids stay valid ModuleIds (stored state, ⌘K, onNavigate) and resolve to their host. */
export const ROUTE_ALIASES: Partial<Record<ModuleId, { id: ModuleId; view: string }>> = {
  health: { id: "mindbody", view: "fitness" },
  neuro: { id: "mindbody", view: "neuro" },
  sessions: { id: "mindbody", view: "neuro-history" },
  // Anchor alias (no sub-view): Home renders the Goals card and App scrolls to #goals.
  trajectory: { id: "terrance", view: "goals" },
  revenue: { id: "business", view: "revenue" },
  // Anchor aliases (design-1110b B): each folded screen is a section of its new host.
  assistant: { id: "chat", view: "activity" },
  lists: { id: "terrance", view: "lists" },
  crews: { id: "chat", view: "crews" },
  voice: { id: "settings", view: "voice" },
  // Anchor alias: Settings renders Devices inline (§9) and App scrolls to #devices.
  devices: { id: "settings", view: "devices" },
};

export type Route = { id: ModuleId; view: string };

/** Home's main view is the lens landing page; the dashboard moved one level down to this sub-view.
 *  No SegmentedControl: Home is left through the landing buttons and returned to through the sidebar. */
export const TODAY_ROUTE: Route = { id: "terrance", view: "today" };
export const TODAY_LABEL = "Today";
/** The landing page's buttons, in sidebar order: Today in Home's slot, then every other section. */
export const LANDING_ENTRIES: Array<SystemEntry & { shortcut: number | null }> = SYSTEMS.map((system, index) => (
  system.id === "terrance" ? { ...system, label: TODAY_LABEL, shortcut: null } : { ...system, shortcut: index + 1 }
));

export function resolveRoute(id: ModuleId, view = "main"): Route {
  const alias = ROUTE_ALIASES[id];
  if (alias) return { id: alias.id, view: alias.view };
  if (SYSTEMS.some((system) => system.id === id)) return { id, view };
  return { id: "terrance", view: "main" };
}

/** The screen that actually renders for a route (the host, or a folded legacy screen). */
export function screenFor(route: Route): ModuleId {
  if (route.id === "terrance") return route.view === "main" ? "landing" : "terrance";
  return HOST_VIEWS[route.id]?.find((entry) => entry.view === route.view)?.screen ?? route.id;
}
