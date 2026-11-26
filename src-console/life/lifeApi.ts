/**
 * Life host data (PLAN-0055, decisions 0055/0056). One loader over the existing Life commands, plus the
 * pure rules each view shows. Nothing here invents a value: a day without a habit record is unknown,
 * a goal without activity is "no data", and a progress fraction appears only when the backend gives one.
 */
import { useCallback, useEffect, useState } from "react";
import type {
  ActivityRow_Serialize as ActivityRow, Goal_Serialize as Goal, HabitDay, LifeArea_Serialize as LifeArea, LifeDateView_Serialize as LifeDateView,
  LifeFields_Deserialize as LifeFieldsDraft, LifeInsight_Serialize as LifeInsight, LifeLink, LifeProgress_Serialize as LifeProgress,
  RecommendationDetail_Serialize as RecommendationDetail, RecommendationRow_Serialize as RecommendationRow, TaskItem_Serialize as TaskItem,
  TerrenceProject_Serialize as Project, WeekPlan_Serialize as WeekPlan,
} from "../bindings.gen";
import { ADDS_EVENT, announceAdds, inTauri, realInvoke, type Invoke } from "../lib/localStores";
import { addDays, jstToday } from "../personal/adapters";

export type { ActivityRow, Goal, HabitDay, LifeArea, LifeDateView, LifeInsight, LifeLink, LifeProgress, Project, RecommendationDetail, RecommendationRow, TaskItem, WeekPlan };

/** How far back habit history is read (12 weeks: the trend chart). */
export const HISTORY_DAYS = 84;
export const LIFE_EVENT = "amara:life";
export const announceLife = () => { try { window.dispatchEvent(new Event(LIFE_EVENT)); } catch { /* no window in node tests */ } };

// ─── Dates ──────────────────────────────────────────────────────────────────

/** The Monday (JST week, Mon–Sun) of a `YYYY-MM-DD` day. */
export function mondayOf(day: string): string {
  const dow = new Date(`${day}T00:00:00Z`).getUTCDay(); // 0 = Sun
  return addDays(day, dow === 0 ? -6 : 1 - dow);
}
const DAY_LABEL = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const WEEKDAY = new Intl.DateTimeFormat("en-US", { weekday: "short", timeZone: "UTC" });
export const dayLabel = (iso: string) => (/^\d{4}-\d{2}-\d{2}$/.test(iso) ? DAY_LABEL.format(new Date(`${iso}T00:00:00Z`)) : iso);
export const weekdayLabel = (iso: string) => WEEKDAY.format(new Date(`${iso}T00:00:00Z`));
/** "Mon Nov 16 – Sun Nov 22" */
export const weekRange = (monday: string) => `${weekdayLabel(monday)} ${dayLabel(monday)} – ${weekdayLabel(addDays(monday, 6))} ${dayLabel(addDays(monday, 6))}`;
export const daysOf = (from: string, n: number) => Array.from({ length: n }, (_, i) => addDays(from, i));
const dayOfIso = (t: string) => t.slice(0, 10);

// ─── Goals ──────────────────────────────────────────────────────────────────

export const isHabit = (g: Goal) => g.category === "habit";
/** The Life status, read the same way as `goals::status_of`. */
export const statusOf = (g: Goal): string => g.life?.status ?? (g.paused ? "paused" : "active");
export const STATUS_LABEL: Record<string, string> = { active: "Active", paused: "Paused", blocked: "Blocked", completed: "Completed", archived: "Archived" };

/** "212 / 300 km", or a percent only when the backend computed a fraction (it needs a baseline). */
export function progressText(p: LifeProgress | undefined, g: Goal): string {
  if (p?.fraction != null) return `${Math.round(p.fraction * 100)}% · ${fmt(p.current)} / ${fmt(p.target)} ${p.unit}`.trim();
  const cur = p ? p.current : g.current, tgt = p ? p.target : g.target;
  if (cur == null && tgt == null) return "No progress recorded";
  return `${fmt(cur)} / ${fmt(tgt)} ${p?.unit ?? g.unit}`.trim();
}
const fmt = (v: number | null | undefined) => (v == null ? "–" : String(Math.round(v * 100) / 100));

/** Days until a goal's deadline (negative once passed), or null. */
export const daysUntil = (iso: string | null | undefined, today: string) => (iso && /^\d{4}-\d{2}-\d{2}/.test(iso) ? Math.round((Date.parse(`${iso.slice(0, 10)}T00:00:00Z`) - Date.parse(`${today}T00:00:00Z`)) / 86_400_000) : null);

