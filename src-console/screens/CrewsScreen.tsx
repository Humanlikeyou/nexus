import { ChevronLeft, Play, Plus, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { INITIAL_SNAPSHOT, getCrewStatus, startCrew, stopCrew } from "../bridge";
import { dateTime, humanize, plainText, sentenceStart } from "../lib/format";
import type { CrewInfo, CrewServiceHealth, SystemSnapshot } from "../types";
import { Button, Card, DataTag, Dialog, More, PageHero, StatusText, TextField, wiredTag, type StatusKind, type Tone } from "../ui";
import { crewsAdapter, type CrewsAdapter } from "../crews/adapters";
import { CrewApprovalsCard, CrewDecidedCard, CrewRunPage } from "./CrewPanels";
import { crewToolsAdapter, type CrewToolsAdapter } from "../crews/toolGrant";
import { CrewToolsPage, GrantCell } from "./CrewToolsPanels";
import { CREW_TOOL_OUTPUT_CAP, CREW_TOOL_TIMEOUT_S } from "../app/statusViews";
import "./CrewsScreen.css";
import "./CommandCenter.css";

/** Same rule as crew-service CrewStartRequest._name. */
export const CREW_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9 _.-]{0,63}$/;
/** crew-service CREW_MAX_TASK_CHARS. */
export const CREW_MAX_TASK_CHARS = 4000;
export const CREW_POLL_MS = 5000;

const HEALTH_BADGE: Record<CrewServiceHealth, { tone: Tone; text: string }> = {
  online: { tone: "success", text: "Online" },
  offline: { tone: "warning", text: "Offline" },
  not_configured: { tone: "neutral", text: "Not configured" },
};

function errorText(e: unknown): string {
  return plainText(String(e instanceof Error ? e.message : e)).slice(0, 300);
}

function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="crews__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

/** Chat & Agents › Crews (Stage 5E): crew-service health, runs, start and stop. */
export const RUN_STATUS: Record<string, StatusKind> = { running: "run", completed: "ok", failed: "fail", stopped: "off", interrupted: "risk" };
type CrewPage = { kind: "overview" } | { kind: "run"; id: string } | { kind: "approvals" } | { kind: "crew"; name: string };

