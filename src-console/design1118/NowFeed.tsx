import { useEffect, useState } from "react";
import type { LucideIcon } from "lucide-react";
import { Bot, Clock, Mail, Server, Users, Zap } from "lucide-react";
import { invokeBound } from "../bridge";
import type { ActivityEvent } from "../legacy-core/terrance/system/activityBus";
import type { AgentsNow } from "../bindings.gen";
import type { SystemSnapshot } from "../types";
import { DataTag, InsetGroup, ListRow, More, StatusText } from "../ui";
import "./design1118.css";

/** Who's working on what right now. Fixed labels and titles only: never prompts, messages, ids or health values. */
export type NowStatus = "running" | "waiting" | "queued";
export type NowRow = { key: string; kind: "amara" | "crew" | "routine" | "service" | "action" | "delivery"; who: string; what: string; status: NowStatus; since: number | null };
export type NowJob = Pick<ActivityEvent, "id" | "type" | "name" | "status" | "startedAt">;

export const MAX_NOW_ROWS = 6;
export const NOW_POLL_MS = 5_000;

const ROUTINE_TYPES = new Set(["ritual", "daily_summary", "health_check", "prune", "scheduler", "consolidation", "compression", "skills_evolution", "diagnosis", "auto_heal", "session_save", "reasoning"]);

/** Fixed label per Amara tool; the action's own title can carry a URL or path, so it is never shown. */
export function actionLabel(toolId: string): string {
  if (toolId.startsWith("system.")) return "A task on your Mac";
  if (toolId.startsWith("filesystem.")) return "A file task";
  if (toolId.startsWith("macos.")) return "A Mac workflow";
  return "An Amara action";
}

const ms = (iso: string | null | undefined): number | null => { if (!iso) return null; const t = Date.parse(iso); return Number.isFinite(t) ? t : null; };

export type NowSources = { backend: AgentsNow | null; jobs: NowJob[] | null };

const SNAPSHOT_SOURCES = ["list_terrence_actions", "get_outbox_status", "list_activity_entries"];

/** True when at least one source could be read. When none can, the feed shows "Needs wiring". */
export function nowReadable(data: SystemSnapshot, src: NowSources): boolean {
  const crewUp = data.crew.health === "online";
  const snapshotUp = SNAPSHOT_SOURCES.some((c) => !data.unavailableSources.some((u) => u.command === c));
  return src.backend != null || src.jobs != null || crewUp || snapshotUp;
}

export function buildNowRows(data: SystemSnapshot, src: NowSources): NowRow[] {
  const rows: NowRow[] = [];
  const down = (c: string) => data.unavailableSources.some((u) => u.command === c);
  src.backend?.items.forEach((b, i) => rows.push({ key: `b${i}`, kind: b.kind === "amara" ? "amara" : "service", who: b.who, what: b.what, status: b.status === "queued" ? "queued" : "running", since: b.sinceMs ?? null }));
  if (data.crew.health === "online") {
    for (const r of data.crew.runs) {
      if (r.status !== "running" && r.status !== "awaiting_approval") continue;
      rows.push({ key: `c${r.id}`, kind: "crew", who: `Crew: ${r.name}`, what: r.status === "running" ? "Working on a task" : "Needs your OK", status: r.status === "running" ? "running" : "waiting", since: ms(r.started_at) });
    }
  }
  if (!down("list_terrence_actions")) {
    for (const a of data.actions) {
      if (a.toolId === "agent.hermes_chat") continue;
      const status: NowStatus | null = a.status === "awaiting_approval" ? "waiting" : a.status === "requested" ? "queued" : a.status === "planning" || a.status === "approved" || a.status === "executing" ? "running" : null;
      if (!status) continue;
      rows.push({ key: `a${a.id}`, kind: "action", who: "Amara", what: status === "waiting" ? `${actionLabel(a.toolId)}: needs your OK` : actionLabel(a.toolId), status, since: ms(a.startedAt) ?? ms(a.requestedAt) });
    }
  }
  for (const j of src.jobs ?? []) {
    if (j.status !== "running" && j.status !== "pending") continue;
    const routine = j.type === "custom_job" || ROUTINE_TYPES.has(j.type);
    rows.push({ key: `j${j.id}`, kind: routine ? "routine" : "amara", who: j.type === "custom_job" ? "Automation" : routine ? "Routine" : "Amara", what: j.name, status: j.status === "pending" ? "queued" : "running", since: j.startedAt });
  }
  if (!down("get_outbox_status") && data.outbox.queue_depth > 0) {
    rows.push({ key: "outbox", kind: "delivery", who: "Delivery", what: `${data.outbox.queue_depth} waiting to send`, status: "queued", since: null });
  }
  const rank: Record<NowStatus, number> = { waiting: 0, running: 1, queued: 2 };
  return rows.sort((a, b) => rank[a.status] - rank[b.status] || (a.since ?? Infinity) - (b.since ?? Infinity));
}

