import { describe, expect, it } from "vitest";
import { ambienceVars, targetAmbience } from "../ambience";

describe("design-1118b bio-reactive ambience", () => {
  it("no WHOOP reading = a neutral ambience (nothing invented)", () => {
    const a = targetAmbience(12 * 60, null, false);
    expect(a.source).toBe("neutral");
    expect(a.energy).toBe(1);
    expect(a.warmth).toBe(0);
    expect(a.dim).toBe(0);
    expect(targetAmbience(12 * 60, Number.NaN, false).source).toBe("neutral");
  });

  it("Calm ignores recovery (no reactive changes)", () => {
    expect(targetAmbience(12 * 60, 20, true).source).toBe("neutral");
  });

  it("low recovery is slower, warmer and dimmer than high recovery", () => {
    const lo = targetAmbience(12 * 60, 18, false), hi = targetAmbience(12 * 60, 92, false);
    expect(lo.source).toBe("recovery");
    expect(lo.energy).toBeLessThan(hi.energy);
    expect(lo.warmth).toBeGreaterThan(hi.warmth);
    expect(lo.dim).toBeGreaterThan(hi.dim);
    expect(lo.energy).toBeGreaterThanOrEqual(0.7);
    expect(hi.energy).toBeLessThanOrEqual(1.2);
  });

  it("the canopy light moves with the time of day and is weakest at night", () => {
    const dawn = targetAmbience(6 * 60 + 40, null, false), noon = targetAmbience(12 * 60, null, false), night = targetAmbience(22 * 60 + 30, null, false);
    expect(dawn.lx).toBeLessThan(noon.lx);
    expect(night.li).toBeLessThan(noon.li);
    expect(night.bg[0][1]).toBeLessThan(noon.bg[0][1]);
  });

  it("CSS vars: light tokens always, background colours only with ambience on", () => {
    const a = targetAmbience(12 * 60, 64, false);
    expect(ambienceVars(a, false)).not.toHaveProperty("--bg-a");
    expect(ambienceVars(a, false)).toHaveProperty("--light-x");
    expect(ambienceVars(a, true)["--bg-a"]).toMatch(/^#[0-9a-f]{6}$/);
  });
});
