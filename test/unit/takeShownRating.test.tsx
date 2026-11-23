import { renderHook, waitFor, act } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: Array<{ cmd: string; args: any }> = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: unknown) => { calls.push({ cmd, args }); return true; }),
}));

import { recordTakeRating, recordTakeShown, resetTakeEventsForTest, setTakeEvents } from "../../src-console/takes/events";
import { resetShownForTest, useAmaraTake } from "../../src-console/amaraTake/useAmaraTake";
import type { TakeDeps } from "../../src-console/amaraTake/generate";
import type { AmaraTake } from "../../src-console/amaraTake/types";

const appended = () => calls.filter((c) => c.cmd === "take_events_append").map((c) => c.args.event);

beforeEach(() => { calls.length = 0; resetTakeEventsForTest(); resetShownForTest(); vi.useRealTimers(); });

describe("recordTakeShown", () => {
  it("appends a take_shown with exactly {takeId} and the given day", async () => {
    expect(await recordTakeShown("fitness", "fitness-1", "2026-11-14")).toBe(true);
    const [e] = appended();
    expect(e).toMatchObject({ kind: "take_shown", page: "fitness", day: "2026-11-14", data: { takeId: "fitness-1" } });
    expect(Object.keys(e.data)).toEqual(["takeId"]);
  });
  it("skips a repeat for the same takeId and day, but not a new day or take", async () => {
    setTakeEvents([]);
    await recordTakeShown("fitness", "fitness-1", "2026-11-14");
    expect(await recordTakeShown("fitness", "fitness-1", "2026-11-14")).toBe(false);
    await recordTakeShown("fitness", "fitness-1", "2026-11-15");
    await recordTakeShown("fitness", "fitness-2", "2026-11-14");
    expect(appended()).toHaveLength(3);
    expect(new Set(appended().map((e) => e.id)).size).toBe(3);
  });
});

describe("recordTakeRating", () => {
  it("appends take_rating with {takeId, helpful} and the JST day", async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date("2026-11-14T16:30:00Z")); // 01:30 on the 15th in Tokyo
    await recordTakeRating("neuro", "neuro-9", true);
    const [e] = appended();
    expect(e).toMatchObject({ kind: "take_rating", page: "neuro", day: "2026-11-15", data: { takeId: "neuro-9", helpful: true } });
    expect(Object.keys(e.data).sort()).toEqual(["helpful", "takeId"]);
  });
  it("gives each rating its own id", async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date("2026-11-14T03:00:00Z"));
    await recordTakeRating("neuro", "neuro-9", true);
    vi.setSystemTime(new Date("2026-11-14T03:00:01Z"));
    await recordTakeRating("neuro", "neuro-9", false);
    expect(new Set(appended().map((e) => e.id)).size).toBe(2);
  });
});

const take: AmaraTake = { page: "fitness", day: "2026-11-14", text: "Slept 7 h.", cited: [], fingerprint: "f", generatedAt: 1763100000000 };
const deps = (r: "live" | "unwired"): TakeDeps => ({ now: () => take.generatedAt, kv: { load: async () => null, save: async () => {} }, facts: async () => (r === "live" ? { page: "fitness", day: take.day, fingerprint: "f", facts: [{ key: "sleep", label: "Sleep", value: 7, unit: "h", display: "7 h", baseline: null, accept: [7] }] } : null), ask: async () => ({ content: "Sleep: 7 h.", tier: "local" }) } as unknown as TakeDeps);

describe("useAmaraTake records shown", () => {
  it("records once per take per day for a live take, across re-renders and remounts", async () => {
    const d = deps("live");
    const { rerender, unmount } = renderHook(() => useAmaraTake("fitness", d));
    await waitFor(() => expect(appended()).toHaveLength(1));
    rerender(); await act(async () => {});
    unmount();
    renderHook(() => useAmaraTake("fitness", d));
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
    const shown = appended().filter((e) => e.kind === "take_shown");
    expect(shown).toHaveLength(1);
    expect(shown[0].data.takeId).toMatch(/^fitness-\d+$/);
  });
  it("never records for an unwired result", async () => {
    const d = deps("unwired");
    const { result } = renderHook(() => useAmaraTake("fitness", d));
    await waitFor(() => expect(result.current.state).toBe("unwired"));
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
    expect(appended()).toHaveLength(0);
  });
});
