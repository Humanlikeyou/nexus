import { announceAdds, announceStores, inTauri, live, realInvoke, type Invoke } from "../lib/localStores";
import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import type { GoalWire, GoalsWeek, Habit, Milestone } from "../personal/adapters";

/**
 * design-1118a Goals flow: add, edit and delete goals and habits in the SAME store the page reads
 * (src-tauri goals.rs: `read_goals` / `save_goals` / `delete_goal`; habits are rows with
 * category "habit", the way adds.rs saves them). After a write the personal adapter re-reads
 * (announceAdds), so the page only ever shows what the store holds. A new goal is queued for
 * Amara's route planning (`roadmap_request`); its path card shows the planning seed until then.
 * Mock mode keeps edits in memory only.
 */

export type GoalDraft = { title: string; target: number; unit: string; deadline: string | null; category?: string };
export type HabitDraft = { title: string; rule: string; perWeek: number };
type Ok = Wired<{ ok: true }>;

export interface GoalStore {
  /** The stored row behind a goal or habit card (for the edit form), or null if not known yet. */
  row(id: string): GoalWire | null;
  addGoal(d: GoalDraft): Promise<Wired<{ id: string }>>;
  addHabit(d: HabitDraft): Promise<Wired<{ id: string }>>;
  updateGoal(id: string, d: GoalDraft): Promise<Ok>;
  updateHabit(id: string, d: HabitDraft): Promise<Ok>;
  remove(id: string): Promise<Ok>;
  /** Mock mode: applies the in-memory edits to the sample week. Live: unchanged (the store already holds them). */
  overlay(week: GoalsWeek): GoalsWeek;
  overlayHabits(habits: Habit[]): Habit[];
}

