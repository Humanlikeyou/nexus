import { useState, type KeyboardEvent } from "react";
import { BrainCircuit, HeartPulse, RefreshCw } from "lucide-react";
import { Badge, Button, Card, InsetGroup, ListRow, StatTile } from "../ui";
import { dateTime, number, time } from "../lib/format";
import type { RecordedSession, SystemSnapshot } from "../types";
import "./SessionsScreen.css";

/** Sessions → Mind–Body › Neuro history (screen-specs §5): source list + detail, like Voice Memos. */
const SESSIONS_COMMAND = "list_sessions";
const title = (s: RecordedSession) => (s.device_type === "muse" ? "Muse session" : `${s.device_type} session`);
const minutes = (s: RecordedSession) => Math.max(1, Math.round(s.duration_secs / 60));
const pct = (v: number | null | undefined) => (v == null ? null : Math.round(v));

export function SessionsField({ data, onRetry, onStartSession }: { data: SystemSnapshot; onRetry?: () => void; onStartSession?: () => void }) {
  const sessions = data.sessions;
  const [selectedId, setSelectedId] = useState<string | null>(sessions[0]?.id ?? null);
  const selected = sessions.find((s) => s.id === selectedId) ?? sessions[0] ?? null;
  const failed = data.unavailableSources.some((s) => s.command === SESSIONS_COMMAND);

  if (failed && sessions.length === 0) {
    return (
      <div className="sessions page-inset">
        <h1 className="sessions__title">History</h1>
        <Card title="Sensor sessions unavailable" headingLevel={2}>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
        </Card>
      </div>
    );
  }
  if (sessions.length === 0) {
    return (
      <div className="sessions page-inset">
        <h1 className="sessions__title">History</h1>
        <Card title="No sessions yet" headingLevel={2}>
          <p className="sessions__copy">Finish a Muse session and Amara OS keeps the recording and summary here.</p>
          {onStartSession && <div><Button variant="primary" onClick={onStartSession}>Start session</Button></div>}
        </Card>
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
        <h1 className="sessions__title">History</h1>
      <div className="sessions__tiles">
        <StatTile label="Sessions" value={sessions.length} />
        <StatTile label="Muse sessions" value={museCount} />
        <StatTile label="Total time" value={totalMin} unit="min" />
      </div>
      <div className="sessions__split">
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- ↑/↓ moves the selection between the row buttons inside */}
        <div className="sessions__list" onKeyDown={onListKey}>
          <InsetGroup title="History" footer="Most recent first">
            {sessions.map((s) => (
              <ListRow key={s.id} icon={s.device_type === "muse" ? BrainCircuit : HeartPulse} title={title(s)} subtitle={dateTime(s.start)} value={`${minutes(s)} min`} onSelect={() => setSelectedId(s.id)} selected={s.id === selected?.id} />
            ))}
          </InsetGroup>
        </div>
        {selected ? (
          <Card className="sessions__detail" title={title(selected)} headingLevel={2} accessory={<Badge tone="success">Saved locally</Badge>}>
            <div className="sessions__metrics">
              <StatTile label="Duration" value={minutes(selected)} unit="min" />
              <StatTile label="Data points" value={selected.data_points.toLocaleString()} />
              <StatTile label="Avg focus" value={selected.summary.avg_focus == null ? null : number(selected.summary.avg_focus)} />
              <StatTile label="Avg relaxation" value={selected.summary.avg_relaxation == null ? null : number(selected.summary.avg_relaxation)} />
            </div>
            <div className="sessions__metrics sessions__metrics--beta">
              <StatTile label={<>Calm <Badge>Beta</Badge></>} value={pct(selected.summary.calm_pct)} unit={selected.summary.calm_pct == null ? undefined : "%"} />
              <StatTile label={<>Time in target <Badge>Beta</Badge></>} value={pct(selected.summary.time_in_target_pct)} unit={selected.summary.time_in_target_pct == null ? undefined : "%"} />
            </div>
            <InsetGroup>
              <ListRow title="Session window" value={<><time dateTime={selected.start}>{time(selected.start)}</time> → <time dateTime={selected.end}>{time(selected.end)}</time></>} />
            </InsetGroup>
            <p className="sessions__callout">Scores summarize the recorded signal for reflection. They aren&apos;t clinical measurements.</p>
          </Card>
        ) : (
          <Card className="sessions__detail"><p className="sessions__copy">Select a session</p></Card>
        )}
      </div>
    </div>
  );
}
