import { useEffect, useState } from "react";
import type { DailyRange_Serialize, NoopHealthSnapshot_Serialize } from "../bindings.gen";
import { invokeBound } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { inTauri, live, needsYouList, type NeedsYouReport } from "../lib/localStores";
import { useLiveHeartRate, type HeartTraceView } from "../hooks/useLiveHeartRate";

/**
 * design-1118b backend: WHOOP recovery and the day rings for Home. Logic only, no markup, class or token.
 * Values go to the UI only: nothing here logs, stores or sends a number.
 *
 * - Heart rate is the existing `live_heart_rate` feed (`useLiveHeartRate`); `useHomeVitals().heart` passes it through.
 * - Recovery is TODAY's score from the NOOP health snapshot, or an empty reason (decision 0035). It never falls
 *   back to an older day. `today` is a Tokyo day, same as the daily table.
 * - Day rings come from `daily_range`. A missing row or a null cell stays null (never 0, never filled).
 */
export type RecoveryEmptyReason = "hr_only" | "pending" | "unavailable";
export type Recovery = { kind: "score"; day: string; percent: number } | { kind: "empty"; reason: RecoveryEmptyReason };
export interface RingDay { day: string; recovery: number | null; sleepMin: number | null; strain: number | null; museMin: number | null }

export const RING_DAYS = 7;
const MAX_RING_DAYS = 31;
export type VitalsDeps = { call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>; now: () => Date };
const defaultDeps: VitalsDeps = { call: (c, a) => invokeBound(c, a), now: () => new Date() };

const tokyoDay = (d: Date) => d.toLocaleDateString("en-CA", { timeZone: "Asia/Tokyo" });
/** `n` consecutive Tokyo days ending at `today`, oldest first. */
export function ringDayKeys(today: string, n: number): string[] {
  const end = Date.parse(`${today}T12:00:00Z`);
  return Array.from({ length: n }, (_, i) => new Date(end - (n - 1 - i) * 86_400_000).toISOString().slice(0, 10));
}
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** Today's recovery only. A row for another day, or no score on today's row, is an empty reason. */
export function recoveryFrom(snap: Pick<NoopHealthSnapshot_Serialize, "today"> | null | undefined, today: string, hrOnly: boolean): Recovery {
  if (!snap) return { kind: "empty", reason: "unavailable" };
  const row = snap.today;
  const percent = row && row.day === today ? num(row.recovery) : null;
  if (percent != null) return { kind: "score", day: today, percent };
  return { kind: "empty", reason: hrOnly ? "hr_only" : "pending" };
}

/** One ring row per requested day, oldest first; days with no row are all-null. */
export function ringsFrom(range: Pick<DailyRange_Serialize, "rows"> | null | undefined, days: string[]): RingDay[] {
  const by = new Map((range?.rows ?? []).map((r) => [r.day, r]));
  return days.map((day) => {
    const r = by.get(day);
    return { day, recovery: num(r?.recovery), sleepMin: num(r?.sleepTotalMin), strain: num(r?.strain), museMin: num(r?.museMin) };
  });
}

const strapHrOnly = (reports: NeedsYouReport[]) => reports.some((x) => x.source === "strap" && x.state === "ok" && x.rows.some((p) => p.id === "strap-hr-only"));

export interface HomeVitalsLive {
  /** Today's recovery or why there is none. `unwired` outside the app or when the snapshot can't be read at all. */
  recovery(): Promise<Wired<Recovery>>;
  /** `n` ring days (default 7, at most 31) ending today. `unwired` outside the app or when the range can't be read. */
  rings(n?: number): Promise<Wired<RingDay[]>>;
}

export function makeHomeVitals(deps: VitalsDeps = defaultDeps): HomeVitalsLive {
  return {
    async recovery() {
      if (!inTauri()) return UNWIRED;
      const today = tokyoDay(deps.now());
      try {
        const snap = await deps.call<NoopHealthSnapshot_Serialize>("get_noop_health_snapshot");
        const r = recoveryFrom(snap, today, false);
        if (r.kind === "score") return live(r);
        const hrOnly = await needsYouList((c, a) => deps.call(c, a) as never).then(strapHrOnly, () => false);
        return live(recoveryFrom(snap, today, hrOnly));
      } catch { return UNWIRED; }
    },
    async rings(n = RING_DAYS) {
      if (!inTauri()) return UNWIRED;
      const count = Math.min(MAX_RING_DAYS, Math.max(1, Math.floor(n)));
      const days = ringDayKeys(tokyoDay(deps.now()), count);
      try {
        const range = await deps.call<DailyRange_Serialize>("daily_range", { from: days[0], to: days[days.length - 1] });
        return live(ringsFrom(range, days));
      } catch { return UNWIRED; }
    },
  };
}

/** Everything Home's lens and rings need: live heart (null when there is no fresh reading), recovery and ring days. */
export function useHomeVitals(deps?: VitalsDeps): { heart: HeartTraceView | null; recovery: Wired<Recovery>; rings: Wired<RingDay[]> } {
  const heart = useLiveHeartRate();
  const [recovery, setRecovery] = useState<Wired<Recovery>>(UNWIRED);
  const [rings, setRings] = useState<Wired<RingDay[]>>(UNWIRED);
  useEffect(() => {
    let on = true;
    const v = makeHomeVitals(deps);
    v.recovery().then((r) => { if (on) setRecovery(r); });
    v.rings().then((r) => { if (on) setRings(r); });
    return () => { on = false; };
  }, [deps]);
  return { heart, recovery, rings };
}
