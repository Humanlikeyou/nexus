import type { Wired } from "../lib/sampleAdapters";

export type TakePage = "fitness" | "neuro" | "mindBody" | "business" | "home" | "life" | "agents" | "smarthome";
export const TAKE_PAGES: TakePage[] = ["fitness", "neuro", "mindBody", "business", "home", "life", "agents", "smarthome"];

/** One deterministic fact. `accept` lists every number a reply may use to cite it (value, baseline, alt units). */
export type Fact = {
  key: string;
  label: string;
  value: number;
  unit: string;
  /** How the prompt shows the value, e.g. "7.2 h". */
  display: string;
  baseline: { mean: number; n: number; display: string } | null;
  accept: number[];
  /** How many prior-day values the baseline had to work with (before the 7-value gate), when known. */
  priorN?: number;
};

export type FactSet = { page: TakePage; day: string; facts: Fact[]; fingerprint: string };

/** The data points a take cited, stored with it. */
export type CitedPoint = { key: string; label: string; display: string; baseline: string | null };

export type AmaraTake = { page: TakePage; day: string; text: string; cited: CitedPoint[]; fingerprint: string; generatedAt: number };

export type TakeResult = Wired<AmaraTake>;

/** What is persisted per page under kv `amara_take_<page>`. `attempts` counts router calls that day (max 2). */
export type StoredTake = { day: string; attempts: number; fingerprint: string | null; take: AmaraTake | null };
