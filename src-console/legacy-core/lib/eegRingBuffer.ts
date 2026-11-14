/**
 * Lock-free ring buffer for EEG sample storage.
 * Stores up to `capacity` samples per channel.
 * Written to from the Tauri event listener (high frequency),
 * read from the display loop (decimated, lower frequency).
 */

import type { EegChannel } from "./types";
import { EEG_CHANNELS } from "./types";

const SAMPLE_RATE = 256;

export class EegRingBuffer {
  /** Per-channel Float64 typed arrays for zero-copy storage */
  private buffers: Record<string, Float64Array>;
  /** Write head per channel (wraps at capacity) */
  private heads: Record<string, number>;
  /** Total samples written per channel (monotonic) */
  private counts: Record<string, number>;
  /** Max samples to store */
  readonly capacity: number;

  constructor(windowSecs: number = 10) {
    this.capacity = windowSecs * SAMPLE_RATE;
    this.buffers = {};
    this.heads = {};
    this.counts = {};
    for (const ch of EEG_CHANNELS) {
      this.buffers[ch] = new Float64Array(this.capacity);
      this.heads[ch] = 0;
      this.counts[ch] = 0;
    }
  }

  /** Push samples into a channel's ring buffer. Called from event listener. */
  push(channel: EegChannel, samples: number[]): void {
    const buf = this.buffers[channel];
    if (!buf) return;
    let head = this.heads[channel];
    for (let i = 0; i < samples.length; i++) {
      buf[head] = samples[i];
      head = (head + 1) % this.capacity;
    }
    this.heads[channel] = head;
    this.counts[channel] += samples.length;
  }

  /**
   * Read the last `count` samples from a channel in chronological order.
   * Returns a plain number[] for chart consumption.
   */
  read(channel: EegChannel, count: number): number[] {
    const buf = this.buffers[channel];
    if (!buf) return [];
    const total = Math.min(count, this.samplesAvailable(channel));
    if (total === 0) return [];

    const result = new Array<number>(total);
    const head = this.heads[channel];
    // Start reading from (head - total) mod capacity
    let readIdx = (head - total + this.capacity) % this.capacity;
    for (let i = 0; i < total; i++) {
      result[i] = buf[readIdx];
      readIdx = (readIdx + 1) % this.capacity;
    }
    return result;
  }

  /** Decimate: read `windowSamples` from channel, return every nth sample for display. */
  readDecimated(channel: EegChannel, windowSamples: number, targetPoints: number): number[] {
    const available = this.samplesAvailable(channel);
    const count = Math.min(windowSamples, available);
    if (count === 0) return [];

    const step = Math.max(1, Math.floor(count / targetPoints));
    const resultLen = Math.ceil(count / step);
    const result = new Array<number>(resultLen);

    const head = this.heads[channel];
    let readIdx = (head - count + this.capacity) % this.capacity;

    for (let i = 0; i < resultLen; i++) {
      // Min-max decimation: take peak in each step window for waveform preservation
      let minVal = Infinity;
      let maxVal = -Infinity;
      const windowEnd = Math.min((i + 1) * step, count);
      const windowStart = i * step;
      let idx = (head - count + windowStart + this.capacity) % this.capacity;

      for (let j = windowStart; j < windowEnd; j++) {
        const v = this.buffers[channel][idx];
        if (v < minVal) minVal = v;
        if (v > maxVal) maxVal = v;
        idx = (idx + 1) % this.capacity;
      }

      // Use the value with larger absolute magnitude (preserves waveform peaks)
      result[i] = Math.abs(maxVal) >= Math.abs(minVal) ? maxVal : minVal;
    }

    return result;
  }

  /** How many samples are available for a channel (capped at capacity). */
  samplesAvailable(channel: EegChannel): number {
    return Math.min(this.counts[channel] || 0, this.capacity);
  }

  /** Total samples written to a channel (monotonic, for packet counting). */
  totalWritten(channel: EegChannel): number {
    return this.counts[channel] || 0;
  }

  /** Reset all buffers. */
  clear(): void {
    for (const ch of EEG_CHANNELS) {
      this.buffers[ch] = new Float64Array(this.capacity);
      this.heads[ch] = 0;
      this.counts[ch] = 0;
    }
  }
}
