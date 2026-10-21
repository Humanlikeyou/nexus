import { useState, type FormEvent } from "react";
import { type ConversationMessage, actionPlan } from "../app/conversation";
import { tone, date, time } from "../lib/format";
import type { SystemSnapshot, TerrenceAction } from "../types";
import { State } from "./shared";
import { Activity, BookOpen, Bot, Check, RotateCcw, Send, SquarePen, Sparkles, X } from "lucide-react";

export function TerranceField({
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
