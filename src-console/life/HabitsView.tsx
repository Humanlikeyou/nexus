import { useState, type ReactNode } from "react";
import type { Route } from "../app/routes";
import { Button, Card, PageHero, Select, TextField, Toggle } from "../ui";
import { ReadyToKeepCard } from "../takes/WeekTakes";
import {
  daysOf, habitPattern, isHabit, mondayOf, statusOf, STATUS_LABEL, weekCounts, weekdayLabel, weekLine, weeklyTarget,
  type DayState, type Goal, type LifeData, type LifeWrites,
} from "./lifeApi";

const STATE_TEXT: Record<DayState, string> = { done: "done", not_done: "not done", no_record: "no record" };
const GLYPH: Record<DayState, string> = { done: "✓", not_done: "–", no_record: "?" };

/** One week, Mon–Sun. Each past or present day is a button: no record → done → not done → done. Future days are inert. */
function WeekCalendar({ habit, data, writes, onMsg }: { habit: Goal; data: LifeData; writes: LifeWrites | null; onMsg: (m: string) => void }) {
  const monday = mondayOf(data.today);
  const byDay = new Map((data.history[habit.id] ?? []).map((d) => [d.day, d.state as DayState]));
  const mark = async (day: string, state: DayState) => {
    if (!writes) return;
    const done = state !== "done";
    try { await writes.markHabit(habit.id, day, done); onMsg(`${habit.title}: ${weekdayLabel(day)} marked ${done ? "done" : "not done"}.`); }
    catch { onMsg("That mark didn't save. Nothing was changed."); }
  };
  return (
    <div className="lf-cal" role="group" aria-label={`${habit.title}, this week`}>
      {daysOf(monday, 7).map((day) => {
        const future = day > data.today;
        const s = byDay.get(day) ?? "no_record";
        return (
          <button key={day} type="button" className="lf-cal__day" data-s={future ? "future" : s} data-today={day === data.today || undefined} disabled={future || !writes}
            aria-label={`${weekdayLabel(day)} ${day.slice(5)}: ${future ? "later this week" : STATE_TEXT[s]}${future ? "" : `. Mark ${s === "done" ? "not done" : "done"}`}`}
            onClick={() => { void mark(day, s); }}>
            <small>{weekdayLabel(day)}</small><b aria-hidden="true">{future ? "" : GLYPH[s]}</b>
          </button>
        );
      })}
    </div>
  );
}

/** 12 weeks of done days against the target. A week's unknown days are drawn apart from "not done". */
function Trend({ habit, data }: { habit: Goal; data: LifeData }) {
  const target = weeklyTarget(habit);
  const counts = weekCounts(data.history[habit.id] ?? [], target, data.today);
  return (
    <div className="lf-trend">
      <ol className="lf-trend__bars" aria-label={`${habit.title}, last 12 weeks`}>
        {counts.map((c) => (
          <li key={c.monday} title={`Week of ${c.monday}: ${weekLine(c)}`} aria-label={`Week of ${c.monday}: ${weekLine(c)}`}>
            <span className="lf-trend__col">
              <i className="u" style={{ height: `${(c.unknown / 7) * 100}%` }} />
              <i className="n" style={{ height: `${(c.notDone / 7) * 100}%` }} />
              <i className="d" style={{ height: `${(c.done / 7) * 100}%` }} />
              {target != null && <em style={{ bottom: `${(target / 7) * 100}%` }} />}
            </span>
          </li>
        ))}
      </ol>
      <p className="pk-note">{habitPattern(counts)} Pale bars are days with no record: unknown, not missed.</p>
    </div>
  );
}

const PER = [{ value: "week", label: "times a week" }, { value: "day", label: "times a day" }];
const STATUSES = ["active", "paused", "completed", "archived"].map((s) => ({ value: s, label: STATUS_LABEL[s] }));

/** The habit's Life fields (`life_update`): rhythm, preferred time, cue, minimum and full versions, reminders, status. */
export function HabitFields({ habit, writes, onDone }: { habit: Goal; writes: LifeWrites; onDone: (msg: string) => void }) {
  const l = habit.life;
  const [times, setTimes] = useState(String(l?.frequency?.times ?? ""));
  const [per, setPer] = useState(l?.frequency?.per ?? "week");
  const [time, setTime] = useState(l?.preferredTime ?? "");
  const [cue, setCue] = useState(l?.cue ?? "");
  const [min, setMin] = useState(l?.minimumVersion ?? "");
  const [full, setFull] = useState(l?.fullVersion ?? "");
  const [reminders, setReminders] = useState(l?.reminders ?? true);
  const [status, setStatus] = useState(statusOf(habit));
  const [err, setErr] = useState<string | null>(null);
  const submit = async () => {
    const n = times.trim() === "" ? null : Number(times);
    if (n != null && (!Number.isInteger(n) || n < 1 || n > (per === "day" ? 24 : 7))) { setErr(per === "day" ? "Times a day is 1 to 24." : "Times a week is 1 to 7."); return; }
    if (time && !/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) { setErr("Preferred time is HH:MM."); return; }
    try {
      await writes.updateLife(habit.id, { frequency: n == null ? null : { times: n, per }, preferredTime: time || null, cue: cue.trim() || null, minimumVersion: min.trim() || null, fullVersion: full.trim() || null, reminders, status });
      onDone(`Saved “${habit.title}”.`);
    } catch (e) { setErr(typeof e === "string" && e.length < 140 ? `Not saved: ${e}.` : "That didn't save. Nothing was changed."); }
  };
  return (
    <form className="ci-form lf-fields" aria-label={`Edit ${habit.title}`} onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <div className="pk-g2">
        <div className="pk-g2">
          <TextField label="How often" type="number" min={1} max={per === "day" ? 24 : 7} value={times} placeholder="e.g. 5" onChange={(e) => setTimes(e.target.value)} />
          <Select label="Per" options={PER} value={per} onChange={(e) => setPer(e.target.value)} />
        </div>
        <TextField label="Preferred time" type="time" value={time} onChange={(e) => setTime(e.target.value)} />
      </div>
      <div className="pk-g2">
        <TextField label="Cue (after…)" value={cue} maxLength={200} placeholder="After my morning coffee" onChange={(e) => setCue(e.target.value)} />
        <Select label="Status" options={STATUSES} value={status} onChange={(e) => setStatus(e.target.value)} />
      </div>
      <div className="pk-g2">
        <TextField label="Minimum version" value={min} maxLength={200} placeholder="On a hard day: one minute" onChange={(e) => setMin(e.target.value)} />
        <TextField label="Full version" value={full} maxLength={200} onChange={(e) => setFull(e.target.value)} />
      </div>
      <span className="lf-toggle"><Toggle aria-label="Reminders" checked={reminders} onCheckedChange={setReminders} /> Reminders</span>
      {err && <p className="pk-note lf-err" role="alert">{err}</p>}
      <div className="pk-actions"><Button size="small" variant="primary" type="submit">Save habit</Button><Button size="small" variant="plain" onClick={() => onDone("")}>Cancel</Button></div>
    </form>
  );
}

