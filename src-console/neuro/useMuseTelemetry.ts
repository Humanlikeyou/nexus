import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { EegSample } from "../types";
import { relativeChannelBandPowers, type FrequencyBands } from "./fft";

const CHANNELS = ["TP9", "AF7", "AF8", "TP10"] as const;
const MAX_BUFFER = 512;
const UI_INTERVAL_MS = 250;

export interface MuseTelemetry {
  channels: Record<string, number[]>;
  bands: FrequencyBands;
  packetCount: number;
  sampleCount: number;
  lastPacketAt: number | null;
  channelLastPacketAt: Record<string, number>;
}

const EMPTY_BANDS: FrequencyBands = { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 };

export function useMuseTelemetry() {
  const buffers = useRef<Record<string, number[]>>({ TP9: [], AF7: [], AF8: [], TP10: [] });
  const packetCount = useRef(0);
  const sampleCount = useRef(0);
  const channelSamples = useRef<Record<string, number>>({ TP9: 0, AF7: 0, AF8: 0, TP10: 0 });
  const lastPacketAt = useRef<number | null>(null);
  const channelLastPacketAt = useRef<Record<string, number>>({});
  const [telemetry, setTelemetry] = useState<MuseTelemetry>({
    channels: { TP9: [], AF7: [], AF8: [], TP10: [] },
    bands: EMPTY_BANDS,
    packetCount: 0,
    sampleCount: 0,
    lastPacketAt: null,
    channelLastPacketAt: {},
  });

  const reset = useCallback(() => {
    buffers.current = { TP9: [], AF7: [], AF8: [], TP10: [] };
    packetCount.current = 0;
    sampleCount.current = 0;
    channelSamples.current = { TP9: 0, AF7: 0, AF8: 0, TP10: 0 };
    lastPacketAt.current = null;
    channelLastPacketAt.current = {};
    setTelemetry({ channels: { TP9: [], AF7: [], AF8: [], TP10: [] }, bands: EMPTY_BANDS, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} });
  }, []);

  const readCounters = useCallback(() => ({
    packetCount: packetCount.current,
    sampleCount: sampleCount.current,
    channelSamples: { ...channelSamples.current },
  }), []);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void listen<EegSample>("muse-eeg", ({ payload }) => {
      if (cancelled || !CHANNELS.includes(payload.channel as (typeof CHANNELS)[number])) return;
      const current = buffers.current[payload.channel] ?? [];
      buffers.current[payload.channel] = [...current, ...payload.samples].slice(-MAX_BUFFER);
      packetCount.current += 1;
      sampleCount.current += payload.samples.length;
      channelSamples.current[payload.channel] += payload.samples.length;
      lastPacketAt.current = Date.now();
      channelLastPacketAt.current[payload.channel] = lastPacketAt.current;
    }).then((cleanup) => {
      if (cancelled) cleanup();
      else unlisten = cleanup;
    }).catch((error) => console.warn("[muse] EEG event listener unavailable", error));

    const timer = window.setInterval(() => {
      const channels = Object.fromEntries(CHANNELS.map((channel) => [channel, [...buffers.current[channel]]]));
      const completeChannels = CHANNELS.map((channel) => channels[channel].slice(-256)).filter((samples) => samples.length === 256);
      setTelemetry({
        channels,
        bands: relativeChannelBandPowers(completeChannels),
        packetCount: packetCount.current,
        sampleCount: sampleCount.current,
        lastPacketAt: lastPacketAt.current,
        channelLastPacketAt: { ...channelLastPacketAt.current },
      });
    }, UI_INTERVAL_MS);

    return () => {
      cancelled = true;
      unlisten?.();
      window.clearInterval(timer);
    };
  }, []);

  return { telemetry, reset, readCounters };
}
