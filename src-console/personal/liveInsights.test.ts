import { describe, expect, it } from "vitest";
import { makeLivePersonal } from "./adapters";
import { patternsFrom, weeklyReviewFrom, type PatternsWire, type ReviewWire } from "./liveInsights";

const finding = (o: Partial<PatternsWire["findings"][number]> = {}): PatternsWire["findings"][number] => ({
  id: "sleep_focus", pair: "sleep_focus", title: "Sleep → next Neuro session", lagDays: 0, windowDays: 28, nWith: 8, nWithout: 6,
  meanWith: 64.04, meanWithout: 55.01, delta: 9.03, r: 0.9, p: 0.001, confidence: 4, strength: "Strong", n: "14 days",
  sentence: "After 7 h+ of sleep, your next session's focus averages **64.0**, vs **55.0** after shorter nights.", detail: "That's **+9.0 points**.",
  computedAt: "2026-11-12T10:00:00.000+09:00", sourceIds: ["session:s1", "noop:sleep_total_min:2026-11-01"],
  chart: { kind: "scatter", points: [[6.04, 51.2]], split: 7, x: [6, 8], y: [50, 70], xLabel: "Sleep (h)", yLabel: "Next-session focus" }, ...o,
});
const ready = (f = [finding()]): PatternsWire => ({ state: "ready", computedAt: "2026-11-12T10:00:00.000+09:00", windowDays: 28, findings: f, withheld: 1, counts: { sleepDays: 20, hrvDays: 20, strainDays: 20, sessions: 14, checkins: 5 } });

const emptyReview = (): ReviewWire => ({
  state: "not_enough_data", weekStart: "2026-11-02", weekEnd: "2026-11-08", dueAt: "x", computedAt: "x",
  health: { state: "none", nights: 0, avgSleepH: null, sleepDeltaH: null, avgHrv: null, avgRecovery: null, note: "No sleep, HRV or recovery rows for this week." },
  work: { state: "none", focusSessions: 0, avgFocus: null, note: "No Muse sessions this week." },
  checkins: { state: "none", count: 0, avgMood: null, avgEnergy: null, notes: [], note: "No check-ins this week." },
  goals: { state: "none", milestones: [], habits: 0, note: "No goals saved yet." },
  money: { state: "none", personalChangeUsd: null, accounts: [], note: "No personal accounts yet." },
  suggestion: null, suggestionNote: "Amara suggests a change only from a pattern with confidence 3 or higher. None has reached that yet.", summary: null,
});

describe("patternsFrom", () => {
  it("maps a ready result without changing any sentence", () => {
    const p = patternsFrom(ready());
    expect(p.findings).toHaveLength(1);
    expect(p.findings[0].sentence).toBe(finding().sentence);
    expect(p.findings[0].confidence).toBe(4);
    expect(p.findings[0].chart).toMatchObject({ kind: "scatter", split: 7, points: [[6, 51.2]] });
    expect(p.window).toContain("14 Neuro sessions");
    expect(p.window).toContain("updated 2026-11-12");
  });
  it("never shows a finding under confidence 2, even if the backend sent one", () => {
    const p = patternsFrom(ready([finding({ id: "low", confidence: 1, title: "Low" }), finding()]));
    expect(p.findings.map((f) => f.id)).toEqual(["sleep_focus"]);
    expect(p.headline).toContain("Sleep");
    const only = patternsFrom(ready([finding({ id: "low", confidence: 1 })]));
    expect(only.findings).toEqual([]);
    expect(only.headline).toBe("Not enough data yet");
  });
  it("not_enough_data is a live, honest empty state with the backend's reason", () => {
    const p = patternsFrom({ ...ready([]), state: "not_enough_data", reason: "Not enough data yet. Amara needs 3 days." });
    expect(p.findings).toEqual([]);
    expect(p.headline).toBe("Not enough data yet");
    expect(p.window).toBe("Not enough data yet. Amara needs 3 days.");
  });
});

