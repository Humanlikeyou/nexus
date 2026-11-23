import { ChevronLeft, Copy, RotateCcw, Square } from "lucide-react";
import { useState } from "react";
import type { CrewApproval, CrewDecision, CrewHandoff, CrewsAdapter } from "../crews/adapters";
import { dateTime, humanize, plainText, sentenceStart } from "../lib/format";
import type { Wired } from "../lib/sampleAdapters";
import type { CrewInfo } from "../types";
import { Button, Card, DataTag, PageHero, StatusText, wiredTag, type StatusKind } from "../ui";

const RUN_KIND: Record<string, StatusKind> = { running: "run", awaiting_approval: "wait", completed: "ok", failed: "fail", stopped: "off", interrupted: "risk", cancelled: "off" };
const acc = (node: React.ReactNode) => <span className="cc-card__acc">{node}</span>;

/** D-C3/C4: crew requests only (Terrance's approvals keep their own queue in chat and Today). */
export function CrewApprovalsCard({ approvals, full = false, onViewAll, onDecide, onOpenRun, runKnown }: { approvals: Wired<CrewApproval[]>; full?: boolean; onViewAll?: () => void; onDecide?: (id: string, approve: boolean) => Promise<string | null>; onOpenRun?: (runId: string) => void; runKnown?: (runId: string) => boolean }) {
  // Decision 0042: a decision says what happens next and links to the run it belongs to.
  const [note, setNote] = useState<{ text: string; runId?: string } | null>(null);
  if (approvals.state === "unwired") {
    return <Card title="Crew approvals" accessory={acc(wiredTag(approvals))}><p className="cc-note">Crew requests that need your yes (outbound email, posts, spending) appear here once crews report them.</p></Card>;
  }
  const items = full ? approvals.value : approvals.value.slice(0, 2);
  const decide = (item: CrewApproval, yes: boolean) => {
    if (approvals.state === "live" && onDecide) {
      void onDecide(item.id, yes).then((error) => setNote(error
        ? { text: error }
        : { text: yes ? `Approved \u201c${item.title}\u201d. The crew is continuing.` : `Denied \u201c${item.title}\u201d. Nothing runs for it.`, runId: item.runId }));
      return;
    }
    setNote(approvals.state === "sample" ? { text: `Sample data: nothing was ${yes ? "approved" : "denied"} for \u201c${item.title}\u201d.` } : null);
  };
  return (
    <Card title={`Crew approvals \u00b7 ${approvals.value.length}`} accessory={acc(<>{wiredTag(approvals)}{!full && onViewAll && approvals.value.length > 0 && <button type="button" className="cc-link" onClick={onViewAll}>View all ›</button>}</>)}>
      {items.length === 0 ? <p className="cc-note">Nothing waiting.</p> : items.map((item) => (
        <div className="cc-row" key={item.id}>
          <StatusText kind="wait" />
          <div className="cc-row__main">
            <span>{item.title}</span>
            <small>{item.crew} · {item.agents} · {item.impact}{item.amount.state === "unwired" && item.amount.reason === "currency" ? <> · amount <DataTag kind="currency" /></> : null}</small>
            {full && <small>Requested {item.requested} · Risk: {item.risk} · Asked because {item.reason}</small>}
          </div>
          {full && <span className="cc-dim cc-risk">{item.risk} risk</span>}
          <Button size="small" variant="secondary" onClick={() => decide(item, false)} aria-label={`Deny ${item.title}`}>Deny</Button>
          <Button size="small" variant="primary" onClick={() => decide(item, true)} aria-label={`Approve ${item.title}`}>Approve</Button>
        </div>
      ))}
      {note && <p className="cc-note" role="status">{note.text}{note.runId && onOpenRun && (!runKnown || runKnown(note.runId)) && <> <button type="button" className="cc-link" onClick={() => onOpenRun(note.runId!)}>Open run ›</button></>}</p>}
    </Card>
  );
}

