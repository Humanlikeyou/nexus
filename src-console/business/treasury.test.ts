import { describe, expect, it, vi } from "vitest";
import { UNWIRED } from "../lib/sampleAdapters";
import { isTreasurySnapshot, liveTreasury, loadTreasury, unwiredTreasury, type TreasuryCalls, type TreasurySnapshot } from "./treasury";

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
