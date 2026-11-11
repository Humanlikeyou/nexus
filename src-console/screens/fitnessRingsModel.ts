import type { NoopDailyMetric } from "../types";
import { calendarDays } from "../ui/charts/chartMath";
import { UNWIRED, sample, type Wired } from "../lib/sampleAdapters";

/**
 * design-1111r option B "Horizon": what the Recovery and Effort cards show, from data the app already has.
 * Usual = mean of up to the previous 13 days with a reading, once there are 7. Nothing is estimated.
 */
export const USUAL_DAYS = 13, USUAL_MIN = 7;
export type RingState = "live" | "building" | "pending" | "unwired";
export type WeekPoint = { day: string; value: number | null };
export type Driver = { key: "hrv" | "rhr"; label: string; value: number | null; unit: string; usual: number | null; delta: number | null };
export type Zone = { key: "Z1" | "Z2" | "Z3" | "Z4" | "Z5"; minutes: number };

export interface RingModel {
  state: RingState;
  value: number | null;
  usual: number | null;
  /** Readings the usual is built from (0–13); "building" until USUAL_MIN. */
  usualCount: number;
  /** Last 7 calendar days ending today, oldest first; pending shows the 6 before today. */
  week: WeekPoint[];
  takeaway: string | null;
}
export interface RecoveryModel extends RingModel { band: "High" | "Medium" | "Low" | null; drivers: Driver[] }
export interface EffortModel extends RingModel { zones: Wired<Zone[]> }

const ok = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);
const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;

/** The usual from the days before `day`: null under USUAL_MIN readings. */
export function usualOf(days: readonly NoopDailyMetric[], day: string, pick: (d: NoopDailyMetric) => number | null): { usual: number | null; count: number } {
  const xs = days.filter((d) => d.day < day).map(pick).filter(ok).slice(-USUAL_DAYS);
  return { usual: xs.length >= USUAL_MIN ? mean(xs) : null, count: xs.length };
}

function weekOf(days: readonly NoopDailyMetric[], day: string, pick: (d: NoopDailyMetric) => number | null, today: number | null, n = 7): WeekPoint[] {
  return calendarDays(days.filter((d) => d.day <= day), (d) => d.day, n, day).map(({ day: d, item }) => ({ day: d, value: d === day && today != null ? today : item ? (ok(pick(item)) ? pick(item) : null) : null }));
}
const prevDay = (day: string) => new Date(Date.parse(`${day}T00:00:00Z`) - 864e5).toISOString().slice(0, 10);

export function recoveryBandWord(v: number | null): RecoveryModel["band"] {
  return v == null ? null : v >= 67 ? "High" : v >= 34 ? "Medium" : "Low";
}

export interface RecoveryInput { day: string; recovery: number | null; hrv: number | null; rhr: number | null; nightly: boolean }

export function recoveryModel(input: RecoveryInput, days: readonly NoopDailyMetric[]): RecoveryModel {
  const { day } = input;
  const value = ok(input.recovery) ? input.recovery : null;
  const ever = value != null || days.some((d) => ok(d.recovery));
  const { usual, count } = usualOf(days, day, (d) => d.recovery);
  const driver = (key: Driver["key"], label: string, unit: string, v: number | null, pick: (d: NoopDailyMetric) => number | null): Driver => {
    const u = usualOf(days, day, pick).usual;
    const val = ok(v) ? v : null;
    return { key, label, unit, value: val, usual: u, delta: val != null && u != null ? val - u : null };
  };
  const drivers = [driver("hrv", "HRV", "ms", input.hrv, (d) => d.avgHrv), driver("rhr", "Rest HR", "bpm", input.rhr, (d) => d.restingHr)];
  if (!ever) return { state: "unwired", value: null, usual: null, usualCount: 0, week: [], takeaway: null, band: null, drivers: [] };
  if (input.nightly || value == null) return { state: "pending", value: null, usual, usualCount: count, week: weekOf(days, prevDay(day), (d) => d.recovery, null, 6), takeaway: null, band: null, drivers: [] };
  const state: RingState = usual == null ? "building" : "live";
  const band = recoveryBandWord(value);
  return { state, value, usual, usualCount: count, week: weekOf(days, day, (d) => d.recovery, value), takeaway: recoveryTakeaway(value, usual, band, drivers), band, drivers };
}

