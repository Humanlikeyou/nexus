import { useCallback, useEffect, useMemo, useState } from "react";
import { MIN_TREND_SESSIONS, READINESS_COPY, matchDriver, pearson, readinessReading, shortDay, tokyoDay, trendText, type DriverKey, type DriverPoint } from "../lib/mindBody";
import { METRICS, driverSentence, drivers as rankDrivers, hoursMinutes, inRange, lean, loadRatio, mergeSeries, metricReading, windowAverage, type MetricKey } from "../lib/mindBodyDeep";
import { NoopShapeError, isSnapshot, noopQuery, parseBodyContext, parseMetricSeries, snapshotDays } from "../lib/noop";
import type { NoopDayRow, NoopLatestNight, NoopMetricPoint, NoopMetricSeries, NoopSnapshotPayload, RecordedSession } from "../types";
import { Badge, Button, PageHero, SegmentedControl } from "../ui";
import { BaselineChart, DeltaBar, StageStrip } from "./MindBodyCharts";
import "./MindBodyScreen.css";

export const INVALID_PATH_STEPS = "The NOOP binary or database path in Settings isn't valid. Open Settings › WHOOP and set full paths that start with /.";
export const SANDBOX_STEPS = "NOOP's local data access isn't set up yet. Open NOOP once so it can create its data folder, then check again.";
export const DISCLAIMER = "Personal estimates, not medical data.";
const DASH = "—";
/* Every plot fills its column edge to edge: no inner gutter, so plots line up across sections. */
const CHART_W = 300, CHART_H = 160, CHART_BASE = 154, CHART_TOP = 8;
/** Keeps a round dot wholly inside the plot at any stretch. */
const DOT_INSET = 6;

export type MindBodySeries = { hrv: NoopMetricSeries; recovery: NoopMetricSeries; sleep: NoopMetricSeries; resting_hr: NoopMetricSeries; strain: NoopMetricSeries };
export type DriverSeries = Record<DriverKey, NoopMetricPoint[]>;
export type MindBodyState = { status: "loading" } | { status: "inactive"; steps: string | null } | { status: "error" }
  | { status: "ready"; series: MindBodySeries; drivers: DriverSeries; long?: Partial<Record<MetricKey, NoopMetricPoint[]>>; days?: NoopDayRow[]; night?: NoopLatestNight | null };

/** Readiness series from health_snapshot rows (recentDays + today); today's sleep prefers latestNight. */
export function seriesFromSnapshot(snap: NoopSnapshotPayload): MindBodySeries {
  const days = snapshotDays(snap);
  const col = (key: MindBodySeries[keyof MindBodySeries]["key"], f: (d: NoopDayRow) => number | null): NoopMetricSeries => ({ key, points: days.map((d) => ({ day: d.day, value: f(d) })) });
  const night = snap.latestNight;
  return {
    hrv: col("hrv", (d) => d.avgHrv),
    recovery: col("recovery", (d) => d.recovery),
    sleep: col("sleep_total_min", (d) => (night && night.day === d.day ? night.totalSleepMin : d.totalSleepMin)),
    resting_hr: col("rhr", (d) => d.restingHr),
    strain: col("strain", (d) => d.strain),
  };
}

/**
 * health_snapshot (29 days: today + 28-day baseline) for readiness, metric_series (365 days) for
 * focus drivers. binary_missing / unregistered command → inactive setup; bad_output, process_failed,
 * timeout or an unexpected shape → retry state (never a crash).
 */
