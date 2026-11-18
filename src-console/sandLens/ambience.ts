/**
 * design-1118b step c: bio-reactive ambience. Pure mapping from the time of day and REAL WHOOP recovery
 * (via the existing NOOP adapter) to light, warmth and pace. Recovery missing → a neutral fallback;
 * nothing is ever invented. Calm turns the bio-reactive shifts off.
 */
export type Ambience = {
  energy: number; warmth: number; dim: number; crisp: number;
  lx: number; ly: number; lc: [number, number, number]; li: number;
  bg: [[number, number, number], [number, number, number], [number, number, number]];
  source: "recovery" | "neutral";
};
const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
type C3 = [number, number, number];
const mixc = (a: C3, b: C3, t: number): C3 => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];

export function targetAmbience(todMinutes: number, recovery: number | null, calm: boolean): Ambience {
  const tod = todMinutes / 60;
  const e = Math.sin((Math.PI * (tod - 6)) / 12);
  const day = clamp(e * 1.4 + 0.15, 0, 1), low = day > 0 ? clamp(1 - e * 2.2, 0, 1) : 0;
  const night = 1 - clamp((e + 0.12) * 4, 0, 1);
  let lx = clamp((tod - 5) / 14, 0.05, 0.95), ly = 0.62 - 0.55 * clamp(e, 0, 1);
  if (night > 0.5) { lx = 0.35; ly = 0.05; }
  let lc = mixc([1.0, 0.97, 0.9], [1.0, 0.62, 0.3], low); lc = mixc(lc, [0.4, 0.8, 0.62], night);
  const li = lerp(0.13 + 0.06 * clamp(e, 0, 1) + 0.05 * low, 0.05, night);
  const late = clamp((tod - 20) / 3, 0, 1) + (tod < 5 ? 1 : 0);
  const rec = calm || recovery == null || !Number.isFinite(recovery) ? null : clamp(recovery, 0, 100);
  const r = rec == null ? 0.6 : rec / 100;
  const lateK = calm ? 0 : clamp(late, 0, 1);
  const energy = clamp((rec == null ? 1 : 0.7 + 0.5 * r) - 0.15 * lateK, 0.7, 1.2);
  const warmth = rec == null ? 0.1 * lateK : clamp((1 - r) * 0.75 + 0.45 * lateK, 0, 1);
  const dim = rec == null ? 0.25 * lateK : clamp((1 - r) * 0.4 + 0.3 * lateK, 0, 0.6);
  const crisp = rec == null ? 0.5 : r;
  let a = mixc([33, 64, 47], [14, 36, 32], night), b = mixc([18, 48, 34], [8, 25, 22], night), c = mixc([10, 31, 22], [4, 15, 13], night);
  const amber: C3 = [92, 70, 30];
  a = mixc(a, amber, 0.24 * low + 0.3 * warmth); b = mixc(b, amber, 0.12 * low + 0.18 * warmth); c = mixc(c, amber, 0.05 * low + 0.06 * warmth);
  a = a.map((v) => v * (1 - dim * 0.6)) as C3; b = b.map((v) => v * (1 - dim * 0.6)) as C3; c = c.map((v) => v * (1 - dim * 0.5)) as C3;
  return { energy, warmth, dim, crisp, lx, ly, lc, li: li * (1 - dim * 0.5), bg: [a, b, c], source: rec == null ? "neutral" : "recovery" };
}

export const hex = (c: C3) => "#" + c.map((v) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, "0")).join("");

/** CSS custom properties for the light/depth tokens (step b) and the ambience colours (step c). */
export function ambienceVars(a: Ambience, withColour: boolean): Record<string, string> {
  const dx = a.lx - 0.5, dy = a.ly - 0.5, L = Math.hypot(dx, dy) || 1;
  const vars: Record<string, string> = {
    "--light-x": `${(a.lx * 100).toFixed(0)}%`, "--light-y": `${(a.ly * 100).toFixed(0)}%`,
    "--light-angle": `${((Math.atan2(dx, -dy) * 180) / Math.PI).toFixed(0)}deg`,
    "--shx": `${((-dx / L) * 10).toFixed(0)}px`, "--shy": `${((-dy / L) * 12 + 4).toFixed(0)}px`,
    "--edge": `rgba(${a.lc.map((v) => Math.round(v * 255)).join(",")},${((0.25 + 0.3 * a.crisp) * (1 - a.dim * 0.5)).toFixed(2)})`,
  };
  if (withColour) { vars["--bg-a"] = hex(a.bg[0]); vars["--bg-b"] = hex(a.bg[1]); vars["--bg-c"] = hex(a.bg[2]); }
  return vars;
}
