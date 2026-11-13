import { useState } from "react";
import type { Hypnogram } from "../bridge";
import { formatMinutes } from "../lib/format";
import type { NoopDailyMetric } from "../types";
import { Card, Meter, More, Select, StatTile } from "../ui";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { usualRange } from "../ui/charts/chartMath";
import { loadHypnogram, useLoad } from "./noopData";
import { PanelState, SampleTag } from "./PanelState";
import { STAGE_LABEL, STAGE_ORDER, hypnogramModel, nightOptions, nightSeries, type HypnoModel } from "./sleepDetailModel";

const ROW_H = 28;

export function HypnogramChart({ model }: { model: HypnoModel }) {
  const h = ROW_H * STAGE_ORDER.length;
  const mid = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }).format(new Date(((model.startSec + model.endSec) / 2) * 1000));
  return (
    <div className="fx-hypno">
      <ul className="fx-hypno__rows" aria-hidden="true">{STAGE_ORDER.map((s) => <li key={s}>{STAGE_LABEL[s]}</li>)}</ul>
      <div className="fx-hypno__plot">
        <svg viewBox={`0 0 100 ${h}`} preserveAspectRatio="none" height={h} role="img" aria-label={model.summary}>
          {STAGE_ORDER.map((s, i) => <line key={s} className="fx-hypno__grid" x1="0" x2="100" y1={i * ROW_H + ROW_H / 2} y2={i * ROW_H + ROW_H / 2} />)}
          {model.blocks.map((b, i) => <rect key={i} className="fx-hypno__block" data-stage={b.stage} x={b.x} width={Math.max(b.w, .25)} y={b.row * ROW_H + 4} height={ROW_H - 8} rx=".6" />)}
        </svg>
        <div className="fx-hypno__axis" aria-hidden="true"><span>{model.bed}</span><span>{mid}</span><span>{model.wake}</span></div>
      </div>
    </div>
  );
}

function StageBreakdown({ model }: { model: HypnoModel }) {
  const order = ["deep", "rem", "light", "awake"] as const;
  return (
    <div className="fx-block">
      <h3 className="fx-h">Time in each stage</h3>
      <Meter label="Time in each sleep stage" format={formatMinutes} segments={order.map((k) => ({ label: STAGE_LABEL[k], value: model.minutes[k], color: `var(--fx-stage-${k})` }))} />
      <ul className="fx-legend">
        {order.map((k) => <li key={k}><i style={{ background: `var(--fx-stage-${k})` }} aria-hidden="true" />{STAGE_LABEL[k]} <span className="fx-num">{formatMinutes(model.minutes[k])} · {Math.round(model.pct[k])}%</span></li>)}
      </ul>
    </div>
  );
}

/** Fitness › Sleep detail (0017 #2): a night's hypnogram from noop_hypnogram, plus 14 nights from the health snapshot. */
export function SleepDetailPanel({ days }: { days: readonly NoopDailyMetric[] }) {
  const nights = nightOptions(days, 14);
  const [picked, setPicked] = useState<string>("");
  const day = picked && nights.some((n) => n.day === picked) ? picked : "";
  const [load, retry] = useLoad(() => loadHypnogram(day || undefined), `hypno-${day}`);
  const hypno: Hypnogram | null = load.state === "live" || load.state === "sample" ? load.value : null;
  const model = hypno ? hypnogramModel(hypno) : null;
  const night = days.find((d) => d.day === (hypno?.day ?? (day || nights[0]?.day)));
  const series = nightSeries(days, 14);
  const hoursOnly = series.hours.filter((v): v is number => v != null);
  const effOnly = series.efficiencyPct.filter((v): v is number => v != null);
  const tagged = load.state === "sample";
  return (
    <div className="fx" data-tab="sleep">
      <Card title="Sleep stages" accessory={tagged ? <SampleTag /> : undefined}>
        {nights.length > 0 && (
          <Select label="Night to show" value={day} onChange={(e) => setPicked(e.target.value)} options={[{ value: "", label: "Latest night" }, ...nights.map((n) => ({ value: n.day, label: n.label }))]} />
        )}
        {!model && <PanelState load={load.state === "live" || load.state === "sample" ? { state: "empty" } : load} what="sleep stages" emptyHint="Stage timing arrives once the strap has finished scoring the night." onRetry={retry} />}
        {model && (
          <>
            <HypnogramChart model={model} />
            <p className="fx-muted">{model.bed} to {model.wake} · {model.awakenings} {model.awakenings === 1 ? "awakening" : "awakenings"}</p>
            <StageBreakdown model={model} />
            <div className="fx-tiles">
              <StatTile label="Time asleep" value={night?.totalSleepMin ? formatMinutes(night.totalSleepMin) : null} />
              <StatTile label="Efficiency" value={night?.efficiency ? `${Math.round(night.efficiency * 100)}` : null} unit="%" />
              <StatTile label="Resting HR" value={night?.restingHr ? String(Math.round(night.restingHr)) : null} unit="bpm" />
              <StatTile label="HRV" value={night?.avgHrv ? String(Math.round(night.avgHrv)) : null} unit="ms" />
            </div>
            <More label="Stage timeline as text">
              <ol className="fx-timeline">
                {model.blocks.map((b, i) => <li key={i}>{b.start} to {b.end}: {STAGE_LABEL[b.stage]}, {formatMinutes(b.minutes)}</li>)}
              </ol>
            </More>
          </>
        )}
      </Card>

      <Card title="Last 14 nights">
        {series.nights < 2
          ? <p className="fx-muted">Nightly charts appear after two nights of sleep records.</p>
          : (
            <div className="fx-grid2">
              <figure className="fx-fig">
                <ChartHeader title="Time asleep" value={hoursOnly.length ? hoursOnly[hoursOnly.length - 1].toFixed(1) : "—"} unit="h" usual={`${series.nights} nights`} />
                <TrendChart kind="bar" values={series.hours} labels={series.labels} lastLabel={series.labels[series.labels.length - 1]} unit="h" domain={[0, 10]} ticks={[0, 5, 10]} height={124}
                  label={`Time asleep, last ${series.days.length} nights: min ${Math.min(...hoursOnly).toFixed(1)} h, max ${Math.max(...hoursOnly).toFixed(1)} h, last ${hoursOnly[hoursOnly.length - 1].toFixed(1)} h`}
                  usual={usualRange(series.hours)} format={(v) => v.toFixed(Number.isInteger(v) ? 0 : 1)} />
              </figure>
              <figure className="fx-fig">
                <ChartHeader title="Efficiency" value={effOnly.length ? String(Math.round(effOnly[effOnly.length - 1])) : "—"} unit="%" usual={effOnly.length ? `${effOnly.length} nights with a reading` : "no readings"} />
                {effOnly.length >= 2
                  ? <TrendChart values={series.efficiencyPct} labels={series.labels} lastLabel={series.labels[series.labels.length - 1]} unit="%" domain={[0, 100]} ticks={[0, 50, 100]} height={124}
                      label={`Sleep efficiency, last ${series.days.length} nights: min ${Math.round(Math.min(...effOnly))}%, max ${Math.round(Math.max(...effOnly))}%, last ${Math.round(effOnly[effOnly.length - 1])}%`}
                      usual={usualRange(series.efficiencyPct)} />
                  : <p className="fx-muted">Efficiency isn&apos;t recorded for these nights.</p>}
              </figure>
            </div>
          )}
      </Card>
    </div>
  );
}
