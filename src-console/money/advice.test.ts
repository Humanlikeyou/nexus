import { describe, expect, it } from "vitest";
import { NFA_LINE } from "./adapters";
import { adviceContext, buildAdvice, canAfford, type AdviceItem } from "./advice";
import { FULL, NO_INCOME, PARTIAL, TODAY } from "./fixtures";
import { mapFacts, checkMoneyText } from "./facts";
import { numbersIn } from "../amaraTake/validate";

const facts = (items: AdviceItem[]) => mapFacts({ day: TODAY, partial: false, partial_reasons: [], personal_accounts: 1, facts: items.flatMap((i) => i.numbers).flatMap((n, k) => numbersIn(n.shown).map((v, j) => ({ key: `${k}-${j}`, label: n.label, value: v, unit: "", as_of: null, source: n.source, stale: false }))) });

describe("money advice", () => {
  const r = buildAdvice(FULL, TODAY);
  it("computes the savings suggestion from medians of complete months", () => {
    expect(r.status).toBe("ok");
    const s = r.items.find((i) => i.id === "savings")!;
    // income median(300000,320000,300000)=300000, spend median(200000,190000,195000)=195000, committed 40000
    expect(s.claim).toContain("65,000 JPY");
    expect(s.numbers.find((n) => n.label === "Usual income")!.source).toMatch(/median of August, September, October income/);
    expect(s.suggestion).toBe(true);
  });
  it("covers runway, bills, categories and goal pace", () => {
    expect(r.items.map((i) => i.kind)).toEqual(["savings", "runway", "bills", "categories", "goal"]);
    expect(r.items.find((i) => i.id === "runway")!.claim).toContain("3.8 months");
    expect(r.items.find((i) => i.id === "bill-0")!.claim).toContain("26,667 JPY a week for 3 weeks");
    expect(r.items.find((i) => i.id === "categories")!.claim).toContain("Housing 40,000 JPY (usual 80,000 JPY)");
    expect(r.items.find((i) => i.id === "goal")!.claim).toContain("75,000 JPY a month is needed");
  });
  it("every claim has sourced numbers, assumptions, a confidence, and every number in the claim is listed", () => {
    for (const i of r.items) {
      expect(i.numbers.length).toBeGreaterThan(0);
      expect(i.assumptions.length).toBeGreaterThan(0);
      expect(["low", "medium", "high"]).toContain(i.confidence);
      for (const n of i.numbers) expect(n.source.length).toBeGreaterThan(0);
      const listed = i.numbers.flatMap((n) => numbersIn(n.shown));
      for (const x of numbersIn(i.claim)) expect(listed).toContain(x);
    }
  });
  it("passes checkMoneyText, with the not-financial-advice line, never a yen sign", () => {
    for (const i of r.items) {
      const text = `${i.claim} ${NFA_LINE}`;
      expect(checkMoneyText(text, facts(r.items), i.suggestion), i.id).toEqual({ ok: true });
      expect(text + JSON.stringify(i)).not.toMatch(/[¥￥]/);
    }
  });
  it("says what to enter, and advises nothing, with fewer than 2 complete months", () => {
    const p = buildAdvice(PARTIAL, TODAY);
    expect(p.status).toBe("needs_data");
    expect(p.items).toEqual([]);
    expect(p.needs.join(" ")).toMatch(/at least two full months \(1 so far in JPY\)/);
    expect(canAfford(PARTIAL, TODAY, 1000).status).toBe("needs_data");
    expect(adviceContext(p)).toMatch(/not enough recorded data/);
  });
  it("says to add income when none is recorded", () => {
    const p = buildAdvice(NO_INCOME, TODAY);
    expect(p.status).toBe("needs_data");
    expect(p.needs).toContain("Add your income in JPY.");
  });
  it("does not suggest saving when outgo is at or above income", () => {
    const tight = { ...FULL, committed_monthly_minor: 200000 };
    const s = buildAdvice(tight, TODAY).items.find((i) => i.id === "savings")!;
    expect(s.suggestion).toBe(false);
    expect(s.claim).toContain("no amount is left to set aside");
  });
  it("can-I-afford-it changes savings, buffer and goal date, with assumptions", () => {
    const a = canAfford(FULL, TODAY, 30000);
    expect(a.items.map((i) => i.id)).toEqual(["afford-savings", "afford-buffer", "afford-goal"]);
    expect(a.items[0].claim).toContain("from 65,000 JPY to 35,000 JPY");
    expect(a.items[1].claim).toContain("3.8 months to 3.7 months");
    for (const i of a.items) { expect(i.assumptions.length).toBeGreaterThan(0); expect(checkMoneyText(`${i.claim} ${NFA_LINE}`, facts(a.items), false), i.id).toEqual({ ok: true }); }
    expect(canAfford(FULL, TODAY, 0).status).toBe("needs_data");
  });
  it("is deterministic and never mixes currencies", () => {
    expect(buildAdvice(FULL, TODAY)).toEqual(buildAdvice(FULL, TODAY));
    expect(JSON.stringify(r)).not.toContain("USD");
  });
});
