import { describe, expect, it } from "vitest";
import { TRACE_HEIGHT, TRACE_SECONDS, TRACE_WIDTH, traceShape, visibleSamples } from "./heartTrace";

const AT = 1_000;
const run = (bpms: number[], end = AT) => bpms.map((bpm, i) => ({ bpm, at: end - (bpms.length - 1 - i) }));

describe("heart trace", () => {
  it("keeps Home's Fibonacci sizes", () => {
    expect([TRACE_SECONDS, TRACE_WIDTH, TRACE_HEIGHT]).toEqual([89, 233, 55]);
  });
  it("windows the samples to the playhead", () => {
    const samples = run(Array.from({ length: 120 }, () => 70));
    const visible = visibleSamples(samples, AT - 5);
    expect(visible).toHaveLength(TRACE_SECONDS);
    expect(visible[visible.length - 1].at).toBe(AT - 5);
    expect(visible[0].at).toBe(AT - 5 - TRACE_SECONDS + 1);
  });
  it("has nothing to draw with fewer than two samples", () => {
    expect(traceShape(run([70]), AT)).toEqual({ paths: [], dot: null });
  });
  it("ends on the live dot at the right edge, inside the box", () => {
    const { paths, dot } = traceShape(run([60, 70, 80, 90, 100, 110, 120]), AT);
    expect(paths).toHaveLength(1);
    expect(dot!.x).toBeCloseTo(TRACE_WIDTH - 8, 1);
    expect(dot!.y).toBeGreaterThanOrEqual(8);
    expect(dot!.y).toBeLessThanOrEqual(TRACE_HEIGHT - 8);
  });
  it("goes up the screen as the heart rate rises", () => {
    const { dot } = traceShape(run([60, 60, 60, 90, 90, 90]), AT);
    expect(dot!.y).toBeLessThan(TRACE_HEIGHT / 2);
  });
  it("keeps a steady heart rate flat instead of magnifying one beat", () => {
    const { dot } = traceShape(run([70, 71, 70, 71, 70, 71]), AT);
    expect(Math.abs(dot!.y - TRACE_HEIGHT / 2)).toBeLessThan(4);
  });
  it("breaks the line across a dropout", () => {
    const samples = [...run([70, 71, 72], AT - 20), ...run([73, 74, 75], AT)];
    expect(traceShape(samples, AT).paths).toHaveLength(2);
  });
});
