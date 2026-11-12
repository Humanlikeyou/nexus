import { describe, expect, it } from "vitest";
import { inRoadmapWindow, localDayKey, roadmapNightDue } from "./roadmapNight";

const at = (h: number, m = 0, day = 12) => new Date(2026, 10, day, h, m).getTime();

describe("roadmap night window", () => {
  it("is open from 02:00 to 04:59 local only", () => {
    expect(inRoadmapWindow(at(1, 59))).toBe(false);
    expect(inRoadmapWindow(at(2))).toBe(true);
    expect(inRoadmapWindow(at(4, 59))).toBe(true);
    expect(inRoadmapWindow(at(5))).toBe(false);
  });
  it("runs once a night and never while something else is running", () => {
    expect(roadmapNightDue(at(3), null, false)).toBe(true);
    expect(roadmapNightDue(at(3), localDayKey(at(3)), false)).toBe(false);
    expect(roadmapNightDue(at(3, 30, 13), localDayKey(at(3)), false)).toBe(true);
    expect(roadmapNightDue(at(3), null, true)).toBe(false);
    expect(roadmapNightDue(at(12), null, false)).toBe(false);
  });
});
