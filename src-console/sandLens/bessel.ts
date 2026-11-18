/**
 * design-1118b: Bessel functions of the first kind and the circular-plate mode table (ported from the
 * Stage A mock, bessel.js). w(r, θ) = J_n(k_mn r) cos(nθ), k_mn = m-th positive zero of J_n
 * (n = 0..8, m = 1..6). J_n via the integral form J_n(x) = (1/π) ∫_0^π cos(nτ − x sin τ) dτ
 * (Simpson, 400 panels: ~1e-12 for x < 40).
 */
export function besselJ(n: number, x: number): number {
  const N = 400, h = Math.PI / N;
  let s = 1 + Math.cos(n * Math.PI - x * Math.sin(Math.PI));
  for (let i = 1; i < N; i++) {
    const t = i * h;
    s += (i % 2 ? 4 : 2) * Math.cos(n * t - x * Math.sin(t));
  }
  return (s * h) / 3 / Math.PI;
}

export function besselZeros(n: number, count: number): number[] {
  const zs: number[] = [];
  let x = 0.5, prev = besselJ(n, x);
  const step = 0.05;
  while (zs.length < count) {
    const nx = x + step, cur = besselJ(n, nx);
    if (prev === 0 || prev * cur < 0) {
      let a = x, b = nx, fa = prev;
      for (let i = 0; i < 60; i++) { const c = (a + b) / 2, fc = besselJ(n, c); if (fa * fc <= 0) b = c; else { a = c; fa = fc; } }
      zs.push((a + b) / 2);
    }
    x = nx; prev = cur;
  }
  return zs;
}

/** f_mn = F0 · k_mn / k_01 (fixed-edge circular membrane form): 80 Hz … ~983 Hz, the speaking-voice band. */
export const F0 = 80;
export const LUT_W = 1024;
export const N_MAX = 8;
export const M_MAX = 6;

export type PlateMode = { n: number; m: number; k: number; f: number; row: number; id: string };

let cache: { modes: PlateMode[]; lut: Float32Array } | null = null;

/** All 54 modes sorted by frequency, and a radial LUT (one row per mode, max |J_n(k r)| on [0,1] = 1). */
export function buildModes(): { modes: PlateMode[]; lut: Float32Array } {
  if (cache) return cache;
  const k01 = besselZeros(0, 1)[0];
  const raw: Omit<PlateMode, "row" | "id">[] = [];
  for (let n = 0; n <= N_MAX; n++) {
    const z = besselZeros(n, M_MAX);
    for (let m = 1; m <= M_MAX; m++) raw.push({ n, m, k: z[m - 1], f: (F0 * z[m - 1]) / k01 });
  }
  raw.sort((a, b) => a.f - b.f);
  const modes: PlateMode[] = raw.map((md, row) => ({ ...md, row, id: `n${md.n}m${md.m}` }));
  const lut = new Float32Array(LUT_W * modes.length);
  modes.forEach((md) => {
    let mx = 0;
    const vals = new Float32Array(LUT_W);
    for (let i = 0; i < LUT_W; i++) { const v = besselJ(md.n, (md.k * i) / (LUT_W - 1)); vals[i] = v; mx = Math.max(mx, Math.abs(v)); }
    for (let i = 0; i < LUT_W; i++) lut[md.row * LUT_W + i] = vals[i] / mx;
  });
  cache = { modes, lut };
  return cache;
}

/** Plate displacement at (r, θ) for a set of weighted modes (CPU mirror of the shader's field()). */
export function plateField(active: Array<{ mode: PlateMode; a: number }>, x: number, y: number): number {
  const r = Math.hypot(x, y), th = Math.atan2(y, x);
  let w = 0;
  for (const { mode, a } of active) w += a * (besselJ(mode.n, mode.k * Math.min(1, r)) ) * Math.cos(mode.n * th);
  return w;
}

/** Nearest mode by log-frequency distance. */
export function nearestMode(f: number, modes = buildModes().modes): PlateMode {
  let best = modes[0], bd = Infinity;
  for (const m of modes) { const d = Math.abs(Math.log(f / m.f)); if (d < bd) { bd = d; best = m; } }
  return best;
}

export type Peak = { f: number; amp: number };

