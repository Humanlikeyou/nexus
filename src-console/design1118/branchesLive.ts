import { invokeBound } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceStores, live } from "../lib/localStores";
import type { BranchAdapter, BranchSuggestion } from "./branches";

/**
 * Live branch adapter (design-1118a 4.2-4.3) over src-tauri/src/commands/goal_route.rs. Synchronous reads over a
 * cache filled on first touch and refreshed after every write; a goal whose read has not landed stays unwired.
 * Accept / decline / undo go through the one write path (History + 8 s Undo, `add_undo("route", id)`).
 * Nothing here invents a branch or a percent: the percent comes back from `goal_route_get` / the roadmap store.
 */
export type BranchDeps = { call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>; notify: () => void };
const defaultDeps: BranchDeps = { call: (c, a) => invokeBound(c, a), notify: announceStores };

const REASONS = new Set(["stagnation", "not_working", "better_route"]);
const SIZES = new Set(["small", "medium", "big"]);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

export function mapBranch(raw: unknown): BranchSuggestion | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const id = str(r.id), goalId = str(r.goalId), fromStepId = str(r.fromStepId), reasonText = str(r.reasonText);
  if (!id || !goalId || fromStepId === null || reasonText === null || !REASONS.has(String(r.reasonKind)) || r.status !== "proposed") return null;
  const alternateSteps = (Array.isArray(r.alternateSteps) ? r.alternateSteps : []).flatMap((s) => {
    const a = s as Record<string, unknown>;
    const sid = str(a.id), title = str(a.title);
    return sid && title !== null && SIZES.has(String(a.size)) ? [{ id: sid, title, size: a.size as "small" | "medium" | "big" }] : [];
  });
  if (alternateSteps.length === 0) return null;
  return { id, goalId, fromStepId, reasonKind: r.reasonKind as BranchSuggestion["reasonKind"], reasonText, alternateSteps, status: "proposed", ...(r.sample === true ? { sample: true } : {}) };
}

export function makeLiveBranches(deps: BranchDeps = defaultDeps): BranchAdapter {
  const cache = new Map<string, BranchSuggestion | null>();
  const loading = new Set<string>();
  const lastWrite = new Map<string, string>(); // suggestion id -> history id, for Undo

  async function load(goalId: string): Promise<void> {
    try { cache.set(goalId, mapBranch(await deps.call("goal_branch_open", { goalId }))); } catch { /* stays as it was */ }
  }
  async function write(command: "goal_branch_accept" | "goal_branch_decline", id: string): Promise<Wired<{ ok: true }>> {
    try {
      const w = await deps.call<{ id?: string }>(command, { suggestionId: id, via: "typed" });
      if (typeof w?.id === "string") lastWrite.set(id, w.id);
    } catch { return UNWIRED; }
    for (const [g, b] of cache) if (b?.id === id) await load(g);
    deps.notify();
    return live({ ok: true as const });
  }
  return {
    open(goalId): Wired<BranchSuggestion | null> {
      if (!cache.has(goalId) && !loading.has(goalId)) {
        loading.add(goalId);
        void load(goalId).finally(() => { loading.delete(goalId); deps.notify(); });
      }
      return cache.has(goalId) ? live(cache.get(goalId) ?? null) : UNWIRED;
    },
    accept: (id) => write("goal_branch_accept", id),
    decline: (id) => write("goal_branch_decline", id),
    async undo(id) {
      const h = lastWrite.get(id);
      if (!h) return UNWIRED;
      try { await deps.call("add_undo", { kind: "route", id: h }); } catch { return UNWIRED; }
      lastWrite.delete(id);
      cache.clear();
      deps.notify();
      return live({ ok: true as const });
    },
  };
}
