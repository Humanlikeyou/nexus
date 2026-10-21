import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  Activity,
  BookOpen,
  BrainCircuit,
  BriefcaseBusiness,
  Bluetooth,
  Bot,
  Check,
  ChevronDown,
  CircleDollarSign,
  Crosshair,
  HardDrive,
  HeartPulse,
  History,
  Pencil,
  Play,
  Radio,
  RefreshCw,
  RotateCcw,
  Route,
  Satellite,
  Send,
  Settings2,
  SlidersHorizontal,
  Square,
  SquarePen,
  Sparkles,
  Sun,
  Trash2,
  Unplug,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { INITIAL_SNAPSHOT, approveTerrenceAction, cancelTerrenceAction, connectMuse, correctTerrenceContextItem, deleteTerrenceContextItem, disconnectMuse, interactWithTerrence, listTerrenceContextItems, listTerrenceInteractions, loadSystemSnapshot, migrateLegacyTerrenceContext, retryTerrenceAction, saveMuseSession, scanMuse, startEegRecording, startEegStream, stopEegRecording, stopEegStream } from "./bridge";
import type { ModuleId, MuseDevice, NoopHealthInsight, RecordedSession, SystemSnapshot, TerrenceAction, TerrenceContextItem, TerrenceInteraction } from "./types";
import { BiometricReadout, CommandSurface, IntelligenceOverlay, SignalStream } from "./design/SystemPrimitives";
import { ActionButton, EmptyState, Metric, PageHeading, SectionHeading, SegmentedControl, StatusIndicator } from "./design/ProductUI";
import { useAppearance, type ThemePreference } from "./design/useAppearance";
import { useMuseTelemetry, type MuseTelemetry } from "./neuro/useMuseTelemetry";
import { useTraining } from "./neuro/useTraining";
import { SessionFinalizer } from "./neuro/sessionFinalizer";

const SYSTEM_GROUPS: Array<{ label: string; systems: Array<{ id: ModuleId; label: string; signal: string; icon: LucideIcon }> }> = [
  { label: "Nexus", systems: [
    { id: "terrance", label: "Overview", signal: "AI", icon: Bot },
    { id: "neuro", label: "Live", signal: "EEG", icon: Radio },
    { id: "sessions", label: "Sessions", signal: "HISTORY", icon: History },
    { id: "health", label: "Health", signal: "NOOP", icon: HeartPulse },
  ] },
  { label: "Direction", systems: [
    { id: "trajectory", label: "Trajectory", signal: "GOALS", icon: Route },
    { id: "business", label: "Business", signal: "WORK", icon: BriefcaseBusiness },
    { id: "revenue", label: "Revenue", signal: "ARM", icon: CircleDollarSign },
  ] },
  { label: "System", systems: [
    { id: "devices", label: "Devices", signal: "BLE", icon: Bluetooth },
    { id: "assistant", label: "Activity", signal: "LOG", icon: Activity },
    { id: "settings", label: "Settings", signal: "APP", icon: Settings2 },
  ] },
];

const SYSTEMS = SYSTEM_GROUPS.flatMap((group) => group.systems);

type Tone = "live" | "watch" | "fault" | "quiet";

type ConversationMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  status?: "failed";
  model?: string;
};

const CONVERSATION_KEY = "nexus.terrence.conversationId";
const HERMES_SESSION_KEY = "nexus.terrence.hermesSessionId";
const HERMES_SESSION_PROFILE_KEY = "nexus.terrence.hermesSessionProfile";
const NEXUS_RUNTIME_PROFILE = "nexus-managed-actions-v5";

