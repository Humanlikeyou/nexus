import type { CatalogEntry, TakeKind } from "../amaraTake/catalog";
import type { Fact, FactSet } from "../amaraTake/types";
import type { Workout } from "./detectors";
import { answerOf, sidOf, todayRecords, type TakeEvent } from "./events";
import type { TakePage } from "../amaraTake/types";

/** Variety inputs for selection: what was offered (shown, not only answered), what he actually did, and the clock. All pure. */
export type OfferRecord = { day: string; recId: string; answer: "done" | "other" | "waiting" };
export type PickCtx = {
  /** Hour of day in Asia/Tokyo (0-23). Unknown leaves time-of-day conditions unmet. */
  hour?: number;
  /** Recent workouts, newest data last. `null` means unknown; an empty list means none. */
  workouts?: readonly Workout[] | null;
  /** Week strain over 28-day strain (mindBodyDeep.loadRatio). */
  loadRatio?: number | null;
  /** Everything offered before today for this page. */
  offers?: readonly OfferRecord[];
};

const DAY = 86_400_000;
export const daysBetween = (a: string, b: string) => Math.round((Date.parse(`${b}T00:00:00Z`) - Date.parse(`${a}T00:00:00Z`)) / DAY);

/** Offers for one page from the event log: every `today` suggestion record plus how it was answered ("did it" counts, detected or tapped). */
export function offerHistory(events: readonly TakeEvent[], page: TakePage, todayDay: string): OfferRecord[] {
  return todayRecords(events)
    .filter((r): r is Extract<typeof r, { kind: "suggestion" }> => r.kind === "suggestion" && r.page === page)
    .map((r) => {
      const st = answerOf(events, sidOf(page, r.day, r.recId), r.day, todayDay).answer.state;
      return { day: r.day, recId: r.recId, answer: st === "done" ? "done" : st === "waiting" || st === "unknown" ? "waiting" : "other" } as OfferRecord;
    });
}

/** Map a WHOOP sport name to the nearest catalog kind (for "don't repeat yesterday's workout"). */
export function sportKind(sport: string): TakeKind | null {
  const s = sport.toLowerCase();
  if (/walk|run|jog|cycl|bik|swim|row|hik|elliptic|stair/.test(s)) return "endurance";
  if (/hiit|interval|crossfit|functional|box|aikido|martial|kick|circuit|sport|tennis|basket|soccer/.test(s)) return "conditioning";
  if (/strength|weight|lift|power/.test(s)) return "strength";
  if (/calisthen|gymnast|climb/.test(s)) return "calisthenics";
  if (/yoga|pilates|stretch|mobility|barre/.test(s)) return "flexibility";
  return null;
}

/** Days since the newest workout of a kind, capped at 99 when none is in the data. Undefined when workouts are unknown. */
export function daysSinceKind(workouts: readonly Workout[] | null | undefined, kind: string, day: string): number | undefined {
  if (!workouts) return undefined;
  const hits = workouts.filter((w) => w.day <= day && (sportKind(w.sport) === kind || w.sport.toLowerCase() === kind)).map((w) => daysBetween(w.day, day));
  return hits.length ? Math.min(...hits) : 99;
}

/** Readiness 1 (low) / 2 (moderate) / 3 (high), from today's facts only. Recovery decides when present; HRV and sleep can pull it down. Undefined when nothing is readable. */
export function readinessLevel(set: FactSet): number | undefined {
  const by = (k: string): Fact | undefined => set.facts.find((f) => f.key === k);
  const rel = (f: Fact | undefined) => (f?.baseline && f.baseline.n >= 7 && f.baseline.mean !== 0 ? (f.value - f.baseline.mean) / Math.abs(f.baseline.mean) : null);
  const rec = by("recovery"), hrv = rel(by("hrv")), slp = rel(by("sleep"));
  let level: number | undefined;
  if (rec) level = rec.value >= 67 ? 3 : rec.value >= 34 ? 2 : 1;
  else if (hrv !== null) level = hrv < -0.15 ? 1 : hrv < -0.05 ? 2 : 3;
  else if (slp !== null) level = slp < -0.25 ? 1 : slp < -0.1 ? 2 : 3;
  if (level === undefined) return undefined;
  if (level === 3 && ((hrv !== null && hrv < -0.15) || (slp !== null && slp < -0.2))) level = 2;
  return level;
}

/** Stable 0..1 value from day and id: the same day always gives the same number, days differ. */
export function seeded(day: string, id: string): number {
  let h = 2166136261;
  for (const ch of `${day}|${id}`) { h ^= ch.charCodeAt(0); h = Math.imul(h, 16777619); }
  return ((h >>> 0) % 10_000) / 10_000;
}

/** Progressive challenge level due now: one above the highest level answered "did it", capped at the series top. */
export function seriesLevel(series: string, entries: readonly CatalogEntry[], offers: readonly OfferRecord[]): number {
  const byId = new Map(entries.map((e) => [e.id, e]));
  const top = Math.max(1, ...entries.filter((e) => e.progression?.series === series).map((e) => e.progression!.level));
  const done = offers.filter((o) => o.answer === "done").map((o) => byId.get(o.recId)?.progression).filter((p) => p?.series === series).map((p) => p!.level);
  return Math.min(top, (done.length ? Math.max(...done) : 0) + 1);
}
