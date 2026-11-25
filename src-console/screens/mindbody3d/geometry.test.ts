import { describe, expect, it } from "vitest";
import { SWEET_HIGH, SWEET_LOW, bandTriangles, barLayout, deltaReach, gapRuns, isOutside, loadSeries, monotoneSample, nearestIndex, plotBox, ratioFraction, ratioZone, seriesDomain, stepIndex, usualZoneHalf, xOf, yOf } from "./geometry";

describe("series geometry", () => {
  const box = plotBox(400, 120);
  it("keeps TrendChart's margins", () => expect(box).toEqual({ x0: 34, x1: 394, y0: 8, y1: 102 }));
  it("makes room for workout markers", () => expect(plotBox(400, 120, { markers: true }).y1).toBe(92));
  it("domain covers values and the usual band, never clips", () => {
    const d = seriesDomain([50, 60, null, 80], { low: 40, high: 90, median: 65 }, "line")!;
    expect(d.lo).toBeLessThanOrEqual(40); expect(d.hi).toBeGreaterThanOrEqual(90);
  });
  it("bars start at zero", () => expect(seriesDomain([6, 7, 8], null, "bar")!.lo).toBe(0));
  it("a fixed domain holds only while it contains the data", () => {
    expect(seriesDomain([6, 7], null, "bar", [0, 10], [0, 5, 10])).toEqual({ lo: 0, hi: 10, ticks: [0, 5, 10] });
    expect(seriesDomain([6, 12], null, "bar", [0, 10], [0, 5, 10])!.hi).toBeGreaterThanOrEqual(12);
  });
  it("no readings, no domain", () => expect(seriesDomain([null, null], null, "line")).toBeNull());
  it("maps values to pixels inside the box", () => {
    expect(yOf(0, 0, 10, box)).toBe(box.y1); expect(yOf(10, 0, 10, box)).toBe(box.y0); expect(yOf(99, 0, 10, box)).toBe(box.y0);
    expect(xOf(0, 5, box, "line")).toBe(box.x0); expect(xOf(4, 5, box, "line")).toBe(box.x1);
  });
  it("bar slots are centred and 62% wide", () => {
    const { slot, bw } = barLayout(10, box);
    expect(xOf(0, 10, box, "bar")).toBeCloseTo(box.x0 + slot / 2); expect(bw / slot).toBeCloseTo(0.62);
  });
  it("breaks runs at gaps", () => expect(gapRuns([1, 2, null, 4, null, null, 7, 8])).toEqual([[0, 1], [3], [6, 7]]));
  it("flags values outside the usual band", () => {
    const u = { low: 10, high: 20, median: 15 };
    expect([isOutside(9, u), isOutside(10, u), isOutside(20, u), isOutside(21, u), isOutside(5, null)]).toEqual([true, false, false, true, false]);
  });
  it("hover finds the nearest day that has a reading", () => {
    const v = [1, null, null, 4];
    expect(nearestIndex(xOf(1, 4, box, "line"), v, box, "line")).toBe(0);
    expect(nearestIndex(box.x1, v, box, "line")).toBe(3);
    expect(nearestIndex(10, [null, null], box, "line")).toBeNull();
  });
  it("keyboard steps skip gaps and stop at the ends", () => {
    const v = [1, null, 3, 4];
    expect(stepIndex(null, 1, v)).toBe(0); expect(stepIndex(null, -1, v)).toBe(3);
    expect(stepIndex(0, 1, v)).toBe(2); expect(stepIndex(3, 1, v)).toBe(3); expect(stepIndex(2, -1, v)).toBe(0);
  });
  it("band polygons: two triangles per column pair, none across a gap", () => {
    expect(bandTriangles([0, 10, 20], [5, 5, 5], [15, 15, 15])).toHaveLength(2 * 12);
    expect(bandTriangles([0, 10, 20], [5, null, 5], [15, 15, 15])).toHaveLength(0);
    expect(bandTriangles([0, 10], [5, 6], [15, 14])).toEqual([0, 5, 10, 6, 0, 15, 10, 6, 10, 14, 0, 15]);
  });
  it("monotone sampling never overshoots a reading", () => {
    const pts = [[0, 10], [10, 0], [20, 0], [30, 10]] as const;
    const s = monotoneSample(pts, 8);
    expect(Math.min(...s.map((p) => p[1]))).toBeGreaterThanOrEqual(0); expect(Math.max(...s.map((p) => p[1]))).toBeLessThanOrEqual(10);
    expect(s[0]).toEqual([0, 10]); expect(s[s.length - 1][0]).toBeCloseTo(30);
  });
});

describe("readiness and load geometry", () => {
  it("caps a bar at 2.5 sd; the usual zone is 1 sd", () => {
    expect(deltaReach(2.5, 100)).toBe(100); expect(deltaReach(-9, 100)).toBe(100); expect(deltaReach(1.25, 100)).toBe(50); expect(usualZoneHalf(100)).toBe(40);
  });
  it("ratio zones follow the loadRatio thresholds", () => {
    expect([ratioZone(0.79), ratioZone(SWEET_LOW), ratioZone(SWEET_HIGH), ratioZone(1.31)]).toEqual(["light", "sweet", "sweet", "high"]);
    expect([ratioFraction(0.5), ratioFraction(1.8), ratioFraction(9), ratioFraction(0)]).toEqual([0, 1, 1, 0]);
  });
  it("load series: last 28 days, trailing averages, honest gaps", () => {
    const pts = Array.from({ length: 40 }, (_, i) => ({ day: `2026-01-${String(i + 1).padStart(2, "0")}`.replace("2026-01-3", "2026-01-3"), value: i === 5 ? null : i }));
    const s = loadSeries(pts.map((p, i) => ({ ...p, day: `d${String(i).padStart(2, "0")}` })));
    expect(s.vals).toHaveLength(28); expect(s.vals[0].day).toBe("d12"); expect(s.count).toBe(28); expect(s.max).toBe(39);
    expect(s.a7[27]).toBeCloseTo((33 + 34 + 35 + 36 + 37 + 38 + 39) / 7);
  });
  it("load series with no data is empty-safe", () => expect(loadSeries([{ day: "a", value: null }]).count).toBe(0));
});
