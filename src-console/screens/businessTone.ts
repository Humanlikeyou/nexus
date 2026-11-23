import { humanize, tone } from "../lib/format";
import type { Tone } from "../ui";

/** Legacy status tone (live/watch/fault/quiet) → ui Badge tone. */
export function badgeTone(value: string | boolean | null | undefined): Tone {
  // One status vocabulary (P0.5): Active is emerald everywhere, as on Today.
  if (typeof value === "string" && value.toLowerCase() === "active") return "accent";
  const t = tone(value);
  return t === "live" ? "success" : t === "watch" ? "warning" : t === "fault" ? "danger" : "neutral";
}

/**
 * Activity run status → Badge (existing Decision 20 tones, no new colours). Completed keeps its
 * stored wording; failures read "Failed" whether stored as "failed" or "error"; skipped,
 * suppressed and cancelled are neutral; anything else falls back to badgeTone.
 */
export function activityBadge(status: string): { tone: Tone; label: string } {
  const s = status.toLowerCase();
  if (s === "complete" || s === "completed") return { tone: "success", label: humanize(status) };
  if (s === "failed" || s === "error") return { tone: "danger", label: "Failed" };
  if (s === "running") return { tone: "accent", label: "Running" };
  if (s === "skipped" || s === "suppressed" || s === "cancelled") return { tone: "neutral", label: humanize(status) };
  return { tone: badgeTone(status), label: humanize(status) };
}
