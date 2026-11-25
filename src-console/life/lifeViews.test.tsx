import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { unwiredRoadmaps } from "../goals/roadmap";
import { GoalsProjects } from "./GoalsProjects";
import { HabitsView } from "./HabitsView";
import {
  goalTree, goalWeeks, habitPattern, lifeWrites, loadLife, mondayOf, progressText, weekCounts, weekLine, weekRange,
  type Goal, type LifeData, type RecommendationRow,
} from "./lifeApi";
import { currentFocus, OverviewView, todaysActions, upcoming } from "./OverviewView";
import { RecommendationCard } from "./RecommendationCard";
import { WeekPlanCard } from "./WeekPlan";

const TODAY = "2026-11-25"; // a Wednesday
const goal = (id: string, extra: Partial<Goal> = {}): Goal => ({ id, title: `Goal ${id}`, category: "personal", target: 300, current: 212, unit: "km", deadline: null, created_at: "", updated_at: "", ...extra } as Goal);
const habit = (id: string, extra: Partial<Goal> = {}): Goal => goal(id, { title: `Habit ${id}`, category: "habit", target: 7, current: 0, unit: "days", ...extra });

function data(over: Partial<LifeData> = {}): LifeData {
  return { today: TODAY, goals: [], areas: [], links: {}, progress: {}, history: {}, projects: [], tasks: [], plan: null, dates: [], activity: [], failed: [], ...over };
}
const rec = (over: Partial<RecommendationRow> = {}): RecommendationRow => ({
  recommendationId: "r1", scope: "personal", domain: "life", kind: "habit_below_target", title: "Meditate was done 1 of 5 days", body: "", observation: "1 done, 1 not done.",
  rationale: "Seeing the gap helps.", suggestedAction: "Try one breath", confidence: null, confidenceBasis: null, uncertainty: "Only recorded days count.", impact: null, effort: "low",
  status: "shown", createdAt: "", updatedAt: "", expiresAt: null, producedBy: "rules:life_week", modelTier: null, targetRef: "goals:h1", supersedesId: null, ...over,
});
const caller = (answers: Record<string, unknown> = {}) => {
  const fn = vi.fn(async (cmd: string) => {
    if (cmd in answers) { const a = answers[cmd]; if (a instanceof Error) throw a; return a; }
    return null;
  });
  return fn as unknown as (<T>(c: string, a?: Record<string, unknown>) => Promise<T>) & typeof fn;
};

