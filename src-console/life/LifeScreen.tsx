import { useMemo, useState, type ReactNode } from "react";
import { goalOf, goalRoute, ventureRoute, type Route } from "../app/routes";
import { addAdapter } from "../adds/adapters";
import { Composer, LinkButton } from "../adds/Composer";
import { inTauri, realInvoke } from "../lib/localStores";
import { PersonalMoneyView } from "../personal/PageViews";
import { personalAdapter } from "../personal/adapters";
import { GoalsSection, WeeklyReviewSection } from "../personal/TodaySections";
import type { SystemSnapshot } from "../types";
import { Card } from "../ui";
import { DatesView } from "./DatesView";
import { GoalsProjects } from "./GoalsProjects";
import { HabitsView } from "./HabitsView";
import { lifeWrites, useLife, type LifeState } from "./lifeApi";
import { OverviewView } from "./OverviewView";
import { WeekReviewLife } from "./WeekReviewLife";
import { BriefingsView } from "./BriefingsView";
import "../adds/v1111.css";
import "./life.css";

/** Shown where a Life view needs the Mac's stores (browser preview, tests without a backend). */
function NotHere({ title }: { title: string }) {
  return <div className="v-page lf-page"><Card title={title}><p className="pk-note">This view reads the Life stores on this Mac. Nothing is shown rather than made up.</p></Card></div>;
}
const loadingCard = (title: string) => <div className="v-page lf-page"><Card title={title}><p className="pk-note">Reading your Life stores…</p></Card></div>;

/** design-1111v → PLAN-0055: the Life host. Overview · Goals & projects · Habits & routines · Weekly review · Briefings (0066) · Dates & deadlines · Personal money.
 *  The route id stays `life`; old links (trajectory, terrance:goals) still open Goals. */
export function LifeField({ data, view = "overview", onOpen, onRetry }: { data: SystemSnapshot; view?: string; onOpen?: (route: Route) => void; onRetry?: () => void }) {
  const personal = personalAdapter();
  const call = inTauri() ? realInvoke : null;
  const writes = useMemo(() => (call ? lifeWrites(call) : null), [call]);
  const { state } = useLife(call);
  const [added, setAdded] = useState<Array<{ id: string; title: string }>>([]);
  const ready = (s: LifeState) => (s.kind === "ready" ? s.data : null);
  const life = ready(state);
  const composer = (
    <Composer title="New habit" kind="Habit" adapter={addAdapter("goal")} placeholder="e.g. Read 20 minutes before bed, 5 nights a week"
      hint="Amara turns it into a goal and shows you a preview first. Voice is transcribed on this Mac." working="Amara is reading your goal…" saveLabel="Save habit" onOpen={onOpen}
      onSaved={(s) => setAdded((a) => [{ id: s.id, title: s.title }, ...a])} onUndone={(id) => setAdded((a) => a.filter((x) => x.id !== id))}
      savedText={(s) => `Added “${s.title}” to Habits & routines.`} />
  );
  const slot = (title: string, render: (d: NonNullable<typeof life>) => ReactNode) => (state.kind === "unwired" ? <NotHere title={title} /> : !life ? loadingCard(title) : render(life));

  if (view === "overview") return <div className="page-inset">{slot("Overview", (d) => <OverviewView data={d} writes={writes} call={call} onOpen={onOpen} />)}</div>;
  if (view === "habits") return <div className="page-inset">{slot("Habits & routines", (d) => <HabitsView data={d} writes={writes} composer={composer} onOpen={onOpen} />)}</div>;
  if (view === "weekly-review") return (
    <div className="page-inset">
      <WeeklyReviewSection adapter={personal} onOpenCheckIn={onOpen ? () => onOpen({ id: "terrance", view: "checkin" }) : undefined} onOpenGoal={onOpen ? (id) => onOpen(goalRoute(id)) : undefined} onOpenPatterns={onOpen ? () => onOpen({ id: "mindbody", view: "patterns" }) : undefined} />
      {life && <WeekReviewLife data={life} writes={writes} call={call} />}
    </div>
  );
  // 0066 (Taisun's design override): the daily briefing, summaries and cross-domain suggestions.
  if (view === "briefings") return <div className="page-inset"><BriefingsView call={call} writes={writes} /></div>;
  if (view === "dates") return <div className="page-inset"><DatesView adapter={personal} onOpen={onOpen} /></div>;
  // Only the mount point moved (HANDOFF-1111v §3): the 1111t money contract is unchanged.
  if (view === "money") return <PersonalMoneyView adapter={personal} onOpenFinances={onOpen ? () => onOpen({ id: "enterprise", view: "finances" }) : undefined} />;
  const focus = goalOf({ id: "life", view });
  return (
    <div className="page-inset">
      <GoalsSection adapter={personal} title="Goals & projects" goals={data.goals} goalsFailed={data.unavailableSources.some((s) => s.command === "read_goals")} onRetry={onRetry} composer={composer} added={added} focusGoal={focus}
        onOpenVenture={onOpen ? (id) => onOpen(ventureRoute(id)) : undefined}
        habitsMoved={<p className="pk-note">Habits have their own page now. {onOpen && <LinkButton small onClick={() => onOpen({ id: "life", view: "habits" })}>Habits &amp; routines ›</LinkButton>}</p>}>
        {life ? <GoalsProjects key={focus ?? "all"} data={life} writes={writes} focusGoal={focus} /> : null}
      </GoalsSection>
    </div>
  );
}
