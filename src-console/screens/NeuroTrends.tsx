import { useMemo } from "react";
import { DataTag, wiredTag } from "../ui";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, usualRange } from "../ui/charts/chartMath";
import type { RecordingListItem, RecordingsAdapter } from "../neuro/recordings";
import type { SessionWithSamples } from "../neuro/sessionSamples";
import { SessionGraphs } from "./SessionGraphs";
import "./MindBodyScreen.css";
import "./SessionsScreen.css";
import "./NeuroSections.css";

const day = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "Asia/Tokyo" });
const whole = (v: number | null | undefined) => (v == null || !Number.isFinite(v) ? "—" : String(Math.round(v)));
const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
const SHOWN = 30;

type Row = { key: "focus" | "calm" | "target"; title: string; unit: string; pick: (r: RecordingListItem) => number | null };
const ROWS: Row[] = [
  { key: "focus", title: "Focus", unit: "/ 100", pick: (r) => r.avgFocus },
  { key: "calm", title: "Calm", unit: "/ 100", pick: (r) => r.avgCalm },
  { key: "target", title: "Time in target", unit: "%", pick: (r) => r.timeInTarget },
];

/** The session trends: the same header + chart rows as Mind–Body, from the recordings adapter (live sessions, or tagged sample). */
export function NeuroTrends({ recordings }: { recordings: RecordingsAdapter }) {
  const list = recordings.list();
  const items = useMemo(() => (list.state === "unwired" ? [] : [...list.value].filter((r) => r.avgFocus != null || r.avgCalm != null).sort((a, b) => a.startedAt.localeCompare(b.startedAt)).slice(-SHOWN)), [list]);
  const labels = items.map((r) => day.format(new Date(r.startedAt)));
  const latest = [...items].reverse().find((r) => r.raw === "available") ?? null;
  const series = latest ? recordings.series(latest.id) : null;
  const focusVals = items.map((r) => r.avgFocus).filter((v): v is number => v != null);

  const graphSession: SessionWithSamples | null = latest && series && series.state !== "unwired"
    ? { id: latest.id, device_type: "muse", start: latest.startedAt, end: latest.startedAt, duration_secs: latest.durationSecs, data_points: 0, summary: {}, samples: series.value, sample: series.state === "sample" }
    : null;

  return (
    <>
      <section className="mb-section" aria-labelledby="nt-trends">
        <div className="mb-section__head"><h2 id="nt-trends" className="mb-section__title">Focus and calm over time</h2>{wiredTag(list)}</div>
        {focusVals.length >= 2 ? (
          <>
            <p className="mb-section__lede">Focus averaged {whole(mean(focusVals))} over your last {items.length} sessions.</p>
            <p className="chart-legend mb-legend-line" aria-hidden="true"><span><i />You · each session</span><span><i data-key="usual" />Your usual</span></p>
            <div className="mb-rows">
              {ROWS.map((row) => {
                const values = items.map(row.pick);
                const real = values.filter((v): v is number => v != null);
                if (real.length < 2) return null;
                const usual = usualRange(values);
                const last = real[real.length - 1];
                return (
                  <div className="mb-row mb-row--stack" key={row.key}>
                    <div className="mb-row__head"><ChartHeader title={row.title} value={whole(last)} unit={row.unit} delta={deltaWords(last, usual?.median ?? null, row.key === "target" ? "%" : " pts")} usual={usual ? `usual ${whole(usual.median)}` : "no usual yet"} /></div>
                    <div className="mb-plot"><TrendChart values={values} labels={labels} lastLabel={labels[labels.length - 1]} label={`${row.title} across your last ${items.length} sessions, latest ${whole(last)}`} domain={[0, 100]} height={120} usual={usual} /></div>
                  </div>
                );
              })}
            </div>
            <p className="mind-body__meta">Each point is one session&apos;s average. Not a medical measurement.</p>
          </>
        ) : list.state === "unwired" ? (
          <p className="mind-body__hint">Session trends need wiring before they can show here.</p>
        ) : (
          <p className="mind-body__hint">Trends appear after two sessions with a focus or calm score.</p>
        )}
      </section>

      <section className="mb-section" aria-labelledby="nt-latest">
        <div className="mb-section__head"><h2 id="nt-latest" className="mb-section__title">Band powers · latest session</h2></div>
        {graphSession
          ? <SessionGraphs session={graphSession} others={[]} />
          : latest && series?.state === "unwired"
            ? <p className="mind-body__hint"><DataTag kind="wire" /> Band power over time is read from the saved recording, and isn&apos;t available for this one yet.</p>
            : <p className="mind-body__hint">Band powers appear here after your first saved recording.</p>}
      </section>
    </>
  );
}
