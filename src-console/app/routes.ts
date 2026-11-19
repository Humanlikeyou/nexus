import type { ModuleId } from "../types";
import { Activity, BriefcaseBusiness, Bot, House, Settings2, Sprout, Users, type LucideIcon } from "lucide-react";

export type SystemEntry = { id: ModuleId; label: string; signal: string; icon: LucideIcon };

/** Section anchors: a route whose view names a section of its host scrolls there (App).
 *  design-1111v: Goals, Life admin and Weekly review left Today for Life (they redirect, see VIEW_REDIRECTS). */
export const SECTION_ANCHORS: Partial<Record<ModuleId, string[]>> = {
  terrance: ["needs-you", "checkin", "lists"],
  chat: ["activity"],
  // design-1111s: devices, voice and memory are Settings tabs now (they open at the top); these stay anchors.
  settings: ["about", "home-assistant", "updates"],
};

/** Seven items in three groups, like macOS Settings (screen-specs Part 2). The first group is untitled
 *  (it sits under the wordmark). ⌘1–7 follow this order. Mind–Body hosts Fitness and Neuro as sub-views.
 *  Order per decision 0009: ⌘1 Home, ⌘2 Life, ⌘3 Mind–Body, ⌘4 Enterprise, ⌘5 Agents, ⌘6 Smart home, ⌘7 Settings. */
