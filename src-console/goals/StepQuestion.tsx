import { useState } from "react";
import { Button, DataTag } from "../ui";
import { roadmapAdapter, type RoadmapAdapter } from "./roadmap";
import "./goals.css";

/** Today's one step question ("Have you done: <step>?"). Exported for the takes; not wired into AmaraTake yet. */
export function StepQuestion({ adapter }: { adapter?: RoadmapAdapter }) {
  const a = adapter ?? roadmapAdapter();
  const [reply, setReply] = useState("");
  const w = a.todaysQuestion();
  const q = w.state === "unwired" ? null : w.value;
  if (!q || reply === "gone") return null;
  const answer = async (ans: "yes" | "not_yet") => {
    try {
      const r = await a.answerStep(q.goalId, q.stepId, ans);
      setReply(r.state === "unwired" ? "Not saved. This needs the roadmap store to be wired first." : ans === "yes" ? "Marked done." : "No problem. Amara will ask again in a few days.");
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
