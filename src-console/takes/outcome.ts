/**
 * Outcome evaluation, method v1 (HANDOFF-1111t §5, with the §10 change: at least 5 days each way).
 * Pure and deterministic: the 90% range comes from a seeded bootstrap, so the same data gives the same words.
 */
export type Stat = { n: number; mean: number | null; median: number | null; sd: number | null };
export type OutcomeVerdict = "too_early" | "none" | "better" | "worse" | "unclear";
export const MIN_EACH = 5;
export const RESAMPLES = 2000;

export type OutcomeInput = {
  seed: string; better: "up" | "down"; wobbleFloor: number; minEach?: number; resamples?: number;
  withVals: number[]; withoutVals: number[]; beforeVals: number[];
  /** The metric over the 28 days before the first suggestion (for the wobble). */
  baseline28: number[];
  confounders?: Array<{ metric: string; withVals: number[]; withoutVals: number[] }>;
  /** Share of weekend days in each group, 0..1. */
  weekend?: { with: number; without: number } | null;
  excluded: { unknown: number; notToday: number; noMeasurement: number };
};
export type OutcomeResult = {
  groups: { with: Stat; without: Stat; before: Stat };
  diff: number | null; ci90: [number, number] | null; wobble: number; verdict: OutcomeVerdict;
  confounders: Array<{ metric: string; with: number; without: number; flagged: boolean }>; weekendFlagged: boolean;
  excluded: OutcomeInput["excluded"];
};

const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
const sd = (xs: number[]) => { const m = mean(xs); return Math.sqrt(xs.reduce((a, b) => a + (b - m) ** 2, 0) / xs.length); };
const median = (xs: number[]) => { const s = [...xs].sort((a, b) => a - b); const h = s.length >> 1; return s.length % 2 ? s[h] : (s[h - 1] + s[h]) / 2; };
export const stat = (xs: number[]): Stat => (xs.length ? { n: xs.length, mean: mean(xs), median: median(xs), sd: sd(xs) } : { n: 0, mean: null, median: null, sd: null });

/** Small seeded PRNG (xmur3 seed, mulberry32 stream). */
function rng(seed: string) {
  let h = 1779033703 ^ seed.length;
  for (let i = 0; i < seed.length; i++) { h = Math.imul(h ^ seed.charCodeAt(i), 3432918353); h = (h << 13) | (h >>> 19); }
  let a = (Math.imul(h ^ (h >>> 16), 2246822507) ^ Math.imul(h ^ (h >>> 13), 3266489909)) >>> 0;
  return () => { a = (a + 0x6d2b79f5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

export function bootstrapDiffCI90(withVals: number[], withoutVals: number[], seed: string, resamples = RESAMPLES): [number, number] {
  const r = rng(seed); const diffs: number[] = [];
  const draw = (xs: number[]) => { let s = 0; for (let i = 0; i < xs.length; i++) s += xs[(r() * xs.length) | 0]; return s / xs.length; };
  for (let i = 0; i < resamples; i++) diffs.push(draw(withVals) - draw(withoutVals));
  diffs.sort((a, b) => a - b);
  return [diffs[Math.floor(0.05 * (resamples - 1))], diffs[Math.ceil(0.95 * (resamples - 1))]];
}

export function computeOutcome(i: OutcomeInput): OutcomeResult {
  const minEach = i.minEach ?? MIN_EACH;
  const groups = { with: stat(i.withVals), without: stat(i.withoutVals), before: stat(i.beforeVals) };
  const wobble = Math.max(i.wobbleFloor, i.baseline28.length >= 2 ? 0.5 * sd(i.baseline28) : 0);
  const conf = (i.confounders ?? []).filter((c) => c.withVals.length >= 2 && c.withoutVals.length >= 2).map((c) => {
    const pooled = [...c.withVals, ...c.withoutVals]; const s = sd(pooled);
    const a = mean(c.withVals), b = mean(c.withoutVals);
    return { metric: c.metric, with: a, without: b, flagged: s > 0 && Math.abs(a - b) > 0.5 * s };
  });
  const weekendFlagged = !!i.weekend && Math.abs(i.weekend.with - i.weekend.without) > 0.3;
  const base = { groups, wobble, confounders: conf, weekendFlagged, excluded: i.excluded };
  if (i.withVals.length < minEach || i.withoutVals.length < minEach) return { ...base, diff: null, ci90: null, verdict: "too_early" };
  const diff = (groups.with.mean as number) - (groups.without.mean as number);
  const ci90 = bootstrapDiffCI90(i.withVals, i.withoutVals, i.seed, i.resamples ?? RESAMPLES);
  if (conf.some((c) => c.flagged) || weekendFlagged) return { ...base, diff, ci90, verdict: "unclear" };
  const up = ci90[0] > 0, down = ci90[1] < 0;
  const big = Math.abs(diff) > wobble;
  const good = i.better === "up" ? up : down, bad = i.better === "up" ? down : up;
  return { ...base, diff, ci90, verdict: big && good ? "better" : big && bad ? "worse" : "none" };
}
