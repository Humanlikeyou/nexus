import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { move, duration, itemFor, prescription, type Exercise, type SessionView, type Template, type WorkoutCalls } from "./workoutApi";
import { WorkoutView } from "./WorkoutView";

const ex = (id: string, name: string, family: string, level: number, measure: "reps" | "hold"): Exercise => ({
  id, name, family, level, measure, difficulty: "beginner", description: `${name} description`, instructions: [`Do ${name}.`], muscles: ["lats"], equipment: family === "pull" ? ["pull-up bar"] : [],
  sets: 3, reps: measure === "reps" ? 8 : null, holdS: measure === "hold" ? 30 : null, secsPerRep: 3, form: [`${name} form`], safety: [`${name} safety`], demoAsset: `demo:${id}`, custom: false,
});
const LIB = [ex("pull-up", "Pull-up", "pull", 5, "reps"), ex("plank", "Plank", "core", 1, "hold"), ex("dead-hang", "Dead hang", "pull", 1, "hold")];

function view(state: SessionView["session"]["state"], over: Partial<SessionView["session"]> = {}, sum: Partial<SessionView["summary"]> = {}): SessionView {
  return {
    session: { id: "ws-1", templateId: "wt-1", templateName: "Pull day", source: "user", sourceRef: null, plan: [{ exerciseId: "pull-up", sets: 2, reps: 6, holdS: null, loadKg: null, restS: 90 }], restBetweenS: 90,
      state, plannedAt: "2026-11-26T10:00:00+09:00", startedAt: state === "planned" ? null : "2026-11-26T10:00:00+09:00", endedAt: null, pauses: [], sets: [], nextItem: 0, nextSet: 0, engine: "workout-engine-v1-0062", offerSid: null, ...over },
    summary: { plannedSets: 2, doneSets: 0, skippedSets: 0, completionPct: 0, totalReps: 0, totalHoldS: 0, loadVolumeKg: 0, setsByMuscle: {}, activeS: null, pausedS: 0, restS: 0, estimatedS: 192, ...sum },
  };
}

function calls(over: Partial<WorkoutCalls> = {}): WorkoutCalls & Record<string, ReturnType<typeof vi.fn>> {
  const t: Template = { id: "wt-1", name: "Pull day", source: "user", sourceRef: null, items: [{ exerciseId: "pull-up", sets: 2, reps: 6, holdS: null, loadKg: null, restS: 90 }], restBetweenS: 90, archived: false, createdAt: "", updatedAt: "" };
  return {
    library: vi.fn(async () => ({ version: "calisthenics-v1-0062", exercises: LIB })),
    templates: vi.fn(async () => [t]),
    saveTemplate: vi.fn(async (x: Template) => ({ ...x, id: "wt-2" })),
    estimate: vi.fn(async () => 312),
    plan: vi.fn(async () => view("planned")),
    act: vi.fn(async () => view("active")),
    history: vi.fn(async () => []),
    fromTake: vi.fn(async () => ({ ...t, id: "wt-a", source: "amara" as const, sourceRef: "take:pushup_ladder_2", name: "Push-up ladder, level 2" })),
    takeIds: vi.fn(async () => [{ takeId: "pushup_ladder_2", name: "Push-up ladder, level 2", estimatedS: 252 }]),
    ...over,
  } as never;
}

