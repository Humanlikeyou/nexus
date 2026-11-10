import { afterEach, describe, expect, it } from "vitest";
import { crewsAdapter, unwiredCrews } from "../crews/adapters";
import { businessAdapter, unwiredBusiness } from "../business/adapters";
import { SAMPLE_ADAPTERS_KEY, sampleAdaptersEnabled } from "./sampleAdapters";

describe("design-1110d adapters", () => {
  afterEach(() => localStorage.removeItem(SAMPLE_ADAPTERS_KEY));
  it("are unwired by default: the app never shows sample values unless a preview opts in", () => {
    expect(sampleAdaptersEnabled()).toBe(false);
    expect(crewsAdapter()).toBe(unwiredCrews);
    expect(businessAdapter()).toBe(unwiredBusiness);
    expect(unwiredCrews.approvals().state).toBe("unwired");
    expect(unwiredBusiness.funds()).toMatchObject({ state: "live", value: [{ name: "Unallocated", balanceUsd: { state: "unwired", reason: "currency" } }, {}, {}, {}] });
  });
  it("opt-in sample adapters return sample-tagged values with no yen and no USD for ledger amounts", () => {
    localStorage.setItem(SAMPLE_ADAPTERS_KEY, "1");
    const b = businessAdapter();
    expect(b.ventures().state).toBe("sample");
    const votes = b.fundingRequests();
    expect(votes.state === "sample" && votes.value.every((v) => v.amountUsd.state === "unwired")).toBe(true);
    expect(JSON.stringify([b.ventures(), b.venture("taiyo-skin"), votes, crewsAdapter().approvals()])).not.toMatch(/¥|JPY/);
  });
});
