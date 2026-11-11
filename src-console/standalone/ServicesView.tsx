import { useState } from "react";
import { Button, DataTag, Dialog, PageHero, StatusText } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { SERVICE_GROUPS, type Service, type StandaloneAdapter } from "./adapters";
import { SaCard, ServiceStatus, Spin, WireNote } from "./parts";

/** Attention means you might act: slow or not running. Restarts the supervisor is handling, stops you made and the transition wait don't count. */
export const needsAttention = (s: Service) => s.group !== "side" && (s.state === "degraded" || s.state === "failed");

/** "1 needs attention" / "All healthy". */
export function servicesStatement(list: Service[]): string {
  const n = list.filter(needsAttention).length;
  return n === 0 ? "All healthy" : `${n} need${n === 1 ? "s" : ""} attention`;
}

export function servicesDetail(list: Service[], checked: string): string {
  const c = (st: Service["state"]) => list.filter((s) => s.state === st).length;
  const parts = [`${c("healthy")} of ${list.length} healthy`];
  if (c("degraded")) parts.push(`${c("degraded")} slow`);
  if (c("restarting")) parts.push(`${c("restarting")} restarting`);
  const byYou = list.filter((s) => s.state === "stopped" && s.reasonOwner === "user").length;
  if (byYou) parts.push(`${byYou} stopped by you`);
  if (c("waiting")) parts.push(`${c("waiting")} waiting`);
  if (c("failed")) parts.push(`${c("failed")} not running`);
  parts.push(`checked ${checked}`);
  return parts.join(" · ");
}

function ServiceRow({ s, onLogs, onAction, onWhy, busy }: { s: Service; onLogs: () => void; onAction: (a: "restart" | "start") => void; onWhy: () => void; busy: boolean }) {
  const restartDisabled = s.state === "restarting" || busy;
  let acts;
  if (s.group === "side") acts = <Button size="small" variant="secondary" disabled aria-label={`Open ${s.name} (needs wiring)`}>Open ›</Button>;
  else if (s.state === "waiting" && s.reasonOwner === "transition") acts = <Button size="small" variant="secondary" onClick={onWhy}>Why?</Button>;
  else acts = <>
    {s.state === "stopped" ? <Button size="small" variant="secondary" onClick={() => onAction("start")} disabled={busy} aria-label={`Start ${s.name}`}>Start</Button>
      : <Button size="small" variant={s.state === "degraded" || s.state === "failed" ? "primary" : "secondary"} onClick={() => onAction("restart")} disabled={restartDisabled} aria-label={`Restart ${s.name}`}>{busy ? "Restarting…" : "Restart"}</Button>}
    <Button size="small" variant="secondary" onClick={onLogs} aria-label={`Logs for ${s.name}`}>Logs</Button>
  </>;
  return (
    <div className="pk-svc" data-hl={s.state === "degraded" || s.state === "failed" || undefined} data-service={s.id}>
      <div className="pk-svc__name"><span>{s.name}</span><small>{s.what}</small></div>
      <div className="pk-svc__st"><ServiceStatus state={s.state} /><small>{s.meta}</small></div>
      <div className="pk-svc__acts">{acts}</div>
    </div>
  );
}

