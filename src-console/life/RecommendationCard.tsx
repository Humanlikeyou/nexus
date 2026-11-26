import { useState } from "react";
import { Button, Card, StatusText, TextField } from "../ui";
import { MEASURED_LABEL, parsePlan, planText } from "../briefings/briefingApi";
import { dayLabel, QUALITY_LABEL, ROLE_LABEL, type LifeWrites, type RecommendationDetail, type RecommendationRow } from "./lifeApi";

/** The three calls a card needs, so a business-scoped caller can supply its own (0066). */
export type RecWrites = Pick<LifeWrites, "recGet" | "recRespond" | "recOutcome">;

const STATUS_TEXT: Record<string, string> = {
  proposed: "New", shown: "Suggested", accepted: "Accepted", modified: "Accepted with changes", dismissed: "Dismissed",
  completed: "Done", evaluated: "Outcome recorded", expired: "Expired",
};
const EVALUATION = [
  { value: "helped", label: "It helped" },
  { value: "no_effect", label: "No difference" },
  { value: "worse", label: "It made things worse" },
] as const;
const windowText = (a: string | null, b: string | null) => (a && b ? `${dayLabel(a.slice(0, 10))} – ${dayLabel(b.slice(0, 10))}` : a ? `from ${dayLabel(a.slice(0, 10))}` : "no time window");

/**
 * One recommendation with its five controls (PLAN-0055): Accept, Modify (edit the suggested action, then
 * `modified` with what changed), Dismiss, Explain (its evidence: role, quality and window, then the rationale
 * and uncertainty) and Record outcome (done, then how it went). Each control is offered only where the
 * backend's status machine allows it, so nothing is shown that would be refused.
 */
