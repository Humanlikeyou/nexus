import { useState } from "react";
import { DataTag } from "../ui";
import { TAKE_PAGE_LABEL } from "../amaraTake/takeView";
import { SKIP_REASONS, type Action, type Answer, type AnswerState, type SkipReason, type Suggestion } from "./adapters";
import { Level, Nums, Rich, SourceLine } from "./parts";

type AnswerFn = (state: AnswerState, reason?: SkipReason | null) => Promise<Action>;

const said = (r: Action, ok: string) => (r.state === "unwired" ? "Saving answers needs wiring, so nothing was stored." : r.state === "sample" ? `Sample data: ${ok}` : ok);

type AnswerCtl = { a: Answer; note: string | null; tap: (state: AnswerState, reason?: SkipReason | null) => Promise<void>; undo: () => Promise<void>; reopen: () => void };

/**
 * One answer for a suggestion. The stored answer (`s.answer`, reduced from the take events) is the truth: whenever it
 * changes (a reload, a detection, an Undo from another view) the local copy follows it. The local copy only bridges the
 * moment between a tap and the store echoing it back.
 */
function useAnswer(s: Suggestion, onAnswer: AnswerFn, onUndo: () => Promise<Action>): AnswerCtl {
  const stored = s.answer;
  const sig = `${s.id}|${stored.state}|${stored.source ?? ""}|${stored.at ?? ""}`;
  const [a, setA] = useState<Answer>(stored);
  const [seen, setSeen] = useState(sig);
  if (seen !== sig) { setSeen(sig); setA(stored); }
  const [note, setNote] = useState<string | null>(null);
  const tap = async (state: AnswerState, reason: SkipReason | null = null) => {
    const r = await onAnswer(state, reason);
    if (r.state !== "unwired") setA({ state, source: "tap", at: "just now", reason });
    setNote(said(r, state === "done" ? "marked done." : state === "skipped" ? (reason ? "reason saved." : "marked skipped.") : "left out today; it doesn't count either way."));
  };
  const undo = async () => { const r = await onUndo(); if (r.state !== "unwired") setA({ state: "waiting" }); setNote(said(r, "undone.")); };
  return { a, note, tap, undo, reopen: () => setA({ state: "waiting" }) };
}

/** Done · Skipped · Not today. One tap, or none: no answer by midnight is Unknown, never Skipped. A Done answer never shows here: the block folds to {@link DoneLine}. */
export function AnswerBar({ s, ctl, compact = false }: { s: Suggestion; ctl: AnswerCtl; compact?: boolean }) {
  const { a, note, tap, reopen } = ctl;
  const options: Array<[AnswerState, string]> = compact ? [["done", "Done"], ["skipped", "Skip"]] : [["done", "Done"], ["skipped", "Skipped"], ["not_today", "Not today"]];
  const label = `Your answer: ${s.title}`;
  if (a.state === "unknown") {
    return (
      <div className="t-answer">
        <span className="t-status"><span aria-hidden="true">–</span><b>Unknown</b></span>
        <span className="t-note">No answer and nothing in the data. Left out of the comparison, never counted as skipped.</span>
        <button type="button" className="t-small" onClick={reopen}>Answer it…</button>
      </div>
    );
  }
  return (
    <div className="t-answer">
      <div className="t-seg" role="group" aria-label={label}>
        {options.map(([st, word]) => <button key={st} type="button" aria-pressed={a.state === st} data-on={a.state === st ? (st === "skipped" ? "skip" : "on") : undefined} onClick={() => { void tap(st); }}>{word}</button>)}
      </div>
      {a.state === "skipped" && !compact && (
        <span className="t-reasons" role="group" aria-label="Why skipped (optional)">
          {SKIP_REASONS.map(([r, word]) => <button key={r} type="button" className="t-reason" aria-pressed={a.reason === r} onClick={() => { void tap("skipped", r); }}>{word}</button>)}
        </span>
      )}
      {a.state === "not_today" && <span className="t-note">Left out of the comparison.</span>}
      {note && <span className="t-note" role="status">{note}</span>}
    </div>
  );
}

