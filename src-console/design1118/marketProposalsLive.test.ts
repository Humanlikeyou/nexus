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
    // An approval also re-reads the watchlist (the pick is added in the same write).
    expect(new Set(names)).toEqual(new Set(["watch_proposals_list", "watch_proposal_record", "watch_proposal_dismiss", "watchlist_get"]));
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

  it("the watchlist reads lazily, keeps null prices null, and drop/undo/research call only their commands", async () => {
    const wlRaw = {
      picks: [
        { pick: { ticker: "NVDA", kind: "stock", reasonText: "r", sourceName: "s", sourceUrl: "https://a.example.com/1", pickedAt: "p", nextReviewAt: "n" }, quote: { price: null, currency: null, fetchedAt: null, status: "stale", error: null } },
        { pick: { ticker: "BAD", kind: "option" }, quote: { status: "open" } },
      ],
      feed: { state: "connected", lastAttemptAt: null, lastOkAt: null, pollMinutes: 15 },
      research: { running: false, lastOutcome: null, lastFinishedAt: null, lastProposed: 0, nextAllowedAt: null },
      reviewDue: true, disclaimer: "Not financial advice",
    };
    const calls: string[] = [];
    const call = vi.fn(async (c: string) => {
      calls.push(c);
      if (c === "watchlist_get") return wlRaw;
      if (c === "watchlist_drop") return { id: "watch-3", ticker: "NVDA", toast: "x" };
      if (c === "watchlist_research_now") return { state: "rate_limited", retryAfterMinutes: 12 };
      if (c === "add_undo") return true;
      return { proposals: [] };
    }) as never;
    const a = makeLiveMarket({ call, notify: () => {} });
    expect(calls).toEqual([]);
    expect(a.watchlist().state).toBe("unwired");
    await vi.waitFor(() => expect(a.watchlist().state).toBe("live"));
    const w = a.watchlist();
    expect(w.state === "live" && w.value.picks.map((p) => [p.ticker, p.quote.price, p.quote.status])).toEqual([["NVDA", null, "stale"]]);
    expect(w.state === "live" && w.value.reviewDue).toBe(true);
    const r = await a.researchNow();
    expect(r.state === "live" && r.value).toEqual({ state: "rate_limited", retryAfterMinutes: 12 });
    const d = await a.dropPick("NVDA");
    expect(d.state === "live" && d.value.historyId).toBe("watch-3");
    expect(await a.undoWatch("watch-3")).toBe(true);
    expect(new Set(calls)).toEqual(new Set(["watchlist_get", "watchlist_research_now", "watchlist_drop", "add_undo", "watch_proposals_list"]));
  });
});
