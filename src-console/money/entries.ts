import { realInvoke, type Invoke } from "../lib/localStores";

/**
 * Personal money entries (decision 0045 item 4). Logic only: wire types for `money_entry_*`, `money_entries_list`
 * and `money_month_view` (src-tauri/src/commands/money_entries.rs), amount parsing and formatting. Amounts are
 * minor units (JPY: yen, USD: cents). A missing value is "–", never 0, and a currency sign is never printed.
 */

export type Currency = "JPY" | "USD";
export type EntryKind = "balance" | "income" | "expense" | "savings" | "bill" | "commitment";
export type Recurrence = "none" | "monthly" | "weekly" | "yearly";

export const CATEGORIES = ["housing", "food", "transport", "utilities", "health", "fun", "shopping", "subscriptions", "debt", "gifts", "travel", "other"] as const;
export type Category = (typeof CATEGORIES)[number];
export const CATEGORY_LABEL: Record<string, string> = { housing: "Housing", food: "Food", transport: "Transport", utilities: "Utilities", health: "Health", fun: "Fun", shopping: "Shopping", subscriptions: "Subscriptions", debt: "Debt", gifts: "Gifts", travel: "Travel", other: "Other", uncategorised: "Uncategorised" };
export const KIND_LABEL: Record<EntryKind, string> = { balance: "Money I have now", income: "Income", expense: "Expense", savings: "Savings", bill: "Bill", commitment: "Commitment" };

export type EntryDraft = { kind: EntryKind; account_id?: string | null; amount_minor: number; currency: Currency; date: string; due?: string | null; recurrence?: Recurrence | null; category?: Category | null; note?: string | null; source?: "app" | "chat" | "telegram" };
export type WireEntry = { id: string; kind: EntryKind; account_id?: string | null; amount_minor: number; currency: Currency; date: string; due?: string | null; recurrence?: Recurrence | null; category?: string | null; note?: string | null; source: string; created_at: string; voided_by?: string | null };
export type WireEntries = { entries: WireEntry[]; skipped_lines: number };

export type WireMonth = { month: string; has_data: boolean; income_minor: number; expense_minor: number; savings_minor: number; net_minor: number; by_category: { category: string; minor: number }[]; manual_income_minor: number; manual_expense_minor: number; statement_income_minor: number; statement_expense_minor: number };
export type WireAvailable = { amount_minor: number; base_minor: number; base_date: string; base_source: string; since_net_minor: number };
export type WireUpcoming = { id: string; label: string; amount_minor: number; due: string; days_until: number; recurrence: string };
export type WireCommitment = { id: string; label: string; amount_minor: number; recurrence: string; monthly_minor: number };
export type WireGoal = { title: string; target_minor: number; deadline: string | null; saved_minor: number; remaining_minor: number; pct: number; per_month_needed_minor: number | null };
export type WireSeries = { label: string; value_minor: number };
export type WireCurrencyView = { currency: Currency; months: WireMonth[]; available: WireAvailable | null; upcoming: WireUpcoming[]; commitments: WireCommitment[]; committed_monthly_minor: number; goal: WireGoal | null; savings_series: WireSeries[]; balance_series: WireSeries[]; has_statement: boolean; statement_through: string | null; excluded_duplicates: number };
export type WireMonthView = { today: string; months: string[]; currencies: WireCurrencyView[]; fx: { rate: number; date: string; source: string } | null; combined_usd: { rate: number; rate_date: string; rate_source: string; income_cents: number; expense_cents: number; savings_cents: number } | null; needs_conversion: boolean; skipped_lines: number };

export const decimals = (c: Currency): number => (c === "USD" ? 2 : 0);
export const toMajor = (minor: number, c: Currency): number => minor / 10 ** decimals(c);

/** "612,000 JPY" / "1,234.50 USD"; "–" for a missing value. Never a currency sign. */
export function formatMinor(minor: number | null | undefined, c: Currency): string {
  if (minor == null || !Number.isFinite(minor)) return "–";
  const d = decimals(c);
  const abs = Math.abs(minor);
  const whole = Math.floor(abs / 10 ** d).toLocaleString("en-US");
  const frac = d ? `.${String(abs % 10 ** d).padStart(d, "0")}` : "";
  return `${minor < 0 ? "-" : ""}${whole}${frac} ${c}`;
}

/** Compact axis/readout text for charts (major units, no unit). */
export function formatMajor(v: number, c: Currency): string {
  return c === "USD" ? v.toLocaleString("en-US", { minimumFractionDigits: 0, maximumFractionDigits: 2 }) : Math.round(v).toLocaleString("en-US");
}