describe("weeklyReviewFrom", () => {
  it("an empty week says No data in every tile and invents nothing", () => {
    const r = weeklyReviewFrom(emptyReview());
    expect(r.title).toBe("Not enough data this week yet");
    expect(r.tiles.map((t) => t.big)).toEqual(["No data", "No data", "No data"]);
    expect(r.change.title).toBe("No change to suggest this week");
    expect(r.notes).toEqual([]);
    expect(r.highlights.map((h) => h.ok)).toEqual([false, false]);
  });
  it("formats real numbers only from the wire", () => {
    const w = emptyReview();
    w.state = "ready";
    w.health = { ...w.health, state: "data", nights: 5, avgSleepH: 7.3, sleepDeltaH: 0.5, avgHrv: 59.2, avgRecovery: 71, note: "" };
    w.work = { ...w.work, state: "data", focusSessions: 5, avgFocus: 61.4 };
    w.money = { state: "data", personalChangeUsd: 250, accounts: [{ needsConversion: false }, { needsConversion: true }], note: "n" };
    w.checkins = { ...w.checkins, state: "data", count: 2, avgMood: 3.5, avgEnergy: 3, notes: [{ text: "Slept well.", day: "2026-11-05", id: "checkin:c1" }] };
    w.goals = { state: "data", milestones: [{ id: "g", title: "Revenue", current: 1250, target: 5000, unit: "USD", pct: 25 }], habits: 0, note: "" };
    const r = weeklyReviewFrom(w);
    expect(r.tiles[0]).toMatchObject({ big: "7h 18m", unit: "avg sleep", delta: "▲ 30 min vs last week", up: true });
    expect(r.tiles[0].sub).toBe("HRV 59 ms · recovery avg 71% · 5 nights");
    expect(r.tiles[1]).toMatchObject({ big: "5", delta: "avg focus 61" });
    expect(r.tiles[2]).toMatchObject({ big: "$250", unit: "personal balance change", delta: "1 USD account(s)" });
    expect(r.highlights[0]).toMatchObject({ ok: false, title: "Revenue: 1250 of 5000 USD", sub: "25%" });
    expect(r.notes).toEqual([{ text: "Slept well.", day: "Thu" }]);
    expect(r.week).toBe("Week of Nov 2–8 · health, work and money");
    expect(JSON.stringify(r)).not.toContain("¥");
  });
  it("money with yen only shows No data, not a number", () => {
    const w = emptyReview();
    w.money = { state: "data", personalChangeUsd: null, accounts: [{ needsConversion: true }], note: "Yen accounts need a conversion rule before they count." };
    expect(weeklyReviewFrom(w).tiles[2]).toMatchObject({ big: "No data", sub: w.money.note });
  });
  it("the suggestion is the backend's, quoted", () => {
    const w = emptyReview();
    w.suggestion = { title: "Protect 7 hours of sleep before your focus sessions.", reason: "After 7 h+ ...", evidenceFindingId: "sleep_focus", confidence: 4 };
    expect(weeklyReviewFrom(w).change).toEqual({ title: w.suggestion.title, body: "After 7 h+ ..." });
  });
});

describe("weeklyReviewFrom experiments", () => {
  it("passes experiment results through untouched, and is empty with none (or an older stored review)", () => {
    const side = { from: "2026-11-01", to: "2026-11-07", n: 7, mean: 80, days: ["2026-11-01"] };
    const e = { followupId: 1, startDay: "2026-11-01", measure: "focus_mean", outcome: "no_clear_difference" as const, label: "no clear difference", reason: "not_significant" as const, p: 0.4, before: side, after: side };
    const w = emptyReview();
    expect(weeklyReviewFrom(w).experiments).toEqual([]);
    w.experiments = [e];
    expect(weeklyReviewFrom(w).experiments).toEqual([e]);
  });
});

describe("live adapter", () => {
  it("is unwired until the first read, live after, and unwired again if the commands fail", async () => {
    const ok = makeLivePersonal(async (cmd: string) => {
      if (cmd === "patterns_read") return ready() as never;
      if (cmd === "weekly_review_read") return emptyReview() as never;
      throw new Error("no");
    });
    expect(ok.patterns().state).toBe("unwired");
    expect(ok.weeklyReview().state).toBe("unwired");
    await ok.refresh();
    expect(ok.patterns().state).toBe("live");
    expect(ok.weeklyReview().state).toBe("live");
    const bad = makeLivePersonal(async () => { throw new Error("no"); });
    await bad.refresh();
    expect(bad.patterns().state).toBe("unwired");
    expect(bad.weeklyReview().state).toBe("unwired");
  });
});
