import { describe, expect, it } from "vitest";
import { alpha, arcStrip, buildBuffers, dashSegments, extrudedBar, mix } from "./prims";
import { parseCssColor } from "./theme";
import { buildSeriesPrims } from "./seriesPrims";
import { buildLoadPrims, loadLayout } from "./LoadGfx";
import { buildReadinessPrims } from "./ReadinessGfx";
import { buildSleepNightPrims, stageWidths } from "./SleepNightGfx";
import { loadSeries, seriesDomain } from "./geometry";
import type { Theme } from "./theme";

const T: Theme = { accent: [0, 1, 0, 1], copper: [1, 0.5, 0, 1], label: [0, 0, 0, 1], label2: [0.3, 0.3, 0.3, 1], label3: [0.6, 0.6, 0.6, 1], sep: [0, 0, 0, 0.1], bg: [1, 1, 1, 1], deep: [0, 1, 0, 1], rem: [0, 1, 0, 0.5], light: [0, 1, 0, 0.25], dark: false };

describe("primitives", () => {
  it("a rect is two triangles with rgba per vertex", () => {
    const b = buildBuffers([{ k: "rect", x: 0, y: 0, w: 10, h: 5, c: [1, 0, 0, 1], c1: [0, 0, 1, 1] }]);
    expect(b.vertices).toBe(6); expect(b.col).toHaveLength(24); expect(b.pos).toHaveLength(18);
    expect(Array.from(b.col.slice(4, 8))).toEqual([0, 0, 1, 1]);
  });
  it("skips empty rects", () => expect(buildBuffers([{ k: "rect", x: 0, y: 0, w: 0, h: 5, c: [1, 1, 1, 1] }]).vertices).toBe(0));
  it("dash pattern splits a line into on pieces", () => {
    const segs = dashSegments([[0, 0], [20, 0]], 5, 5);
    expect(segs.map((s) => [s[0][0], s[s.length - 1][0]])).toEqual([[0, 5], [10, 15]]);
  });
  it("arc strips cover the sweep", () => expect(arcStrip(0, 0, 8, 10, 0, Math.PI, 10)).toHaveLength(10 * 12));
  it("an extruded bar is side, top, front", () => expect(extrudedBar(0, 10, 5, 50, 3, [1, 1, 1, 1], [1, 1, 1, 1], [0, 0, 0, 1]).map((p) => p.k)).toEqual(["poly", "poly", "rect"]));
  it("alpha and mix", () => { expect(alpha([1, 1, 1, 0.5], 0.5)[3]).toBe(0.25); expect(mix([0, 0, 0, 1], [1, 1, 1, 1], 0.5)).toEqual([0.5, 0.5, 0.5, 1]); });
  it("parses the colour formats the browser returns", () => {
    expect(parseCssColor("#0e7a5f")).toEqual([14 / 255, 122 / 255, 95 / 255, 1]);
    expect(parseCssColor("rgba(60, 60, 67, 0.18)")?.[3]).toBeCloseTo(0.18);
    expect(parseCssColor("color(srgb 0.1 0.2 0.3 / 0.5)")).toEqual([0.1, 0.2, 0.3, 0.5]);
    expect(parseCssColor("rgb(255 0 0 / 50%)")).toEqual([1, 0, 0, 0.5]);
    expect(parseCssColor("nonsense")).toBeNull();
  });
});

describe("chart primitives", () => {
  const values = [10, 12, null, 30, 11];
  const usual = { low: 8, high: 14, median: 11 };
  const dom = seriesDomain(values, usual, "line")!;
  const count = (p: ReturnType<typeof buildSeriesPrims>, k: string) => p.filter((x) => x.k === k).length;
  it("line: band, dashed median, one line per run, dots, copper for outside", () => {
    const p = buildSeriesPrims({ w: 400, h: 112, values, usual, kind: "line", dom, hover: null, theme: T });
    expect(p.filter((x) => x.k === "line")).toHaveLength(3); // median + the two runs (a lone day would be a dot)
    expect(p.some((x) => x.k === "line" && x.dash)).toBe(true);
    expect(p.filter((x) => x.k === "dot" && x.c === T.copper).length).toBe(1); // 30 is outside the band
  });
  it("bars: one per reading, the last one solid", () => {
    const d = seriesDomain([5, 6, 7], null, "bar")!;
    const p = buildSeriesPrims({ w: 300, h: 112, values: [5, 6, 7], usual: null, kind: "bar", dom: d, hover: null, theme: T });
    const bars = p.filter((x) => x.k === "rect" && (x.c === T.accent || x.c[3] === 0.4));
    expect(bars).toHaveLength(3);
    expect(count(p, "dot")).toBe(0);
  });
  it("workout markers and hover add shapes", () => {
    const base = buildSeriesPrims({ w: 400, h: 112, values, usual, kind: "line", dom, hover: null, theme: T }).length;
    const more = buildSeriesPrims({ w: 400, h: 112, values, usual, kind: "line", dom, markers: [true, false, false, false, true], hover: 1, theme: T });
    expect(more.length).toBeGreaterThan(base + 2);
  });
  it("readiness: a bar per driver with a reading, none without", () => {
    const tracks = [{ left: 0, width: 100, cy: 10 }, { left: 0, width: 100, cy: 50 }];
    const none = buildReadinessPrims(tracks, [{ z: null, lean: "flat" }, { z: null, lean: "flat" }], null, T).filter((p) => p.k === "dot");
    const some = buildReadinessPrims(tracks, [{ z: 1.3, lean: "up" }, { z: -2, lean: "down" }], 1, T).filter((p) => p.k === "dot");
    expect(none).toHaveLength(0); expect(some).toHaveLength(2);
    expect((some[1] as { x: number }).x).toBeLessThan(50); expect((some[0] as { x: number }).x).toBeGreaterThan(50);
  });
  it("sleep night: stage widths sum to the row and skip empty stages", () => {
    const w = stageWidths([{ label: "Deep", minutes: 60, tone: "deep" }, { label: "REM", minutes: 0, tone: "rem" }, { label: "Light", minutes: 180, tone: "light" }], 303);
    expect(w).toHaveLength(2); expect(w[1].x + w[1].w).toBeCloseTo(303); expect(w[0].w / w[1].w).toBeCloseTo(1 / 3);
    expect(buildSleepNightPrims(300, 70, { s: 0, e: 400, a0: -60, a1: 480 }, [{ label: "Deep", minutes: 10, tone: "deep" }], T).length).toBeGreaterThan(2);
  });
  it("load: bars, 7- and 28-day lines, dial only when wide enough", () => {
    const series = loadSeries(Array.from({ length: 28 }, (_, i) => ({ day: `d${String(i).padStart(2, "0")}`, value: 5 + (i % 7) })));
    const wide = buildLoadPrims({ w: 700, h: 148, series, ratio: 1.1, hover: null, theme: T });
    const narrow = buildLoadPrims({ w: 300, h: 148, series, ratio: 1.1, hover: null, theme: T });
    expect(loadLayout(700, 148).dial).not.toBeNull(); expect(loadLayout(300, 148).dial).toBeNull();
    expect(wide.filter((p) => p.k === "tris")).toHaveLength(3);
    expect(narrow.filter((p) => p.k === "tris")).toHaveLength(0);
    expect(wide.filter((p) => p.k === "line" && p.dash)).toHaveLength(1);
  });
});