/** Text from a field to minor units. JPY: whole yen. USD: up to two decimals. Positive; zero only when `allowZero`. Anything else is null. */
export function parseAmount(raw: string, c: Currency, allowZero = false): number | null {
  const t = raw.trim().replace(/[,\s]/g, "");
  if (!/^\d+(\.\d+)?$/.test(t)) return null;
  const [w, f = ""] = t.split(".");
  if (f.length > decimals(c) || (decimals(c) === 0 && f.length > 0)) return null;
  const minor = Number(w) * 10 ** decimals(c) + (f ? Number(f.padEnd(decimals(c), "0")) : 0);
  if (!Number.isSafeInteger(minor) || minor >= 10_000_000_000_000) return null;
  return minor === 0 && !allowZero ? null : minor;
}

export const isDay = (s: string): boolean => /^\d{4}-\d{2}-\d{2}$/.test(s) && !Number.isNaN(Date.parse(`${s}T00:00:00Z`)) && new Date(`${s}T00:00:00Z`).toISOString().slice(0, 10) === s;

export type FormInput = { kind: EntryKind; amount: string; currency: Currency; date: string; due: string; recurrence: Recurrence; category: Category | ""; note: string };
export type FormResult = { ok: true; draft: EntryDraft } | { ok: false; error: string };

/** Form state to a draft, or the one thing that's wrong in plain words. `today` is YYYY-MM-DD. */
export function draftFrom(f: FormInput, today: string): FormResult {
  const amount = parseAmount(f.amount, f.currency, f.kind === "balance");
  if (amount == null) return { ok: false, error: f.currency === "JPY" ? "Enter a whole number of yen." : "Enter an amount with up to two decimals." };
  if (!isDay(f.date)) return { ok: false, error: "Enter a date as YYYY-MM-DD." };
  const past = f.kind === "balance" || f.kind === "income" || f.kind === "expense" || f.kind === "savings";
  if (past && f.date > today) return { ok: false, error: "That date is in the future. Entries record what has happened." };
  const wantsDue = f.kind === "bill" || f.kind === "commitment";
  if (f.kind === "bill" && !isDay(f.due)) return { ok: false, error: "A bill needs a due date (YYYY-MM-DD)." };
  if (f.kind === "commitment" && f.recurrence === "none" && !isDay(f.due)) return { ok: false, error: "A one-off commitment needs a due date." };
  if (wantsDue && f.due && !isDay(f.due)) return { ok: false, error: "Enter the due date as YYYY-MM-DD." };
  return {
    ok: true,
    draft: {
      kind: f.kind, amount_minor: amount, currency: f.currency, date: f.date,
      due: wantsDue && f.due ? f.due : null,
      recurrence: wantsDue ? f.recurrence : null,
      category: f.kind === "expense" && f.category ? f.category : null,
      note: f.note.trim() ? f.note.trim().slice(0, 200) : null,
      source: "app",
    },
  };
}

const ERRORS: Record<string, string> = {
  invalid_input: "That entry isn't valid. Nothing was saved.",
  not_found: "That entry wasn't found. Nothing changed.",
  already_voided: "That entry was already undone.",
  store_full: "The entries file is full. Nothing was saved.",
  store_error: "The money store couldn't be written. Nothing was saved.",
};
/** Fixed text per backend error code; nothing from the error itself is shown. */
export const entryError = (e: unknown): string => (typeof e === "string" && ERRORS[e]) || "That didn't go through. Nothing was saved.";

export type MoneyEntries = {
  add(d: EntryDraft): Promise<WireEntry>;
  undo(id: string): Promise<void>;
  list(): Promise<WireEntries>;
  monthView(months?: number): Promise<WireMonthView>;
  setSavingsGoal(currency: Currency, targetMinor: number, deadline: string | null): Promise<void>;
  setRate(rate: number, date: string): Promise<void>;
};

export function makeMoneyEntries(call: Invoke = realInvoke): MoneyEntries {
  return {
    add: (draft) => call<WireEntry>("money_entry_add", { draft }),
    undo: async (id) => { await call("money_entry_void", { id, replacement: null }); },
    list: () => call<WireEntries>("money_entries_list", { range: { from: null, to: null, limit: 40 } }),
    monthView: (months = 6) => call<WireMonthView>("money_month_view", { months }),
    setSavingsGoal: async (currency, targetMinor, deadline) => { await call("money_savings_goal_set", { currency, targetMinor, deadline }); },
    setRate: async (rate, date) => { await call("fx_rate_set_manual", { rate, date }); },
  };
}
