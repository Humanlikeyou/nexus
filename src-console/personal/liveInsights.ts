import type { Finding, Patterns, WeeklyReview } from "./adapters";

/** Rows as src-tauri patterns.rs (`patterns_read`) and weekly_review.rs (`weekly_review_read`) return them. Every number is the backend's; nothing is computed or invented here. */
export type FindingWire = {
  id: string; pair: string; title: string; lagDays: number; windowDays: number; nWith: number; nWithout: number;
  meanWith: number; meanWithout: number; delta: number; r: number; p: number; confidence: number; strength: string; n: string;
  sentence: string; detail: string; computedAt: string; sourceIds: string[]; tag?: string;
  chart: { kind: "scatter" | "bars"; points?: Array<[number, number]>; split?: number; x?: [number, number]; y?: [number, number]; xLabel?: string; yLabel?: string; rows?: Array<[string, number, number]>; legend?: [string, string] };
};
export type PatternsWire = {
  state: "ready" | "not_enough_data"; computedAt: string; windowDays: number; findings: FindingWire[]; withheld: number; reason?: string;
  counts: { sleepDays: number; hrvDays: number; strainDays: number; sessions: number; checkins: number };
};

const r1 = (v: number) => Math.round(v * 10) / 10;

function findingFrom(f: FindingWire): Finding {
  const c = f.chart;
  const chart: Finding["chart"] = c.kind === "scatter"
    ? { kind: "scatter", points: (c.points ?? []).map(([a, b]) => [r1(a), r1(b)] as [number, number]), split: c.split ?? 0, x: c.x ?? [0, 1], y: c.y ?? [0, 1], xLabel: c.xLabel ?? "", yLabel: c.yLabel ?? "" }
    : { kind: "bars", rows: (c.rows ?? []).map(([l, a, b]) => [l, r1(a), r1(b)] as [string, number, number]), legend: c.legend ?? ["", ""] };
  return { id: f.id, title: f.title, confidence: Math.min(5, Math.max(1, f.confidence)) as Finding["confidence"], strength: f.strength, n: f.n, sentence: f.sentence, detail: f.detail, chart };
}

/** `ready` -> findings; `not_enough_data` -> a live, honest empty state (no findings, the reason as the window line). */
export function patternsFrom(w: PatternsWire): Patterns {
  const day = w.computedAt.slice(0, 10);
  // Second gate (the backend already withholds these): nothing under confidence 2 is ever shown.
  const shown = w.findings.filter((f) => f.confidence >= 2);
  if (w.state !== "ready" || shown.length === 0) {
    return { headline: "Not enough data yet", window: w.reason ?? "Amara needs more days before it says anything.", findings: [] };
  }
  const k = w.counts;
  return {
    headline: `Your clearest pattern: ${shown[0].title}`,
    window: `Last ${w.windowDays} days · ${k.sessions} Neuro sessions · ${k.sleepDays} nights · ${k.checkins} check-ins · updated ${day}`,
    findings: shown.map(findingFrom),
  };
}

/** Row as src-tauri experiments.rs `ExperimentResult` (camelCase). `label` is the backend's words: better | worse | no clear difference. */
export type ExperimentSideWire = { from: string; to: string; n: number; mean: number | null; days: string[] };
export type ExperimentResultWire = {
  followupId: number; startDay: string; measure: string | null; outcome: "better" | "worse" | "no_clear_difference"; label: string;
  reason: "no_measure" | "not_enough_data" | "not_significant" | null; p: number | null; before: ExperimentSideWire; after: ExperimentSideWire;
};

export type ReviewWire = {
  state: "ready" | "not_enough_data"; weekStart: string; weekEnd: string; dueAt: string; computedAt: string;
  health: { state: "data" | "none"; nights: number; avgSleepH: number | null; sleepDeltaH: number | null; avgHrv: number | null; avgRecovery: number | null; note: string };
  work: { state: "data" | "none"; focusSessions: number; avgFocus: number | null; note: string };
  checkins: { state: "data" | "none"; count: number; avgMood: number | null; avgEnergy: number | null; notes: Array<{ text: string; day: string; id: string }>; note: string };
  goals: { state: "data" | "none"; milestones: Array<{ id: string; title: string; current: number; target: number; unit: string; pct: number }>; habits: number; note: string };
  money: { state: "data" | "none"; personalChangeUsd: number | null; accounts: Array<{ needsConversion: boolean }>; note: string };
  suggestion: { title: string; reason: string; evidenceFindingId: string; confidence: number } | null;
  suggestionNote: string;
  summary: string | null;
  /** Absent in a review stored before experiments existed. */
  experiments?: ExperimentResultWire[];
};

