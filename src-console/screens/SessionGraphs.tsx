import { useMemo, useState } from "react";
import { FlaskConical } from "lucide-react";
import { SegmentedControl } from "../ui";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, usualRange } from "../ui/charts/chartMath";
import { BANDS, SENSORS, SETTLE_SECS, clockAt, hasSamples, sessionStats, type BandKey, type SensorKey, type SessionSamples, type SessionWithSamples } from "../neuro/sessionSamples";

type View = "bands" | "scores" | "sensors";
const VIEWS = [{ value: "bands", label: "Bands" }, { value: "scores", label: "Focus & calm" }, { value: "sensors", label: "Sensors" }] as const;
const whole = (v: number | null | undefined) => (v == null || !Number.isFinite(v) ? "—" : String(Math.round(v)));

/** C20: the loud "Sample data" tag that goes on every card and row that shows generated numbers. */
export function SampleBadge({ children = "Sample data" }: { children?: string }) {
  return <span className="ns-badge" role="note"><FlaskConical size={12} aria-hidden="true" />{children}</span>;
}

/** C16: a session's summary from its samples, then Bands, Focus & calm and Sensors views. */
export function SessionGraphs({ session, others }: { session: SessionWithSamples; others: SessionWithSamples[] }) {
  const [view, setView] = useState<View>("bands");
  const [hover, setHover] = useState<number | null>(null);
  const [band, setBand] = useState<BandKey>("alpha");
  const peers = useMemo(() => others.filter((s) => s.id !== session.id && hasSamples(s)).map((s) => sessionStats(s.samples!)), [others, session.id]);
  if (!hasSamples(session)) {
    return (
      <div className="ns-empty" role="note">
        <p className="ns-empty__title">Graphs over time aren&apos;t read back yet</p>
        <p>The raw EEG and the focus and calm timeline are saved with this session; Nexus can&apos;t read them back into graphs yet.</p>
      </div>
    );
  }
  const s: SessionSamples = session.samples;
  const stats = sessionStats(s);
  const n = s.focus.length;
  const labels = Array.from({ length: n }, (_, i) => clockAt(i, s.stepSecs));
  const end = clockAt(n - 1, s.stepSecs);
  const settle = Math.round(SETTLE_SECS / s.stepSecs);
  const usualOf = (pick: (p: ReturnType<typeof sessionStats>) => number) => {
    const xs = peers.map(pick).filter(Number.isFinite);
    if (!xs.length) return null;
    const m = xs.reduce((a, b) => a + b, 0) / xs.length;
    return usualRange(xs) ?? { low: m, high: m, median: m };
  };
  const focusUsual = usualOf((p) => p.focus), calmUsual = usualOf((p) => p.calm);
  const tag = session.sample ? <SampleBadge /> : null;

  return (
    <div className="ns-graphs">
      <dl className="ns-summary">
        <div><dt>Focus</dt><dd>{whole(stats.focus)}<small>/ 100</small></dd></div>
        <div><dt>Calm</dt><dd>{whole(stats.calm)}<small>/ 100</small></dd></div>
        <div><dt>Strongest band</dt><dd>{stats.strongest.name}<small>{whole(stats.bandAvg[stats.strongest.key])}%</small></dd></div>
        <div><dt>Sensor contact</dt><dd>{stats.contact == null ? "—" : stats.contact >= 90 ? "Good" : stats.contact >= 75 ? "Fair" : "Poor"}<small>{stats.contact == null ? "" : `lowest ${stats.contact}%`}</small></dd></div>
      </dl>
      <div className="ns-graphs__bar">
        <SegmentedControl label="Session view" size="small" value={view} options={VIEWS} onChange={(v) => setView(v as View)} />
        {tag}
      </div>

      {view === "bands" && (
        <section className="ns-bands" aria-label="Bands over the session">
          <p className="ns-readout" aria-hidden="true">
            {hover == null ? <span>Hover the rows to read every band at one moment</span> : (
              <><b>{labels[hover]}</b>{BANDS.map((b) => <span key={b.key}>{b.name} <em>{whole(s.bands[b.key][hover])}%</em></span>)}<span>Focus <em>{whole(s.focus[hover])}</em></span><span>Calm <em>{whole(s.calm[hover])}</em></span></>
            )}
          </p>
          {BANDS.map((b, k) => {
            const usual = usualOf((p) => p.bandAvg[b.key]);
            const delta = deltaWords(stats.bandAvg[b.key], usual?.median ?? null, "%");
            return (
              <div className="ns-band" key={b.key}>
                <div className="ns-band__head"><b>{b.name}</b><small>{b.range}</small><span>{whole(stats.bandAvg[b.key])}% avg</span>{delta && <span className="ns-band__delta">{delta.text}</span>}</div>
                <TrendChart values={s.bands[b.key]} labels={labels} lastLabel={end} label={`${b.name} band power over the session, average ${whole(stats.bandAvg[b.key])}%`}
                  unit={k === 0 ? "%" : undefined} rawDots={false} height={k === BANDS.length - 1 ? 64 : 48} hideX={k !== BANDS.length - 1} hover={hover} onHover={setHover} usual={usual} />
              </div>
            );
          })}
        </section>
      )}

      {view === "scores" && (
        <section className="ns-scores" aria-label="Focus and calm over the session">
          {([["Focus", s.focus, focusUsual, stats.focus], ["Calm", s.calm, calmUsual, stats.calm]] as const).map(([name, values, usual, avg]) => (
            <div key={name} className="ns-score">
              <ChartHeader title={name} value={whole(avg)} unit="/ 100" delta={deltaWords(avg, usual?.median ?? null, " pts")} usual={usual ? `usual ${whole(usual.median)}` : "no usual yet"} />
              <TrendChart values={values} labels={labels} lastLabel={end} label={`${name} over the session, average ${whole(avg)} of 100`} domain={[0, 100]} rawDots={false} height={120}
                usual={usual} shadeTo={settle} shadeLabel="Settling in" readout={(i) => <><strong>{whole(values[i])} / 100</strong><span>{labels[i]} into the session</span></>} />
            </div>
          ))}
        </section>
      )}

      {view === "sensors" && (
        <section className="ns-sensors" aria-label="Sensors">
          <SegmentedControl label="Band" size="small" value={band} options={BANDS.map((b) => ({ value: b.key, label: b.name }))} onChange={(v) => setBand(v as BandKey)} />
          <div className="ns-sensors__body">
            <svg className="ns-head" viewBox="0 0 100 90" role="img" aria-label={`Muse sensors: ${SENSORS.map((x) => `${x.key} ${x.name} contact ${s.contact?.[x.key] ?? "unknown"}%`).join(", ")}`}>
              <ellipse cx="50" cy="48" rx="36" ry="40" className="ns-head__outline" />
              <path d="M44 8 L50 1 L56 8" className="ns-head__outline" />
              {SENSORS.map((x) => <g key={x.key}><circle cx={x.x} cy={x.y} r="6" className="ns-head__dot" data-contact={(s.contact?.[x.key] ?? 0) >= 90 ? "good" : "fair"} /><text x={x.x} y={x.y + 15} textAnchor="middle" className="ns-head__label">{x.key}</text></g>)}
            </svg>
            <div className="ns-sensors__grid">
              {SENSORS.map((x) => {
                const values = s.sensors?.[x.key as SensorKey]?.[band];
                return (
                  <div key={x.key} className="ns-sensor">
                    <div className="ns-band__head"><b>{x.key}</b><small>{x.name}</small><span>contact {s.contact?.[x.key] ?? "—"}%</span></div>
                    {values ? <TrendChart values={values} labels={labels} lastLabel={end} label={`${x.key} ${band} power over the session`} domain={[0, 50]} unit="%" rawDots={false} smoothing={1} height={72} />
                      : <p className="trend-chart__empty">Not split by sensor in this recording</p>}
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
