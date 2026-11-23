import type { Hypnogram, NoopWorkout } from "../bridge";
import type { NoopMetricKey } from "../types";
import { addDays, todayJst } from "../ui/charts/chartMath";
import type { TrendsData } from "./noopData";

/** Preview-only values (nexus.sampleAdapters=1). Every panel that shows them carries a "Sample data" tag. */
const SPORTS = ["running", "strength", "cycling", "aikido", "walking"];

export function sampleWorkouts(days: number, today = todayJst()): NoopWorkout[] {
  const out: NoopWorkout[] = [];
  for (let i = 0; i < days; i += 2) {
    const day = addDays(today, -i);
    const sport = SPORTS[(i / 2) % SPORTS.length];
    const min = 30 + ((i * 7) % 40);
    out.push({
      day, startedAt: `${day}T07:10:00+09:00`, endedAt: `${day}T0${7 + Math.floor(min / 60)}:${String((10 + min) % 60).padStart(2, "0")}:00+09:00`, sport,
      durationMin: min, energyKcal: min * 9, avgHr: 128 + (i % 20), maxHr: 160 + (i % 15), strain: 8 + (i % 7),
      distanceM: sport === "running" || sport === "cycling" ? min * 160 : null, steps: sport === "running" || sport === "walking" ? min * 150 : null,
      zoneMinutes: i % 6 === 4 ? null : [Math.round(min * .1), Math.round(min * .3), Math.round(min * .3), Math.round(min * .2), Math.round(min * .1)],
    });
  }
  return out;
}

export function sampleHypnogram(day?: string, today = todayJst()): Hypnogram {
  const d = day ?? today;
  const start = Date.parse(`${d}T00:00:00+09:00`) - 3600; // 23:00 the night before, JST
  const plan: Array<[Hypnogram["segments"][number]["stage"], number]> = [["light", 25], ["deep", 55], ["light", 30], ["rem", 20], ["awake", 5], ["light", 60], ["deep", 35], ["rem", 40], ["light", 70], ["rem", 35], ["awake", 10], ["light", 45]];
  let t = start;
  const segments = plan.map(([stage, min]) => { const s = { start: t, end: t + min * 60, stage }; t += min * 60; return s; });
  return { day: d, startedAt: new Date(start * 1000).toISOString(), endedAt: new Date(t * 1000).toISOString(), segments };
}

const BASE: Record<NoopMetricKey, [number, number]> = { recovery: [62, 18], hrv: [58, 10], rhr: [52, 3], resp_rate: [14.6, .8], spo2: [97, 1], strain: [42, 14], sleep_total_min: [440, 45], steps: [8200, 2600], active_kcal: [520, 140] };

export function sampleSeries(days: number, today = todayJst()): TrendsData {
  const out = {} as TrendsData;
  (Object.keys(BASE) as NoopMetricKey[]).forEach((key, k) => {
    const [mid, amp] = BASE[key];
    out[key] = { status: "ready", points: Array.from({ length: days }, (_, i) => ({ day: addDays(today, i - days + 1), value: Math.max(0, +(mid + Math.sin((i + k * 3) / 3.1) * amp * .7 + (((i * 37 + k * 11) % 17) - 8) / 8 * amp * .3).toFixed(1)) })) };
  });
  return out;
}
