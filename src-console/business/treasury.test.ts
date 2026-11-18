import { describe, expect, it, vi } from "vitest";
import { UNWIRED } from "../lib/sampleAdapters";
import {
  isTreasurySnapshot, liveTreasury, liveTreasuryContacts, liveTreasuryCounts, loadTreasury, loadTreasuryCounts, loadTreasurySequences,
  unwiredTreasury, type TreasuryCalls, type TreasuryCountsCalls, type TreasurySequenceCalls, type TreasurySnapshot,
} from "./treasury";

const stream = { id: "str-1", ventureId: "v1", name: "Shop", streamType: "product_sales", status: "active", setupDate: null, notes: null, createdAt: "2026-11-01T00:00:00.000+09:00", ventureMissing: false };
const empty: TreasurySnapshot = { streams: [stream], months: [], trend: [], perHour: [], fx: null };
const range = { fromMonth: "2026-10", toMonth: "2026-11", from: "2026-11-01", to: "2026-11-30" };

describe("live Treasury adapter", () => {
  it("no snapshot, junk or no streams: exactly the unwired adapter", () => {
    expect(liveTreasury(null)).toBe(unwiredTreasury);
    for (const junk of [[], {}, "x", 1, { streams: [] }, { ...empty, streams: [] }]) expect(liveTreasury(junk)).toBe(unwiredTreasury);
    expect(isTreasurySnapshot([])).toBe(false);
  });

  it("streams are live, but panels with no real figures stay unwired (never 0)", () => {
    const a = liveTreasury(empty);
    expect(a.streams()).toEqual({ state: "live", value: [stream] });
    for (const k of ["incomeMonths", "trend", "perHour", "fx"] as const) expect(a[k]()).toBe(UNWIRED);
    const nulls: TreasurySnapshot = {
      ...empty,
      months: [{ streamId: "str-1", currency: null, month: "2026-11", totalMinor: null }],
      trend: [{ streamId: "str-1", currency: null, months: [null, null, null], avgMinor: null }],
      perHour: [{ streamId: "str-1", minutes: null, incomeMinor: null, currency: null, fxAsOf: null, fxStale: false, perHourMinor: null, reason: "no_hours" }],
    };
    const b = liveTreasury(nulls);
    for (const k of ["incomeMonths", "trend", "perHour"] as const) expect(b[k]()).toBe(UNWIRED);
  });

  it("real rows are live and keep their nulls and reasons untouched", () => {
    const snap: TreasurySnapshot = {
      ...empty,
      months: [{ streamId: "str-1", currency: "JPY", month: "2026-11", totalMinor: -300 }],
      perHour: [{ streamId: "str-1", minutes: 60, incomeMinor: null, currency: null, fxAsOf: null, fxStale: false, perHourMinor: null, reason: "needs_rate" }],
      fx: { rate: { usdPerJpy: 0.0066, asOf: "2026-11-10" }, stale: false },
    };
    const a = liveTreasury(snap);
    expect(a.incomeMonths()).toEqual({ state: "live", value: snap.months });
    expect(a.perHour()).toEqual({ state: "live", value: snap.perHour });
    expect(a.fx()).toEqual({ state: "live", value: snap.fx });
  });
});

