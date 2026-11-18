import { describe, expect, it, vi } from "vitest";
import { makeLiveMarket, mapProposal } from "./marketProposalsLive";

const raw = (o = {}) => ({ id: "wp-1", createdAt: "x", kind: "stock", side: "watch", ticker: "ABC", reasonText: "Why.", sourceName: "Src", confidence: "low", riskText: "Risk.", status: "proposed", ...o });

describe("live watch-only proposals", () => {
  it("maps well-formed rows and drops sample or malformed ones", () => {
    expect(mapProposal(raw())?.proposal.title).toBe("Watch ABC");
    expect(mapProposal(raw({ sample: true }))).toBeNull();
    expect(mapProposal(raw({ side: "send" }))).toBeNull();
    expect(mapProposal(raw({ ticker: undefined }))).toBeNull();
  });
  it("is unwired until the read lands; the feeds stay unwired; record and dismiss call the two commands only", async () => {
    let list = [raw(), raw({ id: "wp-2", ticker: "DEF" })];
    const calls: [string, unknown][] = [];
    const call = vi.fn(async (c: string, a?: Record<string, unknown>) => {
      calls.push([c, a]);
      if (c === "watch_proposals_list") return { proposals: list, disclaimer: "Not financial advice" };
      list = list.map((p) => (p.id === a?.id ? { ...p, status: c === "watch_proposal_record" ? "recorded" : "dismissed", decidedAt: "t" } : p));
      return { id: "watch-9" };
    }) as never;
    const a = makeLiveMarket({ call, notify: () => {} });
    expect(a.proposals().state).toBe("unwired");
    await vi.waitFor(() => expect(a.proposals().state).toBe("live"));
    expect(a.market().state).toBe("unwired");
    expect(a.newsNote().state).toBe("unwired");
    expect((await a.recordChoice("wp-1", "approved")).state).toBe("live");
    expect((await a.recordChoice("wp-2", "dismissed")).state).toBe("live");
    const names = calls.map(([c]) => c);
    expect(names).toContain("watch_proposal_record");
    expect(names).toContain("watch_proposal_dismiss");
    expect(new Set(names)).toEqual(new Set(["watch_proposals_list", "watch_proposal_record", "watch_proposal_dismiss"]));
    const c = a.choices();
    expect(c.state === "live" && c.value.map((x) => x.proposalId)).toEqual(["wp-1"]);
    const p = a.proposals();
    expect(p.state === "live" && p.value).toEqual([]);
  });
  it("a failed write is unwired and sends nothing else", async () => {
    const call = vi.fn(async (c: string) => { if (c === "watch_proposals_list") return { proposals: [raw()] }; throw new Error("x"); }) as never;
    const a = makeLiveMarket({ call, notify: () => {} });
    expect((await a.recordChoice("wp-1", "approved")).state).toBe("unwired");
  });
});
