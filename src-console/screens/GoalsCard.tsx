import { RefreshCw } from "lucide-react";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, StatTile } from "../ui";
import { humanize } from "../lib/format";
import type { Goal } from "../types";

/** Trajectory folded into Home (screen-specs §6). Goals only; the readiness block is dropped. */
/** Deadlines are calendar dates: no time of day (the shared date() formatter includes one). */
const targetDate = (value: string) => new Date(value).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "Asia/Tokyo" });

export const goalProgress = (goal: Goal) => (goal.target > 0 ? Math.min(100, (goal.current / goal.target) * 100) : 0);

export function GoalsCard({ goals, failed, onRetry }: { goals: Goal[]; failed?: boolean; onRetry?: () => void }) {
  const rows = goals.map((goal) => ({ goal, progress: goalProgress(goal) }));
  const overall = rows.length ? rows.reduce((sum, r) => sum + r.progress, 0) / rows.length : 0;
  const leader = [...rows].sort((a, b) => b.progress - a.progress)[0];

  return (
    <Card id="goals" className="home__goals" title="Goals" accessory={rows.length > 0 ? <Badge>{`${rows.length} active`}</Badge> : undefined}>
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
            {rows.map(({ goal, progress }) => (
              <ListRow
                key={goal.id}
                title={goal.title}
                subtitle={<>{goal.deadline ? `${humanize(goal.category)} · Target ${targetDate(goal.deadline)}` : `${humanize(goal.category)} · No target date`}<span className="home__goal-callout">{goal.target > 0 ? `${goal.current.toLocaleString()} / ${goal.target.toLocaleString()}` : goal.current.toLocaleString()} {goal.unit}</span></>}
                value={goal.target > 0 ? `${Math.round(progress)}%` : "No target"}
                accessory={goal.target > 0 ? <Meter className="home__goal-meter" size="sm" label={`${goal.title} progress`} value={goal.current} max={goal.target} valueText={`${Math.round(progress)}%`} /> : undefined}
              />
            ))}
          </InsetGroup>
        </>
      )}
    </Card>
  );
}