/** Tasks tagged `#goal:<id>` belong to that goal, `#project:<id>` to that project. Business tasks (`#venture:`) stay in Enterprise. */
export const personalTasks = (tasks: TaskItem[]) => tasks.filter((t) => !t.venture);
export const tasksForGoal = (tasks: TaskItem[], goalId: string) => tasks.filter((t) => t.tags.includes(`goal:${goalId}`));
export const tasksForProject = (tasks: TaskItem[], projectId: string) => tasks.filter((t) => t.tags.includes(`project:${projectId}`));

export type GoalNode = { goal: Goal; projects: Array<{ link: LifeLink; project: Project | null }>; habits: Array<{ link: LifeLink; habit: Goal | null }>; tasks: TaskItem[] };
export type AreaNode = { area: LifeArea | null; goals: GoalNode[] };

/** Area → goal → linked projects, habits and tasks. Goals without an area (or with an archived one) sit under "No area". */
export function goalTree(d: Pick<LifeData, "goals" | "areas" | "links" | "projects" | "tasks">, showClosed = false): AreaNode[] {
  const goals = d.goals.filter((g) => !isHabit(g) && (showClosed || !["completed", "archived"].includes(statusOf(g))));
  const habits = new Map(d.goals.filter(isHabit).map((h) => [h.id, h]));
  const projects = new Map(d.projects.map((p) => [p.id, p]));
  const tasks = personalTasks(d.tasks);
  const node = (g: Goal): GoalNode => {
    const links = d.links[g.id] ?? [];
    return {
      goal: g,
      projects: links.filter((l) => l.kind === "goal_project").map((link) => ({ link, project: projects.get(link.otherRef.replace(/^project:/, "")) ?? null })),
      habits: links.filter((l) => l.kind === "goal_habit").map((link) => ({ link, habit: habits.get(link.otherRef.replace(/^goals:/, "")) ?? null })),
      tasks: tasksForGoal(tasks, g.id),
    };
  };
  const active = d.areas.filter((a) => a.status === "active");
  const out: AreaNode[] = active.map((area) => ({ area, goals: goals.filter((g) => g.life?.areaId === area.id).map(node) }));
  const loose = goals.filter((g) => !active.some((a) => a.id === g.life?.areaId));
  if (loose.length) out.push({ area: null, goals: loose.map(node) });
  return out;
}

// ─── Habits ─────────────────────────────────────────────────────────────────

/** Days a week a habit is meant for (`frequency`), else its legacy `target` per week (capped at 7), else null. */
export function weeklyTarget(h: Goal): number | null {
  const f = h.life?.frequency;
  if (f) return f.per === "day" ? 7 : Math.min(7, f.times);
  return null;
}
/** Not paused, archived or completed, and with a frequency (PLAN-0055: "habits due today"). */
export const habitIsDue = (h: Goal) => isHabit(h) && !h.paused && !["paused", "archived", "completed"].includes(statusOf(h)) && h.life?.frequency != null;

export type DayState = "done" | "not_done" | "no_record";
export type WeekCount = { monday: string; done: number; notDone: number; unknown: number; target: number | null };

/** Per-week counts from a habit's day history. Days after `today` are not counted at all (not unknown). */
export function weekCounts(days: HabitDay[], target: number | null, today: string, weeks = 12): WeekCount[] {
  const byDay = new Map(days.map((d) => [d.day, d.state as DayState]));
  const thisMonday = mondayOf(today);
  return Array.from({ length: weeks }, (_, i) => {
    const monday = addDays(thisMonday, -7 * (weeks - 1 - i));
    const c = { monday, done: 0, notDone: 0, unknown: 0, target };
    for (const d of daysOf(monday, 7)) {
      if (d > today) continue;
      const s = byDay.get(d) ?? "no_record";
      if (s === "done") c.done++; else if (s === "not_done") c.notDone++; else c.unknown++;
    }
    return c;
  });
}

/** "4 of 5 · 2 days unknown" for one week. Unknown is never folded into "not done". */
export function weekLine(c: WeekCount): string {
  const head = c.target != null ? `${c.done} of ${c.target} days` : `${c.done} day${c.done === 1 ? "" : "s"} done`;
  const parts = [head];
  if (c.notDone) parts.push(`${c.notDone} not done`);
  if (c.unknown) parts.push(`${c.unknown} day${c.unknown === 1 ? "" : "s"} with no record`);
  return parts.join(" · ");
}

