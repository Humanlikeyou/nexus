/**
 * Pure data → geometry helpers for the Mind–Body Three.js charts (decision 0045/0049). No DOM, no three:
 * everything here is unit-tested, and the drawing code only turns these numbers into triangles.
 */
import { niceTicks } from "../../ui/charts/chartMath";

export type Usual = { low: number; high: number; median: number };
export type Box = { x0: number; x1: number; y0: number; y1: number };
export type Kind = "line" | "bar";

/** The same plot margins TrendChart uses, so a chart swaps in without moving anything. */
export const PLOT = { gutter: 34, right: 6, top: 8, axis: 18, hiddenAxis: 4, mark: 10 } as const;

export function plotBox(w: number, h: number, opts: { hideX?: boolean; markers?: boolean } = {}): Box {
  const bottom = (opts.hideX ? PLOT.hiddenAxis : PLOT.axis) + (opts.markers ? PLOT.mark : 0);
  return { x0: PLOT.gutter, x1: Math.max(PLOT.gutter + 1, w - PLOT.right), y0: PLOT.top, y1: Math.max(PLOT.top + 1, h - bottom) };
}

const finite = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);

/** Value domain and round ticks. A fixed domain is kept only while it holds every value and the usual band. */
/** niceTicks can stop short of the data (e.g. 25/50/75 for a 90 reading): widen by whole steps so nothing is clipped. */
function cover(ticks: number[], min: number, max: number): number[] {
  if (ticks.length < 2) return ticks;
  const step = ticks[1] - ticks[0], out = [...ticks];
  for (let g = 0; out[out.length - 1] < max - 1e-9 && g < 8; g++) out.push(Number((out[out.length - 1] + step).toFixed(6)));
  for (let g = 0; out[0] > min + 1e-9 && g < 8; g++) out.unshift(Number((out[0] - step).toFixed(6)));
  return out;
}

export function seriesDomain(values: readonly (number | null)[], usual: Usual | null, kind: Kind, domain?: [number, number], fixedTicks?: number[]): { lo: number; hi: number; ticks: number[] } | null {
  const nums = values.filter(finite);
  if (nums.length === 0) return null;
  const all = [...nums, ...(usual ? [usual.low, usual.high] : [])];
  if (domain) {
    if (Math.min(...all, domain[0]) >= domain[0] && Math.max(...all, domain[1]) <= domain[1]) return { lo: domain[0], hi: domain[1], ticks: fixedTicks ?? niceTicks(domain[0], domain[1]) };
    const t = cover(niceTicks(Math.min(domain[0], ...all), Math.max(domain[1], ...all)), Math.min(domain[0], ...all), Math.max(domain[1], ...all));
    return { lo: t[0], hi: t[t.length - 1], ticks: t };
  }
  const t = cover(niceTicks(kind === "bar" ? 0 : Math.min(...all), Math.max(...all)), kind === "bar" ? 0 : Math.min(...all), Math.max(...all));
  return { lo: t[0] ?? 0, hi: t[t.length - 1] ?? 1, ticks: t };
}

export const barLayout = (n: number, box: Box) => { const slot = (box.x1 - box.x0) / Math.max(1, n); return { slot, bw: slot * 0.62 }; };
export const xOf = (i: number, n: number, box: Box, kind: Kind) => kind === "bar" ? box.x0 + ((box.x1 - box.x0) / n) * (i + 0.5) : box.x0 + (n <= 1 ? (box.x1 - box.x0) / 2 : ((box.x1 - box.x0) * i) / (n - 1));
export const yOf = (v: number, lo: number, hi: number, box: Box) => box.y1 - ((Math.max(lo, Math.min(hi, v)) - lo) / (hi - lo || 1)) * (box.y1 - box.y0);

/** Runs of consecutive readings (index lists). A missing day ends a run, so the line never bridges a gap. */
export function gapRuns(values: readonly (number | null)[]): number[][] {
  const runs: number[][] = [];
  let cur: number[] = [];
  values.forEach((v, i) => { if (finite(v)) cur.push(i); else if (cur.length) { runs.push(cur); cur = []; } });
  if (cur.length) runs.push(cur);
  return runs;
}

export const isOutside = (v: number, usual: Usual | null) => !!usual && (v < usual.low || v > usual.high);

/** The slot with a reading nearest to pointer x (px, relative to the chart). */
export function nearestIndex(px: number, values: readonly (number | null)[], box: Box, kind: Kind): number | null {
  let best = -1, dist = Infinity;
  values.forEach((v, i) => { if (!finite(v)) return; const d = Math.abs(xOf(i, values.length, box, kind) - px); if (d < dist) { dist = d; best = i; } });
  return best >= 0 ? best : null;
}

