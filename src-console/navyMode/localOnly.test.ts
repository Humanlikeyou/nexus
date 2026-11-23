import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));

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
    expect(navyStore.get().phase).toBe("off"); // entry aborted, the look never starts unenforced
  });

  it("waits for the backend before the look starts", async () => {
    let release: (v: boolean) => void = () => {};
    invoke.mockImplementationOnce(() => new Promise<boolean>((r) => { release = r; }));
    enterNavy("chat");
    expect(navyStore.get().phase).toBe("off");
    release(true);
    await flush();
    expect(navyStore.get().phase).toBe("entering");
  });

  it("speak() stays unwired when no local speak entry is confirmed: voice or typed, no TTS call, no cloud path", async () => {
    invoke.mockImplementation(async (cmd: string, args?: { on: boolean }) => (cmd === "voice_speak_ready" ? false : args?.on));
    for (const by of ["voice", "chat"] as const) {
      invoke.mockClear();
      expect(liveNavy.speak("Local line", { openedBy: by }).state).toBe("unwired");
      enterNavy(by);
      await flush();
      expect(navyStore.get().caption).toBeTruthy(); // caption still shows
      expect(navyStore.get().caption?.spoken).toBe(false); // never claims it was spoken
      expect(invoke.mock.calls.every(([cmd]) => cmd === "set_local_only" || cmd === "voice_speak_ready")).toBe(true);
      expect(invoke.mock.calls.some(([cmd]) => cmd === "voice_speak_local")).toBe(false);
      finishEnter();
      exitNavy(by);
      finishExit();
      await flush();
    }
  });

});
