import type { SystemSnapshot } from "../types";
import { time } from "../lib/format";
import "./shell.css";

/** Live status worth a footer: a refresh in flight, unavailable sources, or Hermes not online. */
export function footerHasLiveStatus(data: SystemSnapshot, busy: boolean): boolean {
  return busy || data.unavailableSources.length > 0 || data.hermes.status !== "online";
}

/** 24px status footer, rendered only while it has live status (screen-specs shared rules). */
export function Footer({ data, busy }: { data: SystemSnapshot; busy: boolean }) {
  if (!footerHasLiveStatus(data, busy)) return null;
  const online = data.hermes.status === "online";
  return (
    <footer className="field-footer">
      <i className={online ? "is-live" : ""} aria-hidden="true" />
      <span>{online ? data.hermes.model : "Amara offline"}</span>
      {data.unavailableSources.length > 0 && <span className="footer-unavailable" title={data.unavailableSources.map((source) => source.label).join(", ")}>{data.unavailableSources.length} unavailable</span>}
      <span className="footer-spacer" />
      <span>{busy ? "Refreshing…" : `Updated ${time(data.refreshedAt)}`}</span>
    </footer>
  );
}
