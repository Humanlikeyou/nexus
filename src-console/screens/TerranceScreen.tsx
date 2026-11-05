import { Bot, Brain, ChevronRight, CircleDollarSign, Cpu, Library, RefreshCw } from "lucide-react";
import { HermesSubtitle } from "./agentLabels";
import { useEffect, useState, type ReactNode } from "react";
import { getSchedulerStatus, type SchedulerStatus } from "@/terrance/system/scheduler";
import type { ConversationMessage } from "../app/conversation";
import { ASSISTANT_TITLE, assistantBadge } from "../app/openclaw";
import { assistantView, kbView } from "../app/statusViews";
import { INITIAL_SNAPSHOT } from "../bridge";
import { dateTime, money, time } from "../lib/format";
import type { ModuleId, SystemSnapshot, TerrenceAction } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Ring, StatTile, StatusDot, type Tone, Line, Lines, More, PageHero } from "../ui";
import { recoveryBand } from "./HealthScreen";
import { GoalsCard } from "./GoalsCard";
import "./TerranceScreen.css";

const DASH = "—";
const HOUR = 3_600_000;
// SchedulerStatus.next*At (Decision 3) wins; last + interval is only the fallback when they are null.
const CONSOLIDATION_MS = 6 * HOUR;
const REASONING_MS = 12 * HOUR;

type SchedulerView = Pick<SchedulerStatus, "running" | "lastConsolidation" | "lastReasoning"> & {
  nextConsolidationAt?: number | null;
  nextReasoningAt?: number | null;
  nextDailySummaryAt?: number | null;
};

export function nextRuns(s: SchedulerView) {
  return {
    consolidation: s.nextConsolidationAt ?? (s.lastConsolidation != null ? s.lastConsolidation + CONSOLIDATION_MS : null),
    reasoning: s.nextReasoningAt ?? (s.lastReasoning != null ? s.lastReasoning + REASONING_MS : null),
  };
}

/** "in 2h 14m" / "now" / "3h 5m ago". */
export function relative(target: number | null, now: number) {
  if (target == null) return null;
  const diff = target - now;
  const abs = Math.abs(diff);
  const h = Math.floor(abs / HOUR);
  const m = Math.round((abs % HOUR) / 60_000);
  const span = h > 0 ? `${h}h ${m}m` : `${m}m`;
  if (abs < 60_000) return "now";
  return diff > 0 ? `in ${span}` : `${span} ago`;
}

/** A next-run time: "in 2h 14m", or "due now" once it has passed (an overdue job has not run yet;
 *  it is never "2744h ago"). */
export function nextIn(target: number | null, now: number) {
  if (target == null) return null;
  return target - now < 60_000 ? "due now" : relative(target, now);
}

const tokyo = (ms: number | string, opts: Intl.DateTimeFormatOptions) => new Date(ms).toLocaleString("en-GB", { timeZone: "Asia/Tokyo", ...opts });
const clock = (ms: number | string | null | undefined) => (ms == null || Number.isNaN(new Date(ms).getTime()) ? DASH : time(ms));
const DATE_WITH_YEAR = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" });
/** A past run time: "1:54 PM" today (Tokyo), "Jul 6, 1:54 PM" on another day, with the year when it differs.
 *  Live check 2026-10-29: "Last: Reasoning 13:54" was July 6 and read like today. */
export function lastRunLabel(ms: number, now: number) {
  const day = (t: number) => tokyo(t, { year: "numeric", month: "2-digit", day: "2-digit" });
  if (day(ms) === day(now)) return time(ms);
  const sameYear = tokyo(ms, { year: "numeric" }) === tokyo(now, { year: "numeric" });
  return sameYear ? dateTime(ms) : `${DATE_WITH_YEAR.format(ms)}, ${time(ms)}`;
}
const exact = (ms: number | null) => (ms == null ? undefined : dateTime(ms));

export function greeting(now: number) {
  const hour = Number(tokyo(now, { hour: "2-digit", hour12: false }));
  return hour >= 5 && hour < 12 ? "Good morning" : hour >= 12 && hour < 18 ? "Good afternoon" : "Good evening";
}

/** Home subline date, e.g. "Thursday, Oct 29" (Asia/Tokyo; Decision 13). */
export function homeDate(now: number) {
  return new Date(now).toLocaleDateString("en-US", { timeZone: "Asia/Tokyo", weekday: "long", month: "short", day: "numeric" });
}

