import { ACTIVITY_ROUTE, CREWS_ROUTE, type Route } from "../app/routes";
import type { SystemSnapshot } from "../types";
import { Button } from "../ui";
import "./design1118.css";

/** design-1118a: one delivery truth for Agents and Activity. Never "All delivered" when nothing ever was. */
export function deliveryLine(outbox: SystemSnapshot["outbox"], down: boolean): string {
  if (down) return "Delivery is out of reach";
  if (outbox.queue_depth > 0) return `${outbox.queue_depth} waiting to send`;
  if (!outbox.total_delivered && !outbox.last_delivered_at) return "No deliveries yet";
  return "All delivered";
}

const HEALTH = /health[\s_-]?check|heartbeat|health/i;
const timeFmt = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

/** Groups today's routine health checks into one line; everything else stays in the log. */
export function groupPulse(activity: SystemSnapshot["activity"], now = Date.now()) {
  const start = new Date(now); start.setHours(0, 0, 0, 0);
  const today = activity.filter((a) => a.startedAt >= start.getTime());
  const checks = today.filter((a) => HEALTH.test(a.name) || HEALTH.test(a.type)).sort((a, b) => b.startedAt - a.startedAt);
  const failed = checks.filter((c) => /fail|error/i.test(c.status));
  return { checks, failed, others: activity.length - checks.length };
}

type Orb = { id: string; label: string; sub: string; tone: "ok" | "idle" | "warn"; x: number; y: number; to?: Route };

export function AgentOrbs({ data, onOpen }: { data: SystemSnapshot; onOpen?: (route: Route) => void }) {
  const online = data.hermes.status === "online";
  const crewRunning = data.crew.runs.filter((r) => r.status === "running").length;
  const crewSub = data.crew.health === "not_configured" ? "Not configured" : data.crew.health === "offline" ? "Offline" : `${crewRunning} running`;
  const toolsReady = data.capabilities.some((c) => /ready|enabled|available|online|running|ok|configured/i.test(c.status));
  const kb = data.kb;
  const delivered = data.outbox.total_delivered;
  const orbs: Orb[] = [
    { id: "crews", label: "Crews", sub: crewSub, tone: crewRunning ? "ok" : data.crew.health === "offline" ? "warn" : "idle", x: 160, y: 120, to: CREWS_ROUTE },
    { id: "tools", label: "Tools", sub: data.capabilities.length ? (toolsReady ? "Configured" : "Not ready") : "None yet", tone: toolsReady ? "ok" : "idle", x: 720, y: 120 },
    { id: "kb", label: "Knowledge", sub: `${kb.docs != null ? `${kb.docs} docs · ` : ""}${kb.state === "not_running" ? "not running" : kb.state}`, tone: kb.state === "running" ? "ok" : kb.state === "error" ? "warn" : "idle", x: 300, y: 210 },
    { id: "deliveries", label: "Deliveries", sub: delivered ? `${delivered} delivered` : "None yet", tone: delivered ? "ok" : "idle", x: 580, y: 210, to: ACTIVITY_ROUTE },
  ];
  return (
    <section className="d18-card d18-orbs" aria-label="Amara and what she uses">
      <svg viewBox="0 0 880 290" className="d18-orbs__svg" aria-hidden="true">
        {orbs.map((o) => <line key={o.id} x1="440" y1="120" x2={o.x} y2={o.y} className="d18-orbs__link" />)}
        <circle cx="440" cy="120" r="72" className="d18-orbs__halo" /><circle cx="440" cy="120" r="56" className="d18-orbs__halo" />
      </svg>
      <ul className="d18-orbs__list">
        <li className="d18-orb d18-orb--core" data-tone={online ? "ok" : "warn"} style={{ left: "50%", top: `${(120 / 290) * 100}%` }}><b>Amara</b><small>{online ? "Online" : "Offline"}</small></li>
        {orbs.map((o) => {
          const go = onOpen && o.to ? () => onOpen(o.to!) : undefined;
          return (
            <li key={o.id} className={`d18-orb${go ? " d18-orb--link" : ""}`} data-tone={o.tone} style={{ left: `${(o.x / 880) * 100}%`, top: `${(o.y / 290) * 100}%` }}>
              {go
                ? <button type="button" className="d18-orb__hit" aria-label={`Open ${o.label}: ${o.sub}`} onClick={go}><b>{o.label}</b><small>{o.sub}</small></button>
                : <><b>{o.label}</b><small>{o.sub}</small></>}
            </li>
          );
        })}
      </ul>
      <p className="d18-note">Hermes Agent{data.hermes.model ? ` · ${data.hermes.model}` : ""}{kb.model ? ` · knowledge base ${kb.model}${kb.docs != null ? `, ${kb.docs} documents` : ""}` : ""}. Crews are {crewRunning ? "running" : data.crew.health === "offline" ? "offline" : "ready and not running"}.</p>
    </section>
  );
}

export function TodaysPulse({ data, now, onOpen }: { data: SystemSnapshot; now?: number; onOpen?: (route: Route) => void }) {
  const { checks, failed, others } = groupPulse(data.activity, now);
  if (!checks.length) return null;
  const last = checks[0];
  const maxMs = Math.max(1, ...checks.map((c) => c.durationMs));
  const nothing = !data.outbox.total_delivered && !data.outbox.last_delivered_at;
  return (
    <section className="d18-card d18-pulse" aria-labelledby="d18-pulse">
      <div className="d18-map__head"><h2 id="d18-pulse" className="d18-h">Today&apos;s pulse</h2><span className={`d18-chip ${failed.length ? "" : "d18-chip--ok"}`}>{failed.length ? `${failed.length} failed` : "Complete"}</span>{onOpen && <Button size="small" variant="plain" onClick={() => onOpen(ACTIVITY_ROUTE)}>See activity ›</Button>}</div>
      <p className="d18-pulse__line" data-testid="health-line"><b>Health check · {failed.length ? `${failed.length} of ${checks.length} failed` : "All systems healthy"}</b></p>
      <p className="d18-note">{checks.length} time{checks.length === 1 ? "" : "s"} today · last at {timeFmt(last.startedAt)} · {last.durationMs} ms</p>
      <div className="d18-pulse__bars" aria-hidden="true">{[...checks].reverse().slice(-12).map((c) => <i key={c.id} style={{ height: `${8 + (c.durationMs / maxMs) * 16}px` }} data-fail={/fail|error/i.test(c.status) || undefined} />)}</div>
      <p className="d18-pulse__chips">{checks.slice(0, 3).map((c) => <span key={c.id} className="d18-chip">{timeFmt(c.startedAt)} · {c.durationMs} ms</span>)}</p>
      <p className="d18-note">{others > 0 ? `${others} more stay tucked away in the log below. ` : ""}{nothing ? "Nothing has been delivered to you yet." : ""}</p>
    </section>
  );
}
