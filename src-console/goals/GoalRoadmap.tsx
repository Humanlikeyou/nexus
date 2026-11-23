import { Check } from "lucide-react";
import { useState } from "react";
import { Button, DataTag, Select, TextField } from "../ui";
import { Bar } from "../personal/parts";
import { roadmapAdapter, STEP_WEIGHT, type Roadmap, type RoadmapAdapter, type RoadmapStep, type StepSize } from "./roadmap";
import "./goals.css";

const SIZE_LABEL: Record<StepSize, string> = { small: "Small", medium: "Medium", big: "Big" };
const SIZE_OPTIONS = (["small", "medium", "big"] as const).map((v) => ({ value: v, label: SIZE_LABEL[v] }));
const NOT_SAVED = "Not saved. This needs the roadmap store to be wired first.";
const FAILED = "That didn't save. Try again in a moment.";

type Patch = Partial<Pick<RoadmapStep, "title" | "size" | "done" | "notYetCount">>;

/** Weighted % the same way the contract defines it; only used after he answers, until the store re-reads. */
const pctOf = (steps: RoadmapStep[]) => {
  const total = steps.reduce((n, s) => n + STEP_WEIGHT[s.size], 0);
  return total ? Math.round((steps.filter((s) => s.done).reduce((n, s) => n + STEP_WEIGHT[s.size], 0) / total) * 100) : 0;
};

