import { describe, expect, it } from "vitest";
import { OnsetDetector } from "../onset";

const run = (d: OnsetDetector, seq: number[], dt = 1 / 30) => seq.map((l) => d.update(l, dt));

describe("OnsetDetector", () => {
  it("fires once on a rising edge and not while held", () => {
    const d = new OnsetDetector();
    const out = run(d, [0, 0, 0, 0.7, 0.7, 0.7, 0.7, 0.7, 0.7, 0.7, 0.7, 0.7]);
    expect(out.filter((x) => x > 0)).toHaveLength(1);
    expect(out[3]).toBeGreaterThan(0);
  });
  it("fires again for each syllable after a dip", () => {
    const d = new OnsetDetector();
    const gap = Array(12).fill(0.02);
    const burst = [0.8, 0.8, 0.8, 0.8];
    const out = run(d, [...gap, ...burst, ...gap, ...burst, ...gap, ...burst]);
    expect(out.filter((x) => x > 0)).toHaveLength(3);
  });
  it("ignores silence, quiet noise and bad input", () => {
    const d = new OnsetDetector();
    expect(run(d, Array(60).fill(0.03)).every((x) => x === 0)).toBe(true);
    expect(d.update(NaN, 0.03)).toBe(0);
  });
  it("respects the refractory gap and scales strength with the jump", () => {
    const soft = new OnsetDetector(), hard = new OnsetDetector();
    const s = run(soft, [0, 0, 0.3, 0.3]).find((x) => x > 0) ?? 0;
    const h = run(hard, [0, 0, 1, 1]).find((x) => x > 0) ?? 0;
    expect(h).toBeGreaterThan(s);
    const d = new OnsetDetector();
    expect(run(d, [0, 0, 0.9, 0, 0.9, 0], 0.02).filter((x) => x > 0).length).toBeLessThanOrEqual(1);
  });
});
