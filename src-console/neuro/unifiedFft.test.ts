import { describe, expect, it } from "vitest";
import { bandFeatures, BAND_EDGES_HZ, relativeChannelBandPowers } from "./fft";
import { computeBands } from "../legacy-core/hooks/useFFT";
import { COMPUTED_BANDS_HZ } from "./protocolConfig";
import { TRAINING_METHOD } from "./training";

const sine = (hz: number, n = 256, amp = 20) => Array.from({ length: n }, (_, i) => amp * Math.sin(2 * Math.PI * hz * (i / 256)) + 3 * Math.sin(2 * Math.PI * 22 * (i / 256)) + 2 * Math.sin(2 * Math.PI * 6 * (i / 256)));

describe("decision 0060: one band-power pipeline", () => {
  it("training features and the display give the same five bands", () => {
    const a = sine(10), b = sine(11);
    const display = relativeChannelBandPowers([a, b]);
    const training = bandFeatures([a, b]);
    for (const k of ["delta", "theta", "alpha", "beta", "gamma"] as const) expect(training[k]).toBeCloseTo(display[k], 1);
  });

  it("every bin is counted once: the five bands add up to 100%", () => {
    const f = bandFeatures([sine(8), sine(13)]);
    expect(f.delta + f.theta + f.alpha + f.beta + f.gamma).toBeCloseTo(100, 0);
  });

  it("computeBands is the same pipeline for one channel", () => {
    const x = sine(10);
    expect(computeBands(x)).toEqual(bandFeatures([x]));
  });

  it("an alpha signal no longer leaks into theta through a shared 8 Hz bin", () => {
    const f = bandFeatures([sine(10, 256, 40)]);
    expect(f.alpha).toBeGreaterThan(f.theta * 3);
    expect(f.alphaPeak).toBe(10);
  });

  it("the edges are stated once and used everywhere they are described", () => {
    expect(TRAINING_METHOD.bands_hz).toBe(BAND_EDGES_HZ);
    expect(COMPUTED_BANDS_HZ.delta).toEqual([...BAND_EDGES_HZ.delta]);
    expect(COMPUTED_BANDS_HZ.gamma).toEqual([...BAND_EDGES_HZ.gamma]);
    expect(COMPUTED_BANDS_HZ.highbeta).toEqual([...BAND_EDGES_HZ.highBeta]);
  });

  it("an incomplete or non-finite window is all zeros, never a made-up value", () => {
    const f = bandFeatures([sine(10, 100)]);
    expect(f.alpha).toBe(0);
    expect(f.focusScore).toBe(0);
    expect(bandFeatures([[...sine(10).slice(0, 255), Number.NaN]]).alpha).toBe(0);
  });
});
