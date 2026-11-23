import { describe, expect, it } from "vitest";
import { PULSE_STALE_S, calmPulse, heartPulse, pulsePeriod } from "../pulse";

describe("design-1118b heartbeat pulse (live heart rate only)", () => {
  const now = 1_800_000_000;
  it("the pulse period is 60 / bpm", () => {
    expect(pulsePeriod(60, now, now)).toBeCloseTo(1);
    expect(pulsePeriod(75, now - 10, now)).toBeCloseTo(0.8);
    expect(pulsePeriod(120, now, now)).toBeCloseTo(0.5);
  });
  it("no pulse when the reading is missing, invalid or stale (never a made-up rate)", () => {
    expect(pulsePeriod(null, now, now)).toBeNull();
    expect(pulsePeriod(undefined, undefined, now)).toBeNull();
    expect(pulsePeriod(0, now, now)).toBeNull();
    expect(pulsePeriod(Number.NaN, now, now)).toBeNull();
    expect(pulsePeriod(64, null, now)).toBeNull();
    expect(pulsePeriod(64, now - PULSE_STALE_S - 1, now)).toBeNull();
  });
  it("each beat is a lub-dub that repeats every beat; calm is an even swell", () => {
    expect(heartPulse(0.06)).toBeCloseTo(1, 1);
    expect(heartPulse(0.3)).toBeGreaterThan(0.45);
    expect(heartPulse(0.3)).toBeLessThan(heartPulse(0.06));
    expect(heartPulse(0.7)).toBeLessThan(0.05);
    expect(heartPulse(1.3)).toBeCloseTo(heartPulse(0.3));
    expect(calmPulse(0)).toBeCloseTo(0); expect(calmPulse(1)).toBeCloseTo(1);
  });
});