function createConversationId() {
  return typeof crypto?.randomUUID === "function"
    ? `nexus-${crypto.randomUUID()}`
    : `nexus-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function displayAssistantMessage(content: string) {
  return content.replace(/^\u{256d}[^\n]*Hermes[^\n]*\r?\n/u, "").trim();
}

function actionPlan(action: TerrenceAction): string[] {
  if (action.toolId === "macos.workflow") {
    const steps = action.parameters.steps;
    if (!Array.isArray(steps)) return [];
    return steps.flatMap((step, index) => {
      if (!step || typeof step !== "object") return [];
      const values = step as Record<string, unknown>;
      const kind = typeof values.kind === "string" ? values.kind.replace(/_/g, " ") : "desktop step";
      const target = typeof values.url === "string" ? values.url
        : typeof values.application === "string" ? values.application
          : typeof values.path === "string" ? values.path
            : typeof values.from === "string" && typeof values.to === "string" ? `${values.from} to ${values.to}`
              : typeof values.from === "string" && typeof values.name === "string" ? `${values.from} to ${values.name}`
                : typeof values.name === "string" ? values.name
                  : typeof values.volume === "number" ? `${values.volume}%`
                    : typeof values.delta === "number" ? `${values.delta > 0 ? "+" : ""}${values.delta}%`
                      : typeof values.muted === "boolean" ? values.muted ? "on" : "off"
                        : "";
      return [`${index + 1}. ${kind}${target ? `: ${target}` : ""}`];
    });
  }
  if (action.toolId === "filesystem.organize_directory") {
    const moves = action.parameters.moves;
    if (!Array.isArray(moves)) return [];
    return moves.flatMap((move, index) => {
      if (!move || typeof move !== "object") return [];
      const values = move as Record<string, unknown>;
      const source = typeof values.from === "string" ? values.from.split("/").filter(Boolean).pop() : null;
      const category = typeof values.category === "string" ? values.category : "organized files";
      return source ? [`${index + 1}. Move ${source} to ${category}`] : [];
    });
  }
  return [];
}

function messagesFromInteractions(interactions: TerrenceInteraction[]): ConversationMessage[] {
  return interactions.flatMap((interaction) => {
    const messages: ConversationMessage[] = [{
      id: `${interaction.id}-user`,
      role: "user",
      content: interaction.userMessage,
      timestamp: interaction.startedAt,
    }];
    if (interaction.assistantMessage) {
      messages.push({
        id: `${interaction.id}-assistant`,
        role: "assistant",
        content: displayAssistantMessage(interaction.assistantMessage),
        timestamp: interaction.completedAt,
        model: interaction.model,
      });
    } else if (interaction.error) {
      messages.push({
        id: `${interaction.id}-failed`,
        role: "assistant",
        content: interaction.error,
        timestamp: interaction.completedAt,
        status: "failed",
      });
    }
    return messages;
  });
}

function tone(value: string | boolean | null | undefined): Tone {
  if (value === true) return "live";
  if (!value) return "fault";
  const text = String(value).toLowerCase();
  if (["healthy", "online", "active", "connected", "configured", "complete", "streaming", "enabled", "watching"].some((item) => text.includes(item))) return "live";
  if (["waiting", "degraded", "watch", "testing", "paused", "pending", "unknown", "proxy"].some((item) => text.includes(item))) return "watch";
  if (["offline", "error", "failed", "missing", "disconnected", "killed", "down"].some((item) => text.includes(item))) return "fault";
  return "quiet";
}

function humanize(value: string | null | undefined) {
  const text = String(value ?? "unknown").replace(/[_-]+/g, " ").trim();
  return text ? text.charAt(0).toUpperCase() + text.slice(1).toLowerCase() : "Unknown";
}

function plainText(value: string) {
  return value
    .replace(/^\s*[-*]\s+/, "")
    .replace(/^\s*\[[ x~>-]\]\s*/i, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
}

function State({ value, label }: { value: string | boolean | null | undefined; label?: string }) {
  const display = label ?? (typeof value === "boolean" ? (value ? "Online" : "Offline") : humanize(String(value ?? "unknown")));
  return <span className={`field-state field-state--${tone(value)}`}><i />{display}</span>;
}

function number(value: number | null | undefined, digits = 0) {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(digits) : "--";
}

function money(value: number) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(value || 0);
}

function date(value: string | number | null | undefined) {
  if (!value) return "No record";
  const parsed = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  if (Number.isNaN(parsed.getTime())) return String(value);
  return parsed.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function time(value: string | number | null | undefined) {
  if (!value) return "--";
  const parsed = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  if (Number.isNaN(parsed.getTime())) return "--";
  return parsed.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function formatMinutes(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return "--";
  const rounded = Math.round(value);
  if (rounded < 60) return `${rounded}m`;
  return `${Math.floor(rounded / 60)}h ${rounded % 60}m`;
}

function shortDuration(ms: number) {
  if (ms < 1000) return `${ms || 0}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms / 60_000)}m`;
}

function readableError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  if (message.toLowerCase().includes("bluetooth adapter")) return "Bluetooth is unavailable. Check that Bluetooth is on, then try again.";
  if (message.toLowerCase().includes("not found") || message.toLowerCase().includes("not in discovered")) return "That Muse is no longer available. Put it back in pairing mode and scan again.";
  if (message.toLowerCase().includes("permission")) return "Nexus needs Bluetooth permission in System Settings before it can connect.";
  return message.replace(/^Error:\s*/i, "") || "The device request could not be completed.";
}

function Trace({ values, labels, tint = "cyan", tall = false }: { values: number[]; labels?: string[]; tint?: "cyan" | "lime" | "amber" | "coral"; tall?: boolean }) {
  const source = values.length > 1 ? values : [0, 0];
  const min = Math.min(...source);
  const max = Math.max(...source);
  const range = max - min || 1;
  const width = 800;
  const height = tall ? 240 : 130;
  const pad = 14;
  const points = source.map((value, index) => `${(index / (source.length - 1)) * width},${height - pad - ((value - min) / range) * (height - pad * 2)}`).join(" ");
  return (
    <figure className={`field-trace field-trace--${tint} ${tall ? "is-tall" : ""}`}>
      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label="Live data trend">
        <path className="field-trace__grid" d={`M0 ${height * .25}H800M0 ${height * .5}H800M0 ${height * .75}H800M160 0V${height}M320 0V${height}M480 0V${height}M640 0V${height}`} />
        <polyline className="field-trace__line" points={points} />
        {source.map((value, index) => <circle key={index} cx={(index / (source.length - 1)) * width} cy={height - pad - ((value - min) / range) * (height - pad * 2)} r="2.5" />)}
      </svg>
      {labels && labels.length > 0 && <figcaption><span>{labels[0]}</span><span>{labels[Math.floor(labels.length / 2)]}</span><span>{labels[labels.length - 1]}</span></figcaption>}
    </figure>
  );
}

function TerranceField({
  data,
  messages,
  sending,
  onSend,
  onNewConversation,
  onOpenContext,
  onApproveAction,
  onRetryAction,
  onCancelAction,
  busy,
}: {
  data: SystemSnapshot;
  messages: ConversationMessage[];
  sending: boolean;
  onSend: (message: string) => Promise<void>;
  onNewConversation: () => void;
  onOpenContext: () => void;
  onApproveAction: (action: TerrenceAction) => void;
  onRetryAction: (action: TerrenceAction) => void;
  onCancelAction: (action: TerrenceAction) => void;
  busy: boolean;
}) {
  const [draft, setDraft] = useState("");
  const priorities = data.prioritySnapshot.items;
  const latestDecision = data.decisions[0];
  const latestAutomationRun = data.automationRuns[0];
  const latestAutomation = latestAutomationRun ? data.automations.find((automation) => automation.id === latestAutomationRun.automationId) : null;
  const automationState = latestAutomationRun?.deliveryStatus === "failed" ? "failed" : latestAutomationRun?.status;
  const activeProject = data.projects[0];
  const recentActionOutcome = data.context.recentObservations.find((item) => item.metadata?.source === "terrence_action_ledger");
  const macActions = data.actions.filter((action) => action.toolId !== "agent.hermes_chat");
  const visibleMacActions = [
    ...macActions.filter((action) => action.status === "awaiting_approval" || action.retryAllowed),
    ...macActions.filter((action) => action.status !== "awaiting_approval" && !action.retryAllowed),
  ].slice(0, 3);
  const unavailableSummary = data.unavailableSources.map((source) => source.label).join(", ");

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const message = draft.trim();
    if (!message || sending) return;
    setDraft("");
    await onSend(message);
  };

  return (
    <div className="system-field terrance-field">
      <section className="terrance-presence">
        <div className={`assistant-mark ${data.hermes.status === "online" ? "is-live" : ""}`}><Bot size={25} /></div>
        <div className="presence-copy">
          <span className="system-kicker">Nexus overview</span>
          <h1>Terrence</h1>
          <p>Your local operating partner and current focus.</p>
        </div>
        <div className="presence-status">
          <State value={data.hermes.status} label={data.hermes.status === "online" ? "Terrence ready" : "Terrence offline"} />
          <State value={data.muse.connected} label={data.muse.streaming ? "Muse streaming" : data.muse.connected ? "Muse connected" : "Muse disconnected"} />
          <State value={data.noop.status === "ready" || data.noop.status === "waiting_for_nightly_data"} label={data.noop.status === "ready" ? "WHOOP current" : data.noop.status === "waiting_for_nightly_data" ? "WHOOP syncing" : "WHOOP unavailable"} />
          <span>{data.muse.streaming ? "A neural session is active" : `${data.capabilities.length} capabilities available`}</span>
          {data.unavailableSources.length > 0 && <span className="availability-warning" title={unavailableSummary}>{data.unavailableSources.length} source{data.unavailableSources.length === 1 ? "" : "s"} unavailable</span>}
        </div>
      </section>

      <section className="terrance-thread">
        <div className="thread-stats">
          <div><span>Runtime</span><strong>Hermes</strong><State value={data.hermes.gatewayRunning} /></div>
          <div><span>Telegram</span><strong>{data.hermes.telegramStatus}</strong><State value={data.hermes.telegramStatus} /></div>
          <div><span>Provider</span><strong>{data.hermes.provider}</strong><State value="configured" /></div>
          <div><span>Checked</span><strong>{date(data.hermes.checkedAt)}</strong></div>
        </div>
      </section>

      <aside className="terrence-context" aria-label="Current focus">
        <header><Sparkles size={16} /><span>Current focus</span><strong>{priorities.length}</strong><button className="context-inspect-toggle" type="button" onClick={onOpenContext} title="Inspect Terrence context"><BookOpen size={15} /></button></header>
        <div className="focus-list">
          {priorities.length === 0 && <p className="field-empty">No current priorities are recorded.</p>}
          {priorities.map((priority) => <article key={`${priority.sourceKind}-${priority.sourceId}`}>
            <span>#{priority.rank}</span>
            <strong>{priority.title}</strong>
          </article>)}
        </div>
        {recentActionOutcome && <section className="context-observation"><span>Recent verified outcome</span><strong>{recentActionOutcome.title}</strong><p>{recentActionOutcome.content || "Terrence recorded a completed action."}</p></section>}
        {activeProject && <section className="context-project"><span>Active project</span><strong>{activeProject.title}</strong><p>{activeProject.nextAction || activeProject.currentState}</p></section>}
      </aside>

      <section className="terrence-conversation" aria-label="Conversation with Terrence">
        <header><span>Conversation</span><div className="conversation-header-meta"><State value={data.hermes.status} /><button className="conversation-new" type="button" onClick={onNewConversation} disabled={sending} title="Start a new conversation"><SquarePen size={15} /></button></div></header>
        <div className="conversation-stream" aria-live="polite">
          {messages.length === 0 && <p className="conversation-empty">Terrence is ready.</p>}
          {messages.map((message) => <article key={message.id} className={`conversation-message conversation-message--${message.role} ${message.status === "failed" ? "is-failed" : ""}`}>
            <header><span>{message.role === "user" ? "You" : "Terrence"}</span><time>{date(message.timestamp)}</time>{message.model && <small>{message.model}</small>}</header>
            <p>{message.content}</p>
          </article>)}
          {sending && <article className="conversation-message conversation-message--assistant is-pending"><header><span>Terrence</span><small>Working</small></header><p>Thinking through it.</p></article>}
        </div>
        <form className="conversation-compose" onSubmit={submit}>
          <label className="sr-only" htmlFor="terrence-message">Message Terrence</label>
          <textarea id="terrence-message" value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Message Terrence" disabled={sending} rows={2} />
          <button type="submit" disabled={sending || !draft.trim()} title="Send message to Terrence"><Send size={16} /></button>
        </form>
      </section>

      <aside className="execution-stream">
        <header><Activity size={14} /><span>Mac actions</span><strong>{macActions.length}</strong></header>
        <div>{visibleMacActions.map((action) => {
          const plan = actionPlan(action);
          return <article key={action.id}><i className={`tone-${tone(action.status)}`} /><time>{date(action.requestedAt)}</time><strong>{action.title}</strong><p>{action.error || action.result || action.summary}</p>{action.status === "awaiting_approval" && plan.length > 0 && <details className="action-preview"><summary>Review {plan.length} planned change{plan.length === 1 ? "" : "s"}</summary><ol>{plan.map((step) => <li key={step}>{step}</li>)}</ol></details>}<small>{action.status.replace(/_/g, " ")}</small>{action.status === "awaiting_approval" && <span className="action-controls"><button type="button" onClick={() => onApproveAction(action)} disabled={busy} title={`Approve ${action.title}`}><Check size={12} /></button><button type="button" onClick={() => onCancelAction(action)} disabled={busy} title={`Cancel ${action.title}`}><X size={12} /></button></span>}{action.retryAllowed && <span className="action-controls"><button type="button" onClick={() => onRetryAction(action)} disabled={busy} title={`Prepare a retry for ${action.title}`}><RotateCcw size={12} /></button></span>}</article>;
        })}</div>
        {macActions.length === 0 && <p className="field-empty">No Mac actions recorded</p>}
      </aside>

      <section className="automation-ribbon">
        <span className="ribbon-title">AUTOMATION</span>
        <div><State value={data.automations.length > 0 ? data.automations.some((automation) => automation.status === "attention") ? "attention" : "active" : data.hermes.activeSchedules > 0} /><strong>{data.automations.filter((automation) => automation.enabled).length || data.hermes.activeSchedules} active Hermes schedules</strong><span>Persistent gateway routines</span><time>{date(data.hermes.checkedAt)}</time></div>
        {latestAutomationRun && <details className="automation-memory"><summary><span>Last run</span><strong>{latestAutomation?.name || "Terrence automation"}</strong><State value={automationState} /></summary><p>{latestAutomationRun.error || latestAutomationRun.result || "Completed without a user-facing message."}</p><small>{latestAutomationRun.deliveryStatus === "delivered" ? `Delivered to ${latestAutomationRun.deliveryTarget || "the configured channel"}.` : latestAutomationRun.deliveryStatus === "suppressed" ? "Intentionally silent." : latestAutomationRun.deliveryStatus === "failed" ? "Delivery needs attention." : "No delivery was requested."}</small></details>}
        {activeProject && <details className="project-memory"><summary><span>Project state</span><strong>{activeProject.title}</strong><State value={activeProject.status} /></summary><p>{activeProject.currentState}</p>{activeProject.nextAction && <small>Next: {activeProject.nextAction}</small>}{activeProject.blockers.length > 0 && <small>Blocked by: {activeProject.blockers.join(" · ")}</small>}</details>}
        {latestDecision && <details className="decision-memory"><summary><span>Decision memory</span><strong>{latestDecision.question}</strong><State value={latestDecision.status} /></summary><p>{latestDecision.recommendation}</p>{latestDecision.uncertainty && <small>Uncertainty: {latestDecision.uncertainty}</small>}{latestDecision.outcome && <small>Outcome: {latestDecision.outcome}</small>}</details>}
      </section>
    </div>
  );
}

type ContextCorrectionDraft = {
  item: TerrenceContextItem;
  title: string;
  content: string;
  status: TerrenceContextItem["status"];
  confidence: string;
};

type ContextScope = "current" | "history";

function ContextInspector({ visible, onClose, onChanged }: { visible: boolean; onClose: () => void; onChanged: () => Promise<void> }) {
  const [items, setItems] = useState<TerrenceContextItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<ContextCorrectionDraft | null>(null);
  const [pendingDeletion, setPendingDeletion] = useState<string | null>(null);
  const [scope, setScope] = useState<ContextScope>("current");
  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setItems(await listTerrenceContextItems(scope === "current"
        ? { statuses: ["active", "blocked", "paused"], limit: 120 }
        : { statuses: ["historical", "superseded", "completed", "archived"], limit: 120 }));
    } catch {
      setError("Context is unavailable right now.");
    } finally {
      setLoading(false);
    }
  }, [scope]);

  useEffect(() => {
    if (!visible) {
      setEditing(null);
      setPendingDeletion(null);
      setScope("current");
      return;
    }
    void load();
  }, [visible, load]);

  if (!visible) return null;

  const startCorrection = (item: TerrenceContextItem) => {
    setPendingDeletion(null);
    setEditing({ item, title: item.title, content: item.content || "", status: item.status, confidence: String(item.confidence) });
  };

  const selectScope = (nextScope: ContextScope) => {
    setScope(nextScope);
    setEditing(null);
    setPendingDeletion(null);
  };

  const saveCorrection = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!editing || saving) return;
    const confidence = Number(editing.confidence);
    if (!Number.isFinite(confidence) || confidence < 0 || confidence > 1 || !editing.title.trim()) {
      setError("Enter a title and a confidence from 0 to 1.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await correctTerrenceContextItem({
        originalId: editing.item.id,
        replacement: {
          kind: editing.item.kind,
          title: editing.title.trim(),
          content: editing.content.trim() || null,
          status: editing.status,
          confidence,
          provenanceSource: "user_correction",
          provenanceRef: `context:${editing.item.id}`,
          occurredAt: editing.item.occurredAt,
          metadata: { ...editing.item.metadata, correctedFrom: editing.item.id },
        },
      });
      setEditing(null);
      await Promise.all([load(), onChanged()]);
    } catch {
      setError("The correction could not be saved.");
    } finally {
      setSaving(false);
    }
  };

  const hideItem = async (item: TerrenceContextItem) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await deleteTerrenceContextItem(item.id);
      setPendingDeletion(null);
      await Promise.all([load(), onChanged()]);
    } catch {
      setError("The context item could not be hidden.");
    } finally {
      setSaving(false);
    }
  };

  return <div className="context-inspector-backdrop">
    <section className="context-inspector" role="dialog" aria-modal="true" aria-label="Terrence context">
      <header><div><span className="system-kicker">Terrence context</span><h2>What Terrence knows</h2></div><button type="button" onClick={onClose} title="Close context"><X size={18} /></button></header>
      <div className={`context-inspector-body${editing ? " is-editing" : ""}`}>
        <div className="context-item-list" aria-live="polite">
          <div className="context-scope-tabs" role="tablist" aria-label="Terrence context view"><button type="button" role="tab" aria-selected={scope === "current"} className={scope === "current" ? "is-selected" : ""} onClick={() => selectScope("current")}>Current</button><button type="button" role="tab" aria-selected={scope === "history"} className={scope === "history" ? "is-selected" : ""} onClick={() => selectScope("history")}>History</button></div>
          {loading && <p className="context-empty">Loading context.</p>}
          {!loading && !error && items.length === 0 && <p className="context-empty">No {scope === "current" ? "current" : "historical"} context is recorded.</p>}
          {items.map((item) => <article key={item.id} className={editing?.item.id === item.id ? "is-editing" : ""}>
            <div className="context-item-heading"><strong>{item.title}</strong><span>{item.kind}</span></div>
            {item.content && <p>{item.content}</p>}
            <footer><span>{item.status} · {Math.round(item.confidence * 100)}% confidence</span><span>{item.provenanceSource}</span><time>{date(item.updatedAt)}</time></footer>
            {scope === "current" && <><div className="context-item-actions"><button type="button" onClick={() => startCorrection(item)} disabled={saving} title={`Correct ${item.title}`}><Pencil size={14} /></button><button type="button" onClick={() => setPendingDeletion(item.id)} disabled={saving} title={`Hide ${item.title}`}><Trash2 size={14} /></button></div>{pendingDeletion === item.id && <div className="context-delete-confirm"><span>Hide this from Terrence?</span><button type="button" onClick={() => void hideItem(item)} disabled={saving}>Hide</button><button type="button" onClick={() => setPendingDeletion(null)} disabled={saving} title="Cancel"><X size={14} /></button></div>}</>}
          </article>)}
          {error && <p className="context-inspector-error">{error}</p>}
        </div>
        {editing && <aside className="context-editor">
          <form onSubmit={(event) => void saveCorrection(event)}>
            <span className="system-kicker">Correcting {editing.item.kind}</span>
            <label>Title<input value={editing.title} onChange={(event) => setEditing((current) => current && { ...current, title: event.target.value })} maxLength={300} autoFocus /></label>
            <label>Details<textarea value={editing.content} onChange={(event) => setEditing((current) => current && { ...current, content: event.target.value })} maxLength={12000} rows={7} /></label>
            <div className="context-editor-row"><label>Status<select value={editing.status} onChange={(event) => setEditing((current) => current && { ...current, status: event.target.value as TerrenceContextItem["status"] })}><option value="active">Active</option><option value="blocked">Blocked</option><option value="paused">Paused</option><option value="completed">Completed</option><option value="archived">Archived</option><option value="historical">Historical</option></select></label><label>Confidence<input type="number" min="0" max="1" step="0.05" value={editing.confidence} onChange={(event) => setEditing((current) => current && { ...current, confidence: event.target.value })} /></label></div>
            <div className="context-editor-actions"><button type="button" onClick={() => setEditing(null)} disabled={saving}>Cancel</button><button type="submit" disabled={saving}><Check size={15} />Save correction</button></div>
          </form>
        </aside>}
      </div>
    </section>
  </div>;
}

function HealthField({ data }: { data: SystemSnapshot }) {
  const health = data.noop;
  const today = health.today;
  const history = health.recentDays;
  const recoveryTrend = history.flatMap((item) => item.recovery == null ? [] : [item.recovery]);
  const hrvTrend = history.flatMap((item) => item.avgHrv == null ? [] : [item.avgHrv]);
  const recoveryLabels = history.flatMap((item) => item.recovery == null ? [] : [item.day.slice(5)]);
  const hrvLabels = history.flatMap((item) => item.avgHrv == null ? [] : [item.day.slice(5)]);
  const overnight = health.latestNight ?? health.latestSleep;
  const overnightSleepMinutes = overnight?.durationMin ?? today?.totalSleepMin ?? null;
  const overnightHrv = overnight?.avgHrv ?? today?.avgHrv ?? null;
  const overnightRhr = overnight?.restingHr ?? today?.restingHr ?? null;
  const overnightEfficiency = overnight?.efficiency ?? today?.efficiency ?? null;
  const sleepHours = overnightSleepMinutes == null ? null : overnightSleepMinutes / 60;
  const healthReady = health.status === "ready";
  const awaitingNightlyData = health.status === "waiting_for_nightly_data";
  const canShowHistoricalSignals = health.status !== "unavailable";
  const sleepDurationLabel = overnightSleepMinutes == null ? "--" : `${Math.floor(overnightSleepMinutes / 60)}h ${Math.round(overnightSleepMinutes % 60)}m`;
  const sleepStages = [
    { label: "Light", minutes: overnight?.lightMin ?? 0, tone: "light" },
    { label: "Deep", minutes: overnight?.deepMin ?? 0, tone: "deep" },
    { label: "REM", minutes: overnight?.remMin ?? 0, tone: "rem" },
  ].filter((stage) => stage.minutes > 0);
  const fallbackInsights: NoopHealthInsight[] = [];

  if (overnightSleepMinutes != null) {
    fallbackInsights.push({
      metric: "latest_sleep",
      fact: `${sleepDurationLabel} of sleep`,
      context: overnight
        ? `NOOP recorded the latest night from ${time(overnight.startedAt)} to ${time(overnight.endedAt)}.`
        : "Taken from the latest completed NOOP daily record.",
    });
  }
  if (sleepStages.length > 0 && overnight) {
    const stageFacts = sleepStages.map((stage) => `${formatMinutes(stage.minutes)} ${stage.label.toLowerCase()}`);
    const context = [
      overnight.sessionCount > 1 ? `Combined across ${overnight.sessionCount} sleep sessions.` : "From last night's sleep session.",
      overnight.awakeMin && overnight.awakeMin > 0 ? `${formatMinutes(overnight.awakeMin)} was staged awake within the sleep window.` : null,
    ].filter(Boolean).join(" ");
    fallbackInsights.push({
      metric: "sleep_stages",
      fact: stageFacts.join(", "),
      context,
    });
  }
  if (overnightHrv != null || overnightRhr != null) {
    const readings = [
      overnightHrv == null ? null : `HRV ${number(overnightHrv)} ms`,
      overnightRhr == null ? null : `resting heart rate ${number(overnightRhr)} bpm`,
    ].filter(Boolean).join(", ");
    fallbackInsights.push({
      metric: "overnight_signals",
      fact: readings.charAt(0).toUpperCase() + readings.slice(1),
      context: "These are measured overnight signals from the latest NOOP sleep record.",
    });
  }
  if (health.battery) {
    fallbackInsights.push({
      metric: "whoop_battery",
      fact: `WHOOP battery is ${number(health.battery.percentage)}%`,
      context: `${health.battery.charging === true ? "Charging" : health.battery.charging === false ? "On strap" : "Charge state unavailable"}; last observed ${date(health.battery.observedAt)}.`,
    });
  }

  const visibleInsights = [...health.insights];
  for (const insight of fallbackInsights) {
    if (!visibleInsights.some((existing) => existing.metric === insight.metric)) visibleInsights.push(insight);
  }

  const secondarySignals = [
    { label: "RHR", value: number(overnightRhr), unit: "BPM" },
    today?.strain != null
      ? { label: "Strain", value: number(today.strain, 1), unit: "" }
      : { label: "Efficiency", value: number(overnightEfficiency == null ? null : overnightEfficiency * 100), unit: "%" },
    today?.respRateBpm != null
      ? { label: "Respiratory", value: number(today.respRateBpm, 1), unit: "RPM" }
      : { label: "Battery", value: number(health.battery?.percentage), unit: "%" },
    today?.spo2Pct != null
      ? { label: "SpO2", value: number(today.spo2Pct, 1), unit: "%" }
      : { label: "Data age", value: health.dataAgeDays == null ? "--" : String(health.dataAgeDays), unit: "D" },
  ];

  if (!canShowHistoricalSignals) {
    return (
      <div className="system-field health-field health-field--awaiting">
        <section className="health-presence">
          <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
          <div className="health-presence-status"><State value={health.status} /><span>Awaiting a NOOP record</span></div>
        </section>
        <section className="health-awaiting">
          <div className="health-awaiting-lead">
            <span className="system-kicker">LOCAL DATA STATUS</span>
            <h2>Awaiting the first daily record.</h2>
            <p>{health.message || "NOOP health data is not available yet."}</p>
          </div>
          <div className="health-awaiting-signals" aria-label="Health signals awaiting NOOP data">
            <div><span>Recovery</span><strong>--</strong><small>Nightly readiness</small></div>
            <div><span>HRV</span><strong>--<small>ms</small></strong><small>Overnight variability</small></div>
            <div><span>Sleep</span><strong>--<small>h</small></strong><small>Restorative sleep</small></div>
          </div>
          <div className="health-awaiting-readiness" aria-label="NOOP integration readiness">
            <article><span>Data access</span><strong>Local store missing</strong><p>Terrence has no NOOP daily record to read yet.</p></article>
            <article><span>Morning briefing</span><strong>Waiting for nightly data</strong><p>Amara stays quiet until a fresh sleep session is available.</p></article>
            <article><span>Battery watch</span><strong>Waiting for device state</strong><p>No WHOOP battery reading is available locally.</p></article>
          </div>
        </section>
      </div>
    );
  }

  if (awaitingNightlyData) {
    return (
      <div className="system-field health-field health-field--awaiting-nightly">
        <section className="health-presence">
          <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
          <div className="health-presence-status"><State value={health.status} /><span>Awaiting overnight sync</span></div>
        </section>
        <section className="health-sync-field">
          <div className="health-sync-lead">
            <span className="system-kicker">LIVE SIGNAL STATUS</span>
            <h2>NOOP is connected. The overnight picture is still forming.</h2>
            <p>{health.message}</p>
            <div className="health-sync-stamp"><span>Daily record</span><strong>{health.latestDay || "Awaiting date"}</strong><small>Strain {number(today?.strain, 1)} is the only complete signal so far.</small></div>
          </div>
          <div className="health-sync-readouts" aria-label="Nightly health signals awaiting synchronization">
            <div><span>Recovery</span><strong>--</strong><small>Score arrives after the sleep record closes.</small></div>
            <div><span>HRV</span><strong>--<small>ms</small></strong><small>Overnight variability is not available yet.</small></div>
            <div><span>Sleep</span><strong>--<small>h</small></strong><small>Waiting for a completed sleep session.</small></div>
          </div>
        </section>
        <section className="health-sync-rail" aria-label="NOOP health automation readiness">
          <article><span>Local source</span><strong>NOOP record is current</strong><p>Terrence is reading the local store directly. No second WHOOP source is active.</p></article>
          <article><span>Morning briefing</span><strong>Held until sleep sync</strong><p>Amara will only speak after a fresh overnight record has meaningful data.</p></article>
          <article><span>Battery watch</span><strong>{health.battery ? `${number(health.battery.percentage)}% on strap` : "Waiting for current telemetry"}</strong><p>{health.battery ? "A current battery reading is available through NOOP." : "Amara stays quiet until NOOP provides a fresh battery reading."}</p></article>
        </section>
      </div>
    );
  }

  return (
    <div className="system-field health-field health-field--ready">
      <section className="health-presence">
        <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
        <div className="health-presence-status"><State value={health.status} /><span>{awaitingNightlyData ? "Awaiting overnight sync" : overnight ? `Latest night ${overnight.day}` : health.latestDay ? `Latest daily record ${health.latestDay}` : "Awaiting a NOOP record"}</span></div>
      </section>

      <section className="health-arcs">
        <BiometricReadout value={today?.recovery} label="Recovery" tint="lime" />
        <BiometricReadout value={overnightHrv} max={120} label="HRV" unit="ms" />
        <BiometricReadout value={sleepHours} max={10} label="Sleep" unit="h" digits={1} tint="amber" />
      </section>

      <section className="health-trace">
        <header><span>Recovery history</span><State value={healthReady ? "current" : health.status} label={healthReady ? "NOOP VERIFIED" : awaitingNightlyData ? "NIGHTLY SYNC PENDING" : health.status.replace(/_/g, " ")} /></header>
        {canShowHistoricalSignals && recoveryTrend.length > 0
          ? <Trace values={recoveryTrend} labels={recoveryLabels} tall tint="lime" />
          : <div className="health-signal-empty">
              <strong>Recovery baseline is still forming.</strong>
              <span>Sleep, HRV, and resting heart rate remain current while NOOP gathers more complete daily records.</span>
              {overnight && <div className="health-night-window" aria-label="Latest NOOP overnight sleep window">
                <div><span>Sleep began</span><time dateTime={overnight.startedAt}>{time(overnight.startedAt)}</time></div>
                <div className="health-night-span"><span>{sleepDurationLabel} asleep</span><i /></div>
                <div><span>Woke</span><time dateTime={overnight.endedAt}>{time(overnight.endedAt)}</time></div>
              </div>}
            </div>}
        <div className="health-signal-metrics">{secondarySignals.map((signal) => <span key={signal.label}>{signal.label}<strong>{signal.value}{signal.unit && <small>{signal.unit}</small>}</strong></span>)}</div>
      </section>

      <aside className="health-insights">
        <header><HeartPulse size={17} /><span>Current health context</span></header>
        {visibleInsights.slice(0, 3).map((insight) => <article key={insight.metric}><span>{insight.metric.replace(/_/g, " ")}</span><strong>{insight.fact}</strong>{insight.context && <p>{insight.context}</p>}</article>)}
        {visibleInsights.length === 0 && health.message && <p className="health-empty">{health.message}</p>}
      </aside>

      <section className="health-baseline">
        <div><span>Last night</span><strong>{sleepDurationLabel}</strong></div>
        <div><span>Sleep efficiency</span><strong>{number(overnightEfficiency == null ? null : overnightEfficiency * 100)}<small>%</small></strong></div>
        <div className="health-battery-summary"><span>Battery</span><div><strong>{number(health.battery?.percentage)}<small>%</small></strong><State value={health.battery?.charging ?? "unknown"} label={health.battery?.charging === true ? "CHARGING" : health.battery?.charging === false ? "ON STRAP" : "UNKNOWN"} /></div></div>
        <div><span>Data age</span><strong>{health.dataAgeDays == null ? "--" : `${health.dataAgeDays}d`}</strong></div>
      </section>

      <section className="health-history">{sleepStages.length > 0
        ? <><header><span>Sleep stages</span><small>{overnight?.sessionCount === 1 ? "Last night" : `${overnight?.sessionCount} merged sessions`}</small></header><div className="health-stage-bar" aria-label={sleepStages.map((stage) => `${stage.label} ${formatMinutes(stage.minutes)}`).join(", ")}>{sleepStages.map((stage) => <i key={stage.label} className={`health-stage--${stage.tone}`} style={{ flexGrow: stage.minutes }} />)}</div><div className="health-stage-labels">{sleepStages.map((stage) => <span key={stage.label}><i className={`health-stage--${stage.tone}`} />{stage.label}<strong>{formatMinutes(stage.minutes)}</strong></span>)}</div></>
        : <><header><span>HRV history</span></header>{canShowHistoricalSignals && hrvTrend.length > 0
          ? <Trace values={hrvTrend} labels={hrvLabels} />
          : <p className="health-history-empty">HRV history will appear after NOOP records multiple complete daily values.</p>}</>}</section>
    </div>
  );
}

type MuseOperation = "scanning" | "connecting" | "starting" | "stopping" | "disconnecting" | null;

interface MuseScreenProps {
  data: SystemSnapshot;
  devices: MuseDevice[];
  telemetry: MuseTelemetry;
  operation: MuseOperation;
  error: string | null;
  sessionStartedAt: number | null;
  sessionEndedAt: number | null;
  now: Date;
  onScan: () => void;
  onConnect: (device: MuseDevice) => void;
  onDisconnect: () => void;
  onStart: () => void;
  onStop: () => void;
}

function museSignalState(data: SystemSnapshot, telemetry: MuseTelemetry) {
  if (!data.muse.streaming) return { tone: "neutral" as const, label: data.muse.connected ? "Ready" : "Disconnected" };
  if (!telemetry.lastPacketAt) return { tone: "warning" as const, label: "Acquiring signal" };
  if (Date.now() - telemetry.lastPacketAt > 2_500) return { tone: "danger" as const, label: "Signal interrupted" };
  return { tone: "success" as const, label: "Data flowing" };
}

function NeuroField({ data, devices, telemetry, operation, error, sessionStartedAt, sessionEndedAt, now, onScan, onConnect, onDisconnect, onStart, onStop, training }: MuseScreenProps & { training: ReturnType<typeof useTraining> }) {
  const sessions = data.sessions.filter((session) => session.device_type === "muse");
  const signal = museSignalState(data, telemetry);
  const elapsedSeconds = sessionStartedAt ? Math.max(0, Math.floor(((sessionEndedAt ?? now.getTime()) - sessionStartedAt) / 1000)) : 0;
  const elapsed = `${String(Math.floor(elapsedSeconds / 60)).padStart(2, "0")}:${String(elapsedSeconds % 60).padStart(2, "0")}`;
  const traceValues = telemetry.channels.TP9?.length ? telemetry.channels.TP9 : Object.values(telemetry.channels).find((values) => values.length) ?? [];
  const actions = data.muse.streaming || sessionStartedAt !== null
    ? <ActionButton icon={sessionEndedAt ? RefreshCw : Square} variant={sessionEndedAt ? "primary" : "danger"} onClick={onStop} disabled={operation !== null}>{sessionEndedAt ? "Retry save" : "Stop session"}</ActionButton>
    : data.muse.connected
      ? <><ActionButton icon={Play} variant="primary" onClick={onStart} disabled={operation !== null}>Start session</ActionButton><ActionButton icon={Unplug} onClick={onDisconnect} disabled={operation !== null}>Disconnect</ActionButton></>
      : <ActionButton icon={Bluetooth} variant="primary" onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Find Muse"}</ActionButton>;

  return <div className="system-field live-field">
    <PageHeading eyebrow="Neural interface" title="Live" description={data.muse.connected ? `${data.muse.device_name || "Muse headset"} is ${data.muse.streaming ? "recording" : "ready"}.` : "Connect a Muse headset for live EEG and neurofeedback sessions."} actions={actions} />
    <section className="training-strip" aria-label="Neurofeedback">
      <label htmlFor="training-protocol">Protocol<span className="protocol-select"><select id="training-protocol" value={training.protocolId} onChange={(event) => training.setProtocolId(event.target.value)} disabled={sessionStartedAt !== null || operation !== null}><option value="off">Recording only</option>{training.protocols.map((protocol) => <option key={protocol.id} value={protocol.id}>{protocol.name}</option>)}</select><ChevronDown size={16} aria-hidden="true" /></span></label>
      <div><span>Neurofeedback</span><StatusIndicator tone={training.state.phase === "held" ? "warning" : training.state.signal?.action === "reward" ? "success" : "neutral"}>{sessionStartedAt ? training.state.label : training.protocolId === "off" ? "Off" : "Ready to calibrate"}</StatusIndicator></div>
      {sessionStartedAt && training.state.phase === "calibrating" && <progress aria-label="Calibration" value={training.state.calibrationPercent} max={100} />}
      {training.state.phase === "training" && <><Metric label="Focus" value={number(training.state.focus)} /><Metric label="Relaxation" value={number(training.state.relaxation)} /></>}
    </section>
    {error && <div className="inline-alert inline-alert--danger" role="alert"><span>{error}</span><ActionButton variant="quiet" onClick={sessionStartedAt !== null ? onStop : onScan} disabled={operation !== null}>{sessionStartedAt !== null ? "Retry stop and save" : "Try again"}</ActionButton></div>}
    {sessionEndedAt !== null && <div className="inline-alert" role="status">Recording stopped at {elapsed}. Saving is pending; keep Nexus open and retry before starting another session.</div>}

    {!data.muse.connected ? <><div className="live-offline-layout">
        <EmptyState icon={Bluetooth} title={operation === "scanning" ? "Searching for Muse" : "No Muse connected"} detail={operation === "scanning" ? "Keep the headset nearby and in pairing mode. Scanning takes about five seconds." : "Put your headset in pairing mode, then let Nexus find it."} action={<ActionButton icon={RefreshCw} variant="primary" onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Search again"}</ActionButton>} />
        <section className="device-results" aria-live="polite">
          <SectionHeading title="Nearby headsets" detail={devices.length ? `${devices.length} Muse device${devices.length === 1 ? "" : "s"} found` : "Results appear here after a scan"} />
          {devices.map((device) => <article key={device.address}><div className="device-glyph"><BrainCircuit size={19} /></div><div><strong>{device.name}</strong><span>{device.rssi == null ? "Signal strength unavailable" : device.rssi > -65 ? "Strong signal" : device.rssi > -80 ? "Fair signal" : "Weak signal"}</span></div><ActionButton onClick={() => onConnect(device)} disabled={operation !== null}>{operation === "connecting" ? "Connecting..." : "Connect"}</ActionButton></article>)}
          {!devices.length && operation !== "scanning" && <p className="subtle-empty">No recent scan results.</p>}
        </section>
      </div><section className="live-readiness-strip" aria-label="Session readiness"><article><span>Muse</span><strong>Connection required</strong><p>Search for a nearby headset to begin.</p></article><article><span>WHOOP context</span><strong>{data.noop.status === "ready" ? "Current" : "Optional"}</strong><p>{data.noop.status === "ready" ? "Recovery context is ready for the session." : "A neural session can run without health context."}</p></article><article><span>Recording</span><strong>Local storage ready</strong><p>Completed sessions are saved on this Mac.</p></article></section></> : <div className="live-workspace">
      <section className="live-primary">
        <div className="live-status-line"><StatusIndicator tone={signal.tone}>{signal.label}</StatusIndicator><span>{data.muse.device_name || "Muse"}</span><span>{data.muse.streaming ? `${elapsed} elapsed` : "Session not started"}</span></div>
        <div className="live-readout">
          <div><span>Session</span><strong>{data.muse.streaming ? elapsed : "Ready"}</strong><p>{data.muse.streaming ? "Recording locally" : "Start when you are settled"}</p></div>
          <div><span>EEG samples</span><strong>{telemetry.sampleCount.toLocaleString()}</strong><p>{data.muse.streaming ? `${telemetry.packetCount.toLocaleString()} packets received` : "Waiting for a session"}</p></div>
          <div><span>WHOOP context</span><strong>{data.noop.today?.recovery == null ? "--" : `${number(data.noop.today.recovery)}%`}</strong><p>{data.noop.status === "ready" ? "Current recovery" : "No current recovery"}</p></div>
        </div>
        <section className="signal-chart">
          <SectionHeading title="EEG signal" detail={data.muse.streaming ? "TP9 · recent samples · visual update 4 Hz" : "Signal appears when the session starts"} />
          {traceValues.length > 1 ? <Trace values={traceValues.slice(-256)} tall /> : <div className="chart-empty"><Radio size={22} /><span>{data.muse.streaming ? "Acquiring the first signal packets." : "Start a session to view the live signal."}</span></div>}
        </section>
      </section>
      <aside className="live-inspector">
        <SectionHeading title="Frequency bands" detail="Relative power · 1–45 Hz" />
        <div className="band-list">{Object.entries(telemetry.bands).map(([band, value]) => <div key={band}><span>{band}</span><i><b style={{ width: `${Math.min(100, value)}%` }} /></i><strong>{data.muse.streaming && telemetry.sampleCount >= 256 ? `${value.toFixed(0)}%` : "--"}</strong></div>)}</div>
        <div className="inspector-note">Band power is a live signal summary, not a medical measurement.</div>
        <details className="diagnostics-disclosure"><summary>Signal diagnostics</summary><dl><div><dt>Last packet</dt><dd>{telemetry.lastPacketAt ? `${Math.max(0, Math.round((now.getTime() - telemetry.lastPacketAt) / 1000))}s ago` : "None"}</dd></div>{Object.entries(telemetry.channels).map(([channel, values]) => <div key={channel}><dt>{channel}</dt><dd>{values.length.toLocaleString()} buffered</dd></div>)}</dl></details>
      </aside>
    </div>}
    <section className="recent-session-rail"><SectionHeading title="Recent sessions" detail={`${sessions.length} recorded`} />{sessions.length ? <div>{sessions.slice(0, 4).map((session) => <article key={session.id}><time>{date(session.start)}</time><strong>{Math.max(1, Math.round(session.duration_secs / 60))} min</strong><span>{session.data_points.toLocaleString()} samples</span></article>)}</div> : <p className="subtle-empty">Your first completed Muse session will appear here.</p>}</section>
  </div>;
}

function SessionsField({ data }: { data: SystemSnapshot }) {
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

function DevicesField({ data, devices, operation, error, onScan, onConnect, onDisconnect }: Omit<MuseScreenProps, "telemetry" | "sessionStartedAt" | "sessionEndedAt" | "now" | "onStart" | "onStop">) {
  const noopReady = data.noop.status === "ready" || data.noop.status === "waiting_for_nightly_data" || data.noop.status === "stale";
  return <div className="system-field devices-field">
    <PageHeading eyebrow="Connections" title="Devices" description="Manage the sensors Nexus reads from and understand their current state." actions={<ActionButton icon={RefreshCw} onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Scan for Muse"}</ActionButton>} />
    {error && <div className="inline-alert inline-alert--danger" role="alert">{error}</div>}
    <section className="integration-list">
      <article className="integration-row">
        <div className="integration-icon"><BrainCircuit size={22} /></div>
        <div className="integration-main"><div><span>Muse</span><h2>{data.muse.device_name || "Muse headset"}</h2></div><p>{data.muse.connected ? data.muse.streaming ? "EEG data is flowing into an active session." : "Connected and ready for a live session." : "Pair a Muse headset for live EEG and session recording."}</p><div className="integration-capabilities">EEG · Frequency bands · Session recording · Neurofeedback</div></div>
        <div className="integration-state"><StatusIndicator tone={data.muse.streaming || data.muse.connected ? "success" : "neutral"}>{data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected"}</StatusIndicator><span>{data.muse.connected ? "Live connection" : "No headset selected"}</span></div>
        <div className="integration-actions">{data.muse.connected ? <ActionButton icon={Unplug} onClick={onDisconnect} disabled={operation !== null}>Disconnect</ActionButton> : <ActionButton icon={Bluetooth} variant="primary" onClick={onScan} disabled={operation !== null}>Find Muse</ActionButton>}</div>
      </article>
      <article className="integration-row">
        <div className="integration-icon"><HeartPulse size={22} /></div>
        <div className="integration-main"><div><span>WHOOP via NOOP</span><h2>{data.noop.deviceId || "WHOOP"}</h2></div><p>{noopReady ? "Nexus is reading your local NOOP health record. Pairing remains managed by NOOP." : data.noop.message || "No current NOOP record is available."}</p><div className="integration-capabilities">Recovery · HRV · Sleep · Strain · Battery</div></div>
        <div className="integration-state"><StatusIndicator tone={data.noop.status === "ready" ? "success" : data.noop.status === "stale" || data.noop.status === "waiting_for_nightly_data" ? "warning" : "neutral"}>{data.noop.status === "ready" ? "Current" : data.noop.status === "waiting_for_nightly_data" ? "Syncing" : data.noop.status === "stale" ? "Data stale" : "Unavailable"}</StatusIndicator><span>{data.noop.generatedAt ? `Updated ${date(data.noop.generatedAt)}` : "No local update"}</span></div>
        <div className="integration-actions"><span className="battery-readout">{data.noop.battery ? `${number(data.noop.battery.percentage)}% battery` : "Battery unavailable"}</span></div>
      </article>
    </section>
    {devices.length > 0 && !data.muse.connected && <section className="available-devices"><SectionHeading title="Available Muse headsets" detail={`${devices.length} found nearby`} /><div>{devices.map((device) => <article key={device.address}><div><strong>{device.name}</strong><span>{device.rssi == null ? "Signal unknown" : `${device.rssi} dBm`}</span></div><ActionButton onClick={() => onConnect(device)} disabled={operation !== null}>Connect</ActionButton></article>)}</div></section>}
    <details className="diagnostics-disclosure device-diagnostics"><summary>Advanced diagnostics</summary><dl><div><dt>Muse transport</dt><dd>{data.muse.connected ? "Bluetooth Low Energy connected" : "Bluetooth Low Energy idle"}</dd></div><div><dt>NOOP source</dt><dd>Read-only local store</dd></div><div><dt>NOOP latest day</dt><dd>{data.noop.latestDay || "No record"}</dd></div><div><dt>Unavailable sources</dt><dd>{data.unavailableSources.length ? data.unavailableSources.map((source) => source.label).join(", ") : "None"}</dd></div></dl></details>
  </div>;
}

function SettingsField({ data, theme, setTheme, reduceMotion, setReduceMotion }: { data: SystemSnapshot; theme: ThemePreference; setTheme: (theme: ThemePreference) => void; reduceMotion: boolean; setReduceMotion: (value: boolean) => void }) {
  return <div className="system-field settings-field">
    <PageHeading eyebrow="Nexus" title="Settings" description="Appearance and local application behavior." />
    <div className="settings-layout">
      <nav className="settings-index" aria-label="Settings sections"><a href="#appearance"><Sun size={15} />Appearance</a><a href="#general"><SlidersHorizontal size={15} />General</a><a href="#advanced"><HardDrive size={15} />Advanced</a></nav>
      <div className="settings-content">
        <section id="appearance" className="settings-section"><SectionHeading title="Appearance" detail="Choose how Nexus looks on this Mac." /><div className="setting-row"><div><strong>Theme</strong><p>Follow macOS or keep a consistent appearance.</p></div><SegmentedControl value={theme} label="Theme" onChange={setTheme} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} /></div><label className="setting-row setting-toggle"><div><strong>Reduce motion</strong><p>Minimize interface transitions and live state animation.</p></div><input type="checkbox" checked={reduceMotion} onChange={(event) => setReduceMotion(event.target.checked)} /><span aria-hidden="true" /></label></section>
        <section id="general" className="settings-section"><SectionHeading title="General" detail="Current application behavior." /><div className="setting-row"><div><strong>Automatic refresh</strong><p>Health, action, and runtime status refresh every 15 seconds.</p></div><StatusIndicator tone="success">On</StatusIndicator></div><div className="setting-row"><div><strong>Local-first data</strong><p>Sessions, context, and preferences stay in local Nexus stores.</p></div><StatusIndicator tone="success">Active</StatusIndicator></div></section>
        <section id="advanced" className="settings-section"><SectionHeading title="Advanced" detail="Runtime details for troubleshooting." /><details className="diagnostics-disclosure" open><summary>Diagnostics</summary><dl><div><dt>Application</dt><dd>Nexus 0.1.0</dd></div><div><dt>Runtime</dt><dd>{data.hermes.runtime} · {data.hermes.model}</dd></div><div><dt>Gateway</dt><dd>{data.hermes.gatewayRunning ? "Running" : "Offline"}</dd></div><div><dt>Muse</dt><dd>{data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected"}</dd></div><div><dt>NOOP health</dt><dd>{data.noop.status.replace(/_/g, " ")}</dd></div><div><dt>Last refresh</dt><dd>{date(data.refreshedAt)}</dd></div></dl></details></section>
      </div>
    </div>
  </div>;
}

function BusinessField({ data }: { data: SystemSnapshot }) {
  const states = ["active", "backlog", "waiting", "done"];
  const stages = ["ideation", "drafting", "review", "scheduled", "published"];
  return (
    <div className="system-field business-field">
      <section className="ops-heading"><span className="system-kicker">Work</span><h1>Business</h1><p>{data.tasks.length} tracked work items</p></section>
      <section className="ops-pulse">{states.map((state) => <div key={state}><span>{state}</span><strong>{data.tasks.filter((task) => task.status === state).length}</strong><i className={`tone-${tone(state)}`} /></div>)}</section>
      <section className="work-ledger"><header><span>Work item</span><span>Priority</span><span>Tags</span><span>Status</span></header>{data.tasks.length > 0 ? data.tasks.map((task, index) => <article key={`${task.title}-${index}`}><strong>{plainText(task.title)}</strong><span>{task.priority ? humanize(task.priority) : "Normal"}</span><span>{task.tags?.map(humanize).join(" · ") || "--"}</span><State value={task.status} /></article>) : <div className="workspace-empty"><BriefcaseBusiness size={24} aria-hidden="true" /><strong>No work items yet</strong><p>Tracked priorities will collect here as they are created.</p></div>}</section>
      <section className="production-arc"><header><span>Content production</span><strong>{data.pipeline.length}</strong></header>{data.pipeline.length > 0 ? <div>{stages.map((stage) => { const items = data.pipeline.filter((item) => item.stage === stage); return <section key={stage}><header><span>{humanize(stage)}</span><strong>{items.length}</strong></header>{items.slice(0, 5).map((item, index) => <article key={`${item.title}-${index}`}><strong>{plainText(item.title)}</strong><span>{humanize(item.content_type)}</span></article>)}</section>; })}</div> : <div className="workspace-empty workspace-empty--compact"><SquarePen size={24} aria-hidden="true" /><strong>Pipeline is clear</strong><p>Content moves through this view from idea to publication.</p></div>}</section>
    </div>
  );
}

function RevenueField({ data }: { data: SystemSnapshot }) {
  const deployed = data.risk.global_capital_limit ? data.risk.global_capital_deployed / data.risk.global_capital_limit * 100 : 0;
  const isAwaitingRevenue = [
    data.revenue.total_revenue,
    data.revenue.total_expenses,
    data.revenue.net_income,
    data.revenue.today_revenue,
    data.revenue.week_revenue,
    data.revenue.month_revenue,
  ].every((value) => value === 0) && data.revenue.by_strategy.length === 0;

  if (isAwaitingRevenue) {
    return (
      <div className="system-field revenue-field revenue-field--awaiting">
        <section className="revenue-heading"><span className="system-kicker">Revenue system</span><h1>ARM</h1><State value={data.risk.kill_switch_active ? "fault" : "healthy"} label={data.risk.kill_switch_active ? "Kill switch engaged" : "Risk envelope clear"} /></section>
        <section className="revenue-zero-state">
          <div className="revenue-zero-copy"><span className="system-kicker">REVENUE STATE</span><h2>Awaiting the first revenue event.</h2><p>There is no recorded revenue or expense activity in the system yet. The operating envelope is ready and will begin tracking when a real source reports in.</p></div>
          <div className="revenue-zero-metrics"><div><span>Revenue</span><strong>{money(data.revenue.total_revenue)}</strong></div><div><span>Net income</span><strong>{money(data.revenue.net_income)}</strong></div><div><span>Capital deployed</span><strong>{money(data.risk.global_capital_deployed)}<small> of {money(data.risk.global_capital_limit)}</small></strong></div><div><span>Active strategies</span><strong>{data.risk.active_strategies}</strong></div></div>
        </section>
        <aside className="revenue-zero-strategies"><header><span>Configured strategies</span><strong>{data.strategies.length}</strong></header>{data.strategies.length > 0 ? data.strategies.map((strategy) => <article key={strategy.id}><State value={strategy.status} /><strong>{strategy.name}</strong><span>{strategy.type.replace(/_/g, " ")}</span><small>{money(strategy.capital_limit)} capital limit</small></article>) : <p>No revenue strategy has been configured.</p>}</aside>
      </div>
    );
  }

  return (
    <div className="system-field revenue-field">
      <section className="revenue-heading"><span className="system-kicker">Revenue system</span><h1>ARM</h1><State value={data.risk.kill_switch_active ? "fault" : "healthy"} label={data.risk.kill_switch_active ? "Kill switch engaged" : "Risk envelope clear"} /></section>
      <section className="revenue-hero"><div><span>TOTAL REVENUE</span><strong>{money(data.revenue.total_revenue)}</strong></div><div><span>NET INCOME</span><strong>{money(data.revenue.net_income)}</strong></div></section>
      <section className="capital-vector"><Crosshair size={96} /><div><span>CAPITAL DEPLOYED</span><strong>{deployed.toFixed(0)}<small>%</small></strong><i><b style={{ width: `${Math.min(100, deployed)}%` }} /></i><p>{money(data.risk.global_capital_deployed)} of {money(data.risk.global_capital_limit)}</p></div></section>
      <section className="period-vector"><div><span>Today</span><strong>{money(data.revenue.today_revenue)}</strong></div><div><span>7 days</span><strong>{money(data.revenue.week_revenue)}</strong></div><div><span>30 days</span><strong>{money(data.revenue.month_revenue)}</strong></div><div><span>Expenses</span><strong>{money(data.revenue.total_expenses)}</strong></div></section>
      <section className="strategy-stream"><header><span>ACTIVE STRATEGY MATRIX</span><strong>{data.strategies.length}</strong></header>{data.strategies.map((strategy) => <article key={strategy.id}><State value={strategy.status} /><strong>{strategy.name}</strong><span>{strategy.type.replace(/_/g, " ")}</span><span>{money(strategy.capital_limit)} deployed</span><em>{money(strategy.total_revenue - strategy.total_expenses)}</em></article>)}</section>
      <aside className="revenue-mix"><header>REVENUE MIX</header>{data.revenue.by_strategy.map((item) => <div key={item.strategy_id}><span>{item.strategy_name}</span><i><b style={{ width: `${data.revenue.total_revenue ? item.revenue / data.revenue.total_revenue * 100 : 0}%` }} /></i><strong>{money(item.net)}</strong></div>)}</aside>
    </div>
  );
}

function ActivityField({ data }: { data: SystemSnapshot }) {
  return (
    <div className="system-field activity-field">
      <section className="activity-heading"><span className="system-kicker">History</span><h1>Activity</h1><p>{data.activity.length} recent operations</p></section>
      <section className="delivery-vector"><Satellite size={38} /><SignalStream active={data.outbox.gateway_online} /><div><strong>{data.outbox.queue_depth}</strong><span>QUEUED</span></div><div><strong>{data.outbox.total_delivered}</strong><span>DELIVERED</span></div></section>
      <section className="activity-river">{data.activity.length > 0 ? data.activity.map((item) => <article key={item.id}><time>{date(item.startedAt)}</time><i className={`tone-${tone(item.status)}`} /><strong>{plainText(item.name.replace(/_/g, " "))}<small>{humanize(item.type)}</small></strong><p>{item.detail || item.outcome || "No detail recorded"}</p><span>{shortDuration(item.durationMs)}</span><State value={item.status} /></article>) : <div className="workspace-empty"><Activity size={24} aria-hidden="true" /><strong>No recent operations</strong><p>Mac actions and assistant work will appear here as Nexus completes them.</p></div>}</section>
      <aside className="route-status"><span>DELIVERY ROUTE</span><State value={data.outbox.gateway_online} label={data.outbox.gateway_online ? "GATEWAY CLEAR" : "GATEWAY WAITING"} /><p>{data.outbox.last_error || "No transport errors"}</p><time>{date(data.outbox.last_delivered_at)}</time></aside>
    </div>
  );
}

function TrajectoryField({ data }: { data: SystemSnapshot }) {
  const goalProgress = data.goals.map((goal) => ({
    goal,
    progress: goal.target > 0 ? Math.min(100, goal.current / goal.target * 100) : 0,
  }));
  const portfolioProgress = goalProgress.length
    ? goalProgress.reduce((total, item) => total + item.progress, 0) / goalProgress.length
    : 0;
  const leadingGoal = [...goalProgress].sort((left, right) => right.progress - left.progress)[0];
  const latestNight = data.noop.latestNight;
  const hasHealthData = data.noop.status === "ready" && (data.noop.today != null || latestNight != null);
  const dailyMatchesNight = latestNight == null || data.noop.latestDay === latestNight.day;
  const readinessRecovery = dailyMatchesNight ? data.noop.today?.recovery : null;
  const readinessHrv = latestNight?.avgHrv ?? (dailyMatchesNight ? data.noop.today?.avgHrv : null);
  const readinessSleep = latestNight?.durationMin ?? (dailyMatchesNight ? data.noop.today?.totalSleepMin : null);

  return (
    <div className="system-field trajectory-field">
      <section className="trajectory-heading">
        <div><span className="system-kicker">Direction</span><h1>Trajectory</h1><p>Keep the work pointed at the life you are building.</p></div>
        <div className="trajectory-heading-status"><strong>{data.goals.length}</strong><span>active directions</span></div>
      </section>

      <section className="trajectory-summary" aria-label="Trajectory summary">
        <div><span>Portfolio progress</span><strong>{portfolioProgress.toFixed(0)}<small>%</small></strong><p>Across every active direction.</p></div>
        <div><span>Furthest along</span><strong>{leadingGoal ? leadingGoal.goal.title : "No goal selected"}</strong><p>{leadingGoal ? `${leadingGoal.progress.toFixed(0)}% of its current target` : "Add a direction when you are ready."}</p></div>
        <div><span>Readiness source</span><strong>{hasHealthData ? "NOOP connected" : "Awaiting NOOP data"}</strong><p>{hasHealthData ? `Latest record: ${latestNight?.day ?? data.noop.latestDay ?? "today"}` : "Pair or import in NOOP to bring daily recovery here."}</p></div>
      </section>

      <section className="trajectory-map">
        <header><span>Active directions</span><small>Progress toward the targets you set</small></header>
        {goalProgress.length > 0 ? goalProgress.map(({ goal, progress }) => <article key={goal.id}>
          <div className="trajectory-goal-title"><span>{goal.category}</span><strong>{goal.title}</strong></div>
          <div className="trajectory-progress"><i><b style={{ width: `${progress}%` }} /></i><small>{goal.current.toLocaleString()} / {goal.target.toLocaleString()} {goal.unit}</small></div>
          <em>{progress.toFixed(0)}%</em>
          <time>{goal.deadline ? `Target ${date(goal.deadline)}` : "No target date"}</time>
        </article>) : <p className="trajectory-empty">Add the directions you want your time and energy to compound toward.</p>}
      </section>

      <section className="wellness-vector">
        <header><span>Daily readiness</span><State value={hasHealthData ? "ready" : "waiting"} label={hasHealthData ? "LIVE DATA" : "WAITING FOR NOOP"} /></header>
        <p>{hasHealthData ? "A compact health signal alongside the work, not the center of it." : "Your daily health signal will appear here once NOOP has a local record."}</p>
        <div className="wellness-metrics">
          <div><span>Recovery</span><strong>{number(readinessRecovery)}<small>%</small></strong></div>
          <div><span>HRV</span><strong>{number(readinessHrv)}<small>ms</small></strong></div>
          <div><span>Sleep</span><strong>{number(readinessSleep == null ? null : readinessSleep / 60, 1)}<small>h</small></strong></div>
          <div><span>Strain</span><strong>{number(data.noop.today?.strain, 1)}</strong></div>
        </div>
      </section>
    </div>
  );
}

export default function App() {
  const [active, setActive] = useState<ModuleId>("terrance");
  const [data, setData] = useState<SystemSnapshot>(INITIAL_SNAPSHOT);
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const [now, setNow] = useState(new Date());
  const [museDevices, setMuseDevices] = useState<MuseDevice[]>([]);
  const [museOperation, setMuseOperation] = useState<MuseOperation>(null);
  const [museError, setMuseError] = useState<string | null>(null);
  const [sessionStartedAt, setSessionStartedAt] = useState<number | null>(null);
  const [recordingFile, setRecordingFile] = useState<string | null>(null);
  const [sessionEndedAt, setSessionEndedAt] = useState<number | null>(null);
  const sessionFinalizer = useRef<SessionFinalizer<{ session: RecordedSession & { file_path: string }; data: Record<string, unknown> }> | null>(null);
  const sessionOperation = useRef(false);
  const sessionStopTime = useRef<number | null>(null);
  const { telemetry, reset: resetTelemetry, readCounters } = useMuseTelemetry();
  const training = useTraining(telemetry, sessionStartedAt !== null && sessionEndedAt === null && data.muse.streaming);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  const [conversationId, setConversationId] = useState(() => {
    const existing = window.localStorage.getItem(CONVERSATION_KEY);
    if (existing) return existing;
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    return created;
  });
  const [hermesSessionId, setHermesSessionId] = useState<string | null>(() => (
    window.localStorage.getItem(HERMES_SESSION_PROFILE_KEY) === NEXUS_RUNTIME_PROFILE
      ? window.localStorage.getItem(HERMES_SESSION_KEY)
      : null
  ));
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [sending, setSending] = useState(false);

  const refresh = useCallback(async () => { setBusy(true); setData(await loadSystemSnapshot()); setBusy(false); }, []);
  useEffect(() => { refresh(); const refreshTimer = window.setInterval(refresh, 15_000); const clockTimer = window.setInterval(() => setNow(new Date()), 1_000); return () => { window.clearInterval(refreshTimer); window.clearInterval(clockTimer); }; }, [refresh]);
  useEffect(() => {
    let activeRequest = true;
    migrateLegacyTerrenceContext()
      .then(() => {
        if (activeRequest) return refresh();
      })
      .catch((error) => console.warn("[console] legacy context migration unavailable", error));
    return () => { activeRequest = false; };
  }, [refresh]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.metaKey || event.ctrlKey || event.altKey || !/^[1-9]$/.test(event.key)) return;
      const index = Number(event.key) - 1;
      if (index < 0 || index >= Math.min(9, SYSTEMS.length)) return;
      event.preventDefault();
      setActive(SYSTEMS[index].id);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  useEffect(() => {
    let activeRequest = true;
    listTerrenceInteractions(conversationId).then((interactions) => {
      if (!activeRequest) return;
      setMessages(messagesFromInteractions(interactions));
      const latestSession = [...interactions]
        .reverse()
        .find((interaction) => interaction.runtimeProfile === NEXUS_RUNTIME_PROFILE && interaction.hermesSessionId)
        ?.hermesSessionId;
      if (latestSession) {
        setHermesSessionId(latestSession);
        window.localStorage.setItem(HERMES_SESSION_KEY, latestSession);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE);
      }
    }).catch((error) => console.warn("[console] interaction history unavailable", error));
    return () => { activeRequest = false; };
  }, [conversationId]);

  const act = useCallback(async (name: string, action: () => Promise<unknown>) => { setBusy(true); setNotice(`${name} in progress`); try { await action(); setNotice(`${name} complete`); await refresh(); } catch { setBusy(false); setNotice(`${name} unavailable`); } window.setTimeout(() => setNotice(null), 3500); }, [refresh]);

  const scanForMuse = useCallback(async () => {
    setMuseOperation("scanning");
    setMuseError(null);
    try {
      const found = await scanMuse();
      setMuseDevices(found);
      setNotice(found.length ? `${found.length} Muse headset${found.length === 1 ? "" : "s"} found` : "No Muse headsets found");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
      await refresh();
    }
  }, [refresh]);

  const connectToMuse = useCallback(async (device: MuseDevice) => {
    if (sessionStartedAt !== null || sessionOperation.current) {
      setMuseError("Stop and save the current session in Live before connecting another headset.");
      return;
    }
    setMuseOperation("connecting");
    setMuseError(null);
    try {
      await connectMuse(device.address);
      setMuseDevices([]);
      setNotice(`${device.name} connected`);
      await refresh();
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, sessionStartedAt]);

  const persistMuseSession = useCallback(async () => {
    if (sessionStartedAt === null || !recordingFile) {
      await stopEegStream();
      await stopEegRecording();
      return false;
    }
    const finalizer = sessionFinalizer.current ??= new SessionFinalizer();
    await finalizer.run({
      stopStream: async () => {
        await stopEegStream();
        sessionStopTime.current = Date.now();
        setSessionEndedAt(sessionStopTime.current);
      },
      stopRecording: stopEegRecording,
      snapshot: () => {
        const endedAt = sessionStopTime.current ?? Date.now();
        const counters = readCounters();
        const trainingSummary = training.summary();
        const session: RecordedSession & { file_path: string } = {
          id: typeof crypto?.randomUUID === "function" ? crypto.randomUUID() : `muse-${sessionStartedAt}`,
          device_type: "muse",
          start: new Date(sessionStartedAt).toISOString(),
          end: new Date(endedAt).toISOString(),
          duration_secs: Math.max(1, Math.round((endedAt - sessionStartedAt) / 1000)),
          data_points: counters.sampleCount,
          file_path: recordingFile,
          summary: { avg_focus: trainingSummary?.avg_focus ?? null, avg_relaxation: trainingSummary?.avg_relaxation ?? null, avg_hr: null, max_hr: null, avg_hrv: null },
        };
        training.finish();
        return { session, data: {
          source: "live_muse",
          replay_file: recordingFile,
          packets: counters.packetCount,
          channel_samples: counters.channelSamples,
          neurofeedback: trainingSummary,
        } };
      },
      save: (payload) => saveMuseSession(payload.session, payload.data),
    });
    return true;
  }, [recordingFile, sessionStartedAt, readCounters, training.summary, training.finish]);

  const startMuseSession = useCallback(async () => {
    if (sessionOperation.current || sessionStartedAt !== null || sessionFinalizer.current) return;
    sessionOperation.current = true;
    setMuseOperation("starting");
    setMuseError(null);
    resetTelemetry();
    let file: string | null = null;
    try {
      file = await startEegRecording();
      await startEegStream();
      const startedAt = Date.now();
      setRecordingFile(file);
      sessionStopTime.current = null;
      setSessionStartedAt(startedAt);
      training.begin();
      setNotice("Muse session started");
      await refresh();
    } catch (error) {
      if (file) await stopEegRecording().catch(() => undefined);
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, resetTelemetry, training.begin, sessionStartedAt]);

  const stopMuseSession = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("stopping");
    setMuseError(null);
    try {
      const saved = await persistMuseSession();
      training.finish();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      setNotice(saved ? "Muse session saved" : "Muse stream stopped");
    } catch (error) {
      setMuseError(`Session could not be finalized. Keep Nexus open and retry. ${readableError(error)}`);
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [persistMuseSession, refresh, training.finish]);

  const disconnectFromMuse = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("disconnecting");
    setMuseError(null);
    try {
      if (data.muse.streaming || sessionStartedAt !== null) {
        await persistMuseSession();
      }
      await disconnectMuse();
      training.finish();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      resetTelemetry();
      setNotice("Muse disconnected");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [data.muse.streaming, sessionStartedAt, persistMuseSession, refresh, resetTelemetry, training.finish]);
  const sendToTerrence = useCallback(async (message: string) => {
    const requestId = `pending-${Date.now()}`;
    setMessages((current) => [...current, { id: `${requestId}-user`, role: "user", content: message, timestamp: new Date().toISOString() }]);
    setSending(true);
    try {
      const interaction = await interactWithTerrence({ message, conversationId, hermesSessionId });
      if (interaction.hermesSessionId) {
        setHermesSessionId(interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_KEY, interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, interaction.runtimeProfile);
      }
      setMessages((current) => [...current, ...messagesFromInteractions([interaction]).filter((entry) => entry.role === "assistant")]);
      await refresh();
    } catch (error) {
      setMessages((current) => [...current, {
        id: `${requestId}-failed`,
        role: "assistant",
        content: error instanceof Error ? error.message : String(error),
        timestamp: new Date().toISOString(),
        status: "failed",
      }]);
    } finally {
      setSending(false);
    }
  }, [conversationId, hermesSessionId, refresh]);
  const startNewConversation = useCallback(() => {
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    window.localStorage.removeItem(HERMES_SESSION_KEY);
    window.localStorage.removeItem(HERMES_SESSION_PROFILE_KEY);
    setHermesSessionId(null);
    setMessages([]);
    setConversationId(created);
  }, []);
  const selected = useMemo(() => SYSTEMS.find((system) => system.id === active)!, [active]);
  const view = active === "terrance" ? <TerranceField data={data} messages={messages} sending={sending} onSend={sendToTerrence} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onApproveAction={(action) => { void act(`Approving ${action.title}`, () => approveTerrenceAction(action.id)); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} />
    : active === "health" ? <HealthField data={data} />
    : active === "neuro" ? <NeuroField training={training} data={data} devices={museDevices} telemetry={telemetry} operation={museOperation} error={museError} sessionStartedAt={sessionStartedAt} sessionEndedAt={sessionEndedAt} now={now} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onStart={() => { void startMuseSession(); }} onStop={() => { void stopMuseSession(); }} />
    : active === "sessions" ? <SessionsField data={data} />
    : active === "devices" ? <DevicesField data={data} devices={museDevices} operation={museOperation} error={museError} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} />
    : active === "business" ? <BusinessField data={data} />
    : active === "revenue" ? <RevenueField data={data} />
    : active === "assistant" ? <ActivityField data={data} />
    : active === "trajectory" ? <TrajectoryField data={data} />
    : <SettingsField data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} />;

  return (
    <CommandSurface mode={active}>
      <IntelligenceOverlay>
        <header className="nexus-mark"><span className="nexus-symbol"><Zap size={16} /></span><div><strong>Nexus</strong><span>Local intelligence</span></div></header>
        <div className="field-clock"><span className="toolbar-location">{selected.label}</span><span>{now.toLocaleDateString([], { month: "short", day: "numeric" })}</span><strong>{now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</strong><button onClick={refresh} disabled={busy} aria-label="Refresh all systems" title="Refresh all systems"><RefreshCw size={15} /></button></div>
      </IntelligenceOverlay>

      <nav className="system-constellation" aria-label="Nexus navigation">
        {SYSTEM_GROUPS.map((group) => <section className="nav-group" key={group.label}><h2>{group.label}</h2>{group.systems.map((system) => { const Icon = system.icon; const index = SYSTEMS.findIndex((entry) => entry.id === system.id); return <button key={system.id} className={active === system.id ? "is-active" : ""} aria-current={active === system.id ? "page" : undefined} onClick={() => setActive(system.id)} title={`${system.label}${index < 9 ? ` (Command+${index + 1})` : ""}`}><i><Icon size={17} /></i><span>{system.label}</span><small>{system.signal}</small></button>; })}</section>)}
        <div className="sidebar-status"><StatusIndicator tone={data.hermes.status === "online" ? "success" : "danger"}>{data.hermes.status === "online" ? "Nexus ready" : "Nexus offline"}</StatusIndicator><span>{data.hermes.model}</span></div>
      </nav>

      <section className="active-field" aria-label={`${selected.label} system`}>{view}</section>
      <footer className="field-footer">
        <span>Local only</span><i className={data.hermes.status === "online" ? "is-live" : ""} />
        <span>{data.hermes.model}</span><span>{data.capabilities.length} capabilities</span>
        {data.unavailableSources.length > 0 && <span className="footer-unavailable" title={data.unavailableSources.map((source) => source.label).join(", ")}>{data.unavailableSources.length} unavailable</span>}
        <span className="footer-spacer" />
        <span>{busy ? "Refreshing" : `Updated ${time(data.refreshedAt)}`}</span>
      </footer>
      {notice && <div className="field-notice"><i />{notice}</div>}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
  );
}