describe("decision 0062: workout helpers", () => {
  it("formats, prescribes and reorders deterministically", () => {
    expect(duration(45)).toBe("45 s");
    expect(duration(312)).toBe("5 min");
    expect(duration(3900)).toBe("1 h 5 min");
    expect(duration(null)).toBe("Not recorded");
    expect(prescription({ exerciseId: "pull-up", sets: 3, reps: 6, holdS: null, loadKg: 10, restS: 90 })).toBe("3 × 6 +10 kg · rest 90 s");
    expect(itemFor(LIB[1])).toEqual({ exerciseId: "plank", sets: 3, reps: null, holdS: 30, loadKg: null, restS: 60 });
    expect(move([1, 2, 3], 0, 1)).toEqual([2, 1, 3]);
    expect(move([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
  });
});

describe("decision 0062: Workout view", () => {
  it("browses the library and shows an exercise's details, form, safety and demonstration reference", async () => {
    render(<WorkoutView calls={calls()} />);
    fireEvent.click(await screen.findByRole("radio", { name: "Library" }));
    fireEvent.click(within(screen.getByRole("list", { name: "Exercises" })).getByRole("button", { name: "Plank" }));
    expect(screen.getByRole("list", { name: "How to do Plank" })).toHaveTextContent("Do Plank.");
    expect(screen.getByRole("list", { name: "Plank safety" })).toHaveTextContent("Plank safety");
    expect(screen.getByText(/Demonstration: not available yet \(demo:plank\)/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Family"), { target: { value: "pull" } });
    expect(within(screen.getByRole("list", { name: "Exercises" })).queryByRole("button", { name: "Plank" })).toBeNull();
  });

  it("builds a workout, reorders it, shows the engine's estimate and saves", async () => {
    const c = calls();
    render(<WorkoutView calls={c} />);
    fireEvent.click(await screen.findByRole("radio", { name: "Build" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Bar day" } });
    fireEvent.change(screen.getByLabelText("Add an exercise"), { target: { value: "pull-up" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(screen.getByLabelText("Add an exercise"), { target: { value: "plank" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("button", { name: "Move Plank 2 up" }));
    const order = screen.getByRole("list", { name: "Workout order" });
    expect(within(order).getAllByRole("listitem")[0]).toHaveTextContent("Plank");
    await screen.findByText("Estimated length: 5 min");
    expect(c.estimate).toHaveBeenLastCalledWith([expect.objectContaining({ exerciseId: "plank" }), expect.objectContaining({ exerciseId: "pull-up" })], 90);
    fireEvent.click(screen.getByRole("button", { name: "Save workout" }));
    await waitFor(() => expect(c.saveTemplate).toHaveBeenCalledWith(expect.objectContaining({ name: "Bar day", source: "user", id: "" })));
  });

  it("starts, logs, pauses, skips and ends a workout through the engine", async () => {
    const c = calls({ act: vi.fn(async (_id: string, a: { kind: string }) => (a.kind === "pause" ? view("paused") : a.kind === "end" ? view("partially_completed", { endedAt: "2026-11-26T10:20:00+09:00" }, { doneSets: 1, completionPct: 50, totalReps: 6, activeS: 1200 }) : view("active"))) as never });
    render(<WorkoutView calls={c} />);
    fireEvent.click(await screen.findByRole("button", { name: "Start Pull day" }));
    expect(c.plan).toHaveBeenCalledWith("wt-1");
    fireEvent.click(await screen.findByRole("button", { name: "Start" }));
    await screen.findByText(/Pull-up/, { selector: "b" });
    fireEvent.change(screen.getByLabelText("Reps done"), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText("Added load kg"), { target: { value: "10" } });
    fireEvent.click(screen.getByRole("button", { name: "Log set" }));
    await waitFor(() => expect(c.act).toHaveBeenCalledWith("ws-1", { kind: "logSet", reps: 5, loadKg: 10 }));
    fireEvent.click(screen.getByRole("button", { name: "Skip set" }));
    await waitFor(() => expect(c.act).toHaveBeenCalledWith("ws-1", { kind: "skip", scope: "set" }));
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(await screen.findByRole("button", { name: "Resume" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "End workout" }));
    const sum = await screen.findByRole("region", { name: "Workout summary" });
    expect(sum).toHaveTextContent("Partly completed: 1 of 2 sets (50%)");
    expect(sum).toHaveTextContent("active 20 min");
  });

  it("starts one of Amara's workouts on the same engine", async () => {
    const c = calls();
    render(<WorkoutView calls={c} />);
    const list = await screen.findByRole("list", { name: "Amara's workouts" });
    expect(list).toHaveTextContent("Push-up ladder, level 2 · about 4 min");
    fireEvent.click(within(list).getByRole("button", { name: "Start Push-up ladder, level 2" }));
    await waitFor(() => expect(c.plan).toHaveBeenCalledWith("wt-a"));
    expect(c.fromTake).toHaveBeenCalledWith("pushup_ladder_2");
  });

  it("an engine refusal is shown, never hidden", async () => {
    render(<WorkoutView calls={calls({ plan: vi.fn(async () => { throw "Another workout is still open. Finish or end it first."; }) as never })} />);
    fireEvent.click(await screen.findByRole("button", { name: "Start Pull day" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Another workout is still open");
  });

  it("history lists each session's state and totals", async () => {
    render(<WorkoutView calls={calls({ history: vi.fn(async () => [view("completed", { endedAt: "x" }, { doneSets: 2, totalReps: 12, activeS: 600 })]) as never })} />);
    fireEvent.click(await screen.findByRole("radio", { name: "History" }));
    expect(screen.getByRole("list", { name: "Workout history" })).toHaveTextContent("Pull day · Completed · 2/2 sets · 12 reps · active 10 min");
  });
});
