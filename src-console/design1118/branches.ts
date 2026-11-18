import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import type { StepSize } from "../goals/roadmap";

/**
 * design-1118a §4.2–4.3: Amara's alternative branches. A service proposes; the UI grows the branch
 * on its own and waits. Nothing about a goal changes until Taisun presses ✓ (accept) or ✗ (decline).
 * The branch store and the stagnation / better-route detector are Needs wiring (Claude, backend):
 * in the app this adapter answers "unwired" and no branch is ever drawn. Sample adapters (preview,
 * tests) return one sample branch, always labelled Sample data.
 */
export type BranchReason = "stagnation" | "not_working" | "better_route";
export type BranchStep = { id: string; title: string; size: StepSize };
export type BranchSuggestion = {
  id: string;
  goalId: string;
  fromStepId: string;
  reasonKind: BranchReason;
  reasonText: string;
  alternateSteps: BranchStep[];
  status: "proposed" | "accepted" | "declined" | "retracted";
  sample?: boolean;
};

export interface BranchAdapter {
  /** The open (proposed) branch for a goal, or null. */
  open(goalId: string): Wired<BranchSuggestion | null>;
  accept(id: string): Promise<Wired<{ ok: true }>>;
  decline(id: string): Promise<Wired<{ ok: true }>>;
  /** Undo of an accept, within the 8 s window: the previous route comes back. */
  undo(id: string): Promise<Wired<{ ok: true }>>;
}

export const unwiredBranches: BranchAdapter = {
  open: () => UNWIRED, accept: async () => UNWIRED, decline: async () => UNWIRED, undo: async () => UNWIRED,
};

/** Sample: one branch off the current station of the first goal it is asked about. */
export function makeSampleBranches(): BranchAdapter {
  const state = new Map<string, BranchSuggestion>();
  let firstGoal: string | null = null;
  const ok = sample({ ok: true as const });
  const set = (id: string, status: BranchSuggestion["status"]) => { for (const b of state.values()) if (b.id === id) b.status = status; };
  return {
    open(goalId) {
      firstGoal ??= goalId;
      if (goalId !== firstGoal) return sample(null);
      if (!state.has(goalId)) {
        state.set(goalId, {
          id: `branch-${goalId}`, goalId, fromStepId: "", reasonKind: "stagnation", sample: true, status: "proposed",
          reasonText: "This step hasn't moved for a while. A smaller route gets the same result.",
          alternateSteps: [
            { id: "b1", title: "Email the supplier for the sample price list", size: "small" },
            { id: "b2", title: "Order one sample first", size: "medium" },
          ],
        });
      }
      const b = state.get(goalId)!;
      return sample(b.status === "proposed" ? { ...b } : null);
    },
    accept: async (id) => { set(id, "accepted"); return ok; },
    decline: async (id) => { set(id, "declined"); return ok; },
    undo: async (id) => { set(id, "proposed"); return ok; },
  };
}

let sampleSingleton: BranchAdapter | null = null;
export function branchAdapter(): BranchAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleBranches());
  return unwiredBranches;
}
export function __resetBranchesForTests() { sampleSingleton = null; }
