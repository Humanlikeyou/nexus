import type { BrainState, StateTimelineEntry } from "@/lib/types";

export function computeTickScore(state: BrainState, inZone: boolean): number {
  if (state === "calm" && inZone) return 3;
  if (inZone) return 2;
  if (state === "calm") return 1;
  return 0;
}

export function computeTimeInZonePct(timeline: StateTimelineEntry[]): number {
  if (timeline.length === 0) return 0;
  const inZoneCount = timeline.filter((e) => e.inZone).length;
  return Math.round((inZoneCount / timeline.length) * 100);
}

export function computeTotalScore(timeline: StateTimelineEntry[]): number {
  return timeline.reduce((sum, e) => sum + e.score, 0);
}

export type MilestoneLevel = "none" | "bronze" | "silver" | "gold" | "platinum";

export function getMilestone(timeInZonePct: number): MilestoneLevel {
  if (timeInZonePct >= 90) return "platinum";
  if (timeInZonePct >= 80) return "gold";
  if (timeInZonePct >= 65) return "silver";
  if (timeInZonePct >= 50) return "bronze";
  return "none";
}

export const MILESTONE_COLORS: Record<MilestoneLevel, string> = {
  none: "#6b7280",
  bronze: "#cd7f32",
  silver: "#c0c0c0",
  gold: "#ffd700",
  platinum: "#e5e4e2",
};
