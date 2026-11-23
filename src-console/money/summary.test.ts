import { describe, expect, it } from "vitest";
import { MONEY_BANNED, NFA_LINE } from "./adapters";
import { checkMoneyText, mapFacts, moneyTakeGate, type MoneyFactsWire } from "./facts";
import { makeLiveMoney } from "./live";
import { buildOverview, type RawSummary } from "./overview";
import { personalMoneyFrom } from "./personalMoney";

const flush = () => new Promise((r) => setTimeout(r, 0));
const acct = { id: "a1", nickname: "Bank", institution: "smbc", kind: "ordinary", route: "csv", currency: "JPY", scope: "personal", createdAt: "2026-11-01T09:00:00+09:00" };
const bal = { account_id: "a1", currency: "JPY", amount: 745500, as_of: "2026-11-05", source: "statement", history: [] };
const fxWire = { rate: 150, date: "2026-11-12", source: "manual" };
const summary: RawSummary = {
  currency: "JPY", as_of: "2026-11-05", months_of_data: 4, single_account: true, account_id: "a1", current_month: "2026-11", current_spend: 10000,
  usual_monthly_spend: 81500, buffer_months: 9.1, savings_rate: { month: "2026-10", pct: 66.2, prev_month: "2026-09", prev_pct: 72.8 }, recurring: [],
};
const facts = (over: Partial<MoneyFactsWire> = {}): MoneyFactsWire => ({
  day: "2026-11-14", partial: false, partial_reasons: [], personal_accounts: 1,
  facts: [
    { key: "balance_a1", label: "Bank", value: 745500, unit: "JPY", as_of: "2026-11-05", source: "balance:a1:statement", stale: false },
    { key: "buffer_months", label: "Buffer", value: 9.1, unit: "months", as_of: "2026-11-05", source: "statement:a1", stale: false },
  ], ...over,
});

describe("overview from the summary", () => {
  it("fills buffer and savings rate only from the summary, never invents them", () => {
    const o = buildOverview([acct], [bal], null, summary)!;
    expect(o.bufferMonths).toBe(9.1);
    expect(o.savingsRate).toMatchObject({ pct: 66.2, month: "October", delta: "72.8% in September" });
    expect(`${o.bufferNote}${o.savingsRate?.note}`).not.toContain("¥");
    const none = buildOverview([acct], [bal], null, null)!;
    expect(none.bufferMonths).toBeNull();
    expect(none.savingsRate).toBeNull();
    expect(buildOverview([acct], [bal], null, { ...summary, buffer_months: null, savings_rate: null })!.bufferMonths).toBeNull();
  });
});

describe("personal.money", () => {
  it("is null without a spend figure or without a stored rate, never a made-up one", () => {
    expect(personalMoneyFrom(null, { rate: 150, at: "2026-11-12", source: "x" })).toBeNull();
    expect(personalMoneyFrom({ ...summary, current_spend: null }, { rate: 150, at: "d", source: "x" })).toBeNull();
    expect(personalMoneyFrom(summary, null)).toBeNull();
  });
  it("converts only with the stored rate, names it, and leaves unsourced parts unwired", () => {
    const m = personalMoneyFrom(summary, { rate: 150, at: "2026-11-12", source: "x" })!;
    expect(m.month).toBe("November");
    expect(m.spent).toMatchObject({ state: "live", value: 10000 / 150 });
    expect(m.spentNote).toContain("150.00 JPY per USD (2026-11-12)");
    expect(m.spentNote).not.toContain("¥");
    expect([m.leftAfterBills.state, m.billsDue.state, m.subsMonthly.state]).toEqual(["unwired", "unwired", "unwired"]);
    expect(m.bills).toEqual([]);
  });
});

