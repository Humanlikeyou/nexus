import { describe, expect, it, vi } from "vitest";
import { makeLiveBranches, mapBranch } from "./branchesLive";

const raw = { id: "br-1", goalId: "g1", fromStepId: "s1", reasonKind: "stagnation", reasonText: "Stalled.", status: "proposed",
  alternateSteps: [{ id: "alt1", title: "Smaller", size: "small", order: 1, state: "upcoming" }] };

describe("live branch adapter", () => {
  it("maps only a proposed, well-formed suggestion", () => {
    expect(mapBranch(raw)?.alternateSteps[0].title).toBe("Smaller");
    expect(mapBranch({ ...raw, status: "accepted" })).toBeNull();
    expect(mapBranch({ ...raw, reasonKind: "other" })).toBeNull();
    expect(mapBranch({ ...raw, alternateSteps: [] })).toBeNull();
  });
  it("is unwired until the read lands, then live; accept then undo use the one write path", async () => {
    const calls: [string, unknown][] = [];
    const call = vi.fn(async (c: string, a?: Record<string, unknown>) => {
      calls.push([c, a]);
      if (c === "goal_branch_open") return calls.filter((x) => x[0] === "goal_branch_accept").length ? null : raw;
      if (c === "goal_branch_accept") return { id: "route-9", route: {} };
      return true;
    }) as never;
    const a = makeLiveBranches({ call, notify: () => {} });
    expect(a.open("g1").state).toBe("unwired");
    await vi.waitFor(() => expect(a.open("g1").state).toBe("live"));
    expect((a.open("g1") as { value: unknown }).value).not.toBeNull();
    expect((await a.accept("br-1")).state).toBe("live");
    expect(calls.some(([c, x]) => c === "goal_branch_accept" && (x as { suggestionId: string }).suggestionId === "br-1")).toBe(true);
    expect((await a.undo("br-1")).state).toBe("live");
    expect(calls).toContainEqual(["add_undo", { kind: "route", id: "route-9" }]);
  });
  it("a failed write is unwired, never a fake success", async () => {
    const a = makeLiveBranches({ call: (async (c: string) => { if (c === "goal_branch_open") return raw; throw new Error("x"); }) as never, notify: () => {} });
    a.open("g1");
    await vi.waitFor(() => expect(a.open("g1").state).toBe("live"));
    expect((await a.decline("br-1")).state).toBe("unwired");
  });
});
