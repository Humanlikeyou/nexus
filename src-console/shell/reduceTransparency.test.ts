import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ listen: vi.fn(), get: vi.fn(), unlisten: vi.fn(), emit: null as ((event: { payload: boolean }) => void) | null }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../bridge", () => ({ getReduceTransparency: mocks.get }));
import { REDUCE_TRANSPARENCY_EVENT, installReduceTransparency } from "./reduceTransparency";

const root = document.documentElement;
const w = window as unknown as Record<string, unknown>;
const settle = () => new Promise((resolve) => setTimeout(resolve));

describe("installReduceTransparency", () => {
  beforeEach(() => {
    w.__TAURI_INTERNALS__ = {};
    mocks.listen.mockImplementation(async (_name: string, handler: (event: { payload: boolean }) => void) => { mocks.emit = handler; return mocks.unlisten; });
  });
  afterEach(() => { delete w.__TAURI_INTERNALS__; delete root.dataset.reduceTransparency; });

  it("sets the attribute from the startup read", async () => {
    mocks.get.mockResolvedValue(true);
    const stop = installReduceTransparency();
    await settle();
    expect(mocks.listen).toHaveBeenCalledWith(REDUCE_TRANSPARENCY_EVENT, expect.any(Function));
    expect(root.getAttribute("data-reduce-transparency")).toBe("true");
    stop();
  });
  it("follows the event both ways", async () => {
    mocks.get.mockResolvedValue(false);
    const stop = installReduceTransparency();
    await settle();
    expect(root.hasAttribute("data-reduce-transparency")).toBe(false);
    mocks.emit?.({ payload: true });
    expect(root.getAttribute("data-reduce-transparency")).toBe("true");
    mocks.emit?.({ payload: false });
    expect(root.hasAttribute("data-reduce-transparency")).toBe(false);
    stop();
  });
  it("lets an event win over a slower startup read", async () => {
    let finish: (on: boolean) => void = () => undefined;
    mocks.get.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const stop = installReduceTransparency();
    await settle();
    mocks.emit?.({ payload: false });
    finish(true);
    await settle();
    expect(root.hasAttribute("data-reduce-transparency")).toBe(false);
    stop();
  });
  it("stops listening when stopped", async () => {
    mocks.get.mockResolvedValue(false);
    const stop = installReduceTransparency();
    await settle();
    stop();
    expect(mocks.unlisten).toHaveBeenCalled();
    mocks.emit?.({ payload: true });
    expect(root.hasAttribute("data-reduce-transparency")).toBe(false);
  });
  it("survives a backend without the command, and does nothing outside Tauri", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.get.mockRejectedValue(new Error("command not found"));
    installReduceTransparency()();
    await settle();
    delete w.__TAURI_INTERNALS__;
    mocks.listen.mockClear();
    installReduceTransparency()();
    expect(mocks.listen).not.toHaveBeenCalled();
  });
});