export function RecommendationCard({ rec, writes, title = "Amara's suggestion", heading = 2, onChanged }: {
  rec: RecommendationRow; writes: RecWrites; title?: string; heading?: 2 | 3; onChanged?: () => void;
}) {
  const [status, setStatus] = useState(rec.status);
  const [action, setAction] = useState(rec.suggestedAction ?? "");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(action);
  const [detail, setDetail] = useState<RecommendationDetail | null | "failed">(null);
  const [open, setOpen] = useState(false);
  const [msg, setMsg] = useState<{ text: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<unknown>, next: string, ok: string) => {
    setBusy(true); setMsg(null);
    try { await fn(); setStatus(next); setMsg({ text: ok }); onChanged?.(); }
    catch { setMsg({ text: "That didn't save. Nothing was changed.", error: true }); }
    finally { setBusy(false); }
  };
  const explain = async () => {
    if (open) { setOpen(false); return; }
    setOpen(true);
    if (detail && detail !== "failed") return;
    try { setDetail(await writes.recGet(rec.recommendationId)); } catch { setDetail("failed"); }
  };
  const answerable = status === "proposed" || status === "shown";
  const acted = status === "accepted" || status === "modified";
  const evaluable = status === "completed" || status === "dismissed" || status === "expired";
  // 0066: a cross-domain suggestion also stores how it's measured and its reporting period.
  const plan = parsePlan(rec.evaluationJson);
  const cross = rec.producedBy === "rules:cross_domain";
  const saveModify = () => {
    const text = draft.trim();
    if (!text) { setMsg({ text: "Write what you'll do instead, or cancel.", error: true }); return; }
    void run(() => writes.recRespond(rec.recommendationId, "modified", { suggestedAction: { from: rec.suggestedAction, to: text } }), "modified", "Saved with your change.").then(() => { setAction(text); setEditing(false); });
  };
  return (
    <Card title={title} headingLevel={heading} className="lf-rec" accessory={<StatusText kind={answerable ? "wait" : status === "dismissed" || status === "expired" ? "off" : "ok"}>{STATUS_TEXT[status] ?? status}</StatusText>}>
      <p className="lf-rec__title"><b>{rec.title}</b></p>
      {rec.observation && <p className="pk-note">{rec.observation}</p>}
      {action && !editing && <p className="lf-rec__action"><span className="lf-k">Suggested</span> {action}</p>}
      {plan && <p className="pk-note"><span className="lf-k">How it&apos;s measured</span> {plan.how ? `${plan.how} ` : ""}{planText(plan)}</p>}
      {editing && (
        <form className="lf-inline" onSubmit={(e) => { e.preventDefault(); saveModify(); }}>
          <TextField label="What you'll do instead" value={draft} maxLength={200} onChange={(e) => setDraft(e.target.value)} />
          <div className="pk-actions"><Button size="small" variant="primary" type="submit" disabled={busy}>Save change</Button><Button size="small" variant="plain" onClick={() => { setEditing(false); setDraft(action); }}>Cancel</Button></div>
        </form>
      )}
      <div className="pk-actions lf-rec__controls" role="group" aria-label={`Respond to: ${rec.title}`}>
        {answerable && !editing && <>
          <Button size="small" variant="primary" disabled={busy} onClick={() => { void run(() => writes.recRespond(rec.recommendationId, "accepted"), "accepted", "Accepted. Record how it went when you've done it."); }}>Accept</Button>
          <Button size="small" variant="secondary" disabled={busy} onClick={() => { setDraft(action); setEditing(true); }}>Modify</Button>
          <Button size="small" variant="plain" disabled={busy} onClick={() => { void run(() => writes.recRespond(rec.recommendationId, "dismissed"), "dismissed", "Dismissed. It won't come back, and nothing like it for 30 days."); }}>Dismiss</Button>
        </>}
        {acted && <Button size="small" variant="secondary" disabled={busy} onClick={() => { void run(() => writes.recOutcome(rec.recommendationId, "completed"), "completed", "Marked done. How did it go?"); }}>Mark done</Button>}
        {evaluable && EVALUATION.map((e) => (
          <Button key={e.value} size="small" variant="plain" disabled={busy} onClick={() => { void run(() => writes.recOutcome(rec.recommendationId, "evaluated", e.value), "evaluated", "Outcome recorded. Amara uses it to judge future suggestions."); }}>{e.label}</Button>
        ))}
        <Button size="small" variant="plain" aria-expanded={open} onClick={() => { void explain(); }}>{open ? "Hide why" : "Explain"}</Button>
      </div>
      {msg && <p className={msg.error ? "pk-note lf-err" : "pk-note"} role="status">{msg.text}</p>}
      {open && (
        <div className="lf-explain" aria-label="Why Amara suggests this">
          {detail === null ? <p className="pk-note">Reading the evidence…</p>
            : detail === "failed" ? <p className="pk-note lf-err">The evidence couldn&apos;t be read just now.</p>
            : <>
              {detail.evidence.length === 0 ? <p className="pk-note">No evidence is stored for this suggestion.</p> : (
                <ul className="lf-list" aria-label="Evidence">
                  {detail.evidence.map((e) => (
                    <li key={e.evidenceId}><b>{ROLE_LABEL[e.role] ?? e.role}</b> · {QUALITY_LABEL[e.quality] ?? e.quality} · {windowText(e.windowStart, e.windowEnd)}<small>{e.note ?? e.recordRef}</small></li>
                  ))}
                </ul>
              )}
              {detail.recommendation.periodStart && detail.recommendation.periodEnd && <p className="pk-note"><span className="lf-k">Reporting period</span> {windowText(detail.recommendation.periodStart, detail.recommendation.periodEnd)}</p>}
              {detail.recommendation.rationale && <p className="pk-note"><span className="lf-k">{cross ? "Interpretation" : "Why it matters"}</span> {detail.recommendation.rationale}</p>}
              {detail.recommendation.confidence !== null && <p className="pk-note"><span className="lf-k">Confidence</span> {Math.round(detail.recommendation.confidence * 100)}%{detail.recommendation.confidenceBasis ? `, from ${detail.recommendation.confidenceBasis}` : ""}</p>}
              {detail.recommendation.uncertainty && <p className="pk-note"><span className="lf-k">{cross ? "Limitations" : "What isn't known"}</span> {detail.recommendation.uncertainty}</p>}
              {detail.outcomes.filter((o) => o.outcome === "measured").map((o) => (
                <p key={o.outcomeId} className="pk-note"><span className="lf-k">{MEASURED_LABEL[o.evaluation ?? ""] ?? "Measured"}</span> {o.note}</p>
              ))}
              <p className="pk-note">Made by {detail.recommendation.producedBy.startsWith("rules:") ? "a fixed rule" : detail.recommendation.producedBy.startsWith("model:") ? "Amara (a model)" : detail.recommendation.producedBy}.</p>
            </>}
        </div>
      )}
    </Card>
  );
}
