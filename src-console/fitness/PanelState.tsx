import { Inbox, RefreshCw } from "lucide-react";
import { Button, DataTag } from "../ui";
import type { Load } from "./noopData";

/** A sample-data tag for any panel showing preview values. */
export const SampleTag = () => <DataTag kind="sample" />;

/**
 * Everything a Fitness tab shows when it has no values: loading, unwired ("Needs wiring"), an honest
 * empty state, or a failed read in plain words. `what` is a lower-case noun phrase ("workouts").
 */
export function PanelState({ load, what, emptyHint, onRetry }: { load: Load<unknown>; what: string; emptyHint: string; onRetry?: () => void }) {
  if (load.state === "loading") return <p className="fx-state" role="status" aria-live="polite"><span className="fx-skel" aria-hidden="true" />Loading {what}…</p>;
  if (load.state === "unwired") {
    return (
      <div className="fx-state fx-state--block" data-kind="unwired">
        <Inbox className="fx-state__icon" aria-hidden="true" />
        <p className="fx-state__lead"><DataTag kind="wire" /></p>
        <p>Amara can&apos;t read {what} on this Mac yet. Once the NOOP health store is available they appear here.</p>
        {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Check again</Button>}
      </div>
    );
  }
  if (load.state === "error") {
    return (
      <div className="fx-state fx-state--block" role="alert" data-kind="error">
        <p>Amara couldn&apos;t read {what} just now. Try again in a moment.</p>
        {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
      </div>
    );
  }
  if (load.state === "empty") {
    return (
      <div className="fx-state fx-state--block" data-kind="empty">
        <Inbox className="fx-state__icon" aria-hidden="true" />
        <p>No {what} yet.</p>
        <p className="fx-state__hint">{emptyHint}</p>
      </div>
    );
  }
  return null;
}
