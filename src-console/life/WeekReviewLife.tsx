import { useEffect, useState } from "react";
import type { Review_Serialize as Review } from "../bindings.gen";
import { addDays } from "../personal/adapters";
import { Card } from "../ui";
import type { Invoke } from "../lib/localStores";
import { RecommendationCard } from "./RecommendationCard";
import { WeekPlanCard } from "./WeekPlan";
import {
  dayLabel, goalWeeks, habitIsDue, habitPattern, isHabit, mondayOf, statusOf, weekCounts, weekLine, weekRange, weeklyTarget,
  type LifeData, type LifeWrites, type RecommendationRow, type WeekCount, type WeekPlan,
} from "./lifeApi";

/** The reviewed week's counts for one habit (Mon–Sun of the review, never days after it). */
function reviewWeek(days: LifeData["history"][string], target: number | null, monday: string): WeekCount {
  const end = addDays(monday, 6);
  return weekCounts(days, target, end, 1)[0];
}

/**
 * Life › Weekly review, the Life part (PLAN-0055 rules): the reporting dates, habits against their targets with unknown days
 * apart, goals that progressed or stalled (or have no data), sleep and money only where their source has data, up to 3
 * rule-made observations with their controls, and next week's plan.
 */
/** Decision 0062: `workout_week_summary`. */
type WorkoutWeek = { weekStart: string; sessions: number; completed: number; partiallyCompleted: number; fromAmara: number; doneSets: number; totalReps: number; totalHoldS: number; loadVolumeKg: number; activeS: number; source: string };