/** The rest of the day once it is Done (tapped or detected): one quiet line, still undoable. */
function DoneLine({ s, ctl, folded }: { s: Suggestion; ctl: AnswerCtl; folded: boolean }) {
  const { a, note, undo } = ctl;
  const detected = a.state === "done" && a.source === "detected";
  return (
    <div className="t-fold" data-open={folded ? "true" : "false"}>
     <div className="t-fold__in" aria-hidden={!folded} inert={!folded}>
      <div className="t-donerow" role={folded ? "group" : undefined} aria-label={folded ? `Done today: ${s.title}` : undefined}>
        <span className="t-status"><span aria-hidden="true">✓</span><b>Done today</b></span>
        <span className="t-donerow__title">{s.title}</span>
        {detected && a.detail && <span className="t-auto"><span aria-hidden="true">◎</span>{a.detail}</span>}
        <span className="t-grow" />
        {note && folded && <span className="t-donerow__live" role="status">{note}</span>}
        <button type="button" className="t-small" onClick={() => { void undo(); }}>{detected ? "Not right? Undo" : "Undo"}</button>
      </div>
     </div>
    </div>
  );
}

/** "Today's one thing" inside the take card (design-1111t t1/t9): a new one every day, with why, numbers and evidence. */
export function SuggestionBlock({ s, onAnswer, onUndo, detects, compact = false, eyebrow, footnote }: { s: Suggestion; onAnswer: AnswerFn; onUndo: () => Promise<Action>; detects: boolean; compact?: boolean; eyebrow?: string; footnote?: string }) {
  const head = eyebrow ?? `Today's one thing · ${TAKE_PAGE_LABEL[s.page]}`;
  const ctl = useAnswer(s, onAnswer, onUndo);
  const folded = ctl.a.state === "done";
  // Folding is derived from the answer, never stored: a reload on the same day, or a store echo, shows the same thing. The next day is a new suggestion id with a new answer.
  return (
    <div className="t-block" data-folded={folded ? "true" : undefined}>
     <div className="t-fold" data-open={folded ? "false" : "true"}>
      <div className="t-fold__in" aria-hidden={folded} inert={folded}>
        {compact ? (
          <section className="t-rec t-rec--compact" aria-label={head}>
            <div className="t-rec__top"><span className="t-eyebrow">{head}</span><span className="t-grow" /><Level level={s.evidence.level} /></div>
            <h3>{s.title}</h3>
            <AnswerBar s={s} ctl={ctl} compact />
          </section>
        ) : (
          <section className="t-rec" aria-label={head}>
            <div className="t-rec__top"><span className="t-eyebrow">{head}</span><span className="t-pill">New today</span><span className="t-pill t-pill--quiet">{s.seen}</span><span className="t-grow" /><Level level={s.evidence.level} /></div>
            <h3>{s.title}</h3>
            <p>{s.how}</p>
            <div className="t-g">
              <div className="t-why">
                <span className="t-k">Why this, from your data</span>
                <p><Rich text={s.why} /></p>
                <Nums rows={s.facts.map((f) => ({ label: f.label, value: f.value, note: f.note }))} />
              </div>
              <div className="t-ev">
                <span className="t-k">Evidence · {s.evidence.level === "own" ? "your data only" : s.evidence.level}</span>
                <p>{s.evidence.summary}</p>
                {s.evidence.sources.length > 0 && <ul className="t-srcs" aria-label="Sources">{s.evidence.sources.map((src) => <SourceLine key={src.cite} s={src} />)}</ul>}
                {s.evidence.risk && <p><b>Risk:</b> {s.evidence.risk}</p>}
                {s.evidence.alsoConsidered && <p><b>Also considered:</b> {s.evidence.alsoConsidered}</p>}
              </div>
            </div>
            <div className="t-track">
              <AnswerBar s={s} ctl={ctl} />
              <span className="t-note t-track__rule">{detects && s.detectRule ? s.detectRule : <>Auto-detect <DataTag kind="wire" />. Tap Done or Skipped for now.</>}</span>
            </div>
            {footnote && <p className="t-foot">{footnote}</p>}
          </section>
        )}
      </div>
     </div>
      <DoneLine s={s} ctl={ctl} folded={folded} />
    </div>
  );
}