/**
 * 2–3 strongest spectral peaks in 80–4000 Hz from a dB spectrum (AnalyserNode.getFloatFrequencyData).
 * Used when a spectrum of her voice is available (Needs wiring: the voice plays in the sidecar).
 */
export function pickPeaks(fd: Float32Array, binHz: number, max = 3): Peak[] {
  const lo = Math.max(1, Math.ceil(80 / binHz)), hi = Math.min(fd.length - 2, Math.floor(4000 / binHz));
  const cands: Array<[number, number]> = [];
  for (let i = lo; i <= hi; i++) { const v = fd[i]; if (v > fd[i - 1] && v >= fd[i + 1] && v > -85) cands.push([i, v]); }
  cands.sort((a, b) => b[1] - a[1]);
  const picks: Array<[number, number]> = [];
  for (const c of cands) {
    if (picks.length >= max) break;
    if (picks.length && c[1] < picks[0][1] - 24) break;
    if (picks.every((p) => Math.abs(p[0] - c[0]) >= 4)) picks.push(c);
  }
  return picks.map(([i, v]) => ({ f: i * binHz, amp: Math.pow(10, v / 10) }));
}

/**
 * Speech fallback (no spectrum in the UI: her voice is played by the sidecar and only rms/peak level
 * events reach the app). Each loudness onset advances a fixed, deterministic walk through speech-band
 * modes (≈100–1000 Hz) and blends the 2–3 nearest, loudness driving the amplitude. Documented in REPORT.
 */
export function fallbackPeaks(onsetIndex: number, modes = buildModes().modes): Peak[] {
  const band = modes.filter((m) => m.f >= 100 && m.f <= 1000 && m.n > 0);
  // A figure holds for a short phrase (4 onsets) so the grains have time to gather on its nodal lines;
  // within the phrase each syllable nudges a neighbouring mode in and out, like pitch moving in speech.
  const step = 7; // co-prime walk: successive phrases land on clearly different figures
  const phrase = Math.floor(onsetIndex / 4);
  const a = band[(phrase * step) % band.length];
  const b = band[(phrase * step + 1) % band.length];
  const c = band[(phrase * step + 3) % band.length];
  const k = onsetIndex % 4;
  return [{ f: a.f, amp: 1 }, { f: b.f, amp: k === 1 || k === 2 ? 0.4 : 0.2 }, { f: c.f, amp: k === 3 ? 0.25 : 0 }].filter((p) => p.amp > 0);
}

/** Mode scheduler: hold ≥ 250 ms, ~400 ms crossfade, at most 4 modes (shader uniforms). */
export class ModeScheduler {
  private w = new Map<number, { w: number; target: number; since: number }>();
  lastDominant: number | null = null;
  constructor(private modes = buildModes().modes) {}
  update(peaks: Peak[], loud: number, dt: number, now: number) {
    const detected = peaks.map((p) => ({ ...p, mode: nearestMode(p.f, this.modes) }));
    if (loud > 0.02 && detected.length) {
      const tot = detected.reduce((a, p) => a + p.amp, 0) || 1;
      const tgt = new Map<number, number>();
      for (const p of detected) tgt.set(p.mode.row, (tgt.get(p.mode.row) ?? 0) + p.amp / tot);
      for (const [row, e] of this.w) { const t = tgt.get(row) ?? 0; if (t === 0 && now - e.since < 250) continue; if (e.target === 0 && t > 0) e.since = now; e.target = t; }
      for (const [row, t] of tgt) if (!this.w.has(row)) this.w.set(row, { w: 0, target: t, since: now });
    }
    const step = dt / 0.4;
    for (const [row, e] of this.w) { e.w += Math.max(-step, Math.min(step, e.target - e.w)); if (e.w <= 0 && e.target <= 0) this.w.delete(row); }
    const top = [...this.w.entries()].sort((a, b) => b[1].w - a[1].w).slice(0, 4);
    const sum = top.reduce((a, [, e]) => a + e.w, 0) || 1;
    const out = top.map(([row, e]) => ({ row, n: this.modes[row].n, a: e.w / sum, mode: this.modes[row] }));
    const dom = out[0] && out[0].a > 0.5 ? out[0].row : null;
    const changed = dom !== null && dom !== this.lastDominant;
    if (changed) this.lastDominant = dom;
    return { active: out, changed: changed ? this.modes[dom!] : null };
  }
  reset() { this.w.clear(); this.lastDominant = null; }
}
