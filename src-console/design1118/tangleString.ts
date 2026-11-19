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
/** Strike points per string: away from the middle so even harmonics ring too. */
const PLUCK = [0.27, 0.38, 0.62, 0.73];

export type StringMode = { n: number; amp: number; phase: number };
export type GoalString = { f: number; pluck: number; modes: StringMode[] };

/** The string for goal slot i (0..3 repeats every 4, like the strand colours). */
export function goalString(i: number): GoalString {
  const k = ((i % 4) + 4) % 4;
  const f = BASE_HZ * Math.pow(PHI, k);
  const p = PLUCK[k];
  const raw = Array.from({ length: HARMONICS }, (_, j) => {
    const n = j + 1;
    return { n, amp: Math.sin(n * Math.PI * p) / Math.pow(n, FALLOFF), phase: (k * GOLDEN_ANGLE * n) % (2 * Math.PI) };
  });
  // Normalise so the largest possible swing (all harmonics in phase) is 1.
  const sum = raw.reduce((s, m) => s + Math.abs(m.amp), 0) || 1;
  return { f, pluck: p, modes: raw.map((m) => ({ ...m, amp: m.amp / sum })) };
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
