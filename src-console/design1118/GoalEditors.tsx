import { Pencil, Plus, Target, Repeat } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { reducedMotion } from "../shell/Splash";
import type { Roadmap, StepSize } from "../goals/roadmap";
import type { GoalWire, Habit } from "../personal/adapters";
import { Button, Select, TextField } from "../ui";
import type { GoalDraft, HabitDraft } from "./goalStore";
import { SIZE_LABEL } from "./pathModel";

const SIZE_OPTIONS = (["small", "medium", "big"] as const).map((v) => ({ value: v, label: SIZE_LABEL[v] }));

/** The one line at the top of Goals: how the pieces fit. */
export function GoalsHelperLine() {
  return <p className="d18-helper" data-testid="goals-helper">A goal has milestones, the stations on its path, and one next step at a time. Habits are what you repeat, and they feed your goals.</p>;
}

/** The one Add button: opens a choice of Goal or Habit, each with a one-line explanation. */
export function AddChooser({ open, onOpen, onChoose, onClose }: { open: boolean; onOpen: () => void; onChoose: (k: "goal" | "habit") => void; onClose: () => void }) {
  return (
    <div className="d18-add">
      <Button size="small" variant="primary" icon={Plus} onClick={open ? onClose : onOpen} aria-expanded={open}>Add</Button>
      {open && (
        <div className="d18-add__menu" role="group" aria-label="Add a goal or a habit">
          <button type="button" className="d18-add__opt" onClick={() => onChoose("goal")}>
            <Target size={18} aria-hidden="true" /><span><b>Goal</b><small>Something you&apos;re working toward. It gets its own path, with milestones as stations.</small></span>
          </button>
          <button type="button" className="d18-add__opt" onClick={() => onChoose("habit")}>
            <Repeat size={18} aria-hidden="true" /><span><b>Habit</b><small>Something you repeat each week. It shows under the map and feeds your goals.</small></span>
          </button>
        </div>
      )}
    </div>
  );
}

/** Pencil button used on every goal card and habit row. */
export function EditPencil({ label, onClick }: { label: string; onClick: () => void }) {
  return <Button size="small" variant="plain" icon={Pencil} iconOnly aria-label={label} className="d18-pencil" onClick={onClick} />;
}

function ConfirmDelete({ what, onDelete }: { what: string; onDelete: () => void }) {
  const [asking, setAsking] = useState(false);
  if (!asking) return <Button size="small" variant="plain" className="d18-danger" onClick={() => setAsking(true)}>Delete</Button>;
  return (
    <span className="d18-confirm" role="alertdialog" aria-label={`Delete ${what}?`}>
      <span>Delete {what}? This can&apos;t be undone.</span>
      <Button size="small" variant="primary" className="d18-danger" onClick={onDelete}>Delete</Button>
      <Button size="small" variant="plain" onClick={() => setAsking(false)}>Keep it</Button>
    </span>
  );
}

function Panel({ title, children, onSubmit }: { title: string; children: ReactNode; onSubmit: (e: FormEvent) => void }) {
  const ref = useRef<HTMLFormElement>(null);
  useEffect(() => {
    // opened from a card further down the page: bring the form into view and put the cursor in it
    const f = ref.current; if (!f) return;
    f.scrollIntoView?.({ block: "nearest", behavior: reducedMotion() ? "auto" : "smooth" });
    f.querySelector<HTMLInputElement>("input")?.focus({ preventScroll: true });
  }, []);
  return (
    <form ref={ref} className="d18-card d18-editor" aria-label={title} onSubmit={(e) => { e.preventDefault(); onSubmit(e); }}>
      <h2 className="d18-h3">{title}</h2>
      {children}
    </form>
  );
}

type StepDraft = { id: string; title: string; size: StepSize; done: boolean };

