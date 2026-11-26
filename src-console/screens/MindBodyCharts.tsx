import { type MetricReading } from "../lib/mindBodyDeep";
import { shortDay } from "../lib/mindBody";
import { TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, todayJst } from "../ui/charts/chartMath";

const DASH = "\u2014";
import type { NoopMetricPoint } from "../types";
import { SeriesGfx } from "./mindbody3d/SeriesGfx";

/**
 * design-1110c: one line against his own baseline, in the shared chart vocabulary: the copper band
 * is his usual (mean ± 1 sd), the dashed copper line its middle, emerald goes through each day's own
 * reading (a dot), breaking at days with none. Hover reads any day back exactly.
 */
export function BaselineChart({ points, reading, tall = false, bars = false, markers }: { points: NoopMetricPoint[]; reading: MetricReading; tall?: boolean; bars?: boolean; markers?: boolean[] }) {
  const shown = points.filter((p) => typeof p.value === "number").length;
  if (shown < 2) return <p className="mb-chart__empty">Not enough days to draw yet</p>;
  const label = `${reading.def.label}, last ${points.length} days, ${shown} readings${reading.base ? `, usual ${reading.def.format(reading.base.mean)}` : ""}`;
  // Sleep is stored in minutes but read in hours; every other metric plots as it is.
  const hours = reading.def.key === "sleep";
  const k = hours ? 1 / 60 : 1;
  const values = points.map((p) => (typeof p.value === "number" && Number.isFinite(p.value) ? p.value * k : null));
  const base = reading.base;
  const usual = base ? { low: (base.mean - base.sd) * k, high: (base.mean + base.sd) * k, median: base.mean * k } : null;
  const unit = hours ? "h" : reading.def.unit.trim() || undefined;
  const tick = (v: number) => (Number.isInteger(v) ? String(v) : v.toFixed(1));
  const domainProps = bars && hours ? { kind: "bar" as const, smoothing: 1, domain: [0, 10] as [number, number], ticks: [0, 5, 10] } : bars ? { kind: "bar" as const, smoothing: 1 } : {};
  const lastLabel = points[points.length - 1].day === todayJst() ? "Today" : shortDay(points[points.length - 1].day);
  const xTicks = points.length > 21 ? [0, 7, 14, 21, points.length - 1] : undefined;
  const readout = (i: number) => {
    const raw = points[i]?.value;
    return (
      <>
        <strong>{typeof raw === "number" ? reading.def.format(raw) : DASH}</strong>
        <span>{shortDay(points[i].day)}</span>
        {base && typeof raw === "number" && <span>{deltaWords(raw * k, base.mean * k, unitSuffix(reading, hours), hours ? 1 : 0)?.text}</span>}
      </>
    );
  };
  const height = tall ? 148 : 112;
  const svg = (
    <TrendChart
      values={values}
      labels={points.map((p) => shortDay(p.day))}
      label={label}
      format={tick}
      unit={unit}
      usual={usual}
      height={height}
      {...domainProps}
      nowClassName="mb-chart__now"
      lastLabel={lastLabel}
      xTicks={xTicks}
      markers={markers}
      readout={readout}
    />
  );
  // The facts as text for the hidden table: the most recent 31 days, each against his usual.
  const rows = points.slice(-31).map((p) => ({ day: shortDay(p.day), value: typeof p.value === "number" ? reading.def.format(p.value) : DASH, note: base && typeof p.value === "number" ? deltaWords(p.value * k, base.mean * k, unitSuffix(reading, hours), hours ? 1 : 0)?.text : undefined }));
  return (
    <div className="mb-chart" data-plot="" data-tall={tall || undefined}>
      <SeriesGfx values={values} labels={points.map((p) => shortDay(p.day))} label={label} format={tick} unit={unit} kind={bars ? "bar" : "line"} usual={usual} height={height}
        domain={bars && hours ? [0, 10] : undefined} ticks={bars && hours ? [0, 5, 10] : undefined} xTicks={xTicks} lastLabel={lastLabel} markers={markers} readout={readout} rows={rows} fallback={svg} />
    </div>
  );
}