export async function loadMindBody(): Promise<MindBodyState> {
  const driverKeys = [["sleep", "sleep_total_min"], ["hrv", "hrv"], ["recovery", "recovery"]] as const;
  const [snap, ...series] = await Promise.all([
    noopQuery("health_snapshot", { days: 29 }),
    ...driverKeys.map(([, key]) => noopQuery("metric_series", { key, days: 365 })),
  ]);
  // Longer history for the two metrics readiness doesn't need: the page still works without it.
  const [rhrRead, strainRead] = await Promise.all((["rhr", "strain"] as const).map((key) => noopQuery("metric_series", { key, days: 365 })));
  const failed = [snap, ...series].find((r) => !r.ok);
  if (failed && !failed.ok) {
    const kind = failed.error.kind;
    // binary_missing detail is NOOP's own setup text; other kinds get fixed copy (details may hold paths).
    if (kind === "binary_missing") return { status: "inactive", steps: failed.error.detail };
    if (kind === "invalid_path") return { status: "inactive", steps: INVALID_PATH_STEPS };
    if (kind === "sandbox_missing") return { status: "inactive", steps: SANDBOX_STEPS };
    if (kind === "spawn_failed") return { status: "inactive", steps: null };
    return { status: "error" };
  }
  try {
    const raw = (snap as { ok: true; value: unknown }).value;
    if (!isSnapshot(raw)) throw new NoopShapeError("health_snapshot");
    const drivers = Object.fromEntries(driverKeys.map(([name, key], i) => [name, parseMetricSeries(key, (series[i] as { ok: true; value: unknown }).value).points])) as DriverSeries;
    const optional = (key: "rhr" | "strain", read: typeof rhrRead) => { try { return read.ok ? parseMetricSeries(key, read.value).points : []; } catch { return []; } };
    return { status: "ready", series: seriesFromSnapshot(raw), drivers, days: snapshotDays(raw), night: raw.latestNight ?? null,
      long: { sleep: drivers.sleep, hrv: drivers.hrv, recovery: drivers.recovery, resting_hr: optional("rhr", rhrRead), strain: optional("strain", strainRead) } };
  } catch {
    console.warn("[console] noop_query returned an unexpected shape");
    return { status: "error" };
  }
}

const num = (v: number | null | undefined, d = 0) => (typeof v === "number" && Number.isFinite(v) ? v.toFixed(d) : DASH);
const readingOf0 = (readings: Array<{ def: { key: string }; value: number | null }>, key: string) => readings.find((item) => item.def.key === key)?.value ?? null;

function scale(values: number[], lo: number, hi: number) {
  const min = Math.min(...values), max = Math.max(...values);
  const span = max - min || 1;
  return (v: number) => hi - ((v - min) / span) * (hi - lo);
}

function ChartBox({ state, label, children, onRetry }: { state: "loading" | "empty" | "error" | "ready"; label: string; children?: React.ReactNode; onRetry?: () => void }) {
  if (state === "loading") return <div className="mind-body__chart mind-body__skeleton" role="status" aria-label={`${label} loading`} />;
  if (state === "error") return <div className="mind-body__chart mind-body__chart-msg" role="alert"><p>Couldn't load this chart.</p>{onRetry && <Button variant="plain" size="small" onClick={onRetry}>Retry</Button>}</div>;
  if (state === "empty") return <div className="mind-body__chart mind-body__chart-msg"><p>No data yet</p><p className="mind-body__hint">Sync your device to see this chart.</p></div>;
  return <div className="mind-body__chart" data-plot="">{children}</div>;
}

function Gridlines() {
  return <g className="mind-body__grid" aria-hidden="true">{[0, 1, 2, 3].map((i) => { const y = CHART_TOP + i * ((CHART_BASE - CHART_TOP) / 3); return <line key={i} x1={0} x2={CHART_W} y1={y} y2={y} />; })}</g>;
}

export function LineChart({ points, label, variant }: { points: NoopMetricPoint[]; label: string; variant: "hrv" | "load" }) {
  const vals = points.filter((p): p is { day: string; value: number } => typeof p.value === "number");
  if (vals.length < 2) return <ChartBox state="empty" label={label} />;
  const y = scale(vals.map((p) => p.value), CHART_TOP, CHART_BASE);
  const x = (i: number) => (i / (vals.length - 1)) * CHART_W;
  const d = vals.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  return (
    <ChartBox state="ready" label={label}>
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`${label}, ${vals.length} days`} preserveAspectRatio="none">
        <Gridlines />
        <path d={`${d} L${CHART_W},${CHART_BASE} L0,${CHART_BASE} Z`} className="mind-body__area" />
        <path d={d} className="mind-body__line" data-series={variant} fill="none" />
      </svg>
    </ChartBox>
  );
}

const BAR_GAP = 2;

