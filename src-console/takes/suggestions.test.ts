// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { CATALOG } from "../amaraTake/catalog";
import { makeFact } from "../amaraTake/facts";
import type { FactSet } from "../amaraTake/types";
import type { NoopDayRow } from "../types";
import { detectEasyDay, detectEveningLights, detectLedgerDecision, detectWalk, hasDetector } from "./detectors";
import { answerOf, autoDoneOn, sidOf, type TakeEvent } from "./events";
import { bootstrapDiffCI90, computeOutcome } from "./outcome";
import { acceptWhy, gate, pickEntry, templateWhy, whyFor } from "./selection";
import { resetDetectThrottle, resolveToday, runDetectors, writeAnswer, writeUndo, type SuggestDeps } from "./suggestions";
import { buildTypeOutcomes, buildWeek, buildGoals, type Sources } from "./typeOutcomes";

const NOW = Date.parse("2026-11-12T10:00:00+09:00");
const prior = [60, 62, 61, 63, 60, 62, 61, 60, 62];
const set = (page: FactSet["page"], day: string, facts: ReturnType<typeof makeFact>[]): FactSet => ({ page, day, facts: facts.filter((f) => f !== null) as never, fingerprint: "fp" });
const hrvLow = (day = "2026-11-12") => set("mindBody", day, [makeFact({ key: "hrv", label: "HRV", unit: "ms" }, 52, prior)]);

function fakeDeps(over: Partial<SuggestDeps> = {}) {
  let events: TakeEvent[] = [];
  const asks: string[] = [];
  const deps: SuggestDeps = {
    now: () => NOW, events: () => events,
    append: async (e) => { if (events.some((x) => x.id === e.id)) return false; events = [...events, e]; return true; },
    facts: async (page, day) => (page === "mindBody" ? hrvLow(day) : null),
    ask: async (p) => { asks.push(p); return { content: "HRV is 52 ms this morning, under your usual 61 ms.", tier: "light" }; },
    sources: async () => ({ rows: [], museRelax: new Map() }), workouts: async () => [], ledger: async () => [], haStates: async () => [],
    ...over,
  };
  return { deps, asks, get events() { return events; }, set(e: TakeEvent[]) { events = e; } };
}

describe("selection", () => {
  it("is deterministic and picks the same entry every time", () => {
    const a = pickEntry({ page: "mindBody", set: hrvLow(), day: "2026-11-12" }), b = pickEntry({ page: "mindBody", set: hrvLow(), day: "2026-11-12" });
    expect(a.entry?.id).toBe("easy_day_low_hrv");
    expect(b.entry?.id).toBe(a.entry?.id);
  });
  it("never picks a weak entry, only lists it as also considered", () => {
    const relax = set("neuro", "2026-11-12", [makeFact({ key: "relax", label: "Average relaxation today", unit: "", decimals: 1 }, 50, prior)]);
    const p = pickEntry({ page: "neuro", set: relax, day: "2026-11-12" });
    expect(p.entry?.id).toBe("breathe_before_session");
    expect(p.alsoConsidered.map((e) => e.id)).toContain("reading_before_session");
  });
  it("skips stopped types and types in cooldown, and picks nothing when no trigger fires", () => {
    expect(pickEntry({ page: "mindBody", set: hrvLow(), day: "d", stopped: new Set(["easy_day_low_hrv"]) }).entry).toBeNull();
    expect(pickEntry({ page: "mindBody", set: hrvLow(), day: "2026-11-12", cooldownUntil: new Map([["easy_day_low_hrv", "2026-12-01"]]) }).entry).toBeNull();
    const normal = set("mindBody", "2026-11-12", [makeFact({ key: "hrv", label: "HRV", unit: "ms" }, 61, prior)]);
    expect(pickEntry({ page: "mindBody", set: normal, day: "2026-11-12" }).entry).toBeNull();
  });
  it("honours contraindications (walk is held back at recovery under 34%)", () => {
    const f = set("fitness", "d", [makeFact({ key: "exercises", label: "Workouts logged today", unit: "" }, 0, []), makeFact({ key: "recovery", label: "Recovery", unit: "%" }, 30, prior)]);
    expect(pickEntry({ page: "fitness", set: f, day: "d" }).entry).toBeNull();
  });
  it("gates on a 7+ day baseline", () => {
    const young = set("mindBody", "d", [makeFact({ key: "hrv", label: "HRV", unit: "ms" }, 52, [60, 61, 62])]);
    expect(gate(young)).toEqual({ ok: false, have: 3 });
    expect(gate(hrvLow()).ok).toBe(true);
  });
});

