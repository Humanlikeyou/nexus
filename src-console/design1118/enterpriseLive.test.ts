import { describe, expect, it, vi } from "vitest";
// @ts-expect-error vite ?raw import (no ambient type in this project)
import enterpriseSource from "./enterpriseLive.ts?raw";
import { enterpriseAdapter, makeEnterpriseLive, unwiredEnterprise } from "./enterpriseLive";

const fin = (o = {}) => ({
  ledger: "ok", treasury: "ok", fx: { usdPerJpy: 0.0065, asOf: "2026-11-15", ageDays: 3, stale: false },
  funds: [{ name: "Taiyo Skin", balanceJpy: 100000, balanceUsd: 650 }],
  sources: [{ streamId: "s1", ventureId: "taiyo-skin", name: "Shop", streamType: "product_sales", status: "active", lastImportAt: null, daysSinceImport: null, imports: 0, stale: false }],
  monitorState: "no_data", monitorAsOf: null, ...o,
});
const trk = (o = {}) => ({ ledger: "ok", treasury: "ok", fx: null, ventures: [{ id: "taiyo-skin", name: "Taiyo Skin", streamIds: ["s1"], incomeMonth: [], attentionMinutes30d: null, perHour30d: [] }], ...o });

function make(f: unknown, t: unknown, extra?: (c: string, a?: Record<string, unknown>) => unknown) {
  const calls: [string, unknown][] = [];
  const call = vi.fn(async (c: string, a?: Record<string, unknown>) => {
    calls.push([c, a]);
    if (c === "finances_view") { if (f instanceof Error) throw f; return f; }
    if (c === "venture_tracking_list") { if (t instanceof Error) throw t; return t; }
    return extra ? extra(c, a) : undefined;
  }) as never;
  return { a: makeEnterpriseLive({ call, notify: () => {} }), calls };
}

describe("enterprise live adapters (B4)", () => {
  it("are unwired until the read lands, then live with real values", async () => {
    const { a } = make(fin(), trk());
    expect(a.finances().state).toBe("unwired");
    await vi.waitFor(() => expect(a.finances().state).toBe("live"));
    const fx = a.fx();
    expect(fx.state === "live" && [fx.value.asOf, fx.value.stale]).toEqual(["2026-11-15", false]);
    const f = a.funds();
    expect(f.state === "live" && f.value[0].balanceUsd).toBe(650);
    expect(a.sources().state).toBe("live");
    expect(a.ventures().state).toBe("live");
  });

  it("no ledger, an invalid ledger or a failed read is unwired, never a made-up row", async () => {
    for (const [f, t] of [[fin({ ledger: "missing", funds: [], sources: [], fx: null }), trk({ ledger: "missing", ventures: [] })], [fin({ ledger: "invalid" }), trk({ ledger: "invalid" })], [new Error("x"), new Error("y")], ["junk", []]] as const) {
      const { a, calls } = make(f, t);
      a.finances();
      await vi.waitFor(() => expect(calls.length).toBe(2));
      await new Promise((r) => setTimeout(r, 0));
      for (const k of ["finances", "fx", "funds", "sources", "ventures"] as const) expect(a[k]().state).toBe("unwired");
    }
  });

  it("no rate means no fx (Yen stays NEEDS_CONVERSION), and no funds or sources means unwired", async () => {
    const { a } = make(fin({ fx: null, funds: [], sources: [] }), trk({ ventures: [] }));
    a.finances();
    await vi.waitFor(() => expect(a.finances().state).toBe("live"));
    expect(a.fx().state).toBe("unwired");
    expect(a.funds().state).toBe("unwired");
    expect(a.sources().state).toBe("unwired");
    expect(a.ventures().state).toBe("unwired");
    const { a: b } = make(fin({ fx: { usdPerJpy: null, asOf: "2026-11-15", ageDays: 3, stale: false } }), trk());
    b.finances();
    await vi.waitFor(() => expect(b.finances().state).toBe("live"));
    expect(b.fx().state).toBe("unwired");
  });

  it("draft and stage moves call only their commands; Published needs postedByMe; failures are unwired", async () => {
    const { a, calls } = make(fin(), trk(), (c) => (c === "content_draft_propose" ? { title: "T", text: "Hi", tier: "local" } : c === "content_stage_set" ? { id: "content-1", title: "T", from: "ideation", to: "drafting", toast: "ok" } : true));
    expect((await a.draft("T")).state).toBe("live");
    expect((await a.setStage("T", "ideation", "published")).state).toBe("live");
    await a.setStage("T", "ideation", "published", true);
    expect(await a.undoStage("content-1")).toBe(true);
    const sets = calls.filter(([c]) => c === "content_stage_set").map(([, x]) => (x as { postedByMe: boolean }).postedByMe);
    expect(sets).toEqual([false, true]);
    expect(calls.find(([c]) => c === "add_undo")?.[1]).toEqual({ kind: "content", id: "content-1" });
    const names = new Set(calls.map(([c]) => c));
    expect([...names].every((n) => ["finances_view", "venture_tracking_list", "content_draft_propose", "content_stage_set", "add_undo"].includes(n))).toBe(true);
    const bad = makeEnterpriseLive({ call: (async () => { throw new Error("x"); }) as never, notify: () => {} });
    expect((await bad.draft("T")).state).toBe("unwired");
    expect((await bad.setStage("T", "ideation", "drafting")).state).toBe("unwired");
    expect(await bad.undoStage("x")).toBe(false);
  });

  it("outside Tauri every read is unwired and nothing is called", async () => {
    expect(enterpriseAdapter()).toBe(unwiredEnterprise);
    expect(unwiredEnterprise.finances().state).toBe("unwired");
    expect((await unwiredEnterprise.draft("T")).state).toBe("unwired");
  });

  it("source pin: no personal money, no outside host, no posting call, no yen sign", () => {
    const src = enterpriseSource as string;
    for (const bad of ["money_", "fx_rate", "moneySummary", "fetch(", "XMLHttpRequest", "WebSocket", "http://", "https://", "¥", "￥", "sendMessage", "publishPost"]) expect(src.includes(bad), bad).toBe(false);
  });
});