describe("Life rules (no invented values)", () => {
  it("weeks start on Monday and show their dates", () => {
    expect(mondayOf("2026-11-25")).toBe("2026-11-23");
    expect(mondayOf("2026-11-29")).toBe("2026-11-23");
    expect(mondayOf("2026-11-23")).toBe("2026-11-23");
    expect(weekRange("2026-11-16")).toBe("Mon Nov 16 – Sun Nov 22");
  });
  it("a day without a record is unknown, never not-done, and future days are not counted", () => {
    const days = [{ day: "2026-11-23", state: "done" }, { day: "2026-11-24", state: "not_done" }];
    const [c] = weekCounts(days, 5, TODAY, 1);
    expect(c).toMatchObject({ done: 1, notDone: 1, unknown: 1, target: 5 });
    expect(weekLine(c)).toBe("1 of 5 days · 1 not done · 1 day with no record");
    expect(habitPattern(weekCounts([], 5, TODAY))).toMatch(/No records yet/);
  });
  it("progress is a percent only when the backend gives a fraction", () => {
    expect(progressText(undefined, goal("a"))).toBe("212 / 300 km");
    expect(progressText({ goalId: "a", status: "active", current: 212, target: 300, unit: "km", baseline: 100, fraction: 0.56, history: [] }, goal("a"))).toBe("56% · 212 / 300 km");
  });
  it("goals: progressed, stalled, steady and no data are told apart", () => {
    const monday = "2026-11-16";
    const act = (id: string, at: string, kind = "goal.progressed") => ({ activityId: `${id}${at}`, scope: "personal", kind, subjectRef: `goals:${id}`, occurredAt: `${at}T10:00:00+09:00`, recordedAt: "", source: "user", provenance: "manual", certainty: "confirmed", sourceRef: null, actor: "taisun", summary: null, dataJson: null });
    const goals = [goal("p"), goal("s", { deadline: "2026-12-20" }), goal("q", { deadline: "2027-12-01" }), goal("n")];
    const w = goalWeeks(goals, [act("p", "2026-11-18"), act("s", "2026-10-01"), act("q", "2026-11-10", "goal.created")], monday);
    expect(Object.fromEntries(w.map((g) => [g.goal.id, g.state]))).toEqual({ p: "progressed", s: "stalled", q: "steady", n: "no_data" });
  });
  it("the tree groups goals by area and resolves links; business tasks stay out", () => {
    const d = data({
      goals: [goal("a", { life: { areaId: "ar1" } as never }), goal("b"), habit("h")],
      areas: [{ id: "ar1", name: "Home", description: null, status: "active", createdAt: "", updatedAt: "" }],
      links: { a: [{ id: "l1", kind: "goal_project", goalId: "a", otherRef: "project:p1", createdAt: "" }, { id: "l2", kind: "goal_habit", goalId: "a", otherRef: "goals:h", createdAt: "" }] },
      projects: [{ id: "p1", title: "Akiya", objective: "", currentState: "", status: "active", milestones: [], nextAction: null, blockers: [], dependencies: [], contextItemIds: [], provenanceSource: "", createdAt: "", updatedAt: "", completedAt: null }],
      tasks: [{ title: "Call", status: "active", priority: null, tags: ["goal:a"], venture: null }, { title: "Biz", status: "active", priority: null, tags: ["goal:a", "venture:x"], venture: "x" }],
    });
    const t = goalTree(d);
    expect(t.map((a) => a.area?.name ?? "none")).toEqual(["Home", "none"]);
    expect(t[0].goals[0].projects[0].project?.title).toBe("Akiya");
    expect(t[0].goals[0].habits[0].habit?.id).toBe("h");
    expect(t[0].goals[0].tasks.map((x) => x.title)).toEqual(["Call"]);
  });
  it("loadLife lists a failed source instead of filling it in", async () => {
    const call = caller({ read_goals: [goal("a")], life_progress: new Error("x") });
    const d = await loadLife(call, TODAY);
    expect(d.goals).toHaveLength(1);
    expect(d.failed).toContain("life_progress");
    expect(d.progress).toEqual({});
  });
});

