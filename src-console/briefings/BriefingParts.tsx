// Decision 0066: the pieces a briefing is shown with. Existing primitives and class names only
// (Card, StatusText, pk-* and lf-* classes); no new styles.
import { Card, StatusText } from "../ui";
import { KIND_LABEL, TRIGGER_LABEL, type BriefingRuntime, type BriefingSection, type GoalTrajectory, type NeuroSummary, type TrainingSummary } from "./briefingApi";

const when = (iso: string | null | undefined) => {
  if (!iso) return "never";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("en-GB", { timeZone: "Asia/Tokyo", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
};

/** How the briefings actually run: shown on every briefing so nobody assumes more. */
export function RuntimeNote({ runtime, job }: { runtime: BriefingRuntime | null; job: "personal" | "enterprise" }) {
  const j = runtime?.jobs.find((x) => x.id === job);
  return (
    <Card title="When this runs" headingLevel={3}>
      <p className="pk-note">{runtime?.note ?? "Briefings are built by Amara OS while it is open. Nothing runs while it is closed."}</p>
      {j && (
        <ul className="pk-list" aria-label="Schedule">
          <li><b>{j.label}</b> · {j.schedule}</li>
          <li>Last built {when(j.state.lastRunAt)}{j.state.lastTrigger ? ` (${TRIGGER_LABEL[j.state.lastTrigger] ?? j.state.lastTrigger})` : ""}{j.state.lastResult === "partial" ? ", with some sources missing" : j.state.lastResult === "failed" ? ", and it failed" : ""}.</li>
          <li>Next due {when(j.nextDueAt)}, if Amara OS is open then.</li>
        </ul>
      )}
      <p className="pk-note">{runtime?.modelInference ?? "No language model is used."}</p>
    </Card>
  );
}

export function SectionCard({ s }: { s: BriefingSection }) {
  return (
    <Card title={s.title} headingLevel={3} accessory={s.state === "unavailable" ? <StatusText kind="off">Unavailable</StatusText> : undefined}>
      {s.lines.length > 0 && (
        <ul className="pk-list" aria-label={s.title}>
          {s.lines.map((l, i) => <li key={i}>{l.text} <small className="pk-dim">{KIND_LABEL[l.kind] ?? l.kind}</small></li>)}
        </ul>
      )}
      {s.note && <p className="pk-note">{s.note}</p>}
    </Card>
  );
}

const STATE_TEXT: Record<string, string> = {
  ahead: "Ahead of pace", on_pace: "On pace", behind: "Behind pace", no_deadline: "No deadline", not_started: "Not started", done: "Done", unknown: "Not enough recorded",
};

export function Trajectories({ list }: { list: GoalTrajectory[] }) {
  return (
    <Card title="Goal trajectories" headingLevel={3}>
      {list.length === 0 ? <p className="pk-note">No active goals.</p> : (
        <ul className="pk-list" aria-label="Goal trajectories">
          {list.map((t) => (
            <li key={t.goalId}>
              <b>{t.title}</b> · {STATE_TEXT[t.state] ?? t.state}
              <br /><span className="pk-note">{t.progressPct === null ? t.measure : `${Math.round(t.progressPct)}% (${t.measure})`}{t.expectedPct !== null ? `; ${Math.round(t.expectedPct)}% of its time has passed` : ""}{t.daysLeft !== null ? `; ${t.daysLeft} days left` : ""}; progress recorded {t.moves14d} time(s) in 14 days.{t.nextStep ? ` Next: ${t.nextStep}` : ""}</span>
            </li>
          ))}
        </ul>
      )}
      <p className="pk-note">Calculated from your recorded progress and each goal&apos;s start date and deadline.</p>
    </Card>
  );
}

export function TrainingCard({ t, title = "Training" }: { t: TrainingSummary; title?: string }) {
  const muscles = Object.entries(t.setsByMuscle ?? {});
  return (
    <Card title={title} headingLevel={3}>
      <p className="pk-note">{t.periodStart} – {t.periodEnd}</p>
      {t.sessions === 0 && t.detectedWorkoutMin === null ? <p className="pk-note">No workouts logged or detected in this period.</p> : (
        <ul className="pk-list" aria-label={title}>
          <li>{t.sessions} logged session(s), {t.doneSets} sets, {t.totalReps} reps, {Math.round(t.activeMin ?? 0)} active minutes.{t.baselineSetsPerWeek !== null ? ` Recent average: ${t.baselineSetsPerWeek} sets a week.` : ""}</li>
          {muscles.length > 0 && <li>Sets by muscle group: {muscles.map(([m, n]) => `${m} ${n}`).join(", ")}.</li>}
          {t.detectedWorkoutMin !== null && <li>WHOOP detected {t.detectedWorkoutMin} workout minutes{t.meanStrain !== null ? `, mean strain ${t.meanStrain}` : ""}.</li>}
        </ul>
      )}
      <p className="pk-note">{t.note}</p>
    </Card>
  );
}

export function NeuroCard({ n, title = "Neurotechnology sessions" }: { n: NeuroSummary; title?: string }) {
  return (
    <Card title={title} headingLevel={3}>
      <p className="pk-note">{n.periodStart} – {n.periodEnd}</p>
      {n.sessions === 0 ? <p className="pk-note">No Muse sessions in this period.</p> : (
        <ul className="pk-list" aria-label={title}>
          <li>{n.sessions} session(s), {Math.round(n.minutes ?? 0)} minutes; {n.eligibleTraining} training session(s) met the quality rules.</li>
          {n.medianTimeInTargetPct !== null && <li>Median time in target: {n.medianTimeInTargetPct}%.</li>}
          {n.medianValidPct !== null && <li>Median valid signal: {n.medianValidPct}%.</li>}
        </ul>
      )}
      {n.trends.length > 0 && <ul className="pk-list" aria-label="Trends">{n.trends.map((x) => <li key={x}>{x}</li>)}</ul>}
      <p className="pk-note">{n.note}</p>
    </Card>
  );
}
