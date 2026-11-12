import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { inRoadmapWindow, localDayKey, roadmapNightDue, roadmapOutcome, ROADMAP_NIGHT_KEY, tickRoadmapNight } from "./roadmapNight";

const at = (h: number, m = 0, day = 12) => new Date(2026, 10, day, h, m).getTime();

describe("roadmap night window", () => {
  it("is open from 02:00 to 04:59 local only", () => {
    expect(inRoadmapWindow(at(1, 59))).toBe(false);
    expect(inRoadmapWindow(at(2))).toBe(true);
    expect(inRoadmapWindow(at(4, 59))).toBe(true);
    expect(inRoadmapWindow(at(5))).toBe(false);
  });
  it("runs once a night and never while something else is running", () => {
    expect(roadmapNightDue(at(3), null, false)).toBe(true);
    expect(roadmapNightDue(at(3), localDayKey(at(3)), false)).toBe(false);
    expect(roadmapNightDue(at(3, 30, 13), localDayKey(at(3)), false)).toBe(true);
    expect(roadmapNightDue(at(3), null, true)).toBe(false);
    expect(roadmapNightDue(at(12), null, false)).toBe(false);
  });
});

describe("roadmap night busy handling", () => {
  beforeEach(() => { invoke.mockReset(); globalThis.localStorage?.removeItem(ROADMAP_NIGHT_KEY); });
  it("classifies the fixed refusals", () => {
    expect(roadmapOutcome("busy")).toBe("retry");
    expect(roadmapOutcome("already_ran")).toBe("done");
    expect(roadmapOutcome("boom")).toBe("failed");
  });
  it("busy does not mark the night done, so the next tick in the window tries again", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd !== "roadmap_draft_pending") return undefined;
      if (invoke.mock.calls.filter((c) => c[0] === cmd).length === 1) throw "busy";
      return 2;
    });
    await tickRoadmapNight(at(3));
    expect(globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY) ?? null).toBeNull();
    await tickRoadmapNight(at(3, 5));
    expect(invoke.mock.calls.filter((c) => c[0] === "roadmap_draft_pending")).toHaveLength(2);
    expect(globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY)).toBe(localDayKey(at(3)));
  });
});
