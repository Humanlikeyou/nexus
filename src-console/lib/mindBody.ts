import type { NoopDayRow, NoopMetricKey, NoopMetricPoint, NoopSeriesPayload, NoopSnapshotPayload, RecordedSession } from "../types";
import { parseBodyContext } from "./noop";

/* ─────────────────────────────── Readiness ───────────────────────────────
 * Readiness is judged against Taisun's OWN rolling baseline, never population norms.
 *
 * Baseline: the BASELINE_DAYS (28) days before today, excluding today. A metric's baseline is the
 * mean (and SD) of its non-null values; it needs MIN_BASELINE_DAYS (7) values or it doesn't count.
 *
 * Signals for today (each compared only to its own baseline):
 *   - HRV:       z = (today − mean) / sd   (sd floored at 1 ms so a flat week can't explode z)
 *   - Recovery:  delta = today − mean      (percentage points)
 *   - Sleep:     ratio = today / mean      (last night's total sleep vs usual)
 *
 * Rules (first match wins):
 *   1. Fewer than 2 signals with a valid baseline + a today value → "none" (no readiness yet).
 *   2. Low      if ANY: HRV z ≤ −1.0, recovery delta ≤ −20, sleep ratio < 0.75.
 *   3. High     if ALL available: HRV z ≥ −0.5, recovery delta ≥ −5, sleep ratio ≥ 0.9.
 *   4. Moderate otherwise.
 * ───────────────────────────────────────────────────────────────────────── */

export const BASELINE_DAYS = 28;
export const MIN_BASELINE_DAYS = 7;

export type ReadinessLevel = "high" | "moderate" | "low" | "none";
export type Baseline = { mean: number; sd: number; n: number };
export type ReadinessInput = { hrv: NoopMetricPoint[]; recovery: NoopMetricPoint[]; sleep: NoopMetricPoint[] };
export type Readiness = {
  level: ReadinessLevel;
  hrvZ: number | null;
  recoveryDelta: number | null;
  sleepRatio: number | null;
};

const finite = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);

export function baseline(values: Array<number | null>): Baseline | null {
  const xs = values.filter(finite);
  if (xs.length < MIN_BASELINE_DAYS) return null;
  const mean = xs.reduce((a, b) => a + b, 0) / xs.length;
  const sd = Math.sqrt(xs.reduce((a, b) => a + (b - mean) ** 2, 0) / xs.length);
  return { mean, sd, n: xs.length };
}

/** Splits a day-sorted series into today (last point) and the prior BASELINE_DAYS. */
function split(points: NoopMetricPoint[]) {
  const sorted = [...points].sort((a, b) => a.day.localeCompare(b.day));
  const today = sorted[sorted.length - 1]?.value ?? null;
  return { today, base: baseline(sorted.slice(-1 - BASELINE_DAYS, -1).map((p) => p.value)) };
}

export function readiness(input: ReadinessInput): Readiness {
  const h = split(input.hrv), r = split(input.recovery), s = split(input.sleep);
  const hrvZ = h.base && finite(h.today) ? (h.today - h.base.mean) / Math.max(h.base.sd, 1) : null;
  const recoveryDelta = r.base && finite(r.today) ? r.today - r.base.mean : null;
  const sleepRatio = s.base && finite(s.today) && s.base.mean > 0 ? s.today / s.base.mean : null;
  const signals = [hrvZ, recoveryDelta, sleepRatio].filter(finite).length;
  let level: ReadinessLevel;
  if (signals < 2) level = "none";
  else if ((finite(hrvZ) && hrvZ <= -1) || (finite(recoveryDelta) && recoveryDelta <= -20) || (finite(sleepRatio) && sleepRatio < 0.75)) level = "low";
  else if ((!finite(hrvZ) || hrvZ >= -0.5) && (!finite(recoveryDelta) || recoveryDelta >= -5) && (!finite(sleepRatio) || sleepRatio >= 0.9)) level = "high";
  else level = "moderate";
  return { level, hrvZ, recoveryDelta, sleepRatio };
}

export const READINESS_COPY: Record<ReadinessLevel, { label: string; tone: "success" | "warning" | "danger" | "neutral"; help: string }> = {
  high: { label: "High", tone: "success", help: "You're well recovered. A good day to push." },
  moderate: { label: "Moderate", tone: "warning", help: "Partly recovered. Keep training steady." },
  low: { label: "Low", tone: "danger", help: "Recovery is low. Favour rest or light work." },
  none: { label: "No data", tone: "neutral", help: "No readiness yet. Sync your device." },
};

/* ───────────────────────────── Focus drivers ─────────────────────────────
 * Each driver pairs a session's average focus with a body metric for that session: the stored
 * body_context first, else the NOOP daily value for the session's start day (Asia/Tokyo).
 * Correlation only (Pearson r) — never presented as cause. Trend text needs 15+ matched sessions.
 * ───────────────────────────────────────────────────────────────────────── */

export const MIN_TREND_SESSIONS = 15;
export type DriverKey = "sleep" | "hrv" | "recovery";
export type DriverPoint = { x: number; y: number };

const BODY_FIELD: Record<DriverKey, "sleep_minutes" | "hrv_ms" | "recovery"> = { sleep: "sleep_minutes", hrv: "hrv_ms", recovery: "recovery" };

