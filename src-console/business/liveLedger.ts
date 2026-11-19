import { NEEDS_CONVERSION, UNWIRED, type Wired } from "../lib/sampleAdapters";
import { LEDGER_FUNDS, unwiredBusiness, type BusinessAdapter, type Health } from "./adapters";

/** `read_business_ledger` (src-tauri/src/commands/business_ledger.rs, decision 0005). Amounts are yen. */
export interface BusinessLedger {
  version: 1;
  fx: { usdPerJpy: number; asOf: string } | null;
  funds: { name: string; balanceJpy: number }[];
  ventures: {
    id: string; name: string; stage: string; health: Health;
    progress: { done: number; total: number; label: string } | null;
    nextStep: string | null; note: string; targetJpy: number | null;
    /** Added by the write path; absent on older ledgers. */
    targetUsdCents?: number | null; currency?: "JPY" | "USD" | null; archived?: boolean;
    detail: {
      headline: string; checklist: { label: string; done: boolean }[];
      milestones: { label: string; due: string; health: Health }[];
      nextActions: { label: string; detail: string; action: string }[];
      budgetUsedJpy: number | null; fundJpy: number | null; openVotes: number;
    } | null;
  }[];
  fundingRequests: { id: string; requester: string; purpose: string; amountJpy: number | null; status: "open" | "approved" | "denied" }[];
  taxFlags: { id: string; label: string; area: string; date: string }[];
  voteRules: { needed: number } | null;
  revenueTargetJpy: number | null;
}

const live = <T,>(value: T): Wired<T> => ({ state: "live", value });

/** Yen to USD with the ledger's own dated rate. No rate: stays NEEDS_CONVERSION. No amount: unwired. Never invents a figure. */
export function usdFrom(jpy: number | null, fx: BusinessLedger["fx"]): Wired<number> {
  if (jpy === null || !Number.isFinite(jpy)) return UNWIRED;
  if (!fx) return NEEDS_CONVERSION;
  return live(Math.round(jpy * fx.usdPerJpy * 100) / 100);
}

/** The rate's as-of date, for the Money tab once Design shows it. Null when there is no rate. */
export const ledgerAsOf = (ledger: BusinessLedger | null): string | null => ledger?.fx?.asOf ?? null;

/** True only for a well-formed v1 ledger object (the IPC reply is untrusted: a mock or an old build can return [] or junk). */
export function isLedger(x: unknown): x is BusinessLedger {
  if (!x || typeof x !== "object" || Array.isArray(x)) return false;
  const l = x as Record<string, unknown>;
  return l.version === 1 && ["funds", "ventures", "fundingRequests", "taxFlags"].every((k) => Array.isArray(l[k]));
}

/** Live Business adapter over a loaded ledger. No (or malformed) ledger: the unwired adapter, unchanged. */
export function liveBusiness(ledger: unknown): BusinessAdapter {
  if (!isLedger(ledger)) return unwiredBusiness;
  const { fx } = ledger;
  return {
    // Archived ventures stay in the file and in History but are not listed.
    ventures: () => live(ledger.ventures.filter((v) => !v.archived).map((v) => ({
      id: v.id, name: v.name, stage: v.stage, health: v.health, progress: v.progress,
      nextStep: v.nextStep, note: v.note,
      target: v.currency === "USD" && v.targetUsdCents != null ? live(v.targetUsdCents / 100) : usdFrom(v.targetJpy, fx),
      currency: v.currency === "USD" ? "USD" as const : "JPY" as const,
      targetMinor: v.currency === "USD" ? v.targetUsdCents ?? null : v.targetJpy,
    }))),
    venture: (id) => {
      const d = ledger.ventures.find((v) => v.id === id)?.detail;
      if (!d) return UNWIRED;
      return live({
        headline: d.headline, checklist: d.checklist, milestones: d.milestones, nextActions: d.nextActions,
        budgetUsedUsd: usdFrom(d.budgetUsedJpy, fx), fundUsd: usdFrom(d.fundJpy, fx), openVotes: d.openVotes,
      });
    },
    fundingRequests: () => live(ledger.fundingRequests.map((r) => ({
      id: r.id, requester: r.requester, purpose: r.purpose, amountUsd: usdFrom(r.amountJpy, fx), status: r.status,
    }))),
    voteRules: () => (ledger.voteRules ? live(ledger.voteRules) : UNWIRED),
    taxFlags: () => live(ledger.taxFlags),
    funds: () => live(ledger.funds.length
      ? ledger.funds.map((f) => ({ name: f.name, balanceUsd: usdFrom(f.balanceJpy, fx) }))
      : LEDGER_FUNDS.map((name) => ({ name, balanceUsd: NEEDS_CONVERSION }))),
    revenueTargetUsd: () => usdFrom(ledger.revenueTargetJpy, fx),
  };
}
