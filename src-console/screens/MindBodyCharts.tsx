import { chartGeometry, type MetricReading } from "../lib/mindBodyDeep";
import type { NoopMetricPoint } from "../types";

/**
 * One line against his own baseline: a faint band for the usual range, a thin line for the days, a
 * point of light on the latest one. No axes or grid: the band is the only reference that matters.
 */
export function BaselineChart({ points, reading, tall = false }: { points: NoopMetricPoint[]; reading: MetricReading; tall?: boolean }) {
  const { path, band, mean, last } = chartGeometry(points, reading.base);
  const shown = points.filter((p) => typeof p.value === "number").length;
  if (shown < 2) return <p className="mb-chart__empty">Not enough days to draw yet</p>;
  const label = `${reading.def.label}, last ${points.length} days, ${shown} readings${reading.base ? `, usual ${reading.def.format(reading.base.mean)}` : ""}`;
  return (
    <div className="mb-chart" data-tall={tall || undefined}>
      <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={label}>
        {band && <rect className="mb-chart__band" x="0" width="100" y={band.top} height={Math.max(band.bottom - band.top, .5)} />}
        {mean !== null && <line className="mb-chart__mean" x1="0" x2="100" y1={mean} y2={mean} />}
        <path className="mb-chart__line" d={path} fill="none" />
      </svg>
      {last && <i className="mb-chart__now" style={{ left: `${(last as { x: number }).x}%`, top: `${(last as { y: number }).y}%` }} aria-hidden="true" />}
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
    <div className="mb-stages" role="img" aria-label={stages.map((stage) => `${stage.label} ${Math.round(stage.minutes)} minutes`).join(", ")}>
      {stages.map((stage) => <i key={stage.label} data-tone={stage.tone} style={{ flexGrow: stage.minutes }} />)}
    </div>
  );
}
