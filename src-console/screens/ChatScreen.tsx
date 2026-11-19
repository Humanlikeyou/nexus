import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { MODEL_LOADING_COPY } from "../app/modelLoading";
import { HermesSubtitle } from "./agentLabels";
import { Activity, Bot, Brain, Check, Maximize2, Mic, Minimize2, PanelRight, RotateCcw, Search, Send, SquarePen, Users, X } from "lucide-react";
import { HOME_COLLAPSED_MESSAGES, type HomeMessage, speakingIndex, withVoiceTurns } from "../app/homeThread";
import { type ConversationMessage, actionPlan } from "../app/conversation";
import { guardExternal, tierLabel, tierTitle } from "../app/amaraChat";
import { dateTime, time } from "../lib/format";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot, TerrenceAction, VoiceTurn } from "../types";
import { Badge, Button, Card, DataTag, InsetGroup, ListRow, More, StatTile, TextField, type Tone } from "../ui";
import { TakeEmber } from "../amaraTake/AmaraTakeCard";
import type { AskMoreDetail } from "../amaraTake/takeView";
import "../amaraTake/AmaraTakeCard.css";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import { useAmaraVoice } from "../hooks/useAmaraVoice";
import "./ChatScreen.css";
import "./HomeChat.css";

export interface ChatFieldProps {
  data: SystemSnapshot;
  messages: ConversationMessage[];
  sending: boolean;
  /** True until the first snapshot load lands (or history is loading). */
  loading?: boolean;
  /** Epoch ms of the last session auto-save (useIdleAutoSave). */
  lastSavedAt?: number | null;
  /** `deep`: the composer Deep toggle (sends /deep through the Amara Router). */
  onSend: (message: string, options?: { deep?: boolean }) => Promise<void>;
  onNewConversation: () => void;
  /** Stop waiting for the pending reply (it is ignored if it lands later). */
  onCancelPending?: () => void;
  /** Router answered 503 "model loading"; the reply is being retried automatically. */
  loadingModel?: boolean;
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
  /** Current conversation: resolved action lines from other conversations are hidden. */
  conversationId?: string | null;
  /** design-1110b A: "home" is the compact chat under the Home lens; "page" the legacy full page. */
  variant?: "page" | "home";
  /** Home only: Expand (the largest the chat gets) vs. the last few messages. */
  expanded?: boolean;
  onToggleExpanded?: () => void;
  /** Home only: spoken turns (voice_recent_turns), merged into the same thread. */
  voiceTurns?: VoiceTurn[];
  /** Home only: bumped by ⌘1; each new value focuses the composer. */
  focusSignal?: number;
  /** design-1110f "Ask more ›": the take this question comes from, shown above the composer until sent or dismissed. */
  askContext?: AskMoreDetail | null;
  onClearContext?: () => void;
  /** Needs you › "Open on Home": scroll to this pending action card and focus its Approve button. `n` re-fires the same id. */
  focusAction?: { id: string; n: number } | null;
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

/** Statuses that still need Taisun; only these render as prominent cards. */
const PROMINENT: ReadonlySet<TerrenceAction["status"]> = new Set(["awaiting_approval", "needs_attention"]);
const MAX_ACTIONS = 5;

export type ChatActionItem = { action: TerrenceAction; prominent: boolean };

/**
 * Actions shown in the thread. Pending ones (any conversation) are prominent cards; resolved or
 * cancelled ones show as compact inline lines, and only for the current conversation, so stale
 * ledger rows from old sessions never surface.
 */
export function chatActions(actions: TerrenceAction[], conversationId: string | null | undefined): ChatActionItem[] {
  return actions
    .filter((a) => a.toolId !== "agent.hermes_chat")
    .filter((a) => PROMINENT.has(a.status) || (conversationId != null && a.conversationId === conversationId))
    .map((action) => ({ action, prominent: PROMINENT.has(action.status) }))
    .sort((x, y) => Date.parse(y.action.requestedAt) - Date.parse(x.action.requestedAt))
    .slice(0, MAX_ACTIONS)
    .reverse();
}

type ThreadItem = { kind: "message"; index: number; at: number } | { kind: "action"; item: ChatActionItem; at: number };

/** Messages and action items interleaved by time (stable: messages keep their order). */
export function threadItems(messages: ConversationMessage[], items: ChatActionItem[]): ThreadItem[] {
  const out: ThreadItem[] = messages.map((m, index) => ({ kind: "message", index, at: Date.parse(m.timestamp) || 0 }));
  for (const item of items) {
    const at = Date.parse(item.action.requestedAt) || 0;
    let i = out.length;
    while (i > 0 && out[i - 1].at > at) i--;
    out.splice(i, 0, { kind: "action", item, at });
  }
  return out;
}

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

/** Pending reply bubble: elapsed seconds and an optional Cancel. */
export function ThinkingBubble({ onCancel, loadingModel = false }: { onCancel?: () => void; loadingModel?: boolean }) {
  const [started] = useState(() => Date.now());
  const [now, setNow] = useState(started);
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const seconds = Math.max(0, Math.floor((now - started) / 1000));
  const label = loadingModel ? MODEL_LOADING_COPY : "Thinking…";
  return (
    <li className="chat-message" data-role="assistant" data-start>
      <div className="chat-bubble chat-bubble--pending" data-role="assistant" role="status" aria-label={loadingModel ? MODEL_LOADING_COPY : "Amara is thinking"}>
        <span className="chat-dots" aria-hidden="true"><i /><i /><i /></span>{label}
        {seconds > 0 && <time className="chat-bubble__elapsed" dateTime={`PT${seconds}S`} aria-hidden="true"> {seconds} s</time>}
      </div>
      {onCancel && <Button variant="plain" size="small" onClick={onCancel} aria-label="Cancel waiting for Amara's reply">Cancel</Button>}
    </li>
  );
}

/** Header model: the router-reported model of Amara's latest reply; Hermes's configured model only when not routed. */
export function headerModel(messages: { role: string; servedModel?: string }[], hermesModel: string, routed: boolean): string {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === "assistant" && m.servedModel) return m.servedModel;
  }
  return routed ? "Amara Router" : hermesModel;
}

