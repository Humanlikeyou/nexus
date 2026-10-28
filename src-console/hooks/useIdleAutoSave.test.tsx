// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

const silent = vi.hoisted(() =>
  vi.fn(async (_m: unknown): Promise<{ saved: boolean; summary: string | null }> => ({ saved: true, summary: "s" })),
);
vi.mock("@/terrance/system/continuity", () => ({ summarizeAndStoreSessionSilent: silent }));

import { useIdleAutoSave, AUTOSAVE_SIGNATURE_KEY, type AutoSaveMessage } from "./useIdleAutoSave";

const msgs: AutoSaveMessage[] = [
  { role: "user", content: "hi", timestamp: "2026-10-21T09:00:00Z" },
  { role: "assistant", content: "hello", timestamp: "2026-10-21T09:00:05Z" },
];

// In-memory Storage (Node's built-in localStorage can shadow jsdom's).
function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k) => (m.has(k) ? m.get(k)! : null),
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => { m.delete(k); },
    setItem: (k, v) => { m.set(k, String(v)); },
  };
}

beforeEach(() => {
  Object.defineProperty(window, "localStorage", { value: memoryStorage(), configurable: true });
  vi.useFakeTimers();
  silent.mockReset();
  silent.mockImplementation(async () => ({ saved: true, summary: "s" }));
});

describe("useIdleAutoSave", () => {
  it("saves once after idle and maps roles", async () => {
    renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(silent).toHaveBeenCalledTimes(1);
    expect(silent.mock.calls[0][0]).toEqual([
      { role: "user", text: "hi", ts: Date.parse(msgs[0].timestamp) },
      { role: "terrance", text: "hello", ts: Date.parse(msgs[1].timestamp) },
    ]);
    expect(window.localStorage.getItem(AUTOSAVE_SIGNATURE_KEY)).toContain("c1:2:");
  });

  it("does not save before idle or with fewer than 2 messages", async () => {
    const { rerender } = renderHook(({ m }) => useIdleAutoSave(m, "c1", 1000), { initialProps: { m: [msgs[0]] } });
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(silent).not.toHaveBeenCalled();
    rerender({ m: msgs });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(silent).not.toHaveBeenCalled();
  });

  it("saves the outgoing transcript on unmount (session end)", async () => {
    const { unmount } = renderHook(() => useIdleAutoSave(msgs, "c1", 60_000));
    unmount();
    expect(silent).toHaveBeenCalledTimes(1);
  });

  it("saves the previous conversation when a new chat starts", async () => {
    const { rerender } = renderHook(({ m, k }) => useIdleAutoSave(m, k, 60_000), { initialProps: { m: msgs, k: "c1" } });
    rerender({ m: [], k: "c2" });
    expect(silent).toHaveBeenCalledTimes(1);
    expect((silent.mock.calls[0][0] as unknown[]).length).toBe(2);
  });

  it("does not re-save an already-saved transcript after relaunch", async () => {
    const first = renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    first.unmount();
    renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(silent).toHaveBeenCalledTimes(1);
  });

  it("retries later if the save did not persist", async () => {
    silent.mockImplementation(async () => ({ saved: false, summary: null }));
    const { unmount } = renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    unmount();
    expect(silent).toHaveBeenCalledTimes(2);
  });

  it("treats a legacy (pre-hash) signature as already saved and upgrades it", async () => {
    window.localStorage.setItem(AUTOSAVE_SIGNATURE_KEY, `c1:2:${Date.parse(msgs[1].timestamp)}`);
    const { unmount } = renderHook(() => useIdleAutoSave(msgs, "c1", 1000));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    unmount();
    expect(silent).not.toHaveBeenCalled();
    expect(window.localStorage.getItem(AUTOSAVE_SIGNATURE_KEY)).toMatch(/^c1:2:\d+:h[0-9a-z]+$/);
  });

  it("saveNow saves immediately, dedupes, and sets lastSavedAt", async () => {
    vi.setSystemTime(new Date("2026-10-28T05:00:00Z"));
    const { result } = renderHook(() => useIdleAutoSave(msgs, "c1", 60_000));
    expect(result.current.lastSavedAt).toBeNull();
    await act(async () => { result.current.saveNow(); await vi.advanceTimersByTimeAsync(0); });
    expect(silent).toHaveBeenCalledTimes(1);
    expect(result.current.lastSavedAt).toBe(Date.parse("2026-10-28T05:00:00Z"));
    await act(async () => { result.current.saveNow(); await vi.advanceTimersByTimeAsync(0); });
    expect(silent).toHaveBeenCalledTimes(1);
  });

  it("saveNow with fewer than 2 messages does nothing", async () => {
    const { result } = renderHook(() => useIdleAutoSave([msgs[0]], "c1", 60_000));
    await act(async () => { result.current.saveNow(); await vi.advanceTimersByTimeAsync(0); });
    expect(silent).not.toHaveBeenCalled();
    expect(result.current.lastSavedAt).toBeNull();
  });
});