export function StrainBars({ points }: { points: NoopMetricPoint[] }) {
  const vals = [...points].sort((a, b) => a.day.localeCompare(b.day)).slice(-28);
  const nums = vals.map((p) => p.value).filter((v): v is number => typeof v === "number");
  if (nums.length < 2) return <ChartBox state="empty" label="Strain" />;
  const max = Math.max(...nums, 1);
  // The first bar starts on the left edge and the last ends on the right edge.
  const step = (CHART_W + BAR_GAP) / vals.length;
  const bw = step - BAR_GAP;
  // 7-day vs 28-day rolling average load (copper comparison line).
  const avg = (i: number, w: number) => { const xs = vals.slice(Math.max(0, i - w + 1), i + 1).map((p) => p.value).filter((v): v is number => typeof v === "number"); return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0; };
  const yOf = (v: number) => CHART_BASE - (v / max) * (CHART_BASE - CHART_TOP);
  const line = (w: number) => vals.map((_, i) => `${i ? "L" : "M"}${(step * i + bw / 2).toFixed(1)},${yOf(avg(i, w)).toFixed(1)}`).join(" ");
  return (
    <ChartBox state="ready" label="Strain">
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`Daily strain with 7 and 28 day load, ${nums.length} days`} preserveAspectRatio="none">
        <Gridlines />
        {vals.map((p, i) => typeof p.value === "number" && <rect key={p.day} className="mind-body__bar" x={step * i} width={bw} y={yOf(p.value)} height={CHART_BASE - yOf(p.value)} />)}
        <path d={line(7)} className="mind-body__line" data-series="load" fill="none" />
        <path d={line(28)} className="mind-body__line" data-series="baseline" fill="none" />
      </svg>
    </ChartBox>
  );
}

export function ScatterChart({ points, label, xUnit }: { points: DriverPoint[]; label: string; xUnit: string }) {
  if (points.length === 0) return <ChartBox state="empty" label={label} />;
  const sy = scale(points.map((p) => p.y), CHART_TOP + DOT_INSET, CHART_BASE - DOT_INSET);
  const lo = Math.min(...points.map((p) => p.x)), hi = Math.max(...points.map((p) => p.x));
  const sx = (v: number) => (hi === lo ? CHART_W / 2 : DOT_INSET + ((v - lo) / (hi - lo)) * (CHART_W - DOT_INSET * 2));
  return (
    <ChartBox state="ready" label={label}>
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`${label}: focus against ${xUnit}, n = ${points.length}`} preserveAspectRatio="none">
        <Gridlines />
        {/* A zero-length round line stays a true dot when the chart stretches. */}
        {points.map((p, i) => { const x = sx(p.x), y = sy(p.y); return <line key={i} className="mind-body__dot" x1={x} x2={x} y1={y} y2={y} />; })}
      </svg>
    </ChartBox>
  );
}

const DRIVERS: Array<{ key: DriverKey; label: string; noun: string; unit: string }> = [
  { key: "sleep", label: "Sleep", noun: "sleep", unit: "hours of sleep" },
  { key: "hrv", label: "HRV", noun: "HRV", unit: "HRV (ms)" },
  { key: "recovery", label: "Recovery", noun: "recovery", unit: "recovery (%)" },
];

export function Inactive({ steps }: { steps: string | null }) {
  return (
    <div className="subsystem-inactive" role="region" aria-label="NOOP setup">
      <h2>NOOP isn't connected</h2>
      <p>Mind-Body needs NOOP's local health data. To set it up:</p>
      {steps ? <p className="subsystem-inactive__detail">{steps}</p> : <ol>
        <li>Build the NOOP CLI: run <code>swift build -c release</code> in Packages/NoopLocalAccess.</li>
        <li>In Settings, set the NOOP binary path and database path.</li>
        <li>Set WHOOP source to "NOOP (recommended)", then come back here.</li>
      </ol>}
    </div>
  );
}

const RANGES = [{ value: "7", label: "Week" }, { value: "30", label: "Month" }, { value: "90", label: "Quarter" }, { value: "365", label: "Year" }];
const TREND_ROWS: MetricKey[] = ["recovery", "hrv", "resting_hr", "strain"];
const COMPARE_ROWS: MetricKey[] = ["sleep", "recovery", "hrv", "resting_hr", "strain"];

export type MindBodyFieldProps = { sessions: RecordedSession[]; load?: () => Promise<MindBodyState> };

