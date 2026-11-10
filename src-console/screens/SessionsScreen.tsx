import { useMemo, useState, type KeyboardEvent } from "react";
import { BrainCircuit, HeartPulse, RefreshCw } from "lucide-react";
import { Badge, Button, Card, InsetGroup, ListRow, More, PageHero, StatTile } from "../ui";
import { dateTime, number, time } from "../lib/format";
import type { RecordedSession, SystemSnapshot } from "../types";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, usualRange } from "../ui/charts/chartMath";
import { sampleMode, sampleSessions, score, type SessionWithSamples } from "../neuro/sessionSamples";
import { SampleBadge, SessionGraphs } from "./SessionGraphs";
import "./SessionsScreen.css";

/** Sessions → Mind–Body › Neuro history (screen-specs §5): source list + detail, like Voice Memos. */
const SESSIONS_COMMAND = "list_sessions";
const title = (s: RecordedSession) => (s.device_type === "muse" ? "Muse session" : `${s.device_type} session`);
const minutes = (s: RecordedSession) => Math.max(1, Math.round(s.duration_secs / 60));
const pct = (v: number | null | undefined) => (v == null ? null : Math.round(v));

const FIRST_GLANCE = 4;

export function SessionsField({ data, onRetry, onStartSession, samples = sampleMode() }: { data: SystemSnapshot; onRetry?: () => void; onStartSession?: () => void; samples?: boolean }) {
  const sessions = data.sessions as SessionWithSamples[];
  const [selectedId, setSelectedId] = useState<string | null>(sessions[0]?.id ?? null);
  const selected = sessions.find((s) => s.id === selectedId) ?? sessions[0] ?? null;
  const [showAll, setShowAll] = useState(false);
  const failed = data.unavailableSources.some((s) => s.command === SESSIONS_COMMAND);

  if (failed && sessions.length === 0) {
    return (
      <div className="sessions page-inset">
        <PageHero title="Neuro history" statement="Sensor sessions unavailable" />
        <Card>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
        </Card>
      </div>
    );
  }
  if (sessions.length === 0) {
    return (
      <div className="sessions page-inset">
        <PageHero title="Neuro history" statement="No sessions yet" />
        <Card>
          <p className="sessions__copy">Finish a Muse session and Amara OS keeps the recording and summary here.</p>
          {onStartSession && <div><Button variant="primary" onClick={onStartSession}>Start session</Button></div>}
        </Card>
        {samples && <SampleHistory />}
      </div>
    );
  }

  const museCount = sessions.filter((s) => s.device_type === "muse").length;
  const totalMin = Math.round(sessions.reduce((sum, s) => sum + s.duration_secs / 60, 0));
  const onListKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const index = Math.max(0, sessions.findIndex((s) => s.id === selected?.id));
    const next = sessions[Math.min(sessions.length - 1, Math.max(0, index + (event.key === "ArrowDown" ? 1 : -1)))];
    if (!next) return;
    event.preventDefault();
    setSelectedId(next.id);
  };

  return (
    <div className="sessions page-inset">
        <PageHero title="Neuro history" statement={`${sessions.length} session${sessions.length === 1 ? "" : "s"}, ${totalMin} min`} detail={museCount === sessions.length ? undefined : `${museCount} with Muse`} />
      <div className="sessions__split">
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- ↑/↓ moves the selection between the row buttons inside */}
        <div className="sessions__list" onKeyDown={onListKey}>
          <InsetGroup title="History" footer="Most recent first">
            {sessions.slice(0, showAll ? undefined : FIRST_GLANCE).map((s) => (
              <ListRow key={s.id} icon={s.device_type === "muse" ? BrainCircuit : HeartPulse} title={title(s)} subtitle={rowLine(s)} value={`${minutes(s)} min`} onSelect={() => setSelectedId(s.id)} selected={s.id === selected?.id} />
            ))}
          </InsetGroup>
          {!showAll && sessions.length > FIRST_GLANCE && <Button variant="plain" size="small" onClick={() => setShowAll(true)}>Show {sessions.length - FIRST_GLANCE} more</Button>}
        </div>
        {selected ? (
          <Card className="sessions__detail" title={title(selected)} headingLevel={2} accessory={<Badge tone="success">Saved locally</Badge>}>
            <div className="sessions__metrics">
              <StatTile label="Duration" value={minutes(selected)} unit="min" />
              <StatTile label="Data points" value={selected.data_points.toLocaleString()} />
              <StatTile label="Avg focus" value={selected.summary.avg_focus == null ? null : number(selected.summary.avg_focus)} />
              <StatTile label="Avg calm" value={selected.summary.avg_relaxation == null ? null : number(selected.summary.avg_relaxation)} />
            </div>
            <More label="Scores and session window">
            <div className="sessions__metrics sessions__metrics--beta">
              <StatTile label={<>Calm <Badge>Beta</Badge></>} value={pct(selected.summary.calm_pct)} unit={selected.summary.calm_pct == null ? undefined : "%"} />
              <StatTile label={<>Time in target <Badge>Beta</Badge></>} value={pct(selected.summary.time_in_target_pct)} unit={selected.summary.time_in_target_pct == null ? undefined : "%"} />
            </div>
            <InsetGroup>
              <ListRow title="Session window" value={<><time dateTime={selected.start}>{time(selected.start)}</time> → <time dateTime={selected.end}>{time(selected.end)}</time></>} />
            </InsetGroup>
            <p className="sessions__callout">Scores summarize the recorded signal for reflection. They aren&apos;t clinical measurements.</p>
            </More>
            <SessionGraphs session={selected} others={sessions} />
          </Card>
        ) : (
          <Card className="sessions__detail"><p className="sessions__copy">Select a session</p></Card>
        )}
      </div>
      <AcrossSessions sessions={sessions} />
      {samples && <SampleHistory />}
    </div>
  );
}

