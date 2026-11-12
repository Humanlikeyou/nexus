import { invoke } from "@tauri-apps/api/core";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceStores, live } from "../lib/localStores";
import type { Roadmap, RoadmapAdapter, RoadmapStep, StepQuestion, StepSize } from "./roadmap";

/**
 * Live goal-roadmap adapter (decision 0013) over src-tauri/src/commands/goal_roadmap.rs.
 * Reads are synchronous over a cache filled on first touch and refreshed after every write; a
 * goal whose first read has not landed (or failed) stays unwired. Nothing here invents progress.
 */

export type RoadmapDeps = {
  call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  today: () => string; // YYYY-MM-DD, local (JST on his Mac)
  notify: () => void;
};

export const localDay = (d: Date = new Date()): string => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

const defaultDeps: RoadmapDeps = { call: (c, a) => invoke(c, a), today: () => localDay(), notify: announceStores };

const SIZES: ReadonlySet<string> = new Set(["small", "medium", "big"]);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);
const num = (v: unknown, d = 0): number => (typeof v === "number" && Number.isFinite(v) ? v : d);

function mapStep(r: Record<string, unknown>): RoadmapStep | null {
  const id = str(r.id), title = str(r.title);
  if (!id || title === null || !SIZES.has(String(r.size))) return null;
  return {
    id, title, why: str(r.why), size: r.size as StepSize, order: num(r.order), done: r.done === true,
    doneAt: str(r.doneAt), notYetCount: num(r.notYetCount), smallerSuggestion: str(r.smallerSuggestion),
  };
}

export function mapRoadmap(raw: unknown): Roadmap | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const goalId = str(r.goalId), status = str(r.status);
  if (!goalId || (status !== "drafting" && status !== "suggested" && status !== "accepted")) return null;
  const steps = (Array.isArray(r.steps) ? r.steps : [])
    .map((s) => mapStep(s as Record<string, unknown>))
    .filter((s): s is RoadmapStep => s !== null)
    .sort((a, b) => a.order - b.order);
  return { goalId, status, draftedAt: str(r.draftedAt), steps, pct: Math.max(0, Math.min(100, num(r.pct))) };
}

export function mapQuestion(raw: unknown): StepQuestion | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const [goalId, goalTitle, stepId, stepTitle, askedOn] = [r.goalId, r.goalTitle, r.stepId, r.stepTitle, r.askedOn].map(str);
  return goalId && goalTitle !== null && stepId && stepTitle !== null && askedOn ? { goalId, goalTitle, stepId, stepTitle, askedOn } : null;
}

export function makeLiveRoadmaps(deps: RoadmapDeps = defaultDeps): RoadmapAdapter {
  const cache = new Map<string, Roadmap | null>();
  const loading = new Set<string>();
  let question: StepQuestion | null | undefined; // undefined = not read yet
  let questionDay = "";
  let questionLoading = false;

  async function loadRoadmap(goalId: string): Promise<void> {
    try { cache.set(goalId, mapRoadmap(await deps.call("roadmap_get", { goalId }))); } catch { /* stays as it was */ }
  }
  async function loadQuestion(): Promise<void> {
    const day = deps.today();
    try { question = mapQuestion(await deps.call("roadmap_todays_question", { today: day })); questionDay = day; } catch { /* unwired */ }
  }
  const touch = (p: Promise<void>, done: () => void) => { void p.finally(() => { done(); deps.notify(); }); };

  async function write(command: string, args: Record<string, unknown>, goalId: string): Promise<Wired<{ ok: true }>> {
    try { await deps.call(command, args); } catch { return UNWIRED; }
    await Promise.all([loadRoadmap(goalId), loadQuestion()]);
    deps.notify();
    return live({ ok: true as const });
  }

  return {
    roadmap(goalId) {
      if (!cache.has(goalId) && !loading.has(goalId)) { loading.add(goalId); touch(loadRoadmap(goalId), () => loading.delete(goalId)); }
      return cache.has(goalId) ? live(cache.get(goalId) ?? null) : UNWIRED;
    },
    async request(goalId) {
      try { await deps.call("roadmap_request", { goalId }); } catch { return UNWIRED; }
      await loadRoadmap(goalId);
      deps.notify();
      return live({ queued: true as const });
    },
    accept: (goalId) => write("roadmap_accept", { goalId }, goalId),
    editStep: (goalId, stepId, patch) => write("roadmap_edit_step", { goalId, stepId, title: patch.title ?? null, size: patch.size ?? null }, goalId),
    answerStep: (goalId, stepId, answer) => write("roadmap_answer_step", { goalId, stepId, answer }, goalId),
    todaysQuestion() {
      if (question !== undefined && questionDay !== deps.today()) question = undefined; // new day: ask again
      if (question === undefined && !questionLoading) { questionLoading = true; touch(loadQuestion(), () => { questionLoading = false; }); }
      return question === undefined ? UNWIRED : live(question);
    },
  };
}
