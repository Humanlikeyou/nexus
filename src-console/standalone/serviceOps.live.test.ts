import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const SETTINGS = { nightly: true, at: "03:00", dir: "~/Amara Backups", keep: 14, beforeUpdate: true, skipOnBatteryBelow: 20 };
const ROWS = [
  { id: "amara-backup-20261110-030000.tar.gz", label: "Nov 10", madeAt: "2026-11-10T03:00:00+09:00", sizeBytes: 10, daysOld: 2 },
  { id: "amara-backup-20261112-030000.tar.gz", label: "Nov 12", madeAt: "2026-11-12T03:00:00+09:00", sizeBytes: 5_000_000, daysOld: 0 },
];
const PREVIEW = { file: ROWS[1].id, verify: { ok: true, mode: "sha256" }, files: 3, bytes: 5_000_000, groups: [{ name: "goals.json", files: 1, bytes: 3 }, { name: "nexus-config/replays", files: 2, bytes: 4_000_000 }], leftOut: [], reenter: [], keyFileNote: "x" };
const flush = () => new Promise((r) => setTimeout(r, 0));

function mock(over: Record<string, unknown> = {}) {
  invoke.mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd in over) { const v = over[cmd]; if (typeof v === "function") return (v as (a: unknown) => unknown)(args); if (v instanceof Error) throw "failed"; return v; }
    if (cmd === "backup_settings_get") return SETTINGS;
    if (cmd === "backup_list") return ROWS;
    if (cmd === "secrets_status") return [];
    if (cmd === "key_file_status") return { made: true, where: "w" };
    return undefined;
  });
}
async function load() {
  vi.resetModules();
  const { unwiredStandalone } = await import("./adapters");
  const live = await import("./liveAdapter");
  live.resetOpsForTests();
  return { a: live.makeLiveStandalone(unwiredStandalone), live };
}

