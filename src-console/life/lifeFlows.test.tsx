import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { goalOf, goalRoute } from "../app/routes";
import { GoalsCard, goalProgress } from "../screens/GoalsCard";
import { goalsFrom, tryChangeWith } from "../personal/adapters";
import { StepQuestion } from "../goals/StepQuestion";
import { MilestonesMap } from "../design1118/GoalPath";
import { makeSampleRoadmaps, type RoadmapAdapter } from "../goals/roadmap";
import { planNowNote } from "../goals/roadmapLive";
import { GoalsThisWeekCard, LastTryCard } from "./WeekReviewExtras";
import { UNWIRED, sample } from "../lib/sampleAdapters";

describe("Life workflow gaps", () => {
  it("goal routes round-trip", () => {
    expect(goalOf(goalRoute("g1"))).toBe("g1");
    expect(goalOf({ id: "life", view: "goals" })).toBeNull();
  });
  it("progress uses the roadmap when there is one (goalsFrom and GoalsCard)", () => {
    const rows = [{ id: "g1", title: "A", category: "personal", target: 25, current: 17, unit: "tasks" }];
    expect(goalsFrom(rows).milestones[0].pct).toBe(68);
    const w = goalsFrom(rows, () => ({ pct: 11, done: 1, total: 9 }));
    expect(w.milestones[0].pct).toBe(11);
    expect(w.milestones[0].right).toContain("1 of 9 steps");
    const goal = { id: "g1", title: "A", category: "personal", target: 25, current: 17, unit: "tasks", deadline: null };
    expect(goalProgress(goal)).toBe(68);
    expect(goalProgress(goal, () => ({ pct: 11, done: 1, total: 9 }))).toBe(11);
    render(<GoalsCard goals={[goal]} roadmapOf={() => ({ pct: 11, done: 1, total: 9 })} />);
    expect(screen.getByText("1 of 9 steps")).toBeTruthy();
  });
  it("try-it keeps its reason and finding id in the follow-up", async () => {
    const call = vi.fn(async (c: string) => (c === "followups_add_item" ? { id: 7 } : {})) as never;
    await tryChangeWith(call, true, "Sleep more", new Date(2026, 10, 19), "sleep_focus", { reason: "After 7 h+ focus is higher.", findingId: "sleep_focus" });
    const args = (call as unknown as ReturnType<typeof vi.fn>).mock.calls[0][1] as { next: string };
    expect(args.next).toContain("Why: After 7 h+");
    expect(args.next).toContain("Finding: sleep_focus");
  });
  it("step question: Yes shows Undo and Open goal", async () => {
    const base = makeSampleRoadmaps();
    const undoStepWrite = vi.fn(async () => true);
    const a = { ...base, undoStepWrite } as unknown as RoadmapAdapter;
    const open = vi.fn();
    render(<StepQuestion adapter={a} onOpenGoal={open} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Yes" })); });
    fireEvent.click(screen.getByRole("button", { name: "Open goal ›" }));
    expect(open).toHaveBeenCalledWith("sample-goal");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Undo" })); });
    expect(undoStepWrite).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Yes" })).toBeTruthy();
  });
  it("a goal opened by link is in focus; a smaller step can be used; a venture chip opens its page", async () => {
    const base = makeSampleRoadmaps();
    const editStep = vi.fn(async () => sample({ ok: true as const }));
    const a: RoadmapAdapter = { ...base, editStep };
    const venture = vi.fn();
    render(<MilestonesMap goals={[{ id: "g1", title: "Launch", ventureId: "taiyo-skin" }]} adapter={a} focusGoal="g1" onOpenVenture={venture} />);
    expect(screen.getByTestId("milestones-focus")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Taiyo skin venture page/i }));
    expect(venture).toHaveBeenCalledWith("taiyo-skin");
    expect(screen.getByTestId("smaller-step").textContent).toContain("Email the supplier");
  });
  it("plan-now words are fixed", () => {
    expect(planNowNote({ err: "not_idle" })).toMatch(/on power/);
    expect(planNowNote({ ok: 1 })).toMatch(/Planned/);
  });
  it("Goals this week lists finished steps and stalled goals with Open goal", async () => {
    const open = vi.fn();
    const load = async () => ({ weekStart: "", weekEnd: "", prevWeekStart: "", goals: [{ goalId: "g1", title: "Alpha", thisWeek: 2, prevWeek: 0, refs: [] }], stalled: [{ goalId: "g2", title: "Beta", lastFinishedDay: null, daysSince: 30, refs: [] }], inCommon: { state: "nothing_stands_out_yet" as const, goalsConsidered: 0, goals: [], skippedCauses: [] } });
    render(<GoalsThisWeekCard onOpenGoal={open} load={load} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open goal: Beta" }));
    expect(open).toHaveBeenCalledWith("g2");
    expect(screen.getByText(/2 steps finished/)).toBeTruthy();
  });
  it("Last week's try shows before/after and no-clear-difference", () => {
    const side = (m: number | null) => ({ from: "a", to: "b", n: 5, mean: m, days: [] });
    const better = { followupId: 1, startDay: "2026-11-09", measure: "focus_mean", outcome: "better" as const, label: "better", reason: null, p: 0.01, before: side(60), after: side(72) };
    const flat = { ...better, followupId: 2, outcome: "no_clear_difference" as const, label: "no clear difference", reason: "not_significant" as const, before: side(60), after: side(61) };
    render(<LastTryCard experiments={[better, flat]} titles={async () => new Map([[1, "Sleep more"]])} />);
    expect(screen.getByText(/before 60 \(5 days\) → after 72/)).toBeTruthy();
    expect(screen.getByText(/No clear difference/)).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Make it a habit" })).toHaveLength(1);
  });
});
void UNWIRED;
