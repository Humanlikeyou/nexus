import { invokeBound } from "../bridge";
import { liveDeps } from "../amaraTake/live";
import { TAKE_PAGE_LABEL } from "../amaraTake/takeView";
import type { TakePage } from "../amaraTake/types";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { isSnapshot, noopQuery, snapshotDays } from "../lib/noop";
import type { RecordedSession } from "../types";
import type { Action, GoalsFromTakes, TakeSettings, TodayState, TypeOutcome, WeekSuggestions } from "./adapters";
import { dayOf, resolveToday, runDetectors, composeToday, computeViews, writeAnswer, writeUndo, SUGGEST_PAGES, type SuggestDeps, type TodayBase } from "./suggestions";
import { hasDetector, type HaState, type LedgerRequest, type Workout } from "./detectors";
import { appendTakeEvent, autoDoneOn, ensureTakeEvents, takeEvents, typeActions } from "./events";
import { notifyTakeLog, takeLogEntries, tokyoSafe } from "./takeLog";
import { CATALOG } from "../amaraTake/catalog";
import type { Sources } from "./typeOutcomes";

/** The live wiring for HANDOFF-1111t §11 steps 2 to 5. Reads are synchronous over a cache this module refreshes. */
async function loadSources(): Promise<Sources | null> {
  const read = await noopQuery("health_snapshot", { days: 120 });
  if (!read.ok || !isSnapshot(read.value)) return null;
  const sessions = await invokeBound<RecordedSession[]>("list_sessions").catch(() => [] as RecordedSession[]);
  const by = new Map<string, number[]>();
  for (const s of sessions) { const d = tokyoSafe(s.start); const v = s.summary?.avg_relaxation; if (d && typeof v === "number" && Number.isFinite(v)) by.set(d, [...(by.get(d) ?? []), v]); }
  return { rows: snapshotDays(read.value), museRelax: new Map([...by].map(([d, xs]) => [d, xs.reduce((a, b) => a + b, 0) / xs.length])) };
}

export const liveSuggestDeps: SuggestDeps = {
  now: () => Date.now(),
  events: takeEvents,
  append: appendTakeEvent,
  facts: liveDeps.facts,
  ask: liveDeps.ask,
  sources: loadSources,
  workouts: async () => (await invokeBound<Workout[]>("noop_recent_workouts", { days: 3 }).catch(() => null)),
  ledger: async () => {
    const l = await invokeBound<{ fundingRequests?: LedgerRequest[] } | null>("read_business_ledger").catch(() => null);
    return l ? l.fundingRequests ?? [] : null;
  },
  // Read only: `ha_states` is the only Home Assistant command this feature ever calls.
  haStates: async () => (await invokeBound<HaState[]>("ha_states").catch(() => null)),
};

type View = { day: string; at: number; today: Map<TakePage, TodayBase>; outcomes: TypeOutcome[] | null; week: WeekSuggestions | null; goals: GoalsFromTakes | null };
let view: View | null = null;
let running = false;
let lastTry = 0;
const STALE_MS = 5 * 60_000;

const takesThisWeek = (deps: SuggestDeps) => {
  const log = takeLogEntries() ?? []; const now = deps.now();
  return log.filter((e) => e.kind === "daily" && e.forgottenAt == null && e.tier !== "home_hr" && now - e.generatedAt < 7 * 86_400_000 && e.text)
    .sort((a, b) => b.generatedAt - a.generatedAt).slice(0, 3)
    .map((e) => ({ text: (e.text.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? e.text).trim(), from: `${TAKE_PAGE_LABEL[e.page]}, ${new Date(e.generatedAt).toLocaleDateString("en-US", { weekday: "short", timeZone: "Asia/Tokyo" })}` }));
};

