/**
 * Milestones arrival: each goal is a struck string, fixed at both ends, vibrating in its own key.
 *
 * Real string physics (ideal string, no damping):
 *   y(u, t) = Σₙ aₙ · sin(nπu) · cos(2π·n·f·t + φₙ),   u ∈ [0, 1] along the string
 * Harmonic n vibrates at n times the string's fundamental f. Struck at p, a string's harmonics have
 *   aₙ ∝ sin(nπp) / n^1.3   (between a pluck's 1/n² and an ideal point strike's 1/n),
 * so where it was struck decides its timbre: which overtones ring and which are silent (nodes at p).
 *
 * Each goal gets its own fundamental, a golden-ratio step from the last (no two strings share a period,
 * so they drift in and out of phase and weave), its own pluck point, and golden-angle phase offsets.
 * The renderer (TangleWaves.tsx) evaluates the same formula in a vertex shader; this module is the
 * single source of the numbers and is what the tests check.
 */
import { GOLDEN_ANGLE, PHI } from "./golden";

/** Harmonics per string (the shader's array size). */
export const HARMONICS = 8;
/** Lowest fundamental, in Hz of screen time: the strings sway slowly while their overtones shimmer. */
/** Harmonic falloff exponent (see above). */
export const FALLOFF = 1.3;
export const BASE_HZ = 0.25;
/** Strike points of the first four strings: away from the middle so even harmonics ring too. */
const PLUCK = [0.27, 0.38, 0.62, 0.73];

export type StringMode = { n: number; amp: number; phase: number };
/** gain scales the swing (1 = full); Neuro strings use it for how strong the band was. */
export type GoalString = { f: number; pluck: number; modes: StringMode[]; gain?: number };

const frac = (x: number) => x - Math.floor(x);

/**
 * Where goal i's string sits: the fundamental's exponent e (f = BASE_HZ · φ^e, e in [0, 3]) and its strike point.
 * Goals 0-3 are the four approved strings (e = 0, 1, 2, 3). Every later goal gets its own: e from the golden
 * sequence frac(i·φ), which never repeats, kept clear of the whole-number exponents the first four use, and its
 * own strike point, kept off the middle (a middle strike silences every even harmonic).
 */
function placement(i: number): { e: number; p: number } {
  if (i < 4) return { e: i, p: PLUCK[i] };
  let e = 3 * frac(i * PHI);
  if (Math.abs(e - Math.round(e)) < 0.12) e = Math.min(3, Math.max(0, e + (e >= Math.round(e) ? 0.24 : -0.24)));
  let p = 0.2 + 0.6 * frac(i * PHI * PHI);
  if (Math.abs(p - 0.5) < 0.05) p += 0.1;
  return { e, p };
}

/** The string for goal i. Every goal has its own; the strand colour repeats every 4, the string never does. */
export function goalString(i: number): GoalString {
  const idx = Math.max(0, Math.floor(i));
  const { e, p } = placement(idx);
  const f = BASE_HZ * Math.pow(PHI, e);
  const raw = Array.from({ length: HARMONICS }, (_, j) => {
    const n = j + 1;
    return { n, amp: Math.sin(n * Math.PI * p) / Math.pow(n, FALLOFF), phase: (idx * GOLDEN_ANGLE * n) % (2 * Math.PI) };
  });
  // Normalise so the largest possible swing (all harmonics in phase) is 1.
  const sum = raw.reduce((s, m) => s + Math.abs(m.amp), 0) || 1;
  return { f, pluck: p, modes: raw.map((m) => ({ ...m, amp: m.amp / sum })) };
}

/** Harmonics of a string struck at p, phases offset by a golden angle per seed, normalised to a swing of 1. */
function modesFor(p: number, seed: number): StringMode[] {
  const raw = Array.from({ length: HARMONICS }, (_, j) => {
    const n = j + 1;
    return { n, amp: Math.sin(n * Math.PI * p) / Math.pow(n, FALLOFF), phase: (seed * GOLDEN_ANGLE * n) % (2 * Math.PI) };
  });
  const sum = raw.reduce((s, m) => s + Math.abs(m.amp), 0) || 1;
  return raw.map((m) => ({ ...m, amp: m.amp / sum }));
}

