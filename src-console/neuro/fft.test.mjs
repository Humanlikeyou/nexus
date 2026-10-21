import assert from "node:assert/strict";
import { test } from "node:test";
import { relativeBandPowers, relativeChannelBandPowers } from "./fft.ts";

const signal = (frequency, amplitude = 1) => Array.from({ length: 256 }, (_, index) => amplitude * Math.sin(2 * Math.PI * frequency * index / 256));

test("isolated EEG frequencies map to their expected bands", () => {
  for (const [band, frequency] of [["delta", 2], ["theta", 6], ["alpha", 10], ["beta", 20], ["gamma", 38]]) {
    const powers = relativeBandPowers(signal(frequency));
    assert.ok(powers[band] > 95, `${frequency} Hz: ${JSON.stringify(powers)}`);
    assert.ok(Math.abs(Object.values(powers).reduce((sum, value) => sum + value, 0) - 100) < 1e-9);
  }
});

test("opposite electrode phases retain their spectral power", () => {
  const powers = relativeChannelBandPowers([signal(10), signal(10, -1)]);
  assert.ok(powers.alpha > 99);
});

test("relative power respects amplitude squared and ignores DC offsets", () => {
  const samples = signal(10, 2).map((value, index) => value + signal(20)[index] + 1000);
  const powers = relativeBandPowers(samples);
  assert.ok(Math.abs(powers.alpha - 80) < 1);
  assert.ok(Math.abs(powers.beta - 20) < 1);
});

test("incomplete, flat, and invalid signals do not produce invalid values", () => {
  for (const samples of [[], [1, 2], Array(256).fill(100), Array(256).fill(NaN)]) {
    assert.deepEqual(relativeBandPowers(samples), { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 });
  }
});
