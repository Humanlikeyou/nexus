import type { AuxInfo } from "./recordings";

/**
 * W12c: is the back-of-head (AUX) electrode on and giving a real signal? Pure functions over raw
 * AUX µV at 256 Hz. A 1 s step is "good" when, after a 0.5 Hz high-pass, its robust SD (1.4826 x MAD)
 * sits in 1.5-40 µV and it is not railed. 50/60 Hz line noise is deliberately NOT a reason to fail a
 * step (a real electrode on a head picks it up). Five good steps in a row = detected.
 */
export const AUX_RATE = 256;
export const AUX_STEP = AUX_RATE;
export const AUX_GOOD_STEPS = 5;
export const AUX_SD_MIN = 1.5;
export const AUX_SD_MAX = 40;
/** Raw magnitude (µV) at which the amplifier is saturated. */
export const AUX_RAIL = 1000;
const RAILED_FRACTION = 0.01;

/** One-pole high-pass after removing the window mean; returns the filtered signal. */
export function highPass(samples: readonly number[], fc = 0.5, fs = AUX_RATE): number[] {
  if (samples.length === 0) return [];
  const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
  const rc = 1 / (2 * Math.PI * fc); const a = rc / (rc + 1 / fs);
  const out = new Array<number>(samples.length); let y = 0; let prev = 0;
  for (let i = 0; i < samples.length; i += 1) { const x = samples[i] - mean; y = a * (y + x - prev); prev = x; out[i] = y; }
  return out;
}

const median = (xs: number[]) => { const s = [...xs].sort((p, q) => p - q); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };
export const robustSd = (xs: readonly number[]) => { const m = median([...xs]); return 1.4826 * median(xs.map((x) => Math.abs(x - m))); };

/** Judge the most recent second of `window` (needs at least one second; extra leading samples only warm the filter up). */
export function auxStepGood(window: readonly number[]): boolean {
  if (window.length < AUX_STEP || window.some((x) => !Number.isFinite(x))) return false;
  const last = window.slice(-AUX_STEP);
  if (last.filter((x) => Math.abs(x) >= AUX_RAIL).length / AUX_STEP >= RAILED_FRACTION) return false;
  const sd = robustSd(highPass(window).slice(-AUX_STEP));
  return sd >= AUX_SD_MIN && sd <= AUX_SD_MAX;
}

/** Streak counter: feed one 1 s window per step (`null` = no AUX data this second). Latches once detected. */
export class AuxDetector {
  private streak = 0;
  private found = false;
  step(window: readonly number[] | null): boolean {
    if (this.found) return true;
    this.streak = window && auxStepGood(window) ? this.streak + 1 : 0;
    if (this.streak >= AUX_GOOD_STEPS) this.found = true;
    return this.found;
  }
  get detected() { return this.found; }
  reset() { this.streak = 0; this.found = false; }
}

/** Batch form over a long AUX recording: non-overlapping 1 s steps. */
export function detectAux(samples: readonly number[]): boolean {
  const d = new AuxDetector();
  for (let i = 0; i + AUX_STEP <= samples.length; i += AUX_STEP) if (d.step(samples.slice(i, i + AUX_STEP))) return true;
  return false;
}

export const auxInfoDetected = (at = new Date()): AuxInfo => ({ detected: true, placement: "occipital", detectedBy: "signal-quality", detectedAt: at.toISOString() });
