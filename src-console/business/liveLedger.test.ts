import { describe, expect, it } from "vitest";
import { NEEDS_CONVERSION, UNWIRED } from "../lib/sampleAdapters";
import { unwiredBusiness } from "./adapters";
import { ledgerAsOf, liveBusiness, usdFrom, type BusinessLedger } from "./liveLedger";

const base: BusinessLedger = {
  version: 1,
  fx: { usdPerJpy: 0.0066, asOf: "2026-11-11" },
  funds: [{ name: "Unallocated", balanceJpy: 150000 }],
  ventures: [{
    id: "taiyo-skin", name: "Taiyo Skin", stage: "Pre-launch", health: "at_risk", progress: { done: 5, total: 9, label: "checklist" },
    nextStep: "Label approval", note: "", targetJpy: 3_000_000,
    detail: { headline: "h", checklist: [], milestones: [], nextActions: [], budgetUsedJpy: 420_000, fundJpy: null, openVotes: 1 },
  }],
  fundingRequests: [{ id: "f1", requester: "Taiyo Skin", purpose: "First batch", amountJpy: 250_000, status: "open" }],
  taxFlags: [],
  voteRules: { needed: 1 },
  revenueTargetJpy: null,
};

describe("live Business adapter over the ledger", () => {
  it("no ledger file: exactly the unwired adapter", () => {
    expect(liveBusiness(null)).toBe(unwiredBusiness);
  });

  it("converts yen with the ledger's dated rate, never otherwise", () => {
    expect(usdFrom(150000, base.fx)).toEqual({ state: "live", value: 990 });
    expect(usdFrom(150000, null)).toEqual(NEEDS_CONVERSION);
    expect(usdFrom(null, base.fx)).toEqual(UNWIRED);
    expect(ledgerAsOf(base)).toBe("2026-11-11");
    expect(ledgerAsOf({ ...base, fx: null })).toBeNull();
  });

  it("serves ventures, detail, requests, rules and funds live; missing amounts stay unwired", () => {
    const a = liveBusiness(base);
    expect(a.ventures()).toMatchObject({ state: "live", value: [{ id: "taiyo-skin", target: { state: "live", value: 19800 } }] });
    expect(a.venture("taiyo-skin")).toMatchObject({ state: "live", value: { budgetUsedUsd: { state: "live", value: 2772 }, fundUsd: UNWIRED } });
    expect(a.venture("akiya")).toEqual(UNWIRED);
    expect(a.fundingRequests()).toMatchObject({ state: "live", value: [{ amountUsd: { state: "live", value: 1650 } }] });
    expect(a.voteRules()).toEqual({ state: "live", value: { needed: 1 } });
    expect(a.funds()).toEqual({ state: "live", value: [{ name: "Unallocated", balanceUsd: { state: "live", value: 990 } }] });
    expect(a.revenueTargetUsd()).toEqual(UNWIRED);
  });

  it("without a rate every money field stays NEEDS_CONVERSION, and no number is in yen form", () => {
    const a = liveBusiness({ ...base, fx: null });
    expect(a.funds()).toEqual({ state: "live", value: [{ name: "Unallocated", balanceUsd: NEEDS_CONVERSION }] });
    expect(a.ventures()).toMatchObject({ value: [{ target: NEEDS_CONVERSION }] });
    expect(JSON.stringify(a.venture("taiyo-skin"))).not.toMatch(/420000|¥/);
  });

  it("an empty funds list keeps the four known fund names, unconverted", () => {
    const a = liveBusiness({ ...base, funds: [] });
    expect(a.funds()).toMatchObject({ state: "live", value: [{ name: "Unallocated", balanceUsd: NEEDS_CONVERSION }, {}, {}, {}] });
  });
});
