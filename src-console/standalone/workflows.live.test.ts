import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const SETTINGS = { nightly: true, at: "03:00", dir: "~/Amara Backups", keep: 14, beforeUpdate: true, skipOnBatteryBelow: 20 };
const ROW = { id: "amara-backup-20261112-030000.tar.gz", label: "Nov 12", madeAt: "2026-11-12T03:00:00+09:00", sizeBytes: 5_000_000, daysOld: 0 };
const PREVIEW = { file: ROW.id, verify: { ok: true, mode: "sha256" }, files: 3, bytes: 5_000_000, groups: [{ name: "goals.json", files: 1, bytes: 3 }], leftOut: [], reenter: [], keyFileNote: "x" };
const UPDATE = { state: "not_configured", configured: false, current_version: "0.1.0", auto_update: true, check_daily: true, announced_version_unverified: null };
const flush = () => new Promise((r) => setTimeout(r, 0));

function mock(over: Record<string, unknown> = {}) {
  invoke.mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd in over) { const v = over[cmd]; if (typeof v === "function") return (v as (a: unknown) => unknown)(args); return v; }
    if (cmd === "backup_settings_get") return SETTINGS;
    if (cmd === "backup_list") return [ROW];
    if (cmd === "secrets_status") return [];
    if (cmd === "key_file_status") return { made: true, where: "w" };
    if (cmd === "update_status") return UPDATE;
    return undefined;
  });
}
const thrower = (e: unknown) => () => { throw e; };
async function load() {
  vi.resetModules();
  const { unwiredStandalone } = await import("./adapters");
  const live = await import("./liveAdapter");
  live.resetOpsForTests(); live.resetRelayForTests();
  const a = live.makeLiveStandalone(unwiredStandalone);
  await live.refreshLive();
  return { a, live };
}

