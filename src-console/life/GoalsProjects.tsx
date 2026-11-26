import { useState } from "react";
import { useStoresTick } from "../lib/localStores";
import { valueOf } from "../lib/sampleAdapters";
import { roadmapAdapter, type RoadmapAdapter } from "../goals/roadmap";
import { Button, Card, Select, TextField } from "../ui";
import {
  dayLabel, goalTree, isHabit, progressText, statusOf, STATUS_LABEL, tasksForProject,
  type Goal, type GoalNode, type LifeData, type LifeWrites,
} from "./lifeApi";

const TYPES = [{ value: "", label: "Not set" }, { value: "outcome", label: "Outcome" }, { value: "performance", label: "Performance" }, { value: "learning", label: "Learning" }];
const STATUSES = ["active", "paused", "blocked", "completed", "archived"].map((s) => ({ value: s, label: STATUS_LABEL[s] }));
const errText = (e: unknown) => (typeof e === "string" && e.length < 140 ? `Not saved: ${e}.` : "That didn't save. Nothing was changed.");

/** A goal's Life fields (`life_update`). Blocked needs a reason (the backend refuses it otherwise). */
export function GoalFields({ goal, data, writes, onDone }: { goal: Goal; data: LifeData; writes: LifeWrites; onDone: (msg: string) => void }) {
  const l = goal.life;
  const [f, setF] = useState({
    goalType: l?.goalType ?? "", status: statusOf(goal), statusReason: l?.statusReason ?? "", areaId: l?.areaId ?? "", motivation: l?.motivation ?? "",
    baseline: l?.baseline == null ? "" : String(l.baseline), desiredResult: l?.desiredResult ?? "", measurement: l?.measurement ?? "",
    completionCriteria: l?.completionCriteria ?? "", nextAction: l?.nextAction ?? "",
  });
  const [err, setErr] = useState<string | null>(null);
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }));
  const areas = [{ value: "", label: "No area" }, ...data.areas.filter((a) => a.status === "active").map((a) => ({ value: a.id, label: a.name }))];
  const submit = async () => {
    const baseline = f.baseline.trim() === "" ? null : Number(f.baseline);
    if (baseline != null && !Number.isFinite(baseline)) { setErr("Baseline is a number in the goal's unit."); return; }
    if (f.status === "blocked" && !f.statusReason.trim()) { setErr("Say what it's blocked by."); return; }
    const t = (v: string) => v.trim() || null;
    try {
      await writes.updateLife(goal.id, { goalType: t(f.goalType), status: f.status, statusReason: t(f.statusReason), areaId: t(f.areaId), motivation: t(f.motivation), baseline,
        desiredResult: t(f.desiredResult), measurement: t(f.measurement), completionCriteria: t(f.completionCriteria), nextAction: t(f.nextAction) });
      onDone(`Saved “${goal.title}”.`);
    } catch (e) { setErr(errText(e)); }
  };
  return (
    <form className="ci-form lf-fields" aria-label={`Life details for ${goal.title}`} onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <div className="pk-g2"><Select label="Type" options={TYPES} value={f.goalType} onChange={set("goalType")} /><Select label="Area" options={areas} value={f.areaId} onChange={set("areaId")} /></div>
      <div className="pk-g2"><Select label="Status" options={STATUSES} value={f.status} onChange={set("status")} /><TextField label="Why (paused or blocked)" value={f.statusReason} maxLength={300} onChange={set("statusReason")} /></div>
      <TextField label="Why it matters" value={f.motivation} maxLength={500} onChange={set("motivation")} />
      <div className="pk-g2"><TextField label={`Baseline (${goal.unit || "its unit"})`} value={f.baseline} inputMode="decimal" onChange={set("baseline")} /><TextField label="Desired result" value={f.desiredResult} maxLength={300} onChange={set("desiredResult")} /></div>
      <div className="pk-g2"><TextField label="How it's measured" value={f.measurement} maxLength={300} onChange={set("measurement")} /><TextField label="Done when" value={f.completionCriteria} maxLength={300} onChange={set("completionCriteria")} /></div>
      <TextField label="Next action" value={f.nextAction} maxLength={200} onChange={set("nextAction")} />
      <p className="pk-note">A percentage appears only once a baseline is set: (current − baseline) ÷ (target − baseline).</p>
      {err && <p className="pk-note lf-err" role="alert">{err}</p>}
      <div className="pk-actions"><Button size="small" variant="primary" type="submit">Save details</Button><Button size="small" variant="plain" onClick={() => onDone("")}>Cancel</Button></div>
    </form>
  );
}

