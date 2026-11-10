import { useEffect, useId, useMemo, useRef, useState, type PointerEvent, type ReactNode } from "react";
import { monotonePath, niceTicks, rollingMean } from "./chartMath";
import "./charts.css";

export type Usual = { low: number; high: number; median: number };

export interface TrendChartProps {
  /** One value per slot, oldest first; null = no reading that day. */
  values: readonly (number | null)[];
  /** X label per slot (e.g. "Oct 21"); the last is shown as `lastLabel`. */
  labels: readonly string[];
  /** Accessible summary of the whole chart. */
  label: string;
  format?: (v: number) => string;
  /** Unit after the top tick (C4). */
  unit?: string;
  /** "line": smooth 3-day average + daily dots (C1–C3). "bar": rounded bars (C10). */
  kind?: "line" | "bar";
  /** Average window for the line; 1 = draw the values themselves. */
  smoothing?: number;
  usual?: Usual | null;
  height?: number;
  /** "you" = emerald, "ember" = Home's heart colour (C14). */
  tone?: "you" | "ember";
  /** Per-slot markers under the plot (e.g. workout days), drawn as small triangles. */
  markers?: readonly boolean[];
  /** Hover readout lines for slot i (C6/C17). Defaults to label · value. */
  readout?: (i: number) => ReactNode;
  lastLabel?: string;
  /** Extra class on the latest-value dot (layout tests and page CSS hook into it). */
  nowClassName?: string;
  domain?: [number, number];
  /** Controlled hover, so stacked charts share one crosshair (C17). */
  hover?: number | null;
  onHover?: (i: number | null) => void;
  /** Shaded slots from the start (e.g. the first 90 s "settling in", C18). */
  shadeTo?: number;
  shadeLabel?: string;
  /** Hide the x-axis labels (inner rows of a stack). */
  hideX?: boolean;
}

const GUTTER = 34, RIGHT = 6, TOP = 8, AXIS = 18, MARK = 10;

/** Width of the element, kept current with a ResizeObserver (no layout reads during render). */
function useWidth<T extends HTMLElement>(fallback: number) {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(fallback);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => { const w = el.clientWidth; if (w > 0) setWidth(w); };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width] as const;
}

/**
 * design-1110c: the one chart every health graph uses. Emerald = you, copper = your usual (a soft
 * band for the middle half plus a thin dashed median). Hairline gridlines with the unit on the top
 * tick. The line is a monotone curve through a short rolling average, and each day's real value
 * stays visible as a dot, so smoothing hides nothing. Hover shows the exact values.
 */
