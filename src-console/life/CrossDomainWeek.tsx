// Decision 0066: the weekly Life review's cross-domain part. It adds to the existing review (built by
// `weekly_review_read`) rather than replacing it: the week's cross-domain findings and neurotechnology sessions (workouts
// are already in the review).
import { useEffect, useMemo, useState } from "react";
import { briefingCalls, type WeekCrossDomain } from "../briefings/briefingApi";
import { NeuroCard } from "../briefings/BriefingParts";
import type { Invoke } from "../lib/localStores";
import { Card } from "../ui";
import type { LifeWrites } from "./lifeApi";
import { RecommendationCard } from "./RecommendationCard";

export function CrossDomainWeek({ call, writes, weekStart }: { call: Invoke; writes: LifeWrites; weekStart: string }) {
  const api = useMemo(() => briefingCalls(call), [call]);
  const [week, setWeek] = useState<WeekCrossDomain | null | "failed">(null);
  useEffect(() => {
    let live = true;
    api.week(weekStart).then((w) => { if (live) setWeek(w); }).catch(() => { if (live) setWeek("failed"); });
    return () => { live = false; };
  }, [api, weekStart]);
  if (week === null) return <Card title="Across your domains" headingLevel={2}><p className="pk-note">Reading this week&apos;s records…</p></Card>;
  if (week === "failed") return <Card title="Across your domains" headingLevel={2}><p className="pk-note lf-err">This week&apos;s cross-domain part couldn&apos;t be read.</p></Card>;
  return (
    <>
      <Card title="Across your domains" headingLevel={2}>
        <p className="pk-note">{week.weekStart} – {week.weekEnd}. Built when you opened this review, from fixed rules over your records.{week.missing.length > 0 ? ` Not available: ${week.missing.join(", ")}.` : ""}</p>
        {week.recommendations.length === 0 && <p className="pk-note">No cross-domain findings for this week.</p>}
      </Card>
      {week.recommendations.map((r) => <RecommendationCard key={r.recommendationId} rec={r} writes={writes} heading={3} title="Across domains" />)}
      <NeuroCard n={week.neuro} title="Neurotechnology this week" />
    </>
  );
}
