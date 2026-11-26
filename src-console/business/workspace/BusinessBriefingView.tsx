// Decision 0066: Enterprise › Workspaces › Briefing (Enterprise design override, 0063). The daily
// Enterprise briefing for one business and its cross-domain suggestions. Business stores only.
import { useEffect, useMemo, useState } from "react";
import { briefingCalls, type BriefingRuntime, type EnterpriseBriefing, type RecommendationDetail } from "../../briefings/briefingApi";
import { RuntimeNote, SectionCard } from "../../briefings/BriefingParts";
import { RecommendationCard, type RecWrites } from "../../life/RecommendationCard";
import { realInvoke, type Invoke } from "../../lib/localStores";
import { Button, Card } from "../../ui";

/** The card's three calls, against the business file. */
export const businessRecWrites = (call: Invoke): RecWrites => ({
  recGet: (id) => call("recommendation_get", { scope: "business", id }),
  recRespond: (id, response, modification) =>
    call<string>("recommendation_respond", { scope: "business", id, response, modificationJson: modification ? JSON.stringify(modification) : null }),
  recOutcome: (id, outcome, evaluation = null, note = null) =>
    call<string>("recommendation_record_outcome", { scope: "business", id, outcome, evaluation, resultRef: null, note }),
});

export function BusinessBriefingTab({ id, call = realInvoke }: { id: string; call?: Invoke }) {
  const api = useMemo(() => briefingCalls(call), [call]);
  const writes = useMemo(() => businessRecWrites(call), [call]);
  const [brief, setBrief] = useState<EnterpriseBriefing | null>(null);
  const [runtime, setRuntime] = useState<BriefingRuntime | null>(null);
  const [recs, setRecs] = useState<RecommendationDetail[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [round, setRound] = useState<{ n: number; rebuild: boolean }>({ n: 0, rebuild: false });
  useEffect(() => {
    let live = true;
    const first = round.rebuild ? api.enterpriseRefresh(id) : api.enterprise(id);
    first
      .then((b) => Promise.all([api.runtime(), api.recommendations("business", id)]).then(([r, list]) => ({ b, r, list })))
      .then(
        ({ b, r, list }) => { if (live) { setErr(null); setBrief(b); setRuntime(r); setRecs(list); } },
        (e: unknown) => { if (live) setErr(typeof e === "string" ? e : "The briefing couldn't be read. Nothing is shown rather than guessed."); },
      )
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [api, id, round]);
  return (
    <>
      <Card title={brief ? `Briefing for ${brief.day}` : "Briefing"} headingLevel={3} accessory={<Button size="small" variant="secondary" disabled={busy} onClick={() => { setBusy(true); setRound((x) => ({ n: x.n + 1, rebuild: true })); }}>Rebuild now</Button>}>
        {err && <p className="pk-note" role="status">{err}</p>}
        {!brief && !err && <p className="pk-note">Reading this business&apos;s records…</p>}
        {brief && <p className="pk-note">{brief.method}{brief.missing.length > 0 ? ` Not available right now: ${brief.missing.join(", ")}.` : ""}</p>}
      </Card>
      {brief?.sections.map((s) => <SectionCard key={s.id} s={s} />)}
      {recs && recs.length > 0 && recs.map((d) => (
        <RecommendationCard key={d.recommendation.recommendationId} rec={d.recommendation} writes={writes} heading={3} title="Enterprise suggestion" onChanged={() => { void api.recommendations("business", id).then(setRecs).catch(() => undefined); }} />
      ))}
      <RuntimeNote runtime={runtime} job="enterprise" />
    </>
  );
}
