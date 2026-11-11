import type { NoopDayRow, NoopSnapshotPayload, RecordedSession, RevenueSummary } from "../types";
import type { HomeBrief } from "../bridge";
import { baseline, BASELINE_DAYS, readingDay, tokyoDay } from "../lib/mindBody";
import { snapshotDays } from "../lib/noop";
import { freshBrief, usableWeather } from "../hooks/useHomeBrief";
import type { Fact, FactSet, TakePage } from "./types";

const finite = (v: number | null | undefined): v is number => typeof v === "number" && Number.isFinite(v);
const round = (v: number, d = 1) => Number(v.toFixed(d));
const fmt = (v: number, d = 1) => round(v, d).toLocaleString("en-US", { maximumFractionDigits: d });

type Spec = { key: string; label: string; unit: string; decimals?: number; scale?: number; alt?: (v: number) => number[] };

/** One fact from a value and its prior-days values; the baseline needs 7+ values (mindBody.baseline). */
export function makeFact(spec: Spec, raw: number | null | undefined, prior: Array<number | null | undefined> = []): Fact | null {
  if (!finite(raw)) return null;
  const k = spec.scale ?? 1, d = spec.decimals ?? 0;
  const value = round(raw * k, d);
  const unit = spec.unit ? ` ${spec.unit}` : "";
  const base = baseline(prior.map((p) => (finite(p) ? p : null)));
  const mean = base ? round(base.mean * k, d) : null;
  return {
    key: spec.key, label: spec.label, value, unit: spec.unit, display: `${fmt(value, d)}${unit}`,
    baseline: base && mean !== null ? { mean, n: base.n, display: `${fmt(mean, d)}${unit}` } : null,
    accept: [value, ...(mean !== null ? [mean] : []), ...(spec.alt?.(raw) ?? [])],
  };
}

/** Same string for the same facts: a changed value (a new session or night) changes it. */
export function fingerprintOf(facts: Fact[]): string {
  return facts.map((f) => `${f.key}=${f.value}|${f.baseline?.mean ?? ""}`).join(";");
}

const build = (page: TakePage, day: string, facts: Array<Fact | null>, extra = ""): FactSet | null => {
  const real = facts.filter((f): f is Fact => f !== null);
  return real.length ? { page, day, facts: real, fingerprint: fingerprintOf(real) + (extra ? `;${extra}` : "") } : null;
};

const hm = (minutes: number) => [Math.floor(minutes / 60), Math.round(minutes % 60)];

const BODY: Record<string, Spec & { pick: (r: NoopDayRow) => number | null }> = {
  hrv: { key: "hrv", label: "HRV", unit: "ms", pick: (r) => r.avgHrv },
  recovery: { key: "recovery", label: "Recovery", unit: "%", pick: (r) => r.recovery },
  rhr: { key: "rhr", label: "Resting heart rate", unit: "bpm", pick: (r) => r.restingHr },
  sleep: { key: "sleep", label: "Sleep last night", unit: "h", decimals: 1, scale: 1 / 60, alt: (min) => hm(min), pick: (r) => r.totalSleepMin },
  steps: { key: "steps", label: "Steps today", unit: "", pick: (r) => r.steps },
  strain: { key: "strain", label: "Strain", unit: "", decimals: 1, pick: (r) => r.strain },
  kcal: { key: "kcal", label: "Active calories", unit: "kcal", pick: (r) => r.activeKcalEst },
  exercises: { key: "exercises", label: "Workouts logged today", unit: "", pick: (r) => r.exerciseCount },
};

const PAGE_METRICS: Record<"mindBody" | "fitness" | "home", string[]> = {
  mindBody: ["hrv", "recovery", "sleep", "rhr"],
  fitness: ["steps", "strain", "kcal", "exercises", "recovery"],
  home: ["recovery", "hrv", "sleep"],
};

/** Facts for a NOOP-backed day. Null unless the reading day is `today` (Asia/Tokyo): yesterday's numbers are not today's take. */
function noopFacts(page: "mindBody" | "fitness" | "home", snap: NoopSnapshotPayload | null, today: string): Fact[] {
  if (!snap) return [];
  const days = snapshotDays(snap);
  const series = (pick: (r: NoopDayRow) => number | null) => days.map((r) => ({ day: r.day, value: pick(r) }));
  const asOf = readingDay({ hrv: series((r) => r.avgHrv), recovery: series((r) => r.recovery), sleep: series((r) => r.totalSleepMin) });
  // Fitness counters (steps, strain) can exist before the night syncs, so it only needs today's row.
  const rowDay = page === "fitness" ? days[days.length - 1]?.day ?? null : asOf;
  if (rowDay !== today) return [];
  const row = days.find((r) => r.day === today);
  if (!row) return [];
  const prior = days.filter((r) => r.day < today).slice(-BASELINE_DAYS);
  return PAGE_METRICS[page].flatMap((m) => {
    const spec = BODY[m];
    const fact = makeFact(spec, spec.pick(row), prior.map(spec.pick));
    return fact ? [fact] : [];
  });
}