/** design-1110d D-C1–C11: Agents › Crews as a command center. Panels with no source read from `adapter`. */
export function CrewsField({ data, onRetry, loading, adapter = crewsAdapter(), tools = crewToolsAdapter() }: { data: SystemSnapshot; onRetry?: () => void; loading?: boolean; adapter?: CrewsAdapter; tools?: CrewToolsAdapter }) {
  const isLoading = loading ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt;
  const { health } = data.crew;
  const listDown = data.unavailableSources.some((s) => s.command === "crew_list");

  // Local copy so polled/started runs update without waiting for the next snapshot.
  const [runs, setRuns] = useState<CrewInfo[]>(data.crew.runs);
  const [snapshotRuns, setSnapshotRuns] = useState(data.crew.runs);
  if (snapshotRuns !== data.crew.runs) {
    // A new snapshot replaces the local copy (adjusting state during render, not in an effect).
    setSnapshotRuns(data.crew.runs);
    setRuns(data.crew.runs);
  }
  const [page, setPage] = useState<CrewPage>({ kind: "overview" });
  const selected = page.kind === "run" ? runs.find((r) => r.id === page.id) ?? null : null;
  const [formOpen, setFormOpen] = useState(false);

  const [name, setName] = useState("");
  const [task, setTask] = useState("");
  const [confirmStart, setConfirmStart] = useState(false);
  const [confirmStop, setConfirmStop] = useState<CrewInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const nameError = name && !CREW_NAME_RE.test(name) ? "Start with a letter or digit; then letters, digits, space, _ . - (max 64)." : undefined;
  const canStart = health === "online" && !busy && CREW_NAME_RE.test(name) && task.trim().length > 0 && task.length <= CREW_MAX_TASK_CHARS;

  // Poll crew_status every 5 s, only while a run is running and this view is mounted.
  const runningIds = useMemo(() => runs.filter((r) => r.status === "running").map((r) => r.id).join(","), [runs]);
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);
  useEffect(() => {
    if (!runningIds) return;
    const timer = window.setInterval(() => {
      for (const id of runningIds.split(",")) {
        getCrewStatus(id).then((info) => {
          if (mounted.current) setRuns((prev) => prev.map((r) => (r.id === info.id ? info : r)));
        }).catch(() => { /* transient; the next poll or snapshot retries */ });
      }
    }, CREW_POLL_MS);
    return () => window.clearInterval(timer);
  }, [runningIds]);

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (canStart) setConfirmStart(true);
  };

  const start = async () => {
    setBusy(true);
    setActionError(null);
    try {
      const info = await startCrew(name.trim(), task.trim());
      setRuns((prev) => [info, ...prev.filter((r) => r.id !== info.id)]);
      setName("");
      setTask("");
    } catch (e) {
      setActionError(errorText(e));
    } finally {
      setBusy(false);
      setConfirmStart(false);
    }
  };

  const stop = async (run: CrewInfo) => {
    setBusy(true);
    setActionError(null);
    try {
      await stopCrew(run.id);
      const info = await getCrewStatus(run.id).catch(() => ({ ...run, status: "stopped" }));
      setRuns((prev) => prev.map((r) => (r.id === run.id ? info : r)));
    } catch (e) {
      setActionError(errorText(e));
    } finally {
      setBusy(false);
      setConfirmStop(null);
    }
  };

  if (isLoading) {
    return (
      <div className="crews page-inset">
        <PageHero title="Crews" statement="Checking" detail={<span role="status">Loading crew service…</span>} />
      </div>
    );
  }

  const badge = HEALTH_BADGE[health];
  const dialogs = (
    <>
      {actionError && <p className="crews__error" role="alert">{actionError}</p>}
      <Dialog open={confirmStart} title="Start this run?" confirmLabel="Start run" busy={busy}
        onConfirm={() => { void start(); }} onCancel={() => setConfirmStart(false)}>
        Runs on the local model inside the crew service. It may take a few minutes; you can stop it from the run.
      </Dialog>
      <Dialog open={confirmStop !== null} title="Stop this run?" confirmLabel="Stop Run" destructive busy={busy}
        onConfirm={() => { if (confirmStop) void stop(confirmStop); }} onCancel={() => setConfirmStop(null)}>
        {confirmStop ? `\u201c${confirmStop.name}\u201d stops now and its partial work is discarded.` : null}
      </Dialog>
    </>
  );

  if (selected) {
    return (
      <div className="crews page-inset cc-page">
        <CrewRunPage run={selected} adapter={adapter} busy={busy} onBack={() => setPage({ kind: "overview" })}
          onStop={() => setConfirmStop(selected)}
          onRetry={health === "online" ? () => { setName(selected.name); setTask(selected.task ?? ""); setConfirmStart(true); } : undefined} />
        {dialogs}
      </div>
    );
  }

  if (page.kind === "crew") {
    const mine = runs.filter((r) => r.name === page.name);
    const last = mine[0];
    return (
      <div className="crews page-inset cc-page">
        <CrewToolsPage crew={page.name} agents={[...new Set(mine.flatMap((r) => r.agents))]} adapter={tools}
          lastRun={last ? `last run ${humanize(last.status).toLowerCase()}${last.started_at ? ` · ${dateTime(last.started_at)}` : ""}` : "no runs yet"}
          onBack={() => setPage({ kind: "overview" })} />
      </div>
    );
  }

  const approvals = adapter.approvals();
  if (page.kind === "approvals") {
    const count = approvals.state === "unwired" ? 0 : approvals.value.length;
    return (
      <div className="crews page-inset cc-page">
        <Button className="cc-back" variant="plain" size="small" icon={ChevronLeft} onClick={() => setPage({ kind: "overview" })}>Crews</Button>
        <PageHero title="Crews" statement={approvals.state === "unwired" ? "Crew approvals" : `${count} crew approval${count === 1 ? "" : "s"} waiting`}
          detail={<>Only crew requests are here; Terrance&apos;s approvals stay in chat and Today.{wiredTag(approvals) && <span className="cc-hero-tag">{wiredTag(approvals)}</span>}</>} />
        <CrewApprovalsCard approvals={approvals} full />
        <CrewDecidedCard decided={adapter.decided()} />
      </div>
    );
  }

  const failed = runs.filter((r) => r.status === "failed");
  const latestFail = failed[0];
  const statement = health !== "online" ? badge.text : runs.length === 0 ? "No runs yet" : failed.length ? `${failed.length} of ${runs.length} recent run${runs.length === 1 ? "" : "s"} failed` : runs.some((r) => r.status === "running") ? "Running" : "All recent runs completed";
  const detail = health !== "online" ? undefined : latestFail
    ? [sentenceStart(latestFail.name), latestFail.result ? plainText(latestFail.result).slice(0, 80) : null, latestFail.started_at ? dateTime(latestFail.started_at) : null].filter(Boolean).join(" · ")
    : "Runs use the local model. At most 2 run at once.";
  const n8n = adapter.n8n();
  const next = adapter.nextSchedule();
  const roster = [...new Set(runs.map((r) => r.name))].map((name) => {
    const mine = runs.filter((r) => r.name === name);
    return { name, last: mine[0], ok: mine.filter((r) => r.status === "completed").length, total: mine.length };
  });
  const toolsDown = data.unavailableSources.some((s) => s.command === "crew_tools");

  return (
    <div className="crews page-inset cc-page">
      <PageHero title="Crews" statement={statement} detail={detail} />
      {health === "not_configured" ? (
        <p className="crews__note">No crew token found. Run <code>scripts/sync-crew-token.sh</code>, then reopen Amara OS.</p>
      ) : health === "offline" ? (
        <div className="crews__unavailable">
          <p>The crew service on 127.0.0.1:8400 isn&apos;t responding.</p>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
        </div>
      ) : null}

      <ul className="cc-strip" aria-label="Crew health">
        <li className="cc-tile"><span className="cc-tile__k">Crew service</span><span className="cc-tile__v"><StatusText kind={health === "online" ? "ok" : health === "offline" ? "risk" : "off"}>{badge.text}</StatusText></span><span className="cc-tile__s">At most 2 runs at once</span></li>
        <li className="cc-tile"><span className="cc-tile__k">OpenClaw gateway</span><span className="cc-tile__v"><StatusText kind={data.openclaw.online ? "ok" : "off"}>{data.openclaw.online ? "Online" : "Offline"}</StatusText></span><span className="cc-tile__s">{data.openclaw.model || "No model"}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">n8n workflows</span><span className="cc-tile__v">{n8n.state === "unwired" ? <StatusText kind="off">Not connected</StatusText> : <StatusText kind={n8n.value.connected ? "ok" : "off"}>{n8n.value.workflows} workflows</StatusText>}</span><span className="cc-tile__s">{wiredTag(n8n)}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Schedules</span><span className="cc-tile__v">{data.hermes.activeSchedules} active</span><span className="cc-tile__s">Next run {next.state === "unwired" ? wiredTag(next) : next.value}</span></li>
      </ul>

      <CrewApprovalsCard approvals={approvals} onViewAll={() => setPage({ kind: "approvals" })} />

      {health === "online" && (
        <Card title="Runs" accessory={<span className="cc-card__acc"><Button variant="secondary" size="small" icon={Plus} onClick={() => { setFormOpen(true); requestAnimationFrame(() => document.getElementById("crews-new-run")?.scrollIntoView({ block: "nearest" })); }}>New run</Button></span>}>
          {listDown && runs.length === 0 ? <Unavailable label="Crew runs" onRetry={onRetry} />
            : runs.length === 0 ? <p className="crews__note"><strong>No runs yet</strong><br />Start one with New run; it appears here while it runs.</p>
              : (
                <table className="cc-table cc-runs" aria-label="Runs">
                  <thead><tr><th scope="col">Run</th><th scope="col">Status</th><th scope="col">Agents</th><th scope="col">Started</th><th scope="col" className="cc-num">Duration</th><th scope="col" className="cc-num">Tokens</th><th scope="col" className="cc-num">Cost</th></tr></thead>
                  <tbody>
                    {runs.map((run) => (
                      <tr key={run.id} tabIndex={0} aria-label={`Open run ${sentenceStart(run.name)}`} onClick={() => setPage({ kind: "run", id: run.id })} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setPage({ kind: "run", id: run.id }); } }}>
                        <td>{sentenceStart(run.name)}<small className="crews__clamp" title={run.task ?? undefined}>{run.task ?? "No task text"}</small></td>
                        <td><StatusText kind={RUN_STATUS[run.status] ?? "off"}>{humanize(run.status)}</StatusText>{run.status === "failed" && run.result && <small>{plainText(run.result).slice(0, 60)}</small>}</td>
                        <td>{run.agents.join(", ") || "\u2014"}</td>
                        <td>{run.started_at ? dateTime(run.started_at) : "\u2014"}</td>
                        <td className="cc-num cc-dim">{"\u2014"}</td><td className="cc-num cc-dim">{"\u2014"}</td><td className="cc-num cc-dim">{"\u2014"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
          <p className="cc-note">Duration, tokens and cost per run <DataTag kind="wire" /> · Open a run for its timeline.</p>
        </Card>
      )}

      <div className="cc-grid2">
        <Card title="Crews" accessory={<span className="cc-card__acc">{wiredTag<unknown>(roster.length ? tools.grant(roster[0].name) : tools.audit(""))}</span>}>
          {roster.length === 0 ? <p className="cc-note">Crews appear after their first run.</p> : (
            <table className="cc-table cc-crews" aria-label="Crews">
              <thead><tr><th scope="col">Crew</th><th scope="col">Last run</th><th scope="col">Files and terminal</th><th scope="col"><span className="ui-visually-hidden">Open</span></th></tr></thead>
              <tbody>
                {roster.map((c) => {
                  const grant = tools.grant(c.name);
                  const asked = grant.state !== "unwired" && grant.value.status === "pending";
                  return (
                    <tr key={c.name}>
                      <td>{c.name}<small>{c.last.agents.join(" · ") || "No agents"} · {c.ok}/{c.total} ok</small></td>
                      <td><StatusText kind={RUN_STATUS[c.last.status] ?? "off"}>{humanize(c.last.status)}</StatusText></td>
                      <td className="cc-grant"><GrantCell grant={grant} /></td>
                      <td className="cc-num"><Button variant="plain" size="small" aria-label={`${asked ? "Review" : "Manage"} ${c.name}`} onClick={() => setPage({ kind: "crew", name: c.name })}>{asked ? "Review ›" : "Manage ›"}</Button></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <p className="cc-note">Files and terminal: per crew, off by default. Open a crew to turn it on or off. Next scheduled run per crew needs wiring.</p>
        </Card>
        <Card title="Tools and guardrails">
          {toolsDown ? <Unavailable label="Crew tools" onRetry={onRetry} />
            : data.crew.tools.length === 0 ? <p className="crews__note">No tools enabled</p>
              : data.crew.tools.map((t) => (
                <div className="cc-row" key={t.name}><div className="cc-row__main"><span>{t.name}</span><small>{CREW_TOOL_TIMEOUT_S} s timeout · {CREW_TOOL_OUTPUT_CAP}-char output cap</small></div>
                  <span aria-label={`${t.name}: ${t.allowed ? "allowed" : "blocked"}`}><StatusText kind={t.allowed ? "ok" : "off"}>{t.allowed ? "Allowed" : "Blocked"}</StatusText></span></div>
              ))}
          <div className="cc-row"><div className="cc-row__main"><span>Files and terminal</span><small>Per crew · off by default · only you can turn it on</small></div><StatusText kind="wait">Per crew</StatusText></div>
          <div className="cc-row"><div className="cc-row__main"><span>Outbound (email, posts, spend)</span><small>Always asks first</small></div><StatusText kind="wait">Approval</StatusText></div>
          <div className="cc-row"><div className="cc-row__main"><span>git push, sudo</span><small>Never for crews</small></div><StatusText kind="fail">Blocked</StatusText></div>
        </Card>
      </div>

      <Card title="n8n workflows" accessory={<span className="cc-card__acc">{wiredTag(n8n)}</span>}>
        <p className="cc-note">{n8n.state === "unwired" ? "Workflows, last execution, failures and run time appear here once n8n is connected." : `${n8n.value.workflows} workflows · ${n8n.value.failures7d} failures in 7 days`}</p>
      </Card>

      {health === "online" && (
        <More label="New run" id="crews-new-run" open={formOpen || undefined}>
          <Card title="New Run">
            <form className="crews__form" onSubmit={onSubmit}>
              <TextField label="Name" value={name} maxLength={64} autoComplete="off" error={nameError} onChange={(e) => setName(e.target.value)} />
              <TextField label="Task" value={task} maxLength={CREW_MAX_TASK_CHARS} autoComplete="off"
                description={`${task.length}/${CREW_MAX_TASK_CHARS}`} onChange={(e) => setTask(e.target.value)} />
              <div className="crews__actions">
                <Button type="submit" icon={Play} disabled={!canStart}>Start run</Button>
              </div>
            </form>
          </Card>
        </More>
      )}
      {dialogs}
    </div>
  );
}
