import { alpha, type Prim } from "./prims";
import { PLOT, barLayout, gapRuns, isOutside, monotoneSample, plotBox, xOf, yOf, type Kind, type Usual } from "./geometry";
import type { Theme } from "./theme";

export type SeriesInput = {
  w: number; h: number; values: readonly (number | null)[]; usual: Usual | null; kind: Kind;
  dom: { lo: number; hi: number; ticks: number[] }; markers?: readonly boolean[]; hover: number | null; theme: Theme;
};

/**
 * Design A (2D clean): hairline grid, the copper usual band with its dashed median, the emerald line (or bars),
 * a dot per reading (copper when outside the usual band), workout triangles, the hover crosshair.
 */
export function buildSeriesPrims({ w, h, values, usual, kind, dom, markers, hover, theme: t }: SeriesInput): Prim[] {
  const box = plotBox(w, h, { markers: !!markers });
  const n = values.length;
  const X = (i: number) => xOf(i, n, box, kind), Y = (v: number) => yOf(v, dom.lo, dom.hi, box);
  const out: Prim[] = [];
  dom.ticks.forEach((tick, k) => out.push({ k: "rect", x: box.x0, y: Math.round(Y(tick)) - 0.5, w: box.x1 - box.x0, h: 1, c: k === 0 ? alpha(t.label, 0.22) : t.sep }));
  if (usual) {
    const top = Y(usual.high), bottom = Y(usual.low);
    out.push({ k: "rect", x: box.x0, y: top, w: box.x1 - box.x0, h: Math.max(1, bottom - top), c: alpha(t.copper, t.dark ? 0.2 : 0.14) });
    out.push({ k: "line", pts: [[box.x0, Y(usual.median)], [box.x1, Y(usual.median)]], w: 1, c: alpha(t.copper, 0.85), dash: [3, 4] });
  }
  const last = values.map((v) => v != null).lastIndexOf(true);
  if (kind === "bar") {
    const { bw } = barLayout(n, box);
    values.forEach((v, i) => {
      if (v == null) return;
      const y = Y(v);
      out.push({ k: "rect", x: X(i) - bw / 2, y, w: bw, h: Math.max(1, box.y1 - y), c: i === last || i === hover ? t.accent : alpha(t.accent, 0.4) });
    });
  } else {
    for (const run of gapRuns(values)) {
      const pts = run.map((i) => [X(i), Y(values[i] as number)] as const);
      if (pts.length > 1) out.push({ k: "line", pts: monotoneSample(pts), w: 2, c: t.accent });
    }
    values.forEach((v, i) => {
      if (v == null || i === last) return;
      out.push(isOutside(v, usual) ? { k: "dot", x: X(i), y: Y(v), r: 3, c: t.copper } : { k: "dot", x: X(i), y: Y(v), r: 2, c: alpha(t.accent, 0.6) });
    });
    if (last >= 0) {
      const v = values[last] as number;
      out.push({ k: "dot", x: X(last), y: Y(v), r: 5, c: t.bg }, { k: "dot", x: X(last), y: Y(v), r: 3.5, c: isOutside(v, usual) ? t.copper : t.accent });
    }
  }
  markers?.forEach((m, i) => { if (m) out.push({ k: "poly", pts: [X(i) - 3, box.y1 + 9, X(i), box.y1 + 4, X(i) + 3, box.y1 + 9], c: t.label3 }); });
  if (hover != null && values[hover] != null) {
    out.push({ k: "rect", x: X(hover) - 0.5, y: PLOT.top - 4, w: 1, h: box.y1 - PLOT.top + 4, c: alpha(t.label3, 0.8) });
    if (kind === "line") out.push({ k: "dot", x: X(hover), y: Y(values[hover] as number), r: 5.5, c: t.accent }, { k: "dot", x: X(hover), y: Y(values[hover] as number), r: 3, c: t.bg });
  }
  return out;
}
