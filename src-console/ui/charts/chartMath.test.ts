import { describe, expect, it } from "vitest";
import { deltaWords, monotonePath, niceTicks, rollingMean, usualRange } from "./chartMath";

/** Samples a cubic-Bézier path at many t and returns every y it reaches. */
function ys(d: string): number[] {
  const nums = d.replace(/[MLC]/g, " ").trim().split(/[\s,]+/).map(Number);
  const out: number[] = [nums[1]];
  for (let i = 2; i + 5 < nums.length; i += 6) {
    const [y0, y1, y2, y3] = [nums[i - 1], nums[i + 1], nums[i + 3], nums[i + 5]];
    for (let t = 0; t <= 1; t += .05) out.push((1 - t) ** 3 * y0 + 3 * (1 - t) ** 2 * t * y1 + 3 * (1 - t) * t ** 2 * y2 + t ** 3 * y3);
  }
  return out;
}

describe("design-1110c chart maths", () => {
  it("C1: the monotone curve never overshoots the data", () => {
    const pts = [[0, 10], [10, 80], [20, 82], [30, 20], [40, 25]] as const;
    const reach = ys(monotonePath(pts));
    expect(Math.max(...reach)).toBeLessThanOrEqual(82 + 1e-6);
    expect(Math.min(...reach)).toBeGreaterThanOrEqual(10 - 1e-6);
    expect(monotonePath([[0, 1]])).toBe("");
    expect(monotonePath([[0, 1], [5, 2]])).toBe("M0,1L5,2");
  });

  it("C2: 3-day average skips missing days and keeps them missing", () => {
    expect(rollingMean([1, 2, 3, null, 5])).toEqual([1.5, 2, 2.5, null, 5]);
  });

  it("C5: usual is the middle half and the median, only with enough readings", () => {
    expect(usualRange([1, 2, 3])).toBeNull();
    expect(usualRange([1, 2, 3, 4, 5, null])).toEqual({ low: 2, high: 4, median: 3 });
  });

  it("C4: two or three round ticks", () => {
    expect(niceTicks(42, 78)).toEqual([40, 60, 80]);
    expect(niceTicks(0, 10)).toEqual([0, 5, 10]);
  });

  it("C6: change vs usual is words with an arrow", () => {
    expect(deltaWords(70, 64, " ms")).toEqual({ text: "▲ 6 ms above usual", dir: "up" });
    expect(deltaWords(60, 64, "%")).toEqual({ text: "▼ 4% below usual", dir: "down" });
    expect(deltaWords(64.2, 64)).toEqual({ text: "≈ at your usual", dir: "flat" });
    expect(deltaWords(null, 64)).toBeNull();
  });
});
