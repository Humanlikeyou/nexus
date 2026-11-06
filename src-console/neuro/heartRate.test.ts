import { describe, expect, it } from "vitest";
import { PPG_RATE, PPG_WINDOW, estimateHeartRate, heartRateSummary } from "./heartRate";

/** Infrared PPG: a large count, slow drift, a pulse about 0.5% of the level, and some noise. */
function ppg(bpm: number, n = PPG_WINDOW, pulse = 290) {
  let seed = 5;
  return Array.from({ length: n }, (_, i) => {
    seed = (seed * 9301 + 49297) % 233280;
    return 53_500 + 400 * Math.sin(2 * Math.PI * .05 * i / PPG_RATE) + pulse * Math.sin(2 * Math.PI * (bpm / 60) * i / PPG_RATE) + (seed / 233280 - .5) * 120;
  });
}

describe("estimateHeartRate", () => {
  it.each([52, 64, 96])("finds a pulse of %i bpm", (bpm) => {
    expect(Math.abs(estimateHeartRate(ppg(bpm))! - bpm)).toBeLessThanOrEqual(1);
  });
  it("won't guess from noise, a flat line, too little data or bad data", () => {
    expect(estimateHeartRate(ppg(64, PPG_WINDOW, 0))).toBeNull();
    expect(estimateHeartRate(Array(PPG_WINDOW).fill(5000))).toBeNull();
    expect(estimateHeartRate(ppg(64, PPG_WINDOW - 1))).toBeNull();
    const broken = ppg(64); broken[3] = NaN;
    expect(estimateHeartRate(broken)).toBeNull();
  });
});

describe("heartRateSummary", () => {
  it("is labelled an estimate and takes the median", () => {
    expect(heartRateSummary([62, 64, 120])).toEqual({ bpm: 64, readings: 3, estimate: true, source: "ppg" });
    expect(heartRateSummary([])).toBeNull();
  });
});
