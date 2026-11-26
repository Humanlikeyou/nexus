import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, PageHero, SegmentedControl, Select, StatusText, TextField } from "../ui";
import { duration, FAMILY_LABEL, itemFor, liveWorkoutCalls, move, OPEN_STATES, prescription, STATE_LABEL, type Exercise, type NoopLink, type SessionView, type Template, type TemplateItem, type WorkoutCalls } from "./workoutApi";
import "../ui/PanelKit.css";

/**
 * Decision 0062: Mind-Body › Fitness › Workout (design boundary lifted for this view only). The Fitness page
 * itself is unchanged. Every number and time shown here comes from the Rust engine.
 */
type Tab = "workouts" | "library" | "build" | "history";
const TABS: Array<{ value: Tab; label: string }> = [{ value: "workouts", label: "Workouts" }, { value: "library", label: "Library" }, { value: "build", label: "Build" }, { value: "history", label: "History" }];
const errText = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : "Something went wrong.");
const when = (iso: string | null) => { if (!iso) return "Not recorded"; const d = new Date(iso); return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }); };

export function ExerciseDetail({ e }: { e: Exercise }) {
  return (
    <Card title={e.name} headingLevel={3} accessory={<span className="pk-acc"><span className="v-note">{FAMILY_LABEL[e.family] ?? e.family} · level {e.level} · {e.difficulty}</span></span>}>
      <p className="pk-note">{e.description}</p>
      <p className="pk-note"><b>Targets:</b> {e.muscles.join(", ")} · <b>Equipment:</b> {e.equipment.length ? e.equipment.join(", ") : "none"} · <b>Recommended:</b> {e.sets} × {e.measure === "reps" ? `${e.reps} reps` : `${e.holdS} s hold`}</p>
      <ol className="pk-list" aria-label={`How to do ${e.name}`}>{e.instructions.map((s) => <li key={s}>{s}</li>)}</ol>
      {e.form.length > 0 && <><p className="pk-note"><b>Form</b></p><ul className="pk-list" aria-label={`${e.name} form`}>{e.form.map((s) => <li key={s}>{s}</li>)}</ul></>}
      {e.safety.length > 0 && <><p className="pk-note"><b>Safety</b></p><ul className="pk-list" aria-label={`${e.name} safety`}>{e.safety.map((s) => <li key={s}>{s}</li>)}</ul></>}
      <p className="pk-note">Demonstration: not available yet ({e.demoAsset ?? "no reference"}).</p>
    </Card>
  );
}

function Library({ exercises }: { exercises: Exercise[] }) {
  const families = useMemo(() => [...new Set(exercises.map((e) => e.family))], [exercises]);
  const [family, setFamily] = useState("all");
  const [open, setOpen] = useState<string | null>(null);
  const shown = exercises.filter((e) => family === "all" || e.family === family).sort((a, b) => a.family.localeCompare(b.family) || a.level - b.level);
  const detail = exercises.find((e) => e.id === open);
  return (
    <>
      <Card title="Exercise library">
        <Select label="Family" options={[{ value: "all", label: "All" }, ...families.map((f) => ({ value: f, label: FAMILY_LABEL[f] ?? f }))]} value={family} onChange={(ev) => setFamily(ev.target.value)} />
        <ul className="pk-list" aria-label="Exercises">
          {shown.map((e) => <li key={e.id}><Button variant="plain" size="small" onClick={() => setOpen(e.id)}>{e.name}</Button> {FAMILY_LABEL[e.family] ?? e.family} level {e.level} · {e.difficulty} · {e.equipment.length ? e.equipment.join(", ") : "no equipment"}</li>)}
        </ul>
      </Card>
      {detail && <ExerciseDetail e={detail} />}
    </>
  );
}