export const mindBodyFacts = (snap: NoopSnapshotPayload | null, today: string) => build("mindBody", today, noopFacts("mindBody", snap, today));
export const fitnessFacts = (snap: NoopSnapshotPayload | null, today: string) => build("fitness", today, noopFacts("fitness", snap, today));

/** Home: the morning brief's weather (fresh only) plus the same body readings. Nothing readable, nothing returned. */
export function homeFacts(snap: NoopSnapshotPayload | null, brief: HomeBrief | null, today: string, now: number): FactSet | null {
  const weather = usableWeather(freshBrief(brief, now)?.weather);
  const w: Spec[] = [
    { key: "tempC", label: `Temperature${weather?.place ? ` in ${weather.place}` : ""}`, unit: "°C" },
    { key: "highC", label: "Today's high", unit: "°C" },
    { key: "lowC", label: "Today's low", unit: "°C" },
  ];
  const values = [weather?.tempC, weather?.highC, weather?.lowC];
  const wf = w.map((s, i) => makeFact(s, values[i]));
  return build("home", today, [...noopFacts("home", snap, today), ...wf]);
}

/** Neuro: Muse sessions that started today (Asia/Tokyo) against the earlier 28 days' sessions. No session today, no facts. */
export function neuroFacts(sessions: RecordedSession[], today: string): FactSet | null {
  const dayOf = (s: RecordedSession) => tokyoDay(s.start);
  const todays = sessions.filter((s) => dayOf(s) === today);
  if (!todays.length) return null;
  const cutoff = Date.parse(`${today}T00:00:00+09:00`) - BASELINE_DAYS * 86_400_000;
  const earlier = sessions.filter((s) => {
    const d = dayOf(s);
    return d !== null && d < today && Date.parse(`${d}T00:00:00+09:00`) >= cutoff;
  });
  const mean = (xs: Array<number | null | undefined>) => {
    const v = xs.filter(finite);
    return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null;
  };
  const todayAvg = (pick: (s: RecordedSession) => number | null | undefined) => mean(todays.map(pick));
  const pr = (pick: (s: RecordedSession) => number | null | undefined) => earlier.map(pick);
  const focus = (s: RecordedSession) => s.summary?.avg_focus;
  const relax = (s: RecordedSession) => s.summary?.avg_relaxation;
  const hrv = (s: RecordedSession) => s.summary?.avg_hrv;
  const mins = (s: RecordedSession) => (finite(s.duration_secs) ? s.duration_secs / 60 : null);
  // Session ids join the fingerprint so a new session refreshes the take even if the averages match.
  return build("neuro", today, [
    makeFact({ key: "sessions", label: "Muse sessions today", unit: "" }, todays.length),
    makeFact({ key: "minutes", label: "Minutes recorded today", unit: "min" }, todays.reduce((a, s) => a + (mins(s) ?? 0), 0), []),
    makeFact({ key: "focus", label: "Average focus today", unit: "", decimals: 1 }, todayAvg(focus), pr(focus)),
    makeFact({ key: "relax", label: "Average relaxation today", unit: "", decimals: 1 }, todayAvg(relax), pr(relax)),
    makeFact({ key: "hrv", label: "Average HRV in sessions today", unit: "ms" }, todayAvg(hrv), pr(hrv)),
  ], todays.map((s) => s.id).join(","));
}

/** Business: ARM revenue summary. Needs some recorded revenue or expense; amounts carry no currency symbol. */
export function businessFacts(rev: RevenueSummary | null, today: string): FactSet | null {
  if (!rev || !(rev.total_revenue > 0 || rev.total_expenses > 0)) return null;
  const dailyAvg = rev.week_revenue / 7;
  return build("business", today, [
    makeFact({ key: "todayRevenue", label: "Revenue today", unit: "", decimals: 2 }, rev.today_revenue, []),
    makeFact({ key: "todayExpenses", label: "Expenses today", unit: "", decimals: 2 }, rev.today_expenses, []),
    makeFact({ key: "weekRevenue", label: "Revenue, last 7 days", unit: "", decimals: 2 }, rev.week_revenue, []),
    makeFact({ key: "monthRevenue", label: "Revenue this month", unit: "", decimals: 2 }, rev.month_revenue, []),
    makeFact({ key: "netIncome", label: "Net income, all time", unit: "", decimals: 2 }, rev.net_income, []),
  ].map((f) => (f && f.key === "todayRevenue" ? { ...f, baseline: { mean: round(dailyAvg, 2), n: 7, display: fmt(dailyAvg, 2) }, accept: [...f.accept, round(dailyAvg, 2)] } : f)));
}

/** The prompt body: only these lines carry data. */
export function factLines(set: FactSet): string {
  return set.facts.map((f) => `- ${f.label}: ${f.display}${f.baseline ? ` (usual over the last ${f.baseline.n} days: ${f.baseline.display})` : ""}`).join("\n");
}
