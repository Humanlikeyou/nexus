/* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex -- a chart is one focusable group: arrow keys read each day, and its facts are also in the hidden table */
import { useCallback, useMemo, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import type { NoopMetricPoint } from "../../types";
import { shortDay } from "../../lib/mindBody";
import { METRICS } from "../../lib/mindBodyDeep";
import { RATIO_MAX, RATIO_MIN, SWEET_HIGH, SWEET_LOW, ZONE_WORDS, loadSeries, ratioFraction, ratioZone } from "./geometry";
import { alpha, arcStrip, extrudedBar, mix, type Prim } from "./prims";
import type { Theme } from "./theme";
import { useGfx } from "./useGfx";
import "./mindbody3d.css";

const DEPTH = 3, TOP = 12, BOTTOM = 18, DIAL_W = 170, MIN_DIAL_PLOT = 330;

export type LoadLayout = { plot: { x0: number; x1: number; y0: number; y1: number }; dial: { cx: number; cy: number; r: number } | null };
/** Plot on the left, ratio dial on the right when there is room. */
export function loadLayout(w: number, h: number): LoadLayout {
  const dialOn = w >= MIN_DIAL_PLOT + DIAL_W;
  const plot = { x0: 28, x1: (dialOn ? w - DIAL_W : w) - DEPTH - 4, y0: TOP, y1: h - BOTTOM };
  return { plot, dial: dialOn ? { cx: w - DIAL_W / 2 + 6, cy: h / 2 + 22, r: 48 } : null };
}
export const loadSlot = (i: number, n: number, p: LoadLayout["plot"]) => { const slot = (p.x1 - p.x0) / n, bw = slot * 0.62; return { x: p.x0 + slot * i + (slot - bw) / 2, bw, slot }; };

export function buildLoadPrims(o: { w: number; h: number; series: ReturnType<typeof loadSeries>; ratio: number | null; hover: number | null; theme: Theme }): Prim[] {
  const { w, h, series, ratio, hover, theme: t } = o;
  const { plot, dial } = loadLayout(w, h);
  const n = series.vals.length, out: Prim[] = [];
  const Y = (v: number) => plot.y1 - (v / series.max) * (plot.y1 - plot.y0);
  for (const f of [0, 0.5, 1]) out.push({ k: "rect", x: plot.x0, y: Math.round(plot.y1 - f * (plot.y1 - plot.y0)) - 0.5, w: plot.x1 - plot.x0 + DEPTH + 2, h: 1, c: f === 0 ? alpha(t.label, 0.22) : t.sep });
  const front = (hot: boolean): [typeof t.accent, typeof t.accent, typeof t.accent] => {
    const base = mix(t.bg, t.accent, hot ? 0.95 : 0.55);
    return [base, mix(base, [1, 1, 1, 1], 0.3), mix(base, [0, 0, 0, 1], 0.28)];
  };
  series.vals.forEach((p, i) => {
    if (typeof p.value !== "number") return;
    const { x, bw } = loadSlot(i, n, plot), y = Y(p.value);
    const [f, tp, sd] = front(i === hover || i === n - 1);
    out.push(...extrudedBar(x, y, bw, plot.y1, Math.min(DEPTH, bw / 2), f, tp, sd));
  });
  const cx = (i: number) => { const { x, bw } = loadSlot(i, n, plot); return x + bw / 2 + DEPTH / 2; };
  const l7 = series.a7.map((v, i) => [cx(i), Y(v) - DEPTH / 2] as const), l28 = series.a28.map((v, i) => [cx(i), Y(v) - DEPTH / 2] as const);
  out.push({ k: "line", pts: l7, w: 5, c: alpha(t.label, 0.12) }, { k: "line", pts: l7, w: 1.6, c: t.label }, { k: "line", pts: l28, w: 1.6, c: t.copper, dash: [5, 3] });
  if (hover != null && series.vals[hover]) out.push({ k: "rect", x: cx(hover) - 0.5, y: plot.y0 - 6, w: 1, h: plot.y1 - plot.y0 + 6, c: alpha(t.label3, 0.8) });
  if (dial && ratio !== null) {
    const ang = (f: number) => Math.PI - f * Math.PI;
    const f8 = ratioFraction(SWEET_LOW), f13 = ratioFraction(SWEET_HIGH);
    out.push({ k: "tris", pts: arcStrip(dial.cx, dial.cy, dial.r - 8, dial.r, ang(0), ang(f8)), c: alpha(t.label3, 0.55) });
    out.push({ k: "tris", pts: arcStrip(dial.cx, dial.cy, dial.r - 8, dial.r, ang(f8), ang(f13)), c: t.accent });
    out.push({ k: "tris", pts: arcStrip(dial.cx, dial.cy, dial.r - 8, dial.r, ang(f13), ang(1)), c: t.copper });
    const a = ang(ratioFraction(ratio));
    out.push({ k: "line", pts: [[dial.cx, dial.cy], [dial.cx + Math.cos(a) * (dial.r - 12), dial.cy - Math.sin(a) * (dial.r - 12)]], w: 2.6, c: t.label }, { k: "dot", x: dial.cx, y: dial.cy, r: 4.5, c: t.label });
  }
  return out;
}

/** Training load: 28 days of strain as lit 3D bars with the 7-day and dashed 28-day lines, and the ratio dial (design B). */
export function LoadGfx({ points, ratio, fallback }: { points: NoopMetricPoint[]; ratio: number | null; fallback: ReactNode }) {
  const series = useMemo(() => loadSeries(points), [points]);
  const [hover, setHover] = useState<number | null>(null);
  const draw = useCallback((w: number, h: number, theme: Theme) => buildLoadPrims({ w, h, series, ratio, hover, theme }), [series, ratio, hover]);
  const { hostRef, canvasRef, status, size } = useGfx(series.count >= 2 ? draw : null);
  if (status === "failed" || series.count < 2) return <>{fallback}</>;
  const n = series.vals.length, w = size.w || 480, h = 148;
  const lay = loadLayout(w, h);
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const px = e.clientX - e.currentTarget.getBoundingClientRect().left;
    const i = Math.floor(((px - lay.plot.x0) / (lay.plot.x1 - lay.plot.x0)) * n);
    setHover(i >= 0 && i < n && typeof series.vals[i].value === "number" ? i : null);
  };
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") { e.preventDefault(); setHover((cur) => Math.max(0, Math.min(n - 1, (cur ?? (e.key === "ArrowRight" ? -1 : n)) + (e.key === "ArrowRight" ? 1 : -1)))); }
    else if (e.key === "Escape") setHover(null);
  };
  const fmt = METRICS.strain.format;
  const zone = ratio !== null ? ZONE_WORDS[ratioZone(ratio)] : null;
  const hp = hover != null ? series.vals[hover] : null;
  const tipX = hover != null ? loadSlot(hover, n, lay.plot).x : 0;
  return (
    <div ref={hostRef} className="gfx gfx--load" data-plot="" style={{ height: h }} tabIndex={0} role="group" aria-label="Daily strain with 7 and 28 day load. Arrow keys read each day."
      onPointerMove={move} onPointerLeave={() => setHover(null)} onBlur={() => setHover(null)} onKeyDown={key}>
      <canvas ref={canvasRef} className="gfx__canvas" aria-hidden="true" />
      <div className="gfx__labels" aria-hidden="true">
        <span className="gfx__tick" style={{ right: w - lay.plot.x0 + 6, top: lay.plot.y0 - 7 }}>{Math.round(series.max)}</span>
        <span className="gfx__tick" style={{ right: w - lay.plot.x0 + 6, top: lay.plot.y1 - 7 }}>0</span>
        {[0, Math.floor((n - 1) / 2), n - 1].map((i, k) => <span key={i} className="gfx__tick" data-x={k === 0 ? "start" : k === 2 ? "end" : "mid"} style={{ left: k === 0 ? lay.plot.x0 : k === 2 ? lay.plot.x1 : loadSlot(i, n, lay.plot).x }}>{k === 2 ? "Today" : shortDay(series.vals[i].day)}</span>)}
        {lay.dial && ratio !== null && (
          <>
            <span className="gfx__zone" style={{ left: lay.dial.cx, top: lay.dial.cy + 8 }}>Ratio {ratio.toFixed(2)}</span>
            <span className="gfx__tick" data-dial="" style={{ left: lay.dial.cx, top: lay.dial.cy + 26 }}>{zone} · {SWEET_LOW} to {SWEET_HIGH}</span>
            <span className="gfx__tick" style={{ left: lay.dial.cx - lay.dial.r, top: lay.dial.cy - 7, transform: "translateX(-100%)" }}>{RATIO_MIN}</span>
            <span className="gfx__tick" style={{ left: lay.dial.cx + lay.dial.r + 4, top: lay.dial.cy - 7 }}>{RATIO_MAX}</span>
          </>
        )}
      </div>
      {hover != null && hp && typeof hp.value === "number" && (
        <div className="trend-chart__tip" aria-hidden="true" style={{ left: Math.max(0, Math.min(tipX + 12, lay.plot.x1 - 170)), top: 0 }}>
          <strong>{fmt(hp.value)}</strong><span>{shortDay(hp.day)}</span><span>7-day avg {fmt(series.a7[hover as number])} · 28-day avg {fmt(series.a28[hover as number])}</span>
        </div>
      )}
      <table className="mb-sr">
        <caption>Daily strain, with 7-day and 28-day averages{ratio !== null ? `. Ratio ${ratio.toFixed(2)}, ${zone?.toLowerCase()}` : ""}</caption>
        <thead><tr><th scope="col">Day</th><th scope="col">Strain</th><th scope="col">7-day average</th><th scope="col">28-day average</th></tr></thead>
        <tbody>{series.vals.map((p, i) => <tr key={p.day}><th scope="row">{shortDay(p.day)}</th><td>{typeof p.value === "number" ? fmt(p.value) : "No reading"}</td><td>{fmt(series.a7[i])}</td><td>{fmt(series.a28[i])}</td></tr>)}</tbody>
      </table>
    </div>
  );
}