/** Text fields keep Home/End for the caret; the empty composer is the one exception (see the key handler). */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** A take's text goes into the user turn: injection-guard it, drop quote/newline/angle-bracket characters, cap it. */
export function takeForPrompt(text: string): string {
  const guarded = guardExternal(text).text;
  return guarded.replace(/[\u200B-\u200F\u2028-\u202F\u2060-\u206F\uFEFF]/g, "").replace(/["<>\r\n]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 400);
}

export function ChatField({
  data, messages, sending, loading: loadingProp, lastSavedAt = null, onSend, onNewConversation, onCancelPending, loadingModel = false, onOpenContext, onOpenActivity, onOpenCrews, initialDraft = "",
  onApproveAction, onRetryAction, onCancelAction, onRetrySources, busy = false, conversationId = null,
  variant = "page", expanded = false, onToggleExpanded, voiceTurns, focusSignal = 0, askContext = null, onClearContext, focusAction = null,
}: ChatFieldProps) {
  const home = variant === "home";
  const [draft, setDraft] = useState(initialDraft);
  const [query, setQuery] = useState("");
  const [deep, setDeep] = useState(false);
  const voice = useAmaraVoice();
  const listRef = useRef<HTMLOListElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  // Loading = first snapshot not in yet and no local transcript to show.
  const loading = (loadingProp ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt) && messages.length === 0;
  const hermes = data.hermes;
  const online = hermes.status === "online";
  const actionsUnavailable = data.unavailableSources.some((s) => s.command === "list_terrence_actions");
  const hermesUnavailable = data.unavailableSources.some((s) => s.command === "get_terrence_runtime_health");
  // Deep is the router's call; with Hermes still on Ollama directly nothing would read /deep.
  const deepAvailable = hermes.routedViaRouter === true;

  // design-1110b A2: on Home, voice and typed turns are one thread.
  const all: HomeMessage[] = useMemo(() => (home ? withVoiceTurns(messages, voiceTurns ?? []) : messages), [home, messages, voiceTurns]);
  const actions = useMemo(() => chatActions(data.actions, conversationId), [data.actions, conversationId]);
  const fullThread = useMemo(() => threadItems(all, actions), [all, actions]);
  // Collapsed Home chat: the last few messages (pending approvals always stay in view).
  const thread = useMemo(() => {
    if (!home || expanded) return fullThread;
    const keep = new Set(fullThread.filter((t) => t.kind === "message").slice(-HOME_COLLAPSED_MESSAGES).map((t) => t.kind === "message" ? t.index : -1));
    return fullThread.filter((t) => (t.kind === "message" ? keep.has(t.index) : t.item.prominent || keep.size === 0));
  }, [home, expanded, fullThread]);
  const speakingAt = speakingIndex(all, home && voice.state === "speaking");
  const renderAction = ({ action: a, prominent }: ChatActionItem) => {
    if (!prominent) {
      return (
        <li key={a.id} className="chat-action-line" data-status={a.status}>
          <span className="chat-action-line__title">{a.title}</span>
          <Badge tone={ACTION_TONE[a.status]}>{ACTION_LABEL[a.status]}</Badge>
          {a.retryAllowed && <Button variant="plain" size="small" icon={RotateCcw} disabled={busy} onClick={() => onRetryAction(a)} aria-label={`Retry ${a.title}`}>Retry</Button>}
        </li>
      );
    }
    const plan = a.status === "awaiting_approval" ? actionPlan(a) : [];
    return (
      <li key={a.id} className="chat-action" data-action-id={a.id}>
        <Card as="article" title={a.title} headingLevel={3} accessory={<Badge tone={ACTION_TONE[a.status]}>{ACTION_LABEL[a.status]}</Badge>}>
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
      </li>
    );
  };
  const suggestions = data.prioritySnapshot.items.slice(0, 3).map((p) => p.title).filter(Boolean);
  const starts = useMemo(() => groupStarts(all), [all]);
  const startedAt = messages[0]?.timestamp;

  const agents = [
    { id: "hermes", icon: Bot, title: "Amara", subtitle: hermesUnavailable ? "Status unavailable" : <HermesSubtitle model={hermes.model} />, searchText: hermesUnavailable ? "Status unavailable" : `Hermes Agent ${hermes.model}`, tone: (online ? "success" : "warning") as Tone, label: online ? "Online" : hermesUnavailable ? "Unknown" : "Offline" },
  ].filter((a) => !query.trim() || `${a.title} ${a.searchText}`.toLowerCase().includes(query.trim().toLowerCase()));

  // Keep the newest exchange in view: scroll to the bottom, but never past the latest user message.
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const users = el.querySelectorAll<HTMLElement>('li[data-role="user"]');
    const lastUser = users[users.length - 1];
    const bottom = el.scrollHeight - el.clientHeight;
    el.scrollTop = lastUser ? Math.min(bottom, Math.max(0, lastUser.offsetTop - el.offsetTop - 16)) : bottom;
  }, [all.length, sending, expanded]);

  // Needs you › Open on Home: the card is in the thread (pending ones always are); bring it into view and put focus on the card (never on Approve).
  useEffect(() => {
    if (!focusAction) return;
    let frame = 0, tries = 0;
    const go = () => {
      const li = Array.from(document.querySelectorAll<HTMLElement>("li.chat-action[data-action-id]")).find((el) => el.dataset.actionId === focusAction.id);
      if (!li) { if (tries++ < 60) frame = requestAnimationFrame(go); return; }
      li.scrollIntoView?.({ block: "center" });
      // The card, not Approve: a stray Enter must not approve anything.
      li.tabIndex = -1;
      li.focus();
    };
    frame = requestAnimationFrame(go);
    return () => cancelAnimationFrame(frame);
  }, [focusAction, actions.length]);

  // ⌘1 (design-1110b A): focus the Home composer. 0 = not asked yet.
  useEffect(() => { if (home && focusSignal > 0) composerRef.current?.focus(); }, [home, focusSignal]);

  // Home → top of the conversation, End → bottom. The composer keeps focus after send, so it scrolls too while empty.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if ((e.key !== "Home" && e.key !== "End") || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.defaultPrevented) return;
      const composer = composerRef.current;
      if (isTypingTarget(e.target) && !(e.target === composer && !composer?.value)) return;
      const el = listRef.current;
      if (!el) return;
      e.preventDefault();
      el.scrollTop = e.key === "Home" ? 0 : el.scrollHeight;
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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
    // A live take's numbers go with the question; sample numbers never reach Amara as if they were real.
    const takeText = askContext && !askContext.sample && askContext.text ? takeForPrompt(askContext.text) : "";
    const context = askContext && !askContext.sample ? `\n\n(From Amara's take on ${askContext.from}${askContext.takeId ? ` [${askContext.takeId}]` : ""}${takeText ? `: "${takeText}"` : ""}${askContext.chips.length ? `. Numbers: ${askContext.chips.map((c) => `${c.label} ${c.value}`).join(", ")}` : ""})` : "";
    if (askContext) onClearContext?.();
    await onSend(message + context, { deep: deep && deepAvailable });
  };
  const submit = (e: FormEvent<HTMLFormElement>) => { e.preventDefault(); void send(draft); };
  const onComposerKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(draft); }
  };
  const retry = (failed: ConversationMessage, index: number) => {
    const user = [...all.slice(0, index)].reverse().find((m) => m.role === "user");
    void send(failed.role === "user" ? failed.content : user?.content ?? "");
  };

  const homeHeader = home && (
    <header className="hchat__head">
      {expanded && <AmaraLens state={voice.state} size={44} decorative className="hchat__lens" />}
      <h2 id="chat-title" className="hchat__title">Amara</h2>
      <span className="hchat__status"><i data-online={online || undefined} />{online ? "Online" : "Offline"} · <span className="chat-screen__model" title={`Hermes configured model: ${hermes.model}`}>{headerModel(messages, hermes.model, deepAvailable)}</span></span>
      <div className="hchat__tools">
        <Button variant="plain" size="small" icon={SquarePen} iconOnly aria-label="New chat" title="New chat (⌘N)" onClick={onNewConversation} />
        {onToggleExpanded && <Button variant="secondary" size="small" icon={expanded ? Minimize2 : Maximize2} aria-expanded={expanded} aria-controls="home-chat-log" onClick={onToggleExpanded}>{expanded ? "Collapse" : "Expand"}</Button>}
      </div>
    </header>
  );

  return (
    <div className={home ? "hchat" : "chat-screen page-inset"} data-state={home ? (expanded ? "expanded" : "collapsed") : undefined} data-speaking={speakingAt >= 0 || undefined}>
      <section className="chat-screen__conversation" aria-labelledby="chat-title">
        {home ? homeHeader : <>
        <header className="chat-screen__header">
          <div className="chat-screen__heading">
            {voice.state === "speaking" && <AmaraLens state="speaking" size={28} className="chat-screen__lens" />}
            <h1 id="chat-title">Amara</h1>
            <Badge tone={online ? "success" : "warning"}>{online ? "Online" : "Offline"}</Badge>
            <span className="chat-screen__model" title={`Hermes configured model: ${hermes.model}`}>{headerModel(messages, hermes.model, deepAvailable)}</span>
            {/* eslint-disable-next-line react-hooks/purity -- the 'Saved …' label is read at render on purpose; it re-renders on every save */}
            {lastSavedAt != null && <span className="chat-screen__saved" title={dateTime(lastSavedAt)}>Saved {relative(lastSavedAt, Date.now())}</span>}
          </div>
          <div className="chat-screen__tools">
            <Button variant="plain" icon={SquarePen} iconOnly aria-label="New chat" title="New chat (⌘N)" onClick={onNewConversation} />
            <Button variant="plain" icon={PanelRight} iconOnly aria-label="Show context" title="Show context (⌘⌥I)" onClick={onOpenContext} />
          </div>
        </header>
        <More label="Agents and focus">
          <div className="chat-screen__side">
      <aside className="chat-screen__agents" aria-label="Agents and conversations">
        <TextField label="Search agents" hideLabel placeholder="Search" leadingIcon={Search} value={query} onChange={(e) => setQuery(e.target.value)} />
        <InsetGroup title="Agents">
          {agents.map((a) => <ListRow key={a.id} icon={a.icon} title={a.title} subtitle={a.subtitle} accessory={<Badge tone={a.tone} aria-label={`${a.title} status: ${a.label}`}>{a.label}</Badge>} />)}
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
        <Button variant="plain" icon={SquarePen} onClick={onNewConversation} title="New chat (⌘N)">New chat</Button>
      </aside>

      <aside className="chat-screen__summary" aria-label="Focus">
        <StatTile label="Open actions" value={String(data.actions.filter((a) => a.status === "awaiting_approval").length)} />
        <InsetGroup title="Current focus">
          {data.prioritySnapshot.items.slice(0, 3).map((p) => <ListRow key={`${p.rank}-${p.title}`} title={p.title} subtitle={p.sourceStatus} />)}
          {data.prioritySnapshot.items.length === 0 && <ListRow title="No current priorities" />}
        </InsetGroup>
        {data.projects[0] && <InsetGroup title="Active project"><ListRow title={data.projects[0].title} subtitle={data.projects[0].nextAction ?? undefined} /></InsetGroup>}
      </aside>
          </div>
        </More>
        </>}

        {loading ? (
          <div className="chat-screen__messages" role="status" aria-label="Loading">
            {[0, 1, 2].map((i) => <div key={i} className="chat-bubble chat-bubble--skeleton" data-role={i === 1 ? "user" : "assistant"} />)}
          </div>
        ) : (
          <ol ref={listRef} id={home ? "home-chat-log" : undefined} className="chat-screen__messages" aria-live="polite" aria-label="Messages">
            {home && expanded && all.length > 0 && <li className="hchat__divider" aria-hidden="true">Today</li>}
            {all.length === 0 && !sending && actions.length === 0 && !actionsUnavailable && (home ? (
              <li className="hchat__empty">Ask Amara anything. She knows your priorities, projects and recent activity.</li>
            ) : (
              <li className="chat-screen__empty">
                <Card title="Ask Amara anything" headingLevel={2}>
                  <p>Amara knows your priorities, projects and recent activity.</p>
                  {suggestions.length > 0 && <div className="chat-screen__suggestions">{suggestions.map((s) => <Button key={s} size="small" onClick={() => void send(s)}>{s}</Button>)}</div>}
                </Card>
              </li>
            ))}
            {thread.map((t) => {
              if (t.kind === "action") return renderAction(t.item);
              const i = t.index; const m = all[i];
              const speakingNow = i === speakingAt;
              return (
              <li key={m.id} className="chat-message" data-role={m.role} data-start={starts[i] || undefined} data-voice={m.voice || undefined} data-speaking={speakingNow || undefined}>
                {(starts[i] || speakingNow) && <span className="chat-message__meta">{m.voice && <><Mic size={11} aria-hidden="true" />Voice · </>}{m.role === "user" ? "You" : "Amara"} · <time dateTime={m.timestamp}>{time(m.timestamp)}</time>{speakingNow && <span className="hchat__speaking"><span className="hchat__bars" aria-hidden="true"><i /><i /><i /></span>Speaking</span>}</span>}
                {m.role === "assistant" && m.source === "home" && <AmaraLens state="idle" size={44} level={0.5} decorative className="chat-message__lens" />}
                <div className="chat-bubble" data-role={m.role} data-failed={m.status === "failed" || undefined} title={m.role === "assistant" ? [m.model, time(m.timestamp)].filter(Boolean).join(" · ") : undefined}>
                  {m.content}
                </div>
                {m.role === "assistant" && m.source === "home" && (
                  <div className="chat-message__tier">
                    <Badge tone="info" title="Answered by Home Assistant" aria-label="Answered by Home Assistant">Home</Badge>
                  </div>
                )}
                {m.role === "assistant" && m.status !== "failed" && m.tier && tierLabel(m.tier) && (
                  <div className="chat-message__tier">
                    <Badge tone={m.tier.tier === "fast" ? "neutral" : "accent"} title={tierTitle(m.tier)} aria-label={`Answered on the ${tierLabel(m.tier)} tier`}>{tierLabel(m.tier)}</Badge>
                  </div>
                )}
                {m.status === "failed" && (
                  <div className="chat-message__failed"><Badge tone="danger">Failed</Badge><Button variant="plain" size="small" icon={RotateCcw} onClick={() => retry(m, i)} disabled={sending}>Retry</Button></div>
                )}
              </li>
            );
            })}
            {sending && <ThinkingBubble onCancel={onCancelPending} loadingModel={loadingModel} />}
          </ol>
        )}

        {actionsUnavailable && (
          <div className="chat-screen__actions" aria-label="Actions">
            <Card padding="md"><p>Action ledger unavailable</p>{onRetrySources && <Button size="small" onClick={onRetrySources}>Retry</Button>}</Card>
          </div>
        )}

        {askContext && (
          <div className="take-ctx" role="note" aria-label="Context for this question" title={askContext.text}>
            <TakeEmber small /><b>From Amara&apos;s take · {askContext.from}</b>
            <span>{askContext.chips.map((c) => `${c.label} ${c.value}`).join(" · ")}</span>
            {askContext.sample && <DataTag kind="sample" />}
            <button type="button" className="take-ctx__x" aria-label="Remove this context" onClick={onClearContext}>✕</button>
          </div>
        )}
        <form className="chat-screen__composer" onSubmit={submit}>
          {/* Wrapper carries the hint: WebKit fires no hover (so no tooltip) on a disabled button. */}
          <span className="chat-composer__deep-wrap" title={deepAvailable ? undefined : "Deep needs the Amara Router"}>
            <Button
              variant="plain"
              icon={Brain}
              className="chat-composer__deep"
              aria-pressed={deep && deepAvailable}
              aria-disabled={deepAvailable ? undefined : true}
              aria-describedby={deepAvailable ? undefined : "chat-deep-reason"}
              title={deepAvailable ? "Send to Amara's deep model" : "Deep needs the Amara Router"}
              onClick={() => { if (deepAvailable) setDeep((on) => !on); }}
            >Deep</Button>
            {!deepAvailable && <span id="chat-deep-reason" className="ui-visually-hidden">Deep needs the Amara Router.</span>}
          </span>
          <label htmlFor="chat-composer" className="ui-visually-hidden">Message Amara</label>
          <textarea
            id="chat-composer"
            ref={composerRef}
            rows={1}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onComposerKey}
            placeholder={!online ? "Amara is offline — messages may fail" : deep && deepAvailable ? "Message Amara (Deep)" : "Message Amara"}
            aria-keyshortcuts={home ? "Meta+1" : undefined}
          />
          {home && <kbd className="hchat__kbd" title="Focus the chat from anywhere" aria-hidden="true">⌘1</kbd>}
          <Button type="submit" variant="primary" icon={Send} iconOnly aria-label="Send" disabled={sending || !draft.trim()} />
        </form>
      </section>

    </div>
  );
}
