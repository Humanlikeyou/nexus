/**
 * Artifact detection: pure functions for identifying EEG artifacts.
 *
 * Blink: slow deflection >75µV lasting 100-400ms on frontal channels (AF7/AF8)
 * Motion: simultaneous >100µV jumps on 3+ channels
 * Jaw clench: 20-40Hz band spike >3x rolling baseline
 */

import type { ArtifactEvent } from "./types";
import { singlePSD, bandPower } from "./fft";

const BLINK_THRESHOLD_UV = 75;
const BLINK_MIN_SAMPLES = 26;  // ~100ms at 256Hz
const BLINK_MAX_SAMPLES = 102; // ~400ms at 256Hz

const MOTION_THRESHOLD_UV = 100;
const MOTION_MIN_CHANNELS = 3;

const JAW_CLENCH_MULTIPLIER = 3;

/** Detect blinks on frontal channels (AF7/AF8). */
export function detectBlinks(
  af7: number[],
  af8: number[],
  ts: number,
): ArtifactEvent[] {
  const events: ArtifactEvent[] = [];

  for (const [ch, samples] of [["AF7", af7], ["AF8", af8]] as const) {
    if (samples.length < BLINK_MIN_SAMPLES) continue;

    // Look for slow deflection pattern: sustained deviation from local mean
    const localMean = samples.reduce((a, b) => a + b, 0) / samples.length;
    let runLength = 0;
    let maxAmp = 0;

    for (let i = 0; i < samples.length; i++) {
      const dev = Math.abs(samples[i] - localMean);
      if (dev > BLINK_THRESHOLD_UV) {
        runLength++;
        if (dev > maxAmp) maxAmp = dev;
      } else {
        if (runLength >= BLINK_MIN_SAMPLES && runLength <= BLINK_MAX_SAMPLES) {
          events.push({ type: "blink", timestamp: ts, channel: ch, amplitude: maxAmp });
          break; // One blink detection per channel per window
        }
        runLength = 0;
        maxAmp = 0;
      }
    }

    // Check trailing run
    if (runLength >= BLINK_MIN_SAMPLES && runLength <= BLINK_MAX_SAMPLES) {
      events.push({ type: "blink", timestamp: ts, channel: ch, amplitude: maxAmp });
    }
  }

  return events;
}

/** Detect motion artifacts: simultaneous large jumps on 3+ channels. */
export function detectMotion(
  allChannels: Record<string, number[]>,
  ts: number,
): ArtifactEvent[] {
  const channelNames = Object.keys(allChannels);
  if (channelNames.length < MOTION_MIN_CHANNELS) return [];

  // Check for simultaneous large jumps
  let channelsWithJumps = 0;
  let maxAmp = 0;

  for (const ch of channelNames) {
    const samples = allChannels[ch];
    if (samples.length < 2) continue;

    let hasLargeJump = false;
    for (let i = 1; i < samples.length; i++) {
      const jump = Math.abs(samples[i] - samples[i - 1]);
      if (jump > MOTION_THRESHOLD_UV) {
        hasLargeJump = true;
        if (jump > maxAmp) maxAmp = jump;
        break;
      }
    }
    if (hasLargeJump) channelsWithJumps++;
  }

  if (channelsWithJumps >= MOTION_MIN_CHANNELS) {
    return [{ type: "motion", timestamp: ts, amplitude: maxAmp }];
  }

  return [];
}

/** Detect jaw clench: 20-40Hz band spike >3x rolling baseline. */
export function detectJawClench(
  samples: number[],
  jawBaseline: number,
  ch: string,
  ts: number,
): ArtifactEvent[] {
  if (samples.length < 256 || jawBaseline <= 0) return [];

  const psd = singlePSD(samples.slice(-256));
  const jawPower = bandPower(psd, 20, 40);

  if (jawPower > jawBaseline * JAW_CLENCH_MULTIPLIER) {
    return [{ type: "jaw_clench", timestamp: ts, channel: ch, amplitude: jawPower }];
  }

  return [];
}

/** Run all artifact detectors on channel data. */
export function detectAllArtifacts(
  channelSamples: Record<string, number[]>,
  jawBaseline: number,
  ts: number,
): ArtifactEvent[] {
  const events: ArtifactEvent[] = [];

  // Blinks (frontal channels)
  const af7 = channelSamples["AF7"] || [];
  const af8 = channelSamples["AF8"] || [];
  events.push(...detectBlinks(af7, af8, ts));

  // Motion (all channels)
  events.push(...detectMotion(channelSamples, ts));

  // Jaw clench (frontal channels — most sensitive to EMG)
  for (const ch of ["AF7", "AF8"]) {
    const samples = channelSamples[ch] || [];
    events.push(...detectJawClench(samples, jawBaseline, ch, ts));
  }

  return events;
}

/** Compute 20-40Hz power from samples for jaw baseline tracking. */
export function computeJawPower(samples: number[]): number {
  if (samples.length < 256) return 0;
  const psd = singlePSD(samples.slice(-256));
  return bandPower(psd, 20, 40);
}
