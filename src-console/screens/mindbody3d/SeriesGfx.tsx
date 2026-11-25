/* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex -- a chart is one focusable group: arrow keys read each day, and its facts are also in the hidden table */
import { useCallback, useMemo, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { PLOT, nearestIndex, plotBox, seriesDomain, stepIndex, xOf, yOf, type Kind, type Usual } from "./geometry";
import { buildSeriesPrims } from "./seriesPrims";
import { useGfx } from "./useGfx";
import "./mindbody3d.css";

export type SeriesRow = { day: string; value: string; note?: string };

export interface SeriesGfxProps {
  values: readonly (number | null)[];
  labels: readonly string[];
  label: string;
  format: (v: number) => string;
  unit?: string;
  kind?: Kind;
  usual: Usual | null;
  height: number;
  domain?: [number, number];
  ticks?: number[];
  xTicks?: number[];
  lastLabel: string;
  markers?: readonly boolean[];
  readout: (i: number) => ReactNode;
  /** The facts as text for the visually hidden table (most recent days). */
  rows: readonly SeriesRow[];
  /** What to show when WebGL is unavailable or fails: the existing SVG chart. */
  fallback: ReactNode;
}

const TIP_W = 170;

/** Mind–Body line / bar chart (Readiness trends, Sleep nights) drawn in Three.js; falls back to `fallback`. */
export function SeriesGfx({ values, labels, label, format, unit, kind = "line", usual, height, domain, ticks, xTicks, lastLabel, markers, readout, rows, fallback }: SeriesGfxProps) {
  const [hover, setHover] = useState<number | null>(null);
  const dom = useMemo(() => seriesDomain(values, usual, kind, domain, ticks), [values, usual, kind, domain, ticks]);
  const draw = useCallback((w: number, h: number, theme: Parameters<typeof buildSeriesPrims>[0]["theme"]) => buildSeriesPrims({ w, h, values, usual, kind, dom: dom!, markers, hover, theme }), [values, usual, kind, dom, markers, hover]);
  const { hostRef, canvasRef, status, size } = useGfx(dom ? draw : null);
  if (status === "failed" || !dom) return <>{fallback}</>;

  const n = values.length, W = size.w || 480;
  const box = plotBox(W, height, { markers: !!markers });
  const X = (i: number) => xOf(i, n, box, kind);
  const onMove = (e: PointerEvent<HTMLDivElement>) => setHover(nearestIndex(e.clientX - e.currentTarget.getBoundingClientRect().left, values, box, kind));
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") { e.preventDefault(); setHover((h) => stepIndex(h, e.key === "ArrowRight" ? 1 : -1, values)); }
    else if (e.key === "Home") { e.preventDefault(); setHover(stepIndex(null, 1, values)); }
    else if (e.key === "End") { e.preventDefault(); setHover(stepIndex(null, -1, values)); }
    else if (e.key === "Escape") setHover(null);
  };
  const xs = xTicks ?? [...new Set([0, Math.round((n - 1) / 2), n - 1])];
  const hv = hover != null ? values[hover] : null;
  const flip = hover != null && X(hover) + 12 + TIP_W > W;
  const tipLeft = hover == null ? 0 : flip ? Math.max(0, X(hover) - 12 - TIP_W) : Math.min(Math.max(X(hover) + 12, 0), W - 180);
  return (
    <div ref={hostRef} className="gfx" data-kind={kind} style={{ height }} tabIndex={0} role="group" aria-label={`${label}. Arrow keys read each day.`}
      onPointerMove={onMove} onPointerLeave={() => setHover(null)} onBlur={() => setHover(null)} onKeyDown={onKey}>
      <canvas ref={canvasRef} className="gfx__canvas" aria-hidden="true" />
      <div className="gfx__labels" aria-hidden="true">
        {dom.ticks.map((t, k) => <span key={t} className="gfx__tick" style={{ right: W - box.x0 + 6, top: yOf(t, dom.lo, dom.hi, box) - 7 }}>{format(t)}{k === dom.ticks.length - 1 && unit ? <em> {unit}</em> : null}</span>)}
        {xs.map((i, k) => <span key={`x${i}`} className="gfx__tick" data-x={k === 0 ? "start" : k === xs.length - 1 ? "end" : "mid"} style={{ left: X(i), bottom: 0 }}>{i === n - 1 ? lastLabel : labels[i]}</span>)}
      </div>
      {hover != null && hv != null && <div className="trend-chart__tip" aria-hidden="true" style={{ left: tipLeft, top: PLOT.top }}>{readout(hover)}</div>}
      <p className="mb-sr" role="status" aria-live="polite">{hover != null && hv != null ? `${labels[hover]}: ${format(hv)}${unit ? ` ${unit}` : ""}` : ""}</p>
      <table className="mb-sr">
        <caption>{label}</caption>
        <thead><tr><th scope="col">Day</th><th scope="col">Value</th><th scope="col">Against your usual</th></tr></thead>
        <tbody>{rows.map((r) => <tr key={r.day}><th scope="row">{r.day}</th><td>{r.value}</td><td>{r.note ?? ""}</td></tr>)}</tbody>
      </table>
    </div>
  );
}
