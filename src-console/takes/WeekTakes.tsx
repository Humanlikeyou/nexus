import { useState } from "react";
import { Button, Card, DataTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { TAKE_PAGE_LABEL } from "../amaraTake/takeView";
import { takesAdapter, VERDICT_WORD, type Action, type TakesAdapter } from "./adapters";
import { AnswerDots, Rich, VerdictChip } from "./parts";
import "./takes.css";

const said = (r: Action, done: string) => (r.state === "unwired" ? "Needs wiring, so nothing was set." : `${r.state === "sample" ? "Sample data: " : ""}${done}`);

/** Weekly review tile: how many of this week's suggestions were done (t5). */
export function WeekSuggestionsTile({ adapter = takesAdapter(), title = "AMARA'S SUGGESTIONS" }: { adapter?: TakesAdapter; title?: string }) {
  const w = adapter.week();
  const v = valueOf(w);
  return (
    <div className="pk-review"><h3>{title}</h3>
      {!v ? <><span className="pk-big">–</span><span className="pk-sub">Answer log <DataTag kind="wire" /></span></> : <>
        <span className="pk-big">{v.done} <small>of {v.of} done</small></span>
        <span className="pk-delta" data-d="up">{Math.round((v.done / Math.max(1, v.of)) * 100)}% · {v.detected} counted by themselves</span>
        <span className="pk-sub">{v.skipped} skipped · {v.notToday} not today · {v.unknown} unknown</span></>}
    </div>
  );
}

/** "Suggestions this week": one row per page with Mon→Sun dots, done count and how it's going so far. */
export function WeekSuggestionsCard({ adapter = takesAdapter() }: { adapter?: TakesAdapter }) {
  const w = adapter.week();
  const v = valueOf(w);
  return (
    <Card title="Suggestions this week" accessory={<span className="pk-acc">{w.state === "sample" ? <DataTag kind="sample" /> : <DataTag kind="wire" />}</span>}>
      {!v ? <p className="pk-note">Each page&apos;s daily suggestion and your answers collect here once the answer log is wired. Nothing is counted until then.</p> : (
        <table className="pk-table" aria-label="Suggestions this week">
          <thead><tr><th scope="col">Page</th><th scope="col">Suggestion</th><th scope="col">Mon → Sun</th><th scope="col" className="r">Done</th><th scope="col">So far</th></tr></thead>
          <tbody>{v.rows.map((r) => (
            <tr key={r.page}><td>{TAKE_PAGE_LABEL[r.page]}</td><td>{r.title}<small>{r.sub}</small></td><td><AnswerDots days={r.days} label={`${r.title}, Monday to Sunday`} /></td><td className="r">{r.done} / {r.of}</td><td><VerdictChip v={r.verdict} short /><small>{r.note}</small></td></tr>
          ))}</tbody>
        </table>
      )}
    </Card>
  );
}

/** The Weekly review's "One change", picked from the clearest follow-through result. Null when there isn't one. */
export function TakeChange({ adapter = takesAdapter() }: { adapter?: TakesAdapter }) {
  const w = adapter.week();
  const v = valueOf(w);
  const [note, setNote] = useState<string | null>(null);
  if (!v?.change) return null;
  const c = v.change;
  return (
    <div className="pk-change">
      <span className="pk-eyebrow">ONE CHANGE TO TRY THIS WEEK · FROM YOUR SUGGESTIONS</span>
      <h3>{c.title}</h3>
      <p><Rich text={c.body} /> {w.state === "sample" && <DataTag kind="sample" />}</p>
      <div className="pk-actions">
        <Button size="small" variant="primary" disabled={note !== null} onClick={() => { void adapter.typeAction(c.type, "keep").then((r) => setNote(said(r, "It's a habit now; it shows in Goals. Amara re-checks it monthly."))); }}>Make it a habit</Button>
        <Button size="small" disabled={note !== null} onClick={() => setNote("Not this week. Amara keeps suggesting it on days it fits.")}>Not this week</Button>
      </div>
      {note && <p className="pk-note" role="status">{note}</p>}
    </div>
  );
}

export function TakesThisWeekCard({ adapter = takesAdapter() }: { adapter?: TakesAdapter }) {
  const w = adapter.week();
  const v = valueOf(w);
  if (!v) return null;
  return (
    <Card title="Amara's takes this week" accessory={<span className="pk-acc">{w.state === "sample" && <DataTag kind="sample" />}</span>}>
      {v.takes.map((t) => <p className="pk-take" key={t.text}>&ldquo;{t.text}&rdquo; · {t.from}</p>)}
    </Card>
  );
}

/** Goals: a suggestion that looks better can become a habit; nothing changes a goal until you tap (t6). */
export function ReadyToKeepCard({ adapter = takesAdapter(), title = "Ready to keep" }: { adapter?: TakesAdapter; title?: string }) {
  const w = adapter.goals();
  const v = valueOf(w);
  const [note, setNote] = useState<string | null>(null);
  if (!v || v.ready.length === 0) return null;
  const r = v.ready[0];
  const o = r.outcome;
  return (
    <Card title={title} accessory={<span className="pk-acc">{w.state === "sample" && <DataTag kind="sample" />}</span>}>
      <div className="t-out__text">
        <VerdictChip v={o.verdict} />
        <h4 className="t-mini__h">{o.title}</h4>
        <p className="pk-note">{o.diff ? `${VERDICT_WORD[o.verdict]}: ${o.diff}${o.range ? ` (range ${o.range})` : ""}. ` : ""}{r.note}</p>
        <div className="pk-actions">
          <Button size="small" variant="primary" disabled={note !== null} onClick={() => { void adapter.typeAction(o.type, "keep").then((x) => setNote(said(x, "Added to your habits with its before-and-after numbers."))); }}>Make it a habit</Button>
          <Button size="small" disabled={note !== null} onClick={() => setNote("Not now. It stays a suggestion.")}>Not now</Button>
        </div>
        {note && <p className="pk-note" role="status">{note}</p>}
      </div>
    </Card>
  );
}

export function SuggestedLatelyCard({ adapter = takesAdapter(), title = "Suggested lately" }: { adapter?: TakesAdapter; title?: string }) {
  const w = adapter.goals();
  const v = valueOf(w);
  if (!v) return null;
  return (
    <Card title={title} accessory={<span className="pk-acc">{w.state === "sample" && <DataTag kind="sample" />}</span>}>
      {v.lately.map((l) => <div className="pk-row" key={`${l.page}-${l.title}`}><div className="pk-row__main"><span>{TAKE_PAGE_LABEL[l.page]} · {l.title}</span><small>{l.sub}</small></div><AnswerDots days={l.days} label={l.title} /></div>)}
      <p className="pk-note">Suggestions aren&apos;t goals. Nothing here changes a goal until you tap Make it a habit.</p>
    </Card>
  );
}