export function WeekReviewLife({ data, writes, call }: { data: LifeData; writes: LifeWrites | null; call: Invoke | null }) {
  const [review, setReview] = useState<Review | null | "failed">(null);
  const [obs, setObs] = useState<RecommendationRow[] | "failed" | null>(null);
  const [plan, setPlan] = useState<WeekPlan | null | "failed">(null);
  // Decision 0062: the week's workouts from the Workout engine (with its source).
  const [workouts, setWorkouts] = useState<WorkoutWeek | null | "failed">(null);
  useEffect(() => {
    if (!call) return;
    let live = true;
    call<Review>("weekly_review_read").then((r) => { if (live) setReview(r && r.weekStart ? r : "failed"); }, () => { if (live) setReview("failed"); });
    return () => { live = false; };
  }, [call]);
  // Fall back to the last full week (Mon–Sun before this one) when the review can't be read, and say so.
  const monday = review && review !== "failed" ? review.weekStart : addDays(mondayOf(data.today), -7);
  const end = addDays(monday, 6);
  const nextMonday = addDays(monday, 7);
  useEffect(() => {
    if (!writes || !review) return;
    let live = true;
    writes.weekObservations(monday).then((r) => { if (live) setObs(r.slice(0, 3)); }, () => { if (live) setObs("failed"); });
    call?.<WeekPlan>("life_week_plan_get", { weekStart: nextMonday }).then((p) => { if (live) setPlan(p); }, () => { if (live) setPlan("failed"); });
    call?.<WorkoutWeek>("workout_week_summary", { weekStart: monday }).then((w) => { if (live) setWorkouts(w); }, () => { if (live) setWorkouts("failed"); });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [monday, review === null]);
  const habits = data.goals.filter((h) => isHabit(h) && !["archived", "completed"].includes(statusOf(h)));
  const rows = habits.map((h) => ({ h, c: reviewWeek(data.history[h.id] ?? [], weeklyTarget(h), monday) }));
  const goals = goalWeeks(data.goals, data.activity, monday);
  const by = (s: string) => goals.filter((g) => g.state === s);
  const r = review && review !== "failed" ? review : null;
  const sleep = r?.health.state === "data" && r.health.nights > 0 ? r.health : null;
  const money = r && r.money.accounts.length > 0 ? r.money : null;
  const allUnknown = rows.filter((x) => x.c.done + x.c.notDone === 0);
  return (
    <div className="lf-review" aria-label="Life this week">
      <Card title="Reporting week" accessory={<span className="pk-acc"><span className="v-note">JST</span></span>}>
        <p className="lf-big"><b>{weekRange(monday)}</b></p>
        <p className="pk-note">Monday 00:00 to Sunday 24:00, Japan time{review === "failed" ? ". The review itself couldn't be read, so this is the last full week." : r ? `, the week this review covers (ready Sunday ${dayLabel(end)} 19:00).` : "."}</p>
      </Card>
      <div className="pk-g2">
        <Card title="Habits against their targets">
          {rows.length === 0 ? <p className="pk-note">No habits.</p> : (
            <ul className="lf-list" aria-label="Habits against their targets">
              {rows.map(({ h, c }) => (
                <li key={h.id}>{h.title}
                  <small>{c.done + c.notDone === 0 ? "No record this week: unknown, not zero" : weekLine(c)}{c.target == null ? " · no target set" : c.done >= c.target ? " · target met" : ""}</small>
                  <small>{habitPattern(weekCounts(data.history[h.id] ?? [], weeklyTarget(h), end))}</small>
                </li>
              ))}
            </ul>
          )}
          {allUnknown.length > 0 && <p className="pk-note">Missing, not zero: {allUnknown.map((x) => x.h.title).join(", ")} had no day recorded this week.</p>}
          {habits.some((h) => !habitIsDue(h)) && <p className="pk-note">A habit without a rhythm has no target to compare with. Set one in Habits &amp; routines.</p>}
        </Card>
        <Card title="Goals this week">
          {goals.length === 0 ? <p className="pk-note">No open goals.</p> : <>
            {by("progressed").length > 0 && <><h3 className="lf-h">Progressed</h3><ul className="lf-list">{by("progressed").map((g) => <li key={g.goal.id}>{g.goal.title}<small>{g.changes} progress update{g.changes === 1 ? "" : "s"} this week</small></li>)}</ul></>}
            {by("stalled").length > 0 && <><h3 className="lf-h">Stalled</h3><ul className="lf-list">{by("stalled").map((g) => <li key={g.goal.id}>{g.goal.title}<small>{g.last ? `last progress ${dayLabel(g.last)}` : "no progress recorded"} · deadline {g.goal.deadline ? dayLabel(g.goal.deadline.slice(0, 10)) : ""}</small></li>)}</ul></>}
            {by("steady").length > 0 && <><h3 className="lf-h">No change this week</h3><ul className="lf-list">{by("steady").map((g) => <li key={g.goal.id}>{g.goal.title}<small>{g.last ? `last progress ${dayLabel(g.last)}` : "activity, but no progress updates"}</small></li>)}</ul></>}
            {by("no_data").length > 0 && <><h3 className="lf-h">No data</h3><ul className="lf-list">{by("no_data").map((g) => <li key={g.goal.id}>{g.goal.title}<small>nothing recorded yet, so no change can be read</small></li>)}</ul></>}
            {data.failed.includes("activity_list") && <p className="pk-note lf-err">Goal activity couldn&apos;t be read, so progress isn&apos;t judged this week.</p>}
          </>}
        </Card>
      </div>
      {(sleep || money || workouts) && (
        <div className="pk-g2">
          {workouts && <Card title="Workouts"><p className="pk-note">{workouts === "failed" ? "Workouts couldn't be read just now."
            : workouts.sessions === 0 ? "No workouts logged in Workout this week."
            : `${workouts.sessions} workout${workouts.sessions === 1 ? "" : "s"} (${workouts.completed} completed${workouts.partiallyCompleted ? `, ${workouts.partiallyCompleted} partly` : ""}${workouts.fromAmara ? `, ${workouts.fromAmara} suggested by Amara` : ""}) · ${workouts.doneSets} sets · ${workouts.totalReps} reps${workouts.totalHoldS ? ` · ${workouts.totalHoldS} s of holds` : ""} · ${Math.round(workouts.activeS / 60)} min active. From the workouts you logged; NOOP-detected workouts are on the Fitness page.`}</p></Card>}
          {sleep && <Card title="Sleep (NOOP)"><p className="pk-note">{sleep.nights} night{sleep.nights === 1 ? "" : "s"} recorded{sleep.avgSleepH != null ? ` · average ${Math.round(sleep.avgSleepH * 10) / 10} h` : ""}{sleep.sleepDeltaH != null ? ` · ${sleep.sleepDeltaH >= 0 ? "+" : ""}${Math.round(sleep.sleepDeltaH * 10) / 10} h on last week` : ""}. Nights NOOP didn&apos;t record aren&apos;t counted.</p></Card>}
          {money && <Card title="Personal money"><p className="pk-note">{money.accounts.length} account{money.accounts.length === 1 ? "" : "s"} with entries{money.personalChangeUsd != null ? ` · change this week ${money.personalChangeUsd >= 0 ? "+" : "−"}$${Math.abs(Math.round(money.personalChangeUsd)).toLocaleString("en-US")}` : ""}. From your money entries only.</p></Card>}
        </div>
      )}
      <section className="lf-obs" aria-label="Suggested next actions">
        <h2 className="d18-h3">Suggested next actions</h2>
        {obs === null ? <p className="pk-note">{writes ? "Checking this week's records…" : "Suggestions appear in the app."}</p>
          : obs === "failed" ? <p className="pk-note lf-err">Observations couldn&apos;t be made just now.</p>
          : obs.length === 0 ? <p className="pk-note">Nothing to suggest from this week&apos;s records. Amara suggests only from what was recorded, at most 3 a week.</p>
          : obs.map((o) => <RecommendationCard key={o.recommendationId} rec={o} writes={writes!} title="Observation" heading={3} />)}
      </section>
      {plan !== "failed" && (
        <WeekPlanCard key={plan?.updatedAt ?? "new"} plan={plan} monday={nextMonday} goals={data.goals} writes={writes} title={nextMonday <= data.today ? "This week's plan" : "Next week's plan"} onSaved={setPlan} />
      )}
    </div>
  );
}
