import { describe, expect, it, vi } from "vitest";
import { makeLiveMoney, mapAccount, mapFx } from "./live";

const raw = { id: "a1", nickname: "Japan bank", institution: "smbc", kind: "time_deposit", route: "csv", currency: "JPY", scope: "personal", createdAt: "2026-11-01T09:00:00+09:00" };
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("live money mapping", () => {
  it("maps an account with no balance, never 0", () => {
    const a = mapAccount(raw)!;
    expect(a).toMatchObject({ institution: "jp_bank", route: "csv", region: "jp", currency: "JPY", scope: "personal", native: "–", usd: null, asOf: null });
    expect(a.synced).toBe("Added 2026-11-01");
  });
  it("drops unknown enum values instead of guessing", () => {
    expect(mapAccount({ ...raw, institution: "mystery" })).toBeNull();
    expect(mapAccount({ ...raw, scope: "x" })).toBeNull();
  });
  it("maps fx with date and source; bad or null is unwired", () => {
    expect(mapFx({ rate: 150.5, date: "2026-11-11", source: "ecb" })).toEqual({ rate: 150.5, at: "2026-11-11", source: "ECB reference rate" });
    expect(mapFx(null)).toBeNull();
    expect(mapFx({ rate: 0, date: "2026-11-11", source: "ecb" })).toBeNull();
    expect(mapFx({ rate: NaN, date: "2026-11-11", source: "ecb" })).toBeNull();
  });
});

describe("live money adapter", () => {
  it("is unwired until the first read, then live; never calls fx_rate_refresh", async () => {
    const call = vi.fn(async (c: string) => (c === "money_accounts_list" ? [raw] : { rate: 150.5, date: "2026-11-11", source: "manual" }));
    const notify = vi.fn();
    const m = makeLiveMoney(call as never, notify);
    expect(m.accounts().state).toBe("unwired");
    await flush();
    const a = m.accounts(); const f = m.fx();
    expect(a.state === "live" && a.value).toHaveLength(1);
    expect(f.state === "live" && f.value.source).toBe("entered by you");
    expect(call.mock.calls.map((c) => c[0]).sort()).toEqual(["fx_rate_get", "money_accounts_list", "money_balances_list"]);
    expect(notify).toHaveBeenCalled();
    expect(m.overview().state).toBe("unwired");
    expect(m.take().state).toBe("unwired");
  });
  it("stays unwired when the commands fail or the rate is null", async () => {
    const m = makeLiveMoney((async () => { throw new Error("no backend"); }) as never, vi.fn());
    m.accounts(); await flush();
    expect(m.accounts().state).toBe("unwired");
    expect(m.fx().state).toBe("unwired");
    const n = makeLiveMoney((async (c: string) => (c === "fx_rate_get" ? null : [])) as never, vi.fn());
    n.accounts(); await flush();
    expect(n.fx().state).toBe("unwired");
    expect(n.accounts()).toEqual({ state: "live", value: [] });
  });
});

const jp = { ...raw, id: "jp1", nickname: "SMBC main", kind: "ordinary" };
const ledger = { id: "l1", nickname: "Cold", institution: "ledger", kind: "btc", route: "manual", currency: "BTC", scope: "personal", createdAt: "2026-11-01T09:00:00+09:00" };
const fxRaw = { rate: 150, date: "2026-11-11", source: "manual", fetched_at: "x" };
const stmt = { account_id: "jp1", currency: "JPY", amount: 300000, as_of: "2026-11-10", source: "statement", history: [{ date: "2026-10-20", amount: 260000 }, { date: "2026-11-10", amount: 300000 }] };
const btc = { account_id: "l1", currency: "BTC", amount: 0.5, as_of: "2026-11-10", source: "manual", history: [{ date: "2026-11-10", amount: 0.5 }] };
const backend = (balances: unknown[], fx: unknown) => vi.fn(async (c: string) => (c === "money_accounts_list" ? [jp, ledger] : c === "fx_rate_get" ? fx : { balances, fx }));