/** Roadmap steps as a timeline: each step is a stop you can open; an open, undone step can be answered Done or Not yet. */
export function MilestoneTimeline({ goal, roadmaps, onMsg }: { goal: Goal; roadmaps: RoadmapAdapter; onMsg: (m: string) => void }) {
  useStoresTick();
  const rm = valueOf(roadmaps.roadmap(goal.id));
  const [sel, setSel] = useState<string | null>(null);
  if (!rm || rm.steps.length === 0) return <p className="pk-note">{rm?.planNote ?? "No milestones yet. Amara plans the route on the Goals map, or you add steps there."}</p>;
  const steps = [...rm.steps].sort((a, b) => a.order - b.order);
  const current = steps.find((s) => !s.done)?.id ?? null;
  const open = steps.find((s) => s.id === (sel ?? current));
  const answer = async (stepId: string, a: "yes" | "not_yet", title: string) => {
    const r = await roadmaps.answerStep(goal.id, stepId, a);
    onMsg(r.state === "unwired" ? "That answer didn't save: the roadmap store isn't answering." : a === "yes" ? `Done: “${title}”.` : `Noted: not yet on “${title}”.`);
  };
  return (
    <div className="lf-tl">
      <ol className="lf-tl__track" aria-label={`${goal.title} milestones, ${rm.pct}% of the route done`}>
        {steps.map((s, i) => (
          <li key={s.id} data-done={s.done || undefined} data-current={s.id === current || undefined}>
            <button type="button" aria-pressed={open?.id === s.id} aria-label={`Step ${i + 1}: ${s.title}, ${s.done ? `done${s.doneAt ? ` ${dayLabel(s.doneAt.slice(0, 10))}` : ""}` : s.id === current ? "next" : "later"}`} onClick={() => setSel(s.id)}>
              <span className="lf-tl__dot" aria-hidden="true" /><small>{s.title}</small>
            </button>
          </li>
        ))}
      </ol>
      {open && (
        <div className="lf-tl__detail" aria-live="polite">
          <p><b>{open.title}</b>{open.why && <small> · {open.why}</small>}</p>
          {open.smallerSuggestion && !open.done && <p className="pk-note">Smaller first move: {open.smallerSuggestion}</p>}
          {!open.done ? <div className="pk-actions">
            <Button size="small" variant="secondary" onClick={() => { void answer(open.id, "yes", open.title); }}>Done</Button>
            <Button size="small" variant="plain" onClick={() => { void answer(open.id, "not_yet", open.title); }}>Not yet</Button>
          </div> : <p className="pk-note">Done{open.doneAt ? ` ${dayLabel(open.doneAt.slice(0, 10))}` : ""}.</p>}
        </div>
      )}
    </div>
  );
}

function LinkForm({ node, data, writes, onMsg }: { node: GoalNode; data: LifeData; writes: LifeWrites; onMsg: (m: string) => void }) {
  const [kind, setKind] = useState<"goal_project" | "goal_habit">("goal_project");
  const [other, setOther] = useState("");
  const taken = new Set([...node.projects.map((p) => p.link.otherRef), ...node.habits.map((h) => h.link.otherRef)]);
  const options = kind === "goal_project"
    ? data.projects.filter((p) => !taken.has(`project:${p.id}`) && p.status !== "archived").map((p) => ({ value: p.id, label: p.title }))
    : data.goals.filter((g) => isHabit(g) && !taken.has(`goals:${g.id}`)).map((g) => ({ value: g.id, label: g.title }));
  const add = async () => {
    if (!other) { onMsg(`Pick a ${kind === "goal_project" ? "project" : "habit"} first.`); return; }
    try { await writes.linkAdd(kind, node.goal.id, other); setOther(""); onMsg(`Linked to “${node.goal.title}”.`); } catch (e) { onMsg(errText(e)); }
  };
  return (
    <form className="pk-g2 lf-link" aria-label={`Link to ${node.goal.title}`} onSubmit={(e) => { e.preventDefault(); void add(); }}>
      <Select label="Link a" options={[{ value: "goal_project", label: "Project" }, { value: "goal_habit", label: "Habit" }]} value={kind} onChange={(e) => { setKind(e.target.value as typeof kind); setOther(""); }} />
      <div className="lf-plan__rowEnd">
        <Select label={kind === "goal_project" ? "Project" : "Habit"} options={[{ value: "", label: options.length ? "Choose…" : "Nothing left to link" }, ...options]} value={other} onChange={(e) => setOther(e.target.value)} />
        <Button size="small" variant="secondary" type="submit" disabled={!options.length}>Link</Button>
      </div>
    </form>
  );
}