describe("why sentence and the CAUSAL guard", () => {
  const f = hrvLow().facts;
  it("rejects causal words and invented numbers, accepts a plain cited sentence", () => {
    expect(acceptWhy("HRV is 52 ms, under your usual 61 ms.", f)).not.toBeNull();
    for (const bad of ["HRV is 52 ms because of poor sleep.", "HRV is 52 ms, and this proves rest works.", "HRV is 52 ms; easy days cured it.", "HRV is 52 ms, guaranteed to recover.", "HRV is 52 ms, which caused this."]) expect(acceptWhy(bad, f)).toBeNull();
    expect(acceptWhy("HRV is 52 ms and 99 is the goal.", f)).toBeNull();
  });
  it("falls back to the template when the reply is causal, from the cloud, or the call fails", async () => {
    const entry = CATALOG.entries.find((e) => e.id === "easy_day_low_hrv")!;
    const fact = f[0], s = hrvLow();
    const t = templateWhy(fact);
    expect((await whyFor(entry, fact, s, async () => ({ content: "HRV is 52 ms because of stress.", tier: "light" }))).source).toBe("template");
    expect((await whyFor(entry, fact, s, async () => ({ content: "HRV is 52 ms, under your usual 61 ms.", tier: "claude" }))).text).toBe(t);
    expect((await whyFor(entry, fact, s, async () => { throw new Error("down"); })).text).toBe(t);
    expect((await whyFor(entry, fact, s, async () => ({ content: "HRV is 52 ms, under your usual 61 ms.", tier: "light" }))).source).toBe("model");
  });
});

describe("daily suggestion", () => {
  it("is chosen once per page per day and stored: a second read makes no router call", async () => {
    const h = fakeDeps();
    const a = await resolveToday("mindBody", h.deps, async () => []);
    const b = await resolveToday("mindBody", h.deps, async () => []);
    expect(a?.kind).toBe("suggestion"); expect(b).toEqual(a);
    expect(h.asks).toHaveLength(1);
    expect(h.events.filter((e) => e.kind === "today")).toHaveLength(1);
  });
  it("asks the router for one why sentence only; the sentence stays inside the facts", async () => {
    const h = fakeDeps(); await resolveToday("mindBody", h.deps, async () => []);
    expect(h.asks[0]).toMatch(/Explain in one sentence why/);
  });
  it("says learning with a real count before 7 days, and none-today without a reading", async () => {
    const young = fakeDeps({ facts: async (_p, d) => set("mindBody", d, [makeFact({ key: "hrv", label: "HRV", unit: "ms" }, 52, [60, 61, 62])]) });
    expect(await resolveToday("mindBody", young.deps, async () => [])).toEqual({ kind: "learning", have: 3, need: 7, unit: "days" });
    const none = fakeDeps({ facts: async () => null });
    expect((await resolveToday("mindBody", none.deps, async () => []))?.kind).toBe("none_today");
  });
  it("Business uses only its own ledger number", async () => {
    const h = fakeDeps({ ledger: async () => [{ id: "r1", status: "open", purpose: "x" }] });
    const b = await resolveToday("business", h.deps, async () => []);
    expect(b?.kind).toBe("suggestion");
    expect(h.asks[0]).toContain("Funding requests waiting: 1");
    expect(h.asks[0]).not.toMatch(/Revenue/);
  });
  it("catalog: every citation is a placeholder (verified false) and Business is own data", () => {
    for (const e of CATALOG.entries) for (const s of e.evidence.sources) expect(s.verified).toBe(false);
    expect(CATALOG.entries.find((e) => e.page === "business")?.evidence.level).toBe("own");
  });
});

