import { RefreshCw } from "lucide-react";
import { humanize, plainText } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, StatTile } from "../ui";
import { badgeTone } from "./businessTone";
import "./BusinessScreen.css";

/** Business › Work (screen-specs §7). */
const STATES = ["active", "backlog", "waiting", "done"] as const;
const STAGES = ["ideation", "drafting", "review", "scheduled", "published"] as const;

function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="biz__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

export function BusinessField({ data, onRetry }: { data: SystemSnapshot; onRetry?: () => void }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const tasks = data.tasks;
  const other = tasks.filter((t) => !(STATES as readonly string[]).includes(t.status)).length;
  return (
    <div className="biz">
      <h1 className="biz__title">Business</h1>
      <div className="biz__tiles">
        {STATES.map((state) => <StatTile key={state} label={humanize(state)} value={tasks.filter((t) => t.status === state).length} />)}
        {other > 0 && <StatTile label="Other" value={other} />}
      </div>

      {down("read_tasks") && tasks.length === 0 ? <Card title="Work items"><Unavailable label="Task source" onRetry={onRetry} /></Card>
        : tasks.length === 0 ? (
          <Card title="Work items"><p className="biz__note"><strong>No work items yet</strong><br />Tracked priorities collect here as they&apos;re created.</p></Card>
        ) : (
          <InsetGroup title="Work items" footer={`${tasks.length} tracked`}>
            {tasks.map((task, i) => (
              <ListRow key={`${task.title}-${i}`} title={plainText(task.title)} subtitle={task.tags?.length ? task.tags.map(humanize).join(" · ") : undefined}
                value={task.priority ? humanize(task.priority) : "Normal"} accessory={<Badge tone={badgeTone(task.status)}>{humanize(task.status)}</Badge>} />
            ))}
          </InsetGroup>
        )}

      <Card title="Content pipeline" accessory={<Badge>{String(data.pipeline.length)}</Badge>}>
        {down("read_content_pipeline") && data.pipeline.length === 0 ? <Unavailable label="Content pipeline" onRetry={onRetry} />
          : data.pipeline.length === 0 ? <p className="biz__note"><strong>Pipeline is clear</strong><br />Content moves here from idea to publication.</p>
            : (
              <div className="biz__stages">
                {STAGES.map((stage) => {
                  const items = data.pipeline.filter((item) => item.stage === stage);
                  return (
                    <InsetGroup key={stage} className={items.length === 0 ? "biz__stage biz__stage--empty" : "biz__stage"} title={`${humanize(stage)} · ${items.length}`}>
                      {items.slice(0, 5).map((item, i) => <ListRow key={`${item.title}-${i}`} title={<span title={item.notes ?? undefined}>{plainText(item.title)}</span>} subtitle={humanize(item.content_type)} />)}
                    </InsetGroup>
                  );
                })}
              </div>
            )}
      </Card>
    </div>
  );
}
