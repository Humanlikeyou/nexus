import { RefreshCw } from "lucide-react";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, StatTile } from "../ui";
import { humanize } from "../lib/format";
import type { Goal } from "../types";

/** Trajectory folded into Home (screen-specs §6). Goals only; the readiness block is dropped. */
/** Deadlines are calendar dates: no time of day (the shared date() formatter includes one). */
const targetDate = (value: string) => new Date(value).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "Asia/Tokyo" });

/** A goal's roadmap progress (steps done / total, percent), when it has a roadmap. */
export type RoadmapOf = (goalId: string) => { pct: number; done: number; total: number } | null;

/** The roadmap's percent when the goal has one (a step done moves it); otherwise the goal's own current / target. */
export const goalProgress = (goal: Goal, roadmapOf?: RoadmapOf) => {
  const rm = roadmapOf?.(goal.id);
  return rm ? Math.max(0, Math.min(100, rm.pct)) : goal.target > 0 ? Math.min(100, (goal.current / goal.target) * 100) : 0;
};

export function GoalsCard({ goals, failed, onRetry, id = "goals", title = "Goals", roadmapOf }: { goals: Goal[]; failed?: boolean; onRetry?: () => void; id?: string; title?: string; roadmapOf?: RoadmapOf }) {
  const rows = goals.map((goal) => ({ goal, progress: goalProgress(goal, roadmapOf), rm: roadmapOf?.(goal.id) ?? null }));
  const overall = rows.length ? rows.reduce((sum, r) => sum + r.progress, 0) / rows.length : 0;
  const leader = [...rows].sort((a, b) => b.progress - a.progress)[0];

  return (
    <Card id={id || undefined} className="home__goals" title={title} accessory={rows.length > 0 ? <Badge>{`${rows.length} active`}</Badge> : undefined}>
      {failed && rows.length === 0 ? (
        <div className="home__unavailable">
          <p>Legacy goals unavailable</p>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
        </div>
      ) : rows.length === 0 ? (
        <div className="home__note"><p><strong>No goals yet</strong></p><p>Goals you set with Amara appear here with their progress.</p></div>
      ) : (
        <>
          <div className="home__pair">
            <StatTile label="Overall" value={Math.round(overall)} unit="%" />
            <StatTile label="Furthest along" value={leader ? `${leader.goal.title} · ${Math.round(leader.progress)}%` : null} />
          </div>
          <InsetGroup>
            {rows.map(({ goal, progress, rm }) => (
              <ListRow
                key={goal.id}
                title={goal.title}
                subtitle={<>{goal.deadline ? `${humanize(goal.category)} · Target ${targetDate(goal.deadline)}` : `${humanize(goal.category)} · No target date`}<span className="home__goal-callout">{rm ? `${rm.done} of ${rm.total} steps` : <>{goal.target > 0 ? `${goal.current.toLocaleString()} / ${goal.target.toLocaleString()}` : goal.current.toLocaleString()} {goal.unit}</>}</span></>}
                value={rm || goal.target > 0 ? `${Math.round(progress)}%` : "No target"}
                accessory={rm || goal.target > 0 ? <Meter className="home__goal-meter" size="sm" label={`${goal.title} progress`} value={rm ? progress : goal.current} max={rm ? 100 : goal.target} valueText={`${Math.round(progress)}%`} /> : undefined}
              />
            ))}
          </InsetGroup>
        </>
      )}
    </Card>
  );
}
