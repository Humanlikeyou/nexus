import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

const FAKE = "FAKE_TOKEN_NOT_REAL";
const hostile = new Error(`EACCES: open '/Users/someone/Library/Application Support/com.taisun.nexus/goals.json' ${FAKE}`);

vi.mock("@/lib/api", () => ({
  readGoals: vi.fn(async () => []),
  saveGoals: vi.fn(async () => { throw hostile; }),
  deleteGoal: vi.fn(),
  insertEvents: vi.fn(async () => undefined),
}));
vi.mock("@/terrance/system/continuity", () => ({
  loadContext: vi.fn(async () => ({})),
  saveContext: vi.fn(async () => undefined),
}));

import { useGoals, GOALS_SAVE_ERROR } from "./useGoals";

describe("useGoals save error", () => {
  beforeEach(() => vi.spyOn(console, "error").mockImplementation(() => {}));

  it("shows fixed text, never the raw error, and logs the raw error locally", async () => {
    const { result } = renderHook(() => useGoals());
    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      await result.current.addGoal({ title: "t" } as never);
    });
    expect(result.current.saveError).toBe(GOALS_SAVE_ERROR);
    expect(result.current.saveError).not.toContain("/Users/");
    expect(result.current.saveError).not.toContain(FAKE);
    expect(console.error).toHaveBeenCalledWith("[goals] save failed:", hostile);
  });
});
