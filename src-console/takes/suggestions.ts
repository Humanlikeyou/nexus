import { entryById, entriesFor, type DetectorId } from "../amaraTake/catalog";
import { fingerprintOf, makeFact } from "../amaraTake/facts";
import type { FactSet, TakePage } from "../amaraTake/types";
import { tokyoDay } from "../lib/mindBody";
import type { Wired } from "../lib/sampleAdapters";
import type { TypeOutcome } from "./adapters";
import type { Suggestion, TodayState } from "./adapters";
import { detectEasyDay, detectEveningLights, detectLedgerDecision, detectWalk, hasDetector, ledgerSeenData, type Detection, type HaState, type LedgerRequest, type Workout } from "./detectors";
import { answerOf, autoDoneOn, sidOf, todayRecords, typeActions, type TakeEvent, type TodayRecord } from "./events";
import { gate, pickEntry, viewFacts, whyFor } from "./selection";
import { buildGoals, buildTypeOutcomes, buildWeek, type Sources } from "./typeOutcomes";

/** Daily selection, answers, detectors and outcomes over the event store. All reads are injected so tests can run it. */
export type SuggestDeps = {
  now: () => number;
  events: () => TakeEvent[] | null;
  append: (e: TakeEvent) => Promise<boolean>;
  facts: (page: TakePage, today: string) => Promise<FactSet | null>;
  ask: (prompt: string) => Promise<{ content: string; tier: string }>;
  sources: () => Promise<Sources | null>;
  workouts: () => Promise<Workout[] | null>;
  ledger: () => Promise<LedgerRequest[] | null>;
  haStates: () => Promise<HaState[] | null>;
};

export const SUGGEST_PAGES: TakePage[] = ["neuro", "fitness", "mindBody", "business", "home"];
export const dayOf = (ms: number) => tokyoDay(new Date(ms).toISOString()) ?? "";
const DAY = 86_400_000;
const addDays = (day: string, n: number) => new Date(Date.parse(`${day}T00:00:00Z`) + n * DAY).toISOString().slice(0, 10);

export const DETECT_RULE: Record<DetectorId, string | null> = {
  whoop_walk: "Counts by itself from WHOOP: a walk of 20+ min starting before 5 PM.",
  whoop_easy_day: "Counts by itself if WHOOP day strain ends under 10.",
  ledger_decision_before_noon: "Counts by itself when a waiting request is decided in your ledger before 12:00 JST.",
  ha_evening_lights: "Counts by itself if Home Assistant shows the evening warm scene applied at 9 PM ± 15 min.",
  muse_breathing_before_session: null, checkin_saved: null,
};

/** The business trigger fact comes from the ledger (own data). Null when the ledger can't be read. */
export function ledgerFact(reqs: readonly LedgerRequest[] | null) {
  return reqs ? makeFact({ key: "openRequests", label: "Funding requests waiting", unit: "" }, reqs.filter((r) => r.status === "open").length) : null;
}
export async function factsFor(page: TakePage, day: string, deps: SuggestDeps): Promise<FactSet | null> {
  const base = await deps.facts(page, day).catch(() => null);
  if (page !== "business") return base;
  const lf = ledgerFact(await deps.ledger().catch(() => null));
  if (!lf) return base;
  const facts = [lf, ...(base?.facts ?? [])];
  return { page, day, facts, fingerprint: fingerprintOf(facts) };
}

export type TodayBase =
  | { kind: "suggestion"; suggestion: Omit<Suggestion, "answer">; day: string }
  | { kind: "learning"; have: number; need: number; unit: string }
  | { kind: "none_today"; why: string }
  | { kind: "followup"; outcome: TypeOutcome };

function suggestionFromRecord(rec: Extract<TodayRecord, { kind: "suggestion" }>, events: readonly TakeEvent[], day: string): TodayBase | null {
  const entry = entryById(rec.recId); if (!entry) return null;
  const since = addDays(day, -30);
  const n = todayRecords(events).filter((r) => r.kind === "suggestion" && r.page === entry.page && r.recId === entry.id && r.day >= since).length;
  const also = (rec as { also?: string }).also;
  return { kind: "suggestion", day, suggestion: {
    id: sidOf(entry.page, day, entry.id), page: entry.page, type: entry.id, typeLabel: entry.typeLabel, title: entry.title, how: entry.how, why: rec.why, facts: rec.facts,
    evidence: { level: entry.evidence.level, summary: entry.evidence.summary, sources: entry.evidence.sources, ...(entry.evidence.risk ? { risk: entry.evidence.risk } : {}), ...(also ? { alsoConsidered: also } : {}) },
    detectRule: entry.detector && hasDetector([entry], entry.page) ? DETECT_RULE[entry.detector] : null,
    seen: n <= 1 ? "First time this month" : `Suggested ${n} times in the last 30 days`,
  } };
}