describe("W2 live adapter: services, whoop, profile, restore preview", () => {
  beforeEach(() => { invoke.mockReset(); });

  it("serviceLogs loads on first use, sends only the id, and shows the tail it was given", async () => {
    mock({ service_logs: { id: "n8n", lines: ["a", "b [redacted]"], truncated: false } });
    const { a } = await load();
    expect(a.serviceLogs("n8n").state).toBe("unwired"); // first call starts the load
    await flush(); await flush();
    const w = a.serviceLogs("n8n");
    if (w.state !== "live") throw new Error("expected live");
    expect(w.value.tail).toEqual(["a", "b [redacted]"]);
    expect(w.value.events).toEqual([]); // no invented plain-words events
    expect(invoke.mock.calls.filter((c) => c[0] === "service_logs")).toEqual([["service_logs", { id: "n8n" }]]);
  });

  it("serviceLogs for a service with no log, or a bad answer, stays unwired", async () => {
    mock({ service_logs: new Error("no_log") });
    const { a } = await load();
    a.serviceLogs("voice"); await flush(); await flush();
    expect(a.serviceLogs("voice").state).toBe("unwired");
    mock({ service_logs: { id: "x", lines: [1, 2] } });
    const b = await load();
    b.a.serviceLogs("router"); await flush(); await flush();
    expect(b.a.serviceLogs("router").state).toBe("unwired");
  });

  it("serviceAction restart calls service_restart with the id only; start has no backend", async () => {
    mock({ service_restart: { id: "n8n", failedTries: 0 } });
    const { a } = await load();
    expect((await a.serviceAction("n8n", "restart")).state).toBe("done");
    expect(invoke.mock.calls.filter((c) => c[0] === "service_restart")).toEqual([["service_restart", { id: "n8n" }]]);
    invoke.mockClear();
    expect((await a.serviceAction("n8n", "start")).state).toBe("unsupported");
    expect(invoke.mock.calls.some((c) => c[0] === "service_restart")).toBe(false);
  });

  it("a declined, unsupported, needs_you or failed restart each get their own outcome and log only a short code", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const want: Array<[string, string]> = [["declined: Denied in the native approval dialog. Nothing was changed.", "declined"], ["unsupported", "unsupported"], ["needs_you: restart tried 3 times", "gave_up"], ["restart_failed", "failed"]];
    for (const [err, state] of want) {
      mock({ service_restart: () => { throw err; } });
      const { a } = await load();
      expect((await a.serviceAction("n8n", "restart")).state).toBe(state);
    }
    for (const c of warn.mock.calls) expect(String(c[1]).length).toBeLessThanOrEqual(40);
    warn.mockRestore();
  });

  it("whoop() maps only what whoop_status knows and never invents a connection or battery", async () => {
    const now = Date.parse("2026-11-14T10:00:00Z");
    const { live } = await load();
    const w = live.toWhoop({ owner: "legacy", storeFound: true, lastSampleAt: now / 1000 - 120, gaps14d: 2 }, now);
    if (w.state !== "live") throw new Error("expected live");
    expect(w.value).toMatchObject({ owner: "legacy", connected: false, batteryPct: null, gaps14d: 2, liveHr: "delayed", lastSync: "Last write 2 min ago" });
    const none = live.toWhoop({ owner: "none", storeFound: false, lastSampleAt: null, gaps14d: null }, now);
    expect(none.state).toBe("unwired"); // unknown gaps are not "0 gaps"
    const quiet = live.toWhoop({ owner: "none", storeFound: true, lastSampleAt: null, gaps14d: 14 }, now);
    if (quiet.state !== "live") throw new Error("expected live");
    expect(quiet.value.lastSync).toBe("No reading yet");
    expect(quiet.value.liveHr).toBe("off");
  });

  it("whoop owner comes from the backend through refreshLive, and strap actions stay unwired with no command sent", async () => {
    mock({ whoop_status: { owner: "legacy", storeFound: true, lastSampleAt: 1, gaps14d: 0 } });
    const { a, live } = await load();
    await live.refreshLive();
    const w = a.whoop();
    if (w.state !== "live") throw new Error("expected live");
    expect(w.value.owner).toBe("legacy");
    invoke.mockClear();
    for (const act of ["sync", "buzz", "forget", "take_over", "lucid_on"] as const) expect((await a.whoopAction(act)).state).toBe("unwired");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("profile() is live with RAM, chip and FileVault; a malformed or unknown FileVault answer is unwired", async () => {
    mock({ system_profile: { ramGb: 36, chip: "Apple M3 Pro", arch: "aarch64", cores: 12, macos: "15.1", fileVault: true } });
    const { a, live } = await load();
    await live.refreshLive();
    expect(a.profile()).toEqual({ state: "live", value: { ramGb: 36, chip: "Apple M3 Pro", fileVault: true } });
    mock({ system_profile: { ramGb: 36, chip: "x", arch: "a", cores: 1, macos: null, fileVault: null } });
    const b = await load();
    await b.live.refreshLive();
    expect(b.a.profile().state).toBe("unwired");
    mock({ system_profile: { ramGb: "36" } });
    const c = await load();
    await c.live.refreshLive();
    expect(c.a.profile().state).toBe("unwired");
  });

  it("restorePreview() previews the newest backup by id only, maps it, and asks once", async () => {
    mock({ restore_preview: PREVIEW });
    const { a, live } = await load();
    await live.refreshLive();
    expect(a.restorePreview().state).toBe("unwired"); // first call starts the load
    await flush(); await flush();
    const p = a.restorePreview();
    if (p.state !== "live") throw new Error("expected live");
    expect(p.value.file).toBe(ROWS[1].id);
    expect(p.value.checksumOk).toBe(true);
    expect(p.value.size).toBe("5.0 MB");
    expect(p.value.contents.map((c) => c.title)).toEqual(["goals.json", "nexus-config/replays"]);
    expect(p.value.keyFileFound).toBe(true);
    a.restorePreview();
    expect(invoke.mock.calls.filter((c) => c[0] === "restore_preview")).toEqual([["restore_preview", { file: ROWS[1].id }]]);
  });

  it("restorePreview() with no backups or a refused preview stays unwired, and restore of an unknown backup never calls restore_backup", async () => {
    mock({ backup_list: [], restore_preview: new Error("x") });
    const { a, live } = await load();
    await live.refreshLive();
    expect(a.restorePreview().state).toBe("unwired");
    expect(invoke.mock.calls.some((c) => c[0] === "restore_preview")).toBe(false);
    mock({ restore_preview: new Error("not_found") });
    const b = await load();
    await b.live.refreshLive();
    b.a.restorePreview(); await flush(); await flush();
    expect(b.a.restorePreview().state).toBe("unwired");
    expect((await b.a.restore("anything")).state).toBe("missing");
    expect(invoke.mock.calls.some((c) => /unpack/.test(String(c[0])))).toBe(false);
  });
});

describe("Phase 8 supervisor wiring", () => {
  beforeEach(() => { invoke.mockReset(); });
  const ST = (id: string, state: string, over: Record<string, unknown> = {}) => ({ id, state, reasonCode: "ok", sinceMs: 1, checkedMs: 1_700_000_000_000, attempt: 0, maxAttempts: 3, owner: "supervisor", adopted: false, can: { restart: true, start: false, stop: false }, ...over });
  const MAP = { checkedAt: "2026-11-12T10:00:00+09:00", nodes: [{ id: "router", label: "Amara Router", kind: "backend" }], paths: [{ id: "p", from: "app", to: "router", carries: "x", status: "ok", lastOkAt: null, reason: null, fix: null }] };

  it("services and summary come from the supervisor when it answers", async () => {
    mock({ services_status: [ST("amara-router", "healthy"), ST("crew-service", "needs_you", { reasonCode: "restart_failed", attempt: 3 })], services_summary: { total: 2, attention: 1 }, system_map_status: MAP });
    const { a, live } = await load();
    await live.refreshLive();
    const s = a.services();
    if (s.state !== "live") throw new Error("expected live");
    expect(s.value.services.map((r) => [r.id, r.state])).toEqual([["amara-router", "healthy"], ["crew-service", "failed"]]);
    expect(a.summary()).toEqual({ state: "live", value: { attention: 1, updateReady: false } });
  });
  it("falls back to the system map when the supervisor command errors (older backend)", async () => {
    mock({ services_status: new Error("x"), services_summary: new Error("x"), system_map_status: MAP });
    const { a, live } = await load();
    await live.refreshLive();
    const s = a.services();
    if (s.state !== "live") throw new Error("expected live");
    expect(s.value.services.map((r) => r.id)).toEqual(["router"]);
    expect(a.summary()).toEqual({ state: "live", value: { attention: 0, updateReady: false } });
  });
  it("with neither source, services stay unwired (nothing invented)", async () => {
    mock({ services_status: new Error("x"), services_summary: new Error("x") });
    const { a, live } = await load();
    await live.refreshLive();
    expect(a.services().state).toBe("unwired");
    expect(a.summary().state).toBe("unwired");
  });
  it("serviceLogs adds the supervisor events (fixed text) next to the tail; events alone are enough", async () => {
    const E = [{ seq: 1, tsMs: 1_700_000_000_000, id: "n8n", kind: "state_changed", state: "degraded", reasonCode: "slow", text: "Slow to answer." }];
    mock({ service_logs: { id: "n8n", lines: ["a"] }, service_events: (args: unknown) => (JSON.stringify(args) === JSON.stringify({ since: null, id: "n8n" }) ? E : []) });
    const { a } = await load();
    a.serviceLogs("n8n"); await flush(); await flush();
    const w = a.serviceLogs("n8n");
    if (w.state !== "live") throw new Error("expected live");
    expect(w.value.tail).toEqual(["a"]);
    expect(w.value.events.map((e) => [e.level, e.text])).toEqual([["warn", "Slow to answer."]]);
    mock({ service_logs: new Error("no_log"), service_events: E });
    const b = await load();
    b.a.serviceLogs("docker"); await flush(); await flush();
    const w2 = b.a.serviceLogs("docker");
    if (w2.state !== "live") throw new Error("expected live");
    expect(w2.value).toMatchObject({ tail: [] });
    expect(w2.value.events).toHaveLength(1);
  });
});
