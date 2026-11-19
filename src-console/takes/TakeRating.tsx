import { useState } from "react";
import type { TakePage } from "../amaraTake/types";
import { ratedOn, recordTakeRating, todayJst } from "./events";

/**
 * "Helpful / Not helpful" in a take footer (decision 0042). One rating per take per day: once rated, the pair
 * is replaced by "Thanks". If the store refuses the event the buttons stay and say so; a rating is never
 * shown as saved when it was not.
 */
export function TakeRating({ page, takeId }: { page: TakePage; takeId: string }) {
  const [done, setDone] = useState(() => ratedOn(takeId, todayJst()));
  const [failed, setFailed] = useState(false);
  const rate = (helpful: boolean) => {
    void recordTakeRating(page, takeId, helpful).then((ok) => { setFailed(!ok); if (ok) setDone(true); }, () => setFailed(true));
  };
  if (done) return <span className="take__src" role="status">Thanks</span>;
  return (
    <span className="take__rate" role="group" aria-label="Was this take helpful?">
      {failed && <span className="take__src" role="status">Couldn&apos;t save that</span>}
      <button type="button" className="take__ask" onClick={() => rate(true)}>Helpful</button>
      <button type="button" className="take__ask" onClick={() => rate(false)}>Not helpful</button>
    </span>
  );
}