describe("Overview", () => {
  it("shows focus, priority, today's actions, due habits, deadlines and the one insight; Done records the habit", async () => {
    const call = caller({ life_insight: { recommendation: rec(), nudged: true, held: null }, habit_mark: { changed: true, id: "h1" }, proactive_settings_get: null });
    const d = data({
      goals: [goal("a", { deadline: "2026-12-20", life: { nextAction: "Email the agent" } as never }), habit("h", { life: { frequency: { times: 5, per: "week" } } as never })],
      plan: { weekStart: "2026-11-23", priority: "Ship the offer", priorityGoalId: null, actions: [{ text: "Visit Kamakura", goalId: null, done: false }], note: null, updatedAt: "x" },
      dates: [{ id: "d1", title: "Visa", date: "2027-01-10", lane: "residence", sub: "", leadDays: [], goalId: null, done: false, inDays: 46, reminders: [], activeWindow: null } as never],
      tasks: [{ title: "Book train", status: "active", priority: null, tags: [], venture: null }],
    });
    expect(currentFocus(d).goal?.id).toBe("a");
    expect(todaysActions(d).map((x) => x.text)).toEqual(["Visit Kamakura", "Email the agent", "Book train"]);
    expect(upcoming(d).map((x) => x.title)).toEqual(["Goal a", "Visa"]);
    render(<OverviewView data={d} writes={lifeWrites(call)} call={null} roadmapOf={() => null} />);
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeTruthy();
    expect(screen.getByText("Ship the offer")).toBeTruthy();
    expect(await screen.findByText("Meditate was done 1 of 5 days")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Done today: Habit h" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("habit_mark", { habitId: "h", day: TODAY, done: true, source: "manual" }));
    expect(await screen.findByText(/Marked “Habit h” done today/)).toBeTruthy();
    expect(call.mock.calls.filter((c) => c[0] === "life_insight")).toHaveLength(1);
  });
  it("says why nothing is suggested (quiet hours)", async () => {
    const call = caller({ life_insight: { recommendation: null, nudged: false, held: "quiet_hours" } });
    render(<OverviewView data={data()} writes={lifeWrites(call)} call={null} roadmapOf={() => null} />);
    expect(await screen.findByText(/Quiet hours are on/)).toBeTruthy();
  });
});

describe("Recommendation controls", () => {
  it("accept, then mark done, then record how it went", async () => {
    const call = caller({ recommendation_respond: "o1", recommendation_record_outcome: "o2" });
    render(<RecommendationCard rec={rec()} writes={lifeWrites(call)} />);
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("recommendation_respond", { scope: "personal", id: "r1", response: "accepted", modificationJson: null }));
    fireEvent.click(await screen.findByRole("button", { name: "Mark done" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("recommendation_record_outcome", expect.objectContaining({ id: "r1", outcome: "completed" })));
    fireEvent.click(await screen.findByRole("button", { name: "It helped" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("recommendation_record_outcome", expect.objectContaining({ outcome: "evaluated", evaluation: "helped" })));
  });
  it("modify sends what changed; dismiss is final; explain lists evidence with role, quality and window", async () => {
    const call = caller({
      recommendation_respond: "o1",
      recommendation_get: { recommendation: rec(), outcomes: [], evidence: [{ evidenceId: "e1", recordRef: "goals:h1", windowStart: "2026-11-16T00:00:00+09:00", windowEnd: "2026-11-22T23:59:59+09:00", role: "observation", quality: "self_report", note: null, capturedAt: "", snapshotSha256: null }] },
    });
    const { unmount } = render(<RecommendationCard rec={rec()} writes={lifeWrites(call)} />);
    fireEvent.click(screen.getByRole("button", { name: "Explain" }));
    const why = await screen.findByLabelText("Why Amara suggests this");
    expect(await within(why).findByText(/Observation/)).toBeTruthy();
    expect(within(why).getByText(/You recorded it · Nov 16 – Nov 22/)).toBeTruthy();
    expect(within(why).getByText(/Only recorded days count/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Modify" }));
    fireEvent.change(screen.getByLabelText("What you'll do instead"), { target: { value: "Two breaths" } });
    fireEvent.click(screen.getByRole("button", { name: "Save change" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("recommendation_respond", expect.objectContaining({ response: "modified", modificationJson: JSON.stringify({ suggestedAction: { from: "Try one breath", to: "Two breaths" } }) })));
    unmount();
    render(<RecommendationCard rec={rec({ recommendationId: "r2" })} writes={lifeWrites(call)} />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(await screen.findByText(/won't come back/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Accept" })).toBeNull();
  });
  it("a refused write says so and changes nothing", async () => {
    const call = caller({ recommendation_respond: new Error("can't go from dismissed to accepted") });
    render(<RecommendationCard rec={rec()} writes={lifeWrites(call)} />);
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(await screen.findByText(/didn't save/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Accept" })).toBeTruthy();
  });
});

describe("Weekly plan", () => {
  it("plans a week: priority, goal and actions are saved; a tick saves at once", async () => {
    const saved = { weekStart: "2026-11-23", priority: "Offer", priorityGoalId: "a", actions: [{ text: "Call", goalId: null, done: false }], note: null, updatedAt: "t" };
    const call = caller({ life_week_plan_set: saved });
    const { rerender } = render(<WeekPlanCard plan={null} monday="2026-11-23" goals={[goal("a")]} writes={lifeWrites(call)} />);
    expect(screen.getByText("No priority set for this week yet.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Plan this week" }));
    fireEvent.change(screen.getByLabelText("Primary priority"), { target: { value: "  Offer " } });
    fireEvent.change(screen.getByLabelText("It moves"), { target: { value: "a" } });
    fireEvent.click(screen.getByRole("button", { name: "Add an action" }));
    fireEvent.change(screen.getByLabelText("Action 1"), { target: { value: "Call" } });
    fireEvent.click(screen.getByRole("button", { name: "Save plan" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("life_week_plan_set", { plan: expect.objectContaining({ weekStart: "2026-11-23", priority: "Offer", priorityGoalId: "a", actions: [{ text: "Call", goalId: null, done: false }] }) }));
    rerender(<WeekPlanCard plan={saved} monday="2026-11-23" goals={[goal("a")]} writes={lifeWrites(call)} />);
    fireEvent.click(screen.getByRole("checkbox"));
    await waitFor(() => expect(call).toHaveBeenLastCalledWith("life_week_plan_set", { plan: expect.objectContaining({ actions: [{ text: "Call", goalId: null, done: true }] }) }));
  });
});

describe("Habits & routines", () => {
  it("a day on the week calendar records a mark; unknown days are labelled as such", async () => {
    const call = caller({ habit_mark: { changed: true, id: "x" }, life_update: habit("h") });
    const d = data({ goals: [habit("h", { life: { frequency: { times: 5, per: "week" } } as never })], history: { h: [{ day: "2026-11-23", state: "done" }] } });
    render(<HabitsView data={d} writes={lifeWrites(call)} />);
    expect(screen.getByText(/This week: 1 of 5 days · 2 days with no record/)).toBeTruthy();
    const tue = screen.getByRole("button", { name: /^Tue 11-24: no record\. Mark done/ });
    expect(screen.getByRole("button", { name: /^Thu 11-26: later this week/ })).toHaveProperty("disabled", true);
    fireEvent.click(tue);
    await waitFor(() => expect(call).toHaveBeenCalledWith("habit_mark", { habitId: "h", day: "2026-11-24", done: true, source: "manual" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit rhythm: Habit h" }));
    fireEvent.change(screen.getByLabelText("How often"), { target: { value: "4" } });
    fireEvent.change(screen.getByLabelText("Minimum version"), { target: { value: "one breath" } });
    fireEvent.click(screen.getByRole("button", { name: "Save habit" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("life_update", { goalId: "h", fields: expect.objectContaining({ frequency: { times: 4, per: "week" }, minimumVersion: "one breath", status: "active" }) }));
  });
});

describe("Goals & projects", () => {
  it("expands a goal, links and unlinks a project, and saves its Life details", async () => {
    const call = caller({ life_link_add: { id: "l9" }, life_link_remove: true, life_update: goal("a") });
    const project = { id: "p1", title: "Akiya", objective: "", currentState: "", status: "active", milestones: [{ title: "Viewing", status: "open", dueAt: null }], nextAction: "Call agent", blockers: [], dependencies: [], contextItemIds: [], provenanceSource: "", createdAt: "", updatedAt: "", completedAt: null };
    const d = data({ goals: [goal("a")], projects: [project, { ...project, id: "p2", title: "Garden" }], links: { a: [{ id: "l1", kind: "goal_project", goalId: "a", otherRef: "project:p1", createdAt: "" }] } });
    render(<GoalsProjects data={d} writes={lifeWrites(call)} roadmaps={unwiredRoadmaps} />);
    fireEvent.click(screen.getByRole("button", { name: /Goal a/ }));
    expect(screen.getByText(/next: Call agent · milestone: Viewing/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Project"), { target: { value: "p2" } });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("life_link_add", { kind: "goal_project", goalId: "a", otherId: "p2" }));
    fireEvent.click(screen.getByRole("button", { name: "Unlink project Akiya" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("life_link_remove", { linkId: "l1" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit details: Goal a" }));
    fireEvent.change(screen.getByLabelText("Status"), { target: { value: "blocked" } });
    fireEvent.click(screen.getByRole("button", { name: "Save details" }));
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Say what it's blocked by.");
    fireEvent.change(screen.getByLabelText("Why (paused or blocked)"), { target: { value: "Waiting on the bank" } });
    fireEvent.change(screen.getByLabelText(/Baseline/), { target: { value: "100" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save details" })); });
    expect(call).toHaveBeenCalledWith("life_update", { goalId: "a", fields: expect.objectContaining({ status: "blocked", statusReason: "Waiting on the bank", baseline: 100 }) });
  });
});
