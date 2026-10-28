import { Bot, Brain, ChevronRight, CircleDollarSign, Cpu, RefreshCw, Sparkles } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { getSchedulerStatus, type SchedulerStatus } from "@/terrance/system/scheduler";
import type { ConversationMessage } from "../app/conversation";
import { openClawBadge } from "../app/openclaw";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { ModuleId, SystemSnapshot, TerrenceAction } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Ring, StatTile, type Tone } from "../ui";
import { recoveryBand } from "./HealthScreen";
import "./TerranceScreen.css";

const DASH = "—";
const HOUR = 3_600_000;
// SchedulerStatus.next*At (Decision 3) wins; last + interval is only the fallback when they are null.
const CONSOLIDATION_MS = 6 * HOUR;
const REASONING_MS = 12 * HOUR;

type SchedulerView = Pick<SchedulerStatus, "running" | "lastConsolidation" | "lastReasoning"> & {
  nextConsolidationAt?: number | null;
  nextReasoningAt?: number | null;
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

const tokyo = (ms: number | string, opts: Intl.DateTimeFormatOptions) => new Date(ms).toLocaleString("en-GB", { timeZone: "Asia/Tokyo", ...opts });
const clock = (ms: number | string | null | undefined) => (ms == null || Number.isNaN(new Date(ms).getTime()) ? DASH : tokyo(ms, { hour: "2-digit", minute: "2-digit", hour12: false }));
const exact = (ms: number | null) => (ms == null ? undefined : tokyo(ms, { dateStyle: "medium", timeStyle: "short", hour12: false }));

export function greeting(now: number) {
  const hour = Number(tokyo(now, { hour: "2-digit", hour12: false }));
  return hour >= 5 && hour < 12 ? "Good morning" : hour >= 12 && hour < 18 ? "Good afternoon" : "Good evening";
}

const STOPPED: SchedulerView = { running: false, lastConsolidation: null, lastReasoning: null };
/** A scheduler read must never take Home down (it is renderer-side module state). */
function readScheduler(): SchedulerView {
  try { return getSchedulerStatus(); } catch { return STOPPED; }
}

function useSchedulerStatus(override?: SchedulerView): SchedulerView {
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
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, [fixed]);
  return now;
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

  const header = (
    <header className="home__header">
      <div>
        <h1>{greeting(now)}, Taisun</h1>
        <p className="home__date">{tokyo(now, { weekday: "long", day: "numeric", month: "long" })}</p>
      </div>
      <div className="home__header-badges">
        {unavailable.length > 0 && <Badge tone="warning" title={unavailable.map((s) => s.label).join(", ")}>{unavailable.length} source{unavailable.length === 1 ? "" : "s"} unavailable</Badge>}
        {!isLoading && (data.outbox.gateway_online || data.hermes.gatewayRunning
          ? <Badge tone="success">Gateway online</Badge>
          : <Badge tone="neutral">Gateway offline</Badge>)}
      </div>
    </header>
  );

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
  const nextConsolidation = relative(next.consolidation, now);
  const nextReasoning = relative(next.reasoning, now);

  let recoveryBody: ReactNode;
  if (down("get_noop_health_snapshot")) recoveryBody = <Unavailable label="NOOP health" onRetry={onRetry} />;
  else recoveryBody = (
    <div className="home__recovery">
      <Ring value={recovery} label="Recovery" color={band?.color} valueText={band ? `${recovery}%, ${band.word}` : undefined}>
        <span className="home__ring-value">{recovery == null ? DASH : `${Math.round(recovery)}%`}</span>
        {band && <span className="home__ring-band">{band.word}</span>}
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

      <div className="home__top">
        <Card title="Recovery & sleep" accessory={<OpenLink to="health" label="Open Fitness" onNavigate={onNavigate} />}>{recoveryBody}</Card>

        <Card title="Scheduler" accessory={scheduler.running ? <Badge tone="success">Running</Badge> : <Badge tone="danger">Stopped</Badge>}>
          <InsetGroup>
            <ListRow title={<span title={exact(next.consolidation)}>{nextConsolidation ? `Next: Consolidation ${nextConsolidation}` : "Next: Consolidation not scheduled yet"}</span>} />
            <ListRow title={<span title={exact(lastReasoning)}>{lastReasoning == null ? "Last: No reasoning run yet" : `Last: Reasoning ${clock(lastReasoning)}`}</span>} />
          </InsetGroup>
          {!scheduler.running && <p className="home__note" role="note">Background jobs aren&apos;t running.</p>}
        </Card>

        <Card title="Latest insight">
          {down("get_noop_health_snapshot")
            ? <Unavailable label="NOOP health" onRetry={onRetry} />
            : insight
              ? <div className="home__insight"><p className="home__insight-fact">{insight.fact}</p>{insight.context && <p className="home__note">{insight.context}</p>}</div>
              : <p className="home__note">Insights appear after NOOP&apos;s nightly sync.</p>}
          {onNavigate && <Button variant="plain" onClick={() => onNavigate("health")}>Open Fitness</Button>}
        </Card>
      </div>

      <Card title="Agents">
        {down("get_terrence_runtime_health")
          ? <Unavailable label="Terrence runtime" onRetry={onRetry} />
          : (
            <InsetGroup>
              <ListRow icon={Bot} title="Hermes" subtitle={[hermes.model, hermes.provider].filter(Boolean).join(" · ") || DASH}
                accessory={<Badge tone={hermesOnline ? "success" : "warning"}>{hermesOnline ? "Online" : hermes.status ? hermes.status.replace(/_/g, " ") : "Offline"}</Badge>}
                value={hermes.telegramStatus ? `Telegram ${hermes.telegramStatus}` : `Checked ${clock(hermes.checkedAt)}`} />
              <ListRow icon={Cpu} title="OpenClaw" subtitle={openClawBadge(data.openclaw).subtitle} accessory={<Badge tone={openClawBadge(data.openclaw).tone}>{openClawBadge(data.openclaw).label}</Badge>} />
            </InsetGroup>
          )}
      </Card>

      <Card title="Priorities" accessory={onNavigate && <Button variant="plain" size="small" onClick={() => onNavigate("chat")}>Open Chat</Button>}>
        {priorities.length === 0
          ? <p className="home__note">No current priorities</p>
          : (
            <InsetGroup>
              {priorities.map((p) => (
                <ListRow key={`${p.sourceKind}-${p.sourceId}`} icon={Sparkles} title={p.title} subtitle={`#${p.rank} · ${p.sourceKind}`}
                  accessory={<Badge tone={PRIORITY_TONE[p.sourceStatus] ?? "neutral"}>{p.sourceStatus.replace(/_/g, " ")}</Badge>} />
              ))}
            </InsetGroup>
          )}
      </Card>

      <div className="home__half">
        <Card title="Neuro" accessory={<OpenLink to="neuro" label="Open Neuro" onNavigate={onNavigate} />}>
          {muse
            ? <StatTile label="Last session" value={Math.round(muse.duration_secs / 60)} unit="min" />
            : <p className="home__note"><Brain className="home__inline-icon" aria-hidden="true" /> Your first completed session will appear here.</p>}
        </Card>
        <Card title="Revenue today" accessory={<OpenLink to="revenue" label="Open Revenue" onNavigate={onNavigate} />}>
          <div className="home__pair">
            <StatTile label="Today" value={`$${data.revenue.today_revenue.toFixed(0)}`} />
            <StatTile label="Net income" value={`$${data.revenue.net_income.toFixed(0)}`} />
          </div>
          <CircleDollarSign className="home__ghost-icon" aria-hidden="true" />
        </Card>
      </div>

      {scheduler.running && (nextConsolidation || nextReasoning) && (
        <footer className="home__footer">
          Scheduler{nextConsolidation ? ` · next consolidation ${nextConsolidation.replace(/^in /, "")}` : ""}{nextReasoning ? ` · next reasoning ${nextReasoning.replace(/^in /, "")}` : ""}
        </footer>
      )}
    </div>
  );
}
