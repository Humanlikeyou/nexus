// Decision 0066 (Taisun's design override: a Briefings view in Life). The daily personal briefing,
// goal trajectories, training and neurotechnology summaries, and Amara's cross-domain suggestions
// with their outcomes. Everything is read from the backend; nothing here computes a figure.
import { useEffect, useMemo, useState } from "react";
import { briefingCalls, type BriefingRuntime, type DailyBriefing, type RecommendationDetail } from "../briefings/briefingApi";
import { NeuroCard, RuntimeNote, SectionCard, Trajectories, TrainingCard } from "../briefings/BriefingParts";
import type { Invoke } from "../lib/localStores";
import { Button, Card } from "../ui";
import type { LifeWrites } from "./lifeApi";
import { RecommendationCard } from "./RecommendationCard";

export function BriefingsView({ call, writes }: { call: Invoke | null; writes: LifeWrites | null }) {
  const api = useMemo(() => (call ? briefingCalls(call) : null), [call]);
  const [brief, setBrief] = useState<DailyBriefing | null>(null);
  const [runtime, setRuntime] = useState<BriefingRuntime | null>(null);
  const [recs, setRecs] = useState<RecommendationDetail[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!api) return;
    let live = true;
    Promise.all([api.daily(), api.runtime(), api.recommendations("personal")]).then(
      ([b, r, list]) => { if (live) { setErr(null); setBrief(b); setRuntime(r); setRecs(list); } },
      () => { if (live) setErr("The briefing couldn't be read. Nothing is shown rather than guessed."); },
    );
    return () => { live = false; };
  }, [api]);

  if (!api || !writes) return <div className="v-page lf-page"><Card title="Briefings"><p className="pk-note">Briefings read the stores on this Mac. Nothing is shown rather than made up.</p></Card></div>;
  const refresh = async () => {
    setBusy(true); setErr(null);
    try { setBrief(await api.dailyRefresh()); setRuntime(await api.runtime()); setRecs(await api.recommendations("personal")); }
    catch (e) { setErr(typeof e === "string" ? e : "The briefing couldn't be rebuilt just now."); }
    finally { setBusy(false); }
  };
  return (
    <div className="v-page lf-page">
      <Card title={brief ? `Your briefing for ${brief.day}` : "Your briefing"} accessory={<Button size="small" variant="secondary" disabled={busy} onClick={() => { void refresh(); }}>Rebuild now</Button>}>
        {err && <p className="pk-note lf-err" role="status">{err}</p>}
        {!brief && !err && <p className="pk-note">Reading your stores…</p>}
        {brief && <p className="pk-note">{brief.method}{brief.missing.length > 0 ? ` Not available right now: ${brief.missing.join(", ")}.` : ""}</p>}
      </Card>
      {brief && brief.sections.map((s) => <SectionCard key={s.id} s={s} />)}
      {brief && <Trajectories list={brief.trajectories} />}
      {brief && <TrainingCard t={brief.training} title="Training, last 7 days" />}
      {brief && <NeuroCard n={brief.neuro} title="Neurotechnology, last 7 days" />}
      <Card title="Across your domains" headingLevel={3}>
        <p className="pk-note">Each suggestion shows what was seen, the records and period behind it, what it may mean, how sure it is, what to try, and how it will be measured. Things that happened in the same period are not treated as cause and effect.</p>
        {recs === null ? null : recs.length === 0 ? <p className="pk-note">No cross-domain suggestions yet.</p> : null}
      </Card>
      {recs?.map((d) => (
        <RecommendationCard key={d.recommendation.recommendationId} rec={d.recommendation} writes={writes} heading={3} title={d.recommendation.domain === "cross" ? "Across domains" : "Amara's suggestion"} onChanged={() => { void api.recommendations("personal").then(setRecs).catch(() => undefined); }} />
      ))}
      <RuntimeNote runtime={runtime} job="personal" />
    </div>
  );
}