export type HomeSummary = { kind: "actions" | "priorities" | "clear"; text: string };
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * Second Home subline (screen-specs › Home › Greeting): the first that applies of
 * actions awaiting approval, priorities being tracked, or "All clear". Returns null when
 * the sources it would read are unavailable, so Home never claims "All clear" on missing data.
 */
export function homeSummary(data: SystemSnapshot): HomeSummary | null {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const waiting = data.actions.filter((a) => a.status === "awaiting_approval").length;
  if (waiting > 0) return { kind: "actions", text: `Amara has ${plural(waiting, "action", "actions")} waiting for approval` };
  const tracking = data.prioritySnapshot.items.length;
  if (tracking > 0) return { kind: "priorities", text: `Amara is tracking ${plural(tracking, "priority", "priorities")} today` };
  if (down("list_terrence_actions") || down("get_current_terrence_priority_snapshot")) return null;
  return { kind: "clear", text: "All clear. Amara is standing by." };
}

const STOPPED: SchedulerView = { running: false, lastConsolidation: null, lastReasoning: null };
/** A scheduler read must never take Home down (it is renderer-side module state). */
function readScheduler(): SchedulerView {
  try { return getSchedulerStatus(); } catch { return STOPPED; }
}

export function useSchedulerStatus(override?: SchedulerView): SchedulerView {
  // An override wins outright (derived, not mirrored into state); otherwise poll every 30s.
  const [polled, setPolled] = useState<SchedulerView>(() => override ?? readScheduler());
  useEffect(() => {
    if (override) return;
    const id = window.setInterval(() => setPolled(readScheduler()), 30_000);
    return () => window.clearInterval(id);
  }, [override]);
  return override ?? polled;
}

function useNow(fixed?: number) {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed != null) return;
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, [fixed]);
  return now;
}

/** "6 min ago" / "3 h ago" / "2 d ago" for the status strip. */
function ago(ms: number, now: number) {
  const min = Math.max(0, Math.round((now - ms) / 60_000));
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  return h < 24 ? `${h} h ago` : `${Math.floor(h / 24)} d ago`;
}

export interface StripSegment {
  key: "agents" | "scheduler" | "noop";
  text: string;
  tone: Tone;
  /** Live and healthy: the dot pulses. */
  live: boolean;
  /** Exact time for the title attribute. */
  title?: string;
  to: ModuleId;
}

/** Home status strip (screen-specs › Command center feel): plain text segments, not Badges. */
/** A "ready" NOOP sync older than this shows the warning tone. */
export const NOOP_STALE_AFTER_MS = 2 * 60 * 60 * 1000;

export function statusStrip(data: SystemSnapshot, scheduler: SchedulerView, now: number): StripSegment[] {
  const amaraOnline = data.hermes.status === "online";
  const assistant = assistantBadge(data.openclaw);
  const agents: StripSegment = {
    key: "agents", to: "chat", tone: amaraOnline ? "success" : "danger", live: amaraOnline,
    text: `Amara ${amaraOnline ? "online" : "offline"}${assistant ? ` · ${ASSISTANT_TITLE} connected` : ""}`,
  };

  const runs = nextRuns(scheduler);
  const next = ([["Consolidation", runs.consolidation], ["Reasoning", runs.reasoning], ["Daily summary", scheduler.nextDailySummaryAt ?? null]] as [string, number | null][])
    .filter((r): r is [string, number] => r[1] != null)
    .sort((a, b) => a[1] - b[1])[0];
  const sched: StripSegment = !scheduler.running
    ? { key: "scheduler", to: "settings", tone: "danger", live: false, text: "Background jobs stopped" }
    : next
      ? { key: "scheduler", to: "settings", tone: "success", live: true, text: `Next: ${next[0]} ${nextIn(next[1], now)}`, title: dateTime(next[1]) }
      : { key: "scheduler", to: "settings", tone: "neutral", live: false, text: "Background jobs running" };

  const noop = data.noop;
  const noopDown = data.unavailableSources.some((s) => s.command === "get_noop_health_snapshot") || noop.status === "unavailable";
  const synced = noop.generatedAt ? Date.parse(noop.generatedAt) : NaN;
  const syncedText = Number.isNaN(synced) ? null : `synced ${ago(synced, now)}`;
  const health: StripSegment = noopDown
    ? { key: "noop", to: "health", tone: "danger", live: false, text: "NOOP unavailable" }
    : noop.status === "stale"
      ? { key: "noop", to: "health", tone: "warning", live: false, text: `NOOP stale${syncedText ? ` · ${syncedText}` : ""}`, title: dateTime(synced) }
      : noop.status === "ready"
        // NOOP is a nightly sync, not a live feed: never pulses; an old sync reads as a warning.
        ? { key: "noop", to: "health", tone: !Number.isNaN(synced) && now - synced > NOOP_STALE_AFTER_MS ? "warning" : "success", live: false, text: `NOOP ${syncedText ?? "synced"}`, title: dateTime(synced) }
        : { key: "noop", to: "health", tone: "neutral", live: false, text: noop.status === "waiting_for_nightly_data" ? "NOOP waiting for tonight's data" : "NOOP waiting for data" };
  return [agents, sched, health];
}