const newId = (kind: "goal" | "habit") => `${kind}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
const clean = (s: string) => s.trim().replace(/\s+/g, " ");
const perWeek = (n: number) => Math.max(1, Math.min(7, Math.round(Number.isFinite(n) ? n : 7)));
const goalRow = (id: string, d: GoalDraft, prev?: GoalWire | null): GoalWire => ({
  id, title: clean(d.title), category: d.category ?? prev?.category ?? "personal", target: Number.isFinite(d.target) ? Math.max(0, d.target) : 0,
  current: prev?.current ?? 0, unit: clean(d.unit), deadline: d.deadline || null,
});
const habitRow = (id: string, d: HabitDraft, prev?: GoalWire | null): GoalWire => ({
  id, title: clean(d.title), category: "habit", target: perWeek(d.perWeek), current: prev?.current ?? 0, unit: clean(d.rule), deadline: prev?.deadline ?? null,
});
export const goalLabel = (r: GoalWire) => {
  const unit = (r.unit ?? "").trim();
  return r.target > 0 ? `${r.current} of ${r.target}${unit ? ` ${unit}` : ""}` : unit || "No target";
};

export function makeLiveGoalStore(call: Invoke = realInvoke, notify: () => void = announceAdds): GoalStore & { load(): Promise<void> } {
  let rows: GoalWire[] = [];
  const load = async () => { try { const r = await call<GoalWire[]>("read_goals"); if (Array.isArray(r)) rows = r; announceStores(); } catch { /* stays as it was */ } };
  /** Read fresh, change, write the whole list back (the same way the store is saved elsewhere). */
  const write = async (change: (rs: GoalWire[]) => GoalWire[]): Promise<boolean> => {
    let fresh: GoalWire[];
    try { fresh = await call<GoalWire[]>("read_goals"); } catch { return false; }
    if (!Array.isArray(fresh)) return false;
    const next = change(fresh.map((r) => ({ ...r, deadline: r.deadline ?? null })));
    try { await call("save_goals", { goals: next }); } catch { return false; }
    rows = next; notify(); return true;
  };
  return {
    load,
    row: (id) => rows.find((r) => r.id === id) ?? null,
    async addGoal(d) {
      const id = newId("goal");
      if (!(await write((rs) => [...rs, goalRow(id, d)]))) return UNWIRED;
      try { await call("roadmap_request", { goalId: id }); } catch { /* the path waits for the planner */ }
      return live({ id });
    },
    async addHabit(d) { const id = newId("habit"); return (await write((rs) => [...rs, habitRow(id, d)])) ? live({ id }) : UNWIRED; },
    async updateGoal(id, d) { let found = false; const ok = await write((rs) => rs.map((r) => (r.id === id ? ((found = true), goalRow(id, d, r)) : r))); return ok && found ? live({ ok: true as const }) : UNWIRED; },
    async updateHabit(id, d) { let found = false; const ok = await write((rs) => rs.map((r) => (r.id === id ? ((found = true), habitRow(id, d, r)) : r))); return ok && found ? live({ ok: true as const }) : UNWIRED; },
    async remove(id) {
      try { const r = await call<GoalWire[]>("delete_goal", { goalId: id }); if (Array.isArray(r)) rows = r; } catch { return UNWIRED; }
      notify(); return live({ ok: true as const });
    },
    overlay: (w) => w,
    overlayHabits: (h) => h,
  };
}

/** Mock mode only: the sample goals' rows, so their edit forms open filled in. */
const SAMPLE_ROWS: GoalWire[] = [
  { id: "ms1", title: "Revenue $5,000 / month", category: "business", target: 5000, current: 1820, unit: "USD / month", deadline: null },
  { id: "ms2", title: "Taiyo Skin launch", category: "business", target: 25, current: 17, unit: "launch tasks", deadline: "2026-11-21" },
  { id: "ms3", title: "Amara OS v1 on this Mac", category: "agents", target: 10, current: 8, unit: "stages", deadline: "2026-12-15" },
];

export function makeSampleGoalStore(): GoalStore {
  const rows = new Map<string, GoalWire>(SAMPLE_ROWS.map((r) => [r.id, r]));
  const added: string[] = [];
  const deleted = new Set<string>();
  const ok = async () => { announceStores(); return sample({ ok: true as const }); };
  const toMilestone = (r: GoalWire, base?: Milestone): Milestone => {
    const pct = r.target > 0 ? Math.round(Math.max(0, Math.min(100, (r.current / r.target) * 100))) : 0;
    return { ...(base ?? {}), id: r.id, title: r.title, detail: base?.detail ?? r.category, right: goalLabel(r), pct };
  };
  return {
    row: (id) => rows.get(id) ?? null,
    async addGoal(d) { const id = newId("goal"); rows.set(id, goalRow(id, d)); added.push(id); announceStores(); return sample({ id }); },
    async addHabit(d) { const id = newId("habit"); rows.set(id, habitRow(id, d)); added.push(id); announceStores(); return sample({ id }); },
    async updateGoal(id, d) { rows.set(id, goalRow(id, d, rows.get(id))); return ok(); },
    async updateHabit(id, d) { rows.set(id, habitRow(id, d, rows.get(id))); return ok(); },
    async remove(id) { deleted.add(id); return ok(); },
    overlay(w) {
      const ms = w.milestones.filter((m) => !deleted.has(m.id)).map((m) => { const r = rows.get(m.id); return r && r.category !== "habit" && r.title !== m.title ? { ...m, title: r.title, right: goalLabel(r) } : r && r.category !== "habit" ? { ...m, right: goalLabel(r) } : m; });
      const extra = added.map((id) => rows.get(id)!).filter((r) => r.category !== "habit" && !deleted.has(r.id)).map((r) => toMilestone(r));
      const habits = [...added.map((id) => rows.get(id)!).filter((r) => r.category === "habit" && !deleted.has(r.id)).map(habitOf), ...w.habits];
      return { ...w, milestones: [...ms, ...extra], habits };
    },
    overlayHabits(hs) {
      return hs.filter((h) => !deleted.has(h.id)).map((h) => { const r = rows.get(h.id); return r && r.category === "habit" ? { ...h, title: r.title, rule: r.unit || h.rule, target: r.target } : h; });
    },
  };
}

const habitOf = (r: GoalWire): Habit => ({ id: r.id, title: r.title, source: "You", rule: r.unit || "daily", days: Array.from({ length: 7 }, () => "none" as const), done: 0, target: perWeek(r.target), note: "added just now" });

export const unwiredGoalStore: GoalStore = {
  row: () => null, addGoal: async () => UNWIRED, addHabit: async () => UNWIRED, updateGoal: async () => UNWIRED, updateHabit: async () => UNWIRED, remove: async () => UNWIRED,
  overlay: (w) => w, overlayHabits: (h) => h,
};

let sampleSingleton: GoalStore | null = null;
let liveSingleton: ReturnType<typeof makeLiveGoalStore> | null = null;
export function goalStore(): GoalStore {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleGoalStore());
  if (!inTauri()) return unwiredGoalStore;
  if (!liveSingleton) { liveSingleton = makeLiveGoalStore(); void liveSingleton.load(); }
  return liveSingleton;
}
/** Test seam. */
export const resetGoalStores = () => { sampleSingleton = null; liveSingleton = null; };
