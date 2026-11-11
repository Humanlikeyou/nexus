import type { NoopDayRow } from "../types";
import type { CatalogEntry, DetectorId } from "../amaraTake/catalog";
import { tokyoDay } from "../lib/mindBody";
import { timeLabel, type TakeEvent } from "./events";

/**
 * Adherence detectors (HANDOFF-1111t §4, §10). Each is pure: it gets data already read and returns a concrete
 * detection (an `evidenceRef` and a `detail` that says what Amara saw) or null. There is never a "probably".
 * Read-only: the Home Assistant detector is handed states that `ha_states` returned; nothing here calls an action.
 *
 * Not built, so the page stays tap-only: `checkin_saved` (no committed check-in store yet) and
 * `muse_breathing_before_session` (sessions carry no breathing/type field). Also not built: the WHOOP
 * "2,500 steps in 30 minutes" rule (NOOP exposes no step series).
 */
export type Detection = { evidenceRef: string; detail: string; at: number };
export const IMPLEMENTED: ReadonlySet<DetectorId> = new Set<DetectorId>(["whoop_walk", "whoop_easy_day", "ledger_decision_before_noon", "ha_evening_lights"]);
export const hasDetector = (entries: readonly CatalogEntry[], page: string) => entries.some((e) => e.page === page && e.detector !== null && IMPLEMENTED.has(e.detector));

export type Workout = { day: string; startedAt: string; endedAt: string; sport: string; durationMin: number | null; steps: number | null };
const tokyoMinutes = (iso: string): number | null => {
  const t = Date.parse(iso); if (!Number.isFinite(t)) return null;
  const p = new Date(t + 9 * 3_600_000); return p.getUTCHours() * 60 + p.getUTCMinutes();
};
const tokyoDayOf = (iso: string) => tokyoDay(iso);

/** A walk of 20+ minutes that started before 17:00 Tokyo on `day`. */
export function detectWalk(workouts: readonly Workout[], day: string): Detection | null {
  const hit = workouts.filter((w) => /walk/i.test(w.sport) && (w.durationMin ?? 0) >= 20 && tokyoDayOf(w.startedAt) === day && (tokyoMinutes(w.startedAt) ?? 1e9) < 17 * 60)
    .sort((a, b) => a.startedAt.localeCompare(b.startedAt))[0];
  if (!hit) return null;
  const t0 = Date.parse(hit.startedAt), t1 = Date.parse(hit.endedAt);
  const steps = hit.steps != null && hit.steps > 0 ? `, ${hit.steps.toLocaleString("en-US")} steps` : "";
  return { evidenceRef: `whoop:${hit.startedAt}`, at: Number.isFinite(t1) ? t1 : t0, detail: `WHOOP: ${Math.round(hit.durationMin as number)}-min walk${steps}, ${timeLabel(t0)}–${Number.isFinite(t1) ? timeLabel(t1) : "?"}` };
}

/** Day strain under 10, judged only once the day is over (by 23:59), from the NOOP day row. */
export function detectEasyDay(row: NoopDayRow | undefined, day: string, todayDay: string): Detection | null {
  if (!row || row.day !== day || day >= todayDay || row.strain == null || !Number.isFinite(row.strain) || row.strain >= 10) return null;
  return { evidenceRef: `noop:${day}:strain`, at: Date.parse(`${day}T23:59:00+09:00`), detail: `WHOOP: day strain ${row.strain.toFixed(1)}, under 10 by midnight` };
}

export type LedgerRequest = { id: string; purpose?: string; status: string };
/** `ledger_seen` events record which requests were open each time the ledger was read. */
export const ledgerSeenData = (reqs: readonly LedgerRequest[]) => ({ open: reqs.filter((r) => r.status === "open").map((r) => r.id) });

/**
 * A request seen open earlier the same Tokyo day and decided now, with the read before 12:00 Tokyo. The ledger
 * holds no decision timestamps, so Amara only claims what she watched: "open at 9:10, decided by 10:42".
 */
export function detectLedgerDecision(seen: readonly TakeEvent[], reqs: readonly LedgerRequest[], now: number, day: string): Detection | null {
  if (tokyoDay(new Date(now).toISOString()) !== day || (tokyoMinutes(new Date(now).toISOString()) ?? 1e9) >= 12 * 60) return null;
  const earlier = seen.filter((e) => e.kind === "ledger_seen" && e.day === day && e.at < now).sort((a, b) => a.at - b.at);
  for (const r of reqs) {
    if (r.status !== "approved" && r.status !== "denied") continue;
    const first = earlier.find((e) => Array.isArray(e.data.open) && (e.data.open as unknown[]).includes(r.id));
    if (!first) continue;
    const name = (r.purpose ?? "a funding request").slice(0, 40);
    return { evidenceRef: `ledger:${r.id}`, at: now, detail: `Ledger: "${name}" was open at ${timeLabel(first.at)} and ${r.status} by ${timeLabel(now)}, before 12:00` };
  }
  return null;
}

export type HaState = { entityId: string; state: string; friendlyName?: string | null; domain: string };
/** The evening warm scene applied at 21:00 ± 15 min Tokyo on `day` (a scene's HA state is its last activation time). */
export function detectEveningLights(states: readonly HaState[], day: string): Detection | null {
  for (const s of states) {
    if (s.domain !== "scene" || !/evening|warm/i.test(`${s.entityId} ${s.friendlyName ?? ""}`)) continue;
    const t = Date.parse(s.state); if (!Number.isFinite(t)) continue;
    const m = tokyoMinutes(new Date(t).toISOString());
    if (tokyoDay(new Date(t).toISOString()) === day && m !== null && m >= 21 * 60 - 15 && m <= 21 * 60 + 15)
      return { evidenceRef: `ha:${s.entityId}@${s.state}`, at: t, detail: `Home Assistant: the "${s.friendlyName ?? s.entityId}" scene was applied at ${timeLabel(t)}` };
  }
  return null;
}