/** Settings › Services (design-1111s s2a/s2b): every service Amara supervises, in plain words. */
export function ServicesView({ adapter }: { adapter: StandaloneAdapter }) {
  const w = adapter.services();
  const v = valueOf(w);
  const [logsFor, setLogsFor] = useState<Service | null>(null);
  const [why, setWhy] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const act = async (s: Service, a: "restart" | "start") => {
    setBusy(s.id);
    const r = await adapter.serviceAction(s.id, a);
    setBusy(null);
    setNote(r.state === "unwired" ? `${s.name}: restart needs wiring.` : `${s.name}: ${a === "start" ? "starting" : "restarting"}.`);
  };
  if (!v) return (
    <div className="pk-host sa-page">
      <PageHero title="Services" statement="Service health" detail={<>Status, restart and logs for everything Amara runs <DataTag kind="wire" /></>} />
      <SaCard title="Services" w={w}><WireNote>Needs wiring: Amara&apos;s supervisor reports each service here once Claude builds it (HANDOFF-1111s §2). Nothing here is made up.</WireNote></SaCard>
    </div>
  );
  const list = v.services;
  const c = (f: (s: Service) => boolean) => list.filter(f);
  const slow = c((s) => s.state === "degraded"), restarting = c((s) => s.state === "restarting"), idle = c((s) => s.state === "stopped" || s.state === "waiting" || s.state === "failed");
  const group = (id: Service["group"]) => {
    const rows = list.filter((s) => s.group === id);
    const title = SERVICE_GROUPS.find((g) => g.id === id)!.title;
    if (rows.length === 0) return null;
    return <SaCard key={id} title={title} w={id === "side" ? undefined : w}>{rows.map((s) => <ServiceRow key={s.id} s={s} busy={busy === s.id} onLogs={() => setLogsFor(s)} onAction={(a) => { void act(s, a); }} onWhy={() => setWhy(true)} />)}</SaCard>;
  };
  const logs = logsFor ? valueOf(adapter.serviceLogs(logsFor.id)) : null;
  return (
    <div className="pk-host sa-page">
      <PageHero title="Services" statement={servicesStatement(list)} detail={servicesDetail(list, v.checked)} />
      <div className="pk-sum">
        <div className="pk-tile"><span className="pk-tile__k">Healthy</span><span className="pk-tile__v">{c((s) => s.state === "healthy").length}</span><span className="pk-tile__s"><StatusText kind="ok">Working normally</StatusText></span></div>
        <div className="pk-tile"><span className="pk-tile__k">Slow</span><span className="pk-tile__v">{slow.length}</span><span className="pk-tile__s">{slow.length ? <StatusText kind="wait">{slow.map((s) => s.name.replace(/^Automations \((.+)\)$/, "$1")).join(" · ")}</StatusText> : "None"}</span></div>
        <div className="pk-tile"><span className="pk-tile__k">Restarting</span><span className="pk-tile__v">{restarting.length}</span><span className="pk-tile__s">{restarting.length ? <Spin>{restarting.map((s) => s.name).join(" · ")}</Spin> : "None"}</span></div>
        <div className="pk-tile"><span className="pk-tile__k">Stopped or waiting</span><span className="pk-tile__v">{idle.length}</span><span className="pk-tile__s">{idle.length ? <StatusText kind="off">{idle.map((s) => (s.id === "homeassistant" ? "HA" : s.name)).join(" · ")}</StatusText> : "None"}</span></div>
      </div>
      {group("core")}
      <div className="pk-g2 sa-g2-even">
        <div className="pk-stack">{group("automation")}{group("memory")}</div>
        <div className="pk-stack">{group("health")}{group("side")}</div>
      </div>
      <p className="pk-note">Every service listens on this Mac only and needs Amara&apos;s key, which changes each launch. Amara restarts a stopped service up to 3 times, then tells you. Logs hide keys and passwords. <DataTag kind="wire" /></p>
      {note && <p className="pk-note" role="status">{note}</p>}
      <Dialog open={logsFor !== null} title={logsFor ? `${logsFor.name} · what happened` : "Logs"} onCancel={() => setLogsFor(null)} className="sa-sheet sa-sheet--wide">
        <p>In plain words, newest first. {logs && <DataTag kind="sample" />}</p>
        {logs && <ol className="pk-log">{logs.events.map((e, i) => <li key={i}><time>{e.at}</time><StatusText kind={e.level === "ok" ? "ok" : e.level === "warn" ? "wait" : "fail"} /><span>{e.text}{e.hint && <small>{e.hint}</small>}</span></li>)}</ol>}
        {logs && <details open><summary className="pk-note">Raw log (last {logs.tail.length} lines, keys hidden)</summary><pre className="pk-raw">{logs.tail.join("\n")}</pre></details>}
        <div className="pk-sheet__actions">
          <Button size="small" variant="secondary" disabled>Copy for Claude</Button>
          <Button size="small" variant="secondary" disabled>Show in Finder</Button>
          <Button size="small" variant="secondary" onClick={() => setLogsFor(null)}>Close</Button>
          {logsFor && logsFor.state !== "stopped" && <Button size="small" variant="primary" onClick={() => { const s = logsFor; setLogsFor(null); void act(s, "restart"); }}>Restart {logsFor.name.replace(/^Automations \((.+)\)$/, "$1")}</Button>}
        </div>
      </Dialog>
      <Dialog open={why} title="Why is Health sync waiting?" onCancel={() => setWhy(false)} confirmLabel="OK" onConfirm={() => setWhy(false)} className="sa-sheet">
        <p>Another app is using your WHOOP; Amara will take over. Until then Amara reads your health data read-only, and nothing is missing. You can switch now in Settings › Devices.</p>
      </Dialog>
    </div>
  );
}

