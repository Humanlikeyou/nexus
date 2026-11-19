import { describe, expect, it } from "vitest";
import { PHI } from "../../src-console/design1118/golden";
import { BASE_HZ, FALLOFF, HARMONICS, displacement, goalString, period } from "../../src-console/design1118/tangleString";

// Taisun 2026-11-19: the Milestones strands as real vibrating waves, each at its own frequency.
describe("goal strings (struck, fixed ends)", () => {
  const strings = [0, 1, 2, 3].map(goalString);

  it("each goal has its own fundamental, a golden-ratio step apart", () => {
    expect(strings.map((s) => s.f)).toEqual([0, 1, 2, 3].map((k) => BASE_HZ * Math.pow(PHI, k)));
    expect(new Set(strings.map((s) => s.f)).size).toBe(4);
  });

  it("harmonic n has the struck-string amplitude sin(nπp)/n^1.3, normalised to a max swing of 1", () => {
    for (const s of strings) {
      expect(s.modes.map((m) => m.n)).toEqual(Array.from({ length: HARMONICS }, (_, j) => j + 1));
      const raw = s.modes.map((m) => Math.sin(m.n * Math.PI * s.pluck) / Math.pow(m.n, FALLOFF));
      const sum = raw.reduce((a, v) => a + Math.abs(v), 0);
      s.modes.forEach((m, j) => expect(m.amp).toBeCloseTo(raw[j] / sum, 12));
      expect(s.modes.reduce((a, m) => a + Math.abs(m.amp), 0)).toBeCloseTo(1, 12);
    }
  });

  it("the ends never move and the swing stays within ±1", () => {
    for (const s of strings) for (let t = 0; t < 10; t += 0.37) {
      expect(displacement(s, 0, t)).toBeCloseTo(0, 12);
      expect(displacement(s, 1, t)).toBeCloseTo(0, 12);
      for (let u = 0; u <= 1; u += 0.05) expect(Math.abs(displacement(s, u, t))).toBeLessThanOrEqual(1 + 1e-12);
    }
  });

  it("each string repeats after its own period, and the strings are out of step with each other", () => {
    for (const s of strings) for (const u of [0.2, 0.5, 0.81]) {
      expect(displacement(s, u, 0.4 + period(s))).toBeCloseTo(displacement(s, u, 0.4), 9);
    }
    const a = strings[0], b = strings[1];
    expect(Math.abs(displacement(b, 0.4, 0.4 + period(a)) - displacement(b, 0.4, 0.4))).toBeGreaterThan(1e-3);
  });

  it("it actually vibrates: the midpoint moves over half a period", () => {
    for (const s of strings) {
      const ys = Array.from({ length: 20 }, (_, k) => displacement(s, 0.45, (k / 20) * period(s) * 0.5));
      expect(Math.max(...ys) - Math.min(...ys)).toBeGreaterThan(0.1);
    }
  });

  // Taisun 2026-11-19: "make sure with every new goal a new wave is entangled".
  it("every goal has its own string: up to 12 goals >= 5% apart, and no two of 40 alike", () => {
    const gap = (n: number) => {
      const fs = Array.from({ length: n }, (_, i) => goalString(i).f).sort((a, b) => a - b);
      return Math.min(...fs.slice(1).map((f, j) => f / fs[j]));
    };
    expect(gap(12)).toBeGreaterThan(1.05);
    expect(gap(40)).toBeGreaterThan(1.002);
    const sig = new Set(Array.from({ length: 40 }, (_, i) => { const s = goalString(i); return `${s.f.toFixed(6)}|${s.pluck.toFixed(6)}`; }));
    expect(sig.size).toBe(40);
  });

  it("later goals stay in the same range, off the middle, and never copy the first four", () => {
    for (let i = 4; i < 40; i++) {
      const s = goalString(i);
      expect(s.f).toBeGreaterThanOrEqual(BASE_HZ);
      expect(s.f).toBeLessThanOrEqual(BASE_HZ * Math.pow(PHI, 3) + 1e-12);
      expect(Math.abs(s.pluck - 0.5)).toBeGreaterThanOrEqual(0.05);
      for (let k = 0; k < 4; k++) expect(s.f).not.toBeCloseTo(goalString(k).f, 3);
    }
  });

  it("the first four strings are the approved ones", () => {
    expect([0, 1, 2, 3].map((i) => goalString(i).pluck)).toEqual([0.27, 0.38, 0.62, 0.73]);
  });
});
