import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TrendChart } from "./TrendChart";
import { addDays, calendarDays, monotonePath, type Pt } from "./chartMath";
import { inRange } from "../../lib/mindBodyDeep";

// Chart geometry at the jsdom fallback width (480) and height 120: gutter 34, right 6, top 8, x axis 18.
const W = 480, H = 120, G = 34, PW = W - G - 6, TOP = 8, PH = H - TOP - 18;
const at = (i: number, n: number, v: number, lo: number, hi: number) => [G + (PW * i) / (n - 1), TOP + PH - ((v - lo) / (hi - lo)) * PH] as const;
const near = (a: number, b: number) => expect(Math.abs(a - b)).toBeLessThan(0.11);

/** Every endpoint of every segment in an SVG path ("M x,y C … x,y"). */
const anchors = (d: string) => { const out: number[][] = []; const m = d.match(/^M([\d.-]+),([\d.-]+)/); if (m) out.push([+m[1], +m[2]]); for (const c of d.matchAll(/C[\d.,\s-]+?\s([\d.-]+),([\d.-]+)(?=C|$)/g)) out.push([+c[1], +c[2]]); for (const l of d.matchAll(/L([\d.-]+),([\d.-]+)/g)) out.push([+l[1], +l[2]]); return out; };

describe("TrendChart plots the source values", () => {
  const values = [10, null, 30, 20, 25];
  const labels = ["a", "b", "c", "d", "e"];
  it("dots and the latest point sit exactly on each reading, by slot", () => {
    const { container } = render(<TrendChart values={values} labels={labels} label="t" domain={[0, 40]} />);
    const dots = [...container.querySelectorAll("circle.trend-chart__raw")].map((c) => [+c.getAttribute("cx")!, +c.getAttribute("cy")!]);
    expect(dots).toHaveLength(3);
    [[0, 10], [2, 30], [3, 20]].forEach(([i, v], k) => { const [x, y] = at(i, 5, v, 0, 40); near(dots[k][0], x); near(dots[k][1], y); });
    const now = container.querySelector("circle.trend-chart__now")!;
    const [x, y] = at(4, 5, 25, 0, 40); near(+now.getAttribute("cx")!, x); near(+now.getAttribute("cy")!, y);
  });
  it("the line goes through the readings and breaks at the missing day", () => {
    const { container } = render(<TrendChart values={values} labels={labels} label="t" domain={[0, 40]} />);
    const lines = [...container.querySelectorAll("path.trend-chart__line")].map((p) => p.getAttribute("d")!);
    expect(lines).toHaveLength(2);
    const run = anchors(lines[1]);
    expect(run).toHaveLength(3);
    [[2, 30], [3, 20], [4, 25]].forEach(([i, v], k) => { const [x, y] = at(i, 5, v, 0, 40); near(run[k][0], x); near(run[k][1], y); });
    const [x0, y0] = at(0, 5, 10, 0, 40); near(anchors(lines[0])[0][0], x0); near(anchors(lines[0])[0][1], y0);
  });
  it("a fixed domain grows to hold every reading instead of clipping it", () => {
    const { container } = render(<TrendChart values={[4, 12.5, 7]} labels={["a", "b", "c"]} label="t" kind="bar" smoothing={1} domain={[0, 10]} ticks={[0, 5, 10]} />);
    const ticks = [...container.querySelectorAll("text.trend-chart__tick")].map((t) => parseFloat(t.textContent ?? ""));
    expect(Math.max(...ticks.filter(Number.isFinite))).toBeGreaterThanOrEqual(12.5);
  });
});

describe("chart maths", () => {
  it("the monotone curve never overshoots its two readings in any segment", () => {
    const pts: Pt[] = [[0, 0], [10, 50], [20, 50], [30, 0], [40, 25], [50, 24]];
    const d = monotonePath(pts);
    const segs = [...d.matchAll(/C([\d.-]+),([\d.-]+) ([\d.-]+),([\d.-]+) ([\d.-]+),([\d.-]+)/g)].map((m) => m.slice(1).map(Number));
    expect(segs).toHaveLength(pts.length - 1);
    segs.forEach((s, i) => {
      const [y0, y3] = [pts[i][1], pts[i + 1][1]], [y1, y2] = [s[1], s[3]];
      for (let t = 0; t <= 1; t += 0.05) {
        const y = (1 - t) ** 3 * y0 + 3 * (1 - t) ** 2 * t * y1 + 3 * (1 - t) * t ** 2 * y2 + t ** 3 * y3;
        expect(y).toBeGreaterThanOrEqual(Math.min(y0, y3) - 0.1);
        expect(y).toBeLessThanOrEqual(Math.max(y0, y3) + 0.1);
      }
    });
  });
  it("calendar days are real days: gaps are null and month ends roll over", () => {
    expect(addDays("2026-10-31", 1)).toBe("2026-11-01");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
    const slots = calendarDays([{ day: "2026-10-25" }, { day: "2026-10-28" }], (d) => d.day, 4);
    expect(slots.map((s) => [s.day, s.item != null])).toEqual([["2026-10-25", true], ["2026-10-26", false], ["2026-10-27", false], ["2026-10-28", true]]);
  });
  it("inRange keeps missing days as null slots, ending on the reading day", () => {
    const pts = [{ day: "2026-10-20", value: 1 }, { day: "2026-10-23", value: 4 }, { day: "2026-10-24", value: 5 }];
    expect(inRange(pts, "2026-10-24", 5).map((p) => p.value)).toEqual([1, null, null, 4, 5]);
  });
});

describe("todayJst", () => {
  it("is the Tokyo date, not the UTC one", async () => {
    const { todayJst } = await import("./chartMath");
    expect(todayJst(new Date("2026-11-10T16:30:00Z"))).toBe("2026-11-11");
    expect(todayJst(new Date("2026-11-10T14:59:00Z"))).toBe("2026-11-10");
  });
});