/** The unit as it follows a number in words: "6 ms", "4%", "0.5h". */
export function unitSuffix(reading: MetricReading, hours = reading.def.key === "sleep") {
  const unit = reading.def.unit.trim();
  return hours ? "h" : !unit ? "" : unit === "%" ? "%" : ` ${unit}`;
}

/** C6: today against his usual, in words with an arrow, for the row header. */
export function readingDelta(reading: MetricReading) {
  if (reading.value === null || !reading.base) return null;
  const hours = reading.def.key === "sleep";
  return deltaWords(hours ? reading.value / 60 : reading.value, hours ? reading.base.mean / 60 : reading.base.mean, reading.def.unit === "%" ? " pts" : unitSuffix(reading), hours ? 1 : reading.def.key === "strain" ? 1 : 0);
}

const clockOf = (iso: string) => { const d = new Date(iso); return new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }).format(d); };
/** Minutes since local midnight (Tokyo). */
const minutesOf = (iso: string) => { const [h, m] = clockOf(iso).split(":").map(Number); return h * 60 + m; };

/** Bed → wake on a clock axis, in minutes (evening bedtimes sit before midnight). Shared by the SVG/HTML and Three.js spans. */
export function spanAxis(start: string, end: string) {
  let s = minutesOf(start), e = minutesOf(end);
  if (s > 12 * 60) s -= 1440; // evening bedtimes sit before midnight on the axis
  if (e < s) e += 1440;
  const a0 = Math.floor((s - 30) / 60) * 60, a1 = Math.max(a0 + 240, Math.ceil((e + 15) / 30) * 30);
  const hours: number[] = [];
  for (let m = a0; m <= a1; m += 120) hours.push(m);
  return { s, e, a0, a1, hours, pct: (m: number) => ((m - a0) / (a1 - a0)) * 100, label: (m: number) => `${String((((Math.round(m / 60) % 24) + 24) % 24)).padStart(2, "0")}:00`, startClock: clockOf(start), endClock: clockOf(end) };
}

/** design-1110c C9: bed → wake on a light clock axis (every 2 h), from the real night record. */
export function SleepSpan({ start, end }: { start: string; end: string }) {
  const { s, e, hours, pct, label } = spanAxis(start, end);
  return (
    <div className="mb-span" role="img" aria-label={`In bed ${clockOf(start)} to ${clockOf(end)}`}>
      <div className="mb-span__track"><i style={{ left: `${pct(s)}%`, width: `${pct(e) - pct(s)}%` }} /></div>
      <div className="mb-span__axis" aria-hidden="true">
        {hours.map((m) => <span key={m} style={{ left: `${pct(m)}%` }}>{label(m)}</span>)}
        <b style={{ left: `${pct(s)}%` }}>Bed {clockOf(start)}</b>
        <b data-end="" style={{ left: `${pct(e)}%` }}>Wake {clockOf(end)}</b>
      </div>
    </div>
  );
}

/** How far today sits from his usual, drawn from a centre line. */
export function DeltaBar({ reading }: { reading: MetricReading }) {
  const { z } = reading;
  if (z === null) return <span className="mb-delta" aria-hidden="true" />;
  const reach = Math.min(Math.abs(z), 2.5) / 2.5 * 50;
  return (
    <span className="mb-delta" aria-hidden="true">
      <i className="mb-delta__fill" data-side={z >= 0 ? "right" : "left"} style={{ width: `${reach}%` }} />
    </span>
  );
}

/** Sleep stages as one fine strip. */
export function StageStrip({ stages }: { stages: Array<{ label: string; minutes: number; tone: string }> }) {
  const total = stages.reduce((sum, stage) => sum + stage.minutes, 0);
  if (total <= 0) return null;
  return (
    <div className="mb-stages" data-plot="" role="img" aria-label={stages.map((stage) => `${stage.label} ${Math.round(stage.minutes)} minutes`).join(", ")}>
      {stages.map((stage) => <i key={stage.label} data-tone={stage.tone} style={{ flexGrow: stage.minutes }} />)}
    </div>
  );
}