export async function refreshView(deps: SuggestDeps = liveSuggestDeps): Promise<void> {
  if (running || deps.events() === null) return;
  running = true; lastTry = deps.now();
  try {
    const day = dayOf(deps.now());
    let cached: TypeOutcome[] | null = null;
    const outcomes = async () => (cached ??= (await computeViews(deps, []))?.outcomes ?? []);
    const today = new Map<TakePage, TodayBase>();
    for (const page of SUGGEST_PAGES) { const b = await resolveToday(page, deps, outcomes).catch(() => null); if (b) today.set(page, b); }
    await runDetectors(deps).catch(() => 0);
    const v = await computeViews(deps, takesThisWeek(deps)).catch(() => null);
    view = { day, at: deps.now(), today, outcomes: v?.outcomes ?? null, week: v?.week ?? null, goals: v?.goals ?? null };
  } finally { running = false; notifyTakeLog(); }
}

/** Starts a refresh when there is none yet, the day changed or the view is stale. Never loops: tries are spaced. */
function kick(): void {
  ensureTakeEvents();
  if (takeEvents() === null || running) return;
  const now = Date.now();
  if (view && view.day === dayOf(now) && now - view.at < STALE_MS) return;
  if (now - lastTry < 30_000) return;
  void refreshView();
}
export const resetLiveSuggestForTest = () => { view = null; running = false; lastTry = 0; };

const ok: Action = { state: "live", value: { ok: true } };

export const liveSuggestMethods = {
  today(page: TakePage): Wired<TodayState> {
    kick();
    const events = takeEvents(); const base = view?.today.get(page);
    if (!events || !view || !base || view.day !== dayOf(Date.now())) return UNWIRED;
    return composeToday(base, events, view.day);
  },
  async answer(page: TakePage, id: string, state: string, reason: string | null = null): Promise<Action> {
    if (!(await writeAnswer(liveSuggestDeps, page, id, state === "waiting" ? "" : state, reason))) return UNWIRED;
    return ok;
  },
  async undo(page: TakePage, id: string): Promise<Action> { return (await writeUndo(liveSuggestDeps, page, id)) ? ok : UNWIRED; },
  outcomes(): Wired<TypeOutcome[]> { kick(); return view?.outcomes ? { state: "live", value: view.outcomes } : UNWIRED; },
  week(): Wired<WeekSuggestions> { kick(); return view?.week ? { state: "live", value: view.week } : UNWIRED; },
  goals(): Wired<GoalsFromTakes> { kick(); return view?.goals ? { state: "live", value: view.goals } : UNWIRED; },
  settings(): Wired<TakeSettings> { ensureTakeEvents(); const ev = takeEvents(); return ev ? { state: "live", value: { keepDays: 365, summarizeAfter: true, autoDone: autoDoneOn(ev) } } : UNWIRED; },
  async setAutoDone(on: boolean): Promise<Action> {
    const now = Date.now();
    const stored = await appendTakeEvent({ id: `set-${now}`, kind: "setting", at: now, data: { autoDone: on } });
    if (stored && on) void refreshView();
    return stored ? ok : UNWIRED;
  },
  async typeAction(type: string, action: "keep" | "keep_suggesting" | "stop"): Promise<Action> {
    const o = view?.outcomes?.find((x) => x.type === type); const entry = CATALOG.entries.find((e) => e.id === type);
    if (!entry || typeActions(takeEvents() ?? []).get(type)?.action === action) return entry ? ok : UNWIRED;
    // Keep records the before/after numbers as they stand today, in words made by code.
    const note = action === "keep" && o ? o.stats.map((s) => `${s.label}: ${s.mean} (${s.n} days)`).join("; ") + "." : null;
    const now = Date.now();
    const stored = await appendTakeEvent({ id: `ta-${type}-${now}`, kind: "type_action", at: now, day: dayOf(now), page: entry.page, data: { type, action, ...(note ? { note } : {}) } });
    if (stored) void refreshView();
    return stored ? ok : UNWIRED;
  },
  detects(page: TakePage): boolean { return hasDetector(CATALOG.entries, page); },
};