// Taisun, 2026-11-12 (decision 0009): Home, Life, Mind–Body, Enterprise, Agents, then System: Smart home, Settings.
export const SYSTEM_GROUPS: Array<{ label: string; systems: SystemEntry[] }> = [
  { label: "", systems: [
    { id: "terrance", label: "Home", signal: "AI", icon: Bot },
    { id: "life", label: "Life", signal: "LIFE", icon: Sprout },
    { id: "mindbody", label: "Mind–Body", signal: "BODY", icon: Activity },
    { id: "enterprise", label: "Enterprise", signal: "WORK", icon: BriefcaseBusiness },
    { id: "chat", label: "Agents", signal: "AGENTS", icon: Users },
  ] },
  { label: "System", systems: [
    { id: "smarthome", label: "Smart home", signal: "HA", icon: House },
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
  // design-1110b B: chat lives on Home (under the lens), so this host is agents and crews only.
  // Activity is a section of Agents, Lists (To-dos) a section of Today, Voice a section of Settings.
  chat: [{ view: "main", label: "Agents", screen: "chat" }, { view: "crews", label: "Crews", screen: "crews" }],
  mindbody: [{ view: "main", label: "Overview", screen: "mindbody" }, { view: "fitness", label: "Fitness", screen: "health" }, { view: "neuro", label: "Neuro", screen: "neuro" }, { view: "neuro-history", label: "Neuro history", screen: "sessions" }, { view: "patterns", label: "Patterns", screen: "mindbody" }],
  // design-1111v: Life is its own host. No "main": the host opens on Goals.
  life: [{ view: "goals", label: "Goals", screen: "life" }, { view: "weekly-review", label: "Weekly review", screen: "life" }, { view: "dates", label: "Dates & deadlines", screen: "life" }, { view: "money", label: "Personal money", screen: "life" }],
  // design-1111s: Settings gets section tabs, the same bar Enterprise uses (Taisun, 14:59 JST Nov 11).
  settings: [{ view: "main", label: "General", screen: "settings" }, { view: "services", label: "Services", screen: "settings" }, { view: "devices", label: "Devices", screen: "settings" }, { view: "notifications", label: "Notifications", screen: "settings" }, { view: "backup", label: "Backup", screen: "settings" }, { view: "voice", label: "Voice", screen: "settings" }, { view: "memory", label: "Memory", screen: "settings" }],
  // design-1111v: Business became Enterprise. Money → Finances (old ids "revenue" and "money" redirect), Work → Tasks,
  // Personal moved to Life › Personal money. A venture is its own route: "ventures/<id>".
  enterprise: [{ view: "main", label: "Overview", screen: "business" }, { view: "decisions", label: "Decisions", screen: "business" }, { view: "ventures", label: "Ventures", screen: "business" }, { view: "finances", label: "Finances", screen: "revenue" }, { view: "treasury", label: "Treasury", screen: "business" }, { view: "tasks", label: "Tasks", screen: "business" }, { view: "content", label: "Content", screen: "business" }],
};

/** Folded ids stay valid ModuleIds (stored state, ⌘K, onNavigate) and resolve to their host. */
export const ROUTE_ALIASES: Partial<Record<ModuleId, { id: ModuleId; view: string }>> = {
  health: { id: "mindbody", view: "fitness" },
  neuro: { id: "mindbody", view: "neuro" },
  sessions: { id: "mindbody", view: "neuro-history" },
  // design-1111v: Goals moved from Today to Life.
  trajectory: { id: "life", view: "goals" },
  revenue: { id: "enterprise", view: "finances" },
  // Anchor aliases (design-1110b B): each folded screen is a section of its new host.
  assistant: { id: "chat", view: "activity" },
  lists: { id: "terrance", view: "lists" },
  crews: { id: "chat", view: "crews" },
  voice: { id: "settings", view: "voice" },
  // design-1111s: Devices and Voice are Settings tabs.
  devices: { id: "settings", view: "devices" },
};

export type Route = { id: ModuleId; view: string };

/** design-1111v §2: old host:view pairs and where they live now. Old links, stored views and ⌘K all land here. */
export const VIEW_REDIRECTS: Record<string, Route> = {
  "terrance:goals": { id: "life", view: "goals" },
  "terrance:weekly-review": { id: "life", view: "weekly-review" },
  "terrance:life-admin": { id: "life", view: "dates" },
  "life:main": { id: "life", view: "goals" },
  "life:personal-money": { id: "life", view: "money" },
  "enterprise:personal": { id: "life", view: "money" },
  "enterprise:revenue": { id: "enterprise", view: "finances" },
  "enterprise:money": { id: "enterprise", view: "finances" },
  "enterprise:work": { id: "enterprise", view: "tasks" },
};

/** The old host id: /business keeps working and keeps its view (business:work → enterprise:tasks). */
const RENAMED_HOSTS: Partial<Record<ModuleId, ModuleId>> = { business: "enterprise" };

/** Any route, old or new, as the route that renders it today. */
export function canonicalRoute(route: Route): Route {
  let next: Route = { id: RENAMED_HOSTS[route.id] ?? route.id, view: route.view || "main" };
  for (let hops = 0; hops < 4; hops++) {
    const to = VIEW_REDIRECTS[`${next.id}:${next.view}`];
    if (!to) break;
    next = to;
  }
  return next;
}

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
  const next = canonicalRoute(alias ? { id: alias.id, view: alias.view } : { id, view });
  if (SYSTEMS.some((system) => system.id === next.id)) return next;
  return { id: "terrance", view: "main" };
}

/** The host tab a view belongs to: "ventures/taiyo-skin" is on the Ventures tab. */
export function tabOf(route: Route): string {
  const views = HOST_VIEWS[route.id] ?? [];
  const exact = views.find((entry) => entry.view === route.view);
  if (exact) return exact.view;
  return views.find((entry) => route.view.startsWith(`${entry.view}/`))?.view ?? "main";
}

/** The screen that actually renders for a route (the host, or a folded legacy screen). */
export function screenFor(route: Route): ModuleId {
  if (route.id === "terrance") return route.view === "main" ? "landing" : "terrance";
  const tab = tabOf(route);
  return HOST_VIEWS[route.id]?.find((entry) => entry.view === tab)?.screen ?? route.id;
}

/** design-1111v: a venture's own page. */
export const ventureRoute = (id: string): Route => ({ id: "enterprise", view: `ventures/${id}` });
export const ventureOf = (route: Route): string | null => (route.id === "enterprise" && route.view.startsWith("ventures/") ? route.view.slice("ventures/".length) || null : null);
