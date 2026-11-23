import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TakeStepQuestion } from "./TakeStepQuestion";
import { stepQuestionStore } from "./stepQuestionStore";
import { UNWIRED } from "../lib/sampleAdapters";
import { live } from "../lib/localStores";
import { makeSampleRoadmaps, unwiredRoadmaps, type RoadmapAdapter } from "./roadmap";
import { lifeGoalFacts } from "../amaraTake/facts";

const q = { goalId: "g1", goalTitle: "Run a half marathon", stepId: "s1", stepTitle: "Book the race", askedOn: "2026-11-12" };
/** A live-like adapter: null after the answer, pct read from the roadmap. */
function liveAdapter(pct = 40): RoadmapAdapter & { answerStep: ReturnType<typeof vi.fn> } {
  let answered = false;
  const answerStep = vi.fn(async () => { answered = true; return { state: "live" as const, value: { ok: true as const } }; });
  return {
    ...unwiredRoadmaps, answerStep,
    todaysQuestion: () => live(answered ? null : q),
    roadmap: () => live({ goalId: "g1", status: "accepted" as const, draftedAt: null, steps: [], pct }),
  } as never;
}

beforeEach(() => { stepQuestionStore.reset(); });

describe("take step question", () => {
  it("shows once across two takes", () => {
    const a = liveAdapter();
    render(<><TakeStepQuestion adapter={a} /><TakeStepQuestion adapter={a} /></>);
    expect(screen.getAllByLabelText("Step question")).toHaveLength(1);
    expect(screen.getByText("Have you done: Book the race?")).toBeInTheDocument();
  });
  it("Yes shows the adapter's pct and the question is gone from every take", async () => {
    const a = liveAdapter(40);
    render(<><TakeStepQuestion adapter={a} /><TakeStepQuestion adapter={a} /></>);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Yes" })); });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Run a half marathon is now 40% of the roadmap done."));
    expect(a.answerStep).toHaveBeenCalledWith("g1", "s1", "yes");
    expect(screen.queryByRole("button", { name: "Yes" })).toBeNull();
    expect(screen.getAllByLabelText("Step question")).toHaveLength(1); // the confirmation only, not a second question
  });
  it("a take that mounts after the answer shows nothing (sample adapter never returns null)", async () => {
    const a = makeSampleRoadmaps();
    const first = render(<TakeStepQuestion adapter={a} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Not yet" })); });
    first.unmount();
    render(<TakeStepQuestion adapter={a} />);
    expect(screen.queryByLabelText("Step question")).toBeNull();
  });
  it("sample state is tagged; unwired shows nothing", () => {
    const { unmount } = render(<TakeStepQuestion adapter={makeSampleRoadmaps()} />);
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    unmount();
    render(<TakeStepQuestion adapter={{ ...unwiredRoadmaps, todaysQuestion: () => UNWIRED }} />);
    expect(screen.queryByLabelText("Step question")).toBeNull();
  });
});

describe("lifeGoalFacts", () => {
  it("maps roadmap pct and milestone text to goal_<id> facts, only numbers in the data", () => {
    const set = lifeGoalFacts([
      { id: "a", title: "Launch", right: "2 of 5 products", pct: 40, roadmapPct: 25 },
      { id: "b", title: "Run", right: "3 of 10 runs", pct: 30, roadmapPct: null },
    ], "2026-11-12")!;
    expect(set.page).toBe("life");
    expect(set.facts.map((f) => [f.key, f.label, f.display])).toEqual([["goal_a", "Launch", "25% of the roadmap done"], ["goal_b", "Run", "3 of 10 runs"]]);
    expect(set.facts[1].accept).toEqual(expect.arrayContaining([30, 3, 10]));
    expect(lifeGoalFacts([], "2026-11-12")).toBeNull();
  });
});
