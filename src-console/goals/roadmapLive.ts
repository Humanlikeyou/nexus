import { invokeBound } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceAdds, announceStores, live } from "../lib/localStores";
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
  /** A step moved (done, edited, undone): the goal's other readers (Weekly review, Home) re-read. */
  moved?: () => void;
};

export const localDay = (d: Date = new Date()): string => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

const defaultDeps: RoadmapDeps = { call: (c, a) => invokeBound(c, a), today: () => localDay(), notify: announceStores, moved: announceAdds };

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
  const planNote = str(r.planNote);
  return { goalId, status, draftedAt: str(r.draftedAt), steps, pct: Math.max(0, Math.min(100, num(r.pct))), ...(planNote ? { planNote } : {}) };
}

export function mapQuestion(raw: unknown): StepQuestion | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const [goalId, goalTitle, stepId, stepTitle, askedOn] = [r.goalId, r.goalTitle, r.stepId, r.stepTitle, r.askedOn].map(str);
  return goalId && goalTitle !== null && stepId && stepTitle !== null && askedOn ? { goalId, goalTitle, stepId, stepTitle, askedOn } : null;
}

/** The live adapter, plus an Undo seam the RoadmapAdapter interface does not have (no screen calls it yet). */
export type LiveRoadmapAdapter = RoadmapAdapter & {
  /** Undo the last logged step write (edit or Yes) for this step: `add_undo("route", historyId)`. True if undone. */
  undoStepWrite(goalId: string, stepId: string): Promise<boolean>;
  /** Re-read one goal's roadmap now (after a manual planning run wrote it). */
  reload(goalId: string): Promise<void>;
};

/** Fixed words for a manual planning run (`roadmap_draft_pending` with catchUp): the Rust side re-checks power, idle time
 *  and "already ran today" itself, so a refusal is a normal answer, never an error to hide. */
export const PLAN_NIGHTLY = "Amara plans new goals overnight, and any time the Mac is on power and idle for 10 minutes.";
export function planNowNote(outcome: { ok: number } | { err: unknown }): string {
  if ("ok" in outcome) return outcome.ok > 0 ? "Planned. The path grows in now." : `Nothing was ready to plan yet. ${PLAN_NIGHTLY}`;
  const e = typeof outcome.err === "string" ? outcome.err : "";
  if (e === "busy") return "Amara is busy with something else right now. Try again in a few minutes. " + PLAN_NIGHTLY;
  if (e === "already_ran") return "Amara already ran today's planning. This goal is planned in tomorrow's run.";
  if (e === "not_idle") return "Planning needs the Mac on power and untouched for 10 minutes. " + PLAN_NIGHTLY;
  return "Planning didn't start. " + PLAN_NIGHTLY;
}

export function makeLiveRoadmaps(deps: RoadmapDeps = defaultDeps): LiveRoadmapAdapter {
  const lastWrite = new Map<string, string>(); // `${goalId}/${stepId}` -> History id, for Undo
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
    deps.moved?.();
    return live({ ok: true as const });
  }

  /** A logged step write (goal_step_edit / goal_step_complete): remember the History id it returns. */
  async function writeLogged(command: string, args: Record<string, unknown>, goalId: string, stepId: string): Promise<Wired<{ ok: true }>> {
    try {
      const w = await deps.call<{ id?: unknown }>(command, { ...args, via: "typed" });
      if (typeof w?.id === "string") lastWrite.set(`${goalId}/${stepId}`, w.id);
    } catch { return UNWIRED; }
    await Promise.all([loadRoadmap(goalId), loadQuestion()]);
    deps.notify();
    deps.moved?.();
    return live({ ok: true as const });
  }

  return {
    async reload(goalId) { await loadRoadmap(goalId); deps.notify(); },
    async undoStepWrite(goalId, stepId) {
      const key = `${goalId}/${stepId}`, id = lastWrite.get(key);
      if (!id) return false;
      lastWrite.delete(key);
      let ok = false;
      try { ok = (await deps.call<boolean>("add_undo", { kind: "route", id })) === true; } catch { return false; }
      await Promise.all([loadRoadmap(goalId), loadQuestion()]);
      deps.notify();
      deps.moved?.();
      return ok;
    },
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
    editStep: (goalId, stepId, patch) => writeLogged("goal_step_edit", { goalId, stepId, title: patch.title ?? null, size: patch.size ?? null }, goalId, stepId),
    // Yes -> goal_step_complete (logged, Undo). "Not yet" has no new-command equivalent: it only bumps
    // notYetCount, which goal_route.rs does not log or expose, so it stays on the legacy roadmap_answer_step.
    answerStep: (goalId, stepId, answer) => answer === "yes"
      ? writeLogged("goal_step_complete", { goalId, stepId }, goalId, stepId)
      : write("roadmap_answer_step", { goalId, stepId, answer }, goalId),
    todaysQuestion() {
      if (question !== undefined && questionDay !== deps.today()) question = undefined; // new day: ask again
      if (question === undefined && !questionLoading) { questionLoading = true; touch(loadQuestion(), () => { questionLoading = false; }); }
      return question === undefined ? UNWIRED : live(question);
    },
  };
}
