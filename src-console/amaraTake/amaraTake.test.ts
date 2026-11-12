import { describe, expect, it, vi } from "vitest";
import { businessFacts, fitnessFacts, homeFacts, mindBodyFacts, neuroFacts } from "./facts";
import { runTake, type TakeDeps } from "./generate";
import { takeKey, type KvDeps } from "./store";
import { MAX_TAKE_CHARS, numbersIn, validateTake } from "./validate";
import { hasBannedPhrase, sentenceCount, templateLift, TEMPLATES, validateLift } from "./encourage";
import type { NoopDayRow, NoopSnapshotPayload, RecordedSession, RevenueSummary } from "../types";
import type { Fact, FactSet, TakePage } from "./types";

const TODAY = "2026-11-11";
const NOW = Date.parse("2026-11-11T09:00:00+09:00");
const dayStr = (back: number) => new Date(Date.parse(`${TODAY}T00:00:00Z`) - back * 86_400_000).toISOString().slice(0, 10);

const row = (day: string, o: Partial<NoopDayRow> = {}): NoopDayRow => ({
  day, source: "t", totalSleepMin: 420, efficiency: 90, deepMin: 80, remMin: 90, lightMin: 250, disturbances: 1, restingHr: 52, avgHrv: 60, recovery: 70, strain: 10,
  exerciseCount: 1, spo2Pct: 97, skinTempDevC: 0, respRateBpm: 14, steps: 8000, activeKcalEst: 500, ...o,
});
const snap = (todayRow: Partial<NoopDayRow> | null, prior = 10): NoopSnapshotPayload => ({
  generatedAt: "x", logicalToday: TODAY, sources: {},
  freshness: { latestHeartRateSample: null, latestDailyMetricDay: null, latestAppleHealthDay: null, dailyRows: 0, appleDailyRows: 0 },
  recentDays: Array.from({ length: prior }, (_, i) => row(dayStr(prior - i))), appleDaily: [], today: todayRow ? row(TODAY, todayRow) : null,
});

describe("facts", () => {
  it("mindBody: today's values with a code-computed baseline", () => {
    const set = mindBodyFacts(snap({ avgHrv: 45, recovery: 41, totalSleepMin: 372 }), TODAY)!;
    const hrv = set.facts.find((f) => f.key === "hrv")!;
    expect(hrv.display).toBe("45 ms");
    expect(hrv.baseline).toMatchObject({ mean: 60, n: 10, display: "60 ms" });
    expect(set.facts.find((f) => f.key === "sleep")).toMatchObject({ display: "6.2 h" });
    expect(set.facts.find((f) => f.key === "sleep")!.accept).toEqual(expect.arrayContaining([6.2, 6, 12]));
  });
  it("no facts when the reading day is not today or the night has not synced", () => {
    expect(mindBodyFacts(snap(null), TODAY)).toBeNull();
    expect(mindBodyFacts(snap({ avgHrv: null, recovery: null, totalSleepMin: null }), TODAY)).toBeNull();
    expect(mindBodyFacts(null, TODAY)).toBeNull();
  });
  it("baseline is dropped under 7 prior days", () => {
    expect(mindBodyFacts(snap({ avgHrv: 45 }, 3), TODAY)!.facts[0].baseline).toBeNull();
  });
  it("fitness reads steps and strain from today's row", () => {
    const set = fitnessFacts(snap({ steps: 12345, strain: 14.2, avgHrv: null, recovery: null, totalSleepMin: null }), TODAY)!;
    expect(set.facts.find((f) => f.key === "steps")!.display).toBe("12,345");
    expect(set.facts.find((f) => f.key === "strain")!.display).toBe("14.2");
  });
  const sess = (id: string, start: string, focus: number): RecordedSession => ({ id, device_type: "muse", start, end: start, duration_secs: 600, data_points: 1, summary: { avg_focus: focus, avg_relaxation: 50, avg_hrv: 40 } });
  it("neuro: only sessions starting today (Tokyo), baseline from earlier ones, id in fingerprint", () => {
    const earlier = Array.from({ length: 8 }, (_, i) => sess(`e${i}`, `${dayStr(i + 1)}T03:00:00Z`, 50));
    const a = neuroFacts([...earlier, sess("t1", "2026-11-11T00:30:00Z", 70)], TODAY)!;
    expect(a.facts.find((f) => f.key === "focus")).toMatchObject({ value: 70, baseline: { mean: 50, n: 8 } });
    const b = neuroFacts([...earlier, sess("t1", "2026-11-11T00:30:00Z", 70), sess("t2", "2026-11-11T01:30:00Z", 70)], TODAY)!;
    expect(b.fingerprint).not.toBe(a.fingerprint);
    expect(neuroFacts(earlier, TODAY)).toBeNull();
  });
  it("business: none without any recorded money; today's revenue vs 7-day daily average", () => {
    const rev: RevenueSummary = { total_revenue: 0, total_expenses: 0, net_income: 0, today_revenue: 0, today_expenses: 0, week_revenue: 0, month_revenue: 0, by_strategy: [] };
    expect(businessFacts(rev, TODAY)).toBeNull();
    const set = businessFacts({ ...rev, total_revenue: 900, net_income: 800, total_expenses: 100, today_revenue: 140, week_revenue: 700, month_revenue: 900 }, TODAY)!;
    expect(set.facts.find((f) => f.key === "todayRevenue")!.baseline!.mean).toBe(100);
  });
  it("home: weather only when the brief is fresh", () => {
    const brief = { greeting: null, weather: { place: "Tokyo", tempC: 18, summary: "clear", highC: 21, lowC: 12 }, updatedAt: new Date(NOW - 60_000).toISOString() };
    expect(homeFacts(null, brief, TODAY, NOW)!.facts.map((f) => f.key)).toEqual(["tempC", "highC", "lowC"]);
    expect(homeFacts(null, { ...brief, updatedAt: new Date(NOW - 5 * 3_600_000).toISOString() }, TODAY, NOW)).toBeNull();
  });
});

