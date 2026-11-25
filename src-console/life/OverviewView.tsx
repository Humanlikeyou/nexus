import { useEffect, useState } from "react";
import { goalRoute, type Route } from "../app/routes";
import { useStoresTick, type Invoke } from "../lib/localStores";
import { valueOf } from "../lib/sampleAdapters";
import { roadmapAdapter, type Roadmap } from "../goals/roadmap";
import { personalAdapter } from "../personal/adapters";
import { Button, Card, PageHero, StatusText, TextField, Toggle, InsetGroup, ListRow } from "../ui";
import { LinkButton } from "../adds/Composer";
import { RecommendationCard } from "./RecommendationCard";
import { WeekPlanCard } from "./WeekPlan";
import {
  dayLabel, daysUntil, habitIsDue, isHabit, mondayOf, personalTasks, progressText, statusOf,
  type Goal, type LifeData, type LifeInsight, type LifeWrites,
} from "./lifeApi";

export type RoadmapOf = (goalId: string) => Roadmap | null;
const defaultRoadmap: RoadmapOf = (id) => valueOf(roadmapAdapter().roadmap(id)) ?? null;

/** The goal Overview calls "current focus": the week's priority goal if set, else the active goal with the nearest deadline. */
export function currentFocus(d: LifeData): { goal: Goal | null; why: string } {
  const open = d.goals.filter((g) => !isHabit(g) && statusOf(g) === "active");
  const pick = d.plan?.priorityGoalId ? d.goals.find((g) => g.id === d.plan?.priorityGoalId) : undefined;
  if (pick) return { goal: pick, why: "This week's priority" };
  const dated = open.filter((g) => g.deadline).sort((a, b) => (a.deadline ?? "").localeCompare(b.deadline ?? ""));
  if (dated[0]) return { goal: dated[0], why: `Nearest deadline · ${dayLabel(dated[0].deadline!.slice(0, 10))}` };
  return { goal: null, why: "" };
}

/** Upcoming deadlines: Life dates not done (today or later) plus goal deadlines, soonest first. */
export function upcoming(d: LifeData, n = 6) {
  const rows = [
    ...d.dates.filter((x) => !x.done && x.inDays >= 0).map((x) => ({ id: `d-${x.id}`, title: x.title, iso: x.date, inDays: x.inDays, kind: "Date" as const, goalId: null as string | null })),
    ...d.goals.filter((g) => !isHabit(g) && g.deadline && !["completed", "archived"].includes(statusOf(g))).map((g) => ({ id: `g-${g.id}`, title: g.title, iso: g.deadline!.slice(0, 10), inDays: daysUntil(g.deadline, d.today) ?? 0, kind: "Goal" as const, goalId: g.id }))
      .filter((x) => x.inDays >= 0),
  ];
  return rows.sort((a, b) => a.inDays - b.inDays).slice(0, n);
}

/** Today's planned actions: unticked plan actions, each active goal's next action, and active personal tasks (TASKS.md). */
export function todaysActions(d: LifeData) {
  const plan = (d.plan?.actions ?? []).filter((a) => !a.done).map((a, i) => ({ id: `p-${i}`, text: a.text, from: "This week's plan" }));
  const next = d.goals.filter((g) => !isHabit(g) && statusOf(g) === "active" && g.life?.nextAction).map((g) => ({ id: `n-${g.id}`, text: g.life!.nextAction!, from: g.title }));
  const tasks = personalTasks(d.tasks).filter((t) => t.status === "active").map((t, i) => ({ id: `t-${i}`, text: t.title, from: "Tasks" }));
  return [...plan, ...next, ...tasks];
}

const HELD: Record<string, string> = {
  quiet_hours: "Quiet hours are on, so Amara is holding new suggestions until they end.",
  daily_limit: "You've had today's suggestions. More come tomorrow if the evidence holds.",
  off: "Suggestions are off.",
  none: "Nothing well-supported to suggest right now. Amara only suggests from measured or self-recorded evidence.",
};