/** Owns its own 30s tick so NOOP staleness (and "synced N ago") re-evaluates without re-rendering the screen. */
function StatusStrip({ data, scheduler, fixedNow, onNavigate }: { data: SystemSnapshot; scheduler: SchedulerView; fixedNow?: number; onNavigate?: (id: ModuleId) => void }) {
  const now = useNow(fixedNow);
  const segments = statusStrip(data, scheduler, now);
  return (
    <div className="home__strip" role="group" aria-label="Status">
      {segments.map((s) => (
        <button key={s.key} type="button" className="home__strip-item" data-key={s.key} data-live={s.live || undefined}
          title={s.title} disabled={!onNavigate} onClick={() => onNavigate?.(s.to)}>
          <StatusDot tone={s.tone}>{s.text}</StatusDot>
        </button>
      ))}
    </div>
  );
}

const PRIORITY_TONE: Record<string, Tone> = { active: "accent", in_progress: "accent", blocked: "danger", attention: "warning", awaiting_approval: "warning", done: "success", completed: "success" };

export type TerranceFieldProps = {
  data: SystemSnapshot;
  /** Navigates to another screen (Home cards deep-link). Links are hidden without it. */
  onNavigate?: (id: ModuleId) => void;
  /** Re-runs the snapshot load. Retry buttons are hidden without it. */
  onRetry?: () => void;
  /** Overrides first-load detection (refreshedAt still equals INITIAL_SNAPSHOT's). */
  loading?: boolean;
  /** Test seams. */
  now?: number;
  scheduler?: SchedulerView;
  // Legacy chat props: the conversation moved to Chat & Agents. Still accepted so App keeps compiling.
  messages?: ConversationMessage[];
  sending?: boolean;
  onSend?: (message: string) => Promise<void>;
  onNewConversation?: () => void;
  onOpenContext?: () => void;
  onApproveAction?: (action: TerrenceAction) => void;
  onRetryAction?: (action: TerrenceAction) => void;
  onCancelAction?: (action: TerrenceAction) => void;
  busy?: boolean;
};

function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="home__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

