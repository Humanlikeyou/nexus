import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), Channel: class {} }));

import { useScreenOpens } from "../../src-console/app/useScreenOpens";
import { recordScreenOpen, screenOpenCounts } from "../../src-console/bridge";
import type { ModuleId } from "../../src-console/types";

beforeEach(() => { invoke.mockReset(); invoke.mockResolvedValue(undefined); });
const calls = () => invoke.mock.calls.filter((c) => c[0] === "screen_open_record");

describe("useScreenOpens", () => {
  it("records the initial screen once on mount", () => {
    renderHook(() => useScreenOpens("terrance"));
    expect(calls()).toEqual([["screen_open_record", { screen: "terrance" }]]);
  });

  it("records on a top-level change", () => {
    const { rerender } = renderHook(({ id }: { id: ModuleId }) => useScreenOpens(id), { initialProps: { id: "terrance" as ModuleId } });
    rerender({ id: "settings" });
    expect(calls().map((c) => (c[1] as { screen: string }).screen)).toEqual(["terrance", "settings"]);
  });

  it("does not record on re-render with the same id (sub-view changes keep the same id)", () => {
    const { rerender } = renderHook(({ id, view }: { id: ModuleId; view: string }) => { void view; useScreenOpens(id); }, { initialProps: { id: "life" as ModuleId, view: "a" } });
    rerender({ id: "life", view: "b" });
    rerender({ id: "life", view: "b" });
    expect(calls()).toHaveLength(1);
  });

  it("ignores ids outside SYSTEMS", () => {
    renderHook(() => useScreenOpens("not-a-screen" as ModuleId));
    expect(calls()).toHaveLength(0);
  });

  it("a rejected invoke does not throw", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    expect(() => renderHook(() => useScreenOpens("chat"))).not.toThrow();
    await expect(recordScreenOpen("chat")).resolves.toBeUndefined();
  });

  it("a failed counts read reaches the caller instead of looking like zero opens", async () => {
    invoke.mockRejectedValue(new Error("store_error"));
    await expect(screenOpenCounts(4)).rejects.toThrow("store_error");
    invoke.mockResolvedValue([{ weekStart: "2026-11-09", screen: "chat", count: 2 }]);
    await expect(screenOpenCounts(4)).resolves.toEqual([{ weekStart: "2026-11-09", screen: "chat", count: 2 }]);
    expect(invoke).toHaveBeenLastCalledWith("screen_open_counts", { weeks: 4 });
  });
});
