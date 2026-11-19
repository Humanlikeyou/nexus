import { describe, expect, it } from "vitest";
import { draftFrom, formatMinor, parseAmount } from "./entries";
import { dashboardTiles } from "./MoneyEntriesPanel";

const base = { kind: "expense", amount: "1,200", currency: "JPY", date: "2026-11-10", due: "", recurrence: "none", category: "food", note: " lunch " } as const;

describe("money entries logic", () => {
  it("formats without a currency sign and shows a dash for missing", () => {
    expect(formatMinor(612000, "JPY")).toBe("612,000 JPY");
    expect(formatMinor(123450, "USD")).toBe("1,234.50 USD");
    expect(formatMinor(null, "JPY")).toBe("–");
    expect(formatMinor(5, "JPY")).not.toMatch(/[¥￥]/);
  });
  it("parses amounts strictly", () => {
    expect(parseAmount("1,200", "JPY")).toBe(1200);
    expect(parseAmount("12.5", "USD")).toBe(1250);
    expect(parseAmount("12.5", "JPY")).toBeNull();
    expect(parseAmount("0", "JPY")).toBeNull();
    expect(parseAmount("0", "JPY", true)).toBe(0);
    expect(parseAmount("-3", "JPY")).toBeNull();
  });
  it("validates the form", () => {
    const ok = draftFrom({ ...base }, "2026-11-19");
    expect(ok.ok && ok.draft).toMatchObject({ amount_minor: 1200, category: "food", note: "lunch" });
    expect(draftFrom({ ...base, date: "2026-12-01" }, "2026-11-19").ok).toBe(false);
    expect(draftFrom({ ...base, kind: "bill" }, "2026-11-19").ok).toBe(false);
    expect(draftFrom({ ...base, kind: "bill", due: "2026-12-01", recurrence: "monthly" }, "2026-11-19").ok).toBe(true);
  });
  it("dashboard tiles show dashes, never 0, with no data", () => {
    const t = dashboardTiles({ currency: "JPY", months: [], available: null, upcoming: [], commitments: [], committed_monthly_minor: 0, goal: null, savings_series: [], balance_series: [], has_statement: false, statement_through: null, excluded_duplicates: 0 });
    expect(t.map((x) => x.v)).toEqual(["–", "–", "–", "–", "–", "–"]);
  });
});
