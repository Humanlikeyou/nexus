import { useCallback, useEffect, useMemo, useState } from "react";
import { READINESS_COPY, matchDriver, readiness, tokyoDay, trendText, type DriverKey, type DriverPoint } from "../lib/mindBody";
import { NoopShapeError, isSnapshot, noopQuery, parseBodyContext, parseMetricSeries, snapshotDays } from "../lib/noop";
import type { NoopDayRow, NoopMetricPoint, NoopMetricSeries, NoopSnapshotPayload, RecordedSession } from "../types";
import { Badge, Button, SegmentedControl } from "../ui";
import "./MindBodyScreen.css";

export const INVALID_PATH_STEPS = "The NOOP binary or database path in Settings isn't valid. Open Settings › WHOOP and set full paths that start with /.";
export const SANDBOX_STEPS = "NOOP's local data access isn't set up yet. Open NOOP once so it can create its data folder, then check again.";
export const DISCLAIMER = "Personal estimates, not medical data.";
const DASH = "—";
const CHART_W = 300, CHART_H = 160, PAD = 24;

export type MindBodySeries = { hrv: NoopMetricSeries; recovery: NoopMetricSeries; sleep: NoopMetricSeries; resting_hr: NoopMetricSeries; strain: NoopMetricSeries };
export type DriverSeries = Record<DriverKey, NoopMetricPoint[]>;
export type MindBodyState = { status: "loading" } | { status: "inactive"; steps: string | null } | { status: "error" } | { status: "ready"; series: MindBodySeries; drivers: DriverSeries };

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
    return { status: "ready", series: seriesFromSnapshot(raw), drivers };
  } catch {
    console.warn("[console] noop_query returned an unexpected shape");
    return { status: "error" };
  }
}

const num = (v: number | null | undefined, d = 0) => (typeof v === "number" && Number.isFinite(v) ? v.toFixed(d) : DASH);
const avgLast = (pts: NoopMetricPoint[], n: number) => { const xs = [...pts].sort((a, b) => a.day.localeCompare(b.day)).slice(-n).map((p) => p.value).filter((v): v is number => typeof v === "number"); return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null; };
const last = (pts: NoopMetricPoint[]) => { const s = [...pts].sort((a, b) => a.day.localeCompare(b.day)); return s[s.length - 1]?.value ?? null; };

function scale(values: number[], lo: number, hi: number) {
  const min = Math.min(...values), max = Math.max(...values);
  const span = max - min || 1;
  return (v: number) => hi - ((v - min) / span) * (hi - lo);
}

function ChartBox({ state, label, children, onRetry }: { state: "loading" | "empty" | "error" | "ready"; label: string; children?: React.ReactNode; onRetry?: () => void }) {
  if (state === "loading") return <div className="mind-body__chart mind-body__skeleton" role="status" aria-label={`${label} loading`} />;
  if (state === "error") return <div className="mind-body__chart mind-body__chart-msg" role="alert"><p>Couldn't load this chart.</p>{onRetry && <Button variant="plain" size="small" onClick={onRetry}>Retry</Button>}</div>;
  if (state === "empty") return <div className="mind-body__chart mind-body__chart-msg"><p>No data yet</p><p className="mind-body__hint">Sync your device to see this chart.</p></div>;
  return <div className="mind-body__chart">{children}</div>;
}

function Gridlines() {
  return <g className="mind-body__grid" aria-hidden="true">{[0, 1, 2, 3].map((i) => <line key={i} x1={PAD} x2={CHART_W - 4} y1={8 + i * ((CHART_H - PAD - 8) / 3)} y2={8 + i * ((CHART_H - PAD - 8) / 3)} />)}</g>;
}

export function LineChart({ points, label, variant }: { points: NoopMetricPoint[]; label: string; variant: "hrv" | "load" }) {
  const vals = points.filter((p): p is { day: string; value: number } => typeof p.value === "number");
  if (vals.length < 2) return <ChartBox state="empty" label={label} />;
  const y = scale(vals.map((p) => p.value), 8, CHART_H - PAD);
  const x = (i: number) => PAD + (i / (vals.length - 1)) * (CHART_W - PAD - 4);
  const d = vals.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  return (
    <ChartBox state="ready" label={label}>
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`${label}, ${vals.length} days`} preserveAspectRatio="none">
        <Gridlines />
        <path d={`${d} L${x(vals.length - 1)},${CHART_H - PAD} L${PAD},${CHART_H - PAD} Z`} className="mind-body__area" />
        <path d={d} className="mind-body__line" data-series={variant} fill="none" />
      </svg>
    </ChartBox>
  );
}