export function TrendChart({ values, labels, label, format = (v) => String(Math.round(v)), unit, kind = "line", smoothing = 3, usual = null, height = 120, tone = "you", markers, readout, lastLabel = "Today", nowClassName, domain, hover: hoverProp, onHover, shadeTo, shadeLabel, hideX = false }: TrendChartProps) {
  const gradient = useId().replace(/:/g, "");
  const [ref, W] = useWidth<HTMLDivElement>(480);
  const [ownHover, setOwnHover] = useState<number | null>(null);
  const hover = hoverProp !== undefined ? hoverProp : ownHover;
  const setHover = (i: number | null) => { if (hoverProp === undefined) setOwnHover(i); onHover?.(i); };
  const n = values.length;
  const nums = values.filter((v): v is number => typeof v === "number" && Number.isFinite(v));
  const H = height, B = (hideX ? 4 : AXIS) + (markers ? MARK : 0);
  const pw = Math.max(1, W - GUTTER - RIGHT), ph = Math.max(1, H - TOP - B);
  const { lo, hi, ticks } = useMemo(() => {
    if (domain) return { lo: domain[0], hi: domain[1], ticks: niceTicks(domain[0], domain[1]) };
    const all = [...nums, ...(usual ? [usual.low, usual.high] : [])];
    const t = niceTicks(kind === "bar" ? 0 : Math.min(...all), Math.max(...all));
    return { lo: t[0] ?? 0, hi: t[t.length - 1] ?? 1, ticks: t };
  }, [domain, nums, usual, kind]);
  const x = (i: number) => kind === "bar" ? GUTTER + (pw / n) * (i + .5) : GUTTER + (n <= 1 ? pw / 2 : (pw * i) / (n - 1));
  const y = (v: number) => TOP + ph - ((Math.max(lo, Math.min(hi, v)) - lo) / (hi - lo || 1)) * ph;
  const line = useMemo(() => (smoothing > 1 ? rollingMean(values, smoothing) : [...values]), [values, smoothing]);
  if (nums.length < 2) return <p className="trend-chart__empty">Not enough days to draw yet</p>;

  // Runs of consecutive readings: a missing day breaks the line rather than bridging it.
  const runs: Array<Array<[number, number]>> = [[]];
  line.forEach((v, i) => { if (v == null) { if (runs[runs.length - 1].length) runs.push([]); } else runs[runs.length - 1].push([x(i), y(v)]); });
  const paths = runs.filter((r) => r.length > 0).map((r) => ({ d: r.length === 1 ? `M${r[0][0]},${r[0][1]}h0.1` : monotonePath(r), r }));
  const lastIndex = values.map((v) => v != null).lastIndexOf(true);
  const base = y(lo);
  const xTicks = [...new Set([0, Math.round((n - 1) / 2), n - 1])];

  const onMove = (e: PointerEvent<SVGSVGElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - box.left;
    let best = -1, dist = Infinity;
    values.forEach((v, i) => { if (v == null) return; const d = Math.abs(x(i) - px); if (d < dist) { dist = d; best = i; } });
    setHover(best >= 0 ? best : null);
  };
  const hv = hover != null ? values[hover] : null;
  const tipLeft = hover != null ? Math.min(Math.max(x(hover) + 12, 0), W - 180) : 0;
  const flip = hover != null && x(hover) + 12 + 170 > W;

  return (
    <div ref={ref} className="trend-chart" data-tone={tone} data-kind={kind} style={{ height: H }}>
      <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={label} onPointerMove={onMove} onPointerLeave={() => setHover(null)}>
        <defs>
          <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" className="trend-chart__stop0" /><stop offset="1" className="trend-chart__stop1" />
          </linearGradient>
        </defs>
        <g aria-hidden="true">
          {ticks.map((t, k) => (
            <g key={t}>
              <line className="trend-chart__grid" data-base={k === 0 || undefined} x1={GUTTER} x2={W - RIGHT} y1={y(t)} y2={y(t)} />
              <text className="trend-chart__tick" x={GUTTER - 6} y={y(t) + 3.5} textAnchor="end">{format(t)}{k === ticks.length - 1 && unit ? <tspan className="trend-chart__unit"> {unit}</tspan> : null}</text>
            </g>
          ))}
          {usual && <rect className="trend-chart__band" x={GUTTER} width={pw} y={y(usual.high)} height={Math.max(1, y(usual.low) - y(usual.high))} rx={3} />}
          {usual && <line className="trend-chart__usual" x1={GUTTER} x2={W - RIGHT} y1={y(usual.median)} y2={y(usual.median)} />}
          {kind === "bar" ? values.map((v, i) => {
            if (v == null) return null;
            const bw = (pw / n) * .62, bx = x(i) - bw / 2, top = y(v), r = Math.min(3, bw / 2);
            const solid = i === lastIndex || i === hover;
            return <path key={i} className="trend-chart__bar" data-solid={solid || undefined} fill={solid ? undefined : `url(#${gradient})`}
              d={`M${bx},${base}V${top + r}Q${bx},${top} ${bx + r},${top}H${bx + bw - r}Q${bx + bw},${top} ${bx + bw},${top + r}V${base}Z`} />;
          }) : (
            <>
              {paths.map(({ d, r }) => r.length > 1 && <path key={`a${d}`} className="trend-chart__area" fill={`url(#${gradient})`} d={`${d}L${r[r.length - 1][0]},${base}L${r[0][0]},${base}Z`} />)}
              {paths.map(({ d }) => <path key={d} className="trend-chart__line" d={d} />)}
              {smoothing > 1 && values.map((v, i) => v != null && i !== lastIndex && <circle key={i} className="trend-chart__raw" cx={x(i)} cy={y(v)} r={1.6} />)}
            </>
          )}
          {markers?.map((m, i) => m && <path key={`m${i}`} className="trend-chart__marker" d={`M${x(i) - 3} ${TOP + ph + 9} L${x(i)} ${TOP + ph + 4} L${x(i) + 3} ${TOP + ph + 9} Z`} />)}
          {shadeTo != null && shadeTo > 0 && <rect className="trend-chart__shade" x={GUTTER} y={TOP} width={Math.max(0, x(Math.min(shadeTo, n - 1)) - GUTTER)} height={ph} />}
          {shadeTo != null && shadeLabel && <text className="trend-chart__tick" x={GUTTER + 4} y={TOP + 11}>{shadeLabel}</text>}
          {!hideX && xTicks.map((i, k) => (
            <text key={`x${i}`} className="trend-chart__tick" x={x(i)} y={H - 4} textAnchor={k === 0 ? "start" : k === xTicks.length - 1 ? "end" : "middle"}>{i === n - 1 ? lastLabel : labels[i]}</text>
          ))}
          {hover != null && <line className="trend-chart__cross" x1={x(hover)} x2={x(hover)} y1={TOP - 4} y2={TOP + ph} />}
          {hover != null && hv != null && kind === "line" && <circle className="trend-chart__hover" cx={x(hover)} cy={y(line[hover] ?? hv)} r={4.5} />}
        </g>
        {kind === "line" && lastIndex >= 0 && <circle className={nowClassName ? `trend-chart__now ${nowClassName}` : "trend-chart__now"} cx={x(lastIndex)} cy={y(values[lastIndex] as number)} r={3.5} aria-hidden="true" />}
      </svg>
      {hover != null && hv != null && (readout || hoverProp === undefined) && (
        <div className="trend-chart__tip" aria-hidden="true" style={{ left: flip ? Math.max(0, x(hover) - 12 - 170) : tipLeft, top: TOP }}>
          {readout ? readout(hover) : <><strong>{format(hv)}{unit ? ` ${unit}` : ""}</strong><span>{labels[hover]}</span></>}
        </div>
      )}
    </div>
  );
}

/** C6: one header for every chart: title, today's value with its unit, and the change vs usual in words. */
export function ChartHeader({ title, value, unit, delta, usual }: { title: string; value: ReactNode; unit?: string; delta?: { text: string; dir: string } | null; usual?: ReactNode }) {
  return (
    <header className="chart-head">
      <div className="chart-head__main"><h3>{title}</h3><p className="chart-head__value">{value}{unit && <small>{unit}</small>}</p></div>
      <div className="chart-head__side">
        {delta && <span className="chart-head__delta" data-dir={delta.dir}>{delta.text}</span>}
        {usual && <span className="chart-head__usual">{usual}</span>}
      </div>
    </header>
  );
}
