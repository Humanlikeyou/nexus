import { Fragment } from "react";
import type { DayMark } from "./adapters";

/** `**64**` → bold. Only sample and templated copy use it (numbers come from the source row). */
export function Rich({ text }: { text: string }) {
  return <>{text.split(/\*\*(.+?)\*\*/g).map((p, i) => (i % 2 ? <b key={i}>{p}</b> : <Fragment key={i}>{p}</Fragment>))}</>;
}

const MARK_WORD: Record<DayMark, string> = { met: "met", nearly: "nearly", missed: "missed", none: "no data" };
/** Seven day squares, today outlined. Labelled with the words, never colour alone. */
export function Dots({ days, label }: { days: DayMark[]; label: string }) {
  return (
    <span className="pk-dots" role="img" aria-label={`${label}: ${days.map((d) => MARK_WORD[d]).join(", ")}`}>
      {days.map((d, i) => <i key={i} data-on={d === "met" || undefined} data-part={d === "nearly" || undefined} data-today={i === days.length - 1 || undefined} />)}
    </span>
  );
}

export function Bar({ pct, copper = false }: { pct: number; copper?: boolean }) {
  return <div className={copper ? "pk-bar pk-bar--copper" : "pk-bar"} aria-hidden="true"><i style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} /></div>;
}

export function Confidence({ n, label }: { n: number; label: string }) {
  return <span className="pk-conf" aria-label={`Confidence ${n} of 5: ${label}`}>{[1, 2, 3, 4, 5].map((i) => <i key={i} data-on={i <= n || undefined} />)}&nbsp;{label}</span>;
}

export function Scatter({ points, split, x, y, xLabel, yLabel }: { points: Array<[number, number]>; split: number; x: [number, number]; y: [number, number]; xLabel: string; yLabel: string }) {
  const w = 340, h = 170;
  const X = (v: number) => 34 + ((v - x[0]) / (x[1] - x[0])) * (w - 47);
  const Y = (v: number) => h - 21 - ((v - y[0]) / (y[1] - y[0])) * (h - 34);
  const xs = [x[0], (x[0] + x[1]) / 2, x[1]], ys = [y[0], (y[0] + y[1]) / 2, y[1]];
  const above = points.filter(([px]) => px >= split), below = points.filter(([px]) => px < split);
  const mean = (a: Array<[number, number]>) => (a.length ? Math.round(a.reduce((s, p) => s + p[1], 0) / a.length) : 0);
  return (
    <svg className="pk-svg" viewBox={`0 0 ${w} ${h}`} role="img" aria-label={`${yLabel} against ${xLabel}: ${points.length} points; ${above.length} at ${split}+ average ${mean(above)}, ${below.length} under ${split} average ${mean(below)}`}>
      {ys.map((v) => <g key={v}><line className="gr" x1="34" x2={w - 13} y1={Y(v)} y2={Y(v)} /><text x="29" y={Y(v) + 4} textAnchor="end">{v}</text></g>)}
      <line className="ax" x1={X(split)} x2={X(split)} y1="8" y2={h - 21} strokeDasharray="3 3" />
      {xs.map((v) => <text key={v} x={X(v)} y={h - 5} textAnchor="middle">{`${v}h`}</text>)}
      {points.map(([px, py], i) => <circle key={i} className={px >= split ? "d1" : "d2"} cx={X(px).toFixed(1)} cy={Y(py).toFixed(1)} r="3.5" />)}
    </svg>
  );
}

export function PairBars({ rows, legend, max = 80 }: { rows: Array<[string, number, number]>; legend: [string, string]; max?: number }) {
  const bw = 21, gap = 55, h = 110, w = 34 + rows.length * (2 * bw + gap);
  const Y = (v: number) => h - 21 - (v / max) * (h - 34);
  return (
    <svg className="pk-svg" viewBox={`0 0 ${w} ${h}`} style={{ width: w, maxWidth: "100%" }} role="img" aria-label={rows.map(([l, a, b]) => `${l}: ${a} ${legend[0]}, ${b} ${legend[1]}`).join("; ")}>
      {[0, max / 2, max].map((v) => <g key={v}><line className="gr" x1="34" x2={w - 8} y1={Y(v)} y2={Y(v)} /><text x="29" y={Y(v) + 4} textAnchor="end">{v}</text></g>)}
      {rows.map(([label, a, b], i) => {
        const x = 34 + i * (2 * bw + gap);
        return (
          <g key={label}>
            <rect className="bar2" x={x} y={Y(a)} width={bw} height={h - 21 - Y(a)} rx="3" />
            <rect className="bar1" x={x + bw + 3} y={Y(b)} width={bw} height={h - 21 - Y(b)} rx="3" />
            <text className="tl" x={x + bw / 2} y={Y(a) - 5} textAnchor="middle">{a}</text>
            <text className="tl" x={x + bw * 1.5 + 3} y={Y(b) - 5} textAnchor="middle">{b}</text>
            <text x={x + bw + 1} y={h - 5} textAnchor="middle">{label}</text>
          </g>
        );
      })}
    </svg>
  );
}

export const money = (v: number) => v.toLocaleString("en-US", { style: "currency", currency: "USD", minimumFractionDigits: Number.isInteger(v) ? 0 : 2 });