/** Add or edit a goal: title, size (target and unit), deadline, and its milestones / steps. */
export function GoalForm({ row, roadmap, onSave, onSaveStep, onDelete, onCancel, msg }: {
  row?: GoalWire | null; roadmap?: Roadmap | null; msg?: string;
  onSave: (d: GoalDraft) => void; onSaveStep?: (stepId: string, title: string, size: StepSize) => void; onDelete?: () => void; onCancel: () => void;
}) {
  const [title, setTitle] = useState(row?.title ?? "");
  const [target, setTarget] = useState(row && row.target > 0 ? String(row.target) : "");
  const [unit, setUnit] = useState(row?.unit ?? "");
  const [deadline, setDeadline] = useState(row?.deadline?.slice(0, 10) ?? "");
  const [steps, setSteps] = useState<StepDraft[]>(() => (roadmap?.steps ?? []).map((s) => ({ id: s.id, title: s.title, size: s.size, done: s.done })));
  const editing = !!row;
  const submit = () => {
    if (!title.trim()) return;
    onSave({ title, target: Number(target) || 0, unit, deadline: deadline || null });
    for (const s of steps) {
      const was = roadmap?.steps.find((x) => x.id === s.id);
      if (was && s.title.trim() && (was.title !== s.title.trim() || was.size !== s.size)) onSaveStep?.(s.id, s.title.trim(), s.size);
    }
  };
  return (
    <Panel title={editing ? `Edit goal · ${row?.title}` : "New goal"} onSubmit={submit}>
      <TextField label="Goal" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="e.g. Launch Taiyo Skin's first product" required />
      <div className="d18-editor__row">
        <TextField label="Size (target)" inputMode="decimal" value={target} onChange={(e) => setTarget(e.target.value.replace(/[^\d.]/g, ""))} placeholder="e.g. 5000" />
        <TextField label="Unit" value={unit} onChange={(e) => setUnit(e.target.value)} placeholder="e.g. USD / month" />
        <TextField label="By (optional)" type="date" value={deadline} onChange={(e) => setDeadline(e.target.value)} />
      </div>
      {editing && (
        <fieldset className="d18-editor__steps">
          <legend className="d18-eyebrow">Milestones and steps</legend>
          {steps.length === 0 ? <p className="d18-note">No stations yet. Amara plans the route overnight, then each station shows here to edit.</p> : steps.map((s, i) => (
            <div key={s.id} className="d18-editor__step" data-done={s.done || undefined}>
              <TextField label={`Station ${i + 1}${s.done ? " (done)" : ""}`} value={s.title} onChange={(e) => setSteps((xs) => xs.map((x) => (x.id === s.id ? { ...x, title: e.target.value } : x)))} />
              <Select label="Size" options={SIZE_OPTIONS} value={s.size} onChange={(e) => setSteps((xs) => xs.map((x) => (x.id === s.id ? { ...x, size: e.target.value as StepSize } : x)))} />
            </div>
          ))}
        </fieldset>
      )}
      {!editing && <p className="d18-note">Amara plans the milestones overnight. The path card shows straight away and grows its stations once the route is ready.</p>}
      <div className="d18-actions">
        <Button size="small" variant="primary" type="submit">{editing ? "Save goal" : "Add goal"}</Button>
        <Button size="small" variant="plain" onClick={onCancel}>Cancel</Button>
        <span className="d18-grow" />
        {editing && onDelete && <ConfirmDelete what="this goal" onDelete={onDelete} />}
      </div>
      {msg && <p className="d18-msg" role="status">{msg}</p>}
    </Panel>
  );
}

/** Edit a habit: title, rule and times a week, plus delete. */
export function HabitForm({ habit, row, onSave, onDelete, onCancel, msg }: { habit: Habit; row?: GoalWire | null; onSave: (d: HabitDraft) => void; onDelete: () => void; onCancel: () => void; msg?: string }) {
  const [title, setTitle] = useState(row?.title ?? habit.title);
  const [rule, setRule] = useState(row?.unit ?? habit.rule);
  const [per, setPer] = useState(String(row && row.target > 0 ? Math.min(7, row.target) : habit.target || 7));
  return (
    <Panel title={`Edit habit · ${habit.title}`} onSubmit={() => { if (title.trim()) onSave({ title, rule, perWeek: Number(per) }); }}>
      <TextField label="Habit" value={title} onChange={(e) => setTitle(e.target.value)} required />
      <div className="d18-editor__row">
        <TextField label="Rule" value={rule} onChange={(e) => setRule(e.target.value)} placeholder="e.g. 20 minutes before bed" />
        <Select label="Times a week" options={[1, 2, 3, 4, 5, 6, 7].map((n) => ({ value: String(n), label: String(n) }))} value={per} onChange={(e) => setPer(e.target.value)} />
      </div>
      <div className="d18-actions">
        <Button size="small" variant="primary" type="submit">Save habit</Button>
        <Button size="small" variant="plain" onClick={onCancel}>Cancel</Button>
        <span className="d18-grow" />
        <ConfirmDelete what="this habit" onDelete={onDelete} />
      </div>
      {msg && <p className="d18-msg" role="status">{msg}</p>}
    </Panel>
  );
}

