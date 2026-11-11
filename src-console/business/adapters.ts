import { NEEDS_CONVERSION, UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { liveBusiness, type BusinessLedger } from "./liveLedger";

export type Health = "on_track" | "at_risk" | "off_track" | "done" | "waiting" | "searching";
export interface Venture { id: string; name: string; stage: string; health: Health; progress: { done: number; total: number; label: string } | null; nextStep: string | null; note: string; target: Wired<number> }
export interface VentureDetail { headline: string; checklist: { label: string; done: boolean }[]; milestones: { label: string; due: string; health: Health }[]; nextActions: { label: string; detail: string; action: string }[]; budgetUsedUsd: Wired<number>; fundUsd: Wired<number>; openVotes: number }
export interface FundingRequest { id: string; requester: string; purpose: string; amountUsd: Wired<number>; status: "open" | "approved" | "denied" }
export interface TaxFlag { id: string; label: string; area: string; date: string }
export interface Fund { name: string; balanceUsd: Wired<number> }

/** Claude wires these (HANDOFF-1110d.md). Ledger amounts are yen today, so USD values need conversion. */
export interface BusinessAdapter {
  ventures(): Wired<Venture[]>;
  venture(id: string): Wired<VentureDetail>;
  fundingRequests(): Wired<FundingRequest[]>;
  voteRules(): Wired<{ needed: number }>;
  taxFlags(): Wired<TaxFlag[]>;
  funds(): Wired<Fund[]>;
  revenueTargetUsd(): Wired<number>;
}

export const LEDGER_FUNDS = ["Unallocated", "Taiyo Skin", "Akiya", "Other projects"] as const;

export const unwiredBusiness: BusinessAdapter = {
  ventures: () => UNWIRED, venture: () => UNWIRED, fundingRequests: () => UNWIRED, voteRules: () => UNWIRED,
  taxFlags: () => UNWIRED, funds: () => ({ state: "live", value: LEDGER_FUNDS.map((name) => ({ name, balanceUsd: NEEDS_CONVERSION })) }), revenueTargetUsd: () => UNWIRED,
};

/** Preview/test only: the design-1110d mockup's sample values (no yen, no invented USD). */
export const sampleBusiness: BusinessAdapter = {
  ...unwiredBusiness,
  ventures: () => sample([
    { id: "taiyo-skin", name: "Taiyo Skin", stage: "Pre-launch", health: "at_risk", progress: { done: 5, total: 9, label: "launch tasks done" }, nextStep: "label approval", note: "", target: UNWIRED },
    { id: "akiya", name: "Akiya", stage: "House search", health: "searching", progress: null, nextStep: null, note: "0 shortlisted", target: NEEDS_CONVERSION },
  ]),
  venture: (id) => (id !== "taiyo-skin" ? UNWIRED : sample({
    headline: "Label approval is 6 days late; launch date holds if it clears this week.",
    checklist: [["Brand name and logo", true], ["Formula sample approved", true], ["Supplier chosen", true], ["Store domain", true], ["Product photos", true], ["Label and ingredient approval · late", false], ["First batch funded", false], ["Payments test order", false], ["Launch post", false]].map(([label, done]) => ({ label: label as string, done: done as boolean })),
    milestones: [{ label: "Samples ready", due: "Oct 15", health: "done" }, { label: "Labels cleared", due: "Nov 4 → Nov 13", health: "at_risk" }, { label: "First batch", due: "Nov 25", health: "waiting" }, { label: "Launch", due: "Dec 8", health: "on_track" }],
    nextActions: [{ label: "Chase label approval with the supplier", detail: "Taiyo Skin crew can draft the email · asks before sending", action: "Draft with crew" }, { label: "Vote on the first-batch funding request", detail: "Amount and vote rules need wiring", action: "Open vote" }],
    budgetUsedUsd: NEEDS_CONVERSION, fundUsd: NEEDS_CONVERSION, openVotes: 1,
  })),
  fundingRequests: () => sample([
    { id: "f1", requester: "Taiyo Skin", purpose: "First batch (500 units)", amountUsd: NEEDS_CONVERSION, status: "open" },
    { id: "f2", requester: "Akiya", purpose: "Survey of 2 listings", amountUsd: NEEDS_CONVERSION, status: "approved" },
  ]),
  taxFlags: () => sample([{ id: "t1", label: "Overseas supplier invoice has no tax line", area: "Costs", date: "Oct 21" }]),
};

/** Live when a ledger file has loaded into the snapshot (decision 0005); otherwise unwired. */
export const businessAdapter = (data?: { ledger?: BusinessLedger | null }): BusinessAdapter => (sampleAdaptersEnabled() ? sampleBusiness : liveBusiness(data?.ledger ?? null));