describe("live money overview", () => {
  it("is live from accounts + balances + fx: USD total, cash, 30-day delta, no USD for crypto", async () => {
    const m = makeLiveMoney(backend([stmt, btc], fxRaw) as never, vi.fn());
    m.overview(); await flush();
    const w = m.overview();
    expect(w.state).toBe("live");
    if (w.state !== "live") return;
    const o = w.value;
    expect(o.totalUsd).toBeCloseTo(2000); // 300000 JPY / 150
    expect(o.cash.usd).toBeCloseTo(2000);
    expect(o.investments.usd).toBeNull();
    expect(o.crypto.usd).toBeNull();
    expect(o.rows[0]).toMatchObject({ name: "SMBC main", native: "300,000 JPY", d30: "+40,000 JPY", d30note: "since 2026-10-20" });
    expect(o.rows[0].usd).toBeCloseTo(2000);
    expect(o.rows[1]).toMatchObject({ native: "0.5 BTC", usd: null, d30: "–" });
    expect(o.bufferMonths).toBeNull();
    expect(o.savingsRate).toBeNull();
  });
  it("has no USD value for yen without an FX rate, and shows – (not 0) for a missing balance", async () => {
    const m = makeLiveMoney(backend([btc], null) as never, vi.fn());
    m.overview(); await flush();
    const w = m.overview();
    expect(w.state).toBe("live");
    if (w.state !== "live") return;
    expect(w.value.fx).toBeNull();
    expect(w.value.rows[0]).toEqual({ name: "SMBC main", native: "–", usd: null, d30: "–" });
    const y = makeLiveMoney(backend([stmt], null) as never, vi.fn());
    y.overview(); await flush();
    const v = y.overview();
    if (v.state !== "live") throw new Error("expected live");
    expect(v.value.totalUsd).toBeNull();
    expect(v.value.cash.usd).toBeNull();
    expect(v.value.rows[0].usd).toBeNull();
    expect(v.value.allocation).toEqual([]);
  });
  it("stays unwired with no balances or when money_balances_list fails", async () => {
    const none = makeLiveMoney(backend([], fxRaw) as never, vi.fn());
    none.overview(); await flush();
    expect(none.overview().state).toBe("unwired");
    const bad = makeLiveMoney((async (c: string) => { if (c === "money_balances_list") throw "store_error"; return c === "money_accounts_list" ? [jp] : null; }) as never, vi.fn());
    bad.overview(); await flush();
    expect(bad.overview().state).toBe("unwired");
    expect(bad.accounts().state).toBe("live");
  });
});

describe("live money watch-only", () => {
  const XPUB = "xpub" + "6".repeat(107);
  it("ledger route is ready, jp_bank stays unwired", () => {
    const m = makeLiveMoney((async () => []) as never, vi.fn());
    const by = Object.fromEntries(m.routes().map((r) => [r.id, r.ready.state]));
    expect(by).toMatchObject({ ledger: "live", jp_bank: "unwired", chime: "unwired", fidelity: "unwired" });
  });
  it("creates the account then adds the public value; never sends a refused value", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "money_account_add" ? { id: "a1" } : []));
    const m = makeLiveMoney(call as never, vi.fn());
    expect((await m.addWatchOnly("btc", XPUB, "Ledger")).state).toBe("live");
    expect(call).toHaveBeenCalledWith("money_watch_add", { accountId: "a1", value: XPUB });
    call.mockClear();
    const phrase = "abandon ability able about above absent absorb abstract absurd abuse access accident";
    expect((await m.addWatchOnly("btc", phrase, "x")).state).toBe("unwired");
    expect((await m.addWatchOnly("eth", XPUB, "x")).state).toBe("unwired"); // chain mismatch
    expect(call.mock.calls.filter((c) => c[0] === "money_account_add" || c[0] === "money_watch_add")).toHaveLength(0);
  });
  it("a backend refusal is unwired", async () => {
    const call = vi.fn(async (cmd: string) => { if (cmd === "money_account_add") throw "invalid_input"; return []; });
    const m = makeLiveMoney(call as never, vi.fn());
    expect((await m.addWatchOnly("btc", XPUB, "x")).state).toBe("unwired");
  });
});

describe("live money account rows", () => {
  it("native from balances, USD only with an fx rate, never for crypto, dash with no balance", () => {
    const bals = [stmt, btc];
    expect(mapAccount(jp, bals, null)).toMatchObject({ native: "300,000 JPY", usd: null });
    expect(mapAccount(jp, bals, { rate: 150, at: "2026-11-11", source: "x" })).toMatchObject({ native: "300,000 JPY", usd: 2000 });
    expect(mapAccount(ledger, bals, { rate: 150, at: "x", source: "x" })).toMatchObject({ native: "0.5 BTC", usd: null });
    expect(mapAccount(jp, [], null)).toMatchObject({ native: "–", usd: null });
  });
  it("adapter rows carry the balances once read", async () => {
    const m = makeLiveMoney(backend([stmt, btc], fxRaw) as never, vi.fn());
    m.accounts(); await flush();
    const a = m.accounts();
    expect(a.state === "live" && a.value.map((x) => [x.native, x.usd])).toEqual([["300,000 JPY", 2000], ["0.5 BTC", null]]);
  });
});