export function MindBodyField({ sessions, load = loadMindBody }: MindBodyFieldProps) {
  const [state, setState] = useState<MindBodyState>({ status: "loading" });
  const [driver, setDriver] = useState<DriverKey>("sleep");
  const refresh = useCallback(() => { setState({ status: "loading" }); void load().then(setState, () => setState({ status: "inactive", steps: null })); }, [load]);
  useEffect(() => { let live = true; void load().then((s) => { if (live) setState(s); }, () => { if (live) setState({ status: "inactive", steps: null }); }); return () => { live = false; }; }, [load]);

  const [range, setRange] = useState("30");
  const series = state.status === "ready" ? state.series : null;
  // Today's row is empty until the night syncs: read the latest day that has values, and say which.
  const reading = useMemo(() => series ? readinessReading({ hrv: series.hrv.points, recovery: series.recovery.points, sleep: series.sleep.points }) : null, [series]);
  const asOf = reading?.asOf ?? null;
  const ready = reading?.ready ?? null;
  const recovery = reading?.recovery ?? null;
  const earlier = reading?.earlier ?? false;
  const meta = DRIVERS.find((d) => d.key === driver)!;
  const driverSeries = state.status === "ready" ? state.drivers : null;
  const points = useMemo(() => driverSeries ? matchDriver(sessions, driver, driverSeries[driver]) : [], [driverSeries, sessions, driver]);
  const lastSession = useMemo(() => [...sessions].sort((a, b) => b.start.localeCompare(a.start))[0] ?? null, [sessions]);
  const ctx = parseBodyContext(lastSession?.body_context);
  const copy = READINESS_COPY[ready?.level ?? "none"];
  const loading = state.status === "loading";

  const ready_ = state.status === "ready" ? state : null;
  const merged = useMemo(() => {
    if (!series) return null;
    const long = ready_?.long ?? {};
    const of = (key: MetricKey, recent: NoopMetricPoint[]) => mergeSeries(long[key] ?? [], recent);
    return { sleep: of("sleep", series.sleep.points), recovery: of("recovery", series.recovery.points), hrv: of("hrv", series.hrv.points), resting_hr: of("resting_hr", series.resting_hr.points), strain: of("strain", series.strain.points) };
  }, [series, ready_]);
  const readings = useMemo(() => merged ? COMPARE_ROWS.map((key) => metricReading(METRICS[key], merged[key], asOf)) : [], [merged, asOf]);
  const readingOf = (key: MetricKey) => readings.find((r) => r.def.key === key)!;
  const day = (ready_?.days ?? []).find((row) => row.day === asOf) ?? null;
  const night = ready_?.night && ready_.night.day === asOf ? ready_.night : null;
  const stale = series !== null && asOf === null;
  const lastDay = merged ? [...merged.recovery, ...merged.hrv, ...merged.sleep].filter((p) => typeof p.value === "number").map((p) => p.day).sort().pop() ?? null : null;
  const days = Number(range);
  const loadRun = merged ? loadRatio(merged.strain, asOf) : null;
  const longest = merged ? Math.max(...COMPARE_ROWS.map((key) => merged[key].filter((p) => typeof p.value === "number").length)) : 0;
  const ranges = RANGES.filter((item) => Number(item.value) <= 30 || longest >= Number(item.value) / 3);
  const r = pearson(points);
  const sleepMin = night?.totalSleepMin ?? readingOf0(readings, "sleep");
  const stages = [
    { label: "Deep", minutes: night?.deepMin ?? day?.deepMin ?? 0, tone: "deep" },
    { label: "REM", minutes: night?.remMin ?? day?.remMin ?? 0, tone: "rem" },
    { label: "Light", minutes: night?.lightMin ?? day?.lightMin ?? 0, tone: "light" },
  ];
  const efficiency = night?.efficiency ?? day?.efficiency ?? null;
  const also = day ? [
    ["Blood oxygen", day.spo2Pct, (v: number) => `${v.toFixed(1)}%`], ["Skin temp", day.skinTempDevC, (v: number) => `${v >= 0 ? "+" : ""}${v.toFixed(1)} °C`],
    ["Breathing", day.respRateBpm, (v: number) => `${v.toFixed(1)}/min`], ["Steps", day.steps, (v: number) => Math.round(v).toLocaleString()],
    ["Workouts", day.exerciseCount, (v: number) => String(v)], ["Disturbances", day.disturbances, (v: number) => String(v)],
  ] as const : [];
  const known = also.filter(([, value]) => typeof value === "number");

  const statement = state.status === "loading" ? "Reading your data"
    : ready && ready.level !== "none" ? <span className="mind-body__hero"><span className="mind-body__score" aria-label={`Recovery ${num(recovery)}%`}>{num(recovery)}</span><Badge tone={copy.tone} aria-label={`Readiness: ${copy.label}`}>{copy.label}</Badge></span>
      : stale ? "No recent reading" : "How your body lines up with focus";

  return (
    <div className="mind-body page-inset">
      <PageHero title="Mind–Body" statement={statement} detail={ready && ready.level !== "none" ? copy.help : stale && lastDay ? `The last reading was ${shortDay(lastDay)}. Open NOOP to sync.` : undefined} />
      {state.status === "inactive" ? <Inactive steps={state.steps} /> : state.status === "error" ? <ChartBox state="error" label="Mind–Body" onRetry={refresh} /> : (
        <>
          {!loading && <p className="mind-body__meta mb-reading">{asOf ? `Reading for ${shortDay(asOf)}${earlier ? ", the latest with data" : ""}` : "No reading day yet"} · Compared with your own 28-day baseline</p>}

          <section className="mb-section" aria-labelledby="mb-readiness">
            <h2 id="mb-readiness" className="mb-section__title">Readiness</h2>
            {loading ? <ChartBox state="loading" label="Readiness" /> : (
              <>
                <p className="mb-section__lede">{driverSentence(readings) ?? "Today's score needs a week of baseline first."}</p>
                <ul className="mb-drivers" aria-label="What's driving today">
                  {[...rankDrivers(readings), ...readings.filter((item) => !rankDrivers(readings).includes(item))].map((item) => (
                    <li key={item.def.key} data-lean={lean(item)}>
                      <span className="mb-drivers__label">{item.def.label}</span>
                      <span className="mb-drivers__value">{item.value === null ? DASH : item.def.format(item.value)}</span>
                      <DeltaBar reading={item} />
                      <span className="mb-drivers__usual">{item.base ? `usual ${item.def.format(item.base.mean)}` : "no baseline yet"}</span>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>

          {merged && (
            <>
              <div className="mb-range"><SegmentedControl label="Range" size="small" value={range} options={ranges} onChange={setRange} /></div>

              <section className="mb-section" aria-labelledby="mb-sleep">
                <h2 id="mb-sleep" className="mb-section__title">Sleep</h2>
                <div className="mb-row">
                  <div className="mb-row__head">
                    <h3>Last night</h3>
                    <p className="mb-row__value">{typeof sleepMin === "number" ? hoursMinutes(sleepMin) : DASH}</p>
                    <p className="mb-row__usual">{typeof efficiency === "number" ? `${Math.round(efficiency * 100)}% efficient` : "\u00a0"}</p>
                  </div>
                  <div className="mb-plot">
                    <StageStrip stages={stages} />
                    {stages.some((stage) => stage.minutes > 0) && <p className="mind-body__meta">{stages.map((stage) => `${stage.label} ${hoursMinutes(stage.minutes)}`).join(" · ")}</p>}
                    <BaselineChart points={inRange(merged.sleep, asOf, days)} reading={readingOf("sleep")} tall />
                  </div>
                </div>
              </section>

              <section className="mb-section" aria-labelledby="mb-trends">
                <h2 id="mb-trends" className="mb-section__title">Trends against your baseline</h2>
                <div className="mb-rows">
                  {TREND_ROWS.map((key) => {
                    const item = readingOf(key);
                    return (
                      <div className="mb-row" key={key}>
                        <div className="mb-row__head">
                          <h3>{item.def.label}</h3>
                          <p className="mb-row__value">{item.value === null ? DASH : item.def.format(item.value)}</p>
                          <p className="mb-row__usual">{item.base ? `usual ${item.def.format(item.base.mean)}` : "no baseline yet"}</p>
                        </div>
                        <div className="mb-plot"><BaselineChart points={inRange(merged[key], asOf, days)} reading={item} /></div>
                      </div>
                    );
                  })}
                </div>
              </section>

              <section className="mb-section" aria-labelledby="mb-load">
                <h2 id="mb-load" className="mb-section__title">Training load</h2>
                {loadRun && <p className="mb-section__lede">{loadRun.line}</p>}
                <div className="mb-row">
                  <div className="mb-row__head">
                    <h3>Week ÷ month</h3>
                    <p className="mb-row__value">{loadRun ? loadRun.ratio.toFixed(2) : DASH}</p>
                    <p className="mb-row__usual">28 days of strain</p>
                  </div>
                  <div className="mb-plot">
                    <StrainBars points={series?.strain.points ?? []} />
                    <p className="mind-body__meta">Daily strain, with the 7-day line and the dashed 28-day line.</p>
                  </div>
                </div>
              </section>

              <section className="mb-section" aria-labelledby="mb-compare">
                <h2 id="mb-compare" className="mb-section__title">Week and month</h2>
                <table className="mb-table">
                  <thead><tr><th scope="col">Average</th><th scope="col">Week</th><th scope="col">Month</th><th scope="col">Quarter</th></tr></thead>
                  <tbody>
                    {COMPARE_ROWS.map((key) => {
                      const cell = (n: number) => { const value = windowAverage(merged[key], asOf, n); return value === null ? DASH : METRICS[key].format(value); };
                      return <tr key={key}><th scope="row">{METRICS[key].label}</th><td>{cell(7)}</td><td>{cell(30)}</td><td>{cell(90)}</td></tr>;
                    })}
                  </tbody>
                </table>
              </section>

              {known.length > 0 && (
                <section className="mb-section" aria-labelledby="mb-also">
                  <h2 id="mb-also" className="mb-section__title">Also that day</h2>
                  <dl className="mb-also">{known.map(([label, value, format]) => <div key={label}><dt>{label}</dt><dd>{format(value as number)}</dd></div>)}</dl>
                </section>
              )}
            </>
          )}

          <section className="mb-section" aria-labelledby="mb-drivers">
            <h2 id="mb-drivers" className="mb-section__title">Focus drivers</h2>
            <p className="mb-section__lede">How your neuro sessions line up with your body.</p>
            <div className="mb-row">
              <div className="mb-row__head">
                <h3>Correlation</h3>
                <p className="mb-row__value">{r !== null ? `r = ${r.toFixed(2)}` : DASH}</p>
                <p className="mb-row__usual">n = {points.length} matched sessions</p>
              </div>
              <div className="mb-plot">
                <SegmentedControl label="Focus driver" size="small" value={driver} options={DRIVERS.map((d) => ({ value: d.key, label: d.label }))} onChange={(v) => setDriver(v as DriverKey)} />
                {loading ? <ChartBox state="loading" label="Focus drivers" /> : <ScatterChart points={points} label={`Focus vs ${meta.noun}`} xUnit={meta.unit} />}
                <p className="mind-body__trend" aria-live="polite">{trendText(points, meta.noun)}</p>
                {points.length < MIN_TREND_SESSIONS && <p className="mind-body__meta">Correlation, not causation.</p>}
              </div>
            </div>
          </section>

          <section className="mb-section" aria-labelledby="mb-context">
            <h2 id="mb-context" className="mb-section__title">Session body context</h2>
            <p className="mind-body__meta">{lastSession ? `Body state at the start of your ${tokyoDay(lastSession.start) ?? "last"} session` : "Body state at the start of each session"}</p>
            {ctx ? (
              <dl className="mb-also">
                <div><dt>Recovery</dt><dd>{num(ctx.recovery)}%</dd></div>
                <div><dt>HRV</dt><dd>{num(ctx.hrv_ms)} ms</dd></div>
                <div><dt>Resting HR</dt><dd>{num(ctx.resting_hr)} bpm</dd></div>
                <div><dt>Sleep</dt><dd>{typeof ctx.sleep_minutes === "number" ? hoursMinutes(ctx.sleep_minutes) : DASH}</dd></div>
              </dl>
            ) : <p className="mind-body__hint">{lastSession ? "No body context saved for this session. Sessions recorded before NOOP was set up don't have one." : "No sessions yet."}</p>}
          </section>
        </>
      )}
      <p className="mind-body__disclaimer">{DISCLAIMER}</p>
      {state.status === "inactive" && <Button variant="secondary" size="small" onClick={refresh}>Check again</Button>}
    </div>
  );
}
