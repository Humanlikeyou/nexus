import { useSyncExternalStore } from "react";
import { localDay } from "./roadmapLive";

/**
 * Which take shows today's step question (decision 0013: one per day across the app). The first take to
 * mount and want it claims it; the others stay quiet. Once he answers, the day is marked so no take
 * (including a sample adapter that never returns null) shows it again until tomorrow.
 */
let claimant: string | null = null;
let answeredDay = "";
let version = 0;
const listeners = new Set<() => void>();
const bump = () => { version += 1; listeners.forEach((l) => l()); };

export const stepQuestionStore = {
  claimant: () => claimant,
  claim(id: string): boolean { if (claimant === null) { claimant = id; bump(); } return claimant === id; },
  release(id: string) { if (claimant === id) { claimant = null; bump(); } },
  answeredToday: (day: string = localDay()) => answeredDay === day,
  markAnswered(day: string = localDay()) { answeredDay = day; bump(); },
  reset() { claimant = null; answeredDay = ""; bump(); },
};

export function useStepQuestionClaim(): number {
  return useSyncExternalStore((l) => { listeners.add(l); return () => { listeners.delete(l); }; }, () => version, () => version);
}