export function StrainBars({ points }: { points: NoopMetricPoint[] }) {
  const vals = [...points].sort((a, b) => a.day.localeCompare(b.day)).slice(-28);
  const nums = vals.map((p) => p.value).filter((v): v is number => typeof v === "number");
  if (nums.length < 2) return <ChartBox state="empty" label="Strain" />;
  const max = Math.max(...nums, 1);
  const bw = (CHART_W - PAD - 4) / vals.length;
  // 7-day vs 28-day rolling average load (copper comparison line).
  const avg = (i: number, w: number) => { const xs = vals.slice(Math.max(0, i - w + 1), i + 1).map((p) => p.value).filter((v): v is number => typeof v === "number"); return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0; };
  const yOf = (v: number) => CHART_H - PAD - (v / max) * (CHART_H - PAD - 8);
  const line = (w: number) => vals.map((_, i) => `${i ? "L" : "M"}${(PAD + bw * i + bw / 2).toFixed(1)},${yOf(avg(i, w)).toFixed(1)}`).join(" ");
  return (
    <ChartBox state="ready" label="Strain">
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`Daily strain with 7 and 28 day load, ${nums.length} days`} preserveAspectRatio="none">
        <Gridlines />
        {vals.map((p, i) => typeof p.value === "number" && <rect key={p.day} className="mind-body__bar" x={PAD + bw * i + 1} width={Math.max(bw - 2, 1)} y={yOf(p.value)} height={CHART_H - PAD - yOf(p.value)} rx={2} />)}
        <path d={line(7)} className="mind-body__line" data-series="load" fill="none" />
        <path d={line(28)} className="mind-body__line" data-series="baseline" fill="none" />
      </svg>
    </ChartBox>
  );
}