const fact = (key: string, value: number, extra: Partial<Fact> = {}): Fact => ({ key, label: key, value, unit: "", display: String(value), baseline: null, accept: [value], ...extra });

describe("validateTake", () => {
  const facts = [fact("hrv", 45, { baseline: { mean: 60, n: 10, display: "60 ms" }, accept: [45, 60] }), fact("steps", 12345)];
  it("accepts a take citing fixture numbers, with tolerant formatting", () => {
    expect(validateTake("HRV is 45 ms, below your usual 60.", facts)).toMatchObject({ ok: true, cited: [{ key: "hrv" }] });
    expect(validateTake("You walked 12,345 steps.", facts)).toMatchObject({ ok: true, cited: [{ key: "steps" }] });
    expect(validateTake("About 12345 steps.", facts).ok).toBe(true);
    expect(validateTake("HRV 45.0 today", facts).ok).toBe(true);
  });
  it("rejects a vague take, other numbers, empty, currency", () => {
    expect(validateTake("You are doing well today, steady as ever.", facts)).toEqual({ ok: false, reason: "no_fact_cited" });
    expect(validateTake("HRV is 99.", facts).ok).toBe(false);
    expect(validateTake("  ", facts)).toEqual({ ok: false, reason: "empty" });
    expect(validateTake("Revenue 45 ¥", facts)).toEqual({ ok: false, reason: "currency" });
    // A real figure next to an invented one is still rejected.
    expect(validateTake("HRV is 45 ms and you slept 9 hours.", facts)).toEqual({ ok: false, reason: "unsupported_number" });
    expect(validateTake("HRV 45 against your 10-day usual of 60.", facts).ok).toBe(true);
    expect(numbersIn("1,234.5 and 7")).toEqual([1234.5, 7]);
  });
});

function harness(over: Partial<TakeDeps> & { setFacts?: () => FactSet | null } = {}) {
  const store = new Map<string, string>();
  const kv: KvDeps = { load: async (k) => store.get(k) ?? null, save: async (k, v) => { store.set(k, v); } };
  let current: FactSet | null = { page: "mindBody", day: TODAY, facts: [fact("hrv", 45)], fingerprint: "hrv=45" };
  const ask = vi.fn(async () => ({ content: "HRV is 45 today.", tier: "light" }));
  const deps: TakeDeps = { now: () => NOW, kv, ask, facts: async (_p: TakePage) => (over.setFacts ? over.setFacts() : current), ...over };
  return { deps, ask, store, set: (s: FactSet | null) => { current = s; } };
}

