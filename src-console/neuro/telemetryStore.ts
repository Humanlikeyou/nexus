import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EegSample } from "../types";
import { relativeChannelBandPowers, type FrequencyBands } from "./fft";
import { GYRO_BUFFER } from "./headMotion";
import { PPG_BUFFER } from "./heartRate";

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
/** design-1110e E9: the live signal draws up to 10 s per channel (AUX included). */
export const TRACE_BUFFER = 2560;

export interface MuseTelemetry {
  channels: Record<string, number[]>;
  bands: FrequencyBands;
  packetCount: number;
  sampleCount: number;
  lastPacketAt: number | null;
  channelLastPacketAt: Record<string, number>;
  /** Muse auxiliary input, only once a real AUX packet has arrived. Unproven: see AUX_PROVEN. */
  aux?: { samples: number[]; lastPacketAt: number } | null;
  /** Recent gyroscope samples as [x, y, z] in deg/s, once a packet has arrived. */
  gyro?: { samples: number[][]; lastPacketAt: number } | null;
  /** Recent infrared PPG counts, once a packet has arrived. */
  ppg?: { samples: number[]; lastPacketAt: number } | null;
  /** Last 10 s per channel for the live signal lanes (TP9…TP10, and AUX once packets arrive). */
  trace?: Record<string, number[]>;
}

/** `muse-motion`, from muse.rs. */
export type MotionPacket = { timestamp: number; sensor: string; unit: string; sequence: number; samples: number[][] };
/** `muse-ppg`, from muse.rs. */
export type PpgPacket = { timestamp: number; channel: string; sequence: number; samples: number[] };

const EMPTY_BANDS: FrequencyBands = { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 };
const emptyTelemetry = (): MuseTelemetry => ({
  channels: { TP9: [], AF7: [], AF8: [], TP10: [] },
  bands: EMPTY_BANDS,
  packetCount: 0,
  sampleCount: 0,
  lastPacketAt: null,
  channelLastPacketAt: {},
  aux: null,
  gyro: null,
  ppg: null,
});

export function createMuseTelemetryStore() {
  let buffers: Record<string, number[]> = { TP9: [], AF7: [], AF8: [], TP10: [] };
  let packetCount = 0;
  let sampleCount = 0;
  let channelSamples: Record<string, number> = { TP9: 0, AF7: 0, AF8: 0, TP10: 0 };
  let lastPacketAt: number | null = null;
  let channelLastPacketAt: Record<string, number> = {};
  let aux: MuseTelemetry["aux"] = null;
  let gyro: MuseTelemetry["gyro"] = null;
  let ppg: MuseTelemetry["ppg"] = null;
  let trace: Record<string, number[]> = {};
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
      aux: aux && { samples: [...aux.samples], lastPacketAt: aux.lastPacketAt },
      gyro: gyro && { samples: [...gyro.samples], lastPacketAt: gyro.lastPacketAt },
      ppg: ppg && { samples: [...ppg.samples], lastPacketAt: ppg.lastPacketAt },
      trace: Object.fromEntries(Object.entries(trace).map(([c, v]) => [c, [...v]])),
    };
    emit();
  };

  const keepTrace = (channel: string, samples: number[]) => {
    const buf = trace[channel] ?? (trace[channel] = []);
    buf.push(...samples);
    if (buf.length > TRACE_BUFFER) buf.splice(0, buf.length - TRACE_BUFFER);
  };

  const ingest = (payload: EegSample) => {
    if (payload.channel === "AUX") {
      keepTrace("AUX", payload.samples);
      aux = { samples: [...(aux?.samples ?? []), ...payload.samples].slice(-MAX_BUFFER), lastPacketAt: Date.now() };
      return;
    }
    if (!CHANNELS.includes(payload.channel as (typeof CHANNELS)[number])) return;
    buffers[payload.channel] = [...(buffers[payload.channel] ?? []), ...payload.samples].slice(-MAX_BUFFER);
    keepTrace(payload.channel, payload.samples);
    packetCount += 1;
    sampleCount += payload.samples.length;
    channelSamples[payload.channel] += payload.samples.length;
    lastPacketAt = Date.now();
    channelLastPacketAt[payload.channel] = lastPacketAt;
  };

  const ingestMotion = (payload: MotionPacket) => {
    if (payload.sensor !== "gyroscope") return;
    gyro = { samples: [...(gyro?.samples ?? []), ...payload.samples].slice(-GYRO_BUFFER), lastPacketAt: Date.now() };
  };

  const ingestPpg = (payload: PpgPacket) => {
    if (payload.channel !== "infrared") return;
    ppg = { samples: [...(ppg?.samples ?? []), ...payload.samples].slice(-PPG_BUFFER), lastPacketAt: Date.now() };
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
      aux = null;
      gyro = null;
      ppg = null;
      trace = {};
      snapshot = emptyTelemetry();
      emit();
    },
    /** Test seam: feed a packet without Tauri. */
    ingest,
    ingestMotion,
    ingestPpg,
    /** Start listening (ref-counted); returns a stop function. */
    start(): () => void {
      starts += 1;
      if (starts === 1) {
        let cancelled = false;
        const unlisteners: UnlistenFn[] = [];
        const keep = (registered: Promise<UnlistenFn>, label: string) => {
          void registered
            .then((cleanup) => { if (cancelled) cleanup(); else unlisteners.push(cleanup); })
            .catch((error) => console.warn(`[muse] ${label} event listener unavailable`, error));
        };
        keep(listen<EegSample>("muse-eeg", ({ payload }) => { if (!cancelled) ingest(payload); }), "EEG");
        // Optional streams: a headset without them still trains on EEG.
        keep(listen<MotionPacket>("muse-motion", ({ payload }) => { if (!cancelled) ingestMotion(payload); }), "motion");
        keep(listen<PpgPacket>("muse-ppg", ({ payload }) => { if (!cancelled) ingestPpg(payload); }), "PPG");
        const timer = window.setInterval(publish, UI_INTERVAL_MS);
        stopFn = () => { cancelled = true; unlisteners.forEach((cleanup) => cleanup()); window.clearInterval(timer); };
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
