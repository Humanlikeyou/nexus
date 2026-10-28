import type { FrequencyBands } from "./fft";
import { museTelemetry, type MuseTelemetry } from "./telemetryStore";

export const BAND_KEYS = ["delta", "theta", "alpha", "beta", "gamma"] as const;
export type BandKey = (typeof BAND_KEYS)[number];
export type BandSeries = Record<BandKey, number[]>;

/** Band powers only mean something once a full 256-sample window exists. */
export const MIN_SAMPLES = 256;
export const HISTORY_MS = 60_000;

type Source = { subscribe: (listener: () => void) => () => void; getSnapshot: () => MuseTelemetry };

const emptySeries = (): BandSeries => ({ delta: [], theta: [], alpha: [], beta: [], gamma: [] });

/**
 * Rolling 60 s history of relative band power, fed by the telemetry store's
 * ≤4 Hz publishes (Decision 7: client-side, Neuro screen only). Points older
 * than the window (relative to the newest point and to `now()`) are dropped; a
 * telemetry reset (packetCount back to 0) clears it, and so does a stall: when
 * the last packet is more than `windowMs` old the sparklines empty out.
 */
export function createBandHistory(source: Source = museTelemetry, windowMs = HISTORY_MS, now: () => number = Date.now) {
  let points: Array<{ at: number; bands: FrequencyBands }> = [];
  let lastPacket = -1;
  let series: BandSeries = emptySeries();
  const listeners = new Set<() => void>();
  let unsubscribe: (() => void) | null = null;

  const rebuild = () => {
    series = emptySeries();
    for (const point of points) for (const key of BAND_KEYS) series[key].push(point.bands[key]);
  };

  /** Drop points outside the window; returns true when anything changed. */
  const prune = (lastPacketAt: number | null): boolean => {
    if (!points.length) return false;
    const at = now();
    const stalled = lastPacketAt == null || at - lastPacketAt > windowMs;
    const newest = points[points.length - 1].at;
    const kept = stalled ? [] : points.filter((point) => newest - point.at <= windowMs && at - point.at <= windowMs);
    if (kept.length === points.length) return false;
    points = kept;
    rebuild();
    return true;
  };

  const onTelemetry = () => {
    const telemetry = source.getSnapshot();
    if (telemetry.packetCount === lastPacket) return;
    if (telemetry.packetCount < lastPacket) points = [];
    lastPacket = telemetry.packetCount;
    if (telemetry.sampleCount >= MIN_SAMPLES && telemetry.lastPacketAt != null) {
      points.push({ at: telemetry.lastPacketAt, bands: telemetry.bands });
    }
    prune(telemetry.lastPacketAt);
    rebuild();
    for (const listener of listeners) listener();
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      if (listeners.size === 1) unsubscribe = source.subscribe(onTelemetry);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) { unsubscribe?.(); unsubscribe = null; }
      };
    },
    /** Also prunes lazily (e.g. on the screen's 1 s tick) so a stall empties the series without new packets. */
    getSnapshot: () => {
      prune(source.getSnapshot().lastPacketAt);
      return series;
    },
  };
}

export type BandHistory = ReturnType<typeof createBandHistory>;
