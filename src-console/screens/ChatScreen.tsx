import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { HermesSubtitle } from "./agentLabels";
import { Activity, Bot, Check, PanelRight, RotateCcw, Search, Send, SquarePen, Users, Workflow, X } from "lucide-react";
import { type ConversationMessage, actionPlan } from "../app/conversation";
import { ASSISTANT_TITLE, assistantBadge } from "../app/openclaw";
import { dateTime, time } from "../lib/format";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot, TerrenceAction } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, StatTile, TextField, type Tone } from "../ui";
import "./ChatScreen.css";

export interface ChatFieldProps {
  data: SystemSnapshot;
  messages: ConversationMessage[];
  sending: boolean;
  /** True until the first snapshot load lands (or history is loading). */
  loading?: boolean;
  /** Epoch ms of the last session auto-save (useIdleAutoSave). */
  lastSavedAt?: number | null;
  onSend: (message: string) => Promise<void>;
  onNewConversation: () => void;
  onOpenContext: () => void;
  onApproveAction: (action: TerrenceAction) => void;
  onRetryAction: (action: TerrenceAction) => void;
  onCancelAction: (action: TerrenceAction) => void;
  onRetrySources?: () => void;
  /** §10: opens Chat & Agents › Activity. */
  onOpenActivity?: () => void;
  /** Stage 5E: opens Chat & Agents › Crews. */
  onOpenCrews?: () => void;
  /** Pre-filled composer text (⌘K “Ask Amara”); remount via key to apply. */
  initialDraft?: string;
  busy?: boolean;
}

const ACTION_TONE: Record<TerrenceAction["status"], Tone> = {
  requested: "info", planning: "info", awaiting_approval: "warning", approved: "accent", executing: "accent",
  succeeded: "success", failed: "danger", cancelled: "neutral", needs_attention: "warning",
};
const ACTION_LABEL: Record<TerrenceAction["status"], string> = {
  requested: "Requested", planning: "Planning", awaiting_approval: "Needs approval", approved: "Approved", executing: "Running",
  succeeded: "Done", failed: "Failed", cancelled: "Cancelled", needs_attention: "Needs attention",
};
const GROUP_MS = 2 * 60_000;
const MAX_COMPOSER_LINES = 6;

/** Consecutive messages from the same sender within 2 min share one header (Messages.app). */
export function groupStarts(messages: ConversationMessage[]): boolean[] {
  return messages.map((m, i) => {
    const prev = messages[i - 1];
    if (!prev || prev.role !== m.role) return true;
    const gap = Date.parse(m.timestamp) - Date.parse(prev.timestamp);
    return !(Number.isFinite(gap) && gap >= 0 && gap <= GROUP_MS);
  });
}