/** A short pattern summary over the last weeks that have any record. */
export function habitPattern(counts: WeekCount[]): string {
  const known = counts.filter((c) => c.done + c.notDone > 0);
  if (known.length === 0) return "No records yet, so there is no pattern to read.";
  const met = known.filter((c) => c.target != null && c.done >= c.target).length;
  const withTarget = known.filter((c) => c.target != null).length;
  const avg = known.reduce((n, c) => n + c.done, 0) / known.length;
  const lead = withTarget ? `Target met in ${met} of ${withTarget} recorded week${withTarget === 1 ? "" : "s"}` : `${known.length} recorded week${known.length === 1 ? "" : "s"}`;
  return `${lead} · about ${Math.round(avg * 10) / 10} done days a week when recorded.`;
}

// ─── Weekly review rules ────────────────────────────────────────────────────

export type GoalWeek = { goal: Goal; state: "progressed" | "stalled" | "steady" | "no_data"; changes: number; last: string | null };

/** Progressed: a `goal.progressed` row inside the week. Stalled: active, none for 14 days up to the week's end, deadline within 90 days.
 *  No data: no activity rows at all for the goal (never "0"). */
export function goalWeeks(goals: Goal[], activity: ActivityRow[], monday: string): GoalWeek[] {
  const end = addDays(monday, 6);
  return goals.filter((g) => !isHabit(g) && !["completed", "archived"].includes(statusOf(g))).map((goal) => {
    const rows = activity.filter((a) => a.subjectRef === `goals:${goal.id}` && dayOfIso(a.occurredAt) <= end);
    const progress = rows.filter((a) => a.kind === "goal.progressed");
    const inWeek = progress.filter((a) => dayOfIso(a.occurredAt) >= monday);
    const last = progress.map((a) => dayOfIso(a.occurredAt)).sort().pop() ?? null;
    if (inWeek.length) return { goal, state: "progressed" as const, changes: inWeek.length, last };
    const left = daysUntil(goal.deadline, end);
    const quiet = !last || last < addDays(end, -13);
    if (statusOf(goal) === "active" && quiet && left != null && left >= 0 && left <= 90) return { goal, state: "stalled" as const, changes: 0, last };
    if (rows.length === 0) return { goal, state: "no_data" as const, changes: 0, last };
    return { goal, state: "steady" as const, changes: 0, last };
  });
}

// ─── Recommendations ────────────────────────────────────────────────────────

const GOOD = new Set(["measured", "self_report"]);
/** Evidence that can back an insight: some, and all measured or self-reported. */
export const goodEvidence = (d: RecommendationDetail | null) => !!d && d.evidence.length > 0 && d.evidence.every((e) => GOOD.has(e.quality));
export const QUALITY_LABEL: Record<string, string> = { measured: "Measured", self_report: "You recorded it", estimate: "Estimate", partial: "Partial", missing: "Missing" };
export const ROLE_LABEL: Record<string, string> = { observation: "Observation", calculation: "Calculation", hypothesis: "Hypothesis" };

// ─── Loader ─────────────────────────────────────────────────────────────────

export type LifeData = {
  today: string;
  goals: Goal[];
  areas: LifeArea[];
  links: Record<string, LifeLink[]>;
  progress: Record<string, LifeProgress>;
  history: Record<string, HabitDay[]>;
  projects: Project[];
  tasks: TaskItem[];
  plan: WeekPlan | null;
  dates: LifeDateView[];
  activity: ActivityRow[];
  /** Commands that failed to load (each view says so where it would have shown them). */
  failed: string[];
};

export type LifeState = { kind: "unwired" } | { kind: "loading" } | { kind: "ready"; data: LifeData };

