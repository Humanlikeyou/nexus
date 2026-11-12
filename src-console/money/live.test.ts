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
    expect(call.mock.calls.map((c) => c[0]).sort()).toEqual(["fx_rate_get", "money_accounts_list"]);
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