describe("Backup restore (workflow)", () => {
  beforeEach(() => { invoke.mockReset(); vi.spyOn(console, "warn").mockImplementation(() => {}); });

  it("previewBackup reads the chosen backup by id through restore_preview and returns its real contents", async () => {
    mock({ restore_preview: PREVIEW });
    const { a } = await load();
    const r = await a.previewBackup(ROW.id);
    if (r.state !== "done") throw new Error("expected done");
    expect(invoke).toHaveBeenCalledWith("restore_preview", { file: ROW.id });
    expect(r.preview.checksumOk).toBe(true);
    expect(r.preview.contents).toEqual([{ title: "goals.json", sub: "1 file · 3 B" }]);
  });
  it("previewBackup says why it can't: unknown backup, a running backup, a bad read", async () => {
    mock({ restore_preview: thrower("backup_running") });
    const { a } = await load();
    expect(await a.previewBackup("nope")).toMatchObject({ state: "missing" });
    expect(await a.previewBackup(ROW.id)).toMatchObject({ state: "failed", message: expect.stringMatching(/backup is running/) });
  });
  it("restore hands only the backup id to restore_backup (no path building in the renderer; the approval gate stays in the backend)", async () => {
    mock({ restore_backup: ["Safety backup of current data saved to /x", "Restored goals.json"] });
    const { a } = await load();
    const r = await a.restore(ROW.id);
    expect(invoke).toHaveBeenCalledWith("restore_backup", { file: ROW.id });
    expect(invoke.mock.calls.some((c) => c[0] === "unpack_backup")).toBe(false);
    expect(r).toEqual({ state: "done", lines: ["Safety backup of current data saved to /x", "Restored goals.json"] });
  });
  it.each([
    ["declined: Denied in the native approval dialog.", "declined", /cancelled/i],
    ["busy: another approval is open", "busy", /already running/],
    ["expired: dialog open too long", "expired", /timed out/],
    ["changed: files changed while the dialog was open", "failed", /data changed while the confirmation was open/],
    ["Archive file not found", "failed", /wasn't found/],
  ])("restore failure %s gets its own outcome and fixed copy", async (err, state, re) => {
    mock({ restore_backup: thrower(err) });
    const { a } = await load();
    const r = await a.restore(ROW.id);
    expect(r.state).toBe(state);
    expect((r as { message?: string }).message).toMatch(re);
    expect(JSON.stringify(r)).not.toContain("native approval dialog.");
  });
  it("pickBackupDir saves the picked folder with backup_settings_set({dir}); cancel changes nothing; a folder outside home says why", async () => {
    const calls: string[] = [];
    mock({ "plugin:dialog|open": () => "/Users/x/Backups", backup_settings_set: (a: unknown) => { calls.push(JSON.stringify(a)); return SETTINGS; } });
    const { a } = await load();
    expect(await a.pickBackupDir()).toEqual({ state: "done", dir: "/Users/x/Backups" });
    expect(calls).toEqual([JSON.stringify({ patch: { dir: "/Users/x/Backups" } })]);
    mock({ "plugin:dialog|open": () => null, backup_settings_set: () => { calls.push("called"); } });
    const b = await load();
    expect(await b.a.pickBackupDir()).toEqual({ state: "cancelled" });
    expect(calls).not.toContain("called");
    mock({ "plugin:dialog|open": () => "/Volumes/x", backup_settings_set: thrower("dir_not_in_home") });
    const c = await load();
    expect(await c.a.pickBackupDir()).toMatchObject({ state: "failed", message: expect.stringMatching(/inside your home folder/) });
  });
});

describe("Services restart (workflow)", () => {
  beforeEach(() => { invoke.mockReset(); vi.spyOn(console, "warn").mockImplementation(() => {}); });
  const sup = (state: string) => [{ id: "n8n", state, reasonCode: "ok", sinceMs: 1, checkedMs: 1_700_000_000_000, attempt: 0, maxAttempts: 3, owner: "supervisor", adopted: false, can: { restart: true, start: false, stop: false } }];

  it("polls the supervisor until the service is healthy, then nudges the app snapshot", async () => {
    let n = 0;
    mock({ service_restart: { id: "n8n", failedTries: 0 }, services_status: () => sup(++n < 3 ? "restarting" : "healthy") });
    const { a, live } = await load();
    live.SERVICE_POLL.everyMs = 1; live.SERVICE_POLL.maxMs = 2000;
    const snap = vi.fn(); window.addEventListener("amara:snapshot", snap);
    const r = await a.serviceAction("n8n", "restart");
    window.removeEventListener("amara:snapshot", snap);
    if (r.state !== "done") throw new Error(`expected done, got ${r.state}`);
    expect(r.healthy).toBe(true);
    expect(n).toBeGreaterThanOrEqual(3);
    expect(snap).toHaveBeenCalledTimes(1);
  });
  it("gives up waiting after the bound and says it restarted but isn't healthy", async () => {
    mock({ service_restart: { id: "n8n", failedTries: 0 }, services_status: () => sup("restarting") });
    const { a, live } = await load();
    live.SERVICE_POLL.everyMs = 1; live.SERVICE_POLL.maxMs = 15;
    expect(await a.serviceAction("n8n", "restart")).toEqual({ state: "done", healthy: false });
  });
  it("a failed restart doesn't poll or nudge the snapshot", async () => {
    mock({ service_restart: thrower("restart_failed") });
    const { a } = await load();
    const snap = vi.fn(); window.addEventListener("amara:snapshot", snap);
    expect(await a.serviceAction("n8n", "restart")).toEqual({ state: "failed" });
    window.removeEventListener("amara:snapshot", snap);
    expect(snap).not.toHaveBeenCalled();
  });
});

describe("Updates (workflow)", () => {
  beforeEach(() => { invoke.mockReset(); vi.spyOn(console, "warn").mockImplementation(() => {}); });

  it("update_status maps to the real off state with no invented update, history or check time", async () => {
    mock();
    const { a } = await load();
    const u = a.updates();
    if (u.state !== "live") throw new Error("expected live");
    expect(u.value).toMatchObject({ current: "0.1.0", backend: "not_configured", available: null, history: [], lastCheck: "not checked this session", autoUpdate: true });
  });
  it("an announced version is shown as available with no notes and no schedule", async () => {
    mock({ update_status: { ...UPDATE, state: "available", configured: true, announced_version_unverified: "0.2.0" } });
    const { a } = await load();
    const u = a.updates();
    if (u.state !== "live") throw new Error("expected live");
    expect(u.value.available).toEqual({ version: "0.2.0", notes: [], scheduledFor: "" });
  });
  it("Check Now: off build, paused outbound, network failure and success each differ; success records the check time", async () => {
    for (const [err, state] of [["not_configured", "unsupported"], ["outbound_disabled", "off"], ["check_failed: x", "failed"]] as const) {
      mock({ update_check: thrower(err) });
      const { a } = await load();
      expect((await a.checkUpdate()).state).toBe(state);
    }
    mock({ update_check: { ...UPDATE, state: "idle" }, update_status: { ...UPDATE, state: "idle", configured: true } });
    const { a } = await load();
    expect(await a.checkUpdate()).toEqual({ state: "done" });
    const u = a.updates();
    if (u.state !== "live") throw new Error("expected live");
    expect(u.value.lastCheck).toMatch(/^today at /);
  });
  it("install refuses honestly (not available / declined) and prefs go to update_set_prefs", async () => {
    mock({ update_install_now: thrower("install_not_available") });
    const { a } = await load();
    expect((await a.installUpdate()).state).toBe("unsupported");
    mock({ update_install_now: thrower("declined: Denied") });
    expect((await (await load()).a.installUpdate()).state).toBe("declined");
    mock({ update_set_prefs: { auto_update: false, check_daily: true } });
    const b = await load();
    expect((await b.a.setUpdatePrefs({ autoUpdate: false })).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("update_set_prefs", { patch: { auto_update: false, check_daily: null } });
  });
});

describe("Notifications (workflow)", () => {
  beforeEach(() => { invoke.mockReset(); vi.spyOn(console, "warn").mockImplementation(() => {}); });

  it.each([
    ["Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED or local-only mode).", "off"],
    ["declined: Denied in the native approval dialog.", "declined"],
    ["Telegram bot token and chat are not both set.", "missing"],
    ["Telegram API returned an error. Check your bot token and chat ID.", "telegram"],
    ["something else", "failed"],
  ])("Send Test failure %s is %s", async (err, state) => {
    mock({ notify_send_test: thrower(err) });
    const { a } = await load();
    expect((await a.relaySendTest()).state).toBe(state);
  });
  it("Write another calls notify_preview again and replaces the text; a known backend line passes through, anything else is generic", async () => {
    let n = 0;
    mock({ notify_preview: () => ({ text: `Text ${++n}`, at: "7:12 AM", from: "From your latest recovery reading." }) });
    const { a } = await load();
    a.relayPreview(); await flush(); await flush();
    expect(await a.writeAnotherPreview()).toEqual({ state: "done" });
    const p = a.relayPreview();
    if (p.state !== "live") throw new Error("expected live");
    expect(p.value.text).toBe("Text 2");
    expect(invoke.mock.calls.filter((c) => c[0] === "notify_preview")).toHaveLength(2);
    mock({ notify_preview: thrower("No reading for this alert yet.") });
    expect(await (await load()).a.writeAnotherPreview()).toEqual({ state: "failed", message: "No reading for this alert yet." });
    mock({ notify_preview: thrower("http://secret-host/boom") });
    expect(await (await load()).a.writeAnotherPreview()).toEqual({ state: "failed", message: "Couldn't write another one right now." });
  });
});
