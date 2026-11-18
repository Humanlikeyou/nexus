import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GoalsSection } from "../../personal/TodaySections";
import { makeSamplePersonal, type GoalWire } from "../../personal/adapters";
import { makeLiveGoalStore, resetGoalStores } from "../goalStore";

describe("design-1118a Goals flow: store writes", () => {
  const rows: GoalWire[] = [
    { id: "g1", title: "Revenue", category: "business", target: 5000, current: 1800, unit: "USD", deadline: null },
    { id: "h1", title: "Read", category: "habit", target: 5, current: 0, unit: "20 min", deadline: null },
  ];
  it("edits write the whole list back through save_goals, keeping progress", async () => {
    const calls: Array<[string, unknown]> = [];
    const call = vi.fn(async (c: string, a?: Record<string, unknown>) => { calls.push([c, a]); return (c === "read_goals" ? rows : null) as never; });
    const s = makeLiveGoalStore(call, () => undefined);
    const r = await s.updateGoal("g1", { title: "Revenue $6k", target: 6000, unit: "USD", deadline: "2027-01-31" });
    expect(r.state).toBe("live");
    const saved = calls.find(([c]) => c === "save_goals")![1] as { goals: GoalWire[] };
    expect(saved.goals).toEqual([{ ...rows[0], title: "Revenue $6k", target: 6000, deadline: "2027-01-31" }, rows[1]]);
    await s.updateHabit("h1", { title: "Read at night", rule: "20 min before bed", perWeek: 9 });
    const saved2 = calls.filter(([c]) => c === "save_goals")[1][1] as { goals: GoalWire[] };
    expect(saved2.goals[1]).toMatchObject({ id: "h1", title: "Read at night", category: "habit", unit: "20 min before bed", target: 7 });
  });
  it("a new goal is appended and queued for route planning; a new habit is a habit row", async () => {
    const calls: Array<[string, Record<string, unknown> | undefined]> = [];
    const call = vi.fn(async (c: string, a?: Record<string, unknown>) => { calls.push([c, a]); return (c === "read_goals" ? rows : null) as never; });
    const s = makeLiveGoalStore(call, () => undefined);
    const r = await s.addGoal({ title: " Run a half marathon ", target: 21, unit: "km", deadline: null });
    expect(r.state).toBe("live");
    const id = r.state === "live" ? r.value.id : "";
    const saved = (calls.find(([c]) => c === "save_goals")![1] as { goals: GoalWire[] }).goals;
    expect(saved).toHaveLength(3);
    expect(saved[2]).toMatchObject({ id, title: "Run a half marathon", target: 21, current: 0, unit: "km" });
    expect(calls.some(([c, a]) => c === "roadmap_request" && a?.goalId === id)).toBe(true);
    await s.addHabit({ title: "Stretch", rule: "10 min", perWeek: 4 });
    const last = (calls.filter(([c]) => c === "save_goals")[1][1] as { goals: GoalWire[] }).goals;
    expect(last[2]).toMatchObject({ title: "Stretch", category: "habit", target: 4 });
  });
  it("delete goes through delete_goal; a failing store saves nothing and says so", async () => {
    const call = vi.fn(async (c: string) => (c === "delete_goal" ? [rows[1]] : null) as never);
    expect((await makeLiveGoalStore(call, () => undefined).remove("g1")).state).toBe("live");
    expect(call).toHaveBeenCalledWith("delete_goal", { goalId: "g1" });
    const broken = makeLiveGoalStore(vi.fn(async () => { throw new Error("no"); }), () => undefined);
    expect((await broken.updateGoal("g1", { title: "x", target: 0, unit: "", deadline: null })).state).toBe("unwired");
    expect((await broken.remove("g1")).state).toBe("unwired");
  });
});

describe("design-1118a Goals flow: the page", () => {
  beforeEach(() => { localStorage.setItem("nexus.sampleAdapters", "1"); resetGoalStores(); });
  afterEach(() => { cleanup(); localStorage.removeItem("nexus.sampleAdapters"); resetGoalStores(); });
  const page = () => render(<GoalsSection adapter={makeSamplePersonal()} goals={[]} composer={<div data-testid="habit-composer">habit composer</div>} />);

  it("has the helper line and one Add button that offers Goal or Habit, each explained", () => {
    page();
    expect(screen.getByTestId("goals-helper").textContent).toMatch(/milestones.*stations.*next step.*Habits.*feed your goals/);
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    const menu = screen.getByRole("group", { name: "Add a goal or a habit" });
    expect(within(menu).getByText(/own path, with milestones as stations/)).toBeTruthy();
    expect(within(menu).getByText(/shows under the map and feeds your goals/)).toBeTruthy();
    fireEvent.click(within(menu).getByRole("button", { name: /^Habit/ }));
    expect(screen.getByTestId("habit-composer")).toBeTruthy();
  });

  it("a new goal gets its own path card with a line and stations", async () => {
    page();
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("button", { name: /^Goal/ }));
    fireEvent.change(screen.getByLabelText("Goal"), { target: { value: "Run a half marathon" } });
    fireEvent.change(screen.getByLabelText("Size (target)"), { target: { value: "21" } });
    fireEvent.click(screen.getByRole("button", { name: "Add goal" }));
    const grid = await screen.findByTestId("milestones-grid");
    const card = await within(grid).findByRole("button", { name: /Open Run a half marathon/ });
    expect(card.querySelector(".d18-path__track")).toBeTruthy();
    expect(card.querySelectorAll(".d18-st").length).toBeGreaterThan(1);
  });

  it("every goal card has an Edit pencil; edits save; delete asks first", async () => {
    page();
    fireEvent.click(screen.getByRole("button", { name: "See all paths" }));
    const grid = screen.getByTestId("milestones-grid");
    const cards = within(grid).getAllByRole("button", { name: /^Open / });
    const pencils = within(grid).getAllByRole("button", { name: /^Edit / });
    expect(pencils).toHaveLength(cards.length);
    fireEvent.click(within(grid).getByRole("button", { name: "Edit Taiyo Skin launch" }));
    const form = screen.getByRole("form", { name: /Edit goal/ });
    expect(within(form).getAllByLabelText(/^Station \d/).length).toBeGreaterThan(0);
    fireEvent.change(within(form).getByLabelText("Goal"), { target: { value: "Taiyo Skin launch v2" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save goal" }));
    await waitFor(() => expect(within(screen.getByTestId("milestones-grid")).getByRole("button", { name: /Open Taiyo Skin launch v2/ })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Edit Taiyo Skin launch v2" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("alertdialog", { name: "Delete this goal?" })).toBeTruthy();
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: /Open Taiyo Skin launch v2/ })).toBeNull());
  });

  it("habits have edit too", async () => {
    page();
    fireEvent.click(screen.getByRole("button", { name: "Edit Workouts" }));
    const form = screen.getByRole("form", { name: /Edit habit/ });
    fireEvent.change(within(form).getByLabelText("Habit"), { target: { value: "Strength workouts" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save habit" }));
    await waitFor(() => expect(screen.getByText("Strength workouts")).toBeTruthy());
  });
});
