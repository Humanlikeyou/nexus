import { number, date } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { State } from "./shared";

export function TrajectoryField({ data }: { data: SystemSnapshot }) {
  const goalProgress = data.goals.map((goal) => ({
    goal,
    progress: goal.target > 0 ? Math.min(100, goal.current / goal.target * 100) : 0,
  }));
  const portfolioProgress = goalProgress.length
    ? goalProgress.reduce((total, item) => total + item.progress, 0) / goalProgress.length
    : 0;
  const leadingGoal = [...goalProgress].sort((left, right) => right.progress - left.progress)[0];
  const latestNight = data.noop.latestNight;
  const hasHealthData = data.noop.status === "ready" && (data.noop.today != null || latestNight != null);
  const dailyMatchesNight = latestNight == null || data.noop.latestDay === latestNight.day;
  const readinessRecovery = dailyMatchesNight ? data.noop.today?.recovery : null;
  const readinessHrv = latestNight?.avgHrv ?? (dailyMatchesNight ? data.noop.today?.avgHrv : null);
  const readinessSleep = latestNight?.durationMin ?? (dailyMatchesNight ? data.noop.today?.totalSleepMin : null);

  return (
    <div className="system-field trajectory-field">
      <section className="trajectory-heading">
        <div><span className="system-kicker">Direction</span><h1>Trajectory</h1><p>Keep the work pointed at the life you are building.</p></div>
        <div className="trajectory-heading-status"><strong>{data.goals.length}</strong><span>active directions</span></div>
      </section>

      <section className="trajectory-summary" aria-label="Trajectory summary">
        <div><span>Portfolio progress</span><strong>{portfolioProgress.toFixed(0)}<small>%</small></strong><p>Across every active direction.</p></div>
        <div><span>Furthest along</span><strong>{leadingGoal ? leadingGoal.goal.title : "No goal selected"}</strong><p>{leadingGoal ? `${leadingGoal.progress.toFixed(0)}% of its current target` : "Add a direction when you are ready."}</p></div>
        <div><span>Readiness source</span><strong>{hasHealthData ? "NOOP connected" : "Awaiting NOOP data"}</strong><p>{hasHealthData ? `Latest record: ${latestNight?.day ?? data.noop.latestDay ?? "today"}` : "Pair or import in NOOP to bring daily recovery here."}</p></div>
      </section>

      <section className="trajectory-map">
        <header><span>Active directions</span><small>Progress toward the targets you set</small></header>
        {goalProgress.length > 0 ? goalProgress.map(({ goal, progress }) => <article key={goal.id}>
          <div className="trajectory-goal-title"><span>{goal.category}</span><strong>{goal.title}</strong></div>
          <div className="trajectory-progress"><i><b style={{ width: `${progress}%` }} /></i><small>{goal.current.toLocaleString()} / {goal.target.toLocaleString()} {goal.unit}</small></div>
          <em>{progress.toFixed(0)}%</em>
          <time>{goal.deadline ? `Target ${date(goal.deadline)}` : "No target date"}</time>
        </article>) : <p className="trajectory-empty">Add the directions you want your time and energy to compound toward.</p>}
      </section>

      <section className="wellness-vector">
        <header><span>Daily readiness</span><State value={hasHealthData ? "ready" : "waiting"} label={hasHealthData ? "LIVE DATA" : "WAITING FOR NOOP"} /></header>
        <p>{hasHealthData ? "A compact health signal alongside the work, not the center of it." : "Your daily health signal will appear here once NOOP has a local record."}</p>
        <div className="wellness-metrics">
          <div><span>Recovery</span><strong>{number(readinessRecovery)}<small>%</small></strong></div>
          <div><span>HRV</span><strong>{number(readinessHrv)}<small>ms</small></strong></div>
          <div><span>Sleep</span><strong>{number(readinessSleep == null ? null : readinessSleep / 60, 1)}<small>h</small></strong></div>
          <div><span>Strain</span><strong>{number(data.noop.today?.strain, 1)}</strong></div>
        </div>
      </section>
    </div>
  );
}
