/**
 * Mains hum removal. Muse ear sensors can pick up 5 to 27 times more 50 Hz power than all of
 * 1-40 Hz EEG, which says nothing about the EEG underneath.
 *
 * The filter runs on the block already being analysed: it clears the hum bins in the frequency
 * domain and transforms back, so it is zero-phase and adds no delay to the feedback. 50 and 60 Hz
 * land on exact bins of a 256 or 512 sample block at 256 Hz, so nothing smears.
 */

import { FFT_SIZE, SAMPLE_RATE, fft, singlePSD } from "./fft";

export type LineHz = 50 | 60;

const LINE_FREQUENCIES: LineHz[] = [50, 60];
/** A hum peak stands this far above the spectrum beside it. */
const MIN_PROMINENCE = 8;
/** Hum drifts in level from second to second, which spreads it a little around the line. */
const NOTCH_HALF_WIDTH_HZ = 3;

function prominence(powers: Float64Array, hz: number): number {
  const peak = Math.max(powers[hz - 1], powers[hz], powers[hz + 1]);
  let floor = 0;
  for (const offset of [-6, -5, -4, 4, 5, 6]) floor += powers[hz + offset] / 6;
  return floor > 0 ? peak / floor : 0;
}

/** Which mains frequency is in this signal, from its newest second. Null when there's no hum. */
export function detectLineHz(samples: number[]): LineHz | null {
  if (samples.length < FFT_SIZE) return null;
  const recent = samples.slice(-FFT_SIZE);
  const mean = recent.reduce((sum, value) => sum + value, 0) / FFT_SIZE;
  const powers = singlePSD(recent.map((value) => value - mean));
  let found: LineHz | null = null;
  let best = MIN_PROMINENCE;
  for (const hz of LINE_FREQUENCIES) {
    const score = prominence(powers, hz);
    if (score > best) { best = score; found = hz; }
  }
  return found;
}

/**
 * The newest 256 or 512 samples with mains hum and its second harmonic taken out, the mean kept.
 * Short or non-finite input comes back as it was: too little to judge, and for callers to reject.
 */
export function removeLineNoise(samples: number[]): number[] {
  if (samples.length < FFT_SIZE || samples.some((value) => !Number.isFinite(value))) return samples;
  const hz = detectLineHz(samples);
  const size = samples.length >= FFT_SIZE * 2 ? FFT_SIZE * 2 : FFT_SIZE;
  const block = samples.slice(-size);
  if (hz === null) return block;
  const mean = block.reduce((sum, value) => sum + value, 0) / size;
  const re = Float64Array.from(block, (value) => value - mean);
  const im = new Float64Array(size);
  fft(re, im);
  const binsPerHz = size / SAMPLE_RATE;
  for (const center of [hz, hz * 2]) {
    const from = Math.round((center - NOTCH_HALF_WIDTH_HZ) * binsPerHz);
    const to = Math.round((center + NOTCH_HALF_WIDTH_HZ) * binsPerHz);
    for (let bin = from; bin <= to && bin <= size / 2; bin += 1) {
      re[bin] = 0; im[bin] = 0;
      re[(size - bin) % size] = 0; im[(size - bin) % size] = 0;
    }
  }
  // Inverse transform: conjugate, transform, conjugate, scale. The result is real.
  for (let index = 0; index < size; index += 1) im[index] = -im[index];
  fft(re, im);
  return Array.from(re, (value) => value / size + mean);
}
