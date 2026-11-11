import { useState } from "react";
import type { Route } from "../app/routes";
import { addAdapter } from "../adds/adapters";
import { Composer } from "../adds/Composer";
import { PersonalMoneyView } from "../personal/PageViews";
import { personalAdapter } from "../personal/adapters";
import { GoalsSection, WeeklyReviewSection } from "../personal/TodaySections";
import type { SystemSnapshot } from "../types";
import { DatesView } from "./DatesView";
import "../adds/v1111.css";

/** design-1111v: the Life host. Goals · Weekly review · Dates & deadlines · Personal money. */
export function LifeField({ data, view = "goals", onOpen, onRetry }: { data: SystemSnapshot; view?: string; onOpen?: (route: Route) => void; onRetry?: () => void }) {
  const personal = personalAdapter();
  const [added, setAdded] = useState<Array<{ id: string; title: string }>>([]);
  if (view === "weekly-review") return <div className="page-inset"><WeeklyReviewSection adapter={personal} onOpenCheckIn={onOpen ? () => onOpen({ id: "terrance", view: "checkin" }) : undefined} /></div>;
  if (view === "dates") return <div className="page-inset"><DatesView adapter={personal} onOpen={onOpen} /></div>;
  // Only the mount point moved (HANDOFF-1111v §3): the 1111t money contract is unchanged.
  if (view === "money") return <PersonalMoneyView adapter={personal} onOpenFinances={onOpen ? () => onOpen({ id: "enterprise", view: "finances" }) : undefined} />;
  const composer = (
    <Composer title="New goal or habit" kind="Habit" adapter={addAdapter("goal")} placeholder="e.g. Read 20 minutes before bed, 5 nights a week"
      hint="Amara turns it into a goal and shows you a preview first. Voice is transcribed on this Mac." working="Amara is reading your goal…" saveLabel="Save habit" onOpen={onOpen}
      onSaved={(s) => setAdded((a) => [{ id: s.id, title: s.title }, ...a])} onUndone={(id) => setAdded((a) => a.filter((x) => x.id !== id))}
      savedText={(s) => `Added “${s.title}” to This week's habits.`} />
  );
  return <div className="page-inset"><GoalsSection adapter={personal} goals={data.goals} goalsFailed={data.unavailableSources.some((s) => s.command === "read_goals")} onRetry={onRetry} composer={composer} added={added} /></div>;
}