/** Reads everything the Life views need. Each source settles on its own; a failed one is listed in `failed`, never filled in. */
export async function loadLife(call: Invoke, today = jstToday()): Promise<LifeData> {
  const failed: string[] = [];
  const get = async <T,>(cmd: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T> => {
    try { const v = await call<T>(cmd, args); return v ?? fallback; } catch { failed.push(cmd); return fallback; }
  };
  const monday = mondayOf(today);
  const [goals, areas, projects, tasks, plan, dates, activity] = await Promise.all([
    get<Goal[]>("read_goals", undefined, []),
    get<LifeArea[]>("life_areas", undefined, []),
    get<Project[]>("list_terrence_projects", { query: null }, []),
    get<TaskItem[]>("read_tasks", { workspacePath: null }, []),
    get<WeekPlan | null>("life_week_plan_get", { weekStart: monday }, null),
    get<LifeDateView[]>("life_date_list", undefined, []),
    get<ActivityRow[]>("activity_list", { scope: "personal", subjectRef: null, since: `${addDays(monday, -27)}T00:00:00+09:00`, limit: 1000 }, []),
  ]);
  const list = Array.isArray(goals) ? goals : [];
  const from = addDays(today, -(HISTORY_DAYS - 1));
  const links: Record<string, LifeLink[]> = {};
  const progress: Record<string, LifeProgress> = {};
  const history: Record<string, HabitDay[]> = {};
  await Promise.all(list.map(async (g) => {
    if (isHabit(g)) { history[g.id] = await get<HabitDay[]>("life_habit_history", { habitId: g.id, from, to: today }, []); return; }
    const [l, p] = await Promise.all([get<LifeLink[]>("life_links", { goalId: g.id }, []), get<LifeProgress | null>("life_progress", { goalId: g.id }, null)]);
    links[g.id] = l; if (p) progress[g.id] = p;
  }));
  return { today, goals: list, areas, links, progress, history, projects, tasks, plan, dates, activity, failed: [...new Set(failed)] };
}

/** The Life views' shared state. Re-reads after any Life write (`announceLife`) and after the add flow (`ADDS_EVENT`, which habit marks also send). */
export function useLife(call: Invoke | null = inTauri() ? realInvoke : null): { state: LifeState; reload: () => Promise<void> } {
  const [state, setState] = useState<LifeState>(call ? { kind: "loading" } : { kind: "unwired" });
  const reload = useCallback(async () => {
    if (!call) return;
    const data = await loadLife(call);
    setState({ kind: "ready", data });
  }, [call]);
  useEffect(() => {
    if (!call) return;
    let live = true;
    const run = () => { void loadLife(call).then((data) => { if (live) setState({ kind: "ready", data }); }); };
    run();
    const on = () => run();
    // Not STORES_EVENT: the roadmap cache announces it on every read it lands, which would re-read Life in a loop.
    window.addEventListener(LIFE_EVENT, on);
    window.addEventListener(ADDS_EVENT, on);
    return () => { live = false; window.removeEventListener(LIFE_EVENT, on); window.removeEventListener(ADDS_EVENT, on); };
  }, [call]);
  return { state, reload };
}

// ─── Writes (each re-reads every Life view on success) ──────────────────────

const after = async <T,>(p: Promise<T>): Promise<T> => { const v = await p; announceLife(); return v; };

export const lifeWrites = (call: Invoke = realInvoke) => ({
  updateLife: (goalId: string, fields: LifeFieldsDraft) => after(call<Goal>("life_update", { goalId, fields })),
  linkAdd: (kind: "goal_project" | "goal_habit", goalId: string, otherId: string) => after(call<LifeLink>("life_link_add", { kind, goalId, otherId })),
  linkRemove: (linkId: string) => after(call<boolean>("life_link_remove", { linkId })),
  areaUpsert: (id: string | null, name: string, description: string | null) => after(call<LifeArea>("life_area_upsert", { id, name, description })),
  areaArchive: (id: string) => after(call<LifeArea>("life_area_archive", { id })),
  markHabit: async (habitId: string, day: string, done: boolean) => { const r = await after(call<{ changed: boolean; id: string | null }>("habit_mark", { habitId, day, done, source: "manual" })); announceAdds(); return r; },
  savePlan: (plan: WeekPlan) => after(call<WeekPlan>("life_week_plan_set", { plan })),
  dateRange: (from: string, to: string) => call<LifeDateView[]>("life_date_range", { from, to }),
  recGet: (id: string) => call<RecommendationDetail | null>("recommendation_get", { scope: "personal", id }),
  recRespond: (id: string, response: "accepted" | "modified" | "dismissed", modification: Record<string, unknown> | null = null) =>
    after(call<string>("recommendation_respond", { scope: "personal", id, response, modificationJson: modification ? JSON.stringify(modification) : null })),
  recOutcome: (id: string, outcome: "completed" | "evaluated", evaluation: string | null = null, note: string | null = null) =>
    after(call<string>("recommendation_record_outcome", { scope: "personal", id, outcome, evaluation, resultRef: null, note })),
  /** Overview's one suggestion. Reading it may bring a new one up (Rust applies the rules and records `shown` once). */
  insight: () => call<LifeInsight>("life_insight"),
  weekObservations: (weekStart: string) => call<RecommendationRow[]>("life_week_observations", { weekStart }),
});
export type LifeWrites = ReturnType<typeof lifeWrites>;
