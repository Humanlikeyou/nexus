import { SignalStream } from "../design/SystemPrimitives";
import { tone, humanize, plainText, date, shortDuration } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { State } from "./shared";
import { Activity, Satellite } from "lucide-react";

export function ActivityField({ data }: { data: SystemSnapshot }) {
  return (
    <div className="system-field activity-field">
      <section className="activity-heading"><span className="system-kicker">History</span><h1>Activity</h1><p>{data.activity.length} recent operations</p></section>
      <section className="delivery-vector"><Satellite size={38} /><SignalStream active={data.outbox.gateway_online} /><div><strong>{data.outbox.queue_depth}</strong><span>QUEUED</span></div><div><strong>{data.outbox.total_delivered}</strong><span>DELIVERED</span></div></section>
      <section className="activity-river">{data.activity.length > 0 ? data.activity.map((item) => <article key={item.id}><time>{date(item.startedAt)}</time><i className={`tone-${tone(item.status)}`} /><strong>{plainText(item.name.replace(/_/g, " "))}<small>{humanize(item.type)}</small></strong><p>{item.detail || item.outcome || "No detail recorded"}</p><span>{shortDuration(item.durationMs)}</span><State value={item.status} /></article>) : <div className="workspace-empty"><Activity size={24} aria-hidden="true" /><strong>No recent operations</strong><p>Mac actions and assistant work will appear here as Nexus completes them.</p></div>}</section>
      <aside className="route-status"><span>DELIVERY ROUTE</span><State value={data.outbox.gateway_online} label={data.outbox.gateway_online ? "GATEWAY CLEAR" : "GATEWAY WAITING"} /><p>{data.outbox.last_error || "No transport errors"}</p><time>{date(data.outbox.last_delivered_at)}</time></aside>
    </div>
  );
}