/** The builder: pick exercises, set sets, reps or hold, load and rest, reorder, and see the engine's estimate. */
export function Builder({ exercises, calls, start, onSaved }: { exercises: Exercise[]; calls: WorkoutCalls; start?: Template | null; onSaved: (t: Template) => void }) {
  const [name, setName] = useState(start?.name ?? "");
  const [items, setItems] = useState<TemplateItem[]>(start?.items ?? []);
  const [restBetween, setRestBetween] = useState(start?.restBetweenS ?? 90);
  const [pick, setPick] = useState(exercises[0]?.id ?? "");
  const [est, setEst] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const byId = useMemo(() => new Map(exercises.map((e) => [e.id, e])), [exercises]);
  useEffect(() => {
    let alive = true;
    if (!items.length) return; // the estimate line says "add an exercise" without one
    const t = window.setTimeout(() => { calls.estimate(items, restBetween).then((s) => { if (alive) setEst(s); }, () => { if (alive) setEst(null); }); }, 150);
    return () => { alive = false; window.clearTimeout(t); };
  }, [items, restBetween, calls]);
  const patch = (i: number, p: Partial<TemplateItem>) => setItems((list) => list.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const num = (v: string) => (v.trim() === "" ? null : Number(v));
  const save = () => {
    const t: Template = { id: start?.source === "user" ? start.id : "", name: name.trim(), source: "user", sourceRef: null, items, restBetweenS: restBetween, archived: false, createdAt: "", updatedAt: "" };
    calls.saveTemplate(t).then((saved) => { setMsg(`Saved ${saved.name}.`); onSaved(saved); }, (e: unknown) => setMsg(errText(e)));
  };
  return (
    <Card title={start?.source === "user" ? `Edit ${start.name}` : "Build a workout"}>
      <TextField label="Name" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="pk-actions">
        <Select label="Add an exercise" options={exercises.map((e) => ({ value: e.id, label: `${e.name} (${FAMILY_LABEL[e.family] ?? e.family} ${e.level})` }))} value={pick} onChange={(e) => setPick(e.target.value)} />
        <Button size="small" variant="secondary" onClick={() => { const e = byId.get(pick); if (e) setItems((l) => [...l, itemFor(e)]); }}>Add</Button>
      </div>
      {items.length === 0 ? <p className="pk-note">No exercises yet.</p> : (
        <ol className="pk-list" aria-label="Workout order">
          {items.map((it, i) => {
            const e = byId.get(it.exerciseId);
            return (
              <li key={`${it.exerciseId}-${i}`}>
                <b>{e?.name ?? it.exerciseId}</b>
                <div className="pk-actions">
                  <TextField label={`Sets for ${e?.name ?? "exercise"} ${i + 1}`} type="number" value={String(it.sets)} onChange={(ev) => patch(i, { sets: Number(ev.target.value) })} />
                  {e?.measure === "hold"
                    ? <TextField label={`Hold seconds for ${e.name} ${i + 1}`} type="number" value={String(it.holdS ?? "")} onChange={(ev) => patch(i, { holdS: num(ev.target.value) })} />
                    : <TextField label={`Reps for ${e?.name ?? "exercise"} ${i + 1}`} type="number" value={String(it.reps ?? "")} onChange={(ev) => patch(i, { reps: num(ev.target.value) })} />}
                  <TextField label={`Added load kg for ${e?.name ?? "exercise"} ${i + 1}`} type="number" value={it.loadKg == null ? "" : String(it.loadKg)} onChange={(ev) => patch(i, { loadKg: num(ev.target.value) })} />
                  <TextField label={`Rest seconds for ${e?.name ?? "exercise"} ${i + 1}`} type="number" value={String(it.restS)} onChange={(ev) => patch(i, { restS: Number(ev.target.value) })} />
                </div>
                <div className="pk-actions">
                  <Button size="small" variant="plain" aria-label={`Move ${e?.name ?? "exercise"} ${i + 1} up`} disabled={i === 0} onClick={() => setItems((l) => move(l, i, -1))}>Up</Button>
                  <Button size="small" variant="plain" aria-label={`Move ${e?.name ?? "exercise"} ${i + 1} down`} disabled={i === items.length - 1} onClick={() => setItems((l) => move(l, i, 1))}>Down</Button>
                  <Button size="small" variant="plain" aria-label={`Remove ${e?.name ?? "exercise"} ${i + 1}`} onClick={() => setItems((l) => l.filter((_, j) => j !== i))}>Remove</Button>
                </div>
              </li>
            );
          })}
        </ol>
      )}
      <TextField label="Rest between exercises (seconds)" type="number" value={String(restBetween)} onChange={(e) => setRestBetween(Number(e.target.value))} />
      <p className="pk-note" role="status">Estimated length: {items.length ? (est == null ? "working it out…" : duration(est)) : "add an exercise"}</p>
      <div className="pk-actions"><Button size="small" variant="primary" disabled={!name.trim() || !items.length} onClick={save}>Save workout</Button></div>
      {msg && <p className="pk-note" role="status">{msg}</p>}
    </Card>
  );
}

/** The running workout: the next set, and start, pause, resume, log, skip and end. */
export function Player({ view, exercises, calls, onChange }: { view: SessionView; exercises: Exercise[]; calls: WorkoutCalls; onChange: (v: SessionView) => void }) {
  const s = view.session;
  const it = s.plan[s.nextItem];
  const e = exercises.find((x) => x.id === it?.exerciseId);
  const [reps, setReps] = useState(String(it?.reps ?? ""));
  const [load, setLoad] = useState(it?.loadKg == null ? "" : String(it.loadKg));
  const [holdStart, setHoldStart] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const key = `${s.nextItem}:${s.nextSet}`;
  const last = useRef(key);
  useEffect(() => { if (last.current !== key) { last.current = key; setReps(String(it?.reps ?? "")); setLoad(it?.loadKg == null ? "" : String(it.loadKg)); setHoldStart(null); } }, [key, it]);
  const act = (a: Parameters<WorkoutCalls["act"]>[1]) => calls.act(s.id, a).then((v) => { setMsg(null); onChange(v); }, (err: unknown) => setMsg(errText(err)));
  const loadKg = load.trim() === "" ? null : Number(load);
  const logHold = () => { if (holdStart == null) return; const secs = Math.max(1, Math.round((Date.now() - holdStart) / 1000)); void act({ kind: "logSet", holdS: secs, loadKg }); };
  return (
    <Card title={`${s.templateName}${s.source === "amara" ? " (suggested by Amara)" : ""}`} accessory={<span className="pk-acc"><StatusText kind={s.state === "active" ? "ok" : "wait"}>{STATE_LABEL[s.state]}</StatusText></span>}>
      {s.state === "planned" && <><p className="pk-note">{s.plan.length} exercises · about {duration(view.summary.estimatedS)}.</p>
        <div className="pk-actions"><Button size="small" variant="primary" onClick={() => void act({ kind: "start" })}>Start</Button><Button size="small" variant="plain" onClick={() => void act({ kind: "end", abandon: true })}>Cancel</Button></div></>}
      {(s.state === "active" || s.state === "paused") && it && e && (
        <>
          <p className="pk-note" role="status"><b>{e.name}</b>, set {s.nextSet + 1} of {it.sets} · exercise {s.nextItem + 1} of {s.plan.length} · {prescription(it)}</p>
          {s.state === "active" && (e.measure === "hold" ? (
            <div className="pk-actions">
              {holdStart == null ? <Button size="small" variant="primary" onClick={() => setHoldStart(Date.now())}>Start hold ({it.holdS} s)</Button>
                : <Button size="small" variant="primary" onClick={logHold}>Stop hold and log</Button>}
            </div>
          ) : (
            <div className="pk-actions">
              <TextField label="Reps done" type="number" value={reps} onChange={(ev) => setReps(ev.target.value)} />
              <TextField label="Added load kg" type="number" value={load} onChange={(ev) => setLoad(ev.target.value)} />
              <Button size="small" variant="primary" disabled={reps.trim() === ""} onClick={() => void act({ kind: "logSet", reps: Number(reps), loadKg })}>Log set</Button>
            </div>
          ))}
          <div className="pk-actions">
            {s.state === "active" ? <Button size="small" variant="secondary" onClick={() => void act({ kind: "pause" })}>Pause</Button> : <Button size="small" variant="secondary" onClick={() => void act({ kind: "resume" })}>Resume</Button>}
            <Button size="small" variant="plain" disabled={s.state !== "active"} onClick={() => void act({ kind: "skip", scope: "set" })}>Skip set</Button>
            <Button size="small" variant="plain" disabled={s.state !== "active"} onClick={() => void act({ kind: "skip", scope: "exercise" })}>Skip exercise</Button>
            <Button size="small" variant="destructive" onClick={() => void act({ kind: "end" })}>End workout</Button>
          </div>
          <p className="pk-note">{e.form[0] ?? ""} {e.safety[0] ?? ""}</p>
        </>
      )}
      {!OPEN_STATES.includes(s.state) && <SummaryBlock view={view} />}
      {msg && <p className="pk-note" role="alert">{msg}</p>}
    </Card>
  );
}

export function SummaryBlock({ view }: { view: SessionView }) {
  const m = view.summary;
  const muscles = Object.entries(m.setsByMuscle).sort((a, b) => b[1] - a[1]).map(([k, n]) => `${k} ${n}`);
  return (
    <section aria-label="Workout summary">
      <p className="pk-note"><b>{STATE_LABEL[view.session.state]}</b>: {m.doneSets} of {m.plannedSets} sets ({m.completionPct}%){m.skippedSets ? `, ${m.skippedSets} skipped` : ""}.</p>
      <p className="pk-note">Reps {m.totalReps} · holds {duration(m.totalHoldS)} · added-load volume {m.loadVolumeKg ? `${m.loadVolumeKg} kg` : "none"} · active {duration(m.activeS)} (estimate {duration(m.estimatedS)}) · paused {duration(m.pausedS)} · rest {duration(m.restS)}</p>
      {muscles.length > 0 && <p className="pk-note">Sets by muscle: {muscles.join(", ")}</p>}
    </section>
  );
}

export function WorkoutView({ calls = liveWorkoutCalls, onDone }: { calls?: WorkoutCalls; onDone?: () => void }) {
  const [tab, setTab] = useState<Tab>("workouts");
  const [lib, setLib] = useState<Exercise[] | null>(null);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [history, setHistory] = useState<SessionView[]>([]);
  const [linked, setLinked] = useState<Map<string, NoopLink | null>>(new Map());
  const [takes, setTakes] = useState<Array<{ takeId: string; name: string; estimatedS: number }>>([]);
  const [current, setCurrent] = useState<SessionView | null>(null);
  const [editing, setEditing] = useState<Template | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const reload = useCallback(() => {
    Promise.all([calls.library(), calls.templates(), calls.history(50), calls.takeIds()]).then(([l, t, h, k]) => {
      setLib(l.exercises); setTemplates(t.filter((x) => !x.archived)); setHistory(h); setTakes(k);
      setCurrent((c) => h.find((v) => OPEN_STATES.includes(v.session.state)) ?? (c && !OPEN_STATES.includes(c.session.state) ? c : null));
    }, (e: unknown) => setErr(errText(e)));
    calls.historyLinked(50).then((rows) => setLinked(new Map(rows.map((r) => [r.view.session.id, r.noop]))), () => setLinked(new Map()));
  }, [calls]);
  useEffect(() => { reload(); }, [reload]);
  const plan = (templateId: string) => calls.plan(templateId).then((v) => { setCurrent(v); setErr(null); }, (e: unknown) => setErr(errText(e)));
  const planTake = (takeId: string) => calls.fromTake(takeId).then((t) => plan(t.id), (e: unknown) => setErr(errText(e)));
  const onChange = (v: SessionView) => { setCurrent(v); if (!OPEN_STATES.includes(v.session.state)) reload(); };
  const open = current && OPEN_STATES.includes(current.session.state);
  return (
    <div className="v-page" id="workout">
      <PageHero title="Workout" statement="Calisthenics training" detail="Build a workout or start one of Amara's, then log each set. Your Fitness page and WHOOP data are unchanged." />
      {err && <p className="pk-note" role="alert">{err}</p>}
      {!lib ? <p className="pk-note" role="status">Loading the library…</p> : (
        <>
          {current && <Player view={current} exercises={lib} calls={calls} onChange={onChange} />}
          {!open && <SegmentedControl label="Workout sections" options={TABS} value={tab} onChange={setTab} />}
          {!open && tab === "workouts" && (
            <>
              <Card title="Your workouts">
                {templates.filter((t) => t.source === "user").length === 0 ? <p className="pk-note">None yet. Build one, or start one of Amara's below.</p> : (
                  <ul className="pk-list" aria-label="Your workouts">{templates.filter((t) => t.source === "user").map((t) => (
                    <li key={t.id}><b>{t.name}</b> · {t.items.length} exercises <Button size="small" variant="primary" aria-label={`Start ${t.name}`} onClick={() => void plan(t.id)}>Start</Button> <Button size="small" variant="plain" aria-label={`Edit ${t.name}`} onClick={() => { setEditing(t); setTab("build"); }}>Edit</Button></li>
                  ))}</ul>
                )}
              </Card>
              <Card title="Suggested by Amara">
                <p className="pk-note">Amara's calisthenics suggestions, run on the same engine as your own workouts.</p>
                <ul className="pk-list" aria-label="Amara's workouts">{takes.map((k) => <li key={k.takeId}><b>{k.name}</b> · about {duration(k.estimatedS)} <Button size="small" variant="secondary" aria-label={`Start ${k.name}`} onClick={() => void planTake(k.takeId)}>Start</Button></li>)}</ul>
              </Card>
            </>
          )}
          {!open && tab === "library" && <Library exercises={lib} />}
          {!open && tab === "build" && <Builder key={editing?.id ?? "new"} exercises={lib} calls={calls} start={editing} onSaved={() => { setEditing(null); reload(); setTab("workouts"); }} />}
          {!open && tab === "history" && (
            <Card title="History">
              {history.length === 0 ? <p className="pk-note">No workouts yet.</p> : (
                <ul className="pk-list" aria-label="Workout history">{history.map((v) => (
                  <li key={v.session.id}><b>{when(v.session.startedAt ?? v.session.plannedAt)} · {v.session.templateName}</b>{v.session.source === "amara" ? " (Amara)" : ""} · {STATE_LABEL[v.session.state]} · {v.summary.doneSets}/{v.summary.plannedSets} sets · {v.summary.totalReps} reps · active {duration(v.summary.activeS)}{(() => { const n = linked.get(v.session.id); return n ? ` · the strap also recorded ${n.sport}${n.durationMin != null ? ` (${Math.round(n.durationMin)} min${n.strain != null ? `, strain ${n.strain.toFixed(1)}` : ""})` : ""} in NOOP` : ""; })()}</li>
                ))}</ul>
              )}
            </Card>
          )}
        </>
      )}
      {onDone && <div className="pk-actions"><Button size="small" variant="plain" onClick={onDone}>Back to Fitness</Button></div>}
    </div>
  );
}
