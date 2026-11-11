import { describe, expect, it } from "vitest";
import { AUX_GOOD_STEPS, AUX_STEP, AuxDetector, auxStepGood, detectAux, robustSd } from "./auxDetect";

let seed = 7;
const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296 - 0.5; };
const secs = (n: number, f: (i: number) => number) => Array.from({ length: n * AUX_STEP }, (_, i) => f(i));
const eeg = (amp: number) => (i: number) => amp * Math.sin((2 * Math.PI * 10 * i) / 256) + amp * 0.6 * rnd();

describe("auxDetect", () => {
  it("on: a head-like signal for 5 s is detected, also with a DC offset and 50 Hz line noise", () => {
    expect(detectAux(secs(5, eeg(10)))).toBe(true);
    expect(detectAux(secs(5, (i) => 300 + eeg(10)(i)))).toBe(true);
    expect(detectAux(secs(5, (i) => eeg(10)(i) + 6 * Math.sin((2 * Math.PI * 50 * i) / 256 * 1)))).toBe(true);
  });
  it("needs 5 consecutive good steps: 4 is not enough, a bad step resets the streak", () => {
    expect(detectAux(secs(AUX_GOOD_STEPS - 1, eeg(10)))).toBe(false);
    const d = new AuxDetector(); const good = secs(1, eeg(10)); const flat = secs(1, () => 0);
    for (let i = 0; i < 4; i += 1) d.step(good);
    d.step(flat); for (let i = 0; i < 4; i += 1) d.step(good);
    expect(d.detected).toBe(false);
    d.step(good); expect(d.detected).toBe(true);
    d.step(flat); expect(d.detected).toBe(true); // latched
  });
  it("off: floating/flat, too noisy, railed and missing data are all rejected", () => {
    expect(detectAux(secs(8, () => 0))).toBe(false);
    expect(detectAux(secs(8, () => 0.2 * rnd()))).toBe(false); // SD below 1.5 µV
    expect(detectAux(secs(8, (i) => 400 * rnd() + 50 * Math.sin(i / 3)))).toBe(false); // SD above 40 µV
    expect(detectAux(secs(8, (i) => (i % 40 < 6 ? 1100 : 8 * rnd())))).toBe(false); // railed
    expect(auxStepGood(secs(1, eeg(10)).slice(0, 100))).toBe(false); // under a second
    expect(auxStepGood(secs(1, () => NaN))).toBe(false);
    const d = new AuxDetector(); for (let i = 0; i < 10; i += 1) d.step(null);
    expect(d.detected).toBe(false);
  });
  it("robust SD ignores a single spike", () => {
    const xs = secs(1, eeg(10)); const base = robustSd(xs); xs[100] = 900;
    expect(Math.abs(robustSd(xs) - base)).toBeLessThan(0.5);
  });
});