/* ---------- Neuro: one string per brainwave band of the most recent Muse session (decision 0041) ---------- */

export type BandKey = "delta" | "theta" | "alpha" | "beta" | "gamma";
/** Slow-to-fast, with each band's centre frequency in Hz (the ranges the Neuro section shows). */
export const BAND_CENTRES: ReadonlyArray<{ key: BandKey; hz: number }> = [
  { key: "delta", hz: 2.5 }, { key: "theta", hz: 6 }, { key: "alpha", hz: 10.5 }, { key: "beta", hz: 21.5 }, { key: "gamma", hz: 37 },
];
/** Brain frequencies are shown 40x slower, so the ratios between bands are real and the motion is calm. */
export const EEG_SLOWDOWN = 40;
/** Every Neuro string makes a whole number of cycles in this many seconds, so the picture loops exactly. */
export const LOOP_S = 60;
const BAND_PLUCK: Record<BandKey, number> = { delta: 0.3, theta: 0.36, alpha: 0.64, beta: 0.7, gamma: 0.24 };

/** A band's string: its real centre frequency slowed down, rounded to whole cycles per loop; gain from its strength. */
export function bandString(key: BandKey, share: number): GoalString {
  const i = BAND_CENTRES.findIndex((b) => b.key === key);
  const cycles = Math.max(1, Math.round((BAND_CENTRES[i].hz / EEG_SLOWDOWN) * LOOP_S));
  const s = Math.max(0, Math.min(1, Number.isFinite(share) ? share : 0));
  return { f: cycles / LOOP_S, pluck: BAND_PLUCK[key], modes: modesFor(BAND_PLUCK[key], i), gain: 0.3 + 0.7 * Math.sqrt(s) };
}

/**
 * The Neuro strings for the most recent session: every band with a measured average, its swing set by its power
 * relative to the strongest band. A session without band data falls back to calm (alpha) and focus (beta), sized
 * by their 0-100 scores. Nothing measured at all: no strings.
 */
export function neuroStrings(input: { bandAvg?: Partial<Record<BandKey, number>> | null; calm?: number | null; focus?: number | null }): Array<{ key: BandKey; string: GoalString }> {
  const avg = input.bandAvg ?? {};
  const have = BAND_CENTRES.filter((b) => { const v = avg[b.key]; return typeof v === "number" && Number.isFinite(v) && v >= 0; });
  const top = Math.max(0, ...have.map((b) => avg[b.key] as number));
  if (have.length && top > 0) return have.map((b) => ({ key: b.key, string: bandString(b.key, (avg[b.key] as number) / top) }));
  const sc = (v: number | null | undefined) => (typeof v === "number" && Number.isFinite(v) ? Math.max(0, Math.min(100, v)) / 100 : null);
  const out: Array<{ key: BandKey; string: GoalString }> = [];
  const calm = sc(input.calm), focus = sc(input.focus);
  if (calm != null) out.push({ key: "alpha", string: bandString("alpha", calm) });
  if (focus != null) out.push({ key: "beta", string: bandString("beta", focus) });
  return out;
}

/** Displacement at u (0..1) and time t (s), in units of the maximum swing (|y| ≤ 1). */
export function displacement(s: GoalString, u: number, t: number): number {
  let y = 0;
  for (const m of s.modes) y += m.amp * Math.sin(m.n * Math.PI * u) * Math.cos(2 * Math.PI * m.n * s.f * t + m.phase);
  return y;
}

/** The time after which string i's shape repeats exactly: its fundamental period 1/f. */
export function period(s: GoalString): number {
  return 1 / s.f;
}

/* ---------- Milestones motion: random drift with occasional harmony (Taisun 2026-11-19) ---------- */