export function ScatterChart({ points, label, xUnit }: { points: DriverPoint[]; label: string; xUnit: string }) {
  if (points.length === 0) return <ChartBox state="empty" label={label} />;
  const sx = scale(points.map((p) => p.x), CHART_W - 4, PAD); // reversed range → left-to-right
  const sy = scale(points.map((p) => p.y), 8, CHART_H - PAD);
  return (
    <ChartBox state="ready" label={label}>
      <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={`${label}: focus against ${xUnit}, n = ${points.length}`} preserveAspectRatio="none">
        <Gridlines />
        {points.map((p, i) => <circle key={i} className="mind-body__dot" cx={CHART_W - 4 + PAD - sx(p.x)} cy={sy(p.y)} r={3} />)}
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

export type MindBodyFieldProps = { sessions: RecordedSession[]; load?: () => Promise<MindBodyState> };

export function MindBodyField({ sessions, load = loadMindBody }: MindBodyFieldProps) {
  const [state, setState] = useState<MindBodyState>({ status: "loading" });
  const [driver, setDriver] = useState<DriverKey>("sleep");
  const refresh = useCallback(() => { setState({ status: "loading" }); void load().then(setState, () => setState({ status: "inactive", steps: null })); }, [load]);
  useEffect(() => { let live = true; void load().then((s) => { if (live) setState(s); }, () => { if (live) setState({ status: "inactive", steps: null }); }); return () => { live = false; }; }, [load]);

  const series = state.status === "ready" ? state.series : null;
  const ready = useMemo(() => series ? readiness({ hrv: series.hrv.points, recovery: series.recovery.points, sleep: series.sleep.points }) : null, [series]);
  const meta = DRIVERS.find((d) => d.key === driver)!;
  const drivers = state.status === "ready" ? state.drivers : null;
  const points = useMemo(() => drivers ? matchDriver(sessions, driver, drivers[driver]) : [], [drivers, sessions, driver]);
  const lastSession = useMemo(() => [...sessions].sort((a, b) => b.start.localeCompare(a.start))[0] ?? null, [sessions]);
  const ctx = parseBodyContext(lastSession?.body_context);
  const copy = READINESS_COPY[ready?.level ?? "none"];
  const loading = state.status === "loading";

  return (
    <div className="mind-body">
      <header className="mind-body__header">
        <h1>Mind–Body</h1>
        <p className="mind-body__subtitle">How your body state lines up with focus. {DISCLAIMER}</p>
      </header>
      {state.status === "inactive" ? <Inactive steps={state.steps} /> : state.status === "error" ? <ChartBox state="error" label="Mind–Body" onRetry={refresh} /> : (
        <div className="mind-body__grid-layout">
          <section className="mind-body__panel" aria-labelledby="mb-readiness">
            <h2 id="mb-readiness" className="mind-body__title">Readiness</h2>
            <p className="mind-body__meta">Compared with your own 28-day baseline</p>
            <div className="mind-body__readiness">
              <Badge tone={copy.tone} aria-label={`Readiness: ${copy.label}`}>{copy.label}</Badge>
              <span className="mind-body__score" aria-label={`Recovery ${num(last(series?.recovery.points ?? []))}%`}>{num(last(series?.recovery.points ?? []))}</span>
            </div>
            <p className="mind-body__help">{copy.help}</p>
            <h3 className="mind-body__sub">HRV, last 14 days <span className="mind-body__unit">ms</span></h3>
            {loading ? <ChartBox state="loading" label="HRV" /> : <LineChart points={(series?.hrv.points ?? []).slice(-14)} label="HRV, last 14 days" variant="hrv" />}
            <h3 className="mind-body__sub">Training load <span className="mind-body__unit">strain · 7-day vs 28-day</span></h3>
            {loading ? <ChartBox state="loading" label="Strain" /> : <StrainBars points={series?.strain.points ?? []} />}
            <dl className="mind-body__stats">
              <div><dt>Resting HR</dt><dd>{num(last(series?.resting_hr.points ?? []))} bpm</dd></div>
              <div><dt>HRV</dt><dd>{num(last(series?.hrv.points ?? []))} ms</dd></div>
              <div><dt>Strain, 7-day average</dt><dd>{num(avgLast(series?.strain.points ?? [], 7), 1)}</dd></div>
            </dl>
          </section>

          <section className="mind-body__panel" aria-labelledby="mb-drivers">
            <h2 id="mb-drivers" className="mind-body__title">Focus drivers</h2>
            <p className="mind-body__meta">Correlation, not causation · n = {points.length} matched sessions</p>
            <SegmentedControl label="Focus driver" size="small" value={driver} options={DRIVERS.map((d) => ({ value: d.key, label: d.label }))} onChange={(v) => setDriver(v as DriverKey)} />
            {loading ? <ChartBox state="loading" label="Focus drivers" /> : <ScatterChart points={points} label={`Focus vs ${meta.noun}`} xUnit={meta.unit} />}
            <p className="mind-body__trend" aria-live="polite">{trendText(points, meta.noun)}</p>
          </section>

          <section className="mind-body__panel" aria-labelledby="mb-context">
            <h2 id="mb-context" className="mind-body__title">Session body context</h2>
            <p className="mind-body__meta">{lastSession ? `Body state at the start of your ${tokyoDay(lastSession.start) ?? "last"} session` : "Body state at the start of each Muse session"}</p>
            {ctx ? (
              <dl className="mind-body__stats">
                <div><dt>Recovery</dt><dd>{num(ctx.recovery)}%</dd></div>
                <div><dt>HRV</dt><dd>{num(ctx.hrv_ms)} ms</dd></div>
                <div><dt>Resting HR</dt><dd>{num(ctx.resting_hr)} bpm</dd></div>
                <div><dt>Sleep</dt><dd>{typeof ctx.sleep_minutes === "number" ? `${Math.floor(ctx.sleep_minutes / 60)}h ${Math.round(ctx.sleep_minutes % 60)}m` : DASH}</dd></div>
              </dl>
            ) : <p className="mind-body__hint">{lastSession ? "No body context saved for this session. Sessions recorded before NOOP was set up don't have one." : "No sessions yet."}</p>}
          </section>
        </div>
      )}
      <p className="mind-body__disclaimer">{DISCLAIMER}</p>
      {state.status === "inactive" && <Button variant="secondary" size="small" onClick={refresh}>Check again</Button>}
    </div>
  );
}