describe("answers", () => {
  const sid = sidOf("mindBody", "2026-11-11", "easy_day_low_hrv");
  const ev = (kind: TakeEvent["kind"], data: Record<string, unknown>, at: number): TakeEvent => ({ id: `${kind}${at}`, kind, at, data: { sid, ...data } });
  it("an unanswered suggestion becomes unknown at midnight, never skipped", () => {
    expect(answerOf([], sid, "2026-11-11", "2026-11-11").answer.state).toBe("waiting");
    expect(answerOf([], sid, "2026-11-11", "2026-11-12").answer.state).toBe("unknown");
  });
  it("a tap wins over a detection, and Undo blocks re-detection", () => {
    const tap = ev("answer", { state: "skipped", source: "tap", reason: "no_time" }, 1), det = ev("answer", { state: "done", source: "detected", detail: "WHOOP" }, 2);
    expect(answerOf([tap, det], sid, "d", "d").answer.state).toBe("skipped");
    expect(answerOf([det], sid, "d", "d").answer).toMatchObject({ state: "done", source: "detected", detail: "WHOOP" });
    const undone = answerOf([det, ev("undo", {}, 3), ev("answer", { state: "done", source: "detected", detail: "again" }, 4)], sid, "d", "d");
    expect(undone.answer.state).toBe("waiting"); expect(undone.blockedDetection).toBe(true);
  });
  it("writeAnswer / writeUndo store events and refuse unknown suggestions or states", async () => {
    const h = fakeDeps(); await resolveToday("mindBody", h.deps, async () => []);
    const day = "2026-11-12", id = sidOf("mindBody", day, "easy_day_low_hrv");
    expect(await writeAnswer(h.deps, "mindBody", "nope", "done", null)).toBe(false);
    expect(await writeAnswer(h.deps, "mindBody", id, "unknown", null)).toBe(false);
    expect(await writeAnswer(h.deps, "mindBody", id, "skipped", "forgot")).toBe(true);
    expect(answerOf(h.events, id, day, day).answer).toMatchObject({ state: "skipped", reason: "forgot" });
    expect(await writeUndo(h.deps, "mindBody", id)).toBe(true);
    expect(answerOf(h.events, id, day, day).answer.state).toBe("waiting");
  });
  it("auto-done defaults on and follows the latest setting", () => {
    expect(autoDoneOn([])).toBe(true);
    expect(autoDoneOn([{ id: "1", kind: "setting", at: 1, data: { autoDone: false } }])).toBe(false);
  });
});

describe("detectors", () => {
  const walk = { day: "2026-11-12", startedAt: "2026-11-12T06:45:00Z", endedAt: "2026-11-12T07:12:00Z", sport: "Walking", durationMin: 27, steps: 3110 };
  it("WHOOP walk: 20+ min before 17:00 Tokyo gives a detail saying what was seen", () => {
    const d = detectWalk([walk], "2026-11-12");
    expect(d?.evidenceRef).toBe("whoop:2026-11-12T06:45:00Z");
    expect(d?.detail).toMatch(/WHOOP: 27-min walk, 3,110 steps/);
    expect(detectWalk([{ ...walk, durationMin: 12 }], "2026-11-12")).toBeNull();
    expect(detectWalk([{ ...walk, startedAt: "2026-11-12T09:00:00Z" }], "2026-11-12")).toBeNull();
    expect(detectWalk([{ ...walk, sport: "Running" }], "2026-11-12")).toBeNull();
  });
  it("WHOOP easy day: strain under 10, only once the day is over", () => {
    const row = { day: "2026-11-11", strain: 6.2 } as NoopDayRow;
    expect(detectEasyDay(row, "2026-11-11", "2026-11-12")?.detail).toMatch(/strain 6\.2/);
    expect(detectEasyDay(row, "2026-11-11", "2026-11-11")).toBeNull();
    expect(detectEasyDay({ ...row, strain: 12 }, "2026-11-11", "2026-11-12")).toBeNull();
    expect(detectEasyDay({ ...row, strain: null }, "2026-11-11", "2026-11-12")).toBeNull();
  });
  it("ledger: a request seen open earlier today and decided before noon counts; an overnight change does not", () => {
    const seen: TakeEvent[] = [{ id: "l", kind: "ledger_seen", at: Date.parse("2026-11-12T09:10:00+09:00"), day: "2026-11-12", data: { open: ["r1"] } }];
    const reqs = [{ id: "r1", status: "approved", purpose: "Taiyo Skin funding" }];
    expect(detectLedgerDecision(seen, reqs, NOW, "2026-11-12")?.evidenceRef).toBe("ledger:r1");
    expect(detectLedgerDecision([{ ...seen[0], day: "2026-11-11" }], reqs, NOW, "2026-11-12")).toBeNull();
    expect(detectLedgerDecision(seen, reqs, Date.parse("2026-11-12T13:00:00+09:00"), "2026-11-12")).toBeNull();
  });
  it("Home Assistant: the evening scene at 21:00 +/- 15 min; read-only", () => {
    const s = { entityId: "scene.evening_warm", state: "2026-11-12T12:04:00+00:00", friendlyName: "Evening warm", domain: "scene" };
    expect(detectEveningLights([s], "2026-11-12")?.detail).toMatch(/9:04 PM/);
    expect(detectEveningLights([{ ...s, state: "2026-11-12T13:00:00+00:00" }], "2026-11-12")).toBeNull();
  });
  it("only pages with a built detector report detects()", () => {
    expect(["fitness", "mindBody", "business", "home"].every((p) => hasDetector(CATALOG.entries, p))).toBe(true);
    expect(hasDetector(CATALOG.entries, "neuro")).toBe(false);
    expect(hasDetector(CATALOG.entries, "life")).toBe(false);
  });
  it("runDetectors writes a detected answer with a detail, once, and never over a tap or when auto-done is off", async () => {
    const h = fakeDeps({ workouts: async () => [walk], facts: async (p, d) => (p === "fitness" ? set("fitness", d, [makeFact({ key: "exercises", label: "Workouts logged today", unit: "" }, 0, []), makeFact({ key: "recovery", label: "Recovery", unit: "%" }, 70, prior)]) : null) });
    await resolveToday("fitness", h.deps, async () => []);
    resetDetectThrottle();
    expect(await runDetectors(h.deps, true)).toBe(1);
    const sid = sidOf("fitness", "2026-11-12", "brisk_walk");
    expect(answerOf(h.events, sid, "2026-11-12", "2026-11-12").answer).toMatchObject({ state: "done", source: "detected" });
    expect(answerOf(h.events, sid, "2026-11-12", "2026-11-12").answer.detail).toMatch(/WHOOP/);
    expect(await runDetectors(h.deps, true)).toBe(0);
    const off = fakeDeps({ workouts: async () => [walk], facts: h.deps.facts });
    await resolveToday("fitness", off.deps, async () => []);
    await off.deps.append({ id: "s", kind: "setting", at: 1, data: { autoDone: false } });
    expect(await runDetectors(off.deps, true)).toBe(0);
    const tapped = fakeDeps({ workouts: async () => [walk], facts: h.deps.facts });
    await resolveToday("fitness", tapped.deps, async () => []);
    await writeAnswer(tapped.deps, "fitness", sid, "skipped", "no_time");
    expect(await runDetectors(tapped.deps, true)).toBe(0);
  });
});