/** Life › Overview: one read of everything, each item from its own source (PLAN-0055 table). */
export function OverviewView({ data, writes, call, onOpen, roadmapOf = defaultRoadmap }: {
  data: LifeData; writes: LifeWrites | null; call: Invoke | null; onOpen?: (route: Route) => void; roadmapOf?: RoadmapOf;
}) {
  useStoresTick();
  const [insight, setInsight] = useState<LifeInsight | "failed" | null>(null);
  const [habitMsg, setHabitMsg] = useState<string | null>(null);
  useEffect(() => {
    if (!writes) return;
    let live = true;
    writes.insight().then((r) => { if (live) setInsight(r); }, () => { if (live) setInsight("failed"); });
    return () => { live = false; };
    // One read when Overview opens: reading may bring a suggestion up, so it is not repeated on every re-render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const focus = currentFocus(data);
  const monday = mondayOf(data.today);
  const actions = todaysActions(data);
  const due = data.goals.filter(habitIsDue);
  const goals = data.goals.filter((g) => !isHabit(g) && statusOf(g) === "active");
  const milestones = goals.map((g) => ({ g, step: roadmapOf(g.id)?.steps.find((s) => !s.done) ?? null })).filter((x) => x.step);
  const deadlines = upcoming(data);
  const money = valueOf(personalAdapter().money());
  const todayState = (h: Goal) => data.history[h.id]?.find((x) => x.day === data.today)?.state ?? "no_record";
  const mark = async (h: Goal, done: boolean) => {
    if (!writes) return;
    setHabitMsg(null);
    try { await writes.markHabit(h.id, data.today, done); setHabitMsg(done ? `Marked “${h.title}” done today.` : `Marked “${h.title}” not done today.`); }
    catch { setHabitMsg("That mark didn't save. Nothing was changed."); }
  };
  const open = (id: string) => onOpen?.(goalRoute(id));
  return (
    <div className="v-page lf-page" id="overview">
      <PageHero title="Overview" statement={focus.goal ? `Focus: ${focus.goal.title}` : data.plan?.priority ? `Priority: ${data.plan.priority}` : "Your life, at a glance"}
        detail={<>{goals.length} active goal{goals.length === 1 ? "" : "s"} · {due.length} habit{due.length === 1 ? "" : "s"} with a rhythm · {deadlines.length ? `next deadline ${deadlines[0].title} in ${deadlines[0].inDays} days` : "no deadlines coming up"}</>} />
      {data.failed.length > 0 && <p className="pk-note lf-err" role="status">Some Life sources couldn&apos;t be read just now ({data.failed.join(", ")}). What they would show is left out, not guessed.</p>}
      <div className="pk-g2">
        <Card title="Current focus">
          {focus.goal ? <>
            <p className="lf-big"><b>{focus.goal.title}</b></p>
            <p className="pk-note">{focus.why} · {progressText(data.progress[focus.goal.id], focus.goal)}</p>
            {onOpen && <LinkButton small onClick={() => open(focus.goal!.id)} label={`Open goal: ${focus.goal.title}`}>Open goal ›</LinkButton>}
          </> : <p className="pk-note">No active goal with a deadline, and no weekly priority goal. Set one in the plan.</p>}
        </Card>
        <WeekPlanCard plan={data.plan} monday={monday} goals={data.goals} writes={writes} title="Primary weekly priority" />
      </div>
      <div className="pk-g2">
        <Card title="Today's planned actions">
          {actions.length === 0 ? <p className="pk-note">Nothing planned for today. Plan actions in this week&apos;s plan or give a goal a next action.</p> : (
            <ul className="lf-list" aria-label="Today's planned actions">{actions.map((a) => <li key={a.id}>{a.text}<small>{a.from}</small></li>)}</ul>
          )}
        </Card>
        <Card title="Habits due today">
          {due.length === 0 ? <p className="pk-note">No habit with a rhythm yet. Give one a frequency in Habits &amp; routines.</p> : (
            <ul className="lf-list" aria-label="Habits due today">
              {due.map((h) => {
                const s = todayState(h);
                return (
                  <li key={h.id} className="lf-row">
                    <span>{h.title}<small>{s === "done" ? "Done today" : s === "not_done" ? "Marked not done" : "Not recorded yet"}</small></span>
                    <span className="lf-row__end">
                      {s !== "done" && <Button size="small" variant="secondary" disabled={!writes} onClick={() => { void mark(h, true); }} aria-label={`Done today: ${h.title}`}>Done</Button>}
                      {s !== "not_done" && <Button size="small" variant="plain" disabled={!writes} onClick={() => { void mark(h, false); }} aria-label={`Not today: ${h.title}`}>Not today</Button>}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
          {habitMsg && <p className="pk-note" role="status">{habitMsg}</p>}
        </Card>
      </div>
      <div className="pk-g2">
        <Card title="Goal progress">
          {goals.length === 0 ? <p className="pk-note">No active goals.</p> : (
            <ul className="lf-list" aria-label="Goal progress">
              {goals.map((g) => {
                const p = data.progress[g.id];
                return (
                  <li key={g.id}>
                    <button type="button" className="v-link" onClick={() => open(g.id)} disabled={!onOpen}>{g.title}</button>
                    <small>{progressText(p, g)}</small>
                    {p?.fraction != null && <span className="lf-bar" role="meter" aria-label={`${g.title} progress`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(p.fraction * 100)}><i style={{ width: `${Math.max(0, Math.min(100, p.fraction * 100))}%` }} /></span>}
                  </li>
                );
              })}
            </ul>
          )}
        </Card>
        <Card title="Next milestones">
          {milestones.length === 0 ? <p className="pk-note">No open roadmap steps. A goal&apos;s steps appear once its route is planned.</p> : (
            <ul className="lf-list" aria-label="Next milestones">{milestones.map(({ g, step }) => <li key={g.id}>{step!.title}<small>{g.title}</small></li>)}</ul>
          )}
        </Card>
      </div>
      <div className="pk-g2">
        <Card title="Upcoming deadlines">
          {deadlines.length === 0 ? <p className="pk-note">No dates or goal deadlines coming up.</p> : (
            <ul className="lf-list" aria-label="Upcoming deadlines">{deadlines.map((x) => <li key={x.id}>{x.title}<small>{x.kind} · {dayLabel(x.iso)} · in {x.inDays} day{x.inDays === 1 ? "" : "s"}</small></li>)}</ul>
          )}
          {onOpen && <LinkButton small onClick={() => onOpen({ id: "life", view: "dates" })}>Dates &amp; deadlines ›</LinkButton>}
        </Card>
        {insight && insight !== "failed" && insight.recommendation ? (
          <RecommendationCard rec={insight.recommendation} writes={writes!} title="Amara's insight" />
        ) : (
          <Card title="Amara's insight">
            <p className="pk-note">{insight === "failed" ? "Suggestions couldn't be read just now." : insight ? HELD[insight.held ?? "none"] ?? HELD.none : writes ? "Reading…" : HELD.none}</p>
          </Card>
        )}
      </div>
      {money && onOpen && <p className="pk-note">Personal money has entries for {money.month}. <LinkButton small onClick={() => onOpen({ id: "life", view: "money" })}>Personal money ›</LinkButton></p>}
      {call && <NudgeSettings call={call} />}
    </div>
  );
}

type Nudges = { enabled: boolean; telegram: boolean; maxPerDay: number; quietFrom: string; quietTo: string };
type Settings = { nudges: Nudges } & Record<string, unknown>;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;

/** How often Amara may bring a suggestion up (ProactiveSettings.nudges). Saves the whole settings object back, so check-in settings are kept. */
export function NudgeSettings({ call }: { call: Invoke }) {
  const [s, setS] = useState<Settings | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [times, setTimes] = useState<{ quietFrom: string; quietTo: string; max: string } | null>(null);
  useEffect(() => {
    let live = true;
    call<Settings>("proactive_settings_get").then((v) => { if (live && v?.nudges) { setS(v); setTimes({ quietFrom: v.nudges.quietFrom, quietTo: v.nudges.quietTo, max: String(v.nudges.maxPerDay) }); } }, () => { if (live) setErr("Suggestion settings couldn't be read."); });
    return () => { live = false; };
  }, [call]);
  const save = async (patch: Partial<Nudges>) => {
    if (!s) return;
    setErr(null);
    try { const v = await call<Settings>("proactive_settings_set", { settings: { ...s, nudges: { ...s.nudges, ...patch } } }); setS(v); setTimes({ quietFrom: v.nudges.quietFrom, quietTo: v.nudges.quietTo, max: String(v.nudges.maxPerDay) }); }
    catch { setErr("Couldn't save that. Try again."); }
  };
  const n = s?.nudges;
  return (
    <InsetGroup title="Suggestions from Amara" footer={err ?? "Amara only suggests from measured or self-recorded evidence, never repeats one, and stays quiet for 30 days on anything like a suggestion you dismissed. Telegram messages hold while outbound sending is off."}>
      <ListRow title="Suggestions" subtitle="Bring one up in Overview when the evidence supports it"
        accessory={<Toggle aria-label="Suggestions" checked={n?.enabled ?? false} disabled={!n} onCheckedChange={(v) => { void save({ enabled: v }); }} />} />
      <ListRow title="Also on Telegram" subtitle="Sends the suggestion's title and action"
        accessory={<Toggle aria-label="Also on Telegram" checked={n?.telegram ?? false} disabled={!n} onCheckedChange={(v) => { void save({ telegram: v }); }} />} />
      <ListRow title="At most a day" subtitle="App and Telegram together"
        accessory={<TextField label="Suggestions a day" hideLabel type="number" min={0} max={10} value={times?.max ?? "2"} disabled={!n}
          onChange={(e) => { const v = e.target.value; setTimes((t) => (t ? { ...t, max: v } : t)); const k = Number(v); if (Number.isInteger(k) && k >= 0 && k <= 10 && k !== n?.maxPerDay) void save({ maxPerDay: k }); }} />} />
      <ListRow title="Quiet from" subtitle="Japan time"
        accessory={<TextField label="Quiet from" hideLabel type="time" value={times?.quietFrom ?? "22:00"} disabled={!n}
          onChange={(e) => { const v = e.target.value; setTimes((t) => (t ? { ...t, quietFrom: v } : t)); if (TIME.test(v) && v !== n?.quietFrom) void save({ quietFrom: v }); }} />} />
      <ListRow title="Quiet until" subtitle="Japan time"
        accessory={<TextField label="Quiet until" hideLabel type="time" value={times?.quietTo ?? "07:00"} disabled={!n}
          onChange={(e) => { const v = e.target.value; setTimes((t) => (t ? { ...t, quietTo: v } : t)); if (TIME.test(v) && v !== n?.quietTo) void save({ quietTo: v }); }} />} />
      {!n && !err && <ListRow title="Reading settings…" accessory={<StatusText kind="wait" />} />}
    </InsetGroup>
  );
}
