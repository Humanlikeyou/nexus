import { Play, RefreshCw, Square } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { INITIAL_SNAPSHOT, getCrewStatus, startCrew, stopCrew } from "../bridge";
import { date, humanize, plainText } from "../lib/format";
import type { CrewInfo, CrewServiceHealth, SystemSnapshot } from "../types";
import { Badge, Button, Card, Dialog, InsetGroup, ListRow, TextField, type Tone } from "../ui";
import "./CrewsScreen.css";

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

const STATUS_TONE: Record<string, Tone> = {
  running: "info",
  completed: "success",
  failed: "danger",
  stopped: "neutral",
  interrupted: "warning",
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
export function CrewsField({ data, onRetry, loading }: { data: SystemSnapshot; onRetry?: () => void; loading?: boolean }) {
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
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = runs.find((r) => r.id === selectedId) ?? null;

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
      setSelectedId(info.id);
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
      <div className="crews">
        <h1 className="crews__title">Crews</h1>
        <Card title="Crew service"><p className="crews__note" role="status">Loading crew service…</p></Card>
      </div>
    );
  }

  const badge = HEALTH_BADGE[health];
  return (
    <div className="crews">
      <h1 className="crews__title">Crews</h1>
      <Card title="Crew service" accessory={<Badge tone={badge.tone}>{badge.text}</Badge>}>
        {health === "not_configured" ? (
          <p className="crews__note">No crew token found. Run <code>scripts/sync-crew-token.sh</code>, then reopen Amara OS.</p>
        ) : health === "offline" ? (
          <div className="crews__unavailable">
            <p>The crew service on 127.0.0.1:8400 isn&apos;t responding.</p>
            {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
          </div>
        ) : (
          <p className="crews__note">Runs use the local model. At most 2 run at once.</p>
        )}
      </Card>

      {health === "online" && (
        listDown && runs.length === 0 ? <Card title="Runs"><Unavailable label="Crew runs" onRetry={onRetry} /></Card>
          : runs.length === 0 ? (
            <Card title="Runs"><p className="crews__note"><strong>No runs yet</strong><br />Start one below; it appears here while it runs.</p></Card>
          ) : (
            <InsetGroup title="Runs">
              {runs.map((run) => (
                <ListRow key={run.id} title={run.name} selected={run.id === selectedId} onSelect={() => setSelectedId(run.id === selectedId ? null : run.id)}
                  subtitle={<span className="crews__clamp" title={run.task ?? undefined}>{run.task ?? "No task text"}</span>}
                  value={[run.started_at ? date(run.started_at) : null, run.agents.length ? run.agents.join(", ") : null].filter(Boolean).join(" · ")}
                  accessory={<Badge tone={STATUS_TONE[run.status] ?? "neutral"}>{humanize(run.status)}</Badge>} />
              ))}
            </InsetGroup>
          )
      )}

      {selected && (
        <Card title={selected.name} headingLevel={2}
          accessory={selected.status === "running" ? <Button variant="secondary" size="small" icon={Square} disabled={busy} onClick={() => setConfirmStop(selected)}>Stop</Button> : undefined}>
          {selected.status === "running" ? <p className="crews__note" role="status">Running… checking every 5 seconds.</p>
            : selected.result ? (
              <pre className={selected.status === "failed" ? "crews__result crews__result--error" : "crews__result"}>{selected.result}</pre>
            ) : <p className="crews__note">No result recorded.</p>}
        </Card>
      )}

      {health === "online" && (
        <Card title="New Run">
          <form className="crews__form" onSubmit={onSubmit}>
            <TextField label="Name" value={name} maxLength={64} autoComplete="off" error={nameError} onChange={(e) => setName(e.target.value)} />
            <TextField label="Task" value={task} maxLength={CREW_MAX_TASK_CHARS} autoComplete="off"
              description={`${task.length}/${CREW_MAX_TASK_CHARS}`} onChange={(e) => setTask(e.target.value)} />
            <div className="crews__actions">
              <Button type="submit" icon={Play} disabled={!canStart}>Start Run</Button>
            </div>
          </form>
        </Card>
      )}

      {actionError && <p className="crews__error" role="alert">{actionError}</p>}

      <Dialog open={confirmStart} title="Start this run?" confirmLabel="Start Run" busy={busy}
        onConfirm={() => { void start(); }} onCancel={() => setConfirmStart(false)}>
        Runs on the local model inside the crew service. It may take a few minutes; you can stop it from the run.
      </Dialog>
      <Dialog open={confirmStop !== null} title="Stop this run?" confirmLabel="Stop Run" destructive busy={busy}
        onConfirm={() => { if (confirmStop) void stop(confirmStop); }} onCancel={() => setConfirmStop(null)}>
        {confirmStop ? `“${confirmStop.name}” stops now and its partial work is discarded.` : null}
      </Dialog>
    </div>
  );
}
