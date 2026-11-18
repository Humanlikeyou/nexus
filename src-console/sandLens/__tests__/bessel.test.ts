import { describe, expect, it } from "vitest";
import { F0, M_MAX, ModeScheduler, N_MAX, besselJ, besselZeros, buildModes, fallbackPeaks, nearestMode, pickPeaks, plateField } from "../bessel";

describe("design-1118b Bessel / Chladni modes", () => {
  it("finds the known zeros of J0 and J1", () => {
    const [k01, k02] = besselZeros(0, 2);
    const [k11] = besselZeros(1, 1);
    expect(k01).toBeCloseTo(2.4048, 3);
    expect(k02).toBeCloseTo(5.5201, 3);
    expect(k11).toBeCloseTo(3.8317, 3);
    expect(Math.abs(besselJ(0, k01))).toBeLessThan(1e-6);
    expect(Math.abs(besselJ(1, k11))).toBeLessThan(1e-6);
  });

  it("builds all 54 clamped-plate modes, sorted by frequency, with f ∝ k (lowest mode = F0)", () => {
    const { modes, lut } = buildModes();
    expect(modes).toHaveLength((N_MAX + 1) * M_MAX);
    expect(modes).toHaveLength(54);
    expect(modes[0].id).toBe("n0m1");
    expect(modes[0].f).toBeCloseTo(F0, 6);
    for (let i = 1; i < modes.length; i++) expect(modes[i].f).toBeGreaterThanOrEqual(modes[i - 1].f);
    expect(lut.length).toBe(modes.length * 1024);
    // every LUT row is normalised so the strongest radial value is ±1
    for (const m of modes) { let mx = 0; for (let i = 0; i < 1024; i++) mx = Math.max(mx, Math.abs(lut[m.row * 1024 + i])); expect(mx).toBeCloseTo(1, 5); }
  });

  it("the plate field is zero on the nodal lines (true Chladni physics)", () => {
    const m = buildModes().modes.find((x) => x.id === "n2m1")!;
    // n = 2: nodal diameters at θ = 45° and 135°
    expect(Math.abs(plateField([{ mode: m, a: 1 }], 0.5 * Math.cos(Math.PI / 4), 0.5 * Math.sin(Math.PI / 4)))).toBeLessThan(1e-9);
    expect(Math.abs(plateField([{ mode: m, a: 1 }], 0.5, 0))).toBeGreaterThan(0.05);
  });

  it("maps a frequency to its nearest mode by log distance", () => {
    const { modes } = buildModes();
    expect(nearestMode(F0).id).toBe("n0m1");
    expect(nearestMode(modes[10].f).f).toBeCloseTo(modes[10].f, 9);
    expect(nearestMode(modes[30].f * 1.001).f).toBeCloseTo(modes[30].f, 0);
    expect(nearestMode(1).row).toBe(0);
    expect(nearestMode(1e6).row).toBe(modes.length - 1);
  });

  it("picks the 2–3 strongest separated peaks in the speech band, ignoring silence", () => {
    const binHz = 10; const fd = new Float32Array(1024).fill(-120);
    fd[22] = -20; fd[50] = -30; fd[51] = -31; fd[90] = -40; fd[300] = -110; fd[5] = -10; // 50 Hz is below the band
    const peaks = pickPeaks(fd, binHz);
    expect(peaks.map((p) => p.f)).toEqual([220, 500, 900]);
    expect(pickPeaks(new Float32Array(1024).fill(-120), binHz)).toEqual([]);
  });

  it("the speech fallback is deterministic, stays in the speech band and holds a figure for a phrase", () => {
    const a = fallbackPeaks(5), b = fallbackPeaks(5);
    expect(a).toEqual(b);
    for (let i = 0; i < 40; i++) for (const p of fallbackPeaks(i)) { expect(p.f).toBeGreaterThanOrEqual(100); expect(p.f).toBeLessThanOrEqual(1000); }
    expect(fallbackPeaks(0)[0].f).toBe(fallbackPeaks(5)[0].f); // same phrase, same dominant figure
    expect(fallbackPeaks(6)[0].f).not.toBe(fallbackPeaks(0)[0].f); // next phrase moves on
    expect(fallbackPeaks(0).length).toBe(1); // most syllables hold one pure mode (a crisp figure)
    expect(fallbackPeaks(1).length).toBeGreaterThanOrEqual(2); // an early syllable nudges a neighbour in
  });

  it("the scheduler crossfades (~400 ms), holds ≥ 250 ms and keeps at most 4 modes", () => {
    const s = new ModeScheduler();
    const { modes } = buildModes();
    let out = s.update([{ f: modes[5].f, amp: 1 }], 0.8, 0.1, 0);
    expect(out.active[0].row).toBe(5);
    out = s.update([{ f: modes[20].f, amp: 1 }], 0.8, 0.1, 100); // inside the hold: mode 5 keeps its target
    expect(out.active.some((m) => m.row === 5)).toBe(true);
    for (let t = 0; t < 12; t++) out = s.update([{ f: modes[20].f, amp: 1 }], 0.8, 0.1, 400 + t * 100);
    expect(out.active[0].row).toBe(20);
    expect(out.active.find((m) => m.row === 5)).toBeUndefined();
    for (let i = 0; i < 8; i++) out = s.update([{ f: modes[i * 3].f, amp: 1 }, { f: modes[i * 3 + 1].f, amp: 1 }, { f: modes[i * 3 + 2].f, amp: 1 }], 0.8, 0.05, 2000 + i * 300);
    expect(out.active.length).toBeLessThanOrEqual(4);
    const sum = out.active.reduce((x, m) => x + m.a, 0);
    expect(sum).toBeCloseTo(1, 5);
  });
});
