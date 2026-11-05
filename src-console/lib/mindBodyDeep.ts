import type { NoopMetricPoint } from "../types";
import { BASELINE_DAYS, baseline, type Baseline } from "./mindBody";

/**
 * The analysis on Mind–Body. Everything compares a day with Taisun's own prior 28 days, never with a
 * population. A reading is what happened; none of this says what it means for his health.
 */
export type MetricKey = "sleep" | "recovery" | "hrv" | "resting_hr" | "strain";
export type MetricDef = { key: MetricKey; label: string; unit: string; higherIsBetter: boolean | null; format: (v: number) => string };

export const hoursMinutes = (minutes: number) => `${Math.floor(minutes / 60)}h ${String(Math.round(minutes % 60)).padStart(2, "0")}m`;

export const METRICS: Record<MetricKey, MetricDef> = {
  sleep: { key: "sleep", label: "Sleep", unit: "", higherIsBetter: true, format: hoursMinutes },
  recovery: { key: "recovery", label: "Recovery", unit: "%", higherIsBetter: true, format: (v) => `${Math.round(v)}%` },
  hrv: { key: "hrv", label: "HRV", unit: "ms", higherIsBetter: true, format: (v) => `${Math.round(v)} ms` },
  resting_hr: { key: "resting_hr", label: "Resting HR", unit: "bpm", higherIsBetter: false, format: (v) => `${Math.round(v)} bpm` },
  strain: { key: "strain", label: "Strain", unit: "", higherIsBetter: null, format: (v) => v.toFixed(1) },
};

const sorted = (points: NoopMetricPoint[]) => [...points].sort((a, b) => a.day.localeCompare(b.day));
const finite = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);

/** Merges a long series with the snapshot's days; the snapshot wins, because it's the fresher read. */
export function mergeSeries(long: NoopMetricPoint[], recent: NoopMetricPoint[]): NoopMetricPoint[] {
  const map = new Map(long.map((p) => [p.day, p.value]));
  for (const p of recent) if (finite(p.value) || !map.has(p.day)) map.set(p.day, p.value);
  return sorted([...map].map(([day, value]) => ({ day, value })));
}

/** The 28 days before `day`, excluding it. */
export function baselineFor(points: NoopMetricPoint[], day: string | null): Baseline | null {
  const prior = sorted(points).filter((p) => !day || p.day < day).slice(-BASELINE_DAYS);
  return baseline(prior.map((p) => p.value));
}

export type MetricReading = { def: MetricDef; value: number | null; base: Baseline | null; delta: number | null; z: number | null };

export function metricReading(def: MetricDef, points: NoopMetricPoint[], day: string | null): MetricReading {
  const on = day ? sorted(points).filter((p) => p.day === day) : sorted(points).slice(-1);
  const value = on[0]?.value ?? null;
  const base = baselineFor(points, day);
  const delta = finite(value) && base ? value - base.mean : null;
  // sd floored as readiness does, so a flat month can't make a small change look huge.
  const z = delta !== null && base ? delta / Math.max(base.sd, def.key === "hrv" ? 1 : base.mean * .02 || 1) : null;
  return { def, value, base, delta, z };
}

/** Which way a change leans for readiness: better, worse, or just different. */
export function lean(reading: MetricReading): "up" | "down" | "flat" {
  if (reading.z === null || Math.abs(reading.z) < .25 || reading.def.higherIsBetter === null) return "flat";
  return (reading.z > 0) === reading.def.higherIsBetter ? "up" : "down";
}

const gap = (reading: MetricReading) => {
  const { def, delta } = reading;
  if (delta === null) return "";
  const size = def.key === "sleep" ? `${Math.round(Math.abs(delta))} min` : def.key === "strain" ? Math.abs(delta).toFixed(1) : `${Math.round(Math.abs(delta))}${def.unit === "%" ? " points" : ` ${def.unit}`}`;
  return `${size} ${delta >= 0 ? "above" : "below"} your usual`;
};

const display = (reading: MetricReading) => (reading.def.key === "hrv" || reading.def.key === "resting_hr" ? reading.def.label : reading.def.label.toLowerCase());

/** What moved today's score most, in plain words. Strain is context, not a driver of readiness. */
export function drivers(readings: MetricReading[]): MetricReading[] {
  return readings.filter((r) => r.z !== null && r.def.higherIsBetter !== null).sort((a, b) => Math.abs(b.z as number) - Math.abs(a.z as number));
}

export function driverSentence(readings: MetricReading[]): string | null {
  const ranked = drivers(readings);
  if (ranked.length === 0) return null;
  const top = ranked[0];
  if (Math.abs(top.z as number) < .5) return "Everything sits close to your usual today.";
  const second = ranked[1] && Math.abs(ranked[1].z as number) >= .5 ? `, then ${display(ranked[1])}, ${gap(ranked[1])}` : "";
  return `Mostly ${display(top)}, ${gap(top)}${second}.`;
}

const averageOf = (points: NoopMetricPoint[]) => { const xs = points.map((p) => p.value).filter(finite); return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null; };

/** Mean over the `n` days ending at `day`. */
export function windowAverage(points: NoopMetricPoint[], day: string | null, n: number): number | null {
  return averageOf(sorted(points).filter((p) => !day || p.day <= day).slice(-n));
}

/** Week against the 28 days: how load is running compared with what he's used to. */
export function loadRatio(points: NoopMetricPoint[], day: string | null): { ratio: number; line: string } | null {
  const week = windowAverage(points, day, 7), month = windowAverage(points, day, 28);
  if (week === null || month === null || month <= 0) return null;
  const ratio = week / month;
  const line = ratio > 1.3 ? "This week is running well above your usual load." : ratio < .8 ? "This week is lighter than your usual load." : "This week sits close to your usual load.";
  return { ratio, line };
}

/** The last `days` days ending at `day`, for a chart's range. */
export function inRange(points: NoopMetricPoint[], day: string | null, days: number): NoopMetricPoint[] {
  return sorted(points).filter((p) => !day || p.day <= day).slice(-days);
}

export type ChartGeometry = { path: string; band: { top: number; bottom: number } | null; mean: number | null; last: { x: number; y: number } | null };

/** Draws into a 0–100 box, so a chart can stretch to any width. Missing days break the line. */
export function chartGeometry(points: NoopMetricPoint[], base: Baseline | null): ChartGeometry {
  const values = points.map((p) => p.value).filter(finite);
  if (values.length === 0) return { path: "", band: null, mean: null, last: null };
  const low = Math.min(...values, ...(base ? [base.mean - base.sd] : []));
  const high = Math.max(...values, ...(base ? [base.mean + base.sd] : []));
  const span = high - low || 1;
  const y = (v: number) => 92 - ((v - low) / span) * 84;
  const x = (i: number) => (points.length === 1 ? 50 : (i / (points.length - 1)) * 100);
  let path = "", open = false, last: ChartGeometry["last"] = null;
  points.forEach((p, i) => {
    if (!finite(p.value)) { open = false; return; }
    path += `${open ? "L" : "M"}${x(i).toFixed(2)},${y(p.value).toFixed(2)} `;
    open = true;
    last = { x: x(i), y: y(p.value) };
  });
  return { path: path.trim(), band: base ? { top: y(base.mean + base.sd), bottom: y(base.mean - base.sd) } : null, mean: base ? y(base.mean) : null, last };
}
