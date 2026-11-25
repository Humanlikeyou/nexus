import { formatMinor, toMajor, type Currency, type WireCurrencyView, type WireMonth } from "./entries";

/**
 * Chart data for one currency at a time (JPY and USD are never summed here: the month view has no conversion).
 * Pure helpers. A month with nothing recorded is null (a gap), never 0. Values are major units (JPY yen, USD dollars).
 */

const SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/** "2026-10" -> "Oct"; a full day "2026-10-03" -> "Oct 3". Anything else is returned as is. */
export function shortLabel(s: string): string {
  const m = /^\d{4}-(\d{2})(?:-(\d{2}))?$/.exec(s);
  const name = m ? SHORT[Number(m[1]) - 1] : undefined;
  if (!m || !name) return s;
  return m[2] ? `${name} ${Number(m[2])}` : name;
}

export const median = (xs: readonly number[]): number | null => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

const major = (minor: number, c: Currency) => toMajor(minor, c);

export type IncomeExpenseData = { labels: string[]; months: string[]; income: (number | null)[]; expense: (number | null)[]; hasAny: boolean };
export function incomeVsExpenses(v: WireCurrencyView, months = 6): IncomeExpenseData {
  const ms = v.months.slice(-months);
  const pick = (f: (m: WireMonth) => number) => ms.map((m) => (m.has_data ? major(f(m), v.currency) : null));
  const income = pick((m) => m.income_minor);
  const expense = pick((m) => m.expense_minor);
  return { labels: ms.map((m) => shortLabel(m.month)), months: ms.map((m) => m.month), income, expense, hasAny: [...income, ...expense].some((x) => x != null) };
}

export type CategoryRow = { category: string; thisMonth: number; usual: number | null; thisShown: string; usualShown: string };
/** Top categories of the latest month against the median of up to three earlier months with data. `usual` is null with no earlier month. */
export function categoryCompare(v: WireCurrencyView, top = 6): { month: string | null; rows: CategoryRow[]; priorMonths: number } {
  const cur = v.months[v.months.length - 1];
  if (!cur || !cur.has_data) return { month: null, rows: [], priorMonths: 0 };
  const prior = v.months.slice(0, -1).filter((m) => m.has_data).slice(-3);
  const rows = [...cur.by_category].sort((a, b) => b.minor - a.minor).slice(0, top).map((c) => {
    const usualMinor = prior.length ? median(prior.map((m) => m.by_category.find((x) => x.category === c.category)?.minor ?? 0)) : null;
    return { category: c.category, thisMonth: major(c.minor, v.currency), usual: usualMinor == null ? null : major(usualMinor, v.currency), thisShown: formatMinor(c.minor, v.currency), usualShown: usualMinor == null ? "–" : formatMinor(Math.round(usualMinor), v.currency) };
  });
  return { month: cur.month, rows, priorMonths: prior.length };
}

export type SeriesData = { labels: string[]; values: number[]; shown: string[] };
const series = (pts: { label: string; value_minor: number }[], c: Currency): SeriesData => ({ labels: pts.map((p) => shortLabel(p.label)), values: pts.map((p) => major(p.value_minor, c)), shown: pts.map((p) => formatMinor(p.value_minor, c)) });
/** Cumulative savings by month plus the goal target (null without a goal). */
export function savingsOverTime(v: WireCurrencyView): SeriesData & { target: number | null; targetShown: string | null } {
  return { ...series(v.savings_series, v.currency), target: v.goal ? major(v.goal.target_minor, v.currency) : null, targetShown: v.goal ? formatMinor(v.goal.target_minor, v.currency) : null };
}
/** Balance entries you typed, oldest first. This is not a computed daily balance. */
export function balanceOverTime(v: WireCurrencyView): SeriesData {
  return series(v.balance_series, v.currency);
}

/** Short axis text: 1.5M, 500k, 8,000. Readouts and tables keep the full figure. */
export function compact(v: number): string {
  const a = Math.abs(v);
  if (a >= 1e6) return `${Number((v / 1e6).toFixed(1))}M`;
  if (a >= 1e4) return `${Math.round(v / 1e3)}k`;
  return Math.round(v * 100) % 100 === 0 ? Math.round(v).toLocaleString("en-US") : v.toLocaleString("en-US", { maximumFractionDigits: 2 });
}
