import { describe, expect, it } from "vitest";
import { WEATHER_MAX_AGE_MS, freshBrief, usableWeather } from "./useHomeBrief";

const weather = { place: "Utsunomiya", tempC: 14, summary: "rain", highC: 16, lowC: 11 };
const now = Date.parse("2026-11-05T09:00:00Z");
const brief = (age: number) => ({ greeting: "Good evening, Taisun", weather, updatedAt: new Date(now - age).toISOString() });

describe("Home brief", () => {
  it("keeps a fresh brief and drops one past three hours", () => {
    expect(freshBrief(brief(WEATHER_MAX_AGE_MS), now)).not.toBeNull();
    expect(freshBrief(brief(WEATHER_MAX_AGE_MS + 1), now)).toBeNull();
  });
  it("drops a brief with no usable timestamp", () => {
    expect(freshBrief({ greeting: "Hi", weather, updatedAt: null }, now)).toBeNull();
    expect(freshBrief({ greeting: "Hi", weather, updatedAt: "not a date" }, now)).toBeNull();
    expect(freshBrief(null, now)).toBeNull();
  });
  it("needs a temperature or a word to show weather at all", () => {
    expect(usableWeather(null)).toBeNull();
    expect(usableWeather({ place: "Tokyo", tempC: null, summary: null, highC: 3, lowC: 1 })).toBeNull();
    expect(usableWeather({ place: null, tempC: null, summary: "rain", highC: null, lowC: null })).not.toBeNull();
  });
});
