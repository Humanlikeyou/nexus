import type { NoopMetricKey, NoopMetricPoint } from "../types";
import { calendarDays } from "../ui/charts/chartMath";

export interface MetricDef {
  key: NoopMetricKey;
  label: string;
  unit: string;
  digits: number;
  color: string;
  /** Zero means "the band didn't read it" for these, so it's a gap (same rule as Last 14 days). */
  zeroIsMissing?: boolean;
  /** Stored value → shown value (minutes → hours). */
  convert?: (v: number) => number;
  bars?: boolean;
  domain?: [number, number];
  ticks?: number[];
}

export const METRICS: readonly MetricDef[] = [
  { key: "recovery", label: "Recovery", unit: "%", digits: 0, color: "var(--data-recovery-high)", domain: [0, 100], ticks: [0, 50, 100] },
  { key: "hrv", label: "HRV", unit: "ms", digits: 0, color: "var(--data-hrv)", zeroIsMissing: true },
  { key: "rhr", label: "Resting heart rate", unit: "bpm", digits: 0, color: "var(--sys-red)", zeroIsMissing: true },
  { key: "resp_rate", label: "Respiratory rate", unit: "br/min", digits: 1, color: "var(--sys-teal)", zeroIsMissing: true },
  { key: "spo2", label: "Blood oxygen", unit: "%", digits: 1, color: "var(--sys-blue)", zeroIsMissing: true },
  { key: "strain", label: "Effort", unit: "", digits: 0, color: "var(--data-strain)" },
  { key: "sleep_total_min", label: "Sleep", unit: "h", digits: 1, color: "var(--data-sleep)", zeroIsMissing: true, convert: (v) => v / 60, bars: true },
  { key: "steps", label: "Steps", unit: "", digits: 0, color: "var(--accent)", bars: true },
  { key: "active_kcal", label: "Active energy", unit: "kcal", digits: 0, color: "var(--copper)", bars: true },
];
export const METRIC_BY_KEY = Object.fromEntries(METRICS.map((m) => [m.key, m])) as Record<NoopMetricKey, MetricDef>;

export const RANGES = [{ value: "7", label: "Week", days: 7 }, { value: "30", label: "Month", days: 30 }, { value: "90", label: "3 months", days: 90 }, { value: "365", label: "Year", days: 365 }] as const;
export type RangeValue = (typeof RANGES)[number]["value"];

/** Latest day any series has, so every metric's window ends on the same day. */
export function endDay(all: readonly (readonly NoopMetricPoint[])[]): string | null {
  let last: string | null = null;
  for (const pts of all) for (const p of pts) if (p.value != null && (last == null || p.day > last)) last = p.day;
  return last;
}

export interface Slot { day: string; value: number | null }
/** One slot per calendar day of the window ending at `end`; missing or unreadable days are null. */
export function windowSeries(def: MetricDef, points: readonly NoopMetricPoint[], days: number, end: string | null): Slot[] {
  const clean = points.filter((p) => p.value != null && Number.isFinite(p.value));
  return calendarDays(clean, (p) => p.day, days, end).map(({ day, item }) => {
    let v = item ? (item.value as number) : null;
    if (v != null && def.zeroIsMissing && v <= 0) v = null;
    return { day, value: v != null && def.convert ? def.convert(v) : v };
  });
}

export interface Stats { latest: number | null; mean: number | null; min: number | null; max: number | null; count: number }
export function metricStats(slots: readonly Slot[]): Stats {
  const nums = slots.map((s) => s.value).filter((v): v is number => v != null);
  if (nums.length === 0) return { latest: null, mean: null, min: null, max: null, count: 0 };
  return { latest: nums[nums.length - 1], mean: nums.reduce((a, b) => a + b, 0) / nums.length, min: Math.min(...nums), max: Math.max(...nums), count: nums.length };
}

export const formatValue = (def: MetricDef, v: number | null) => (v == null ? "—" : def.digits === 0 ? Math.round(v).toLocaleString("en-US") : v.toFixed(def.digits));
export const withUnit = (def: MetricDef, v: number | null) => (v == null ? "No reading" : `${formatValue(def, v)}${def.unit === "%" ? "%" : def.unit ? ` ${def.unit}` : ""}`);