describe("runTake", () => {
  it("no generation when the day's data is missing", async () => {
    const h = harness({ setFacts: () => null });
    expect(await runTake("mindBody", h.deps)).toEqual({ state: "unwired" });
    expect(h.ask).not.toHaveBeenCalled();
  });
  it("miss generates and stores with cited points, keyed page+day; hit does not call the router", async () => {
    const h = harness();
    const first = await runTake("mindBody", h.deps);
    expect(first).toMatchObject({ state: "live", value: { page: "mindBody", day: TODAY, text: "HRV is 45 today.", cited: [{ key: "hrv" }] } });
    expect(JSON.parse(h.store.get(takeKey("mindBody"))!)).toMatchObject({ day: TODAY, attempts: 1 });
    const second = await runTake("mindBody", h.deps);
    expect(second).toEqual(first);
    expect(h.ask).toHaveBeenCalledTimes(1);
  });
  it("refreshes once when the fingerprint changes, then holds", async () => {
    const h = harness();
    await runTake("mindBody", h.deps);
    h.set({ page: "mindBody", day: TODAY, facts: [fact("hrv", 52)], fingerprint: "hrv=52" });
    h.ask.mockResolvedValueOnce({ content: "HRV rose to 52.", tier: "light" });
    const refreshed = await runTake("mindBody", h.deps);
    expect(refreshed).toMatchObject({ state: "live", value: { text: "HRV rose to 52." } });
    h.set({ page: "mindBody", day: TODAY, facts: [fact("hrv", 58)], fingerprint: "hrv=58" });
    const held = await runTake("mindBody", h.deps);
    expect(held).toMatchObject({ state: "live", value: { text: "HRV rose to 52." } });
    expect(h.ask).toHaveBeenCalledTimes(2);
  });
  it("a new day misses even with a stored take from yesterday", async () => {
    const h = harness();
    await runTake("mindBody", h.deps);
    const tomorrow = { ...h.deps, now: () => NOW + 86_400_000 };
    await runTake("mindBody", tomorrow);
    expect(h.ask).toHaveBeenCalledTimes(2);
  });
  it("a vague reply, a router failure or a cloud reply stays unwired and is bounded to 2 attempts", async () => {
    const h = harness();
    h.ask.mockResolvedValue({ content: "Looking good, keep going!", tier: "light" });
    expect(await runTake("mindBody", h.deps)).toEqual({ state: "unwired" });
    h.ask.mockRejectedValueOnce(new Error("router down"));
    expect(await runTake("mindBody", h.deps)).toEqual({ state: "unwired" });
    expect(await runTake("mindBody", h.deps)).toEqual({ state: "unwired" });
    expect(h.ask).toHaveBeenCalledTimes(2);
    const c = harness();
    c.ask.mockResolvedValue({ content: "HRV is 45.", tier: "claude" });
    expect(await runTake("mindBody", c.deps)).toEqual({ state: "unwired" });
  });
  it("the prompt carries only the facts", async () => {
    const h = harness();
    await runTake("mindBody", h.deps);
    const prompt = (h.ask.mock.calls[0] as unknown as [string])[0];
    expect(prompt).toContain("- hrv: 45");
  });
  it("a failed refresh keeps the earlier valid take from the same day", async () => {
    const h = harness();
    await runTake("mindBody", h.deps);
    h.set({ page: "mindBody", day: TODAY, facts: [fact("hrv", 52)], fingerprint: "hrv=52" });
    h.ask.mockResolvedValueOnce({ content: "vague", tier: "light" });
    expect(await runTake("mindBody", h.deps)).toMatchObject({ state: "live", value: { text: "HRV is 45 today." } });
  });
});

