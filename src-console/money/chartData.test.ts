import { describe, expect, it } from "vitest";
import { balanceOverTime, compact, categoryCompare, incomeVsExpenses, median, savingsOverTime, shortLabel } from "./chartData";
import { FULL } from "./fixtures";

describe("chart data", () => {
  it("labels months and takes medians", () => {
    expect(shortLabel("2026-10")).toBe("Oct");
    expect(shortLabel("2026-10-03")).toBe("Oct 3");
    expect(median([3, 1, 2])).toBe(2);
    expect(median([1, 2, 3, 4])).toBe(2.5);
    expect(median([])).toBeNull();
  });
  it("income vs expenses: empty months are gaps, not zero", () => {
    const d = incomeVsExpenses(FULL);
    expect(d.income[0]).toBeNull();
    expect(d.expense[1]).toBe(180000);
    expect(d.labels).toHaveLength(6);
    expect(d.hasAny).toBe(true);
  });
  it("categories: this month against the median of the earlier months", () => {
    const c = categoryCompare(FULL);
    expect(c.month).toBe("2026-11");
    expect(c.priorMonths).toBe(3);
    const food = c.rows.find((r) => r.category === "food")!;
    expect(food.thisMonth).toBe(40000);
    expect(food.usual).toBe(66000);
    expect(c.rows.find((r) => r.category === "fun")!.usual).toBe(0);
  });
  it("USD values are in dollars, and series carry their formatted text", () => {
    const usd = { ...FULL, currency: "USD" as const, savings_series: [{ label: "2026-10", value_minor: 123450 }, { label: "2026-11", value_minor: 150000 }] };
    const s = savingsOverTime(usd);
    expect(s.values).toEqual([1234.5, 1500]);
    expect(s.shown[0]).toBe("1,234.50 USD");
    expect(s.target).toBe(10000);
    expect(balanceOverTime(FULL).shown).toEqual(["700,000 JPY", "800,000 JPY"]);
  });
  it("compact axis text", () => { expect(compact(1000000)).toBe("1M"); expect(compact(1500000)).toBe("1.5M"); expect(compact(500000)).toBe("500k"); expect(compact(8000)).toBe("8,000"); expect(compact(0)).toBe("0"); });
  it("no goal means no target", () => { expect(savingsOverTime({ ...FULL, goal: null }).target).toBeNull(); });
});
