import type { StressScore } from "@/lib/types";

/**
 * Stress calculator — ported from OpenWhoop stress.rs
 *
 * Algorithm:
 * 1. Convert HR (BPM) to RR intervals (ms): rr = 60000 / hr
 * 2. Build histogram of RR intervals (bin width = 50ms)
 * 3. Find mode (most frequent bin), min, max
 * 4. Compute variation range (VR) = max - min
 * 5. Stress score = (mode_amplitude) / (2 * VR * (mode / 1000))
 * 6. Clamp to 0-10
 *
 * Requires MIN_READINGS (120 samples, ~2 min at 1Hz) to produce a score.
 */

const MIN_READINGS = 120;
const BIN_WIDTH = 50; // ms

export function computeStress(hrReadings: number[]): StressScore {
  if (hrReadings.length < MIN_READINGS) {
    return {
      score: 0,
      mode: 0,
      variationRange: 0,
      sampleCount: hrReadings.length,
      ready: false,
    };
  }

  // Convert HR to RR intervals
  const rrIntervals = hrReadings
    .filter((hr) => hr > 30 && hr < 220) // sanity filter
    .map((hr) => 60000 / hr);

  if (rrIntervals.length < MIN_READINGS) {
    return {
      score: 0,
      mode: 0,
      variationRange: 0,
      sampleCount: rrIntervals.length,
      ready: false,
    };
  }

  // Build histogram
  const bins = new Map<number, number>();
  let minRR = Infinity;
  let maxRR = -Infinity;

  for (const rr of rrIntervals) {
    const bin = Math.floor(rr / BIN_WIDTH) * BIN_WIDTH;
    bins.set(bin, (bins.get(bin) || 0) + 1);
    if (rr < minRR) minRR = rr;
    if (rr > maxRR) maxRR = rr;
  }

  // Find mode (bin with highest frequency)
  let modeBin = 0;
  let modeFreq = 0;
  for (const [bin, freq] of bins) {
    if (freq > modeFreq) {
      modeBin = bin;
      modeFreq = freq;
    }
  }

  const modeCenter = modeBin + BIN_WIDTH / 2;
  const variationRange = maxRR - minRR;

  // Stress formula from OpenWhoop:
  // stress = aMode / (2 * vr * (mode / 1000))
  // where aMode = frequency of mode bin, vr = variation range
  let stress = 0;
  if (variationRange > 0 && modeCenter > 0) {
    stress = modeFreq / (2 * variationRange * (modeCenter / 1000));
  }

  // Clamp 0-10
  stress = Math.max(0, Math.min(10, stress));
  // Round to 1 decimal
  stress = Math.round(stress * 10) / 10;

  return {
    score: stress,
    mode: Math.round(modeCenter),
    variationRange: Math.round(variationRange),
    sampleCount: rrIntervals.length,
    ready: true,
  };
}
