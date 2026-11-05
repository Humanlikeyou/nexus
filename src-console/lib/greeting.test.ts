import { describe, expect, it } from "vitest";
import { dayPart, localGreeting } from "./greeting";

const at = (day: number, hour: number) => new Date(2026, 10, day, hour, 30);

describe("Home greeting", () => {
  it.each([[4, "night"], [5, "morning"], [11, "morning"], [12, "afternoon"], [16, "afternoon"], [17, "evening"], [21, "evening"], [22, "night"]] as const)("%i:00 is %s", (hour, part) => {
    expect(dayPart(hour)).toBe(part);
  });
  it("stays the same all morning, and varies between days", () => {
    expect(localGreeting(at(5, 6))).toBe(localGreeting(at(5, 11)));
    const lines = new Set(Array.from({ length: 10 }, (_, i) => localGreeting(at(i + 1, 9))));
    expect(lines.size).toBeGreaterThan(1);
    for (const line of lines) expect(line).toMatch(/Taisun/);
  });
  it("the classic line comes up most days", () => {
    const days = Array.from({ length: 25 }, (_, i) => localGreeting(at(i + 1, 14)));
    expect(days.filter((line) => line === "Good afternoon, Taisun").length).toBeGreaterThan(12);
  });
});
