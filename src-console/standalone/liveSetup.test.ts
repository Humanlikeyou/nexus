import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));
let progress: ((e: { payload: unknown }) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({ listen: async (n: string, h: (e: { payload: unknown }) => void) => { if (n === "model://progress") progress = h; return () => {}; } }));

const TIERS = [
  { id: "compact", name: "Compact", minRamGb: 16, models: ["a"], download: "Size not published yet", note: null, recommended: false, available: true },
  { id: "standard", name: "Standard", minRamGb: 32, models: ["a", "b"], download: "Size not published yet", note: "x", recommended: true, available: true },
];
const sup = (id: string, state: string) => ({ id, state, reasonCode: "ok", sinceMs: 0, checkedMs: 0, attempt: 0, maxAttempts: 3, owner: "supervisor", adopted: false, can: {} });
const ev = (role: string, state: string, pct: number, done: number, total = 1000) => progress?.({ payload: { role, state, pct, bytesDone: done, bytesTotal: total } });

describe("live standalone adapter: first-run setup", () => {
  beforeEach(() => { vi.resetModules(); invoke.mockReset(); progress = null; });
  const load = async (over: Record<string, unknown> = {}) => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd in over) { const v = over[cmd]; if (v instanceof Error) throw "failed"; return v; }
      if (cmd === "model_tiers") return TIERS;
      if (cmd === "setup_done") return false;
      if (cmd === "secrets_status") return [];
      if (cmd === "key_file_status") return { made: false, where: "w" };
      return undefined;
    });
    const { unwiredStandalone } = await import("./adapters");
    const l = await import("./liveAdapter");
    const a = l.makeLiveStandalone(unwiredStandalone);
    await l.refreshLive();
    return a;
  };

  it("tiers map note null to undefined; setupDone is live from setup_done", async () => {
    const a = await load();
    const t = a.tiers();
    if (t.state !== "live") throw new Error("expected live");
    expect(t.value[0].note).toBeUndefined();
    expect(t.value[1].note).toBe("x");
    expect(a.setupDone()).toEqual({ state: "live", value: false });
  });

  it("tiers and setupDone stay unwired on an error or a malformed row", async () => {
    const a = await load({ model_tiers: new Error("x"), setup_done: new Error("x") });
    expect(a.tiers().state).toBe("unwired");
    expect(a.setupDone().state).toBe("unwired");
    const b = await load({ model_tiers: [{ id: "x" }], setup_done: "yes" });
    expect(b.tiers().state).toBe("unwired");
    expect(b.setupDone().state).toBe("unwired");
  });

  it("progress is merged monotonically and never invented", async () => {
    const a = await load();
    expect(a.setupDownload().state).toBe("unwired"); // no events, no supervisor: nothing to show
    ev("fast", "downloading", 40, 400);
    ev("fast", "downloading", 25, 250); // out of order: ignored
    const d = a.setupDownload();
    if (d.state !== "live") throw new Error("expected live");
    expect(d.value.models).toEqual([{ name: "Fast model", state: "downloading", detail: "400 B of 1.0 KB", pct: 40 }]);
    ev("fast", "verifying", 100, 1000);
    ev("fast", "done", 100, 1000);
    ev("fast", "downloading", 10, 100); // after done: ignored
    ev("deep", "failed", 0, 0, 500);
    ev("bogus", "nope", 1, 1); // malformed: ignored
    const e = a.setupDownload();
    if (e.state !== "live") throw new Error("expected live");
    expect(e.value.models).toEqual([
      { name: "Fast model", state: "done", detail: "1.0 KB of 1.0 KB", pct: 100 },
      { name: "Deep model", state: "waiting", detail: "Download failed", pct: 0 },
    ]);
    ev("fast", "waiting", 0, 0); // a new run starts over
    const f = a.setupDownload();
    if (f.state !== "live") throw new Error("expected live");
    expect(f.value.models[0]).toMatchObject({ state: "waiting", pct: 0 });
  });

  it("services come from the supervisor; start-all rows narrow the list and fill a stale state", async () => {
    const a = await load({ services_status: [sup("amara-router", "healthy"), sup("model-runtime", "unknown"), sup("docker", "stopped")], services_summary: { total: 3, attention: 0, healthy: 3, updateReady: false },
      services_start_all: [{ id: "amara-router", outcome: "adopted" }, { id: "model-runtime", outcome: "started" }] });
    const before = a.setupDownload();
    if (before.state !== "live") throw new Error("expected live");
    expect(before.value.services.map((s) => s.state)).toEqual(["running", "waiting", "waiting"]);
    expect((await a.startServices()).state).toBe("live");
    const after = a.setupDownload();
    if (after.state !== "live") throw new Error("expected live");
    expect(after.value.services).toEqual([{ name: "Router", state: "running" }, { name: "Model runtime", state: "running" }]);
  });

  it("skipped_act_off and failed rows never show as running", async () => {
    const a = await load({ services_status: [sup("amara-router", "unknown"), sup("model-runtime", "unknown")], services_start_all: [{ id: "amara-router", outcome: "skipped_act_off" }, { id: "model-runtime", outcome: "failed" }] });
    await a.startServices();
    const d = a.setupDownload();
    if (d.state !== "live") throw new Error("expected live");
    expect(d.value.services.map((s) => s.state)).toEqual(["waiting", "waiting"]);
  });

  it("actions call the commands with the tier; errors come back unwired and log fixed codes only", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const a = await load();
    expect((await a.pickTier("standard")).state).toBe("live");
    expect((await a.pickTier("nope")).state).toBe("unwired");
    expect((await a.startDownload("standard")).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("model_download", { tier: "standard" });
    expect((await a.finishSetup("standard")).state).toBe("unwired"); // download ok but start-all has not run
    expect(invoke).not.toHaveBeenCalledWith("setup_complete", expect.anything());
    expect((await a.resetSetup()).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("setup_reset");
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "model_download") throw "outbound_blocked"; if (cmd === "setup_complete") throw "/Users/x/secret raw text"; return undefined; });
    expect((await a.startDownload("standard")).state).toBe("unwired");
    expect((await a.finishSetup("standard")).state).toBe("unwired");
    expect(JSON.stringify(warn.mock.calls)).toContain("setup_not_ready");
    const logged = JSON.stringify(warn.mock.calls);
    expect(logged).toContain("outbound_blocked");
    expect(logged).not.toContain("/Users");
    warn.mockRestore();
  });

  it("setup is marked done only after a live download and a clean start-all; today (no host, act off) it never is", async () => {
    const ok = [{ id: "amara-router", outcome: "adopted" }, { id: "model-runtime", outcome: "started" }];
    const run = async (rows: unknown, download: "ok" | "blocked") => {
      const a = await load({ services_start_all: rows, ...(download === "blocked" ? {} : {}) });
      const base = invoke.getMockImplementation()!;
      invoke.mockImplementation(async (cmd: string) => { if (cmd === "model_download" && download === "blocked") throw "outbound_blocked"; return base(cmd); });
      await a.startDownload("standard"); await a.startServices();
      invoke.mockClear();
      const r = await a.finishSetup("standard");
      return { r, called: invoke.mock.calls.some((c) => c[0] === "setup_complete") };
    };
    expect((await run(ok, "ok")).called).toBe(true);
    expect((await run(ok, "blocked")).called).toBe(false);
    const today = [{ id: "amara-router", outcome: "skipped_act_off" }, { id: "model-runtime", outcome: "skipped_act_off" }];
    expect((await run(today, "blocked")).called).toBe(false);
    for (const bad of ["failed", "needs_you", "binary_missing"]) expect((await run([{ id: "amara-router", outcome: bad }], "ok")).called).toBe(false);
    expect((await run(ok, "ok")).r.state).toBe("live");
  });
});
