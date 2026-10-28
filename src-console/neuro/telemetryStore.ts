import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EegSample } from "../types";
import { relativeChannelBandPowers, type FrequencyBands } from "./fft";

/**
 * Muse EEG telemetry as an external store (useSyncExternalStore). Raw samples
 * accumulate in plain refs; a new snapshot is published at most every 250 ms
 * and ONLY when new packets arrived since the last one, so an idle or
 * disconnected headset causes zero React updates. Only components that
 * subscribe (the Neuro/Live screen) re-render; the app root never does.
 */

const CHANNELS = ["TP9", "AF7", "AF8", "TP10"] as const;
const MAX_BUFFER = 512;
export const UI_INTERVAL_MS = 250;

export interface MuseTelemetry {
  channels: Record<string, number[]>;
  bands: FrequencyBands;
  packetCount: number;
  sampleCount: number;
  lastPacketAt: number | null;
  channelLastPacketAt: Record<string, number>;
}

const EMPTY_BANDS: FrequencyBands = { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 };
const emptyTelemetry = (): MuseTelemetry => ({
  channels: { TP9: [], AF7: [], AF8: [], TP10: [] },
  bands: EMPTY_BANDS,
  packetCount: 0,
  sampleCount: 0,
  lastPacketAt: null,
  channelLastPacketAt: {},
});

export function createMuseTelemetryStore() {
  let buffers: Record<string, number[]> = { TP9: [], AF7: [], AF8: [], TP10: [] };
  let packetCount = 0;
  let sampleCount = 0;
  let channelSamples: Record<string, number> = { TP9: 0, AF7: 0, AF8: 0, TP10: 0 };
  let lastPacketAt: number | null = null;
  let channelLastPacketAt: Record<string, number> = {};
  let snapshot: MuseTelemetry = emptyTelemetry();
  let publishedPackets = 0;
  const listeners = new Set<() => void>();
  let starts = 0;
  let stopFn: (() => void) | null = null;

  const emit = () => { for (const l of listeners) l(); };

  /** Publish a new snapshot only if packets arrived since the last publish. */
  const publish = () => {
    if (packetCount === publishedPackets) return;
    publishedPackets = packetCount;
    const channels = Object.fromEntries(CHANNELS.map((c) => [c, [...buffers[c]]]));
    const complete = CHANNELS.map((c) => channels[c].slice(-256)).filter((s) => s.length === 256);
    snapshot = {
      channels,
      bands: relativeChannelBandPowers(complete),
      packetCount,
      sampleCount,
      lastPacketAt,
      channelLastPacketAt: { ...channelLastPacketAt },
    };
    emit();
  };

  const ingest = (payload: EegSample) => {
    if (!CHANNELS.includes(payload.channel as (typeof CHANNELS)[number])) return;
    buffers[payload.channel] = [...(buffers[payload.channel] ?? []), ...payload.samples].slice(-MAX_BUFFER);
    packetCount += 1;
    sampleCount += payload.samples.length;
    channelSamples[payload.channel] += payload.samples.length;
    lastPacketAt = Date.now();
    channelLastPacketAt[payload.channel] = lastPacketAt;
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    getSnapshot: () => snapshot,
    readCounters: () => ({ packetCount, sampleCount, channelSamples: { ...channelSamples } }),
    reset() {
      buffers = { TP9: [], AF7: [], AF8: [], TP10: [] };
      packetCount = 0;
      publishedPackets = 0;
      sampleCount = 0;
      channelSamples = { TP9: 0, AF7: 0, AF8: 0, TP10: 0 };
      lastPacketAt = null;
      channelLastPacketAt = {};
      snapshot = emptyTelemetry();
      emit();
    },
    /** Test seam: feed a packet without Tauri. */
    ingest,
    /** Start listening (ref-counted); returns a stop function. */
    start(): () => void {
      starts += 1;
      if (starts === 1) {
        let cancelled = false;
        let unlisten: UnlistenFn | null = null;
        void listen<EegSample>("muse-eeg", ({ payload }) => { if (!cancelled) ingest(payload); })
          .then((cleanup) => { if (cancelled) cleanup(); else unlisten = cleanup; })
          .catch((error) => console.warn("[muse] EEG event listener unavailable", error));
        const timer = window.setInterval(publish, UI_INTERVAL_MS);
        stopFn = () => { cancelled = true; unlisten?.(); window.clearInterval(timer); };
      }
      let stopped = false;
      return () => {
        if (stopped) return;
        stopped = true;
        starts -= 1;
        if (starts === 0) { stopFn?.(); stopFn = null; }
      };
    },
  };
}

export type MuseTelemetryStore = ReturnType<typeof createMuseTelemetryStore>;

/** App-wide store instance. */
export const museTelemetry = createMuseTelemetryStore();
