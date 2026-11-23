// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { CATALOG, FITNESS_KINDS, type CatalogEntry } from "../amaraTake/catalog";
import { makeFact } from "../amaraTake/facts";
import type { FactSet } from "../amaraTake/types";
import { blocked, evalTrigger, pickEntry } from "./selection";
import { readinessLevel, seeded, seriesLevel, type OfferRecord } from "./variety";

const prior = [60, 62, 61, 63, 60, 62, 61, 60, 62];
const sleepPrior = [7.5, 7.4, 7.6, 7.5, 7.5, 7.4, 7.6, 7.5, 7.5].map((h) => h * 60);
const facts = (day: string, o: { recovery?: number; sleepH?: number } = {}): FactSet => {
  const fs = [
    makeFact({ key: "exercises", label: "Workouts logged today", unit: "" }, 0, []),
    makeFact({ key: "steps", label: "Steps today", unit: "" }, 3000, []),
    makeFact({ key: "strain", label: "Strain", unit: "", decimals: 1 }, 5, [8, 9, 8, 10, 9, 8, 9, 9, 8]),
    makeFact({ key: "recovery", label: "Recovery", unit: "%" }, o.recovery ?? 55, prior),
    makeFact({ key: "sleep", label: "Sleep last night", unit: "h", decimals: 1, scale: 1 / 60 }, (o.sleepH ?? 7.5) * 60, sleepPrior),
  ].filter((f) => f !== null);
  return { page: "fitness", day, facts: fs as never, fingerprint: "fp" };
};
const dayN = (i: number) => new Date(Date.UTC(2026, 10, 1 + i)).toISOString().slice(0, 10);
const FIT = CATALOG.entries.filter((e) => e.page === "fitness");

/** Runs `n` days, offering one entry a day and remembering it, as the daily suggestion does. */
function simulate(n: number, make: (day: string) => FactSet, answer: (e: CatalogEntry) => OfferRecord["answer"] = () => "waiting") {
  const offers: OfferRecord[] = []; const picks: CatalogEntry[] = [];
  for (let i = 0; i < n; i++) {
    const day = dayN(i);
    const p = pickEntry({ page: "fitness", set: make(day), day, ctx: { hour: 10, workouts: [], loadRatio: 1, offers } });
    expect(p.entry).not.toBeNull();
    picks.push(p.entry!); offers.push({ day, recId: p.entry!.id, answer: answer(p.entry!) });
  }
  return picks;
}

describe("catalog schema", () => {
  it("every entry is valid and ids are unique", () => {
    const ids = new Set<string>();
    for (const e of CATALOG.entries) {
      expect(ids.has(e.id)).toBe(false); ids.add(e.id);
      for (const k of ["title", "how", "typeLabel", "shortTitle"] as const) expect(e[k].length).toBeGreaterThan(8);
      expect(e.cooldownDays).toBeGreaterThan(0);
      expect(["strong", "moderate", "early", "weak", "own"]).toContain(e.evidence.level);
      expect(e.trigger || e.triggers || e.page !== "mindBody").toBeTruthy();
      for (const s of e.evidence.sources) expect(s.verified).toBe(false);
      if (e.page === "fitness") { expect(FITNESS_KINDS).toContain(e.kind); expect(["easy", "moderate", "hard"]).toContain(e.intensity); }
      if (e.progression) expect(e.progression.level).toBeGreaterThan(0);
      if (e.intensity === "hard") expect((e.contraindications ?? []).length).toBeGreaterThan(0);
    }
  });
  it("has 30-40 fitness entries and every kind has at least 3", () => {
    expect(FIT.length).toBeGreaterThanOrEqual(30); expect(FIT.length).toBeLessThanOrEqual(40);
    for (const k of FITNESS_KINDS) expect(FIT.filter((e) => e.kind === k).length).toBeGreaterThanOrEqual(3);
  });
  it("progression series have consecutive levels from 1", () => {
    const bySeries = new Map<string, number[]>();
    for (const e of FIT) if (e.progression) bySeries.set(e.progression.series, [...(bySeries.get(e.progression.series) ?? []), e.progression.level]);
    expect(bySeries.size).toBeGreaterThanOrEqual(3);
    for (const lv of bySeries.values()) expect(lv.sort()).toEqual(lv.map((_, i) => i + 1));
  });
});

describe("variety over time", () => {
  it("offers at least 7 different entries in 14 days with the same facts, never the same kind 3 days running", () => {
    const picks = simulate(14, (d) => facts(d));
    expect(new Set(picks.map((e) => e.id)).size).toBeGreaterThanOrEqual(7);
    for (let i = 2; i < picks.length; i++) expect(picks[i].kind === picks[i - 1].kind && picks[i].kind === picks[i - 2].kind).toBe(false);
    for (let i = 1; i < picks.length; i++) expect(picks[i].id).not.toBe(picks[i - 1].id);
  });
  it("is stable within a day and differs between days", () => {
    const a = pickEntry({ page: "fitness", set: facts("2026-11-05"), day: "2026-11-05", ctx: { hour: 10 } });
    const b = pickEntry({ page: "fitness", set: facts("2026-11-05"), day: "2026-11-05", ctx: { hour: 10 } });
    expect(b.entry?.id).toBe(a.entry?.id);
    const ids = new Set(Array.from({ length: 10 }, (_, i) => pickEntry({ page: "fitness", set: facts(dayN(i)), day: dayN(i), ctx: { hour: 10 } }).entry?.id));
    expect(ids.size).toBeGreaterThan(2);
    expect(seeded("2026-11-05", "x")).toBe(seeded("2026-11-05", "x"));
  });
  it("avoids the kind of yesterday's actual workout when an alternative fits", () => {
    const day = "2026-11-10";
    const w = [{ day: "2026-11-09", startedAt: "", endedAt: "", sport: "Weightlifting", durationMin: 50, steps: null }];
    for (let i = 0; i < 6; i++) {
      const d = dayN(i + 10);
      const p = pickEntry({ page: "fitness", set: facts(d), day: d, ctx: { hour: 10, workouts: [{ ...w[0], day: new Date(Date.parse(`${d}T00:00:00Z`) - 86_400_000).toISOString().slice(0, 10) }] } });
      expect(p.entry?.kind).not.toBe("strength");
    }
    expect(day).toBeTruthy();
  });
});

