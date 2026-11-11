import { Mic } from "lucide-react";
import { useState, type FormEvent, type ReactNode } from "react";
import { Button, Card, DataTag, PageHero, PageSection, StatusText, wiredTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { GoalsCard } from "../screens/GoalsCard";
import type { Goal } from "../types";
import { CHECKIN_EVENT, ENERGY_STEPS, MOOD_STEPS, PERSONAL_DEFAULTS, type CheckIn, type PersonalAdapter } from "./adapters";
import { Bar, Dots, Rich } from "./parts";
import { ReadyToKeepCard, SuggestedLatelyCard, TakeChange, TakesThisWeekCard, WeekSuggestionsCard, WeekSuggestionsTile } from "../takes/WeekTakes";
import { takesAdapter } from "../takes/adapters";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import "../ui/PanelKit.css";
import "./personal.css";
import "../adds/v1111.css";

const WIRE_NOTE = "Needs wiring: this appears once Claude builds its store (HANDOFF-1111a). Nothing here is made up.";

/** Life › Goals (1111a #1, design-1111v): the add composer, habits counted from their sources, and milestones.
 *  "Shows on" is gone and "Suggested lately" moved to Weekly review. */
export function GoalsSection({ adapter, goals, goalsFailed, onRetry, composer, added = [] }: { adapter: PersonalAdapter; goals: Goal[]; goalsFailed?: boolean; onRetry?: () => void; composer?: ReactNode; added?: Array<{ id: string; title: string }> }) {
  const week = adapter.goals();
  const w = valueOf(week);
  const onTrack = w ? w.habits.filter((h) => h.target - h.done <= 1).length : 0;
  const ready = valueOf(takesAdapter().goals())?.ready.length ?? 0;
  return (
    <div className="v-page" id="goals">
      <PageHero title="Goals" statement={w ? `${onTrack} of ${w.habits.length} habits on track this week` : "Habits and milestones"}
        detail={w ? <>{w.milestones.length} milestones{w.nextMilestone ? ` · next: ${w.nextMilestone}` : ""}{ready ? ` · ${ready} suggestion ready to become a habit` : ""} <DataTag kind="sample" /></> : <>Habits count themselves from Fitness, Neuro and check-ins once wired <DataTag kind="wire" /></>} />
      <AmaraTake page="life" className="life__take" />
      {composer}
      <div className="pk-g2">
        <Card title="This week's habits" accessory={<span className="pk-acc">{wiredTag(week)}</span>}>
          {!w ? <p className="pk-note">{WIRE_NOTE}</p> : (
            <>
              {added.map((h) => <div className="pk-habit v-new" key={h.id}><div>{h.title}<small>You · added just now · starts Monday</small></div><span /><div className="pk-streak"><b>0</b> / –<small>new</small></div></div>)}
              {w.habits.map((h) => (
                <div className="pk-habit" key={h.id}>
                  <div>{h.title}<small>{h.source} · {h.rule}</small></div>
                  <Dots days={h.days} label={h.title} />
                  <div className="pk-streak"><b>{h.done}</b> / {h.target}<small>{h.note}</small></div>
                </div>
              ))}
              <p className="pk-note">Days run Thu → today. Half squares: nearly (within 15 min). Amara counts from the source data; you can&apos;t tick these by hand unless a source is missing.</p>
            </>
          )}
        </Card>
        <div className="pk-stack">
          <ReadyToKeepCard title="Ready to become a habit" />
          {w ? (
            <Card title="Milestones" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
              {w.milestones.map((m) => (
                <div className="pk-ms" key={m.id}>
                  <div className="pk-ms__top"><span>{m.title}</span><span>{m.right}</span></div>
                  <Bar pct={m.pct} copper={m.tone === "copper"} />
                  <small className="pk-sub">{m.detail}</small>
                </div>
              ))}
            </Card>
          ) : <GoalsCard goals={goals} failed={goalsFailed} onRetry={onRetry} id="" title="Milestones" />}
        </div>
      </div>
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
export function WeeklyReviewSection({ adapter, onOpenCheckIn }: { adapter: PersonalAdapter; onOpenCheckIn?: () => void }) {
  const wr = adapter.weeklyReview();
  const v = valueOf(wr);
  const [why, setWhy] = useState(false);
  const [decided, setDecided] = useState<string | null>(null);
  const hasTakeChange = valueOf(takesAdapter().week())?.change != null;
  const decide = async (take: boolean) => {
    const r = await adapter.tryChange(take);
    setDecided(r.state === "unwired" ? "Needs wiring, so nothing was set." : take ? `${r.state === "sample" ? "Sample data: " : ""}${r.value.reminder ?? "Reminder set"}. Your goals didn't change.` : `${r.state === "sample" ? "Sample data: " : ""}Skipped for this week.`);
  };
  return (
    <div className="v-page" id="weekly-review">
      <PageHero title="Weekly review" statement={v ? v.title : "Your week, every Sunday"} detail={v ? <>{v.week} <DataTag kind="sample" /></> : <>Ready {PERSONAL_DEFAULTS.weeklyReview} from your health, work and money <DataTag kind="wire" /></>} />
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
            <p><Rich text={v.change.body} /> <DataTag kind="sample" /></p>
            {why && <p className="pk-note">From the pattern &ldquo;Training load → focus&rdquo; (Mind–Body › Patterns, medium confidence, 21 days). Try it sets a reminder; it never changes a goal by itself.</p>}
            <div className="pk-actions">
              <Button size="small" variant="primary" onClick={() => { void decide(true); }} disabled={decided !== null}>Try it this week</Button>
              <Button size="small" onClick={() => { void decide(false); }} disabled={decided !== null}>Not this week</Button>
              <button type="button" className="pk-link" aria-expanded={why} onClick={() => setWhy((x) => !x)}>Why? ›</button>
            </div>
            {decided && <p className="pk-note" role="status">{decided}</p>}
          </div>}
          <TakesThisWeekCard />
          <div className="pk-g2">
            <Card title="Highlights" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
              {v.highlights.map((h) => <div className="pk-row" key={h.title}><StatusText kind={h.ok ? "ok" : "wait"} /><div className="pk-row__main"><span>{h.title}</span><small>{h.sub}</small></div></div>)}
            </Card>
            <Card title="Your notes this week" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
              {v.notes.map((n) => <p className="pk-take" key={n.text}>&ldquo;{n.text}&rdquo; · {n.day}</p>)}
            </Card>
          </div>
          <SuggestedLatelyCard title="Suggested lately · not goals yet" />
          <Card title="Journal" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
            <div className="v-linkrow"><p>Your evening check-ins live on Today. Each one shows on the Mind–Body timeline.</p><span className="v-grow" />{onOpenCheckIn && <button type="button" className="v-link" onClick={onOpenCheckIn}>Open Check-in ›</button>}</div>
          </Card>
        </>
      )}
    </div>
  );
}
