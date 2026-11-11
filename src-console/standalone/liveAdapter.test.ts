import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const SENTINEL = "SENTINEL-live-adapter-7f3a";

describe("live standalone adapter (secrets + n8n key file)", () => {
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
});
