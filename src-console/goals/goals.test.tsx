import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UNWIRED, sample } from "../lib/sampleAdapters";
import { GoalRoadmap } from "./GoalRoadmap";
import { StepQuestion } from "./StepQuestion";
import { streakOf, missedYesterday, Heatmap } from "./HabitTracker";
import { makeSampleRoadmaps, unwiredRoadmaps, type Roadmap, type RoadmapAdapter, type RoadmapStep } from "./roadmap";

const step = (o: Partial<RoadmapStep>): RoadmapStep => ({ id: "a", title: "Step A", why: null, size: "small", order: 1, done: false, doneAt: null, notYetCount: 0, smallerSuggestion: null, ...o });
const live = (rm: Roadmap | null, over: Partial<RoadmapAdapter> = {}): RoadmapAdapter => ({
  ...unwiredRoadmaps, roadmap: () => ({ state: "live" as const, value: rm }),
  request: vi.fn(async () => ({ state: "live" as const, value: { queued: true as const } })),
  accept: vi.fn(async () => ({ state: "live" as const, value: { ok: true as const } })),
  editStep: vi.fn(async () => ({ state: "live" as const, value: { ok: true as const } })),
  answerStep: vi.fn(async () => ({ state: "live" as const, value: { ok: true as const } })), ...over,
});
const rm = (status: Roadmap["status"], steps: RoadmapStep[], pct = 0): Roadmap => ({ goalId: "g", status, draftedAt: null, steps, pct });

describe("GoalRoadmap states", () => {
  it("unwired: says so, no buttons", () => {
    render(<GoalRoadmap goalId="g" adapter={unwiredRoadmaps} />);
    expect(screen.getByTestId("roadmap-unwired")).toHaveTextContent("Needs wiring");
    expect(screen.queryByRole("button")).toBeNull();
  });
  it("empty: Draft a roadmap tonight calls request then confirms", async () => {
    const a = live(null);
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    await userEvent.click(screen.getByRole("button", { name: "Draft a roadmap tonight" }));
    expect(a.request).toHaveBeenCalledWith("g");
    expect(await screen.findByTestId("roadmap-drafting")).toHaveTextContent("Amara drafts it tonight, ready in the morning");
    expect(screen.queryByText("Sample data")).toBeNull();
  });
  it("request on an unwired store claims nothing", async () => {
    const a = live(null, { request: async () => UNWIRED });
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    await userEvent.click(screen.getByRole("button", { name: "Draft a roadmap tonight" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Not saved");
    expect(screen.queryByTestId("roadmap-drafting")).toBeNull();
  });
  it("drafting status shows the overnight note, no steps", () => {
    render(<GoalRoadmap goalId="g" adapter={live(rm("drafting", []))} defaultOpen />);
    expect(screen.getByTestId("roadmap-drafting")).toBeInTheDocument();
    expect(screen.queryByRole("list")).toBeNull();
  });
  it("sample + suggested: tag, own pct, size chips, Accept, smaller move", async () => {
    const a = makeSampleRoadmaps();
    render(<GoalRoadmap goalId="x" adapter={a} defaultOpen />);
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    expect(screen.getAllByText("13%").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Medium")).toHaveLength(2);
    expect(screen.getByText(/A smaller first move: Email the supplier/)).toBeInTheDocument();
    const accept = vi.spyOn(a, "accept");
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(accept).toHaveBeenCalledWith("x");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Accept" })).toBeNull());
  });
  it("live accepted: no sample tag, Yes calls answerStep and ticks, pct follows weights", async () => {
    const a = live(rm("accepted", [step({ id: "a", size: "small" }), step({ id: "b", title: "Step B", size: "medium", order: 2 })], 0));
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByRole("button", { name: "Accept" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Yes, I did: Step B" }));
    expect(a.answerStep).toHaveBeenCalledWith("g", "b", "yes");
    expect((await screen.findAllByText("67%")).length).toBeGreaterThan(0);
  });
  it("Not yet calls answerStep with not_yet", async () => {
    const a = live(rm("accepted", [step({})]));
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    await userEvent.click(screen.getByRole("button", { name: "Not yet: Step A" }));
    expect(a.answerStep).toHaveBeenCalledWith("g", "a", "not_yet");
    expect(await screen.findByRole("status")).toHaveTextContent("ask again in a few days");
  });
  it("inline edit sends title and size", async () => {
    const a = live(rm("accepted", [step({})]));
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    await userEvent.click(screen.getByRole("button", { name: "Edit step: Step A" }));
    const t = screen.getByLabelText("Step");
    await userEvent.clear(t); await userEvent.type(t, "Renamed");
    await userEvent.selectOptions(screen.getByLabelText("Size"), "big");
    await userEvent.click(screen.getByRole("button", { name: "Save step" }));
    expect(a.editStep).toHaveBeenCalledWith("g", "a", { title: "Renamed", size: "big" });
    expect(await screen.findByText("Renamed")).toBeInTheDocument();
  });
  it("all done: celebration, no Yes / Not yet", () => {
    render(<GoalRoadmap goalId="g" adapter={live(rm("accepted", [step({ done: true })], 100))} defaultOpen />);
    expect(screen.getByText(/All steps done\. Nice work/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Yes/ })).toBeNull();
  });
  it("a failing save says it in plain words", async () => {
    const a = live(rm("accepted", [step({})]), { answerStep: async () => { throw new Error("ECONNRESET 127.0.0.1"); } });
    render(<GoalRoadmap goalId="g" adapter={a} defaultOpen />);
    await userEvent.click(screen.getByRole("button", { name: "Yes, I did: Step A" }));
    expect(await screen.findByRole("status")).toHaveTextContent("That didn't save");
    expect(screen.queryByText(/ECONNRESET/)).toBeNull();
  });
});

describe("habit helpers", () => {
  it("never invents a streak", () => {
    expect(streakOf(["none", "none"])).toBeNull();
    expect(streakOf(["met", "met", "none"])).toBe(2);
    expect(streakOf(["met", "missed", "met"])).toBe(1);
    expect(missedYesterday(["met", "missed", "none"])).toBe(true);
    expect(missedYesterday(["met", "none", "none"])).toBe(false);
  });
  it("heatmap is 84 cells and honest when empty", () => {
    const { container } = render(<Heatmap days={Array(7).fill("none")} label="Run" />);
    expect(container.querySelectorAll("i")).toHaveLength(84);
    expect(container.querySelectorAll("i[data-m='met']")).toHaveLength(0);
    expect(screen.getByRole("img")).toHaveAttribute("aria-label", expect.stringContaining("no data yet"));
  });
});

describe("StepQuestion", () => {
  it("asks and answers", async () => {
    const a = makeSampleRoadmaps(); const spy = vi.spyOn(a, "answerStep");
    render(<StepQuestion adapter={a} />);
    expect(screen.getByText(/Have you done: Order three samples/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Yes" }));
    expect(spy).toHaveBeenCalledWith("sample-goal", "s2", "yes");
  });
  it("renders nothing when unwired or no question", () => {
    expect(render(<StepQuestion adapter={unwiredRoadmaps} />).container).toBeEmptyDOMElement();
    expect(render(<StepQuestion adapter={{ ...unwiredRoadmaps, todaysQuestion: () => sample(null) }} />).container).toBeEmptyDOMElement();
  });
});
