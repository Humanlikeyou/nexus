/**
 * Heart rate from the Muse's forehead PPG (infrared, about 64 Hz). This is an estimate: one
 * spectral peak over a 20 second window, not a medical or beat-by-beat reading.
 */

export const PPG_RATE = 64;
export const PPG_BUFFER = 30 * PPG_RATE;
export const PPG_WINDOW = 20 * PPG_RATE;
const MIN_BPM = 42;
const MAX_BPM = 180;
/** How far the peak must stand above the band's median. A real capture read over 100, noise under 20. */
const MIN_PEAK_RATIO = 40;

export type HeartRateSummary = { bpm: number; readings: number; estimate: true; source: "ppg" };

/** Beats per minute from raw PPG counts, or null when no clear pulse stands out. */
export function estimateHeartRate(samples: number[], rate = PPG_RATE): number | null {
  if (samples.length < PPG_WINDOW || samples.some((value) => !Number.isFinite(value))) return null;
  const recent = samples.slice(-PPG_WINDOW);
  // Take out the slow drift with a one second moving average.
  const span = Math.round(rate / 2);
  const flat = recent.map((value, index) => {
    const from = Math.max(0, index - span), to = Math.min(recent.length, index + span + 1);
    let sum = 0;
    for (let at = from; at < to; at += 1) sum += recent[at];
    return value - sum / (to - from);
  });
  const powers: number[] = [];
  for (let bpm = MIN_BPM; bpm <= MAX_BPM; bpm += 1) {
    let re = 0, im = 0;
    const step = 2 * Math.PI * (bpm / 60) / rate;
    for (let index = 0; index < flat.length; index += 1) {
      const taper = .5 - .5 * Math.cos(2 * Math.PI * index / (flat.length - 1));
      re += flat[index] * taper * Math.cos(step * index);
      im -= flat[index] * taper * Math.sin(step * index);
    }
    powers.push(re * re + im * im);
  }
  const typical = [...powers].sort((a, b) => a - b)[powers.length >> 1];
  if (!(typical > 0)) return null;
  const peak = powers.indexOf(Math.max(...powers));
  return powers[peak] / typical >= MIN_PEAK_RATIO ? peak + MIN_BPM : null;
}

export function heartRateSummary(readings: number[]): HeartRateSummary | null {
  if (!readings.length) return null;
  const sorted = [...readings].sort((a, b) => a - b);
  return { bpm: sorted[sorted.length >> 1], readings: readings.length, estimate: true, source: "ppg" };
}
