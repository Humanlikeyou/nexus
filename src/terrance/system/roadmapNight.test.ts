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

import { activityBus } from "./activityBus";
import { catchUpAllowed, roadmapTurn, type CatchUpStatus } from "./roadmapNight";

const ok: CatchUpStatus = { due: true, ranToday: false, onAc: true, busy: false, idleSecs: 601 };

describe("catch-up decision (0018)", () => {
  it("is allowed when due, not run today, on AC, idle 10+ min, bus idle", () => {
    expect(catchUpAllowed(ok, false)).toBe(true);
    expect(catchUpAllowed({ ...ok, idleSecs: 600 }, false)).toBe(true);
  });
  it("is blocked by battery, recent activity, busy now, a busy bus, an already-ran day, or nothing due", () => {
    expect(catchUpAllowed({ ...ok, onAc: false }, false)).toBe(false);
    expect(catchUpAllowed({ ...ok, idleSecs: 599 }, false)).toBe(false);
    expect(catchUpAllowed({ ...ok, busy: true }, false)).toBe(false);
    expect(catchUpAllowed(ok, true)).toBe(false);
    expect(catchUpAllowed({ ...ok, ranToday: true }, false)).toBe(false);
    expect(catchUpAllowed({ ...ok, due: false }, false)).toBe(false);
  });
  it("the window stays first choice and unchanged", () => {
    expect(roadmapNightDue(at(3), null, false)).toBe(true);
    expect(roadmapNightDue(at(12), null, false)).toBe(false); // outside the window only catch-up can run
  });
  it("roadmaps get a turn even when heavy jobs took the budget, from the second skipped tick", () => {
    expect(roadmapTurn(1, 0)).toBe(true);
    expect(roadmapTurn(0, 0)).toBe(false);
    expect(roadmapTurn(0, 1)).toBe(true);
  });
});

describe("tickRoadmapNight catch-up", () => {
  const status = (over: Partial<CatchUpStatus> = {}) => ({ ...ok, ...over });
  const wire = (st: CatchUpStatus) =>
    invoke.mockImplementation(async (cmd: string) => (cmd === "roadmap_catchup_status" ? st : cmd === "roadmap_draft_pending" ? 3 : undefined));
  const drafts = () => invoke.mock.calls.filter((c) => c[0] === "roadmap_draft_pending");
  beforeEach(() => { invoke.mockReset(); globalThis.localStorage?.removeItem(ROADMAP_NIGHT_KEY); vi.spyOn(activityBus, "getActiveTasks").mockReturnValue([]); });

  it("runs out of window (noon) when a draft is due and it is safe", async () => {
    wire(status());
    await tickRoadmapNight(at(12));
    expect(drafts()).toHaveLength(1);
    expect(drafts()[0][1]).toEqual({ catchUp: true });
    expect(globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY)).toBe(localDayKey(at(12)));
  });
  it.each([
    ["battery", { onAc: false }],
    ["recent activity", { idleSecs: 120 }],
    ["already ran today (Rust)", { ranToday: true }],
    ["nothing waiting", { due: false }],
  ])("does not run out of window: %s", async (_n, over) => {
    wire(status(over));
    await tickRoadmapNight(at(12));
    expect(drafts()).toHaveLength(0);
  });
  it("does not run out of window while the activity bus is busy, and does not ask Rust", async () => {
    wire(status());
    vi.spyOn(activityBus, "getActiveTasks").mockReturnValue([{ id: "x" } as never]);
    await tickRoadmapNight(at(12));
    expect(invoke).not.toHaveBeenCalled();
  });
  it("in the window it runs without asking for catch-up", async () => {
    wire(status());
    await tickRoadmapNight(at(3));
    expect(drafts()[0][1]).toEqual({ catchUp: false });
  });
  it("Rust refusing a catch-up as not_idle retries next tick and is not marked done", async () => {
    invoke.mockImplementation(async (cmd: string) => { if (cmd === "roadmap_catchup_status") return ok; throw "not_idle"; });
    await tickRoadmapNight(at(12));
    expect(globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY) ?? null).toBeNull();
    expect(roadmapOutcome("not_idle")).toBe("retry");
  });
});