describe("money facts", () => {
  it("every mapped fact carries a source ref; a fact without one is dropped and the set is partial", () => {
    const f = mapFacts(facts())!;
    expect(f.facts.every((x) => x.source.length > 0)).toBe(true);
    const bad = mapFacts(facts({ facts: [...facts().facts, { key: "x", label: "x", value: 5, unit: "JPY", as_of: null, source: "", stale: false }] }))!;
    expect(bad.facts).toHaveLength(2);
    expect(bad.partial).toBe(true);
    expect(mapFacts({ nope: 1 } as never)).toBeNull();
  });
  it("gates: none without accounts, partial when any account is stale or unknown, ok otherwise", () => {
    expect(moneyTakeGate(null).mode).toBe("none");
    expect(moneyTakeGate(mapFacts(facts({ personal_accounts: 0, facts: [], partial: true, partial_reasons: ["no_accounts"] })))).toEqual({ mode: "none" });
    expect(moneyTakeGate(mapFacts(facts({ partial: true, partial_reasons: ["stale:a2"] })))).toEqual({ mode: "partial", reasons: ["stale:a2"] });
    expect(moneyTakeGate(mapFacts(facts())).mode).toBe("ok");
  });
  it("text: no number without a sourced fact, banned words and the yen sign are rejected, NFA line required, no suggestion while partial", () => {
    const f = mapFacts(facts())!;
    const t = (s: string) => `${s} ${NFA_LINE}`;
    expect(checkMoneyText(t("Your buffer is 9.1 months."), f)).toEqual({ ok: true });
    expect(checkMoneyText(t("Your buffer is 12 months."), f)).toMatchObject({ reason: "unsupported_number" });
    expect(checkMoneyText(t("Your buffer is 9.1 months."), null)).toMatchObject({ reason: "no_source" });
    expect(checkMoneyText("Your buffer is 9.1 months.", f)).toMatchObject({ reason: "no_nfa" });
    for (const w of ["A guaranteed return.", "This is risk-free.", "A sure thing.", "It will go up.", "To the moon.", "Get rich."]) {
      expect(w.match(MONEY_BANNED) || /guarantee/.test(w)).toBeTruthy();
      expect(checkMoneyText(t(w), f)).toMatchObject({ ok: false });
    }
    expect(checkMoneyText(t("Balance ¥745,500."), f)).toMatchObject({ reason: "currency_sign" });
    expect(checkMoneyText(t("Saving more cured it."), f)).toMatchObject({ reason: "causal" });
    const partial = mapFacts(facts({ partial: true, partial_reasons: ["unknown:a2"] }))!;
    expect(checkMoneyText(t("Your buffer is 9.1 months."), partial, true)).toMatchObject({ reason: "suggestion_while_partial" });
    expect(checkMoneyText(t("Your buffer is 9.1 months."), partial)).toEqual({ ok: true });
  });
});

describe("live adapter with the new reads", () => {
  const reply = (over: Record<string, unknown> = {}) => async (c: string) => {
    if (c in over) { const v = over[c]; if (v instanceof Error) throw v; return v; }
    return ({ money_accounts_list: [acct], fx_rate_get: fxWire, money_balances_list: { balances: [bal], fx: fxWire }, money_summary_read: summary, money_facts_read: facts() } as Record<string, unknown>)[c];
  };
  it("overview carries buffer and savings only with real summary data; take and followUps stay unwired", async () => {
    const m = makeLiveMoney(reply() as never, () => {});
    m.overview(); await flush();
    const o = m.overview();
    expect(o.state === "live" && o.value.bufferMonths).toBe(9.1);
    expect(m.facts()?.facts.length).toBe(2);
    expect(m.take().state).toBe("unwired");
    expect(m.followUps().state).toBe("unwired");
  });
  it("without a summary the overview is live but buffer and savings stay null", async () => {
    const m = makeLiveMoney(reply({ money_summary_read: null }) as never, () => {});
    m.overview(); await flush();
    const o = m.overview();
    expect(o.state === "live" && o.value.bufferMonths).toBeNull();
    expect(o.state === "live" && o.value.savingsRate).toBeNull();
  });
  it("a failing summary or facts read leaves the rest live and facts null", async () => {
    const m = makeLiveMoney(reply({ money_summary_read: new Error("x"), money_facts_read: new Error("y") }) as never, () => {});
    m.overview(); await flush();
    expect(m.overview().state).toBe("live");
    expect(m.facts()).toBeNull();
  });
});
