import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";

/**
 * design-1118a Ventures › Trading: Amara's PROPOSALS from world news. Proposals only.
 *
 * Hard rules (HANDOFF-1118a §10, Taisun):
 * - Amara never places a trade, holds keys or talks to a broker. The ledger stays watch-only.
 * - Approve opens a second confirm sheet; Confirm only RECORDS his choice. There is no order path:
 *   this adapter has no method that sends, places, executes or submits anything.
 * - Not financial advice. Never invent prices or headlines: until the market feed, news feed and the
 *   choice store are wired (Needs wiring), the app shows a designed empty state; the sample adapter's
 *   fictional tickers are always labelled Sample data.
 */
export type ProposalSide = "buy" | "sell" | "watch";
export type ProposalKind = "crypto" | "stock";
export type MarketProposal = {
  id: string;
  side: ProposalSide;
  kind: ProposalKind;
  symbol: string;
  title: string;
  reason: string;
  source: string;
  confidence: "low" | "medium" | "high";
  risk: string;
};
export type ProposalChoice = { proposalId: string; choice: "approved" | "dismissed"; recordedAt: string };
export type MarketPoint = number;

export interface MarketProposalAdapter {
  proposals(): Wired<MarketProposal[]>;
  market(): Wired<MarketPoint[]>;
  newsNote(): Wired<string>;
  choices(): Wired<ProposalChoice[]>;
  /** Records his choice after the second confirm. Records only: no order is ever sent. */
  recordChoice(proposalId: string, choice: ProposalChoice["choice"]): Promise<Wired<ProposalChoice>>;
}

export const unwiredMarket: MarketProposalAdapter = {
  proposals: () => UNWIRED, market: () => UNWIRED, newsNote: () => UNWIRED, choices: () => UNWIRED, recordChoice: async () => UNWIRED,
};

export function makeSampleMarket(now: () => Date = () => new Date()): MarketProposalAdapter {
  const choices: ProposalChoice[] = [];
  const list: MarketProposal[] = [
    { id: "p-aurx", side: "buy", kind: "crypto", symbol: "AURX", title: "Buy AURX", confidence: "medium", source: "Sample news feed",
      reason: "A sample shipping-industry note is used here as the kind of world-news reason Amara would attach. This headline is not real.",
      risk: "Fits inside the daily loss cap, if a size were set later. No size is set." },
    { id: "p-hklo", side: "watch", kind: "stock", symbol: "HKLO", title: "Watch HKLO", confidence: "low", source: "Sample news feed",
      reason: "A sample regulatory brief is used here as the kind of source Amara would cite. This brief is not real.",
      risk: "Watch only. No order is proposed until you approve a later size." },
  ];
  return {
    proposals: () => sample(list.filter((p) => !choices.some((c) => c.proposalId === p.id && c.choice === "dismissed"))),
    market: () => sample([12, 14, 13, 16, 15, 18, 17, 19, 18, 21, 20, 22, 21, 23]),
    newsNote: () => sample("Amara would gather crypto and stock ideas from world news here. The news feed is not connected, so the two proposals below are sample data with fictional tickers."),
    choices: () => sample([...choices]),
    recordChoice: async (proposalId, choice) => {
      const c: ProposalChoice = { proposalId, choice, recordedAt: now().toISOString() };
      choices.push(c);
      return sample(c);
    },
  };
}

let sampleSingleton: MarketProposalAdapter | null = null;
export function marketAdapter(): MarketProposalAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleMarket());
  return unwiredMarket;
}
export function __resetMarketForTests() { sampleSingleton = null; }
