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
