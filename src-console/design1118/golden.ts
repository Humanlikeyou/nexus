/**
 * design-1118a round 6: the golden-ratio character of the new pages' art (1110g φ system).
 * Felt, not seen: φ-related frequencies and amplitudes, golden-angle phases, and a faint
 * logarithmic golden spiral / φ grid behind the art. Pure geometry (viewBox units).
 */
export const PHI = (1 + Math.sqrt(5)) / 2;
/** 137.5°: the angle between successive sunflower seeds. */
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
export const FIB = [1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89] as const;

/**
 * Logarithmic golden spiral (grows by φ every quarter turn) centred at (cx, cy), from radius r0,
 * over `quarters` quarter turns, as an SVG polyline path.
 */
export function goldenSpiralPath(cx: number, cy: number, r0: number, quarters = 8, rotate = 0, steps = 24): string {
  const n = Math.max(1, Math.round(quarters * steps));
  let d = "";
  for (let k = 0; k <= n; k++) {
    const th = (k / steps) * (Math.PI / 2);
    const r = r0 * Math.pow(PHI, th / (Math.PI / 2));
    const x = cx + r * Math.cos(th + rotate), y = cy - r * Math.sin(th + rotate);
    d += `${k ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`;
  }
  return d;
}

/** The φ grid of a w×h box: the golden-section lines at 1/φ² and 1/φ in both directions. */
export function phiGrid(w: number, h: number): string {
  const xs = [w / (PHI * PHI), w / PHI], ys = [h / (PHI * PHI), h / PHI];
  return [...xs.map((x) => `M${x.toFixed(1)},0V${h}`), ...ys.map((y) => `M0,${y.toFixed(1)}H${w}`)].join("");
}

/** Wave term i of a φ wave: frequency k·φ^i, amplitude a/φ^i, golden-angle phase. */
export function phiWave(t: number, k: number, amp: number, seed: number, terms = 2): number {
  let y = 0;
  for (let i = 0; i < terms; i++) y += (amp / Math.pow(PHI, i * 2)) * Math.sin(t * Math.PI * k * Math.pow(PHI, i) + seed * GOLDEN_ANGLE * (i + 1));
  return y;
}

/** A spiral that fills the right golden section of a w×h box (where the eye rests), for faint underlays. */
export function underlaySpiral(w: number, h: number, quarters = 9): string {
  const cx = w / PHI, cy = h / PHI;
  // largest radius after `quarters` quarter turns reaches about the box edge
  const r0 = Math.max(w, h) * 0.62 / Math.pow(PHI, quarters);
  return goldenSpiralPath(cx, cy, r0, quarters, Math.PI);
}
