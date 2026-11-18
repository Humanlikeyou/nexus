/**
 * design-1118a: the golden-ratio character of the new pages, in code only (no pictures).
 * Felt in frequencies, amplitudes, golden-angle phases, Fibonacci periods, and layout tokens.
 */
export const PHI = (1 + Math.sqrt(5)) / 2;
/** 137.5°: the angle between successive sunflower seeds. */
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
export const FIB = [1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89] as const;

/** Wave term i of a φ wave: frequency k·φ^i, amplitude a/φ^(2i), golden-angle phase. */
export function phiWave(t: number, k: number, amp: number, seed: number, terms = 2): number {
  let y = 0;
  for (let i = 0; i < terms; i++) y += (amp / Math.pow(PHI, i * 2)) * Math.sin(t * Math.PI * k * Math.pow(PHI, i) + seed * GOLDEN_ANGLE * (i + 1));
  return y;
}

/** Fraction of a cycle equal to `k` golden angles, for animation-delay (k · 137.5° / 360°). */
export function goldenPhase(k: number): number {
  return (k * GOLDEN_ANGLE) / (2 * Math.PI);
}