/** One goal's roadmap (decision 0013). Every state: unwired, empty, drafting, suggested, accepted, all done. */
export function GoalRoadmap({ goalId, adapter, defaultOpen = false }: { goalId: string; adapter?: RoadmapAdapter; defaultOpen?: boolean }) {
  const a = adapter ?? roadmapAdapter();
  const [tick, setTick] = useState(0);
  const [queued, setQueued] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [over, setOver] = useState<Record<string, Patch>>({});
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<{ title: string; size: StepSize }>({ title: "", size: "small" });
  const [msg, setMsg] = useState("");
  void tick;
  const w = a.roadmap(goalId);
  const rm: Roadmap | null = w.state === "unwired" ? null : w.value;
  const tag = w.state === "sample" ? <DataTag kind="sample" /> : null;

  const run = async (act: () => Promise<{ state: string }>, ok: string, after?: () => void) => {
    try {
      const r = await act();
      if (r.state === "unwired") { setMsg(NOT_SAVED); return; }
      after?.(); setMsg(ok); setTick((t) => t + 1);
    } catch { setMsg(FAILED); }
  };
  const patchStep = (id: string, p: Patch) => setOver((o) => ({ ...o, [id]: { ...o[id], ...p } }));

  if (w.state === "unwired") {
    return <p className="gl-note" data-testid="roadmap-unwired">Roadmaps appear once Amara&apos;s roadmap store is wired. Nothing here is made up. <DataTag kind="wire" /></p>;
  }

  const steps = (rm?.steps ?? []).slice().sort((x, y) => x.order - y.order).map((s) => ({ ...s, ...over[s.id] }));
  const status = accepted && rm?.status === "suggested" ? "accepted" : rm?.status;
  const touched = Object.keys(over).length > 0;
  const pct = rm ? (touched ? pctOf(steps) : rm.pct) : 0;
  const allDone = !!rm && steps.length > 0 && steps.every((s) => s.done);
  const openCount = steps.filter((s) => !s.done).length;
  const summary = !rm ? "No roadmap yet" : status === "drafting" ? "Drafting tonight" : allDone ? "All steps done" : `${pct}% · ${openCount} step${openCount === 1 ? "" : "s"} left`;

  return (
    <details className="gl-rm" open={defaultOpen || undefined} data-status={allDone ? "done" : status ?? "empty"}>
      <summary><span>Roadmap</span><span className="gl-rm__sum">{summary}</span></summary>
      <div className="gl-rm__body">
        {tag}
        {(!rm && !queued) && (
          <div className="gl-empty">
            <p className="gl-note">No roadmap yet. Amara can draft one overnight on the local deep model.</p>
            <Button size="small" onClick={() => run(() => a.request(goalId), "", () => setQueued(true))}>Draft a roadmap tonight</Button>
          </div>
        )}
        {((!rm && queued) || status === "drafting") && <p className="gl-note" data-testid="roadmap-drafting">Amara drafts it tonight, ready in the morning.</p>}
        {rm && status !== "drafting" && (
          <>
            {status === "suggested" && (
              <div className="gl-suggest">
                <p className="gl-note">Amara suggested this roadmap. Edit any step, then accept it.</p>
                <Button size="small" variant="primary" onClick={() => run(() => a.accept(goalId), "Roadmap accepted.", () => setAccepted(true))}>Accept</Button>
              </div>
            )}
            <div className="gl-pct"><Bar pct={pct} /><span className="gl-pct__n">{pct}%</span></div>
            {allDone && <p className="gl-done" role="status"><Check aria-hidden="true" size={13} /> All steps done. Nice work.</p>}
            {steps.length === 0 && <p className="gl-note">This roadmap has no steps yet.</p>}
            <ol className="gl-steps">
              {steps.map((s) => (
                <li key={s.id} className="gl-step" data-done={s.done || undefined}>
                  {editing === s.id ? (
                    <form className="gl-edit" onSubmit={(e) => { e.preventDefault(); const t = draft.title.trim(); if (!t) return; run(() => a.editStep(goalId, s.id, { title: t, size: draft.size }), "Step updated.", () => { patchStep(s.id, { title: t, size: draft.size }); setEditing(null); }); }}>
                      <TextField label="Step" value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} />
                      <Select label="Size" options={SIZE_OPTIONS} value={draft.size} onChange={(e) => setDraft({ ...draft, size: e.target.value as StepSize })} />
                      <Button size="small" variant="primary" type="submit">Save step</Button>
                      <Button size="small" variant="plain" onClick={() => setEditing(null)}>Cancel</Button>
                    </form>
                  ) : (
                    <>
                      <span className="gl-tick" data-on={s.done || undefined} aria-hidden="true">{s.done && <Check size={11} />}</span>
                      <div className="gl-step__main">
                        <div className="gl-step__title">{s.title}<span className="gl-size" data-size={s.size}>{SIZE_LABEL[s.size]}</span>{s.done && <span className="gl-vh"> (done)</span>}</div>
                        {s.why && <small className="pk-sub">{s.why}</small>}
                        {!s.done && s.smallerSuggestion && (
                          <p className="gl-smaller">A smaller first move: {s.smallerSuggestion}
                            {" "}<Button size="small" variant="plain" onClick={() => run(() => a.editStep(goalId, s.id, { title: s.smallerSuggestion ?? s.title }), "Step swapped for the smaller move.", () => patchStep(s.id, { title: s.smallerSuggestion ?? s.title }))}>Use this</Button>
                          </p>
                        )}
                      </div>
                      <div className="gl-step__act">
                        {!s.done && (
                          <>
                            <Button size="small" variant="secondary" aria-label={`Yes, I did: ${s.title}`} onClick={() => run(() => a.answerStep(goalId, s.id, "yes"), "Marked done.", () => patchStep(s.id, { done: true }))}>Yes</Button>
                            <Button size="small" variant="plain" aria-label={`Not yet: ${s.title}`} onClick={() => run(() => a.answerStep(goalId, s.id, "not_yet"), "No problem. Amara will ask again in a few days.", () => patchStep(s.id, { notYetCount: s.notYetCount + 1 }))}>Not yet</Button>
                          </>
                        )}
                        <Button size="small" variant="plain" aria-label={`Edit step: ${s.title}`} onClick={() => { setEditing(s.id); setDraft({ title: s.title, size: s.size }); }}>Edit</Button>
                      </div>
                    </>
                  )}
                </li>
              ))}
            </ol>
          </>
        )}
        <p className="gl-msg" role="status" aria-live="polite">{msg || (queued ? "Amara drafts it tonight, ready in the morning." : "")}</p>
      </div>
    </details>
  );
}