/** Next slot with a reading in a direction (keyboard), or the same slot at the ends. */
export function stepIndex(from: number | null, dir: 1 | -1, values: readonly (number | null)[]): number | null {
  const idx = values.map((v, i) => (finite(v) ? i : -1)).filter((i) => i >= 0);
  if (idx.length === 0) return null;
  if (from === null) return dir > 0 ? idx[0] : idx[idx.length - 1];
  const next = dir > 0 ? idx.find((i) => i > from) : [...idx].reverse().find((i) => i < from);
  return next ?? from;
}

/**
 * A usual band as triangles (x,y pairs, 3 vertices each): one strip per consecutive pair of columns, so a band
 * that varies per day (or has gaps) is drawn exactly. `low`/`high` are pixel y values per column; null breaks the strip.
 */
export function bandTriangles(xs: readonly number[], high: readonly (number | null)[], low: readonly (number | null)[]): number[] {
  const out: number[] = [];
  for (let i = 0; i + 1 < xs.length; i++) {
    const a = high[i], b = high[i + 1], c = low[i], d = low[i + 1];
    if (a == null || b == null || c == null || d == null) continue;
    out.push(xs[i], a, xs[i + 1], b, xs[i], c, xs[i + 1], b, xs[i + 1], d, xs[i], c);
  }
  return out;
}

/** Monotone cubic (Fritsch–Carlson) sampled to points, so a smoothed line never overshoots a reading. */
export function monotoneSample(points: ReadonlyArray<readonly [number, number]>, per = 8): Array<[number, number]> {
  const n = points.length;
  if (n < 3) return points.map((p) => [p[0], p[1]]);
  const dx: number[] = [], m: number[] = [], t: number[] = new Array(n);
  for (let i = 0; i < n - 1; i++) { dx[i] = points[i + 1][0] - points[i][0] || 1e-9; m[i] = (points[i + 1][1] - points[i][1]) / dx[i]; }
  t[0] = m[0]; t[n - 1] = m[n - 2];
  for (let i = 1; i < n - 1; i++) t[i] = m[i - 1] * m[i] <= 0 ? 0 : (3 * (dx[i - 1] + dx[i])) / ((2 * dx[i] + dx[i - 1]) / m[i - 1] + (dx[i] + 2 * dx[i - 1]) / m[i]);
  const out: Array<[number, number]> = [[points[0][0], points[0][1]]];
  for (let i = 0; i < n - 1; i++) {
    const h = dx[i];
    for (let s = 1; s <= per; s++) {
      const u = s / per, u2 = u * u, u3 = u2 * u;
      const h00 = 2 * u3 - 3 * u2 + 1, h10 = u3 - 2 * u2 + u, h01 = -2 * u3 + 3 * u2, h11 = u3 - u2;
      out.push([points[i][0] + h * u, h00 * points[i][1] + h10 * h * t[i] + h01 * points[i + 1][1] + h11 * h * t[i + 1]]);
    }
  }
  return out;
}

/** Readiness: a driver's distance from his usual, as a reach from the centre line (capped at 2.5 sd). */
export const deltaReach = (z: number, half: number) => (Math.min(Math.abs(z), 2.5) / 2.5) * half;
/** The usual zone is ±1 sd on the same scale. */
export const usualZoneHalf = (half: number) => (1 / 2.5) * half;

/** Training load ratio dial: scale 0.5 to 1.8, sweet spot 0.8 to 1.3 (same thresholds as loadRatio's sentence). */
export const RATIO_MIN = 0.5, RATIO_MAX = 1.8, SWEET_LOW = 0.8, SWEET_HIGH = 1.3;
export const ratioFraction = (r: number) => Math.max(0, Math.min(1, (r - RATIO_MIN) / (RATIO_MAX - RATIO_MIN)));
export type RatioZone = "light" | "sweet" | "high";
export const ratioZone = (r: number): RatioZone => (r > SWEET_HIGH ? "high" : r < SWEET_LOW ? "light" : "sweet");
export const ZONE_WORDS: Record<RatioZone, string> = { light: "Lighter than usual", sweet: "Close to usual", high: "Above usual" };

/** The last 28 days of strain with trailing 7- and 28-day averages (same rule as the SVG chart). */
export function loadSeries(points: ReadonlyArray<{ day: string; value: number | null }>) {
  const vals = [...points].sort((a, b) => a.day.localeCompare(b.day)).slice(-28);
  const avg = (i: number, w: number) => { const xs = vals.slice(Math.max(0, i - w + 1), i + 1).map((p) => p.value).filter(finite); return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0; };
  const nums = vals.map((p) => p.value).filter(finite);
  return { vals, a7: vals.map((_, i) => avg(i, 7)), a28: vals.map((_, i) => avg(i, 28)), max: Math.max(...nums, 1), count: nums.length };
}
