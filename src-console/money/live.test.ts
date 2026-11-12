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
