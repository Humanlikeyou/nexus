import type { SystemSnapshot } from "../types";
import { time } from "../lib/format";

export function Footer({ data, busy }: { data: SystemSnapshot; busy: boolean }) {
  return (
    <footer className="field-footer">
      <span>Local only</span><i className={data.hermes.status === "online" ? "is-live" : ""} />
      <span>{data.hermes.model}</span><span>{data.capabilities.length} capabilities</span>
      {data.unavailableSources.length > 0 && <span className="footer-unavailable" title={data.unavailableSources.map((source) => source.label).join(", ")}>{data.unavailableSources.length} unavailable</span>}
      <span className="footer-spacer" />
      <span>{busy ? "Refreshing" : `Updated ${time(data.refreshedAt)}`}</span>
    </footer>
  );
}