describe("outcome method v1", () => {
  const A = [67, 64, 70, 63, 69, 72, 66, 68, 71, 65, 69], B = [61, 58, 63, 59, 60, 59], C = [57, 60, 55, 59, 58, 54, 61, 56, 58, 57, 59, 60];
  const base = { seed: "breathe", better: "up" as const, wobbleFloor: 3, withVals: A, withoutVals: B, beforeVals: C, baseline28: C, excluded: { unknown: 1, notToday: 0, noMeasurement: 0 } };
  it("the bootstrap is seeded: same seed same range, another seed another range", () => {
    expect(bootstrapDiffCI90(A, B, "x")).toEqual(bootstrapDiffCI90(A, B, "x"));
    expect(bootstrapDiffCI90(A, B, "x")).not.toEqual(bootstrapDiffCI90(A, B, "y"));
    expect(computeOutcome(base)).toEqual(computeOutcome(base));
  });
  it("breathing fixture is better; with 4 days it is too early", () => {
    const r = computeOutcome(base);
    expect(r.verdict).toBe("better"); expect(r.diff).toBeGreaterThan(7); expect(r.diff).toBeLessThan(8); expect(r.ci90![0]).toBeGreaterThan(0);
    expect(computeOutcome({ ...base, withVals: A.slice(0, 4) }).verdict).toBe("too_early");
  });
  it("none, worse (for up and for down metrics) and unclear", () => {
    expect(computeOutcome({ ...base, withVals: [60, 59, 61, 60, 60, 59, 60], withoutVals: [59, 60, 60, 59, 61, 60] }).verdict).toBe("none");
    expect(computeOutcome({ ...base, withVals: B, withoutVals: A }).verdict).toBe("worse");
    expect(computeOutcome({ ...base, better: "down", withVals: B, withoutVals: A }).verdict).toBe("better");
    expect(computeOutcome({ ...base, confounders: [{ metric: "sleep duration", withVals: [450, 460, 455, 470], withoutVals: [380, 390, 385, 395] }] }).verdict).toBe("unclear");
    expect(computeOutcome({ ...base, weekend: { with: 0.6, without: 0.1 } }).verdict).toBe("unclear");
  });
  it("wobble is at least the floor and half the 28-day SD", () => {
    expect(computeOutcome({ ...base, wobbleFloor: 0.1 }).wobble).toBeGreaterThan(0.5);
    expect(computeOutcome(base).wobble).toBeGreaterThanOrEqual(3);
  });
});

