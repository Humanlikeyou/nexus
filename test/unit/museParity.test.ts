// @vitest-environment node
/**
 * Rust<->TS parity for the recording readers (src-tauri/src/commands/recordings_dsp.rs). This test
 * pins what the TypeScript originals return for synthetic inputs in test/fixtures/muse; the Rust
 * test `parity_with_typescript` checks its ports against the same JSON. Refresh the expected file
 * with MUSE_PARITY_UPDATE=1 only when the TypeScript maths changes on purpose.
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { relativeChannelBandPowers } from "../../src-console/neuro/fft";
import { computeBands } from "../../src/hooks/useFFT";
import { assessChannel } from "../../src/hooks/useSignalQuality";

const dir = new URL("../fixtures/muse/", import.meta.url);
const inputs = JSON.parse(readFileSync(new URL("parity_inputs.json", dir), "utf8")) as Record<string, number[]>;
const expectedUrl = new URL("parity_expected.json", dir);
const BAND_KEYS = ["delta", "theta", "alpha", "beta", "gamma"] as const;

function compute() {
  return Object.fromEntries(Object.entries(inputs).map(([name, samples]) => {
    const bands = relativeChannelBandPowers([samples]);
    const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
    const scores = computeBands(samples.map((v) => v - mean));
    return [name, { bands: BAND_KEYS.map((k) => bands[k]), quality: assessChannel(samples), focus: scores.focusScore, relaxation: scores.relaxationScore }];
  }));
}

describe("muse maths parity fixtures", () => {
  it("TypeScript still produces the pinned values", () => {
    const actual = compute();
    if (process.env.MUSE_PARITY_UPDATE === "1" || !existsSync(expectedUrl)) writeFileSync(expectedUrl, JSON.stringify(actual, null, 1) + "\n");
    const expected = JSON.parse(readFileSync(expectedUrl, "utf8"));
    for (const [name, value] of Object.entries(actual)) {
      expect(expected[name].quality).toBe(value.quality);
      expect(expected[name].focus).toBe(value.focus);
      expect(expected[name].relaxation).toBe(value.relaxation);
      value.bands.forEach((b, i) => expect(expected[name].bands[i]).toBeCloseTo(b, 9));
    }
  });
});