function GoalRow({ node, data, writes, roadmaps, onMsg, expanded, onToggle }: { node: GoalNode; data: LifeData; writes: LifeWrites | null; roadmaps: RoadmapAdapter; onMsg: (m: string) => void; expanded: boolean; onToggle: () => void }) {
  const [editing, setEditing] = useState(false);
  const g = node.goal;
  const remove = async (linkId: string, what: string) => {
    if (!writes) return;
    try { await writes.linkRemove(linkId); onMsg(`Unlinked ${what}.`); } catch (e) { onMsg(errText(e)); }
  };
  return (
    <li className="lf-goal" data-open={expanded || undefined}>
      <button type="button" className="lf-goal__head" aria-expanded={expanded} onClick={onToggle}>
        <b>{g.title}</b><small>{STATUS_LABEL[statusOf(g)]} · {progressText(data.progress[g.id], g)}{g.deadline ? ` · by ${dayLabel(g.deadline.slice(0, 10))}` : ""}{node.projects.length ? ` · ${node.projects.length} project${node.projects.length === 1 ? "" : "s"}` : ""}</small>
      </button>
      {expanded && (
        <div className="lf-goal__body">
          {g.life?.statusReason && <p className="pk-note"><span className="lf-k">{STATUS_LABEL[statusOf(g)]}</span> {g.life.statusReason}</p>}
          {g.life?.nextAction && <p className="pk-note"><span className="lf-k">Next action</span> {g.life.nextAction}</p>}
          <h4 className="lf-h">Milestones</h4>
          <MilestoneTimeline goal={g} roadmaps={roadmaps} onMsg={onMsg} />
          <h4 className="lf-h">Projects</h4>
          {node.projects.length === 0 ? <p className="pk-note">No linked projects.</p> : (
            <ul className="lf-list">
              {node.projects.map(({ link, project }) => (
                <li key={link.id} className="lf-row">
                  <span>{project?.title ?? "A project that no longer exists"}
                    <small>{project ? [project.status, project.nextAction ? `next: ${project.nextAction}` : null, (project.milestones ?? []).find((m) => m.status !== "done") ? `milestone: ${(project.milestones ?? []).find((m) => m.status !== "done")!.title}` : null].filter(Boolean).join(" · ") : link.otherRef}</small>
                    {project && tasksForProject(data.tasks, project.id).map((t) => <small key={t.title}>Task: {t.title} ({t.status})</small>)}
                  </span>
                  {writes && <Button size="small" variant="plain" aria-label={`Unlink project ${project?.title ?? link.otherRef}`} onClick={() => { void remove(link.id, project ? `“${project.title}”` : "the project"); }}>Unlink</Button>}
                </li>
              ))}
            </ul>
          )}
          <h4 className="lf-h">Habits</h4>
          {node.habits.length === 0 ? <p className="pk-note">No linked habits.</p> : (
            <ul className="lf-list">
              {node.habits.map(({ link, habit }) => (
                <li key={link.id} className="lf-row"><span>{habit?.title ?? "A habit that no longer exists"}</span>
                  {writes && <Button size="small" variant="plain" aria-label={`Unlink habit ${habit?.title ?? link.otherRef}`} onClick={() => { void remove(link.id, habit ? `“${habit.title}”` : "the habit"); }}>Unlink</Button>}</li>
              ))}
            </ul>
          )}
          <h4 className="lf-h">Tasks</h4>
          {node.tasks.length === 0 ? <p className="pk-note">No tasks tagged <code>#goal:{g.id}</code> in TASKS.md.</p> : <ul className="lf-list">{node.tasks.map((t) => <li key={t.title}>{t.title}<small>{t.status}</small></li>)}</ul>}
          {writes && <LinkForm node={node} data={data} writes={writes} onMsg={onMsg} />}
          {writes && (editing ? <GoalFields goal={g} data={data} writes={writes} onDone={(m) => { setEditing(false); if (m) onMsg(m); }} />
            : <div className="pk-actions"><Button size="small" variant="secondary" onClick={() => setEditing(true)} aria-label={`Edit details: ${g.title}`}>Edit details</Button></div>)}
        </div>
      )}
    </li>
  );
}

function Areas({ data, writes, onMsg }: { data: LifeData; writes: LifeWrites; onMsg: (m: string) => void }) {
  const [name, setName] = useState("");
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const active = data.areas.filter((a) => a.status === "active");
  const add = async () => {
    if (!name.trim()) { onMsg("Give the area a name."); return; }
    try { await writes.areaUpsert(null, name.trim(), null); setName(""); onMsg(`Added the area “${name.trim()}”.`); } catch (e) { onMsg(errText(e)); }
  };
  return (
    <Card title="Life areas" headingLevel={3}>
      {active.length === 0 ? <p className="pk-note">No areas yet. Areas group goals, for example Health, Home or Japan.</p> : (
        <ul className="lf-list">
          {active.map((a) => (
            <li key={a.id} className="lf-row">
              {renaming?.id === a.id ? (
                <form className="lf-plan__rowEnd" onSubmit={(e) => { e.preventDefault(); void writes.areaUpsert(a.id, renaming.name.trim(), a.description).then(() => { setRenaming(null); onMsg("Area renamed."); }, (err) => onMsg(errText(err))); }}>
                  <TextField label={`New name for ${a.name}`} hideLabel value={renaming.name} maxLength={80} onChange={(e) => setRenaming({ id: a.id, name: e.target.value })} />
                  <Button size="small" variant="primary" type="submit">Save</Button><Button size="small" variant="plain" onClick={() => setRenaming(null)}>Cancel</Button>
                </form>
              ) : <>
                <span>{a.name}<small>{data.goals.filter((g) => g.life?.areaId === a.id && !isHabit(g)).length} goals</small></span>
                <span className="lf-row__end">
                  <Button size="small" variant="plain" aria-label={`Rename area ${a.name}`} onClick={() => setRenaming({ id: a.id, name: a.name })}>Rename</Button>
                  <Button size="small" variant="plain" aria-label={`Archive area ${a.name}`} onClick={() => { void writes.areaArchive(a.id).then(() => onMsg(`Archived “${a.name}”. Its goals are unchanged.`), (err) => onMsg(errText(err))); }}>Archive</Button>
                </span>
              </>}
            </li>
          ))}
        </ul>
      )}
      <form className="lf-plan__rowEnd" aria-label="Add a life area" onSubmit={(e) => { e.preventDefault(); void add(); }}>
        <TextField label="New area" value={name} maxLength={80} placeholder="e.g. Health" onChange={(e) => setName(e.target.value)} />
        <Button size="small" variant="secondary" type="submit">Add area</Button>
      </form>
    </Card>
  );
}

/** Life › Goals & projects: the hierarchy (area → goal → projects, habits, tasks), each goal's milestones and Life details. */
export function GoalsProjects({ data, writes, focusGoal = null, roadmaps = roadmapAdapter() }: { data: LifeData; writes: LifeWrites | null; focusGoal?: string | null; roadmaps?: RoadmapAdapter }) {
  const [msg, setMsg] = useState<string | null>(null);
  const [showClosed, setShowClosed] = useState(false);
  const [open, setOpen] = useState<Set<string>>(() => new Set(focusGoal ? [focusGoal] : []));
  const tree = goalTree(data, showClosed);
  const count = tree.reduce((n, a) => n + a.goals.length, 0);
  const toggle = (id: string) => setOpen((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  return (
    <section className="lf-tree" aria-label="Goals and projects">
      <header className="lf-tree__head"><h2 className="d18-h3">Goals and projects</h2><span className="d18-grow" />
        <Button size="small" variant="plain" onClick={() => setShowClosed((v) => !v)}>{showClosed ? "Hide completed and archived" : "Show completed and archived"}</Button></header>
      {msg && <p className="pk-note" role="status">{msg}</p>}
      {data.failed.includes("read_goals") ? <p className="pk-note lf-err">Goals couldn&apos;t be read just now.</p> : count === 0 ? <p className="pk-note">No goals yet. Add one with + above.</p> : tree.map((a) => (
        <div key={a.area?.id ?? "none"} className="lf-area" role="group" aria-label={a.area?.name ?? "No area"}>
          <h3 className="lf-h">{a.area?.name ?? "No area"}</h3>
          {a.goals.length === 0 ? <p className="pk-note">No goals in this area.</p> : (
            <ul className="lf-goals">{a.goals.map((n) => <GoalRow key={n.goal.id} node={n} data={data} writes={writes} roadmaps={roadmaps} onMsg={setMsg} expanded={open.has(n.goal.id)} onToggle={() => toggle(n.goal.id)} />)}</ul>
          )}
        </div>
      ))}
      {writes && <Areas data={data} writes={writes} onMsg={setMsg} />}
    </section>
  );
}
