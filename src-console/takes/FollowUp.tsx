import { useState } from "react";
import { Button, DataTag } from "../ui";
import type { Action, Point, TypeOutcome } from "./adapters";
import { Nums, Rich, VerdictChip } from "./parts";

/** Each dot is one suggested day (or one day before it was ever suggested); dashed lines are the group means. */
export function OutcomeChart({ o, sample }: { o: TypeOutcome; sample: boolean }) {
  const pts = o.points;
  if (!pts.length) return null;
  const w = 380, h = 170, xs = pts.map((p) => p.x), ys = pts.map((p) => p.y);
  const x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.floor((Math.min(...ys) - 3) / 5) * 5, y1 = Math.ceil((Math.max(...ys) + 3) / 5) * 5;
  const X = (v: number) => 34 + ((v - x0) / Math.max(1, x1 - x0)) * (w - 110);
  const Y = (v: number) => h - 21 - ((v - y0) / Math.max(1, y1 - y0)) * (h - 34);
  const mean = (g: Point["g"]) => { const a = pts.filter((p) => p.g === g); return a.length ? a.reduce((s, p) => s + p.y, 0) / a.length : null; };
  const start = Math.min(...pts.filter((p) => p.g !== "before").map((p) => p.x));
  const lines: Array<[Point["g"], string]> = [["done", o.stats[0]?.label ?? "done"], ["other", "not done"], ["before", "before"]];
  const label = `${o.metric}: ${o.stats.map((s) => `${s.label} ${s.mean} over ${s.n} ${s.unit}`).join("; ")}`;
  return (
    <figure className="t-chart">
      <svg className="pk-svg" viewBox={`0 0 ${w} ${h}`} role="img" aria-label={label}>
        {[y0, (y0 + y1) / 2, y1].map((v) => <g key={v}><line className="gr" x1="34" x2={w - 76} y1={Y(v)} y2={Y(v)} /><text x="29" y={Y(v) + 4} textAnchor="end">{v}</text></g>)}
        <line className="ax" x1={X(start) - 5} x2={X(start) - 5} y1="8" y2={h - 21} strokeDasharray="3 3" />
        <text x={X(start) - 5} y="8" textAnchor="middle">first suggested</text>
        {lines.map(([g, word]) => { const m = mean(g); return m == null ? null : <g key={g}><line className={`t-mean t-mean--${g}`} x1={g === "before" ? 34 : X(start)} x2={g === "before" ? X(start) - 8 : w - 76} y1={Y(m)} y2={Y(m)} /><text className="t-mean__l" x={w - 71} y={Y(m) + 4}>{Math.round(m)} {word === o.stats[0]?.label ? "done" : word}</text></g>; })}
        {pts.map((p, i) => p.g === "other" ? <circle key={i} className="t-dot t-dot--other" cx={X(p.x)} cy={Y(p.y)} r="3.5" /> : <circle key={i} className={`t-dot t-dot--${p.g}`} cx={X(p.x)} cy={Y(p.y)} r="3.5" />)}
      </svg>
      <figcaption className="t-legend"><span><i data-s="b" />before</span><span><i data-s="d" />done</span><span><i data-s="o" />suggested, not done</span><span>each dot = one day</span>{sample && <DataTag kind="sample" />}</figcaption>
    </figure>
  );
}

/** Follow-up take (t3a): replaces the suggestion block on the day a type's result is ready. Words are fixed by rule. */
export function FollowUpBlock({ o, sample, onAction }: { o: TypeOutcome; sample: boolean; onAction: (a: "keep" | "keep_suggesting" | "stop") => Promise<Action> }) {
  const [note, setNote] = useState<string | null>(null);
  const act = async (a: "keep" | "keep_suggesting" | "stop", ok: string) => { const r = await onAction(a); setNote(r.state === "unwired" ? "Needs wiring, so nothing changed." : `${r.state === "sample" ? "Sample data: " : ""}${ok}`); };
  return (
    <section className="t-out" aria-label={`Follow-up: ${o.title}`}>
      <div className="t-out__text">
        <VerdictChip v={o.verdict} />
        <h3><Rich text={o.headline} /></h3>
        <p><Rich text={o.sentence} /></p>
        <Nums rows={o.stats.map((s) => ({ label: s.label, value: s.mean, note: `${s.n} ${s.unit}` }))} />
        <p><b>Checked:</b> {o.checked} {o.excluded}</p>
      </div>
      <OutcomeChart o={o} sample={sample} />
      {o.verdict === "better" && (
        <div className="t-out__acts">
          <Button size="small" variant="primary" onClick={() => { void act("keep", "it's a habit now; it shows in Today › Goals."); }}>Make it a habit</Button>
          <Button size="small" onClick={() => { void act("keep_suggesting", "Amara keeps suggesting it on days it fits."); }}>Keep suggesting it</Button>
          <Button size="small" onClick={() => { void act("stop", "Amara stops suggesting it."); }}>Stop suggesting it</Button>
          <span className="t-note">A habit shows up in Today › Goals. Amara keeps checking it monthly.</span>
        </div>
      )}
      {note && <p className="t-note" role="status">{note}</p>}
    </section>
  );
}

/** One type's result as a small card (Patterns › Follow-through list, the verdict board). */
export function OutcomeMini({ o }: { o: TypeOutcome }) {
  return (
    <div className="t-mini">
      <VerdictChip v={o.verdict} />
      <h4>{o.title} · {o.window}</h4>
      <p><Rich text={o.headline} /> <Rich text={o.sentence} /></p>
      <Nums rows={o.stats.map((s) => ({ label: s.label, value: s.mean, note: `${s.n} ${s.unit}` }))} />
    </div>
  );
}
