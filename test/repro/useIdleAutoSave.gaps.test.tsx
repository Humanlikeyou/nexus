// @vitest-environment jsdom
/** Coverage-gap tests for src-console/hooks/useIdleAutoSave.ts (complements its own test file). */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

const silent = vi.hoisted(() => vi.fn());
vi.mock("@/terrance/system/continuity", () => ({ summarizeAndStoreSessionSilent: silent }));

import { useIdleAutoSave, toSessionEntries, type AutoSaveMessage } from "../../src-console/hooks/useIdleAutoSave";

const m = (role: "user" | "assistant", content: string, ts: string, status?: "failed"): AutoSaveMessage =>
  ({ role, content, timestamp: ts, ...(status ? { status } : {}) });
const msgs = [m("user", "hi", "2026-10-21T09:00:00Z"), m("assistant", "hello", "2026-10-21T09:00:05Z")];

function memoryStorage(): Storage {
  const s = new Map<string, string>();
  return {
    get length() { return s.size; }, clear: () => s.clear(),
    getItem: (k) => (s.has(k) ? s.get(k)! : null), key: (i) => [...s.keys()][i] ?? null,
    removeItem: (k) => { s.delete(k); }, setItem: (k, v) => { s.set(k, String(v)); },
  };
}

beforeEach(() => {
  Object.defineProperty(window, "localStorage", { value: memoryStorage(), configurable: true });
  vi.useFakeTimers();
  silent.mockReset();
  silent.mockImplementation(async () => ({ saved: true, summary: "s" }));
});

describe("toSessionEntries", () => {
  it("drops failed and blank messages and maps an invalid timestamp to 0", () => {
    const out = toSessionEntries([
      m("user", "ok", "bad-date"), m("assistant", "   ", "2026-10-21T09:00:00Z"),
      m("assistant", "boom", "2026-10-21T09:00:00Z", "failed"),
    ]);
    expect(out).toEqual([{ role: "user", text: "ok", ts: 0 }]);
  });
});

describe("useIdleAutoSave gaps", () => {
  it("idle timer restarts on every new message (debounce)", async () => {
    const { rerender } = renderHook(({ x }) => useIdleAutoSave(x, "c1", 1000), { initialProps: { x: msgs } });
    await act(async () => { await vi.advanceTimersByTimeAsync(900); });
    rerender({ x: [...msgs, m("user", "more", "2026-10-21T09:01:00Z")] });
    await act(async () => { await vi.advanceTimersByTimeAsync(900); });
    expect(silent).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(silent).toHaveBeenCalledTimes(1);
    expect((silent.mock.calls[0][0] as unknown[]).length).toBe(3);
  });

  it("saves when the window becomes hidden", async () => {
    renderHook(() => useIdleAutoSave(msgs, "c1", 60_000));
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
    expect(silent).toHaveBeenCalledTimes(1);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });

  it("saves on pagehide", async () => {
    renderHook(() => useIdleAutoSave(msgs, "c1", 60_000));
    await act(async () => { window.dispatchEvent(new Event("pagehide")); });
    expect(silent).toHaveBeenCalledTimes(1);
  });

  it("does not double-save while a save is in flight", async () => {
    let resolve!: (v: unknown) => void;
    silent.mockImplementation(() => new Promise((r) => { resolve = r; }));
    const { unmount } = renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    await act(async () => { window.dispatchEvent(new Event("pagehide")); });
    unmount();
    expect(silent).toHaveBeenCalledTimes(1);
    await act(async () => { resolve({ saved: true, summary: "s" }); });
  });

  it("a rejected save does not throw into React and is retried later", async () => {
    silent.mockImplementationOnce(async () => { throw new Error("ipc"); });
    const { unmount } = renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    unmount();
    expect(silent).toHaveBeenCalledTimes(2);
  });

  it("an edited last message (same count/timestamp) is saved again", async () => {
    const { rerender } = renderHook(({ x }) => useIdleAutoSave(x, "c1", 1000), { initialProps: { x: msgs } });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    rerender({ x: [msgs[0], { ...msgs[1], content: "hello (edited)" }] });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(silent).toHaveBeenCalledTimes(2);
  });

  it("does not save a conversation with only failed messages", async () => {
    const failed = msgs.map((x) => ({ ...x, status: "failed" as const }));
    const { unmount } = renderHook(() => useIdleAutoSave(failed, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    unmount();
    expect(silent).not.toHaveBeenCalled();
  });
});
