import type { NoopWorkout } from "../bridge";
import { formatMinutes } from "../lib/format";

export const ZONE_COUNT = 5;
export const ZONE_NAMES = ["Zone 1", "Zone 2", "Zone 3", "Zone 4", "Zone 5"] as const;

const SPORT_WORDS: Record<string, string> = { hiit: "HIIT", aikido: "Aikido" };
/** "functional_strength" → "Functional strength". Sentence case, never the stored id. */
export function sportLabel(sport: string | null | undefined): string {
  const raw = (sport ?? "").trim();
  if (!raw) return "Workout";
  const key = raw.toLowerCase();
  if (SPORT_WORDS[key]) return SPORT_WORDS[key];
  const words = key.replace(/[_-]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

const finite = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);
const clock = (iso: string) => {
  const t = Date.parse(iso);
  return Number.isFinite(t) ? new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }).format(new Date(t)) : null;
};
const dayWords = (day: string) => {
  const t = Date.parse(`${day}T12:00:00Z`);
  return Number.isFinite(t) ? new Date(t).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" }) : day;
};

export interface WorkoutRow {
  key: string;
  sport: string;
  dayLabel: string;
  time: string | null;
  minutes: number | null;
  duration: string | null;
  kcal: number | null;
  avgHr: number | null;
  maxHr: number | null;
  strain: number | null;
  distanceKm: number | null;
  steps: number | null;
  /** Minutes in zones 1..5, or null when NOOP stored none. */
  zones: number[] | null;
  zoneTotal: number;
  /** Everything the row shows, as one sentence-case line (also the screen-reader text). */
  facts: string;
}

function zonesOf(raw: number[] | null | undefined): number[] | null {
  if (!Array.isArray(raw) || raw.length === 0) return null;
  const z = Array.from({ length: ZONE_COUNT }, (_, i) => (finite(raw[i]) && raw[i] > 0 ? raw[i] : 0));
  return z.some((m) => m > 0) ? z : null;
}

export function workoutRows(workouts: readonly NoopWorkout[]): WorkoutRow[] {
  return [...workouts]
    .sort((a, b) => b.startedAt.localeCompare(a.startedAt))
    .map((w, i) => {
      const zones = zonesOf(w.zoneMinutes);
      const minutes = finite(w.durationMin) && w.durationMin > 0 ? w.durationMin : null;
      const distanceKm = finite(w.distanceM) && w.distanceM > 0 ? w.distanceM / 1000 : null;
      const row: WorkoutRow = {
        key: `${w.startedAt}-${i}`, sport: sportLabel(w.sport), dayLabel: dayWords(w.day), time: clock(w.startedAt),
        minutes, duration: minutes == null ? null : formatMinutes(minutes),
        kcal: finite(w.energyKcal) && w.energyKcal > 0 ? Math.round(w.energyKcal) : null,
        avgHr: finite(w.avgHr) && w.avgHr > 0 ? Math.round(w.avgHr) : null, maxHr: finite(w.maxHr) && w.maxHr > 0 ? Math.round(w.maxHr) : null,
        strain: finite(w.strain) ? w.strain : null, distanceKm, steps: finite(w.steps) && w.steps > 0 ? Math.round(w.steps) : null,
        zones, zoneTotal: zones ? zones.reduce((a, b) => a + b, 0) : 0, facts: "",
      };
      row.facts = [
        row.duration, distanceKm != null ? `${distanceKm.toFixed(distanceKm < 10 ? 1 : 0)} km` : null, row.kcal != null ? `${row.kcal} kcal` : null,
        row.avgHr != null ? `avg ${row.avgHr} bpm` : null, row.maxHr != null ? `max ${row.maxHr} bpm` : null,
      ].filter(Boolean).join(" · ");
      return row;
    });
}

export interface WorkoutSummary { sessions: number; minutes: number; zoneTotals: number[]; zoneMinutes: number; withZones: number }
export function workoutSummary(rows: readonly WorkoutRow[]): WorkoutSummary {
  const zoneTotals = Array.from({ length: ZONE_COUNT }, () => 0);
  let minutes = 0, withZones = 0;
  for (const r of rows) {
    minutes += r.minutes ?? 0;
    if (r.zones) { withZones++; r.zones.forEach((m, i) => { zoneTotals[i] += m; }); }
  }
  return { sessions: rows.length, minutes, zoneTotals, zoneMinutes: zoneTotals.reduce((a, b) => a + b, 0), withZones };
}

/** "Zone 1 12m, Zone 2 30m": the text alternative for a zone bar. */
export const zoneText = (zones: readonly number[]) => zones.map((m, i) => `${ZONE_NAMES[i]} ${formatMinutes(m)}`).join(", ");
