import { useState } from "react";
import { Card, SegmentedControl, Select, Sparkline } from "../ui";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, usualRange } from "../ui/charts/chartMath";
import type { NoopMetricKey } from "../types";
import { loadTrends, useLoad, type TrendsData } from "./noopData";
import { PanelState, SampleTag } from "./PanelState";
import { METRICS, METRIC_BY_KEY, RANGES, endDay, formatValue, metricStats, windowSeries, withUnit, type MetricDef, type RangeValue } from "./trendsModel";

const short = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const long = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" });

function MainChart({ def, data, days, end }: { def: MetricDef; data: TrendsData; days: number; end: string | null }) {
  const read = data[def.key];
  if (read.status !== "ready") {
    return <p className="fx-muted" data-kind={read.status}>{read.status === "error" ? `Amara couldn't read ${def.label.toLowerCase()} just now.` : `No ${def.label.toLowerCase()} readings yet.`}</p>;
  }
  const slots = windowSeries(def, read.points, days, end);
  const stats = metricStats(slots);
  if (stats.count < 2) return <p className="fx-muted">Not enough {def.label.toLowerCase()} readings in this range. Try a longer one.</p>;
  const values = slots.map((s) => s.value);
  const unitWord = def.unit === "%" ? "%" : def.unit ? ` ${def.unit}` : "";
  const smooth = days >= 90 ? 7 : 1;
  const labels = slots.map((s) => short(s.day));
  const usual = usualRange(values);
  return (
    <figure className="fx-fig">
      <ChartHeader title={def.label} value={formatValue(def, stats.latest)} unit={def.unit} delta={deltaWords(stats.latest, stats.mean, unitWord, def.digits)} usual={`average ${formatValue(def, stats.mean)}${unitWord}`} />
      <TrendChart key={`${def.key}-${days}`} values={values} labels={labels} lastLabel={labels[labels.length - 1]} unit={def.unit || undefined} height={168}
        kind={def.bars && days <= 90 ? "bar" : "line"} smoothing={smooth} rawDots={days <= 30} usual={usual}
        {...(def.domain ? { domain: def.domain, ticks: def.ticks } : {})}
        tone={def.key === "active_kcal" ? "ember" : "you"}
        format={(v) => formatValue(def, v)}
        label={`${def.label}, last ${slots.length} days: lowest ${withUnit(def, stats.min)}, highest ${withUnit(def, stats.max)}, latest ${withUnit(def, stats.latest)}, average ${withUnit(def, stats.mean)}`}
        readout={(i) => <><strong>{long(slots[i].day)}</strong><span>{def.label} <em>{withUnit(def, values[i])}</em></span></>} />
      <figcaption className="fx-caption">Average {withUnit(def, stats.mean)} · Lowest {withUnit(def, stats.min)} · Highest {withUnit(def, stats.max)} · {stats.count} of {slots.length} days have a reading</figcaption>
    </figure>
  );
}

/** Fitness › Trends (0017 #3): every metric_series key over a week to a year, one chart plus a tile per metric. */
export function TrendsExplorer() {
  const [metric, setMetric] = useState<NoopMetricKey>("recovery");
  const [range, setRange] = useState<RangeValue>("30");
  const [load, retry] = useLoad(loadTrends, "trends");
  const days = RANGES.find((r) => r.value === range)?.days ?? 30;
  const data = load.state === "live" || load.state === "sample" ? load.value : null;
  const end = data ? endDay(Object.values(data).map((r) => (r.status === "ready" ? r.points : []))) : null;
  const def = METRIC_BY_KEY[metric];
  return (
    <div className="fx" data-tab="trends">
      <Card title="Trends" accessory={load.state === "sample" ? <SampleTag /> : undefined}>
        {!data && <PanelState load={load} what="trends" emptyHint="Charts appear after your strap has synced a few days." onRetry={retry} />}
        {data && (
          <>
            <div className="fx-controls">
              <Select label="Metric" value={metric} onChange={(e) => setMetric(e.target.value as NoopMetricKey)} options={METRICS.map((m) => ({ value: m.key, label: m.label }))} />
              <SegmentedControl label="Trend range" size="small" value={range} options={RANGES.map((r) => ({ value: r.value, label: r.label }))} onChange={setRange} />
            </div>
            <MainChart def={def} data={data} days={days} end={end} />
          </>
        )}
      </Card>
      {data && (
        <Card title="All metrics" accessory={<span className="fx-muted">{RANGES.find((r) => r.value === range)?.label.toLowerCase()}</span>}>
          <ul className="fx-metrics" aria-label="All metrics">
            {METRICS.map((m) => {
              const read = data[m.key];
              const slots = read.status === "ready" ? windowSeries(m, read.points, days, end) : [];
              const stats = metricStats(slots);
              const note = read.status === "error" ? "Couldn't read" : stats.count === 0 ? "No readings" : null;
              return (
                <li key={m.key}>
                  <button type="button" className="fx-metric" aria-pressed={m.key === metric} onClick={() => setMetric(m.key)}>
                    <span className="fx-metric__label">{m.label}</span>
                    <span className="fx-metric__value">{note ?? withUnit(m, stats.latest)}</span>
                    {stats.count > 1 && <Sparkline values={slots.map((s) => s.value)} label={m.label} color={m.color} format={(v) => formatValue(m, v)} />}
                    <span className="fx-metric__sub">{stats.count > 0 ? `average ${withUnit(m, stats.mean)}` : " "}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </Card>
      )}
    </div>
  );
}