/** Cooldowns and stops, from earlier decisive follow-ups and the user's own Stop. */
export function holds(events: readonly TakeEvent[]): { stopped: Set<string>; cooldownUntil: Map<string, string> } {
  const stopped = new Set<string>(); const cooldownUntil = new Map<string, string>();
  for (const a of typeActions(events).values()) if (a.action === "stop") stopped.add(a.type);
  for (const r of todayRecords(events)) {
    if (r.kind !== "followup" || (r.verdict !== "none" && r.verdict !== "worse")) continue;
    const e = entryById(r.type); if (!e) continue;
    const until = addDays(r.day, e.cooldownDays);
    if (until > (cooldownUntil.get(e.id) ?? "")) cooldownUntil.set(e.id, until);
  }
  return { stopped, cooldownUntil };
}

export async function resolveToday(page: TakePage, deps: SuggestDeps, outcomes: () => Promise<TypeOutcome[]>): Promise<TodayBase | null> {
  const events = deps.events(); if (!events) return null;
  const now = deps.now(), day = dayOf(now);
  const recs = todayRecords(events).filter((r) => r.page === page && r.day === day);
  const fu = recs.find((r) => r.kind === "followup");
  if (fu && fu.kind === "followup") { const o = (await outcomes()).find((x) => x.type === fu.type); if (o) return { kind: "followup", outcome: o }; }
  const sug = recs.find((r) => r.kind === "suggestion");
  if (sug && sug.kind === "suggestion") return suggestionFromRecord(sug, events, day);
  // A decisive result nobody has been told about yet becomes today's follow-up (once per type and verdict).
  const told = new Set(todayRecords(events).filter((r) => r.kind === "followup").map((r) => r.kind === "followup" ? `${r.type}:${r.verdict}` : ""));
  const decisive = (await outcomes()).filter((o) => o.page === page && ["better", "worse", "none"].includes(o.verdict) && !told.has(`${o.type}:${o.verdict}`) && !typeActions(events).has(o.type));
  if (decisive[0]) {
    const o = decisive[0];
    const ok = await deps.append({ id: `today-${page}-${day}`, kind: "today", at: now, page, day, data: { kind: "followup", type: o.type, verdict: o.verdict } });
    return ok ? { kind: "followup", outcome: o } : null;
  }
  const set = await factsFor(page, day, deps);
  if (!set || !set.facts.length) return { kind: "none_today", why: "I have no reading for today yet, so I won't suggest anything." };
  const g = gate(set);
  if (!g.ok) return { kind: "learning", have: g.have, need: 7, unit: page === "neuro" ? "sessions" : page === "business" ? "readings" : "days" };
  const { stopped, cooldownUntil } = holds(events);
  const pick = pickEntry({ page, set, stopped, cooldownUntil, day });
  if (!pick.entry || !pick.fact) return { kind: "none_today", why: "Your numbers are where they usually are. A suggestion now would be noise." };
  const why = await whyFor(pick.entry, pick.fact, set, deps.ask);
  const facts = viewFacts(pick.fact, set);
  const also = pick.alsoConsidered.length ? pick.alsoConsidered.map((e) => `${e.shortTitle.toLowerCase()}${e.evidence.level === "weak" ? " (weak: not suggested)" : ""}`).join(" · ") : undefined;
  const data = { kind: "suggestion", recId: pick.entry.id, why: why.text, whySource: why.source, facts, ...(also ? { also } : {}) };
  const ok = await deps.append({ id: `today-${page}-${day}`, kind: "today", at: now, page, day, data });
  if (!ok) return null;
  // Re-read so the record shown is exactly the stored one.
  const fresh = deps.events() ?? [];
  const rec = todayRecords(fresh).find((r) => r.page === page && r.day === day && r.kind === "suggestion");
  return rec && rec.kind === "suggestion" ? suggestionFromRecord(rec, fresh, day) : null;
}

