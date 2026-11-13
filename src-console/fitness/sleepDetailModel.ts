import type { Hypnogram, HypnogramStage } from "../bridge";
import { formatMinutes } from "../lib/format";
import type { NoopDailyMetric } from "../types";
import { calendarDays } from "../ui/charts/chartMath";

/** Top to bottom on the chart: awake sits on top, deep at the bottom (the usual hypnogram order). */
export const STAGE_ORDER: readonly HypnogramStage[] = ["awake", "rem", "light", "deep"];
export const STAGE_LABEL: Record<HypnogramStage, string> = { awake: "Awake", rem: "REM", light: "Light", deep: "Deep" };

const clock = (sec: number) => new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }).format(new Date(sec * 1000));

export interface HypnoBlock { stage: HypnogramStage; x: number; w: number; row: number; start: string; end: string; minutes: number }
export interface HypnoModel {
  startSec: number; endSec: number; spanMin: number;
  blocks: HypnoBlock[];
  minutes: Record<HypnogramStage, number>;
  /** Share of the night (0..100) per stage. */
  pct: Record<HypnogramStage, number>;
  /** Separate awake stretches inside the night. */
  awakenings: number;
  bed: string; wake: string;
  /** One sentence for the chart's accessible name. */
  summary: string;
}

export function hypnogramModel(h: Hypnogram): HypnoModel | null {
  const segs = h.segments.filter((s) => Number.isFinite(s.start) && Number.isFinite(s.end) && s.end > s.start && STAGE_ORDER.includes(s.stage)).sort((a, b) => a.start - b.start);
  if (segs.length === 0) return null;
  const startSec = segs[0].start, endSec = Math.max(...segs.map((s) => s.end));
  const span = endSec - startSec;
  const minutes = { awake: 0, rem: 0, light: 0, deep: 0 } as Record<HypnogramStage, number>;
  const blocks = segs.map((s) => {
    const min = (s.end - s.start) / 60;
    minutes[s.stage] += min;
    return { stage: s.stage, x: ((s.start - startSec) / span) * 100, w: ((s.end - s.start) / span) * 100, row: STAGE_ORDER.indexOf(s.stage), start: clock(s.start), end: clock(s.end), minutes: min };
  });
  const total = Object.values(minutes).reduce((a, b) => a + b, 0);
  const pct = Object.fromEntries(STAGE_ORDER.map((k) => [k, total ? (minutes[k] / total) * 100 : 0])) as Record<HypnogramStage, number>;
  let awakenings = 0;
  segs.forEach((s, i) => { if (s.stage === "awake" && i > 0 && i < segs.length - 1) awakenings++; });
  const bed = clock(startSec), wake = clock(endSec);
  const summary = `Sleep stages from ${bed} to ${wake}: ${STAGE_ORDER.slice().reverse().map((k) => `${STAGE_LABEL[k]} ${formatMinutes(minutes[k])}`).join(", ")}; ${awakenings} ${awakenings === 1 ? "awakening" : "awakenings"}.`;
  return { startSec, endSec, spanMin: span / 60, blocks, minutes, pct, awakenings, bed, wake, summary };
}

export interface NightOption { day: string; label: string; minutes: number }
const dayWords = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" });
const real = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v) && v > 0;

/** The last `n` nights that have a sleep record, newest first. A 0 is a missing reading, not a night. */
export function nightOptions(days: readonly NoopDailyMetric[], n = 14): NightOption[] {
  return days.filter((d) => real(d.totalSleepMin)).sort((a, b) => b.day.localeCompare(a.day)).slice(0, n)
    .map((d) => ({ day: d.day, label: `${dayWords(d.day)} · ${formatMinutes(d.totalSleepMin)}`, minutes: d.totalSleepMin as number }));
}

export interface NightSeries { days: string[]; labels: string[]; hours: (number | null)[]; efficiencyPct: (number | null)[]; nights: number }
/** `n` calendar nights ending on the latest night; a night with no record is a gap, never a zero. */
export function nightSeries(days: readonly NoopDailyMetric[], n = 14): NightSeries {
  const slots = calendarDays(days.filter((d) => real(d.totalSleepMin)), (d) => d.day, n);
  const short = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
  return {
    days: slots.map((s) => s.day), labels: slots.map((s) => short(s.day)),
    hours: slots.map((s) => (s.item && real(s.item.totalSleepMin) ? s.item.totalSleepMin / 60 : null)),
    efficiencyPct: slots.map((s) => (s.item && real(s.item.efficiency) ? Math.min(100, s.item.efficiency * 100) : null)),
    nights: slots.filter((s) => s.item).length,
  };
}
