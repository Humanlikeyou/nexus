import { tone } from "../lib/format";
import type { Tone } from "../ui";

/** Legacy status tone (live/watch/fault/quiet) → ui Badge tone. */
export function badgeTone(value: string | boolean | null | undefined): Tone {
  const t = tone(value);
  return t === "live" ? "success" : t === "watch" ? "warning" : t === "fault" ? "danger" : "neutral";
}