export function CrewDecidedCard({ decided, onOpenRun, runKnown }: { decided: Wired<CrewDecision[]>; onOpenRun?: (runId: string) => void; runKnown?: (runId: string) => boolean }) {
  return (
    <Card title="Recently decided" accessory={acc(wiredTag(decided))}>
      {decided.state === "unwired" ? <p className="cc-note">Your past crew decisions appear here.</p> : decided.value.map((d) => (
        <div className="cc-row" key={d.id}><div className="cc-row__main"><span>{d.title}</span><small>{d.crew} · {d.approved ? "approved" : "denied"} by you · {d.when}</small></div>{d.runId && onOpenRun && (!runKnown || runKnown(d.runId)) && <button type="button" className="cc-link" onClick={() => onOpenRun(d.runId!)} aria-label={`Open run for ${d.title}`}>Open run ›</button>}<StatusText kind={d.approved ? "ok" : "fail"}>{d.approved ? "Approved" : "Denied"}</StatusText></div>
      ))}
    </Card>
  );
}

function HandoffGraph({ handoffs, failed }: { handoffs: CrewHandoff[]; failed: string | null }) {
  // Main chain left to right; a node reached from a mid-chain agent hangs below it (tools).
  const chain: string[] = [];
  for (const h of handoffs) { if (!chain.length) chain.push(h.from); if (chain[chain.length - 1] === h.from && !chain.includes(h.to) && !/_/.test(h.to)) chain.push(h.to); }
  const below = handoffs.filter((h) => !chain.includes(h.to));
  const x = (i: number) => 4 + i * 100;
  const label = handoffs.map((h) => `${h.from} to ${h.to}`).join(", ");
  return (
    <svg className="cc-graph" viewBox="0 0 300 116" role="img" aria-label={`Handoffs: ${label}${failed ? `; ${failed} failed` : ""}`}>
      <defs><marker id="cc-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0L8 4L0 8z" className="cc-graph__tip" /></marker></defs>
      {chain.slice(1).map((_, i) => <path key={i} className="e" d={`M${x(i) + 76} 30 H${x(i + 1)}`} />)}
      {chain.map((n, i) => {
        const after = failed ? chain.indexOf(failed) : -1;
        const state = n === failed ? "fail" : after >= 0 && i > after ? "skip" : "ok";
        return <g key={n} className={`n ${state}`}><rect x={x(i)} y="12" width="76" height="36" rx="8" strokeDasharray={state === "skip" ? "3 3" : undefined} /><text x={x(i) + 10} y="28">{n}</text><text className="sub" x={x(i) + 10} y="41">{state === "fail" ? "\u2715 failed" : state === "skip" ? "\u2013 not run" : "\u2713 done"}</text></g>;
      })}
      {below.map((h, j) => { const i = Math.max(0, chain.indexOf(h.from)); return <g key={h.to} className="n"><path className="e" d={`M${x(i) + 38} 48 V${72 + j * 0}`} /><rect x={x(i) + j * 80} y="74" width="76" height="32" rx="8" /><text x={x(i) + j * 80 + 10} y="94">{h.to}</text></g>; })}
    </svg>
  );
}