/** Unavailable + Retry with a tone: fixed copy only, never backend error text. */
function StatusRetry({ tone, badge, text, onRetry }: { tone: Tone; badge: string | null; text: string; onRetry?: () => void }) {
  return (
    <div className="home__unavailable" data-tone={tone} role="status">
      <p>{badge && <Badge tone={tone}>{badge}</Badge>} {text}</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

function OpenLink({ to, label, onNavigate }: { to: ModuleId; label: string; onNavigate?: (id: ModuleId) => void }) {
  if (!onNavigate) return null;
  return <Button variant="plain" size="small" onClick={() => onNavigate(to)} aria-label={label}>Open<ChevronRight className="home__chevron" aria-hidden="true" /></Button>;
}

export function TerranceField({ data, onNavigate, onRetry, loading, now: fixedNow, scheduler: schedulerOverride }: TerranceFieldProps) {
  const now = useNow(fixedNow);
  const scheduler = useSchedulerStatus(schedulerOverride);
  const isLoading = loading ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt;
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const unavailable = data.unavailableSources;
  // Nothing to say until the first snapshot lands (no "All clear" flash while loading).
  const summary = isLoading ? null : homeSummary(data);

  const header = (
    <PageHero title="Command Center" statement={`${greeting(now)}, Taisun`}
      detail={<span className="home__summary">
        {summary && (summary.kind === "actions" && onNavigate
          ? <button type="button" className="home__summary-link" onClick={() => onNavigate("chat")}>{summary.text}</button>
          : <span>{summary.text}</span>)}
        {summary && " · "}
        <time dateTime={new Date(now).toLocaleDateString("en-CA", { timeZone: "Asia/Tokyo" })}>{homeDate(now)}</time>
      </span>} />
  );
  const sourcesNote = unavailable.length > 0 && <span className="home__status" data-tone="warning" title={unavailable.map((s) => s.label).join(", ")}><i className="home__dot" aria-hidden="true" />{unavailable.length} source{unavailable.length === 1 ? "" : "s"} unavailable</span>;

  if (isLoading) {
    return (
      <div className="home" data-state="loading" aria-busy="true" aria-label="Loading Home">
        {header}
        <div className="home__top">{["Recovery & sleep", "Scheduler", "Latest insight"].map((t) => <Card key={t} title={t}><span className="home__skel" /></Card>)}</div>
        <Card title="Agents"><span className="home__skel home__skel--short" /></Card>
        <Card title="Priorities"><span className="home__skel home__skel--short" /></Card>
      </div>
    );
  }

  const noop = data.noop;
  const night = noop.latestNight ?? noop.latestSleep;
  const recovery = noop.today?.recovery ?? null;
  const band = recoveryBand(recovery);
  const sleepMin = night?.durationMin ?? noop.today?.totalSleepMin ?? null;
  const hrv = night?.avgHrv ?? noop.today?.avgHrv ?? null;
  const next = nextRuns(scheduler);
  const lastReasoning = scheduler.lastReasoning;
  const insight = noop.insights[0];
  const priorities = data.prioritySnapshot.items.slice(0, 5);
  const muse = data.sessions.find((s) => s.device_type === "muse");
  const hermes = data.hermes;
  const hermesOnline = hermes.status === "online";
  const assistant = assistantView(data.openclaw, { loading: false, unavailable: down("read_openclaw_status") });
  const kb = kbView(data.kb, down("kb_status"));
  const nextConsolidation = nextIn(next.consolidation, now);
  const nextReasoning = nextIn(next.reasoning, now);

  let recoveryBody: ReactNode;
  if (down("get_noop_health_snapshot")) recoveryBody = <Unavailable label="NOOP health" onRetry={onRetry} />;
  else recoveryBody = (
    <div className="home__recovery">
      <Ring value={recovery} label="Recovery" color={band?.color} valueText={band ? `${recovery}%, ${band.word}` : undefined}>
        <span className="home__ring-value">{recovery == null ? DASH : `${Math.round(recovery)}%`}</span>
        {band && <span className="home__ring-band" data-band={band.word.toLowerCase()}>{band.word}</span>}
      </Ring>
      <div className="home__recovery-stats">
        <StatTile label="Sleep" value={sleepMin == null ? DASH : `${Math.floor(sleepMin / 60)}h ${Math.round(sleepMin % 60)}m`} />
        <StatTile label="HRV" value={hrv == null ? DASH : Math.round(hrv)} unit="ms" />
      </div>
      {recovery == null && <p className="home__note">No recovery yet{noop.message ? ` · ${noop.message}` : ""}</p>}
    </div>
  );

  return (
    <div className="home" data-state="ready">
      {header}

      {sourcesNote}
      <section className="home__first" aria-label="Recovery & sleep">
        {recoveryBody}
        <OpenLink to="health" label="Open Fitness" onNavigate={onNavigate} />
      </section>

      <Card title="Priorities" accessory={<OpenLink to="chat" label="Open Chat" onNavigate={onNavigate} />}>
        {priorities.length === 0
          ? <p className="home__note">No current priorities</p>
          : <Lines label="Priorities">{priorities.slice(0, 3).map((p) => <Line key={`${p.sourceKind}-${p.sourceId}`} title={p.title} trailing={<Badge tone={PRIORITY_TONE[p.sourceStatus] ?? "neutral"}>{p.sourceStatus.replace(/_/g, " ")}</Badge>} />)}</Lines>}
      </Card>

      <More label="Agents, schedule and everything else" id="home-more">
      {!isLoading && <StatusStrip data={data} scheduler={scheduler} fixedNow={fixedNow} onNavigate={onNavigate} />}
      <div className="home__top">
        <Card title="Scheduler" accessory={scheduler.running ? <Badge tone="success">Running</Badge> : undefined /* stopped is already stated by the status strip */}>
          <InsetGroup>
            <ListRow title={<span title={exact(next.consolidation)}>{nextConsolidation ? `Next: Consolidation ${nextConsolidation}` : "Next: Consolidation not scheduled yet"}</span>} />
            <ListRow title={<span title={exact(lastReasoning)}>{lastReasoning == null ? "Last: No reasoning run yet" : `Last: Reasoning ${lastRunLabel(lastReasoning, now)}`}</span>} />
          </InsetGroup>
          {!scheduler.running && <p className="home__note" role="note">Background jobs aren&apos;t running.</p>}
        </Card>

        <Card title="Latest insight" accessory={<OpenLink to="health" label="Open insights in Fitness" onNavigate={onNavigate} />}>
          {down("get_noop_health_snapshot")
            ? <Unavailable label="NOOP health" onRetry={onRetry} />
            : insight
              ? <div className="home__insight"><p className="home__insight-fact">{insight.fact}</p>{insight.context && <p className="home__note">{insight.context}</p>}</div>
              : <p className="home__note">Insights appear after NOOP&apos;s nightly sync.</p>}
        </Card>
      </div>

      <Card title="Agents" accessory={<OpenLink to="chat" label="Open Chat & Agents" onNavigate={onNavigate} />}>
        {down("get_terrence_runtime_health")
          ? <Unavailable label="Amara runtime" onRetry={onRetry} />
          : (
            <InsetGroup>
              <ListRow icon={Bot} title="Amara" subtitle={<HermesSubtitle model={hermes.model} />}
                accessory={<Badge tone={hermesOnline ? "success" : "warning"}>{hermesOnline ? "Online" : hermes.status ? hermes.status.replace(/_/g, " ") : "Offline"}</Badge>}
                value={hermes.telegramStatus ? `Telegram ${hermes.telegramStatus}` : `Checked ${clock(hermes.checkedAt)}`} />
              {assistant?.kind === "row" && <ListRow icon={Cpu} title={assistant.title} subtitle={assistant.subtitle} accessory={<Badge tone={assistant.tone} aria-label={`${assistant.title} status: ${assistant.label}`}>{assistant.label}</Badge>} />}
            </InsetGroup>
          )}
        {assistant?.kind === "unreachable" && <StatusRetry tone="warning" badge="Not reachable" text={assistant.message} onRetry={onRetry} />}
      </Card>

      <Card title="Knowledge base" accessory={<Badge tone={kb.tone} aria-label={`Knowledge base status: ${kb.label}`}>{kb.label}</Badge>}>
        {kb.retry
          ? <StatusRetry tone={kb.tone} badge={null} text={`Knowledge base: ${kb.body}`} onRetry={onRetry} />
          : <InsetGroup><ListRow icon={Library} title={kb.title ?? "Verba"} subtitle={kb.body} /></InsetGroup>}
      </Card>

      {priorities.length > 3 && (
        <InsetGroup title="More priorities">
          {priorities.slice(3).map((p) => (
            <ListRow key={`${p.sourceKind}-${p.sourceId}`} title={p.title} subtitle={`#${p.rank} · ${p.sourceKind}`}
              accessory={<Badge tone={PRIORITY_TONE[p.sourceStatus] ?? "neutral"}>{p.sourceStatus.replace(/_/g, " ")}</Badge>} />
          ))}
        </InsetGroup>
      )}

      <GoalsCard goals={data.goals} failed={down("read_goals")} onRetry={onRetry} />

      <div className="home__half">
        <Card title="Neuro" accessory={<OpenLink to="neuro" label="Open Neuro" onNavigate={onNavigate} />}>
          {muse
            ? (
              <div className="home__pair">
                <StatTile label="Last session" value={Math.round(muse.duration_secs / 60)} unit="min" />
                {/* Decision 5: sessions saved before calm_pct existed show "—". */}
                <StatTile label={<>Calm <Badge tone="neutral">Beta</Badge></>} value={muse.summary.calm_pct == null ? null : Math.round(muse.summary.calm_pct)} unit="%" />
              </div>
            )
            : <p className="home__note"><Brain className="home__inline-icon" aria-hidden="true" /> Your first completed session will appear here.</p>}
        </Card>
        <Card title="Revenue today" accessory={<OpenLink to="revenue" label="Open Revenue" onNavigate={onNavigate} />}>
          <div className="home__pair">
            <StatTile label="Today" value={money(data.revenue.today_revenue)} />
            <StatTile label="Net income" value={money(data.revenue.net_income)} />
          </div>
          <CircleDollarSign className="home__ghost-icon" aria-hidden="true" />
        </Card>
      </div>

      {scheduler.running && (nextConsolidation || nextReasoning) && (
        <footer className="home__footer">
          Scheduler{nextConsolidation ? ` · next consolidation ${nextConsolidation.replace(/^in /, "")}` : ""}{nextReasoning ? ` · next reasoning ${nextReasoning.replace(/^in /, "")}` : ""}
        </footer>
      )}
      </More>
    </div>
  );
}
