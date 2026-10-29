/**
 * Artifact detection hook: runs detectors on ring buffer data every 1s.
 * Maintains rolling jaw-clench baseline (EMA), keeps last 60 events,
 * computes artifact rate (events/min over 60s window).
 */

import { useState, useEffect, useRef } from "react";
import type { EegRingBuffer } from "@/lib/eegRingBuffer";
import type { ArtifactEvent, EegChannel } from "@/lib/types";
import { EEG_CHANNELS } from "@/lib/types";
import { detectAllArtifacts, computeJawPower } from "@/lib/artifactDetection";

const ANALYSIS_INTERVAL_MS = 1000;
const ANALYSIS_WINDOW = 256; // 1s at 256Hz
const MAX_EVENTS = 60;
const RATE_WINDOW_MS = 60_000; // 60s window for rate calculation
const JAW_EMA_ALPHA = 0.1;
const JAW_WARMUP_MS = 5000; // Skip first 5s for jaw baseline

export interface ArtifactDetectionState {
  recentArtifacts: ArtifactEvent[];
  artifactRate: number; // events per minute
  jawBaseline: number;
}

export function useArtifactDetection(
  ringBuffer: EegRingBuffer | null,
  streaming: boolean,
): ArtifactDetectionState {
  const [state, setState] = useState<ArtifactDetectionState>({
    recentArtifacts: [],
    artifactRate: 0,
    jawBaseline: 0,
  });

  const eventsRef = useRef<ArtifactEvent[]>([]);
  const jawBaselineRef = useRef(0);
  const streamStartRef = useRef(0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!ringBuffer || !streaming) {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      eventsRef.current = [];
      jawBaselineRef.current = 0;
      streamStartRef.current = 0;
      setState({ recentArtifacts: [], artifactRate: 0, jawBaseline: 0 });
      return;
    }

    streamStartRef.current = Date.now();

    intervalRef.current = setInterval(() => {
      const now = Date.now();

      // Read last 256 samples per channel
      const channelSamples: Record<string, number[]> = {};
      for (const ch of EEG_CHANNELS) {
        channelSamples[ch] = ringBuffer.read(ch as EegChannel, ANALYSIS_WINDOW);
      }

      // Update jaw baseline (skip warmup period)
      const elapsed = now - streamStartRef.current;
      if (elapsed > JAW_WARMUP_MS) {
        const af7 = channelSamples["AF7"] || [];
        const af8 = channelSamples["AF8"] || [];
        const avgJaw = (computeJawPower(af7) + computeJawPower(af8)) / 2;

        if (jawBaselineRef.current <= 0) {
          jawBaselineRef.current = avgJaw;
        } else {
          jawBaselineRef.current =
            JAW_EMA_ALPHA * avgJaw + (1 - JAW_EMA_ALPHA) * jawBaselineRef.current;
        }
      }

      // Run all artifact detectors
      const newEvents = detectAllArtifacts(channelSamples, jawBaselineRef.current, now);

      if (newEvents.length > 0) {
        eventsRef.current = [...eventsRef.current, ...newEvents].slice(-MAX_EVENTS);
      }

      // Compute rate: events in last 60s
      const cutoff = now - RATE_WINDOW_MS;
      const recentCount = eventsRef.current.filter((e) => e.timestamp > cutoff).length;
      const windowMinutes = Math.min(elapsed, RATE_WINDOW_MS) / 60_000;
      const rate = windowMinutes > 0 ? recentCount / windowMinutes : 0;

      setState({
        recentArtifacts: eventsRef.current,
        artifactRate: Math.round(rate * 10) / 10,
        jawBaseline: jawBaselineRef.current,
      });
    }, ANALYSIS_INTERVAL_MS);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [ringBuffer, streaming]);

  return state;
}
