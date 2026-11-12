import { describe, expect, it } from "vitest";
import { unwiredCrews } from "./adapters";
import { liveN8nAdapter, loadN8n, getN8n, setN8n } from "./n8n";
import { mapAudit, mapGrant } from "./toolGrantLive";

describe("n8n live adapter (R13-F)", () => {
  it("stays unwired unless n8n answered and both counts are known", () => {
    for (const d of [null, { connected: false, workflows: null, failures7d: null }, { connected: true, workflows: null, failures7d: null }, { connected: true, workflows: 3, failures7d: null }]) {
      expect(liveN8nAdapter(unwiredCrews, d)).toBe(unwiredCrews);
    }
  });
  it("is live with real counts, and never overrides a sample adapter", () => {
    const a = liveN8nAdapter(unwiredCrews, { connected: true, workflows: 4, failures7d: 1 });
    expect(a.n8n()).toEqual({ state: "live", value: { connected: true, workflows: 4, failures7d: 1 } });
    const sampleN8n = { ...unwiredCrews, n8n: () => ({ state: "sample", value: { connected: true, workflows: 9, failures7d: 0 } }) as ReturnType<typeof unwiredCrews.n8n> };
    expect(liveN8nAdapter(sampleN8n, { connected: true, workflows: 4, failures7d: 1 })).toBe(sampleN8n);
  });
  it("loadN8n caches the probe and a failure clears it", async () => {
    await loadN8n(async () => ({ connected: true, workflows: 2, failures7d: 0 }));
    expect(getN8n()?.workflows).toBe(2);
    await loadN8n(async () => { throw new Error("down"); });
    expect(getN8n()).toBeNull();
    setN8n(null);
  });
});

describe("crew tool grant live mapping (1110i)", () => {
  it("is on only when crew-service reports it enforcing tools", () => {
    expect(mapGrant("Research", { enabled: true, tools: ["shell"], enforced_allowlist: [] }).status).toBe("off");
    const on = mapGrant("Research", { enabled: true, tools: ["shell"], duration: "1h", scope_path: "/crew-sandbox/research", expires_at: 1_800_000_000, enforced_allowlist: ["kb_query", "shell"] });
    expect(on).toMatchObject({ status: "on", duration: "1h", scopePath: "/crew-sandbox/research", enforced: ["kb_query", "shell"] });
    expect(on.expiresAt).toBeTruthy();
  });
  it("is off by default and pending when a request exists", () => {
    expect(mapGrant("Taiyo Skin", { enabled: false, tools: [], enforced_allowlist: ["kb_query"] })).toEqual({ crew: "Taiyo Skin", status: "off", scopePath: "~/crew-sandbox/taiyo-skin" });
    const p = mapGrant("R", { enabled: false, tools: [], enforced_allowlist: [], pending_request: { id: "a", tools: ["shell"], duration: "run", reason: "tests", scope_path: "~/crew-sandbox/r" } });
    expect(p.status).toBe("pending");
    expect(p.request?.duration).toBe("run");
  });
  it("audit rows map known kinds and never carry output fields", () => {
    const rows = mapAudit([{ id: 3, at: 1_800_000_000, actor: "taisun", kind: "grant", text: "x", target: "~/f", exit_code: 0, checkpoint: "abc" }, { id: 2, at: 1_800_000_000, actor: "a", kind: "weird", text: "y" }]);
    expect(rows.map((r) => r.kind)).toEqual(["grant", "info"]);
    expect(Object.keys(rows[0]).sort()).toEqual(["actor", "at", "id", "kind", "target", "text"]);
  });
});
