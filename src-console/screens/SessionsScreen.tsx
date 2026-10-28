import { useState } from "react";
import { EmptyState, Metric, PageHeading, SectionHeading, StatusIndicator } from "../design/ProductUI";
import { number, date, time } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { BrainCircuit, HeartPulse, History } from "lucide-react";

export function SessionsField({ data }: { data: SystemSnapshot }) {
  const sessions = data.sessions;
  const [selectedId, setSelectedId] = useState<string | null>(sessions[0]?.id ?? null);
  const selected = sessions.find((session) => session.id === selectedId) ?? sessions[0] ?? null;
  const museSessions = sessions.filter((session) => session.device_type === "muse");
  const totalMinutes = sessions.reduce((sum, session) => sum + session.duration_secs / 60, 0);

  return <div className="system-field sessions-field">
    <PageHeading eyebrow="History" title="Sessions" description="Your recordings and session summaries." />
    {sessions.length === 0 ? <EmptyState icon={History} title="No sessions yet" detail="Complete a Muse session and Nexus will keep the recording and summary here." /> : <>
      <section className="session-summary-strip"><Metric label="Recorded" value={sessions.length} detail="All devices" /><Metric label="Muse sessions" value={museSessions.length} detail="Neural recordings" /><Metric label="Total time" value={Math.round(totalMinutes)} unit="min" detail="Across history" /></section>
      <div className="sessions-workspace">
        <section className="session-list"><SectionHeading title="History" detail="Most recent first" />{sessions.map((session) => <button key={session.id} type="button" className={session.id === selected?.id ? "is-selected" : ""} onClick={() => setSelectedId(session.id)}><span className="session-device-icon">{session.device_type === "muse" ? <BrainCircuit size={17} /> : <HeartPulse size={17} />}</span><span><strong>{session.device_type === "muse" ? "Muse session" : `${session.device_type} session`}</strong><small>{date(session.start)}</small></span><em>{Math.max(1, Math.round(session.duration_secs / 60))} min</em></button>)}</section>
        {selected && <section className="session-detail"><SectionHeading title={selected.device_type === "muse" ? "Muse session" : `${selected.device_type} session`} detail={date(selected.start)} action={<StatusIndicator tone="success">Saved locally</StatusIndicator>} /><div className="session-detail-metrics"><Metric label="Duration" value={Math.max(1, Math.round(selected.duration_secs / 60))} unit="min" /><Metric label="Data points" value={selected.data_points.toLocaleString()} /><Metric label="Average focus" value={number(selected.summary.avg_focus)} /><Metric label="Average relaxation" value={number(selected.summary.avg_relaxation)} /></div><div className="session-timeline"><span>Session window</span><div><time>{time(selected.start)}</time><i /><time>{time(selected.end)}</time></div></div><p className="session-caveat">Session scores summarize the recorded signal for reflection. They are not clinical measurements.</p></section>}
      </div>
    </>}
  </div>;
}
