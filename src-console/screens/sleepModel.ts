import type { NoopDailyMetric } from "../types";
import { calendarDays, quantile } from "../ui/charts/chartMath";

/** design-1110h option B ("Dawn"): everything the Sleep card shows, from data the app already has. */
export interface SleepNight {
  day: string;
  durationMin: number | null;
  /** 0–1, as NOOP sends it. */
  efficiency: number | null;
  startedAt?: string | null;
  endedAt?: string | null;
  deepMin: number | null;
  remMin: number | null;
  lightMin: number | null;
  awakeMin: number | null;
}
export type StageKey = "deep" | "rem" | "light" | "awake";
export interface SleepCardModel {
  durationMin: number | null;
  efficiencyPct: number | null;
  bed: string | null;
  wake: string | null;
  stages: Array<{ key: StageKey; label: string; minutes: number }>;
  usualMin: number | null;
  usualEffPct: number | null;
  /** Last 7 calendar nights ending on this night, oldest first; null = no record. */
  week: Array<{ day: string; minutes: number | null }>;
  takeaway: string | null;
}

const BASE_NIGHTS = 28, MIN_NIGHTS = 5, LONG = 60;
export const hoursMinutes = (m: number) => { const r = Math.round(m); return `${Math.floor(r / 60)}h ${String(r % 60).padStart(2, "0")}m`; };
const clock = (iso: string | null | undefined) => {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isFinite(t) ? new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }).format(new Date(t)) : null;
};
const num = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v) && v > 0;

export function sleepCardModel(night: SleepNight, days: readonly NoopDailyMetric[]): SleepCardModel {
  const prior = days.filter((d) => d.day < night.day);
  const lens = prior.slice(-BASE_NIGHTS).map((d) => d.totalSleepMin).filter(num);
  const effs = prior.slice(-BASE_NIGHTS).map((d) => d.efficiency).filter(num).map((e) => e * 100);
  const usualMin = lens.length >= MIN_NIGHTS ? quantile(lens, .5) : null;
  const usualEffPct = effs.length >= MIN_NIGHTS ? quantile(effs, .5) : null;
  const durationMin = num(night.durationMin) ? night.durationMin : null;
  const byDay = days.filter((d) => d.day <= night.day);
  const week = calendarDays(byDay, (d) => d.day, 7, night.day).map(({ day, item }) => ({ day, minutes: day === night.day && durationMin != null ? durationMin : item && num(item.totalSleepMin) ? item.totalSleepMin : null }));
  const stages = ([["deep", "Deep", night.deepMin], ["rem", "REM", night.remMin], ["light", "Light", night.lightMin], ["awake", "Awake", night.awakeMin]] as const)
    .map(([key, label, m]) => ({ key, label, minutes: num(m) ? m : 0 }));
  return { durationMin, efficiencyPct: num(night.efficiency) ? night.efficiency * 100 : null, bed: clock(night.startedAt), wake: clock(night.endedAt), stages, usualMin, usualEffPct, week, takeaway: takeaway(durationMin, usualMin, week, night.awakeMin) };
}

/** One factual line from the numbers on the card; nothing it says isn't shown above it. */
export function takeaway(dur: number | null, usual: number | null, week: SleepCardModel["week"], awake: number | null): string | null {
  if (dur == null || usual == null) return null;
  const before = week.slice(0, -1).map((w) => w.minutes);
  let run = 0, side = 0;
  for (let i = before.length - 1; i >= 0; i--) {
    const m = before[i]; if (m == null) break;
    const s = m > usual + LONG ? 1 : m < usual - LONG ? -1 : 0;
    if (s === 0 || (side && s !== side)) break;
    side = s; run++;
  }
  const near = Math.abs(dur - usual) <= 30;
  let line: string;
  if (near && run >= 2) line = `Normal length after ${run} ${side > 0 ? "long" : "short"} nights (${side > 0 ? "to" : "down to"} ${hoursMinutes((side > 0 ? Math.max : Math.min)(...(before.slice(-run) as number[])))})`;
  else if (near) line = "Close to your usual length";
  else line = `${hoursMinutes(Math.abs(dur - usual))} ${dur > usual ? "longer" : "shorter"} than your usual`;
  return num(awake) ? `${line} · ${Math.round(awake)} min awake.` : `${line}.`;
}
