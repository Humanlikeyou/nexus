import { useEffect, useState } from "react";
import { announceSnapshot, inTauri, realInvoke, type Invoke } from "../lib/localStores";

/**
 * Enterprise write paths (decision 0045 item 8). Ventures and the yen rate go to business_ledger.rs; a typed
 * income or cost goes to Treasury (treasury_entries.rs). Money is always integer minor units (yen, or US cents)
 * and a yen amount is never shown with a currency sign. Nothing here moves money or contacts anyone.
 */
export type Currency = "JPY" | "USD";
export const STAGES = ["Idea", "Building", "Pre-launch", "Live", "Paused"] as const;

export interface VentureDraft { id: string | null; name: string; stage: string; oneLiner: string; nextStep: string | null; monthlyTargetMinor: number | null; currency: Currency }
export interface LedgerChange { id: string; at: string; action: string; subject: string; label: string; undone: boolean }
export interface VentureMonth { ventureId: string; currency: string; month: string; incomeMinor: number; costMinor: number; rows: number }
export interface MoneyEntry { id: string; ventureId: string; date: string; amountMinor: number; currency: string }

/** The backend's plain-language error, or a calm default. Never quotes ledger contents. */
export const writeError = (e: unknown): string => {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : "";
  const m = raw.replace(/^ledger:\s*/, "");
  if (raw.startsWith("ledger:") && m) return `${m.charAt(0).toUpperCase()}${m.slice(1)}${/[.!?]$/.test(m) ? "" : "."}`;
  if (raw === "unknown_venture") return "That venture isn't in the ledger. Add it first.";
  if (raw === "invalid_input") return "Check the date and amount, then try again.";
  return "That didn't save. Nothing was changed.";
};

/** Parses "12,000", "12000" or "12.50" into minor units. null when it isn't a positive amount. */
export function parseMinor(text: string, currency: Currency): number | null {
  const t = text.replace(/[,\s]/g, "");
  if (!/^\d+(\.\d{1,2})?$/.test(t)) return null;
  const n = Number(t);
  if (!Number.isFinite(n)) return null;
  if (currency === "JPY") return Number.isInteger(n) ? n : null;
  return Math.round(n * 100);
}

/** "612,000 JPY" or "USD 12.50" style, never a yen sign. */
export const amountText = (minor: number, currency: string): string => {
  const v = currency === "JPY" ? minor : minor / 100;
  const s = v.toLocaleString("en-US", { maximumFractionDigits: currency === "JPY" ? 0 : 2, minimumFractionDigits: currency === "JPY" ? 0 : 2 });
  return currency === "JPY" ? `${s} JPY` : `${s} ${currency}`;
};

/** Today and this month in JST. */
export const jstDay = (d = new Date()): string => d.toLocaleDateString("en-CA", { timeZone: "Asia/Tokyo" });
export const jstMonth = (d = new Date()): string => jstDay(d).slice(0, 7);

export interface LedgerWrites {
  saveVenture(d: VentureDraft): Promise<LedgerChange>;
  archiveVenture(id: string, archived: boolean): Promise<LedgerChange>;
  setRate(jpyPerUsd: number, asOf: string): Promise<LedgerChange>;
  undoChange(id: string): Promise<boolean>;
  history(): Promise<LedgerChange[]>;
  addEntry(e: { ventureId: string; date: string; amountMinor: number; currency: Currency; kind: "income" | "cost"; note: string | null }): Promise<MoneyEntry>;
  undoEntry(id: string): Promise<boolean>;
  ventureMonth(month: string): Promise<VentureMonth[]>;
}

export function makeLedgerWrites(call: Invoke = realInvoke, notify: () => void = announceSnapshot): LedgerWrites {
  const after = async <T,>(p: Promise<T>): Promise<T> => { const r = await p; notify(); return r; };
  return {
    saveVenture: (draft) => after(call<LedgerChange>("ledger_venture_save", { draft })),
    archiveVenture: (id, archived) => after(call<LedgerChange>("ledger_venture_archive", { id, archived })),
    setRate: (jpyPerUsd, asOf) => after(call<LedgerChange>("ledger_fx_set", { jpyPerUsd, asOf })),
    undoChange: (id) => after(call<boolean>("ledger_undo", { id })),
    history: () => call<LedgerChange[]>("ledger_history_list"),
    addEntry: (e) => after(call<MoneyEntry>("treasury_entry_add", e)),
    undoEntry: (id) => after(call<boolean>("treasury_entry_undo", { id })),
    ventureMonth: (month) => call<VentureMonth[]>("treasury_venture_month", { month }),
  };
}

let shared: LedgerWrites | null = null;
/** Null outside the app (browser, unit tests): forms then say saving needs the desktop app instead of pretending. */
export const ledgerWrites = (): LedgerWrites | null => (inTauri() ? (shared ??= makeLedgerWrites()) : null);

/** This month's income and costs per venture. `null` until loaded or when Treasury can't be read; refresh on snapshot events. */
export function useVentureMonth(writes: LedgerWrites | null = ledgerWrites(), month = jstMonth(), tick = 0): VentureMonth[] | null {
  const [rows, setRows] = useState<VentureMonth[] | null>(null);
  useEffect(() => {
    if (!writes) return;
    let live = true;
    void writes.ventureMonth(month).then((r) => { if (live) setRows(r); }).catch(() => { if (live) setRows(null); });
    return () => { live = false; };
  }, [writes, month, tick]);
  return rows;
}