/** One line, from the numbers on the card. HRV up and resting HR down both help recovery. */
export function recoveryTakeaway(value: number, usual: number | null, band: RecoveryModel["band"], drivers: Driver[]): string {
  if (usual == null) return `${band} for you so far; the usual line appears after ${USUAL_MIN} days.`;
  const d = Math.round(value - usual);
  const side = d >= 3 ? 1 : d <= -3 ? -1 : 0;
  const lead = side === 0 ? null : drivers
    .filter((x) => x.delta != null && x.usual)
    .map((x) => ({ x, push: ((x.key === "rhr" ? -1 : 1) * (x.delta as number)) / (x.usual as number) }))
    .filter((p) => Math.sign(p.push) === side && Math.abs(p.push) >= .03)
    .sort((a, b) => Math.abs(b.push) - Math.abs(a.push))[0]?.x;
  const head = side > 0 ? "Above your usual" : side < 0 ? "Below your usual" : "Close to your usual";
  const led = lead ? `, led by ${lead.key === "hrv" ? "HRV" : "resting HR"}` : "";
  const tail = band === "High" && side >= 0 ? " Room for a harder session today." : band === "Low" ? " An easier day may suit." : "";
  return `${head}${led}.${tail}`;
}

export interface EffortInput { day: string; effort: number | null; sampleZones: boolean }
/** Sample zone minutes for preview mode only (tagged Sample data); NOOP stores the day's score, not HR-zone minutes. */
export const SAMPLE_ZONES: Zone[] = [{ key: "Z1", minutes: 22 }, { key: "Z2", minutes: 31 }, { key: "Z3", minutes: 14 }, { key: "Z4", minutes: 6 }, { key: "Z5", minutes: 0 }];

export function effortModel(input: EffortInput, days: readonly NoopDailyMetric[], clamp: (v: number) => number): EffortModel {
  const { day } = input;
  const pick = (d: NoopDailyMetric) => (ok(d.strain) ? clamp(d.strain) : null);
  const value = ok(input.effort) ? clamp(input.effort) : null;
  const zones: Wired<Zone[]> = input.sampleZones ? sample(SAMPLE_ZONES) : UNWIRED;
  const ever = value != null || days.some((d) => ok(d.strain));
  const { usual, count } = usualOf(days, day, pick);
  if (!ever) return { state: "unwired", value: null, usual: null, usualCount: 0, week: [], takeaway: null, zones };
  if (value == null) return { state: "pending", value: null, usual, usualCount: count, week: weekOf(days, prevDay(day), pick, null, 6), takeaway: null, zones };
  const week = weekOf(days, day, pick, value);
  return { state: usual == null ? "building" : "live", value, usual, usualCount: count, week, takeaway: effortTakeaway(value, usual, week), zones };
}

export function effortTakeaway(value: number, usual: number | null, week: WeekPoint[]): string {
  if (usual == null) return `${Math.round(value)} so far today; the comparison starts after ${USUAL_MIN} days.`;
  const d = Math.round(value - usual);
  const head = Math.abs(d) < 3 ? "Close to your usual so far today" : `${d > 0 ? "Above" : "Below"} your usual so far today`;
  const prior = week.slice(0, -1).map((w) => w.value).filter(ok);
  const inRange = prior.length >= 2 && value >= Math.min(...prior) && value <= Math.max(...prior);
  return prior.length >= 2 ? `${head}; ${inRange ? "inside" : value > Math.max(...prior) ? "above" : "below"} this week's range.` : `${head}.`;
}
