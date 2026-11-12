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
