import { type MetricReading } from "../lib/mindBodyDeep";
import { shortDay } from "../lib/mindBody";
import { TrendChart } from "../ui/charts/TrendChart";
import { deltaWords } from "../ui/charts/chartMath";

const DASH = "\u2014";
import type { NoopMetricPoint } from "../types";

/**
 * design-1110c: one line against his own baseline, in the shared chart vocabulary: the copper band
 * is his usual (mean ± 1 sd), the dashed copper line its middle, emerald is the 3-day average with
 * each day's reading as a dot. Hover reads any day back exactly.
 */
export function BaselineChart({ points, reading, tall = false, bars = false }: { points: NoopMetricPoint[]; reading: MetricReading; tall?: boolean; bars?: boolean }) {
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
  return (
    <div className="mb-chart" data-plot="" data-tall={tall || undefined}>
      <TrendChart
        values={values}
        labels={points.map((p) => shortDay(p.day))}
        label={label}
        format={tick}
        unit={unit}
        usual={usual}
        height={tall ? 148 : 112}
        {...(bars ? { kind: "bar" as const, smoothing: 1, domain: [0, Math.max(10, Math.ceil(Math.max(...values.filter((v): v is number => v != null)) / 5) * 5)] as [number, number] } : {})}
        nowClassName="mb-chart__now"
        readout={(i) => {
          const raw = points[i]?.value;
          return (
            <>
              <strong>{typeof raw === "number" ? reading.def.format(raw) : DASH}</strong>
              <span>{shortDay(points[i].day)}</span>
              {base && typeof raw === "number" && <span>{deltaWords(raw * k, base.mean * k, unitSuffix(reading, hours), hours ? 1 : 0)?.text}</span>}
            </>
          );
        }}
      />
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
  return deltaWords(hours ? reading.value / 60 : reading.value, hours ? reading.base.mean / 60 : reading.base.mean, unitSuffix(reading), hours ? 1 : reading.def.key === "strain" ? 1 : 0);
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
