import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { live } from "../lib/localStores";
import type { PersonalMoney } from "../personal/adapters";
import type { Fx } from "./adapters";
import { formatNative, monthName, type RawSummary } from "./overview";

/**
 * `personal.money` from `money_summary_read` + the stored FX rate (pure). Live only when this month's spend is real AND a stored
 * rate converts yen to dollars (the card is in USD); otherwise null and the card stays "Needs wiring". Bills due, left after bills,
 * subscriptions and categories have no source (a statement has no due dates or categories), so they stay unwired or empty:
 * recurring debits are in `money_summary_read` but are not called "subscriptions" (rent and loans recur too).
 */
export function personalMoneyFrom(s: RawSummary | null, fx: Fx | null): PersonalMoney | null {
  if (!s || s.currency !== "JPY" || s.current_spend == null || !Number.isFinite(s.current_spend) || !fx || !(fx.rate > 0)) return null;
  const spent: Wired<number> = live(s.current_spend / fx.rate);
  const usual = s.usual_monthly_spend != null ? ` · usual ${formatNative(s.usual_monthly_spend, "JPY")} a month` : "";
  return {
    month: monthName(s.current_month), spent,
    spentNote: `${formatNative(s.current_spend, "JPY")} so far this month, to ${s.as_of}${usual} · at ${fx.rate.toFixed(2)} JPY per USD (${fx.at})`,
    leftAfterBills: UNWIRED, subsMonthly: UNWIRED, subsNote: "", billsDue: UNWIRED, billsNote: "", payday: "",
    bills: [], subscriptions: [], moreSubs: "", categories: [], summary: "",
  };
}