// ---------------- detectors ----------------
let lastDetect = 0;
/** Runs the detectors that exist for today's and yesterday's suggestions. A tap or an Undo always beats a detection. */
export async function runDetectors(deps: SuggestDeps, force = false): Promise<number> {
  const events = deps.events(); const now = deps.now();
  if (!events || (!force && now - lastDetect < 120_000)) return 0;
  lastDetect = now;
  const day = dayOf(now), yesterday = addDays(day, -1);
  const auto = autoDoneOn(events);
  let wrote = 0;
  const data: { workouts?: Workout[] | null; rows?: Sources | null; ledger?: LedgerRequest[] | null; ha?: HaState[] | null } = {};
  const sug = todayRecords(events).filter((r): r is Extract<TodayRecord, { kind: "suggestion" }> => r.kind === "suggestion" && (r.day === day || r.day === yesterday));

  // The ledger is watched (open requests per read) so a decision can be placed before noon; this is a record, not an answer.
  if (sug.some((r) => entryById(r.recId)?.detector === "ledger_decision_before_noon") || entriesFor("business").length) {
    data.ledger = await deps.ledger().catch(() => null);
    if (data.ledger) {
      const open = ledgerSeenData(data.ledger).open.sort().join(",");
      const last = [...events].reverse().find((e) => e.kind === "ledger_seen" && e.day === day);
      if (!last || (last.data.open as string[]).slice().sort().join(",") !== open) await deps.append({ id: `ledger-${now}`, kind: "ledger_seen", at: now, page: "business", day, data: ledgerSeenData(data.ledger) });
    }
  }
  if (!auto) return 0;
  const fresh = deps.events() ?? events;
  for (const r of sug) {
    const entry = entryById(r.recId); if (!entry?.detector) continue;
    const sid = sidOf(entry.page, r.day, entry.id);
    const view = answerOf(fresh, sid, r.day, day);
    if (view.blockedDetection || (view.answer.state !== "waiting" && view.answer.state !== "unknown") || view.answer.source === "tap") continue;
    let hit: Detection | null = null;
    if (entry.detector === "whoop_walk") { data.workouts ??= await deps.workouts().catch(() => null); hit = data.workouts ? detectWalk(data.workouts, r.day) : null; }
    else if (entry.detector === "whoop_easy_day") { data.rows ??= await deps.sources().catch(() => null); hit = data.rows ? detectEasyDay(data.rows.rows.find((x) => x.day === r.day), r.day, day) : null; }
    else if (entry.detector === "ledger_decision_before_noon" && r.day === day) hit = data.ledger ? detectLedgerDecision(fresh, data.ledger, now, r.day) : null;
    else if (entry.detector === "ha_evening_lights") { data.ha ??= await deps.haStates().catch(() => null); hit = data.ha ? detectEveningLights(data.ha, r.day) : null; }
    if (!hit) continue;
    if (await deps.append({ id: `det-${sid}`, kind: "answer", at: hit.at <= now ? hit.at : now, page: entry.page, day: r.day, data: { sid, state: "done", source: "detected", evidenceRef: hit.evidenceRef, detail: hit.detail } })) wrote += 1;
  }
  return wrote;
}
export const resetDetectThrottle = () => { lastDetect = 0; };

// ---------------- answers ----------------
export async function writeAnswer(deps: SuggestDeps, page: TakePage, sid: string, state: string, reason: string | null): Promise<boolean> {
  const events = deps.events(); if (!events) return false;
  const day = dayOf(deps.now());
  const rec = todayRecords(events).find((r) => r.page === page && r.kind === "suggestion" && sidOf(page, r.day, r.recId) === sid && r.day === day);
  if (!rec || !["done", "skipped", "not_today"].includes(state)) return false;
  const now = deps.now();
  return deps.append({ id: `ans-${sid}-${now}`, kind: "answer", at: now, page, day, data: { sid, state, source: "tap", reason: state === "skipped" ? reason : null } });
}
export async function writeUndo(deps: SuggestDeps, page: TakePage, sid: string): Promise<boolean> {
  const events = deps.events(); if (!events) return false;
  const now = deps.now();
  const day = dayOf(now);
  if (!todayRecords(events).some((r) => r.page === page && r.kind === "suggestion" && sidOf(page, r.day, r.recId) === sid)) return false;
  return deps.append({ id: `undo-${sid}-${now}`, kind: "undo", at: now, page, day, data: { sid } });
}

/** Outcomes, week and goals for the adapter, from stored events and measurement sources. */
export async function computeViews(deps: SuggestDeps, takes: Array<{ text: string; from: string }>) {
  const events = deps.events(); const src = await deps.sources().catch(() => null);
  if (!events || !src) return null;
  const day = dayOf(deps.now());
  const outcomes = buildTypeOutcomes(events, day, src);
  return { outcomes, week: buildWeek(events, day, outcomes, takes), goals: buildGoals(events, day, outcomes, src), src };
}

export const composeToday = (base: TodayBase, events: readonly TakeEvent[], todayDay: string): Wired<TodayState> => {
  if (base.kind !== "suggestion") return { state: "live", value: base };
  const view = answerOf(events, base.suggestion.id, base.day, todayDay);
  return { state: "live", value: { kind: "suggestion", suggestion: { ...base.suggestion, answer: view.answer } } };
};
