import { describe, expect, it } from "vitest";
import { goalsFrom, makeLivePersonal, type GoalWire } from "./adapters";

const g = (o: Partial<GoalWire> & { id: string }): GoalWire => ({ title: o.id, category: "business", target: 0, current: 0, unit: "", deadline: null, ...o });

describe("goalsFrom", () => {
  it("maps milestones with right text, detail and clamped pct", () => {
    const w = goalsFrom([
      g({ id: "a", title: "Revenue", category: "money", target: 5000, current: 1820, unit: "USD", deadline: "2026-12-15" }),
      g({ id: "b", title: "Over", target: 10, current: 25, unit: "tasks" }),
      g({ id: "c", title: "Pct", target: 50, current: 20, unit: "%" }),
      g({ id: "d", title: "No target", target: 0, current: 3, unit: "kg" }),
    ]);
    const [a, b, c, d] = w.milestones;
    expect(a).toMatchObject({ detail: "money · Dec 15", right: "1820 of 5000 USD", pct: 36 });
    expect(b.pct).toBe(100);
    expect(c.right).toBe("40%");
    expect(d).toMatchObject({ pct: 0, right: "3 kg" });
  });
  it("nextMilestone: nearest deadline among incomplete, else highest pct, else null", () => {
    expect(goalsFrom([
      g({ id: "x", title: "Later", target: 10, current: 1, deadline: "2027-01-01" }),
      g({ id: "y", title: "Sooner", target: 10, current: 1, deadline: "2026-12-01" }),
      g({ id: "z", title: "Done", target: 10, current: 10, deadline: "2026-11-01" }),
    ]).nextMilestone).toBe("Sooner");
    expect(goalsFrom([g({ id: "p", title: "Low", target: 10, current: 1 }), g({ id: "q", title: "High", target: 10, current: 7 })]).nextMilestone).toBe("High");
    expect(goalsFrom([g({ id: "h", category: "habit" })]).nextMilestone).toBeNull();
  });
  it("habits never show met or missed", () => {
    const w = goalsFrom([g({ id: "h1", title: "Walk", category: "habit", target: 3, unit: "3 times a week" }), g({ id: "h2", title: "Read", category: "habit", target: 0, unit: "daily", current: 5 })]);
    expect(w.milestones).toEqual([]);
    expect(w.habits.map((h) => h.target)).toEqual([3, 7]);
    for (const h of w.habits) {
      expect(h.days).toEqual(Array(7).fill("none"));
      expect(h.done).toBe(0);
      expect(h.source).toBe("You");
      expect(h.note).toBe("No data source linked yet");
    }
  });
});

describe("live goals adapter", () => {
  const ok = (cmd: string): Promise<never> => (cmd === "read_goals" ? Promise.resolve([g({ id: "a", target: 2, current: 1 })] as never) : Promise.reject("nope"));
  it("is unwired until a successful read, and stays unwired on failure or a non-array", async () => {
    const good = makeLivePersonal(ok as never);
    expect(good.goals().state).toBe("unwired");
    await good.refresh();
    expect(good.goals().state).toBe("live");
    const bad = makeLivePersonal((() => Promise.reject("x")) as never);
    await bad.refresh();
    expect(bad.goals().state).toBe("unwired");
    const weird = makeLivePersonal(((cmd: string) => Promise.resolve(cmd === "read_goals" ? { not: "array" } : [])) as never);
    await weird.refresh();
    expect(weird.goals().state).toBe("unwired");
  });
});

describe("goalsFrom with habit marks", () => {
  const h = { id: "h1", title: "Walk", category: "habit", target: 5, current: 0, unit: "daily", deadline: null, created_at: "2026-11-10T09:00:00+09:00" };
  it("fills days, week count and today from habit_days", () => {
    const m = (day: string, done = true) => ({ habitId: "h1", day, done, source: "manual", at: "" });
    const w = goalsFrom([h], undefined, [m("2026-11-18"), m("2026-11-17"), m("2026-11-16"), m("2026-11-15"), m("other", true)], "2026-11-19");
    const x = w.habits[0];
    expect(x.days).toHaveLength(84);
    expect(x.days.slice(-5)).toEqual(["met", "met", "met", "met", "none"]);
    expect(x.done).toBe(3);
    expect(x.today).toBe("open");
    expect(x.note).toBe("3 of 5 this week");
  });
  it("an unmarked past day since creation is missed; before creation and today are none", () => {
    const x = goalsFrom([h], undefined, [], "2026-11-19").habits[0];
    expect(x.days.slice(-11)).toEqual(["none", ...Array(9).fill("missed"), "none"]);
  });
});
