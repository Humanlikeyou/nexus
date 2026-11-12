import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

let progressHandler: ((e: { payload: unknown }) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({ listen: async (_n: string, h: (e: { payload: unknown }) => void) => { progressHandler = h; return () => {}; } }));

const SENTINEL = "SENTINEL-live-adapter-7f3a";

describe("live standalone adapter (secrets + n8n key file + backups)", () => {
  beforeEach(() => { vi.resetModules(); invoke.mockReset(); });

  it("is unwired until the first load, then live with set flags only", async () => {
    invoke.mockImplementation(async (cmd: string) => cmd === "secrets_status" ? [{ name: "ha_token", label: "Home Assistant token", set: true }] : { made: false, where: "~/Amara Backups" });
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    expect(a.secrets().state).toBe("unwired");
    await refreshLive();
    const s = a.secrets();
    expect(s.state).toBe("live");
    expect(a.keyFile().state).toBe("live");
    expect(a.services().state).toBe("unwired"); // everything else stays unwired
  });

  it("setSecret passes the value to secret_set once, refreshes, and returns no value", async () => {
    invoke.mockImplementation(async (cmd: string) => cmd === "secrets_status" ? [] : cmd === "key_file_status" ? { made: false, where: "w" } : undefined);
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    const r = await a.setSecret("ha_token", SENTINEL);
    expect(JSON.stringify(r)).not.toContain("SENTINEL");
    expect(invoke).toHaveBeenCalledWith("secret_set", { name: "ha_token", value: SENTINEL });
    expect(invoke.mock.calls.filter((c) => c[0] === "secret_set")).toHaveLength(1);
  });

  it("a refused write is unwired and logs no value or passphrase", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "key_file_make") throw "no_key"; if (cmd === "secrets_status") return []; return { made: false, where: "w" }; });
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    const r = await a.makeKeyFile(SENTINEL);
    expect(r.state).toBe("unwired");
    expect(JSON.stringify(warn.mock.calls)).not.toContain("SENTINEL");
    warn.mockRestore();
  });

  const SETTINGS = { nightly: true, at: "03:00", dir: "~/Amara Backups", keep: 14, beforeUpdate: true, skipOnBatteryBelow: 20 };
  const ROWS = [{ id: "amara-backup-20261112-030000.tar.gz", label: "Nov 12, 3:00 AM", madeAt: "2026-11-12T03:00:00+09:00", sizeBytes: 5, daysOld: 0 }];
  const backupMock = (over: Record<string, unknown> = {}) => async (cmd: string) => {
    if (cmd in over) { const v = over[cmd]; if (v instanceof Error) throw "failed"; return v; }
    if (cmd === "backup_settings_get") return SETTINGS;
    if (cmd === "backup_list") return ROWS;
    if (cmd === "secrets_status") return [];
    if (cmd === "key_file_status") return { made: false, where: "w" };
    return undefined;
  };

  it("backup() is live from settings + list, with no invented last-run or version", async () => {
    invoke.mockImplementation(backupMock());
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    expect(a.backup().state).toBe("unwired");
    await refreshLive();
    const b = a.backup();
    if (b.state !== "live") throw new Error("expected live");
    expect(b.value.settings).toEqual(SETTINGS);
    expect(b.value.last).toBeNull();
    expect(b.value.running).toBeNull();
    expect(b.value.backups).toHaveLength(1);
    expect(b.value.backups[0]).toMatchObject({ id: ROWS[0].id, label: "Nov 12, 3:00 AM", version: "", daysOld: 0 });
    expect(a.restore).toBe(unwiredStandalone.restore);
  });

  it("a failed or malformed backup call leaves backup() unwired", async () => {
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    invoke.mockImplementation(backupMock({ backup_list: new Error("x") }));
    await refreshLive();
    expect(a.backup().state).toBe("unwired");
    invoke.mockImplementation(backupMock({ backup_settings_get: { made: false } }));
    await refreshLive();
    expect(a.backup().state).toBe("unwired");
  });

  it("backupNow shows a running bar while the command runs, then clears it and refreshes", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => { release = r; });
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "backup_now") { await gate; return {}; } return backupMock()(cmd); });
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    await refreshLive();
    const p = a.backupNow();
    const during = a.backup();
    if (during.state !== "live") throw new Error("expected live");
    expect(during.value.running).toMatchObject({ pct: 0, phase: "Starting" });
    release();
    expect((await p).state).toBe("live");
    const after = a.backup();
    if (after.state !== "live") throw new Error("expected live");
    expect(after.value.running).toBeNull();
  });

  it("a refused backupNow is unwired and clears the running bar", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "backup_now") throw "already_running"; return backupMock()(cmd); });
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    await refreshLive();
    expect((await a.backupNow()).state).toBe("unwired");
    const b = a.backup();
    if (b.state !== "live") throw new Error("expected live");
    expect(b.value.running).toBeNull();
    warn.mockRestore();
  });

  it("setBackupSettings sends only the fields given, and a refusal is unwired", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    invoke.mockImplementation(backupMock());
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    expect((await a.setBackupSettings({ nightly: false })).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("backup_settings_set", { patch: { nightly: false } });
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "backup_settings_set") throw "invalid_keep"; return backupMock()(cmd); });
    expect((await a.setBackupSettings({ keep: 0 })).state).toBe("unwired");
    warn.mockRestore();
  });

  it("backup://progress events drive the running bar while a backup runs, and are ignored otherwise", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => { release = r; });
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "backup_now") { await gate; return {}; } return backupMock()(cmd); });
    const { unwiredStandalone } = await import("./adapters");
    const { makeLiveStandalone, refreshLive } = await import("./liveAdapter");
    const a = makeLiveStandalone(unwiredStandalone);
    await refreshLive();
    progressHandler?.({ payload: { phase: "Compressing", pct: 75 } });
    const idle = a.backup();
    if (idle.state !== "live") throw new Error("expected live");
    expect(idle.value.running).toBeNull();
    const p = a.backupNow();
    progressHandler?.({ payload: { phase: "Compressing", pct: 75 } });
    progressHandler?.({ payload: { phase: 5, pct: "x" } });
    const mid = a.backup();
    if (mid.state !== "live") throw new Error("expected live");
    expect(mid.value.running).toEqual({ pct: 75, phase: "Compressing", done: "", total: "", left: "" });
    release();
    await p;
  });
});
