import { inTauri } from "../lib/localStores";
import { makeLiveRoadmaps } from "./roadmapLive";
import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";

/**
 * Goal roadmaps (decision 0013): the CONTRACT between the backend (Rust store + nightly deep job +
 * `live` below) and Design (Life › Goals). Amara drafts a roadmap overnight on the local deep tier;
 * he accepts or edits it; progress moves ONLY when he answers Yes to a step (weighted by size).
 */

export type StepSize = "small" | "medium" | "big";
export const STEP_WEIGHT: Record<StepSize, number> = { small: 1, medium: 2, big: 3 };

export type RoadmapStep = {
  id: string;
  title: string;            // "Order three samples from the supplier"
  why: string | null;       // one line: how it moves the goal
  size: StepSize;
  order: number;
  done: boolean;
  doneAt: string | null;    // ISO
  notYetCount: number;      // after 2, Amara offers a smaller first move
  smallerSuggestion: string | null;
};

export type Roadmap = {
  goalId: string;           // goals.json id
  status: "drafting" | "suggested" | "accepted"; // drafting = queued for tonight's run
  draftedAt: string | null; // ISO, when the overnight job wrote it
  steps: RoadmapStep[];
  pct: number;              // done weight / total weight × 100 (0 if no steps)
};

/** The one step question of the day (across the whole app), shown in whichever take he sees first. */
export type StepQuestion = { goalId: string; goalTitle: string; stepId: string; stepTitle: string; askedOn: string };

export interface RoadmapAdapter {
  roadmap(goalId: string): Wired<Roadmap | null>; // null = no roadmap yet
  /** Queue the goal for tonight's deep run (no work happens now). */
  request(goalId: string): Promise<Wired<{ queued: true }>>;
  accept(goalId: string): Promise<Wired<{ ok: true }>>;
  editStep(goalId: string, stepId: string, patch: { title?: string; size?: StepSize }): Promise<Wired<{ ok: true }>>;
  /** Yes / Not yet. Yes marks it done and moves pct; Not yet counts, and after 2 sets smallerSuggestion. */
  answerStep(goalId: string, stepId: string, answer: "yes" | "not_yet"): Promise<Wired<{ ok: true }>>;
  /** Today's question, or null when one was already asked/answered today or nothing is open. */
  todaysQuestion(): Wired<StepQuestion | null>;
}

export const unwiredRoadmaps: RoadmapAdapter = {
  roadmap: () => UNWIRED, request: async () => UNWIRED, accept: async () => UNWIRED,
  editStep: async () => UNWIRED, answerStep: async () => UNWIRED, todaysQuestion: () => UNWIRED,
};

export function makeSampleRoadmaps(): RoadmapAdapter {
  const steps: RoadmapStep[] = [
    { id: "s1", title: "Pick the one product to launch first", why: "Everything else depends on it", size: "small", order: 1, done: true, doneAt: "2026-11-10T09:00:00+09:00", notYetCount: 0, smallerSuggestion: null },
    { id: "s2", title: "Order three samples from the supplier", why: "You need the real thing to photograph and test", size: "medium", order: 2, done: false, doneAt: null, notYetCount: 2, smallerSuggestion: "Email the supplier asking for the sample price list" },
    { id: "s3", title: "Shoot the product photos", why: "The listing needs them", size: "medium", order: 3, done: false, doneAt: null, notYetCount: 0, smallerSuggestion: null },
    { id: "s4", title: "Open the store page and list it", why: "This is the launch", size: "big", order: 4, done: false, doneAt: null, notYetCount: 0, smallerSuggestion: null },
  ];
  const total = steps.reduce((n, s) => n + STEP_WEIGHT[s.size], 0);
  const done = steps.filter((s) => s.done).reduce((n, s) => n + STEP_WEIGHT[s.size], 0);
  const rm: Roadmap = { goalId: "sample-goal", status: "suggested", draftedAt: "2026-11-12T03:10:00+09:00", steps, pct: Math.round((done / total) * 100) };
  const ok = sample({ ok: true as const });
  return {
    // design-1118a: one sample goal is still being planned (a quiet seed, no path) so previews show both states.
    roadmap: (goalId: string) => sample(goalId === "ms3" ? { ...rm, goalId, status: "drafting" as const, steps: [], pct: 0 } : rm), request: async () => sample({ queued: true as const }), accept: async () => ok,
    editStep: async () => ok, answerStep: async () => ok,
    todaysQuestion: () => sample({ goalId: "sample-goal", goalTitle: "Launch TaiyoSkin's first product", stepId: "s2", stepTitle: steps[1].title, askedOn: "2026-11-12" }),
  };
}

let sampleSingleton: RoadmapAdapter | null = null;
let liveSingleton: RoadmapAdapter | null = null;
/** Sample in mock mode; the live adapter (roadmapLive.ts) inside Tauri; otherwise unwired. */
export function roadmapAdapter(): RoadmapAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleRoadmaps());
  if (inTauri()) return (liveSingleton ??= makeLiveRoadmaps());
  return unwiredRoadmaps;
}