export function tokyoDay(iso: string): string | null {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return d.toLocaleDateString("en-CA", { timeZone: "Asia/Tokyo" });
}

export function matchDriver(sessions: RecordedSession[], key: DriverKey, series: NoopMetricPoint[]): DriverPoint[] {
  const byDay = new Map(series.map((p) => [p.day, p.value]));
  const out: DriverPoint[] = [];
  for (const s of sessions) {
    const y = s.summary?.avg_focus;
    if (!finite(y)) continue;
    const ctx = parseBodyContext(s.body_context)?.[BODY_FIELD[key]];
    const day = tokyoDay(s.start);
    const x = finite(ctx) ? ctx : day ? byDay.get(day) ?? null : null;
    if (finite(x)) out.push({ x: key === "sleep" ? x / 60 : x, y });
  }
  return out;
}

export function pearson(points: DriverPoint[]): number | null {
  const n = points.length;
  if (n < 3) return null;
  const mx = points.reduce((a, p) => a + p.x, 0) / n, my = points.reduce((a, p) => a + p.y, 0) / n;
  let sxy = 0, sxx = 0, syy = 0;
  for (const p of points) { sxy += (p.x - mx) * (p.y - my); sxx += (p.x - mx) ** 2; syy += (p.y - my) ** 2; }
  return sxx === 0 || syy === 0 ? null : sxy / Math.sqrt(sxx * syy);
}

/** Trend sentence at 15+ matched sessions; otherwise the collecting-data counter. */
export function trendText(points: DriverPoint[], driver: string): string {
  const n = points.length;
  if (n < MIN_TREND_SESSIONS) return `Collecting data: ${n}/${MIN_TREND_SESSIONS} sessions`;
  const r = pearson(points);
  if (r === null || Math.abs(r) < 0.2) return `No clear link between ${driver} and focus (correlation, not causation).`;
  const strength = Math.abs(r) >= 0.5 ? "strongly" : "loosely";
  const dir = r > 0 ? "higher" : "lower";
  return `Focus tends to be ${dir} when ${driver} is higher, ${strength} (r = ${r.toFixed(2)}; correlation, not causation).`;
}

/* ───────────────────────────── Typed mocks ───────────────────────────────
 * Sample data until Integrations' noop_query lands (tests and previews only; never shown as real).
 * ───────────────────────────────────────────────────────────────────────── */

const mockDay = (i: number): string => new Date(Date.UTC(2026, 9, 7) + i * 86_400_000).toISOString().slice(0, 10);
const wave = (base: number, swing: number, i: number) => Math.round(base + Math.sin(i * 1.3) * swing);

/** A DayRow in the real shape (all metric keys present, nullable). */
export function mockDayRow(day: string, over: Partial<NoopDayRow> = {}): NoopDayRow {
  return { day, source: "my-whoop", totalSleepMin: 440, efficiency: 91, deepMin: 90, remMin: 100, lightMin: 230, disturbances: 3,
    restingHr: 52, avgHrv: 62, recovery: 68, strain: 11, exerciseCount: 1, spo2Pct: 97, skinTempDevC: 0.2, respRateBpm: 14.6,
    steps: 8000, activeKcalEst: 450, ...over };
}

/** health_snapshot payload in the real shape (noop-shapes-1104), fake values. */
export function mockSnapshot(days = 29): NoopSnapshotPayload {
  const rows = Array.from({ length: days }, (_, i) => mockDayRow(mockDay(i), { avgHrv: wave(62, 6, i), recovery: wave(68, 12, i), totalSleepMin: wave(440, 35, i), restingHr: wave(52, 2, i), strain: wave(11, 3, i) }));
  const today = rows[rows.length - 1] ?? null;
  return {
    generatedAt: "2026-11-04T08:10:00+09:00", logicalToday: today?.day ?? "2026-11-04",
    sources: { whoopImported: "my-whoop", noopComputed: "my-whoop-noop", appleHealth: null, nutrition: null, mood: null, journal: null },
    freshness: { latestHeartRateSample: null, latestDailyMetricDay: today?.day ?? null, latestAppleHealthDay: null, dailyRows: rows.length, appleDailyRows: 0 },
    today, recentDays: rows.slice(0, -1), appleDaily: [], latestSleep: null,
    latestNight: today ? { day: today.day, startTs: 0, endTs: 0, start: "", end: "", sessionCount: 1, totalSleepMin: today.totalSleepMin ?? 0, inBedMin: 470, efficiency: 91, deepMin: 90, remMin: 100, lightMin: 230 } : null,
    battery: null,
  };
}

/** metric_series payload in the real shape. */
export function mockSeriesPayload(key: NoopMetricKey, values: number[]): NoopSeriesPayload {
  const points = values.map((value, i) => ({ day: mockDay(i), key, value, source: "my-whoop", sourceKey: key }));
  return { key, requestedSource: "my-whoop", range: { from: mockDay(0), to: mockDay(Math.max(values.length - 1, 0)) }, resolution: { candidates: [{ source: "my-whoop", key }], usedSources: values.length ? ["my-whoop"] : [] }, returned: points.length, points };
}