describe("loadTreasury", () => {
  it("calls the typed bridge with the range and returns each part", async () => {
    const calls = {
      treasuryStreamsList: vi.fn().mockResolvedValue([stream]),
      treasuryIncomeMonth: vi.fn().mockResolvedValue([]),
      treasuryIncomeTrend: vi.fn().mockResolvedValue([]),
      treasuryPerHour: vi.fn().mockResolvedValue([]),
      treasuryFxStatus: vi.fn().mockResolvedValue({ rate: null, stale: false }),
    } as unknown as TreasuryCalls;
    const snap = await loadTreasury(range, calls);
    expect(snap.streams).toEqual([stream]);
    expect(calls.treasuryIncomeMonth).toHaveBeenCalledWith("2026-10", "2026-11");
    expect(calls.treasuryPerHour).toHaveBeenCalledWith("2026-11-01", "2026-11-30");
    expect(calls.treasuryIncomeTrend).toHaveBeenCalledWith(null);
  });

  it("one failed read leaves that part empty and never hides the others", async () => {
    const calls = {
      treasuryStreamsList: vi.fn().mockResolvedValue([stream]),
      treasuryIncomeMonth: vi.fn().mockRejectedValue("store_error"),
      treasuryIncomeTrend: vi.fn().mockResolvedValue([]),
      treasuryPerHour: vi.fn().mockResolvedValue([]),
      treasuryFxStatus: vi.fn().mockRejectedValue("ledger_invalid"),
    } as unknown as TreasuryCalls;
    const snap = await loadTreasury(range, calls);
    expect(snap).toEqual({ streams: [stream], months: [], trend: [], perHour: [], fx: null });
  });
});

describe("Treasury counts and sequences (phases 3 and 4)", () => {
  const stages = (lead: number | null) => ["lead", "waitlist", "first_purchase", "repeat", "lapsed"].map((stage, i) => ({ stage, count: i === 0 ? lead : null }));
  const counts = { asOf: "2026-11-18", stages: stages(12), segments: [{ stage: "lead", segment: "newsletter", count: 7 }], trend: [{ asOf: "2026-11-18", stages: stages(12) }] };
  const draft = { id: "seq-1", ventureId: "v1", kind: "reorder", emails: [{ subject: "S", body: "b {{unsubscribe_link}} {{mailing_address}}" }], checklist: ["Send only to contacts with recorded consent."], status: "draft", approvedAt: null, createdAt: "x", updatedAt: "x" };
  const row = (sends: number | null) => ({ sequenceId: "seq-1", day: "2026-11-02", sends, bounces: null, unsubscribes: null, purchasesAfter: null });

  it("counts are live only for a real import with a reported stage, and nulls stay null", () => {
    expect(liveTreasuryCounts(null)).toBe(UNWIRED);
    expect(liveTreasuryCounts({})).toBe(UNWIRED);
    expect(liveTreasuryCounts({ ...counts, stages: stages(null) })).toBe(UNWIRED);
    const w = liveTreasuryCounts(counts);
    expect(w).toEqual({ state: "live", value: counts });
    if (w.state === "live") expect(w.value.stages[1].count).toBeNull();
  });

  it("sequences and results are live only with real rows", () => {
    const none = liveTreasuryContacts(null, null);
    for (const k of ["counts", "sequences", "results"] as const) expect(none[k]()).toBe(UNWIRED);
    expect(liveTreasuryContacts(null, { drafts: [], results: [row(null)] }).results()).toBe(UNWIRED);
    const a = liveTreasuryContacts(counts, { drafts: [draft], results: [row(5)] });
    expect(a.sequences()).toEqual({ state: "live", value: [draft] });
    expect(a.results()).toEqual({ state: "live", value: [row(5)] });
    expect(a.counts().state).toBe("live");
  });

  it("loaders call the typed bridge and a failed read leaves that part empty", async () => {
    const cc = { treasuryCountsList: vi.fn().mockResolvedValue(counts) } as unknown as TreasuryCountsCalls;
    expect(await loadTreasuryCounts("str-1", cc)).toEqual(counts);
    expect(cc.treasuryCountsList).toHaveBeenCalledWith("str-1");
    const bad = { treasuryCountsList: vi.fn().mockRejectedValue("unknown_stream") } as unknown as TreasuryCountsCalls;
    expect(await loadTreasuryCounts("x", bad)).toBeNull();
    const sc = { treasurySequenceList: vi.fn().mockResolvedValue([draft]), treasuryResultsList: vi.fn().mockRejectedValue("store_error") } as unknown as TreasurySequenceCalls;
    expect(await loadTreasurySequences(sc)).toEqual({ drafts: [draft], results: [] });
    expect(sc.treasuryResultsList).toHaveBeenCalledWith(null);
  });
});