/** Life › Habits & routines (the habit UI moved here from Goals): record today, the week calendar, 12-week trends and each habit's rhythm. */
export function HabitsView({ data, writes, composer, onOpen }: { data: LifeData; writes: LifeWrites | null; composer?: ReactNode; onOpen?: (route: Route) => void }) {
  const [msg, setMsg] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [showClosed, setShowClosed] = useState(false);
  const all = data.goals.filter(isHabit);
  const habits = all.filter((h) => showClosed || !["archived", "completed"].includes(statusOf(h)));
  const closed = all.length - all.filter((h) => !["archived", "completed"].includes(statusOf(h))).length;
  const linkedGoals = (h: Goal) => Object.entries(data.links).flatMap(([gid, ls]) => (ls.some((l) => l.kind === "goal_habit" && l.otherRef === `goals:${h.id}`) ? [data.goals.find((g) => g.id === gid)?.title ?? "A goal"] : []));
  const thisWeek = (h: Goal) => weekCounts(data.history[h.id] ?? [], weeklyTarget(h), data.today, 1)[0];
  return (
    <div className="v-page lf-page" id="habits">
      <PageHero title="Habits & routines" statement={habits.length ? `${habits.length} habit${habits.length === 1 ? "" : "s"}` : "Small things, often"}
        detail="Mark a day done or not done. A day you didn't mark stays unknown: it never counts as missed." />
      {composer}
      {msg && <p className="pk-note" role="status">{msg}</p>}
      {data.failed.includes("read_goals") ? <Card title="Habits this week"><p className="pk-note lf-err">Habits couldn&apos;t be read just now.</p></Card>
        : habits.length === 0 ? <Card title="Habits this week"><p className="pk-note">No habits yet. Add one above.</p></Card> : (
          <section className="lf-habits" aria-label="Habits this week">
            {habits.map((h) => {
              const wk = thisWeek(h);
              const goals = linkedGoals(h);
              const failed = data.failed.includes("life_habit_history");
              return (
                <Card key={h.id} title={h.title} headingLevel={3} className="lf-habit"
                  accessory={<span className="pk-acc"><span className="v-note">{STATUS_LABEL[statusOf(h)]}{h.life?.frequency ? ` · ${h.life.frequency.times}× a ${h.life.frequency.per}` : " · no rhythm set"}</span></span>}>
                  <p className="pk-note">{failed ? "Its history couldn't be read, so nothing is shown rather than guessed." : `This week: ${weekLine(wk)}`}
                    {h.life?.cue ? ` · after ${h.life.cue}` : ""}{h.life?.preferredTime ? ` · around ${h.life.preferredTime}` : ""}{goals.length ? ` · serves ${goals.join(", ")}` : ""}</p>
                  {!failed && <WeekCalendar habit={h} data={data} writes={writes} onMsg={setMsg} />}
                  {!failed && <Trend habit={h} data={data} />}
                  {h.life?.minimumVersion && <p className="pk-note"><span className="lf-k">Minimum</span> {h.life.minimumVersion}</p>}
                  {editing === h.id && writes ? <HabitFields habit={h} writes={writes} onDone={(m) => { setEditing(null); if (m) setMsg(m); }} />
                    : <div className="pk-actions"><Button size="small" variant="plain" disabled={!writes} onClick={() => setEditing(h.id)} aria-label={`Edit rhythm: ${h.title}`}>Edit rhythm</Button>
                      {onOpen && goals.length === 0 && <Button size="small" variant="plain" onClick={() => onOpen({ id: "life", view: "goals" })}>Link to a goal ›</Button>}</div>}
                </Card>
              );
            })}
          </section>
        )}
      {closed > 0 && <Button size="small" variant="plain" onClick={() => setShowClosed((v) => !v)}>{showClosed ? "Hide archived and completed" : `Show ${closed} archived or completed`}</Button>}
      <ReadyToKeepCard title="Ready to become a habit" />
    </div>
  );
}
