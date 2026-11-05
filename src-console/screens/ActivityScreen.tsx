import { RefreshCw } from "lucide-react";
import { dateTime, humanize, plainText, shortDuration } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, StatTile } from "../ui";
import { activityBadge } from "./businessTone";
import "./ActivityScreen.css";

/** Chat & Agents › Activity (screen-specs §10): delivery outbox health + Amara's operation log.
 *  SignalStream (the decorative sci-fi line) is retired. */
function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="activity__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

export function ActivityField({ data, onRetry }: { data: SystemSnapshot; onRetry?: () => void }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const { outbox, activity } = data;
  const outboxDown = down("get_outbox_status");
  return (
    <div className="activity page-inset">
      <h1 className="activity__title">Activity</h1>
      <Card title="Gateway delivery" accessory={outboxDown ? undefined : <Badge tone={outbox.gateway_online ? "success" : "warning"}>{outbox.gateway_online ? "Gateway online" : "Gateway waiting"}</Badge>}>
        {outboxDown ? <Unavailable label="Delivery outbox" onRetry={onRetry} /> : (
          <>
            <div className="activity__tiles">
              <StatTile label="Queued" value={outbox.queue_depth} />
              <StatTile label="Delivered" value={outbox.total_delivered} />
            </div>
            <InsetGroup>
              <ListRow title="Last delivered" value={outbox.last_delivered_at ? dateTime(outbox.last_delivered_at) : "Never"} />
              {(outbox.expired ?? 0) > 0 && <ListRow title="Expired" subtitle="Older than 24h, kept, not sent" value={String(outbox.expired)} />}
              {outbox.last_error && <ListRow title="Last error" subtitle={<span className="activity__error">{outbox.last_error}</span>} />}
            </InsetGroup>
          </>
        )}
      </Card>

      {down("list_activity_entries") && activity.length === 0 ? <Card title="Recent activity"><Unavailable label="Activity log" onRetry={onRetry} /></Card>
        : activity.length === 0 ? (
          <Card title="Recent activity"><p className="activity__note"><strong>No recent activity</strong><br />Mac actions and Amara&apos;s work appear here as they finish.</p></Card>
        ) : (
          <InsetGroup title="Recent activity" footer="Last 30 operations">
            {activity.map((item) => {
              const detail = item.detail || item.outcome || "";
              const badge = activityBadge(item.status);
              return (
                <ListRow key={item.id} title={plainText(item.name.replace(/_/g, " "))}
                  subtitle={<span className="activity__clamp" title={detail || undefined}>{humanize(item.type)}{detail ? ` · ${detail}` : ""}</span>}
                  value={`${shortDuration(item.durationMs)} · ${dateTime(item.startedAt)}`}
                  accessory={<Badge tone={badge.tone}>{badge.label}</Badge>} />
              );
            })}
          </InsetGroup>
        )}
    </div>
  );
}
