import { beatSeconds } from "../hooks/useLiveHeartRate";

/** A reading older than this (seconds) is stale: no pulse, never a guessed rate. Live batches land about every 15 s
 *  and play back up to ~34 s behind, so two minutes means the sync has stopped. */
export const PULSE_STALE_S = 120;

/** Seconds per beat from the live heart rate, or null (no pulse) when the reading is missing, invalid or stale.
 *  Same clamp as the Home heart card (beatSeconds), so a bad value can never strobe. */
export function pulsePeriod(bpm: number | null | undefined, at: number | null | undefined, nowS = Date.now() / 1000): number | null {
  const p = beatSeconds(bpm);
  if (p == null || typeof at !== "number" || !Number.isFinite(at)) return null;
  const atS = at > 1e12 ? at / 1000 : at; // tolerate ms timestamps
  if (nowS - atS > PULSE_STALE_S) return null;
  return p;
}

/** One heartbeat as a soft lub-dub swell, 0..1, for a phase in beats (0..1 is one beat). */
export function heartPulse(phase: number): number {
  const x = phase - Math.floor(phase);
  const lub = Math.exp(-(((x - 0.06) / 0.055) ** 2));
  const dub = 0.55 * Math.exp(-(((x - 0.3) / 0.065) ** 2));
  return Math.min(1, lub + dub);
}

/** Calm / reduced motion: one slow, even brightness swell over two beats (no lub-dub, no scale). */
export function calmPulse(phase: number): number {
  return 0.5 - 0.5 * Math.cos(Math.PI * phase);
}