const MONTH_DAY = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const WEEKDAY = new Intl.DateTimeFormat("en-US", { weekday: "short", timeZone: "UTC" });
const utc = (iso: string) => new Date(`${iso}T00:00:00Z`);
const hm = (h: number) => { const m = Math.round(h * 60); return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`; };
const usd = (v: number) => `${v < 0 ? "-" : ""}$${Math.abs(Math.round(v)).toLocaleString("en-US")}`;

export function weeklyReviewFrom(w: ReviewWire): WeeklyReview {
  const h = w.health, k = w.work, c = w.checkins, g = w.goals, m = w.money;
  const subHealth = [h.avgHrv != null ? `HRV ${Math.round(h.avgHrv)} ms` : null, h.avgRecovery != null ? `recovery avg ${Math.round(h.avgRecovery)}%` : null, h.nights ? `${h.nights} nights` : null].filter(Boolean).join(" · ");
  const delta = h.sleepDeltaH;
  const tiles: WeeklyReview["tiles"] = [
    h.state === "data" && h.avgSleepH != null
      ? { key: "Health", big: hm(h.avgSleepH), unit: "avg sleep", delta: delta == null ? "No comparison with last week yet" : `${delta >= 0 ? "▲" : "▼"} ${Math.abs(Math.round(delta * 60))} min vs last week`, up: delta == null || delta >= 0, sub: subHealth || h.note }
      : { key: "Health", big: "No data", unit: "", delta: "", up: true, sub: h.state === "data" ? subHealth : h.note },
    k.state === "data"
      ? { key: "Work", big: String(k.focusSessions), unit: k.focusSessions === 1 ? "focus session" : "focus sessions", delta: k.avgFocus != null ? `avg focus ${Math.round(k.avgFocus)}` : "", up: true, sub: k.note }
      : { key: "Work", big: "No data", unit: "", delta: "", up: true, sub: k.note },
    m.state === "data" && m.personalChangeUsd != null
      ? { key: "Money", big: usd(m.personalChangeUsd), unit: "personal balance change", delta: `${m.accounts.filter((a) => !a.needsConversion).length} USD account(s)`, up: m.personalChangeUsd >= 0, sub: m.note }
      : { key: "Money", big: "No data", unit: "", delta: "", up: true, sub: m.note },
  ];
  const s = w.suggestion;
  const change = s ? { title: s.title, body: s.reason } : { title: "No change to suggest this week", body: w.suggestionNote };
  const highlights: WeeklyReview["highlights"] = g.milestones.map((x) => ({ ok: x.pct >= 100, title: `${x.title}: ${x.current} of ${x.target} ${x.unit}`.trim(), sub: `${x.pct}%` }));
  if (g.state === "none") highlights.push({ ok: false, title: "No goals saved yet", sub: "" });
  if (g.habits > 0) highlights.push({ ok: false, title: "Habit days aren't tracked yet", sub: g.note });
  highlights.push(c.state === "data"
    ? { ok: true, title: `${c.count} check-in${c.count === 1 ? "" : "s"} this week`, sub: [c.avgMood != null ? `avg mood ${r1(c.avgMood)} of 5` : null, c.avgEnergy != null ? `energy ${r1(c.avgEnergy)} of 5` : null].filter(Boolean).join(" · ") }
    : { ok: false, title: "No check-ins this week", sub: "" });
  const a = utc(w.weekStart), b = utc(w.weekEnd);
  const range = `${MONTH_DAY.format(a)}–${a.getUTCMonth() === b.getUTCMonth() ? b.getUTCDate() : MONTH_DAY.format(b)}`;
  return {
    title: w.state === "ready" ? `Your week, ${range}` : "Not enough data this week yet",
    week: `Week of ${range} · health, work and money`,
    tiles, change, highlights,
    notes: c.notes.map((n) => ({ text: n.text, day: WEEKDAY.format(utc(n.day)) })),
    experiments: w.experiments ?? [],
  };
}
