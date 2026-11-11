import { useState } from "react";
import { Button, Card, DataTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { TAKE_PAGE_LABEL } from "../amaraTake/takeView";
import { takesAdapter, VERDICT_WORD, type Action, type TakesAdapter, type TypeOutcome } from "./adapters";
import { OutcomeChart } from "./FollowUp";
import { VerdictChip } from "./parts";
import "./takes.css";

const NOT_CLAIMED: Record<TypeOutcome["verdict"], string> = {
  better: "that the suggestion made the difference. Days you did it may differ in other ways.",
  none: "that it doesn't help you. It just didn't move this number in the days checked.",
  worse: "that it harms you. The days it was done were worse on this number, so Amara stops suggesting it unless you ask.",
  unclear: "anything. Too much else changed on the same days to tell.",
  too_early: "anything yet. Amara waits for enough days both ways before it judges.",
};

/** Patterns › Follow-through (t7, reworked for daily suggestions): results grouped by suggestion type over time. */
export function FollowThroughView({ adapter = takesAdapter() }: { adapter?: TakesAdapter }) {
  const w = adapter.outcomes();
  const list = valueOf(w);
  const [pick, setPick] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  if (!list) {
    return <Card title="Follow-through" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">Results <DataTag kind="wire" />: Amara needs the answer log and each page&apos;s metric store. Until then nothing is judged and no verdict is shown.</p></Card>;
  }
  if (list.length === 0) return <Card title="Follow-through"><p className="pk-note">No results yet. Each suggestion type needs enough days done and not done before Amara says anything.</p></Card>;
  const sel = list.find((o) => o.type === pick) ?? list[0];
  const sample = w.state === "sample";
  const act = async (a: "keep" | "keep_suggesting" | "stop", done: string) => { const r: Action = await adapter.typeAction(sel.type, a); setNote(r.state === "unwired" ? "Saving this needs wiring, so nothing changed." : `${r.state === "sample" ? "Sample data: " : ""}${done}`); };
  return (
    <div className="t-split">
      <Card title={`${sel.title} · ${sel.window}`} accessory={<span className="pk-acc">{sample && <DataTag kind="sample" />}</span>}>
        <div className="t-out__text">
          <div className="t-rec__top"><VerdictChip v={sel.verdict} /><span className="t-note">{TAKE_PAGE_LABEL[sel.page]} · measured: {sel.metric} · suggested {sel.suggested}× · done {sel.done} ({sel.detected} detected)</span></div>
          {sel.points.length > 0 && <OutcomeChart o={sel} sample={false} />}
          <table className="pk-table" aria-label={`${sel.title}: days compared`}>
            <thead><tr><th scope="col">Compared</th><th scope="col" className="r">n</th><th scope="col" className="r">{sel.metric}</th></tr></thead>
            <tbody>{sel.stats.map((s) => <tr key={s.label}><td>{s.label}</td><td className="r">{s.n} {s.unit}</td><td className="r">{s.mean}</td></tr>)}</tbody>
          </table>
          {sel.periods.length > 0 && (
            <table className="pk-table" aria-label={`${sel.title}: over time`}>
              <thead><tr><th scope="col">Period</th><th scope="col" className="r">Suggested</th><th scope="col" className="r">Done</th><th scope="col" className="r">Difference</th><th scope="col">Verdict</th></tr></thead>
              <tbody>{sel.periods.map((p) => <tr key={p.label}><td>{p.label}</td><td className="r">{p.suggested}</td><td className="r">{p.done}</td><td className="r">{p.diff ?? "–"}</td><td><VerdictChip v={p.verdict} short /></td></tr>)}</tbody>
            </table>
          )}
          <p className="t-rules"><b>Result:</b> {sel.diff ? `${sel.diff}${sel.range ? ` (90% range ${sel.range})` : ""}${sel.wobble ? `, your normal wobble is ${sel.wobble}` : ""}. ` : ""}{VERDICT_WORD[sel.verdict]}. <b>Method:</b> {sel.checked} {sel.excluded} <b>Not claimed:</b> {NOT_CLAIMED[sel.verdict]}</p>
          {sel.verdict === "better" && <div className="t-out__acts">
            <Button size="small" variant="primary" onClick={() => { void act("keep", "it's a habit now; it shows in Today › Goals."); }}>Make it a habit</Button>
            <Button size="small" onClick={() => { void act("stop", "Amara stops suggesting it."); }}>Stop suggesting it</Button>
          </div>}
          {note && <p className="t-note" role="status">{note}</p>}
        </div>
      </Card>
      <Card title="By suggestion type" accessory={<span className="pk-acc">{sample && <DataTag kind="sample" />}</span>}>
        <div className="t-types">
          {list.map((o) => (
            <button key={o.type} type="button" className="t-type" aria-current={o.type === sel.type} onClick={() => { setPick(o.type); setNote(null); }}>
              <span>{o.title}<br /><small>{TAKE_PAGE_LABEL[o.page]} · {o.metric} · done {o.done} of {o.suggested}</small></span><VerdictChip v={o.verdict} short />
            </button>
          ))}
        </div>
        <p className="t-rules"><b>Same rules for every type:</b> at least 5 days done and 5 not before any verdict; the 90% range and n always shown; &ldquo;better&rdquo; only when the range clears your normal wobble; linked, never proven.</p>
      </Card>
    </div>
  );
}
