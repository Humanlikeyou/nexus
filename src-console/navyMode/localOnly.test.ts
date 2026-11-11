import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { liveNavy, navyAdapter } from "./adapters";
import { enterNavy, exitNavy, finishEnter, finishExit, navyStore } from "./store";

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("navy mode keeps everything on this Mac (design-1111x localOnly)", () => {
  beforeEach(async () => {
    invoke.mockReset();
    invoke.mockImplementation(async (_cmd: string, args: { on: boolean }) => args.on);
    localStorage.removeItem("nexus.sampleAdapters");
    navyStore.reset();
    // Leave the module's confirmed flag off from any previous test.
    invoke.mockImplementationOnce(async () => false);
    const { setNavyLocalOnly } = await import("./adapters");
    await setNavyLocalOnly(false);
    invoke.mockReset();
    invoke.mockImplementation(async (_cmd: string, args: { on: boolean }) => args.on);
  });

  it("turns the backend switch on when the look starts and off only once it is fully off", async () => {
    expect(navyAdapter()).toBe(liveNavy);
    expect(liveNavy.localOnly().state).toBe("unwired");
    enterNavy("chat");
    await flush();
    expect(invoke).toHaveBeenCalledWith("set_local_only", { on: true });
    expect(liveNavy.localOnly()).toEqual({ state: "live", value: { ok: true } });
    finishEnter();
    exitNavy("chat");
    await flush();
    expect(invoke).not.toHaveBeenCalledWith("set_local_only", { on: false });
    expect(liveNavy.localOnly().state).toBe("live");
    finishExit();
    await flush();
    expect(invoke).toHaveBeenLastCalledWith("set_local_only", { on: false });
    expect(liveNavy.localOnly().state).toBe("unwired");
  });

  it("fails closed: if the backend can't confirm, it never claims local-only", async () => {
    invoke.mockImplementation(async () => { throw new Error("no backend"); });
    enterNavy("voice");
    await flush();
    expect(liveNavy.localOnly().state).toBe("unwired");
  });
});
