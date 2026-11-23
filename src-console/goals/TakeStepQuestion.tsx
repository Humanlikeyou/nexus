import { useEffect, useId, useState } from "react";
import { useStoresTick } from "../lib/localStores";
import { roadmapAdapter, type RoadmapAdapter } from "./roadmap";
import { StepQuestion } from "./StepQuestion";
import { stepQuestionStore, useStepQuestionClaim } from "./stepQuestionStore";

/** Under a take: today's step question, in the first take that mounts. Nothing when unwired, null, answered, or another take has it. */
export function TakeStepQuestion({ adapter }: { adapter?: RoadmapAdapter }) {
  const id = useId();
  const a = adapter ?? roadmapAdapter();
  useStoresTick();
  useStepQuestionClaim();
  const [engaged, setEngaged] = useState(false);
  const w = a.todaysQuestion();
  const open = w.state !== "unwired" && w.value !== null && !stepQuestionStore.answeredToday();
  useEffect(() => { if (open) stepQuestionStore.claim(id); }, [open, id]);
  useEffect(() => () => stepQuestionStore.release(id), [id]);
  const mine = stepQuestionStore.claimant() === id;
  if (!mine || !(open || engaged)) return null;
  return <StepQuestion adapter={a} onAnswered={() => { setEngaged(true); stepQuestionStore.markAnswered(); }} />;
}
