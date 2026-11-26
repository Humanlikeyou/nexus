import { describe, expect, it } from "vitest";
import { lifeAdminFrom, type LifeDateWire } from "../personal/adapters";
import { parseLeadDays } from "./DatesView";

const row = (over: Partial<LifeDateWire>): LifeDateWire => ({
  id: "a", title: "Visa", date: "2027-09-14", lane: "residence", sub: "", inDays: 20,
  reminders: [{ days: 90, on: "2027-06-16", passed: true }, { days: 30, on: "2027-08-15", passed: true }, { days: 7, on: "2027-09-07", passed: false }],
  activeWindow: null, ...over,
});

describe("decision 0045: dates with their own lead times, done, and a goal", () => {
  it("parses lead days: blank is the default ladder, bad input is refused", () => {
    expect(parseLeadDays("")).toEqual([]);
    expect(parseLeadDays("7, 30, 90, 30")).toEqual([90, 30, 7]);
    expect(parseLeadDays("14 and 3")).toEqual([14, 3]);
    expect(parseLeadDays("0")).toBeNull();
    expect(parseLeadDays("731")).toBeNull();
    expect(parseLeadDays("1,2,3,4,5")).toBeNull();
  });

  it("carries lead days, done and the goal; a done date shows Done and never 'due'", () => {
    const { dates, next } = lifeAdminFrom([
      row({ id: "a", title: "Passport", inDays: 5, leadDays: [14, 3], goalId: "g1", activeWindow: 14, reminders: [{ days: 14, on: "2027-03-01", passed: true }, { days: 3, on: "2027-03-12", passed: false }] }),
      row({ id: "b", title: "Taxes", inDays: 2, done: true, activeWindow: null }),
    ]);
    const [a, b] = dates;
    expect(a.leadDays).toEqual([14, 3]);
    expect(a.goalId).toBe("g1");
    expect(a.reminder).toContain("3 days");
    expect(a.due).toBe(true);
    expect(b.done).toBe(true);
    expect(b.reminder).toBe("Done");
    expect(b.due).toBe(false);
    // "next up" skips a date that is done even when it is nearer
    expect(next).toBe("Passport in 5 days");
  });

  it("older rows without the new fields still read as the default ladder, not done, no goal", () => {
    const { dates } = lifeAdminFrom([row({})]);
    expect(dates[0]).toMatchObject({ leadDays: [], done: false, goalId: null });
  });
});
