import { CATALOG, entryById, type CatalogEntry } from "../amaraTake/catalog";
import type { TakePage } from "../amaraTake/types";
import type { NoopDayRow } from "../types";
import type { AnswerState, GoalsFromTakes, Lately, Period, Point, ReadyToKeep, TypeOutcome, Verdict, WeekRow, WeekSuggestions } from "./adapters";
import { answerOf, sidOf, todayRecords, typeActions, type TakeEvent } from "./events";
import { computeOutcome, MIN_EACH, type OutcomeResult } from "./outcome";

/** Outcomes by suggestion type over time, plus the Weekly review and Goals views built from the same rows. */
export type Sources = { rows: readonly NoopDayRow[]; museRelax: ReadonlyMap<string, number> };

const DAY = 86_400_000;
const addDays = (day: string, n: number) => new Date(Date.parse(`${day}T00:00:00Z`) + n * DAY).toISOString().slice(0, 10);
const label = (day: string) => new Date(`${day}T00:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const isWeekend = (day: string) => [0, 6].includes(new Date(`${day}T00:00:00Z`).getUTCDay());
const num = (v: number, d = 1) => Number(v.toFixed(d)).toLocaleString("en-US", { maximumFractionDigits: d });
const signed = (v: number, d = 1) => `${v < 0 ? "−" : "+"}${num(Math.abs(v), d)}`;

/** The measured outcome for suggestion day `d`, or null (never a guess). Lagged metrics read the next day's NOOP row. */
export function measure(entry: CatalogEntry, d: string, s: Sources): number | null {
  const row = (day: string) => s.rows.find((r) => r.day === day);
  switch (entry.outcome.source) {
    case "muse_relax": return s.museRelax.get(d) ?? null;
    case "noop_hrv": return row(addDays(d, 1))?.avgHrv ?? null;
    case "noop_efficiency": { const e = row(addDays(d, 1))?.efficiency; return e == null ? null : e <= 1 ? e * 100 : e; }
    case "none": return null;
  }
}
const nn = (xs: Array<number | null | undefined>) => xs.filter((x): x is number => typeof x === "number" && Number.isFinite(x));

type Day = { day: string; state: AnswerState; detected: boolean };

function evaluate(entry: CatalogEntry, days: Day[], first: string, s: Sources): { res: OutcomeResult; withDays: string[]; withoutDays: string[]; before: Array<{ day: string; v: number }> } {
  const done = days.filter((d) => d.state === "done"), skipped = days.filter((d) => d.state === "skipped");
  const withM = done.map((d) => ({ d: d.day, v: measure(entry, d.day, s) })), withoutM = skipped.map((d) => ({ d: d.day, v: measure(entry, d.day, s) }));
  const w = withM.filter((x) => x.v !== null), wo = withoutM.filter((x) => x.v !== null);
  const range = (n: number) => Array.from({ length: n }, (_, i) => addDays(first, -n + i));
  const before = range(14).map((d) => ({ day: d, v: measure(entry, d, s) })).filter((x): x is { day: string; v: number } => x.v !== null);
  const base28 = nn(range(28).map((d) => measure(entry, d, s)));
  const rowOf = (d: string) => s.rows.find((r) => r.day === d);
  const conf = (metric: string, pick: (r: NoopDayRow) => number | null) => ({ metric, withVals: nn(w.map((x) => { const r = rowOf(x.d); return r ? pick(r) : null; })), withoutVals: nn(wo.map((x) => { const r = rowOf(x.d); return r ? pick(r) : null; })) });
  const share = (xs: Array<{ d: string }>) => (xs.length ? xs.filter((x) => isWeekend(x.d)).length / xs.length : 0);
  const res = computeOutcome({
    seed: entry.id, better: entry.outcome.better, wobbleFloor: entry.outcome.wobbleFloor,
    withVals: w.map((x) => x.v as number), withoutVals: wo.map((x) => x.v as number), beforeVals: before.map((x) => x.v), baseline28: base28,
    confounders: [conf("sleep duration", (r) => r.totalSleepMin), conf("recovery", (r) => r.recovery)],
    weekend: w.length && wo.length ? { with: share(w), without: share(wo) } : null,
    excluded: { unknown: days.filter((d) => d.state === "unknown").length, notToday: days.filter((d) => d.state === "not_today").length, noMeasurement: withM.length - w.length + (withoutM.length - wo.length) },
  });
  return { res, withDays: w.map((x) => x.d), withoutDays: wo.map((x) => x.d), before };
}

const fmtStat = (v: number | null, unit: string) => (v === null ? "–" : `${num(v)}${unit}`);

function sentences(entry: CatalogEntry, r: OutcomeResult, doneN: number, otherN: number): { headline: string; sentence: string } {
  const u = entry.outcome.unit, m = entry.outcome.metric;
  if (entry.outcome.source === "none") return { headline: `**${doneN}** done, **${otherN}** days without.`, sentence: "Amara can't measure this one yet: the ledger holds no decision dates, so there is nothing to compare. It stays a suggestion you answer by hand." };
  if (r.verdict === "too_early") return { headline: `**${doneN}** done, **${otherN}** days without.`, sentence: `Amara needs at least **${MIN_EACH} + ${MIN_EACH}** days with a measurement before she says anything about ${m}.` };
  const d = r.diff as number, [lo, hi] = r.ci90 as [number, number];
  const rng = `**${signed(lo)} to ${signed(hi)}**${u}`;
  if (r.verdict === "unclear") return { headline: `${m[0].toUpperCase()}${m.slice(1)} differed by **${signed(d)}**${u}.`, sentence: "But something else differed between those days too (shown below), so the comparison isn't fair. Amara won't credit the suggestion." };
  const head = `${m[0].toUpperCase()}${m.slice(1)} was **${signed(d)}**${u} on days you did it.`;
  if (r.verdict === "none") return { headline: head, sentence: `Likely range ${rng}, which includes zero or sits inside your normal wobble (±${num(r.wobble)}${u}). No clear change. Amara stops suggesting it on this page for ${entry.cooldownDays} days.` };
  if (r.verdict === "worse") return { headline: head, sentence: `Likely range ${rng}, past your normal wobble (±${num(r.wobble)}${u}). Amara has stopped suggesting it for ${entry.cooldownDays} days. It doesn't show that the suggestion did it.` };
  return { headline: head, sentence: `Likely range ${rng}. That's bigger than your normal day-to-day wobble (±${num(r.wobble)}${u}), so it's probably not noise. It doesn't show that the suggestion did it.` };
}

