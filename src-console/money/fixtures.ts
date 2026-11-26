import type { WireCurrencyView, WireMonth, WireMonthView } from "./entries";

/** Test data for the money page (not shipped in any UI path). Minor units: JPY yen. */
const m = (month: string, income: number, expense: number, savings: number, cats: [string, number][]): WireMonth => ({ month, has_data: true, income_minor: income, expense_minor: expense, savings_minor: savings, net_minor: income - expense, by_category: cats.map(([category, minor]) => ({ category, minor })), manual_income_minor: income, manual_expense_minor: expense, statement_income_minor: 0, statement_expense_minor: 0 });
const empty = (month: string): WireMonth => ({ ...m(month, 0, 0, 0, []), has_data: false, by_category: [] });

export const TODAY = "2026-11-12";
export const FULL: WireCurrencyView = {
  currency: "JPY",
  months: [empty("2026-06"), m("2026-07", 300000, 180000, 20000, [["food", 60000], ["housing", 80000]]), m("2026-08", 300000, 200000, 30000, [["food", 70000], ["housing", 80000]]), m("2026-09", 320000, 190000, 40000, [["food", 66000], ["housing", 80000]]), m("2026-10", 300000, 195000, 30000, [["food", 64000], ["housing", 80000], ["fun", 20000]]), m("2026-11", 150000, 90000, 10000, [["food", 40000], ["housing", 40000], ["fun", 10000]])],
  available: { amount_minor: 900000, base_minor: 800000, base_date: "2026-11-01", base_source: "manual", since_net_minor: 100000 },
  upcoming: [{ id: "b1", label: "Rent", amount_minor: 80000, due: "2026-11-30", days_until: 18, recurrence: "monthly" }],
  commitments: [], committed_monthly_minor: 40000,
  goal: { title: "Buffer", target_minor: 1000000, deadline: "2027-06-30", saved_minor: 400000, remaining_minor: 600000, pct: 40, per_month_needed_minor: 75000 },
  savings_series: [{ label: "2026-06", value_minor: 300000 }, { label: "2026-07", value_minor: 320000 }, { label: "2026-08", value_minor: 350000 }, { label: "2026-09", value_minor: 390000 }, { label: "2026-10", value_minor: 420000 }, { label: "2026-11", value_minor: 430000 }],
  balance_series: [{ label: "2026-10-01", value_minor: 700000 }, { label: "2026-11-01", value_minor: 800000 }],
  has_statement: false, statement_through: null, excluded_duplicates: 0,
};
export const PARTIAL: WireCurrencyView = { ...FULL, months: [empty("2026-08"), empty("2026-09"), m("2026-10", 300000, 195000, 30000, [["food", 64000]]), m("2026-11", 0, 20000, 0, [["food", 20000]])], goal: null, upcoming: [] };
export const NO_INCOME: WireCurrencyView = { ...FULL, months: FULL.months.map((x) => ({ ...x, income_minor: 0 })) };
export const VIEW: WireMonthView = { today: TODAY, months: FULL.months.map((x) => x.month), currencies: [FULL], fx: null, combined_usd: null, needs_conversion: true, skipped_lines: 0 };
