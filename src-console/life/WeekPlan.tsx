import { useState } from "react";
import { Button, Card, Select, TextField } from "../ui";
import { isHabit, statusOf, weekRange, type Goal, type LifeWrites, type WeekPlan } from "./lifeApi";

const MAX_ACTIONS = 5;
type Action = { text: string; goalId: string | null; done: boolean };

/** The plan for one week (`life_week_plan_get/set`): the primary priority, its goal, up to 5 planned actions and a note.
 *  Read mode lists them with a tick per action; Edit opens the form. A week never planned says so. */
export function WeekPlanCard({ plan, monday, goals, writes, title = "This week's plan", onSaved }: {
  plan: WeekPlan | null; monday: string; goals: Goal[]; writes: LifeWrites | null; title?: string; onSaved?: (p: WeekPlan) => void;
}) {
  const base: WeekPlan = plan ?? { weekStart: monday, priority: null, priorityGoalId: null, actions: [], note: null, updatedAt: null };
  const [editing, setEditing] = useState(false);
  const [priority, setPriority] = useState(base.priority ?? "");
  const [goalId, setGoalId] = useState(base.priorityGoalId ?? "");
  const [actions, setActions] = useState<Action[]>(base.actions.map((a) => ({ text: a.text, goalId: a.goalId, done: a.done })));
  const [note, setNote] = useState(base.note ?? "");
  const [msg, setMsg] = useState<{ text: string; error?: boolean } | null>(null);
  const open = goals.filter((g) => !isHabit(g) && !["completed", "archived"].includes(statusOf(g)));
  const options = [{ value: "", label: "No goal" }, ...open.map((g) => ({ value: g.id, label: g.title }))];
  const goalName = (id: string | null) => (id ? goals.find((g) => g.id === id)?.title : undefined);
  const reset = () => { setPriority(base.priority ?? ""); setGoalId(base.priorityGoalId ?? ""); setActions(base.actions.map((a) => ({ text: a.text, goalId: a.goalId, done: a.done }))); setNote(base.note ?? ""); };
  const save = async (next: WeekPlan, ok: string) => {
    if (!writes) return;
    setMsg(null);
    try { const saved = await writes.savePlan(next); onSaved?.(saved); setMsg({ text: ok }); return true; }
    catch (e) { setMsg({ text: typeof e === "string" && e.length < 120 ? `Not saved: ${e}.` : "That plan didn't save. Nothing was changed.", error: true }); return false; }
  };
  const submit = async () => {
    const kept = actions.map((a) => ({ ...a, text: a.text.trim() })).filter((a) => a.text);
    const ok = await save({ ...base, weekStart: monday, priority: priority.trim() || null, priorityGoalId: goalId || null, actions: kept, note: note.trim() || null }, "Plan saved.");
    if (ok) setEditing(false);
  };
  const tick = (i: number) => {
    const next = base.actions.map((a, k) => (k === i ? { ...a, done: !a.done } : a));
    void save({ ...base, actions: next }, next[i].done ? `Ticked “${next[i].text}”.` : `Unticked “${next[i].text}”.`);
  };
  return (
    <Card title={title} className="lf-plan" accessory={<span className="pk-acc"><span className="v-note">{weekRange(monday)}</span></span>}>
      {!editing ? (
        <>
          {base.priority ? <p className="lf-plan__priority"><span className="lf-k">Priority</span> <b>{base.priority}</b>{goalName(base.priorityGoalId) && <small> · {goalName(base.priorityGoalId)}</small>}</p>
            : <p className="pk-note">No priority set for this week yet.</p>}
          {base.actions.length > 0 && (
            <ul className="lf-checks" aria-label="Planned actions">
              {base.actions.map((a, i) => (
                <li key={`${i}-${a.text}`}>
                  <label><input type="checkbox" checked={a.done} disabled={!writes} onChange={() => tick(i)} /> <span data-done={a.done || undefined}>{a.text}</span></label>
                  {goalName(a.goalId) && <small>{goalName(a.goalId)}</small>}
                </li>
              ))}
            </ul>
          )}
          {base.note && <p className="pk-note">{base.note}</p>}
          <div className="pk-actions"><Button size="small" variant="secondary" disabled={!writes} onClick={() => { reset(); setEditing(true); setMsg(null); }}>{base.updatedAt ? "Edit plan" : "Plan this week"}</Button></div>
        </>
      ) : (
        <form className="ci-form" aria-label={`Plan for ${weekRange(monday)}`} onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <div className="pk-g2">
            <TextField label="Primary priority" value={priority} maxLength={200} placeholder="The one thing that matters most this week" onChange={(e) => setPriority(e.target.value)} />
            <Select label="It moves" options={options} value={goalId} onChange={(e) => setGoalId(e.target.value)} />
          </div>
          {actions.map((a, i) => (
            <div className="pk-g2 lf-plan__row" key={i}>
              <TextField label={`Action ${i + 1}`} value={a.text} maxLength={200} onChange={(e) => setActions((x) => x.map((y, k) => (k === i ? { ...y, text: e.target.value } : y)))} />
              <div className="lf-plan__rowEnd">
                <Select label={`Goal for action ${i + 1}`} options={options} value={a.goalId ?? ""} onChange={(e) => setActions((x) => x.map((y, k) => (k === i ? { ...y, goalId: e.target.value || null } : y)))} />
                <Button size="small" variant="plain" aria-label={`Remove action ${i + 1}`} onClick={() => setActions((x) => x.filter((_, k) => k !== i))}>Remove</Button>
              </div>
            </div>
          ))}
          {actions.length < MAX_ACTIONS && <Button size="small" variant="plain" onClick={() => setActions((x) => [...x, { text: "", goalId: null, done: false }])}>Add an action</Button>}
          <TextField label="Note" value={note} maxLength={1000} onChange={(e) => setNote(e.target.value)} />
          <div className="pk-actions"><Button size="small" variant="primary" type="submit">Save plan</Button><Button size="small" variant="plain" onClick={() => { setEditing(false); reset(); }}>Cancel</Button></div>
        </form>
      )}
      {msg && <p className={msg.error ? "pk-note lf-err" : "pk-note"} role="status">{msg.text}</p>}
    </Card>
  );
}