/** D-C6–C9: one run, its summary, timeline, handoffs and output. */
export function CrewRunPage({ run, adapter, busy, onBack, onStop, onRetry }: { run: CrewInfo; adapter: CrewsAdapter; busy: boolean; onBack: () => void; onStop: () => void; onRetry?: () => void }) {
  const metrics = adapter.runMetrics(run.id);
  const steps = adapter.steps(run.id);
  const handoffs = adapter.handoffs(run.id);
  const m = metrics.state === "unwired" ? null : metrics.value;
  const live = run.status === "running" || run.status === "awaiting_approval";
  const failedStep = steps.state === "unwired" ? null : steps.value.find((s) => s.status === "failed")?.agent ?? null;
  const result = run.result ? plainText(run.result) : null;
  const tile = (k: string, v: string, w: Wired<unknown>) => <li className="cc-tile"><span className="cc-tile__k">{k}</span><span className="cc-tile__v">{v}</span><span className="cc-tile__s">{wiredTag(w)}</span></li>;
  return (
    <>
      <nav className="cc-crumb" aria-label="Breadcrumb"><Button className="cc-back" variant="plain" size="small" icon={ChevronLeft} onClick={onBack}>Crews</Button> › Runs › {sentenceStart(run.name)}</nav>
      <PageHero title="Crew run" statement={`${sentenceStart(run.name)} ${humanize(run.status).toLowerCase()}`}
        detail={[run.task, run.agents.join(", "), run.started_at ? `started ${dateTime(run.started_at)}` : null].filter(Boolean).join(" · ")} />
      <ul className="cc-strip" aria-label="Run summary">
        <li className="cc-tile"><span className="cc-tile__k">Status</span><span className="cc-tile__v"><StatusText kind={RUN_KIND[run.status] ?? "off"}>{humanize(run.status)}</StatusText></span><span className="cc-tile__s">{run.status === "failed" && result ? result.slice(0, 40) : live ? "Checking every 5 seconds" : ""}</span></li>
        {tile("Duration", m ? `${m.durationS} s` : "\u2014", metrics)}
        {tile("Tokens \u00b7 requests", m ? `${m.tokens.toLocaleString("en-US")} \u00b7 ${m.requests}` : "\u2014", metrics)}
        {tile("Cost", m ? (m.costUsd === 0 ? "$0.00 local" : `$${m.costUsd.toFixed(2)}`) : "\u2014", metrics)}
      </ul>
      <Card title="Timeline" accessory={acc(wiredTag(steps))}>
        {steps.state === "unwired" ? <p className="cc-note">Per-step timing appears here once the crew service reports steps.</p> : (
          <div className="cc-wf" role="table" aria-label="Task timeline">
            {steps.value.map((s) => (
              <div className="cc-wf__row" role="row" key={s.id}><span role="cell">{s.label} · {s.agent}</span>
                <span className="cc-wf__track" role="cell"><i className="cc-wf__bar" data-s={s.status === "failed" ? "fail" : s.status === "skipped" ? "wait" : "ok"} style={{ left: `${s.startPct}%`, width: `${s.endPct - s.startPct}%` }} /></span>
                <span role="cell" className="cc-num">{s.took}{s.status === "failed" ? " \u2715" : ""}</span></div>
            ))}
          </div>
        )}
      </Card>
      <div className="cc-grid2">
        <Card title="Handoffs" accessory={acc(wiredTag(handoffs))}>
          {handoffs.state === "unwired" ? <p className="cc-note">Which agent passed work to which appears here once runs report handoffs.</p> : <HandoffGraph handoffs={handoffs.value} failed={failedStep} />}
        </Card>
        <Card title="Output" accessory={live ? <Button variant="secondary" size="small" icon={Square} disabled={busy} onClick={onStop}>Stop</Button> : undefined}>
          <dl className="cc-kv"><dt>Task</dt><dd>{run.task ?? "No task text"}</dd><dt>Agents</dt><dd>{run.agents.join(", ") || "\u2014"}</dd><dt>Result</dt><dd><StatusText kind={RUN_KIND[run.status] ?? "off"}>{humanize(run.status)}</StatusText></dd></dl>
          {live ? <p className="crews__note" role="status">Running… checking every 5 seconds.</p>
            : result ? <pre className={run.status === "failed" ? "crews__result crews__result--error" : "crews__result"}>{run.result}</pre>
              : <p className="crews__note">No result recorded.</p>}
          {!live && (
            <div className="cc-actions">
              {onRetry && <Button size="small" variant="primary" icon={RotateCcw} onClick={onRetry}>Retry run</Button>}
              {result && <Button size="small" variant="secondary" icon={Copy} onClick={() => { void navigator.clipboard?.writeText(result).catch(() => undefined); }}>{run.status === "failed" ? "Copy error" : "Copy result"}</Button>}
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