/** Small seeded PRNG (mulberry32), so a page's randomness is its own and tests are repeatable. */
export function seeded(seed: number): () => number {
  let a = seed | 0;
  return () => { a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

const TAU = 2 * Math.PI;
const wrapPi = (x: number) => x - TAU * Math.round(x / TAU);
const smooth = (x: number) => { const c = Math.max(0, Math.min(1, x)); return c * c * (3 - 2 * c); };

/** Harmony timing (seconds): a random gap, a glide in, a hold, a glide out. */
export const HARMONY = { firstMin: 15, firstMax: 35, gapMin: 25, gapMax: 55, rise: 3, hold: 5, fall: 4 } as const;
/** How far a string's frequency wanders (in log-frequency, about ±50 %), and how hard harmony pulls phases into step. */
export const WANDER = 0.42, PULL = 1.6;

/**
 * Each string wanders through its own smooth random frequency (three slow incommensurate sines per string,
 * random rates and phases), so the tangle never repeats. At random moments the strings glide into whole-number
 * ratios of one fundamental (1:2:3...) and a phase pull (like coupled oscillators) brings them into step, so the
 * tangle resolves into one chord; then it lets go. Phases only ever advance continuously, so nothing jumps.
 */
export class DriftField {
  t = 0;
  /** 0 = free and random, 1 = fully in harmony. */
  harmony = 0;
  /** Each string's phase, radians, wrapped to [-π, π]. */
  phases: number[];
  /** Each string's frequency right now (Hz). */
  freqs: number[];
  /** Start times of the harmony events so far (s). */
  events: number[] = [];
  private theta = 0;
  private nextAt: number;
  private wander: Array<Array<{ rate: number; ph: number; w: number }>>;
  constructor(private base: GoalString[], seed = (Math.random() * 2 ** 31) | 0) {
    const r = seeded(seed);
    this.phases = base.map(() => (r() * 2 - 1) * Math.PI);
    this.freqs = base.map((s) => s.f);
    this.wander = base.map(() => [0, 1, 2].map((k) => ({ rate: 0.011 + r() * 0.03 * (k + 1) / 2, ph: r() * TAU, w: [0.55, 0.3, 0.15][k] })));
    this.rand = r;
    this.nextAt = HARMONY.firstMin + r() * (HARMONY.firstMax - HARMONY.firstMin);
  }
  private rand: () => number;
  /** The harmonic each string locks to: 1, 2, 3 ... 6, then round again. */
  static ratio(i: number) { return 1 + (i % 6); }
  /** The shared fundamental while in harmony: the first string's own base frequency. */
  get fundamental() { return this.base[0]?.f ?? BASE_HZ; }

  private envelope(): number {
    const s = this.events[this.events.length - 1];
    if (s == null) return 0;
    const x = this.t - s, { rise, hold, fall } = HARMONY;
    if (x < rise) return smooth(x / rise);
    if (x < rise + hold) return 1;
    return 1 - smooth((x - rise - hold) / fall);
  }

  step(dt: number): this {
    const d = Math.max(0, Math.min(0.1, dt)); // a stalled frame never lurches
    this.t += d;
    if (this.t >= this.nextAt) {
      this.events.push(this.nextAt);
      const { rise, hold, fall, gapMin, gapMax } = HARMONY;
      this.nextAt = this.nextAt + rise + hold + fall + gapMin + this.rand() * (gapMax - gapMin);
    }
    const h = (this.harmony = this.envelope());
    const F = this.fundamental;
    // Pull toward where the shared clock is now, then advance it: a string in lock keeps exactly in step.
    this.base.forEach((s, i) => {
      const noise = this.wander[i].reduce((a, w) => a + w.w * Math.sin(TAU * w.rate * this.t + w.ph), 0);
      const free = s.f * Math.exp(WANDER * noise);
      const k = DriftField.ratio(i);
      const f = (this.freqs[i] = free * (1 - h) + F * k * h);
      const pull = h * PULL * wrapPi(k * this.theta - this.phases[i]);
      this.phases[i] = wrapPi(this.phases[i] + TAU * f * d + pull * d);
    });
    this.theta = wrapPi(this.theta + TAU * F * d);
    return this;
  }

  /** How far string i is from its harmonic lock (radians, 0 = locked). */
  offLock(i: number) { return Math.abs(wrapPi(DriftField.ratio(i) * this.theta - this.phases[i])); }
}