/** C15: "Thu, Oct 22, 9:10 PM · focus 61 · calm 48" under each session's title. */
function rowLine(s: SessionWithSamples) {
  const f = score(s.summary.avg_focus), c = score(s.summary.avg_relaxation);
  return [dateTime(s.start), f == null ? null : `focus ${Math.round(f)}`, c == null ? null : `calm ${Math.round(c)}`].filter(Boolean).join(" · ");
}

/** C15: focus and calm per session against his usual across sessions. Needs two sessions with scores. */
function AcrossSessions({ sessions, sample = false }: { sessions: SessionWithSamples[]; sample?: boolean }) {
  const ordered = [...sessions].sort((a, b) => a.start.localeCompare(b.start));
  const rows = (["Focus", "Calm"] as const).map((name) => ({ name, values: ordered.map((s) => score(name === "Focus" ? s.summary.avg_focus : s.summary.avg_relaxation)) }));
  if (rows.every((r) => r.values.filter((v) => v != null).length < 2)) return null;
  const labels = ordered.map((s) => new Date(s.start).toLocaleDateString("en-US", { month: "short", day: "numeric" }));
  return (
    <Card title="Across sessions" headingLevel={2} accessory={sample ? <SampleBadge /> : undefined}>
      <div className="ns-across">
        {rows.map(({ name, values }) => {
          const nums = values.filter((v): v is number => v != null);
          const prior = nums.slice(0, -1), m = prior.length ? prior.reduce((a, b) => a + b, 0) / prior.length : null;
          const usual = usualRange(prior) ?? (m == null ? null : { low: m, high: m, median: m });
          const last = nums[nums.length - 1];
          return (
            <div key={name}>
              <ChartHeader title={`Average ${name.toLowerCase()} · per session`} value={Math.round(last)} unit="/ 100" delta={deltaWords(last, m, " pts")} usual={m == null ? undefined : `usual ${Math.round(m)}`} />
              <TrendChart values={values} labels={labels} lastLabel={labels[labels.length - 1]} label={`${name} per session${sample ? " (sample data)" : ""}`} domain={[0, 100]} smoothing={1} height={108} usual={usual} />
            </div>
          );
        })}
      </div>
    </Card>
  );
}

/**
 * C20: generated sessions to preview the graphs, only in dev and the e2e/capture mock. They live in
 * their own section with a banner, a badge on every card and a "Sample" tag on every row, and are
 * never merged into the real list above.
 */
function SampleHistory() {
  const list = useMemo(() => sampleSessions(), []);
  const [id, setId] = useState(list[0].id);
  const picked = list.find((s) => s.id === id) ?? list[0];
  return (
    <section className="ns-sample" aria-label="Sample sessions">
      <div className="ns-banner" role="note"><SampleBadge /><p><b>Sample data.</b> These sessions are generated to preview Neuro history. None of it is a real reading; real sessions show graphs once the Muse recording keeps per-minute samples.</p></div>
      <AcrossSessions sessions={list} sample />
      <div className="sessions__split">
        <InsetGroup title="Sample sessions" footer="Generated, most recent first">
          {list.map((s) => (
            <ListRow key={s.id} icon={BrainCircuit} title={<>{new Date(s.start).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" })} <span className="ns-tag">Sample</span></>}
              subtitle={rowLine(s)} value={`${minutes(s)} min`} onSelect={() => setId(s.id)} selected={s.id === picked.id} />
          ))}
        </InsetGroup>
        <Card className="sessions__detail" title="Sample session" headingLevel={2} accessory={<SampleBadge />}>
          <SessionGraphs session={picked} others={list} />
        </Card>
      </div>
    </section>
  );
}
