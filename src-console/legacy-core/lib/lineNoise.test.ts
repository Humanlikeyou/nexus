import { describe, expect, it } from "vitest";
import { detectLineHz, removeLineNoise } from "./lineNoise";
import { assessChannel } from "../hooks/useSignalQuality";

const RATE = 256;
const tone = (hz: number, size: number, n = 256) => Array.from({ length: n }, (_, i) => size * Math.sin(2 * Math.PI * hz * i / RATE));
const add = (...signals: number[][]) => signals[0].map((_, i) => signals.reduce((sum, signal) => sum + signal[i], 0));
/** Repeatable noise, about 5 µV sd: what a worn sensor has left once hum is gone. */
function noise(sd = 5, n = 256) {
  let seed = 11;
  return Array.from({ length: n }, () => { seed = (seed * 9301 + 49297) % 233280; return (seed / 233280 - .5) * sd * 3.46; });
}
const eeg = (n = 256) => add(tone(10, 6, n), tone(6, 3, n), tone(22, 3, n), noise(5, n));
const sd = (values: number[]) => { const mean = values.reduce((a, b) => a + b) / values.length; return Math.sqrt(values.reduce((a, b) => a + (b - mean) ** 2, 0) / values.length); };

describe("mains hum", () => {
  it("tells 50 Hz from 60 Hz, and clean EEG from either", () => {
    expect(detectLineHz(add(eeg(), tone(50, 60)))).toBe(50);
    expect(detectLineHz(add(eeg(), tone(60, 60)))).toBe(60);
    expect(detectLineHz(eeg())).toBeNull();
  });
  it("takes out hum and its harmonic, and leaves in-band EEG in place", () => {
    for (const hz of [50, 60]) {
      const clean = removeLineNoise(add(eeg(), tone(hz, 85), tone(hz * 2, 8)));
      const expected = eeg();
      const error = clean.map((value, i) => value - expected[i]);
      expect(sd(error)).toBeLessThan(2);
      expect(Math.abs(error[255])).toBeLessThan(4); // the newest sample: nothing lags
    }
  });
  it("handles the full two second buffer", () => {
    expect(removeLineNoise(add(eeg(512), tone(50, 60, 512)))).toHaveLength(512);
  });
  it("copes with hum whose level drifts within the window", () => {
    const drifting = eeg().map((value, i) => value + (40 + 30 * Math.sin(2 * Math.PI * i / 256)) * Math.sin(2 * Math.PI * 50 * i / RATE));
    expect(sd(removeLineNoise(drifting))).toBeLessThan(sd(eeg()) * 1.2);
  });
  it("leaves short or broken input for the caller to judge", () => {
    expect(removeLineNoise([1, 2, 3])).toEqual([1, 2, 3]);
    const broken = eeg(); broken[9] = NaN;
    expect(removeLineNoise(broken)).toBe(broken);
  });
});

describe("assessChannel on real amplitudes", () => {
  it("rates worn sensors good at the measured raw sd of 14 to 67 µV", () => {
    expect(assessChannel(eeg())).toBe("good");
    expect(assessChannel(add(eeg(), tone(50, 20)))).toBe("good");
    expect(assessChannel(add(eeg(), tone(50, 94)))).toBe("good"); // sd about 67 µV, mostly hum
    expect(assessChannel(add(eeg(), tone(60, 94)))).toBe("good");
  });
  it("doesn't call a blink bad contact", () => {
    const blink = eeg().map((value, i) => value + (i > 100 && i < 140 ? 150 : 0));
    expect(assessChannel(blink)).toBe("good");
  });
  it("still flags bad contact", () => {
    expect(assessChannel(Array(256).fill(0))).toBe("poor");
    expect(assessChannel(tone(50, 94))).toBe("poor"); // hum with nothing under it: off the skin
    expect(assessChannel(Array(256).fill(990))).toBe("poor"); // railed
    expect(assessChannel(noise(70))).toBe("fair");
    expect(assessChannel(noise(160))).toBe("poor");
  });
});
