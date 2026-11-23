import { useEffect, useState } from "react";
import { goalProgressWeek, listFollowups, type GoalProgressWire } from "../bridge";
import { goalStore } from "../design1118/goalStore";
import { humanize } from "../lib/format";
import { inTauri } from "../lib/localStores";
import type { ExperimentResultWire } from "../personal/liveInsights";
import { Button, Card } from "../ui";

/** Weekly review › "Goals this week": steps finished per goal (`goal_progress_week`) and goals that have stalled.
 *  Nothing when there is no backend to ask (browser, tests); a failed read says so. */
export function GoalsThisWeekCard({ onOpenGoal, load }: { onOpenGoal?: (goalId: string) => void; load?: () => Promise<GoalProgressWire> }) {
  const [state, setState] = useState<{ kind: "wait" } | { kind: "ok"; w: GoalProgressWire } | { kind: "failed" }>({ kind: "wait" });
  const loader = load ?? (inTauri() ? () => goalProgressWeek() : null);
  useEffect(() => {
    if (!loader) return;
    let live = true;
    loader().then((w) => { if (live) setState({ kind: "ok", w }); }).catch(() => { if (live) setState({ kind: "failed" }); });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!loader || state.kind === "wait") return null;
  if (state.kind === "failed") return <Card title="Goals this week"><p className="pk-note">Goal progress couldn&apos;t be read just now. Your goals are unchanged.</p></Card>;
  const { goals, stalled } = state.w;
  const finished = goals.filter((g) => g.thisWeek > 0);
  const total = finished.reduce((n, g) => n + g.thisWeek, 0);
  return (
    <Card title="Goals this week">
      <p className="pk-note">{total > 0 ? `${total} step${total === 1 ? "" : "s"} finished across ${finished.length} goal${finished.length === 1 ? "" : "s"}.` : "No steps finished this week."}</p>
      {finished.map((g) => (
        <div className="pk-row" key={g.goalId}>
          <div className="pk-row__main"><span>{g.title ?? "A goal"}</span><small>{g.thisWeek} step{g.thisWeek === 1 ? "" : "s"} this week · {g.prevWeek} last week</small></div>
          {onOpenGoal && <Button size="small" variant="plain" onClick={() => onOpenGoal(g.goalId)} aria-label={`Open goal: ${g.title ?? "goal"}`}>Open goal ›</Button>}
        </div>
      ))}
      {stalled.length > 0 && <p className="pk-note">Stalled: no step finished for a while.</p>}
      {stalled.map((g) => (
        <div className="pk-row" key={g.goalId}>
          <div className="pk-row__main"><span>{g.title}</span><small>{g.daysSince != null ? `Last step ${g.daysSince} days ago` : "No step finished yet"}</small></div>
          {onOpenGoal && <Button size="small" variant="plain" onClick={() => onOpenGoal(g.goalId)} aria-label={`Open goal: ${g.title}`}>Open goal ›</Button>}
        </div>
      ))}
    </Card>
  );
}

const MONTH_DAY = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const mean = (v: number | null) => (v == null ? "–" : String(Math.round(v * 10) / 10));
const dayOf = (iso: string) => (/^\d{4}-\d{2}-\d{2}$/.test(iso) ? MONTH_DAY.format(new Date(`${iso}T00:00:00Z`)) : iso);

/** The try's own words: the follow-up text Try-it wrote ("Try this week: <title>"), by follow-up id. */
async function tryTitles(): Promise<Map<number, string>> {
  const m = new Map<number, string>();
  try {
    const l = await listFollowups();
    for (const i of [...l.open, ...l.recentlyDone]) m.set(i.id, i.text.replace(/^Try this week:\s*/i, ""));
  } catch { /* the card falls back to the measure */ }
  return m;
}

/** Weekly review › "Last week's try": each finished experiment's label and before/after, with "Make it a habit". */
export function LastTryCard({ experiments, titles }: { experiments: ExperimentResultWire[]; titles?: () => Promise<Map<number, string>> }) {
  const [names, setNames] = useState<Map<number, string>>(new Map());
  const [notes, setNotes] = useState<Record<number, string>>({});
  useEffect(() => {
    if (experiments.length === 0 || (!titles && !inTauri())) return;
    let live = true;
    (titles ?? tryTitles)().then((m) => { if (live) setNames(m); }).catch(() => { /* fallback title */ });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [experiments.length]);
  if (experiments.length === 0) return null;
  const habit = async (e: ExperimentResultWire, title: string) => {
    const r = await goalStore().addHabit({ title, rule: "from a try that worked", perWeek: 5 });
    setNotes((n) => ({ ...n, [e.followupId]: r.state === "unwired" ? "Not saved. The goals store isn't answering, so nothing was added." : `Added “${title}” to your habits. See Goals.` }));
  };
  return (
    <Card title="Last week's try">
      {experiments.map((e) => {
        const title = names.get(e.followupId) ?? (e.measure ? `Try on ${humanize(e.measure.replace(/_mean$/, ""))}` : "Your try");
        const clear = e.outcome !== "no_clear_difference";
        const both = e.before.mean != null && e.after.mean != null;
        return (
          <div className="pk-row" key={e.followupId}>
            <div className="pk-row__main">
              <span>{title}</span>
              <small>Started {dayOf(e.startDay)} · {clear ? `Looks ${e.label}` : "No clear difference"}
                {both ? ` · before ${mean(e.before.mean)} (${e.before.n} days) → after ${mean(e.after.mean)} (${e.after.n} days)` : e.reason === "not_enough_data" ? " · not enough days with data yet" : e.reason === "no_measure" ? " · nothing to measure it by" : ""}</small>
              {notes[e.followupId] && <small role="status">{notes[e.followupId]}</small>}
            </div>
            {e.outcome === "better" && !notes[e.followupId] && <Button size="small" variant="primary" onClick={() => { void habit(e, title); }}>Make it a habit</Button>}
          </div>
        );
      })}
    </Card>
  );
}
