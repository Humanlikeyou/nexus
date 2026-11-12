import { useState } from "react";
import { Button, DataTag } from "../ui";
import { roadmapAdapter, type RoadmapAdapter, type StepQuestion as Q } from "./roadmap";
import "./goals.css";

/** Today's one step question ("Have you done: <step>?"). Mounted inside the take by TakeStepQuestion. */
export function StepQuestion({ adapter, onAnswered }: { adapter?: RoadmapAdapter; onAnswered?: () => void }) {
  const a = adapter ?? roadmapAdapter();
  const [reply, setReply] = useState("");
  const [held, setHeld] = useState<Q | null>(null);
  const w = a.todaysQuestion();
  // Keep the question on screen after the answer: the backend returns null once it is answered.
  const q = held ?? (w.state === "unwired" ? null : w.value);
  if (!q) return null;
  const answer = async (ans: "yes" | "not_yet") => {
    setHeld(q);
    try {
      const r = await a.answerStep(q.goalId, q.stepId, ans);
      if (r.state === "unwired") { setReply("Not saved. This needs the roadmap store to be wired first."); return; }
      onAnswered?.();
      if (ans === "not_yet") { setReply("No problem. Amara will ask again in a few days."); return; }
      const rm = a.roadmap(q.goalId); // the adapter's own percentage, never computed here
      const pct = rm.state !== "unwired" && rm.value ? rm.value.pct : null;
      setReply(pct === null ? "Marked done." : `Marked done. ${q.goalTitle} is now ${pct}% of the roadmap done.`);
    } catch { setReply("That didn't save. Try again in a moment."); }
  };
  return (
    <section className="gl-q" aria-label="Step question">
      <p className="pk-eyebrow">{q.goalTitle} {w.state === "sample" && <DataTag kind="sample" />}</p>
      <p className="gl-q__text">Have you done: {q.stepTitle}?</p>
      {reply ? <p className="gl-msg" role="status">{reply}</p> : (
        <div className="gl-step__act">
          <Button size="small" variant="secondary" onClick={() => answer("yes")}>Yes</Button>
          <Button size="small" variant="plain" onClick={() => answer("not_yet")}>Not yet</Button>
        </div>
      )}
    </section>
  );
}