describe("safety", () => {
  it("low recovery yields only easy recovery, mobility, flexibility or endurance sessions, every day", () => {
    const picks = simulate(14, (d) => facts(d, { recovery: 20 }));
    expect(readinessLevel(facts("d", { recovery: 20 }))).toBe(1);
    for (const e of picks) { expect(e.intensity).toBe("easy"); expect(["recovery", "mobility", "flexibility", "endurance", "combo", "calisthenics"]).toContain(e.kind); }
    expect(new Set(picks.map((e) => e.id)).size).toBeGreaterThanOrEqual(5);
  });
  it("never offers intensity when contraindicated: poor sleep blocks hard and max-effort sessions", () => {
    const poor = (d: string) => facts(d, { recovery: 80, sleepH: 4.5 });
    const picks = simulate(14, poor);
    for (const e of picks) { expect(e.intensity).not.toBe("hard"); expect(blocked(e, poor("d"), { loadRatio: 1 })).toBe(false); }
    const hard = FIT.filter((e) => e.intensity === "hard");
    expect(hard.length).toBeGreaterThan(0);
    for (const e of hard) expect(blocked(e, poor("d"))).toBe(true);
  });
  it("hard sessions need a high readiness reading, so unknown readiness never gets one", () => {
    const noRead: FactSet = { page: "fitness", day: "d", facts: [makeFact({ key: "steps", label: "Steps today", unit: "" }, 3000, [])!], fingerprint: "x" };
    for (const e of FIT.filter((x) => x.intensity === "hard")) expect(evalTrigger(e, noRead, { loadRatio: 1 }).fires).toBe(false);
  });
});

describe("progressive challenges", () => {
  const ladder = FIT.filter((e) => e.progression?.series === "pushup_ladder");
  const pick = (offers: OfferRecord[], day: string) => pickEntry({ page: "fitness", set: facts(day), day, entries: ladder, ctx: { hour: 10, offers } }).entry?.id;
  it("starts at level 1 and advances one level after the user answered did it", () => {
    expect(pick([], "2026-11-10")).toBe("pushup_ladder_1");
    expect(pick([{ day: "2026-11-05", recId: "pushup_ladder_1", answer: "waiting" }], "2026-11-10")).toBe("pushup_ladder_1");
    expect(pick([{ day: "2026-11-05", recId: "pushup_ladder_1", answer: "done" }], "2026-11-10")).toBe("pushup_ladder_2");
    expect(pick([{ day: "2026-11-05", recId: "pushup_ladder_1", answer: "done" }, { day: "2026-11-08", recId: "pushup_ladder_2", answer: "done" }], "2026-11-12")).toBe("pushup_ladder_3");
  });
  it("stays at the top level once it is done", () => {
    expect(seriesLevel("pushup_ladder", CATALOG.entries, [{ day: "d", recId: "pushup_ladder_4", answer: "done" }])).toBe(4);
  });
});

describe("back-compat for old entries", () => {
  const old = CATALOG.entries.find((e) => e.id === "easy_day_low_hrv")!;
  const noKind: CatalogEntry = { ...old, id: "old", kind: undefined, intensity: undefined, triggers: undefined, trigger: { factKey: "hrv", op: "below_baseline", by: 0.05 } };
  const hrv = (v: number): FactSet => ({ page: "mindBody", day: "2026-11-12", fingerprint: "f", facts: [makeFact({ key: "hrv", label: "HRV", unit: "ms" }, v, prior)!] });
  it("a single trigger still fires and ranks", () => {
    expect(evalTrigger(noKind, hrv(52)).fires).toBe(true);
    expect(evalTrigger(noKind, hrv(61)).fires).toBe(false);
    expect(pickEntry({ page: "mindBody", set: hrv(52), day: "2026-11-12", entries: [noKind] }).entry?.id).toBe("old");
  });
  it("old contraindications with `value` still block, and an entry with no trigger always fires", () => {
    const c: CatalogEntry = { ...noKind, trigger: undefined, contraindications: [{ factKey: "hrv", op: "below", value: 55 }] };
    expect(evalTrigger(c, hrv(61)).fires).toBe(true);
    expect(blocked(c, hrv(52))).toBe(true); expect(blocked(c, hrv(61))).toBe(false);
  });
  it("the mind-body entries still produce a pick when HRV is low and none when it is normal", () => {
    expect(pickEntry({ page: "mindBody", set: hrv(52), day: "2026-11-12" }).entry).not.toBeNull();
    expect(pickEntry({ page: "mindBody", set: hrv(61), day: "2026-11-12" }).entry).toBeNull();
  });
});
