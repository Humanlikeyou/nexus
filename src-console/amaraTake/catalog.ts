import raw from "./catalog.json";
import type { EvidenceLevel } from "../takes/adapters";
import type { TakePage } from "./types";

/** The reviewed suggestion list (catalog.json). The model never adds entries; copy and sources come only from here. */
export type TriggerOp = "below_baseline" | "above_baseline" | "below" | "above";
export type OutcomeSource = "noop_hrv" | "noop_efficiency" | "muse_relax" | "none";
export type DetectorId = "muse_breathing_before_session" | "whoop_walk" | "whoop_easy_day" | "ha_evening_lights" | "ledger_decision_before_noon" | "checkin_saved";

export type CatalogEntry = {
  id: string; page: TakePage; version: number; title: string; how: string; typeLabel: string; shortTitle: string;
  /** `by` is a fraction of the usual for the *_baseline ops and an absolute value for below/above. */
  trigger: { factKey: string; op: TriggerOp; by?: number };
  outcome: { source: OutcomeSource; metric: string; lag: "same_session" | "same_day" | "next_morning" | "that_night" | "event"; better: "up" | "down"; wobbleFloor: number; unit: string };
  detector: DetectorId | null; cooldownDays: number;
  contraindications?: Array<{ factKey: string; op: "below" | "above"; value: number }>;
  evidence: { level: EvidenceLevel; summary: string; sources: Array<{ cite: string; verified: boolean }>; risk?: string };
};

export const CATALOG: { version: number; entries: CatalogEntry[] } = raw as { version: number; entries: CatalogEntry[] };
export const entriesFor = (page: TakePage, entries: CatalogEntry[] = CATALOG.entries) => entries.filter((e) => e.page === page);
export const entryById = (id: string, entries: CatalogEntry[] = CATALOG.entries) => entries.find((e) => e.id === id) ?? null;
