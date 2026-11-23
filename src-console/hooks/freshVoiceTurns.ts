import type { VoiceTurn } from "../types";

/** A spoken turn older than this is history, never a live command. */
export const FRESH_TURN_MS = 2 * 60_000;
const MAX_SEEN = 400;

/** What was said: the wake-stripped request (`text`); `heard` only for old lines that have no `text`. */
export function spokenText(t: VoiceTurn): string {
  return (t.text !== undefined ? t.text : t.heard ?? "").trim();
}

/** Turns amara-voice did not act on (ignored, noise, low confidence) are never commands. */
export function actedOn(t: VoiceTurn): boolean {
  return t.route !== "ignored" && !t.ignored && t.verdict !== "noise" && t.verdict !== "low";
}

/**
 * Tracks which spoken turns were already seen. `take` returns the new, fresh (under 2 minutes), acted-on turns, oldest
 * first. Everything unseen is marked seen, so the history that loads after an empty first poll is never replayed.
 */
export function createTurnTracker(now: () => number = () => Date.now()) {
  const seen = new Set<string>();
  return (turns: VoiceTurn[]): VoiceTurn[] => {
    const out: { t: VoiceTurn; at: number }[] = [];
    for (const t of turns) {
      const key = `${t.time ?? ""}:${t.text ?? t.heard ?? ""}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const at = Date.parse(t.time ?? "");
      if (Number.isFinite(at) && now() - at <= FRESH_TURN_MS && now() - at >= -FRESH_TURN_MS && actedOn(t)) out.push({ t, at });
    }
    if (seen.size > MAX_SEEN) [...seen].slice(0, seen.size - MAX_SEEN).forEach((k) => seen.delete(k));
    return out.sort((a, b) => a.at - b.at).map((x) => x.t);
  };
}
