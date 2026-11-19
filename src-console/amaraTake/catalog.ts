import raw from "./catalog.json";
import type { EvidenceLevel } from "../takes/adapters";
import type { TakePage } from "./types";

/** The reviewed suggestion list (catalog.json). The model never adds entries; copy and sources come only from here. */
export type TriggerOp = "below_baseline" | "above_baseline" | "below" | "above";
/** One condition. `factKey` is a fact key or a derived one: readiness (1 low, 2 moderate, 3 high), hour (JST), loadRatio (week strain over 28-day), daysSince:<kind>. */
export type Cond = { factKey: string; op: TriggerOp; by?: number };
/** A blocking condition. The threshold is `value` (old form) or `by`. Baseline ops use `by` as a fraction of the usual. */
export type Contra = { factKey: string; op: TriggerOp; value?: number; by?: number };
export type TakeKind = "calisthenics" | "strength" | "mobility" | "flexibility" | "recovery" | "endurance" | "conditioning" | "challenge" | "combo";
export const FITNESS_KINDS: TakeKind[] = ["calisthenics", "strength", "mobility", "flexibility", "recovery", "endurance", "conditioning", "challenge", "combo"];
export type Intensity = "easy" | "moderate" | "hard";
export type OutcomeSource = "noop_hrv" | "noop_efficiency" | "muse_relax" | "none";
export type DetectorId = "muse_breathing_before_session" | "whoop_walk" | "whoop_easy_day" | "ha_evening_lights" | "ledger_decision_before_noon" | "checkin_saved";

export type CatalogEntry = {
  id: string; page: TakePage; version: number; title: string; how: string; typeLabel: string; shortTitle: string;
  /** `by` is a fraction of the usual for the *_baseline ops and an absolute value for below/above. */
  trigger?: Cond;
  /** Composite trigger: every `all` condition and at least one `any` condition (when present) must hold. Wins over `trigger`. */
  triggers?: { all?: Cond[]; any?: Cond[] };
  /** Exercise category. Old entries without one are treated as their own kind. */
  kind?: TakeKind;
  /** How hard the session is. Used to match readiness and to guarantee easy options on low days. */
  intensity?: Intensity;
  /** Progressive challenge: one series, one entry per level. The next level is offered after the user answers "did it". */
  progression?: { series: string; level: number };
  outcome: { source: OutcomeSource; metric: string; lag: "same_session" | "same_day" | "next_morning" | "that_night" | "event"; better: "up" | "down"; wobbleFloor: number; unit: string };
  detector: DetectorId | null; cooldownDays: number;
  contraindications?: Contra[];
  evidence: { level: EvidenceLevel; summary: string; sources: Array<{ cite: string; verified: boolean }>; risk?: string };
};

export const CATALOG: { version: number; entries: CatalogEntry[] } = raw as { version: number; entries: CatalogEntry[] };
export const entriesFor = (page: TakePage, entries: CatalogEntry[] = CATALOG.entries) => entries.filter((e) => e.page === page);
export const entryById = (id: string, entries: CatalogEntry[] = CATALOG.entries) => entries.find((e) => e.id === id) ?? null;