describe("encouragement lift", () => {
  const withRecent = (log: Array<{ text: string; generatedAt: number }>, over: Partial<TakeDeps> = {}) => harness({ recent: () => log, ...over });
  it("banned phrases are rejected in takes and lifts", () => {
    const facts = [fact("hrv", 45)];
    for (const p of ["you've got this", "crushing it", "Amazing", "so proud of you", "keep it up", "nailed it"]) {
      expect(validateTake(`HRV is 45, ${p}.`, facts)).toEqual({ ok: false, reason: "banned_phrase" });
      expect(validateLift(`That is ${p}.`, facts)).toEqual({ ok: false, reason: "banned_phrase" });
    }
  });
  it("lift guard: at most two sentences, no unsourced numbers, no causal or medical wording", () => {
    const facts = [fact("hrv", 45)];
    expect(validateLift("Steady work. Well kept.", facts).ok).toBe(true);
    expect(validateLift("One. Two. Three.", facts)).toEqual({ ok: false, reason: "too_many_sentences" });
    expect(validateLift("You did 7 sessions.", facts)).toEqual({ ok: false, reason: "unsupported_number" });
    expect(validateLift("HRV 45 is steady.", facts).ok).toBe(true);
    expect(validateLift("Your practice fixed it.", facts)).toEqual({ ok: false, reason: "causal" });
    expect(validateLift("See a doctor.", facts)).toEqual({ ok: false, reason: "medical" });
  });
  it("every template passes its own guard with a numbered fact, and none is banned", () => {
    const facts = [fact("goalRun", 12, { label: "Goal: runs", display: "12 runs" })];
    for (const t of TEMPLATES) {
      const text = t.text.replace("{fact}", "Goal: runs at 12 runs");
      expect(validateLift(text, facts), t.text).toEqual({ ok: true });
    }
  });
  it("a take gets one lift, stays within 3 sentences and the length cap, and cites a fact", async () => {
    const h = withRecent([]);
    const r = await runTake("mindBody", h.deps);
    if (r.state !== "live") throw new Error("expected live");
    expect(r.value.text.startsWith("HRV is 45 today. ")).toBe(true);
    expect(sentenceCount(r.value.text)).toBeLessThanOrEqual(3);
    expect(r.value.text.length).toBeLessThanOrEqual(MAX_TAKE_CHARS);
    expect(hasBannedPhrase(r.value.text)).toBe(false);
  });
  it("rotates to the least-engaged dimension and never repeats a line within 14 days", () => {
    const facts = [fact("hrv", 45)];
    const ctx = (log: Array<{ text: string; generatedAt: number }>) => ({ page: "mindBody" as TakePage, facts, log, now: NOW });
    const first = templateLift(ctx([]))!;
    const dim1 = TEMPLATES.find((t) => t.text === first)!.dim;
    expect(dim1).toBe("body");
    const second = templateLift(ctx([{ text: first, generatedAt: NOW - 86_400_000 }]))!;
    expect(second).not.toBe(first);
    expect(TEMPLATES.find((t) => t.text === second)!.dim).toBe("mind");
    // After 14 days the line is free again.
    expect(templateLift(ctx([{ text: first, generatedAt: NOW - 15 * 86_400_000 }]))).toBe(first);
    // Once every fitting line was used recently there is no lift rather than a repeat.
    const all = TEMPLATES.filter((t) => !t.fact || !t.goal).map((t) => ({ text: t.text.replace("{fact}", "x"), generatedAt: NOW - 1000 }));
    expect(templateLift(ctx(all))).toBeNull();
  });
  it("a goal fact selects goal-tied encouragement using only that fact's numbers", () => {
    const g = fact("goalRun", 12, { label: "Goal: runs", display: "12 runs" });
    const text = templateLift({ page: "life", facts: [g], log: [], now: NOW })!;
    expect(text).toContain("Goal: runs at 12 runs");
    expect(validateLift(text, [g]).ok).toBe(true);
  });
  it("router down falls back to a template take with a lift; without the log it stays unwired", async () => {
    const h = withRecent([]);
    h.ask.mockRejectedValue(new Error("router down"));
    const r = await runTake("mindBody", h.deps);
    expect(r).toMatchObject({ state: "live", value: { cited: [{ key: "hrv" }] } });
    if (r.state === "live") { expect(r.value.text.startsWith("hrv: 45.")).toBe(true); expect(sentenceCount(r.value.text)).toBeLessThanOrEqual(3); }
    const plain = harness();
    plain.ask.mockRejectedValue(new Error("router down"));
    expect(await runTake("mindBody", plain.deps)).toEqual({ state: "unwired" });
  });
});