/** One TypeOutcome per suggestion type that has been suggested at least once. */
export function buildTypeOutcomes(events: readonly TakeEvent[], todayDay: string, s: Sources, entries: CatalogEntry[] = CATALOG.entries): TypeOutcome[] {
  const recs = todayRecords(events).filter((r) => r.kind === "suggestion");
  const out: TypeOutcome[] = [];
  for (const entry of entries) {
    const mine = recs.filter((r) => r.recId === entry.id).sort((a, b) => a.day.localeCompare(b.day));
    if (!mine.length) continue;
    const days: Day[] = mine.map((r) => { const v = answerOf(events, sidOf(entry.page, r.day, entry.id), r.day, todayDay); return { day: r.day, state: v.answer.state, detected: v.answer.source === "detected" && v.answer.state === "done" }; });
    const first = mine[0].day, last = mine[mine.length - 1].day;
    // Today's pending suggestion is not yet a "day without": only answered or past days count.
    const counted = days.filter((d) => d.state !== "waiting");
    const { res, withDays, withoutDays, before } = evaluate(entry, counted, first, s);
    const doneN = counted.filter((d) => d.state === "done").length, otherN = counted.filter((d) => d.state === "skipped").length;
    const { headline, sentence } = sentences(entry, res, doneN, otherN);
    const u = entry.outcome.unit;
    const checked = res.confounders.length
      ? `${res.confounders.map((c) => `${c.metric} ${num(c.with)} vs ${num(c.without)}${c.flagged ? " (differs)" : ""}`).join("; ")}.${res.weekendFlagged ? " Weekend share differed a lot." : ""}`
      : "Not compared yet.";
    const ex = res.excluded;
    const excluded = `Unknown days (${ex.unknown}), "not today" days (${ex.notToday}) and days with no measurement (${ex.noMeasurement}) are left out.`;
    const pts: Point[] = [
      ...before.map((b, i) => ({ x: i, y: b.v, g: "before" as const })),
      ...withDays.map((d) => ({ x: 15 + Math.max(0, (Date.parse(d) - Date.parse(first)) / DAY), y: measure(entry, d, s) as number, g: "done" as const })),
      ...withoutDays.map((d) => ({ x: 15 + Math.max(0, (Date.parse(d) - Date.parse(first)) / DAY), y: measure(entry, d, s) as number, g: "other" as const })),
    ];
    // Weekly periods: each shows the result so far (cumulative to the end of that week).
    const periods: Period[] = [];
    for (let start = first; start <= last; start = addDays(start, 7)) {
      const end = addDays(start, 6);
      const upto = counted.filter((d) => d.day <= end), inWeek = days.filter((d) => d.day >= start && d.day <= end);
      const cum = evaluate(entry, upto, first, s).res;
      periods.push({ label: `${label(start)} – ${label(end < last ? end : last)}`, suggested: inWeek.length, done: inWeek.filter((d) => d.state === "done").length, diff: cum.diff === null ? null : `${signed(cum.diff)}${u}`, verdict: cum.verdict });
    }
    out.push({
      type: entry.id, page: entry.page, title: entry.typeLabel, metric: entry.outcome.metric, verdict: res.verdict as Verdict, window: `${label(first)} – ${last === todayDay ? "today" : label(last)}`,
      suggested: days.length, done: doneN, detected: days.filter((d) => d.detected).length, headline, sentence,
      stats: [
        { label: "Did it", n: res.groups.with.n, mean: fmtStat(res.groups.with.mean, u), unit: "days" },
        { label: "Suggested, not done", n: res.groups.without.n, mean: fmtStat(res.groups.without.mean, u), unit: "days" },
        { label: "Before it was suggested", n: res.groups.before.n, mean: fmtStat(res.groups.before.mean, u), unit: "days" },
      ].filter((x) => x.n > 0),
      diff: res.diff === null ? null : `${signed(res.diff)}${u}`, range: res.ci90 ? `${signed(res.ci90[0])} to ${signed(res.ci90[1])}` : null, wobble: res.diff === null ? null : `±${num(res.wobble)}${u}`,
      checked, excluded, periods, points: pts,
    });
  }
  return out;
}

