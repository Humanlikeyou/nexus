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
    expect((await a.serviceAction("n8n", "restart")).state).toBe("live");
    expect(invoke.mock.calls.filter((c) => c[0] === "service_restart")).toEqual([["service_restart", { id: "n8n" }]]);
    invoke.mockClear();
    expect((await a.serviceAction("n8n", "start")).state).toBe("unwired");
    expect(invoke.mock.calls.some((c) => c[0] === "service_restart")).toBe(false);
  });

  it("a declined, unsupported or needs_you restart is unwired and logs only a short code", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    for (const err of ["declined: Denied in the native approval dialog. Nothing was changed.", "unsupported", "needs_you: restart tried 3 times"]) {
      mock({ service_restart: () => { throw err; } });
      const { a } = await load();
      expect((await a.serviceAction("n8n", "restart")).state).toBe("unwired");
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

  it("restorePreview() with no backups or a refused preview stays unwired, and restore itself stays unbound", async () => {
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
    expect((await b.a.restore("anything")).state).toBe("unwired");
    expect(invoke.mock.calls.some((c) => /restore$|unpack/.test(String(c[0])) && c[0] !== "restore_preview")).toBe(false);
  });
});