function relative(ms: number, now: number): string {
  const min = Math.round((now - ms) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  return time(ms);
}

/** Crews row value: running count when the service is up, else its state. */
function crewsValue(crew: SystemSnapshot["crew"]): string {
  if (crew.health === "not_configured") return "Not configured";
  if (crew.health === "offline") return "Offline";
  const running = crew.runs.filter((r) => r.status === "running").length;
  return running > 0 ? `${running} running` : "0 running";
}

export function ChatField({
  data, messages, sending, loading: loadingProp, lastSavedAt = null, onSend, onNewConversation, onOpenContext, onOpenActivity, onOpenCrews, initialDraft = "",
  onApproveAction, onRetryAction, onCancelAction, onRetrySources, busy = false,
}: ChatFieldProps) {
  const [draft, setDraft] = useState(initialDraft);
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLOListElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  // Loading = first snapshot not in yet and no local transcript to show.
  const loading = (loadingProp ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt) && messages.length === 0;
  const hermes = data.hermes;
  const online = hermes.status === "online";
  const assistant = assistantBadge(data.openclaw);
  const actionsUnavailable = data.unavailableSources.some((s) => s.command === "list_terrence_actions");
  const hermesUnavailable = data.unavailableSources.some((s) => s.command === "get_terrence_runtime_health");

  const actions = useMemo(() => {
    const tools = data.actions.filter((a) => a.toolId !== "agent.hermes_chat");
    const first = (a: TerrenceAction) => a.status === "awaiting_approval" || a.retryAllowed;
    return [...tools.filter(first), ...tools.filter((a) => !first(a))].slice(0, 5);
  }, [data.actions]);
  const suggestions = data.prioritySnapshot.items.slice(0, 3).map((p) => p.title).filter(Boolean);
  const starts = useMemo(() => groupStarts(messages), [messages]);
  const startedAt = messages[0]?.timestamp;

  const agents = [
    { id: "hermes", icon: Bot, title: "Amara", subtitle: hermesUnavailable ? "Status unavailable" : <HermesSubtitle model={hermes.model} />, searchText: hermesUnavailable ? "Status unavailable" : `Hermes Agent ${hermes.model}`, tone: (online ? "success" : "warning") as Tone, label: online ? "Online" : hermesUnavailable ? "Unknown" : "Offline" },
    ...(assistant ? [{ id: "assistant", icon: Workflow, title: ASSISTANT_TITLE, subtitle: assistant.subtitle, searchText: String(assistant.subtitle), tone: assistant.tone, label: assistant.label }] : []),
  ].filter((a) => !query.trim() || `${a.title} ${a.searchText}`.toLowerCase().includes(query.trim().toLowerCase()));

  // Keep the newest message in view.
  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length, sending]);

  // Auto-grow the composer up to 6 lines.
  useEffect(() => {
    const el = composerRef.current;
    if (!el) return;
    el.style.height = "auto";
    const line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    el.style.height = `${Math.min(el.scrollHeight, line * MAX_COMPOSER_LINES + 16)}px`;
  }, [draft]);

  // ⌘N new chat, ⌘⌥I context.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey) return;
      if (!e.altKey && e.key.toLowerCase() === "n") { e.preventDefault(); onNewConversation(); }
      else if (e.altKey && (e.key.toLowerCase() === "i" || e.code === "KeyI")) { e.preventDefault(); onOpenContext(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onNewConversation, onOpenContext]);

  const send = async (text: string) => {
    const message = text.trim();
    if (!message || sending) return;
    setDraft("");
    await onSend(message);
  };
  const submit = (e: FormEvent<HTMLFormElement>) => { e.preventDefault(); void send(draft); };
  const onComposerKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(draft); }
  };
  const retry = (failed: ConversationMessage, index: number) => {
    const user = [...messages.slice(0, index)].reverse().find((m) => m.role === "user");
    void send(failed.role === "user" ? failed.content : user?.content ?? "");
  };

  return (
    <div className="chat-screen page-inset">
      <aside className="chat-screen__agents" aria-label="Agents and conversations">
        <TextField label="Search agents" hideLabel placeholder="Search" leadingIcon={Search} value={query} onChange={(e) => setQuery(e.target.value)} />
        <InsetGroup title="Agents">
          {agents.map((a) => <ListRow key={a.id} icon={a.icon} title={a.title} subtitle={a.subtitle} accessory={<Badge tone={a.tone}>{a.label}</Badge>} />)}
          {agents.length === 0 && <ListRow title="No matching agents" />}
          {onOpenCrews && <ListRow icon={Users} title="Crews" value={crewsValue(data.crew)} onSelect={onOpenCrews} />}
        </InsetGroup>
        <InsetGroup title="Conversation" footer="Earlier conversations will be listed here later.">
          <ListRow icon={SquarePen} title="Current" subtitle={startedAt ? `Started ${time(startedAt)}` : "No messages yet"} selected onSelect={() => composerRef.current?.focus()} />
        </InsetGroup>
        {onOpenActivity && (
          <InsetGroup title="History">
            <ListRow icon={Activity} title="Activity" value={String(data.activity.length)} onSelect={onOpenActivity} />
          </InsetGroup>
        )}
        <Button variant="plain" icon={SquarePen} onClick={onNewConversation} title="New chat (⌘N)">New Chat</Button>
      </aside>

      <section className="chat-screen__conversation" aria-labelledby="chat-title">
        <header className="chat-screen__header">
          <div className="chat-screen__heading">
            <h1 id="chat-title">Amara</h1>
            <Badge tone={online ? "success" : "warning"}>{online ? "Online" : "Offline"}</Badge>
            <span className="chat-screen__model">{hermes.model}</span>
            {/* eslint-disable-next-line react-hooks/purity -- the 'Saved …' label is read at render on purpose; it re-renders on every save */}
            {lastSavedAt != null && <span className="chat-screen__saved" title={dateTime(lastSavedAt)}>Saved {relative(lastSavedAt, Date.now())}</span>}
          </div>
          <div className="chat-screen__tools">
            <Button variant="plain" icon={SquarePen} iconOnly aria-label="New chat" title="New chat (⌘N)" onClick={onNewConversation} />
            <Button variant="plain" icon={PanelRight} iconOnly aria-label="Show context" title="Show context (⌘⌥I)" onClick={onOpenContext} />
          </div>
        </header>

        {loading ? (
          <div className="chat-screen__messages" role="status" aria-label="Loading">
            {[0, 1, 2].map((i) => <div key={i} className="chat-bubble chat-bubble--skeleton" data-role={i === 1 ? "user" : "assistant"} />)}
          </div>
        ) : (
          <ol ref={listRef} className="chat-screen__messages" aria-live="polite" aria-label="Messages">
            {messages.length === 0 && !sending && actions.length === 0 && !actionsUnavailable && (
              <li className="chat-screen__empty">
                <Card title="Ask Amara anything" headingLevel={2}>
                  <p>Amara knows your priorities, projects and recent activity.</p>
                  {suggestions.length > 0 && <div className="chat-screen__suggestions">{suggestions.map((s) => <Button key={s} size="small" onClick={() => void send(s)}>{s}</Button>)}</div>}
                </Card>
              </li>
            )}
            {messages.map((m, i) => (
              <li key={m.id} className="chat-message" data-role={m.role} data-start={starts[i] || undefined}>
                {starts[i] && <span className="chat-message__meta">{m.role === "user" ? "You" : "Amara"} · <time dateTime={m.timestamp}>{time(m.timestamp)}</time></span>}
                <div className="chat-bubble" data-role={m.role} data-failed={m.status === "failed" || undefined} title={m.role === "assistant" ? [m.model, time(m.timestamp)].filter(Boolean).join(" · ") : undefined}>
                  {m.content}
                </div>
                {m.status === "failed" && (
                  <div className="chat-message__failed"><Badge tone="danger">Failed</Badge><Button variant="plain" size="small" icon={RotateCcw} onClick={() => retry(m, i)} disabled={sending}>Retry</Button></div>
                )}
              </li>
            ))}
            {sending && (
              <li className="chat-message" data-role="assistant" data-start>
                <div className="chat-bubble chat-bubble--pending" data-role="assistant" role="status" aria-label="Amara is thinking">
                  <span className="chat-dots" aria-hidden="true"><i /><i /><i /></span>Thinking…
                </div>
              </li>
            )}
          </ol>
        )}

        {(actions.length > 0 || actionsUnavailable) && (
          <div className="chat-screen__actions" aria-label="Actions">
            {actionsUnavailable ? (
              <Card padding="md"><p>Action ledger unavailable</p>{onRetrySources && <Button size="small" onClick={onRetrySources}>Retry</Button>}</Card>
            ) : actions.map((a) => {
              const plan = a.status === "awaiting_approval" ? actionPlan(a) : [];
              return (
                <Card key={a.id} as="article" title={a.title} headingLevel={3} accessory={<Badge tone={ACTION_TONE[a.status]}>{ACTION_LABEL[a.status]}</Badge>}>
                  {a.summary && <p className="chat-action__summary">{a.summary}</p>}
                  {plan.length > 0 && <InsetGroup title="Plan">{plan.map((step, i) => <ListRow key={i} title={step} />)}</InsetGroup>}
                  <div className="chat-action__buttons">
                    {a.status === "awaiting_approval" && <>
                      <Button variant="primary" size="small" icon={Check} disabled={busy} onClick={() => onApproveAction(a)}>Approve</Button>
                      <Button size="small" icon={X} disabled={busy} onClick={() => onCancelAction(a)}>Cancel</Button>
                    </>}
                    {a.retryAllowed && <Button size="small" icon={RotateCcw} disabled={busy} onClick={() => onRetryAction(a)}>Retry</Button>}
                  </div>
                </Card>
              );
            })}
          </div>
        )}

        <form className="chat-screen__composer" onSubmit={submit}>
          <label htmlFor="chat-composer" className="ui-visually-hidden">Message Amara</label>
          <textarea
            id="chat-composer"
            ref={composerRef}
            rows={1}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onComposerKey}
            placeholder={online ? "Message Amara" : "Amara is offline — messages may fail"}
          />
          <Button type="submit" variant="primary" icon={Send} iconOnly aria-label="Send" disabled={sending || !draft.trim()} />
        </form>
      </section>

      <aside className="chat-screen__summary" aria-label="Focus">
        <StatTile label="Open actions" value={String(data.actions.filter((a) => a.status === "awaiting_approval").length)} />
        <InsetGroup title="Current focus">
          {data.prioritySnapshot.items.slice(0, 3).map((p) => <ListRow key={`${p.rank}-${p.title}`} title={p.title} subtitle={p.sourceStatus} />)}
          {data.prioritySnapshot.items.length === 0 && <ListRow title="No current priorities" />}
        </InsetGroup>
        {data.projects[0] && <InsetGroup title="Active project"><ListRow title={data.projects[0].title} subtitle={data.projects[0].nextAction ?? undefined} /></InsetGroup>}
      </aside>
    </div>
  );
}