const dotOf = (state: AnswerState, detected: boolean): AnswerState | "detected" => (detected ? "detected" : state);

/** Monday of the week containing `day`. */
const monday = (day: string) => addDays(day, -((new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7));

export function buildWeek(events: readonly TakeEvent[], todayDay: string, outcomes: readonly TypeOutcome[], takes: Array<{ text: string; from: string }>): WeekSuggestions | null {
  const start = monday(todayDay), week = Array.from({ length: 7 }, (_, i) => addDays(start, i));
  const recs = todayRecords(events).filter((r) => r.kind === "suggestion" && r.day >= start && r.day <= week[6]);
  if (!recs.length) return null;
  const tally = { done: 0, of: 0, detected: 0, skipped: 0, notToday: 0, unknown: 0 };
  const rows: WeekRow[] = [];
  for (const page of ["neuro", "fitness", "mindBody", "business", "home"] as TakePage[]) {
    const mine = recs.filter((r) => r.page === page); if (!mine.length) continue;
    const days = week.map((d) => { const r = mine.find((x) => x.day === d); if (!r || r.kind !== "suggestion") return null; const v = answerOf(events, sidOf(page, d, r.recId), d, todayDay).answer; return { state: v.state, detected: v.source === "detected" && v.state === "done" }; });
    const latest = mine[mine.length - 1]; const entry = latest.kind === "suggestion" ? entryById(latest.recId) : null;
    for (const d of days) { if (!d) continue; if (d.state !== "waiting") tally.of += 1; if (d.state === "done") tally.done += 1; if (d.detected) tally.detected += 1; if (d.state === "skipped") tally.skipped += 1; if (d.state === "not_today") tally.notToday += 1; if (d.state === "unknown") tally.unknown += 1; }
    const o = outcomes.find((x) => x.type === entry?.id);
    rows.push({ page, title: entry?.shortTitle ?? page, sub: `Suggested on ${days.filter(Boolean).length} of 7 days`, days: days.map((d) => (d ? dotOf(d.state, d.detected) : null)), done: days.filter((d) => d?.state === "done").length, of: days.filter((d) => d && d.state !== "waiting").length, verdict: o?.verdict ?? "too_early", note: o?.diff ? `${o.metric} ${o.diff}${o.range ? ` (${o.range})` : ""}` : o?.verdict === "too_early" ? "too early to tell" : "" });
  }
  const acted = typeActions(events);
  const best = outcomes.filter((o) => o.verdict === "better" && !acted.has(o.type)).sort((a, b) => b.done - a.done)[0];
  return { ...tally, rows, change: best ? { type: best.type, title: `Make “${entryById(best.type)?.shortTitle ?? best.title}” a habit.`, body: `${best.headline} ${best.sentence}` } : null, takes };
}

/** Ready to keep = better and not yet answered; kept habits stay listed with before/after numbers and a 28-day re-check. */
export function buildGoals(events: readonly TakeEvent[], todayDay: string, outcomes: readonly TypeOutcome[], s: Sources): GoalsFromTakes | null {
  const acted = typeActions(events);
  const ready: ReadyToKeep[] = [];
  for (const o of outcomes) {
    const a = acted.get(o.type);
    if (!a && o.verdict === "better") ready.push({ outcome: o, note: `You did it on ${o.done} of ${o.suggested} suggested days.` });
    if (a?.action === "keep") {
      const entry = entryById(o.type); if (!entry) continue;
      const recheck = recheck28(events, entry, todayDay, s, a.day);
      ready.push({ outcome: o, note: `Habit since ${label(a.day)}. ${a.note ?? ""} ${recheck}`.replace(/\s+/g, " ").trim() });
    }
  }
  const start = addDays(todayDay, -6);
  const lately: Lately[] = [];
  for (const page of ["neuro", "fitness", "mindBody", "business", "home"] as TakePage[]) {
    const mine = todayRecords(events).filter((r) => r.page === page && r.kind === "suggestion" && r.day >= start); if (!mine.length) continue;
    const latest = mine[mine.length - 1]; if (latest.kind !== "suggestion") continue;
    const entry = entryById(latest.recId);
    const days = Array.from({ length: 7 }, (_, i) => addDays(start, i)).map((d) => { const r = mine.find((x) => x.day === d); if (!r || r.kind !== "suggestion") return null; const v = answerOf(events, sidOf(page, d, r.recId), d, todayDay).answer; return dotOf(v.state, v.source === "detected" && v.state === "done"); });
    lately.push({ page, title: entry?.shortTitle ?? page, sub: `${mine.length} day${mine.length === 1 ? "" : "s"} suggested this week`, days });
  }
  void s;
  return ready.length || lately.length ? { ready, lately } : null;
}

/** The monthly re-check of a kept habit: the same method over the last 28 days. Says "drifting" when the clear result is gone. */
export function recheck28(events: readonly TakeEvent[], entry: CatalogEntry, todayDay: string, s: Sources, since: string): string {
  const from = addDays(todayDay, -28);
  if (todayDay < addDays(since, 28)) return "First re-check 28 days after you made it a habit.";
  const recs = todayRecords(events).filter((r) => r.kind === "suggestion" && r.recId === entry.id && r.day >= from);
  const days: Day[] = recs.map((r) => { const v = answerOf(events, sidOf(entry.page, r.day, entry.id), r.day, todayDay).answer; return { day: r.day, state: v.state, detected: false }; }).filter((d) => d.state !== "waiting");
  if (!days.length) return "Re-check: no days in the last 28.";
  const { res } = evaluate(entry, days, from, s);
  return res.verdict === "better" ? "Re-check: still looks better over the last 28 days." : res.verdict === "too_early" ? "Re-check: too few measured days in the last 28 to say." : res.verdict === "none" ? "Re-check: no clear change over the last 28 days, so it may be drifting." : res.verdict === "worse" ? "Re-check: looks worse over the last 28 days." : "Re-check: can't tell over the last 28 days.";
}