describe("outcomes by type over time (from events)", () => {
  const entry = CATALOG.entries.find((e) => e.id === "easy_day_low_hrv")!;
  const days = Array.from({ length: 16 }, (_, i) => new Date(Date.parse("2026-10-20T00:00:00Z") + i * 86_400_000).toISOString().slice(0, 10));
  const rows = Array.from({ length: 60 }, (_, i) => { const day = new Date(Date.parse("2026-09-25T00:00:00Z") + i * 86_400_000).toISOString().slice(0, 10); return { day, avgHrv: 58 + (i % 3), totalSleepMin: 420, recovery: 66, efficiency: 90 } as NoopDayRow; });
  const events: TakeEvent[] = []; const sources: Sources = { rows, museRelax: new Map() };
  days.forEach((d, i) => {
    const sid = sidOf("mindBody", d, entry.id);
    events.push({ id: `t${d}`, kind: "today", at: i, page: "mindBody", day: d, data: { kind: "suggestion", recId: entry.id, why: "w", whySource: "template", facts: [] } });
    const state = i < 6 ? "done" : i < 12 ? "skipped" : i === 12 ? "not_today" : "done";
    events.push({ id: `a${d}`, kind: "answer", at: i, page: "mindBody", day: d, data: { sid, state, source: i % 2 ? "detected" : "tap" } });
    // next-morning HRV: higher after done days
    const next = new Date(Date.parse(`${d}T00:00:00Z`) + 86_400_000).toISOString().slice(0, 10);
    const row = rows.find((r) => r.day === next); if (row) row.avgHrv = state === "done" ? 66 + (i % 2) : 56 + (i % 3);
  });
  it("Not today and unknown days are excluded and counted; verdict and periods come from the rows", () => {
    const [o] = buildTypeOutcomes(events, "2026-11-12", sources);
    expect(o.type).toBe("easy_day_low_hrv");
    expect(o.excluded).toContain('"not today" days (1)');
    expect(o.stats.find((s) => s.label === "Did it")?.n).toBe(9);
    expect(o.stats.find((s) => s.label === "Suggested, not done")?.n).toBe(6);
    expect(o.verdict).toBe("better");
    expect(o.periods.length).toBeGreaterThanOrEqual(2);
    expect(o.periods[0].verdict).toBe("too_early");
    expect(buildTypeOutcomes(events, "2026-11-12", sources)).toEqual([o]);
  });
  it("Business has no measurement, so it is too early with a plain reason, never a number", () => {
    const biz = CATALOG.entries.find((e) => e.id === "decide_before_noon")!;
    const ev: TakeEvent[] = days.map((d) => ({ id: `b${d}`, kind: "today", at: 1, page: "business", day: d, data: { kind: "suggestion", recId: biz.id, why: "w", whySource: "template", facts: [] } }));
    const [o] = buildTypeOutcomes(ev, "2026-11-12", sources);
    expect(o.verdict).toBe("too_early"); expect(o.diff).toBeNull(); expect(o.sentence).toMatch(/can't measure/);
  });
  it("week and goals: better results are ready to keep; keeping shows before/after and a 28-day re-check", () => {
    const [o] = buildTypeOutcomes(events, "2026-11-12", sources);
    expect(buildWeek(events, "2026-11-12", [o], [])).toBeNull();
    const w = buildWeek(events, "2026-11-04", [o], [{ text: "t", from: "Home, Wed" }]);
    expect(w?.rows[0]).toMatchObject({ page: "mindBody", verdict: "better" });
    expect(w?.rows[0].days).toHaveLength(7);
    expect(w?.change?.type).toBe("easy_day_low_hrv");
    const g = buildGoals(events, "2026-11-12", [o], sources);
    expect(g?.ready[0].outcome.type).toBe("easy_day_low_hrv");
    const kept = [...events, { id: "ta", kind: "type_action" as const, at: 1, day: "2026-10-01", page: "mindBody" as const, data: { type: "easy_day_low_hrv", action: "keep", note: "Did it: 67 ms (9 days)." } }];
    const g2 = buildGoals(kept, "2026-11-12", [o], sources);
    expect(g2?.ready[0].note).toMatch(/Habit since Oct 1\. Did it: 67 ms/);
    expect(g2?.ready[0].note).toMatch(/Re-check/);
    expect(buildWeek(kept, "2026-11-04", [o], [])?.change).toBeNull();
  });
});
