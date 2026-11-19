import { Mic } from "lucide-react";
import { useState, type FormEvent, type ReactNode } from "react";
import { Button, Card, DataTag, More, PageHero, PageSection, StatusText, wiredTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import type { Goal } from "../types";
import { useStoresTick } from "../lib/localStores";
import { CHECKIN_EVENT, ENERGY_STEPS, MOOD_STEPS, PERSONAL_DEFAULTS, type CheckIn, type Habit, type PersonalAdapter } from "./adapters";
import { Rich } from "./parts";
import { HabitRow } from "../goals/HabitTracker";
import { HabitsUnderMap, MilestonesMap, type PathGoal } from "../design1118/GoalPath";
import { PageTake } from "../design1118/PageTake";
import { AddChooser, GoalForm, GoalsHelperLine, HabitForm } from "../design1118/GoalEditors";
import { goalStore, type GoalDraft, type HabitDraft } from "../design1118/goalStore";
import { roadmapAdapter, type StepSize } from "../goals/roadmap";
import { ReadyToKeepCard, SuggestedLatelyCard, TakeChange, TakesThisWeekCard, WeekSuggestionsCard, WeekSuggestionsTile } from "../takes/WeekTakes";
import { takesAdapter } from "../takes/adapters";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { GoalsThisWeekCard, LastTryCard } from "../life/WeekReviewExtras";
import { humanize } from "../lib/format";
import "../ui/PanelKit.css";
import "./personal.css";
import "../adds/v1111.css";

const WIRE_NOTE = "Needs wiring: this appears once Claude builds its store (HANDOFF-1111a). Nothing here is made up.";

/** Life › Goals (design-1118a, Direction A "the Path"): hero, Amara's take under it, the Milestones map
 *  (arrival tangle → a goal in focus → the four-path grid), habits under the map with one shared
 *  "Link a source", then the add composer. One progress meaning: roadmap percent + steps done. */
export function GoalsSection({ adapter, goals, goalsFailed, onRetry, composer, added = [], focusGoal = null, onOpenVenture }: { adapter: PersonalAdapter; goals: Goal[]; goalsFailed?: boolean; onRetry?: () => void; composer?: ReactNode; added?: Array<{ id: string; title: string }>; focusGoal?: string | null; onOpenVenture?: (ventureId: string) => void }) {
  useStoresTick();
  const store = goalStore();
  const roadmaps = roadmapAdapter();
  const [adding, setAdding] = useState<null | "choose" | "goal" | "habit">(null);
  const [editing, setEditing] = useState<{ kind: "goal" | "habit"; id: string } | null>(null);
  const [msg, setMsg] = useState("");
  const [newGoalId, setNewGoalId] = useState<string | null>(null);
  const [req, setReq] = useState<{ id: string | null; n: number }>({ id: null, n: 0 });
  const week = adapter.goals();
  const raw = valueOf(week);
  const w = raw ? store.overlay(raw) : null;
  const ready = valueOf(takesAdapter().goals())?.ready.length ?? 0;
  const pathGoals: PathGoal[] = w ? w.milestones.map((m) => ({ id: m.id, title: m.title, tag: m.detail.split(" · ")[0] || undefined, ventureId: store.row(m.id)?.venture_id ?? null }))
    : goals.map((g) => ({ id: g.id, title: g.title, tag: g.category ? g.category.charAt(0).toUpperCase() + g.category.slice(1) : undefined, ventureId: store.row(g.id)?.venture_id ?? null }));
  const habits: Habit[] = store.overlayHabits([
    ...added.filter((h) => !w?.habits.some((x) => x.id === h.id)).map((h) => ({ id: h.id, title: h.title, source: "You", rule: "added just now · starts Monday", days: Array.from({ length: 7 }, () => "none" as const), done: 0, target: 7, note: "new" })),
    ...(w?.habits ?? []),
  ]);
  const NOT_SAVED = "Not saved. The goals store isn't answering on this Mac, so nothing changed.";
  const done = (r: { state: string }, ok: string) => { if (r.state === "unwired") { setMsg(NOT_SAVED); return false; } setMsg(ok); return true; };
  const addGoal = async (d: GoalDraft) => { const r = await store.addGoal(d); if (done(r, `Added “${d.title.trim()}”. It's open below: Amara is planning its route.`)) { setAdding(null); if (r.state !== "unwired") setNewGoalId(r.value.id); } };
  const saveGoal = async (id: string, d: GoalDraft) => { const r = await store.updateGoal(id, d); if (done(r, "Goal saved.")) setEditing(null); };
  const saveStep = async (id: string, stepId: string, title: string, size: StepSize) => { const r = await roadmaps.editStep(id, stepId, { title, size }); if (r.state === "unwired") setMsg("The goal saved, but its steps didn't: the roadmap store isn't answering."); };
  const saveHabit = async (id: string, d: HabitDraft) => { const r = await store.updateHabit(id, d); if (done(r, "Habit saved.")) setEditing(null); };
  const remove = async (id: string, what: string) => { const r = await store.remove(id); if (done(r, `Deleted ${what}.`)) setEditing(null); };
  const editGoal = editing?.kind === "goal" ? pathGoals.find((g) => g.id === editing.id) : undefined;
  const editHabit = editing?.kind === "habit" ? habits.find((h) => h.id === editing.id) : undefined;
  const n = pathGoals.length;
  const nextGoal = w?.nextMilestone ? w.milestones.find((m) => m.title === w.nextMilestone) : undefined;
  return (
    <div className="v-page d18-page" id="goals">
      <PageHero title="Goals" statement={n ? `${n} path${n === 1 ? "" : "s"}${w?.nextMilestone ? ` · next: ${w.nextMilestone}` : ""}` : "Habits and milestones"}
        detail={w ? <>{w.habits.length} habit{w.habits.length === 1 ? "" : "s"} this week{ready ? ` · ${ready} suggestion ready to become a habit` : ""} {week.state === "sample" && <DataTag kind="sample" />}{nextGoal && <> · <button type="button" className="v-link" onClick={() => setReq((r) => ({ id: nextGoal.id, n: r.n + 1 }))}>Open {nextGoal.title} ›</button></>}</> : <>Habits count themselves from Fitness, Neuro and check-ins once wired <DataTag kind="wire" /></>} />
      <div className="d18-goalbar"><GoalsHelperLine /><span className="d18-grow" />
        <AddChooser open={adding === "choose"} onOpen={() => { setAdding("choose"); setEditing(null); }} onClose={() => setAdding(null)} onChoose={(k) => { setAdding(k); setMsg(""); }} /></div>
      {adding === "goal" && <GoalForm onSave={(d) => { void addGoal(d); }} onCancel={() => setAdding(null)} />}
      {adding === "habit" && (composer ?? <p className="d18-note">Adding habits needs the add store to be wired first.</p>)}
      {editGoal && <GoalForm key={editGoal.id} row={store.row(editGoal.id) ?? { id: editGoal.id, title: editGoal.title, category: "personal", target: 0, current: 0, unit: "", deadline: null }}
        roadmap={valueOf(roadmaps.roadmap(editGoal.id))} onSave={(d) => { void saveGoal(editGoal.id, d); }} onSaveStep={(sid, t, sz) => { void saveStep(editGoal.id, sid, t, sz); }}
        onDelete={() => { void remove(editGoal.id, `“${editGoal.title}”`); }} onCancel={() => setEditing(null)} />}
      {editHabit && <HabitForm key={editHabit.id} habit={editHabit} row={store.row(editHabit.id)} onSave={(d) => { void saveHabit(editHabit.id, d); }} onDelete={() => { void remove(editHabit.id, `“${editHabit.title}”`); }} onCancel={() => setEditing(null)} />}
      {msg && <p className="d18-msg" role="status">{msg}</p>}
      <PageTake surface="goals" className="life__take" />
      {goalsFailed && pathGoals.length === 0 ? (
        <Card title="Milestones"><p className="pk-note">Goals couldn&apos;t be read.</p>{onRetry && <Button variant="secondary" size="small" onClick={onRetry}>Retry</Button>}</Card>
      ) : <MilestonesMap goals={pathGoals} sampleTag={week.state === "sample"} focusGoal={req.id ?? focusGoal} focusNonce={req.n} newGoalId={newGoalId} onOpenVenture={onOpenVenture} onEditGoal={(id) => { setEditing({ kind: "goal", id }); setAdding(null); }} />}
      {w ? <HabitsUnderMap habits={habits} sample={week.state === "sample"} onMark={(h, d) => { void store.markHabit(h.id, d); }} onEdit={(h) => { setEditing({ kind: "habit", id: h.id }); setAdding(null); }} /> : <Card title="Habits this week" accessory={<span className="pk-acc">{wiredTag(week)}</span>}><p className="pk-note">{WIRE_NOTE}</p></Card>}
      {w && w.habits.length > 0 && (
        <More label="The full habit grid">
          {w.habits.map((h) => <HabitRow h={h} key={h.id} />)}
          <p className="pk-note">Each square is a day, newest on the right. Half squares: nearly (within 15 min). Amara counts from the source data; you can&apos;t tick these by hand unless a source is missing.</p>
        </More>
      )}
      <ReadyToKeepCard title="Ready to become a habit" />
    </div>
  );
}

const TAGS = ["deep work", "coffee late", "training", "social", "travel", "Taiyo Skin"];

function Scale({ label, steps, value, onChange }: { label: string; steps: readonly string[]; value: number | null; onChange: (v: number) => void }) {
  return (
    <div className="pk-field">
      <span id={`ci-${label}`}>{label}</span>
      <div className="pk-scale pk-scale5 ci-scale" role="radiogroup" aria-labelledby={`ci-${label}`}>
        {steps.map((s, i) => (
          <button key={s} type="button" role="radio" aria-checked={value === i + 1} data-on={value === i + 1 || undefined} onClick={() => onChange(i + 1)}>
            <b>{i + 1}</b>{s}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Home › Today check-in (1111a #3): 5-step mood, energy, tags and text. Voice is transcribed to text; no audio is kept. */
export function CheckInSection({ adapter }: { adapter: PersonalAdapter }) {
  useStoresTick();
  const list = adapter.checkins();
  const [mood, setMood] = useState<number | null>(null);
  const [energy, setEnergy] = useState<number | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const [text, setText] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [, bump] = useState(0);
  const wired = list.state !== "unwired";
  const save = async (e: FormEvent) => {
    e.preventDefault();
    const r = await adapter.saveCheckIn({ mood, energy, tags, text: text.trim() });
    setNote(r.state === "unwired" ? "Saving check-ins needs wiring, so nothing was saved." : r.state === "sample" ? "Sample data: saved to the preview list only." : "Saved to this Mac.");
    if (r.state !== "unwired") { setMood(null); setEnergy(null); setTags([]); setText(""); bump((n) => n + 1); window.dispatchEvent(new Event(CHECKIN_EVENT)); }
  };
  const voice = async () => {
    const r = await adapter.transcribe();
    if (r.state === "unwired") setNote("Voice check-ins need a transcription path on this Mac. Type it for now; no audio is recorded.");
    else setText((t) => (t ? `${t} ${r.value}` : r.value));
  };
  const entries = valueOf(list) ?? [];
  return (
    <PageSection id="checkin">
      <PageHero title="Check-in" statement="How are you, Taisun?" detail={`Evening check-in · takes about 10 seconds · reminder at ${PERSONAL_DEFAULTS.checkInReminder}`} />
      <Card title="Check-in" accessory={<span className="pk-acc">{wiredTag(list)}</span>}>
        <form className="ci-form" onSubmit={(e) => { void save(e); }}>
          <Scale label="Mood" steps={MOOD_STEPS} value={mood} onChange={setMood} />
          <div className="pk-g2 ci-row">
            <Scale label="Energy" steps={ENERGY_STEPS} value={energy} onChange={setEnergy} />
            <div className="pk-field"><span id="ci-tags">Tags</span>
              <div className="pk-chips" role="group" aria-labelledby="ci-tags">
                {TAGS.map((t) => <button key={t} type="button" className="pk-chip ci-chip" aria-pressed={tags.includes(t)} data-on={tags.includes(t) || undefined} onClick={() => setTags((x) => (x.includes(t) ? x.filter((y) => y !== t) : [...x, t]))}>{t}</button>)}
              </div>
            </div>
          </div>
          <div className="pk-composer ci-composer">
            <label htmlFor="ci-text" className="ui-visually-hidden">What happened today</label>
            <input id="ci-text" className="ci-input" value={text} maxLength={2000} placeholder="Anything worth remembering about today?" onChange={(e) => setText(e.target.value)} />
            <button type="button" className="pk-mic ci-mic" aria-label="Say it: transcribed to text, no audio kept" onClick={() => { void voice(); }}><Mic size={13} aria-hidden="true" /></button>
            <Button type="submit" size="small" variant="primary" disabled={mood == null || energy == null}>Save</Button>
          </div>
          <p className="pk-note">Pick a mood and energy, add tags or a line if you like. The mic transcribes on this Mac and only the text is saved; no audio is stored. {wired ? "" : "Saving needs wiring."}</p>
          {note && <p className="pk-note ci-status" role="status">{note}</p>}
        </form>
      </Card>
      <div className="pk-g2">
        <Card title="Recent entries" accessory={<span className="pk-acc">{wiredTag(list)}</span>}>
          {!wired ? <p className="pk-note">{WIRE_NOTE}</p> : entries.length === 0 ? <p className="pk-note">No check-ins yet.</p> : entries.slice(0, 3).map((c: CheckIn) => (
            <div className="pk-entry" key={c.id}>
              <time>{c.when}</time>
              <div><span className="pk-sub">Mood {c.mood} · {MOOD_STEPS[c.mood - 1]} · Energy {c.energy}{c.transcribed ? " · transcribed" : ""}</span>
                <p>{c.text || "No note"}</p>
                {c.tags.length > 0 && <span className="pk-chips">{c.tags.map((t) => <span key={t} className="pk-chip">{t}</span>)}</span>}</div>
            </div>
          ))}
        </Card>
        <Card title="Next to your health data">
          <p className="pk-note">Each check-in shows on the Mind–Body timeline beside that day&apos;s sleep, HRV and Neuro session. Patterns use mood and tags after enough days.</p>
        </Card>
      </div>
    </PageSection>
  );
}

/** Life › Weekly review (1111a #7, design-1111v): health, work and money, one suggested change, highlights and notes.
 *  Today shows "Your weekly review is ready ›" on Sundays and links here. */
/** "Not this week" is kept on this Mac per week and suggestion, so the same suggestion doesn't come straight back. */
const SKIP_KEY = "nexus.review.skipped";
const readSkips = (): string[] => { try { const v: unknown = JSON.parse(localStorage.getItem(SKIP_KEY) ?? "[]"); return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string").slice(-40) : []; } catch { return []; } };
const writeSkips = (v: string[]) => { try { localStorage.setItem(SKIP_KEY, JSON.stringify(v.slice(-40))); } catch { /* private window: the choice holds until the page is left */ } };

export function WeeklyReviewSection({ adapter, onOpenCheckIn, onOpenGoal, onOpenPatterns }: { adapter: PersonalAdapter; onOpenCheckIn?: () => void; onOpenGoal?: (goalId: string) => void; onOpenPatterns?: () => void }) {
  const wr = adapter.weeklyReview();
  const v = valueOf(wr);
  const [why, setWhy] = useState(false);
  const [decided, setDecided] = useState<string | null>(null);
  const hasTakeChange = valueOf(takesAdapter().week())?.change != null;
  const [skips, setSkips] = useState<string[]>(readSkips);
  const skipKey = v ? `${v.week}|${v.change.findingId ?? v.change.title}` : "";
  const skipped = skipKey !== "" && skips.includes(skipKey);
  const setSkipped = (on: boolean) => { const next = on ? [...skips, skipKey] : skips.filter((k) => k !== skipKey); setSkips(next); writeSkips(next); };
  const decide = async (take: boolean) => {
    if (!take) { setSkipped(true); setDecided(null); return; }
    const r = await adapter.tryChange(take);
    setDecided(r.state === "unwired" ? "Needs wiring, so nothing was set." : `${r.state === "sample" ? "Sample data: " : ""}${r.value.reminder ?? "Reminder set"}. Your goals didn't change.`);
  };
  return (
    <div className="v-page" id="weekly-review">
      <PageHero title="Weekly review" statement={v ? v.title : "Your week, every Sunday"} detail={v ? <>{v.week} {wr.state === "sample" && <DataTag kind="sample" />}</> : <>Ready {PERSONAL_DEFAULTS.weeklyReview} from your health, work and money <DataTag kind="wire" /></>} />
      <AmaraTake page="life" className="life__take" />
      {!v ? <Card title="This week" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">{WIRE_NOTE}</p></Card> : (
        <>
          <div className="t-tiles4">
            {v.tiles.map((t) => (
              <div className="pk-review" key={t.key}><h3>{t.key.toUpperCase()}</h3><span className="pk-big">{t.big} <small>{t.unit}</small></span>
                <span className="pk-delta" data-d={t.up ? "up" : "down"}>{t.delta}</span><span className="pk-sub">{t.sub}</span></div>
            ))}
            <WeekSuggestionsTile title="SUGGESTIONS DONE" />
          </div>
          <WeekSuggestionsCard />
          {hasTakeChange ? <TakeChange /> : <div className="pk-change">
            <span className="pk-eyebrow">ONE CHANGE TO TRY THIS WEEK</span>
            <h3>{v.change.title}</h3>
            <p><Rich text={v.change.body} /> {wr.state === "sample" && <DataTag kind="sample" />}</p>
            {why && <p className="pk-note">{v.change.findingId ? `Why: Amara's pattern “${humanize(v.change.findingId.replace(/[:_]/g, " "))}” in Mind–Body › Patterns says so.` : "Why: this comes from your own data in Mind–Body › Patterns."} {v.change.body} Try it sets a reminder; it never changes a goal by itself. {onOpenPatterns && <button type="button" className="pk-link" onClick={onOpenPatterns}>Open Patterns ›</button>}</p>}
            {skipped ? (
              <div className="pk-actions"><p className="pk-note" role="status">Skipped for this week. Amara won&apos;t bring this one back until next Sunday&apos;s review.</p><Button size="small" variant="plain" onClick={() => setSkipped(false)}>Undo</Button></div>
            ) : <div className="pk-actions">
              <Button size="small" variant="primary" onClick={() => { void decide(true); }} disabled={decided !== null}>Try it this week</Button>
              <Button size="small" onClick={() => { void decide(false); }} disabled={decided !== null}>Not this week</Button>
              <button type="button" className="pk-link" aria-expanded={why} onClick={() => setWhy((x) => !x)}>Why? ›</button>
            </div>}
            {decided && <p className="pk-note" role="status">{decided}</p>}
          </div>}
          <LastTryCard experiments={v.experiments ?? []} />
          <GoalsThisWeekCard onOpenGoal={onOpenGoal} />
          <TakesThisWeekCard />
          <div className="pk-g2">
            <Card title="Highlights" accessory={<span className="pk-acc">{wr.state === "sample" && <DataTag kind="sample" />}</span>}>
              {v.highlights.map((h) => <div className="pk-row" key={h.title}><StatusText kind={h.ok ? "ok" : "wait"} /><div className="pk-row__main"><span>{h.title}</span><small>{h.sub}</small></div>{h.goalId && onOpenGoal && <Button size="small" variant="plain" onClick={() => onOpenGoal(h.goalId as string)} aria-label={`Open goal: ${h.title}`}>Open goal ›</Button>}</div>)}
            </Card>
            <Card title="Your notes this week" accessory={<span className="pk-acc">{wr.state === "sample" && <DataTag kind="sample" />}</span>}>
              {v.notes.map((n) => <p className="pk-take" key={n.text}>&ldquo;{n.text}&rdquo; · {n.day}</p>)}
            </Card>
          </div>
          <SuggestedLatelyCard title="Suggested lately · not goals yet" />
          <Card title="Journal" accessory={<span className="pk-acc">{wr.state === "sample" && <DataTag kind="sample" />}</span>}>
            <div className="v-linkrow"><p>Your evening check-ins live on Today. Each one shows on the Mind–Body timeline.</p><span className="v-grow" />{onOpenCheckIn && <button type="button" className="v-link" onClick={onOpenCheckIn}>Open Check-in ›</button>}</div>
          </Card>
        </>
      )}
    </div>
  );
}