/** When the most recent thing finished, from the sources the snapshot holds. Null when none is known. */
export function lastFinishedAt(data: SystemSnapshot): number | null {
  const times: number[] = [];
  for (const a of data.activity) if (a.completedAt > 0) times.push(a.completedAt);
  for (const r of data.crew.runs) { const t = ms(r.finished_at); if (t != null) times.push(t); }
  for (const a of data.actions) { const t = ms(a.completedAt); if (t != null) times.push(t); }
  for (const r of data.automationRuns) { const t = ms(r.completedAt); if (t != null) times.push(t); }
  return times.length ? Math.max(...times) : null;
}

export function sinceText(t: number | null, now: number): string | null {
  if (t == null) return null;
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

const ICON: Record<NowRow["kind"], LucideIcon> = { amara: Bot, crew: Users, routine: Clock, service: Server, action: Zap, delivery: Mail };
const STATUS: Record<NowStatus, { kind: "run" | "wait" | "off"; text: string }> = { running: { kind: "run", text: "Running" }, waiting: { kind: "wait", text: "Waiting for you" }, queued: { kind: "off", text: "Queued" } };

function NowRowItem({ r, now }: { r: NowRow; now: number }) {
  const since = sinceText(r.since, now);
  const st = STATUS[r.status];
  return <ListRow icon={ICON[r.kind]} title={r.who} subtitle={[r.what, since ? `since ${since}` : null].filter(Boolean).join(" · ")} accessory={<StatusText kind={st.kind}>{st.text}</StatusText>} />;
}

/** Presentational feed. `now` is passed in so tests are deterministic. */
export function NowFeedView({ data, backend, jobs, now }: { data: SystemSnapshot; backend: AgentsNow | null; jobs: NowJob[] | null; now: number }) {
  if (!nowReadable(data, { backend, jobs })) {
    return <section className="d18-card" aria-label="Working now"><h2 className="d18-h">Working now</h2><p className="d18-note"><DataTag kind="wire" /></p></section>;
  }
  const rows = buildNowRows(data, { backend, jobs });
  if (!rows.length) {
    const last = lastFinishedAt(data);
    const when = sinceText(last, now);
    return (
      <section className="d18-card" aria-label="Working now">
        <h2 className="d18-h">Working now</h2>
        <p className="d18-note" data-testid="now-empty">Nobody is working right now.{when ? ` The last thing finished ${when}.` : ""}</p>
      </section>
    );
  }
  const shown = rows.slice(0, MAX_NOW_ROWS), rest = rows.slice(MAX_NOW_ROWS);
  return (
    <section aria-label="Working now" data-testid="now-feed">
      <InsetGroup title="Working now">
        {shown.map((r) => <NowRowItem key={r.key} r={r} now={now} />)}
      </InsetGroup>
      {rest.length > 0 && (
        <More label={`${rest.length} more`}>
          <InsetGroup>{rest.map((r) => <NowRowItem key={r.key} r={r} now={now} />)}</InsetGroup>
        </More>
      )}
    </section>
  );
}

/** Live wrapper: polls the backend every few seconds while the page is visible, and reads renderer-only jobs. */
export function NowFeed({ data }: { data: SystemSnapshot }) {
  const [backend, setBackend] = useState<AgentsNow | null>(null);
  const [jobs, setJobs] = useState<NowJob[] | null>(null);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      if (document.hidden) return;
      setNow(Date.now());
      try { const r = await invokeBound<AgentsNow>("agents_now"); if (alive) setBackend(r); } catch { if (alive) setBackend(null); }
      try {
        const { activityBus } = await import("../legacy-core/terrance/system/activityBus");
        if (alive) setJobs(activityBus.getActiveTasks());
      } catch { if (alive) setJobs(null); }
    };
    const onVis = () => { if (!document.hidden) void tick(); };
    void tick();
    const timer = window.setInterval(() => { void tick(); }, NOW_POLL_MS);
    document.addEventListener("visibilitychange", onVis);
    return () => { alive = false; window.clearInterval(timer); document.removeEventListener("visibilitychange", onVis); };
  }, []);
  return <NowFeedView data={data} backend={backend} jobs={jobs} now={now} />;
}
