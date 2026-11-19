import { RefreshCw } from "lucide-react";
import { dateTime, humanize, plainText, sentenceStart, shortDuration } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, InsetGroup, ListRow, More, PageHero } from "../ui";
import { activityBadge } from "./businessTone";
import { CREWS_ROUTE, type Route } from "../app/routes";
import { deliveryLine } from "../design1118/Agents";
import "./ActivityScreen.css";

/** Agents › Activity (screen-specs §10): delivery outbox health + Amara's operation log.
 *  SignalStream (the decorative sci-fi line) is retired. */
function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="activity__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

const FIRST_GLANCE = 3;

export function ActivityField({ data, onRetry, onOpen }: { data: SystemSnapshot; onRetry?: () => void; onOpen?: (route: Route) => void }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const { outbox, activity } = data;
  const outboxDown = down("get_outbox_status");
  // design-1118a round-1 fix: never "All delivered" next to "Nothing delivered yet".
  const activityStatement = deliveryLine(outbox, outboxDown);
  const row = (item: SystemSnapshot["activity"][number]) => {
    const detail = item.detail || item.outcome || "";
    const badge = activityBadge(item.status);
    return (
      <ListRow key={item.id} title={sentenceStart(plainText(item.name.replace(/_/g, " ")))}
        subtitle={<span className="activity__clamp" title={detail || undefined}>{humanize(item.type)}{detail ? ` · ${detail}` : ""}</span>}
        value={`${shortDuration(item.durationMs)} · ${dateTime(item.startedAt)}`}
        accessory={<Badge tone={badge.tone}>{badge.label}</Badge>}
        onSelect={onOpen && /crew/i.test(`${item.type} ${item.name}`) ? () => onOpen(CREWS_ROUTE) : undefined} />
    );
  };
  return (
    <div className="activity page-inset">
      <PageHero title="Activity" statement={activityStatement}
        detail={outboxDown ? undefined : outbox.last_delivered_at ? `Last delivered ${dateTime(outbox.last_delivered_at)}` : activityStatement === "No deliveries yet" ? "Amara delivers to you through the gateway once something is ready." : "Nothing delivered yet"} />
      {outboxDown && <Unavailable label="Delivery outbox" onRetry={onRetry} />}

      {down("list_activity_entries") && activity.length === 0 ? <Unavailable label="Activity log" onRetry={onRetry} />
        : activity.length === 0 ? <p className="activity__note">Mac actions and Amara&apos;s work appear here as they finish.</p>
          : <InsetGroup title="Recent activity">{activity.slice(0, FIRST_GLANCE).map(row)}</InsetGroup>}

      <More label={activity.length > FIRST_GLANCE ? `${activity.length - FIRST_GLANCE} more, and delivery` : "Delivery"}>
        {activity.length > FIRST_GLANCE && <InsetGroup title="Earlier" footer="Last 30 operations">{activity.slice(FIRST_GLANCE).map(row)}</InsetGroup>}
        {!outboxDown && (
          <InsetGroup title="Gateway delivery">
            <ListRow title="Gateway" accessory={<Badge tone={outbox.gateway_online ? "success" : "warning"}>{outbox.gateway_online ? "Gateway online" : "Gateway waiting"}</Badge>} />
            <ListRow title="Queued" value={String(outbox.queue_depth)} />
            <ListRow title="Delivered" value={String(outbox.total_delivered)} />
            {(outbox.expired ?? 0) > 0 && <ListRow title="Expired" subtitle="Older than 24h, kept, not sent" value={String(outbox.expired)} />}
            {outbox.last_error && <ListRow title="Last error" subtitle={<span className="activity__error">{outbox.last_error}</span>} />}
          </InsetGroup>
        )}
      </More>
    </div>
  );
}
