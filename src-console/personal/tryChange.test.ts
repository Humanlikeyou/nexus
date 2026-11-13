import { describe, expect, it, vi } from "vitest";
import { localDatePlus, makeLivePersonal, tryChangeWith } from "./adapters";

const NOW = new Date(2026, 10, 13, 9, 0); // Fri 13 Nov 2026, local

describe("Weekly review › Try it this week (live)", () => {
  it("adds one follow-up due in 6 days through followups_add_item and never touches goals", async () => {
    const call = vi.fn().mockResolvedValue({ id: "f1" });
    const r = await tryChangeWith(call, true, "Sleep 7 h before hard days", NOW);
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("followups_add_item", { text: "Try this week: Sleep 7 h before hard days", due: "2026-11-19", next: null });
    expect(r).toEqual({ state: "live", value: { reminder: "Follow-up added for 2026-11-19: Sleep 7 h before hard days" } });
    expect(call.mock.calls.some(([cmd]) => /goal/.test(String(cmd)))).toBe(false);
  });

  it("Not this week stores nothing", async () => {
    const call = vi.fn();
    expect(await tryChangeWith(call, false, "x", NOW)).toEqual({ state: "live", value: { reminder: null } });
    expect(call).not.toHaveBeenCalled();
  });

  it("with no real suggestion there's nothing to try (stays unwired, no call)", async () => {
    const call = vi.fn();
    expect((await tryChangeWith(call, true, null, NOW)).state).toBe("unwired");
    expect(call).not.toHaveBeenCalled();
  });

  it("a cancelled approval says nothing was set, never 'Reminder set'", async () => {
    const call = vi.fn().mockRejectedValue("cancelled");
    const r = await tryChangeWith(call, true, "x", NOW);
    expect(r.state).toBe("live");
    expect(r.state === "live" && r.value.reminder).toMatch(/^Nothing was set/);
  });

  it("the live adapter uses the weekly review's own suggestion title", async () => {
    const review = {
      state: "ready", weekStart: "2026-11-02", weekEnd: "2026-11-08", dueAt: "x", computedAt: "x",
      health: { state: "none", nights: 0, avgSleepH: null, sleepDeltaH: null, avgHrv: null, avgRecovery: null, note: "n" },
      work: { state: "none", focusSessions: 0, avgFocus: null, note: "n" },
      checkins: { state: "none", count: 0, avgMood: null, avgEnergy: null, notes: [], note: "n" },
      goals: { state: "none", milestones: [], habits: 0, note: "n" },
      money: { state: "none", personalChangeUsd: null, accounts: [], note: "n" },
      suggestion: { title: "Walk after lunch", reason: "r", evidenceFindingId: "f", confidence: 3 }, suggestionNote: "", summary: null,
    };
    const call = vi.fn(async (cmd: string) => {
      if (cmd === "weekly_review_read") return review;
      if (cmd === "followups_add_item") return { id: "f1" };
      throw new Error("not in test");
    });
    const a = makeLivePersonal(call as never);
    await a.refresh();
    expect(a.weeklyReview().state).toBe("live");
    await a.tryChange(true);
    expect(call).toHaveBeenCalledWith("followups_add_item", expect.objectContaining({ text: "Try this week: Walk after lunch" }));
  });

  it("localDatePlus crosses month ends", () => {
    expect(localDatePlus(new Date(2026, 10, 28), 6)).toBe("2026-12-04");
  });
});

describe("live personal refresh", () => {
  it("a malformed reply clears only its own store; the others still load", async () => {
    const call = vi.fn(async (cmd: string) => {
      if (cmd === "weekly_review_read") return { health: { state: "data" } }; // malformed: the mapper throws
      if (cmd === "read_goals") return [];
      if (cmd === "checkin_list") return [];
      if (cmd === "life_date_list") return [];
      throw new Error("off");
    });
    const a = makeLivePersonal(call as never);
    await expect(a.refresh()).resolves.toBeUndefined();
    expect(a.weeklyReview().state).toBe("unwired");
    expect(a.goals().state).toBe("live");
    expect(a.checkins().state).toBe("live");
  });
});
