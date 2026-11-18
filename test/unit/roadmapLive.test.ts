// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import { localDay, makeLiveRoadmaps, mapQuestion, mapRoadmap } from "../../src-console/goals/roadmapLive";

const raw = {
  goalId: "g1", status: "suggested", draftedAt: "2026-11-12T03:00:00+09:00", pct: 43,
  steps: [
    { id: "s2", title: "B", why: null, size: "medium", order: 2, done: false, doneAt: null, notYetCount: 0, smallerSuggestion: null, lastAsked: "2026-11-12" },
    { id: "s1", title: "A", why: "w", size: "small", order: 1, done: true, doneAt: "x", notYetCount: 1, smallerSuggestion: null },
    { id: "bad", title: "C", size: "huge", order: 3 },
  ],
};
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("roadmap live adapter", () => {
  it("maps and sorts steps, dropping malformed ones", () => {
    const r = mapRoadmap(raw)!;
    expect(r.steps.map((s) => s.id)).toEqual(["s1", "s2"]);
    expect(r.pct).toBe(43);
    expect(mapRoadmap({ goalId: "g", status: "weird" })).toBeNull();
    expect(mapRoadmap(null)).toBeNull();
    expect(mapQuestion({ goalId: "g" })).toBeNull();
  });

  it("is unwired until the first read lands, then live; null roadmap is live(null)", async () => {
    const call = vi.fn(async (c: string) => (c === "roadmap_get" ? raw : null)) as never;
    const a = makeLiveRoadmaps({ call, today: () => "2026-11-12", notify: () => undefined });
    expect(a.roadmap("g1").state).toBe("unwired");
    await flush();
    const w = a.roadmap("g1");
    expect(w.state === "live" && w.value?.steps.length).toBe(2);
    const none = makeLiveRoadmaps({ call: (async () => null) as never, today: () => "d", notify: () => undefined });
    none.roadmap("x");
    await flush();
    expect(none.roadmap("x")).toEqual({ state: "live", value: null });
  });

  it("a failing backend stays unwired and writes report unwired", async () => {
    const a = makeLiveRoadmaps({ call: (async () => { throw new Error("x"); }) as never, today: () => "d", notify: () => undefined });
    a.roadmap("g1");
    a.todaysQuestion();
    await flush();
    expect(a.roadmap("g1").state).toBe("unwired");
    expect(a.todaysQuestion().state).toBe("unwired");
    expect((await a.answerStep("g1", "s1", "yes")).state).toBe("unwired");
  });

  it("writes call the right commands and refresh", async () => {
    const calls: Array<[string, unknown]> = [];
    const call = (async (c: string, args: unknown) => { calls.push([c, args]); return c === "roadmap_get" ? raw : null; }) as never;
    const a = makeLiveRoadmaps({ call, today: () => "2026-11-12", notify: () => undefined });
    expect((await a.answerStep("g1", "s2", "not_yet")).state).toBe("live");
    expect(calls[0]).toEqual(["roadmap_answer_step", { goalId: "g1", stepId: "s2", answer: "not_yet" }]);
    await a.editStep("g1", "s2", { size: "big" });
    expect(calls.find(([c]) => c === "goal_step_edit")![1]).toEqual({ goalId: "g1", stepId: "s2", title: null, size: "big", via: "typed" });
    expect(calls.some(([c]) => c === "roadmap_edit_step")).toBe(false);
    expect((await a.request("g2")).state).toBe("live");
    expect(calls.some(([c]) => c === "roadmap_get")).toBe(true);
  });

  it("Yes uses goal_step_complete and Undo calls add_undo(route, historyId)", async () => {
    const calls: Array<[string, unknown]> = [];
    const call = (async (c: string, args: unknown) => {
      calls.push([c, args]);
      if (c === "goal_step_complete" || c === "goal_step_edit") return { id: "h-42", route: {} };
      if (c === "add_undo") return true;
      return c === "roadmap_get" ? raw : null;
    }) as never;
    const a = makeLiveRoadmaps({ call, today: () => "2026-11-12", notify: () => undefined });
    expect(await a.undoStepWrite("g1", "s2")).toBe(false); // nothing logged yet
    expect((await a.answerStep("g1", "s2", "yes")).state).toBe("live");
    expect(calls[0]).toEqual(["goal_step_complete", { goalId: "g1", stepId: "s2", via: "typed" }]);
    expect(calls.some(([c]) => c === "roadmap_answer_step")).toBe(false);
    expect(await a.undoStepWrite("g1", "s2")).toBe(true);
    expect(calls.find(([c]) => c === "add_undo")![1]).toEqual({ kind: "route", id: "h-42" });
    expect(await a.undoStepWrite("g1", "s2")).toBe(false); // one Undo per write
  });

  it("a refused logged write (e.g. step already done) reports unwired and keeps no Undo id", async () => {
    const call = (async (c: string) => { if (c === "goal_step_complete") throw "route: that step can't be completed"; return null; }) as never;
    const a = makeLiveRoadmaps({ call, today: () => "d", notify: () => undefined });
    expect((await a.answerStep("g1", "s1", "yes")).state).toBe("unwired");
    expect(await a.undoStepWrite("g1", "s1")).toBe(false);
  });

  it("todays question is cached per day and re-read on a new day", async () => {
    let day = "2026-11-12";
    const q = { goalId: "g1", goalTitle: "G", stepId: "s2", stepTitle: "B", askedOn: "2026-11-12" };
    const call = vi.fn(async () => q) as never;
    const a = makeLiveRoadmaps({ call, today: () => day, notify: () => undefined });
    a.todaysQuestion();
    await flush();
    expect(a.todaysQuestion()).toEqual({ state: "live", value: q });
    a.todaysQuestion();
    expect((call as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1);
    day = "2026-11-13";
    expect(a.todaysQuestion().state).toBe("unwired");
    await flush();
    expect((call as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBe(2);
  });

  it("localDay formats local dates", () => {
    expect(localDay(new Date(2026, 10, 5))).toBe("2026-11-05");
  });
});
