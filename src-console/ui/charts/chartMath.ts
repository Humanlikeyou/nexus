/**
 * design-1110c chart vocabulary (C1–C5): the maths, kept pure so it is unit-tested.
 * Monotone cubic (Fritsch–Carlson) curves never overshoot the data, so a smoothed line cannot
 * invent a peak or a dip that the readings don't have.
 */
export type Pt = readonly [number, number];

const f1 = (n: number) => (Math.round(n * 10) / 10).toString();

/** SVG path through the points as a monotone cubic. One point: nothing; two: a straight line. */
export function monotonePath(points: readonly Pt[]): string {
  const n = points.length;
  if (n < 2) return "";
  const [x0, y0] = points[0];
  if (n === 2) return `M${f1(x0)},${f1(y0)}L${f1(points[1][0])},${f1(points[1][1])}`;
  const dx: number[] = [], m: number[] = [], t: number[] = [];
  for (let i = 0; i < n - 1; i++) { dx[i] = points[i + 1][0] - points[i][0] || 1e-9; m[i] = (points[i + 1][1] - points[i][1]) / dx[i]; }
  t[0] = m[0]; t[n - 1] = m[n - 2];
  for (let i = 1; i < n - 1; i++) {
    t[i] = m[i - 1] * m[i] <= 0 ? 0 : (3 * (dx[i - 1] + dx[i])) / ((2 * dx[i] + dx[i - 1]) / m[i - 1] + (dx[i] + 2 * dx[i - 1]) / m[i]);
  }
  let d = `M${f1(x0)},${f1(y0)}`;
  for (let i = 0; i < n - 1; i++) {
    const h = dx[i] / 3;
    const [xa, ya] = points[i], [xb, yb] = points[i + 1];
    d += `C${f1(xa + h)},${f1(ya + t[i] * h)} ${f1(xb - h)},${f1(yb - t[i + 1] * h)} ${f1(xb)},${f1(yb)}`;
  }
  return d;
}

/** Centred rolling mean over `k` values (edges use what exists). Nulls are skipped, and stay null. */
export function rollingMean(values: readonly (number | null)[], k = 3): (number | null)[] {
  const h = Math.floor(k / 2);
  return values.map((v, i) => {
    if (v == null) return null;
    const near = values.slice(Math.max(0, i - h), i + h + 1).filter((x): x is number => x != null);
    return near.reduce((s, x) => s + x, 0) / near.length;
  });
}

export function quantile(values: readonly number[], q: number): number {
  const b = [...values].sort((x, y) => x - y);
  if (b.length === 0) return NaN;
  const p = (b.length - 1) * q, lo = Math.floor(p);
  return b[lo] + (b[Math.min(lo + 1, b.length - 1)] - b[lo]) * (p - lo);
}

/** "Your usual": the middle half (25th–75th percentile) and the median, from at least 5 readings. */
export function usualRange(values: readonly (number | null)[]): { low: number; high: number; median: number } | null {
  const xs = values.filter((v): v is number => typeof v === "number" && Number.isFinite(v));
  if (xs.length < 5) return null;
  return { low: quantile(xs, .25), high: quantile(xs, .75), median: quantile(xs, .5) };
}

/** Two or three round ticks that cover the values (and the band), for hairline gridlines. */
export function niceTicks(lo: number, hi: number, count = 3): number[] {
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return [];
  if (hi - lo < 1e-9) { hi = lo + 1; }
  const raw = (hi - lo) / (count - 1);
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((s) => s * mag).find((s) => s >= raw) ?? 10 * mag;
  const start = Math.floor(lo / step) * step;
  const ticks: number[] = [];
  for (let v = start; ticks.length < count + 2 && v <= hi + step * .999; v += step) ticks.push(Number(v.toFixed(6)));
  while (ticks.length > count && ticks[ticks.length - 1] > hi && ticks[0] < lo) ticks.pop();
  return ticks;
}

/** Change vs usual in words with an arrow (never colour alone): "▲ 6 ms above usual". */
export function deltaWords(value: number | null, usual: number | null, unit = "", digits = 0): { text: string; dir: "up" | "down" | "flat" } | null {
  if (value == null || usual == null || !Number.isFinite(value) || !Number.isFinite(usual)) return null;
  const d = value - usual;
  if (Math.abs(d) < (digits ? .5 * 10 ** -digits : .5)) return { text: "≈ at your usual", dir: "flat" };
  return { text: `${d > 0 ? "▲" : "▼"} ${Math.abs(d).toFixed(digits)}${unit} ${d > 0 ? "above" : "below"} usual`, dir: d > 0 ? "up" : "down" };
}
