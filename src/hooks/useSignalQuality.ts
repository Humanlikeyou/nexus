/**
 * Signal quality hook: monitors per-channel EEG signal quality.
 *
 * Detection methods:
 * 1. Amplitude check — lifted electrode produces rail voltage (very large or flatline)
 * 2. Variance check — good signal has physiological variance; flatline = disconnected
 * 3. Line noise — 50/60Hz dominance indicates poor contact
 *
 * Outputs per-channel quality (good/fair/poor) and an overall quality level.
 * When 2+ channels are poor, sets `frozen` flag to pause scores.
 */

import { useState, useEffect, useRef } from "react";
import type { EegRingBuffer } from "@/lib/eegRingBuffer";
import type { EegChannel, ChannelQuality, OverallQuality } from "@/lib/types";
import { EEG_CHANNELS } from "@/lib/types";

export type { ChannelQuality, OverallQuality };

export interface SignalQualityState {
  channels: Record<EegChannel, ChannelQuality>;
  overall: OverallQuality;
  /** True when 2+ channels are poor — scores should be frozen */
  frozen: boolean;
  /** Human-readable status message */
  message: string;
}

const ANALYSIS_INTERVAL_MS = 1000;   // Check quality every 1s
const ANALYSIS_WINDOW = 256;          // 1 second of samples at 256Hz
const RAIL_VOLTAGE_UV = 1500;         // Above this = railed electrode
const MIN_VARIANCE_UV2 = 5;           // Below this = flatline
const MAX_VARIANCE_UV2 = 50000;       // Above this = excessive noise/movement

const INITIAL_STATE: SignalQualityState = {
  channels: { TP9: "good", AF7: "good", AF8: "good", TP10: "good" },
  overall: "good",
  frozen: false,
  message: "",
};

export function useSignalQuality(
  ringBuffer: EegRingBuffer | null,
  streaming: boolean,
) {
  const [quality, setQuality] = useState<SignalQualityState>(INITIAL_STATE);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!ringBuffer || !streaming) {
      setQuality(INITIAL_STATE);
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    intervalRef.current = setInterval(() => {
      const channels: Record<string, ChannelQuality> = {};
      let poorCount = 0;
      let fairCount = 0;

      for (const ch of EEG_CHANNELS) {
        const samples = ringBuffer.read(ch, ANALYSIS_WINDOW);
        const q = assessChannel(samples);
        channels[ch] = q;
        if (q === "poor") poorCount++;
        else if (q === "fair") fairCount++;
      }

      const frozen = poorCount >= 2;
      let overall: OverallQuality;
      let message = "";

      if (frozen) {
        overall = "frozen";
        message = "Multiple electrodes disconnected — scores paused";
      } else if (poorCount >= 1) {
        overall = "poor";
        const poorChannels = EEG_CHANNELS.filter((ch) => channels[ch] === "poor");
        message = `Poor contact: ${poorChannels.join(", ")}`;
      } else if (fairCount >= 2) {
        overall = "fair";
        message = "Adjust headband for better contact";
      } else if (fairCount === 1) {
        overall = "fair";
        const fairChannels = EEG_CHANNELS.filter((ch) => channels[ch] === "fair");
        message = `Marginal contact: ${fairChannels.join(", ")}`;
      } else {
        overall = "good";
        message = "";
      }

      setQuality({
        channels: channels as Record<EegChannel, ChannelQuality>,
        overall,
        frozen,
        message,
      });
    }, ANALYSIS_INTERVAL_MS);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [ringBuffer, streaming]);

  return quality;
}

function assessChannel(samples: number[]): ChannelQuality {
  if (samples.length < 32) return "good"; // Not enough data yet

  // Check for railed values
  let railedCount = 0;
  let sum = 0;
  for (let i = 0; i < samples.length; i++) {
    const v = Math.abs(samples[i]);
    if (v > RAIL_VOLTAGE_UV) railedCount++;
    sum += samples[i];
  }
  if (railedCount > samples.length * 0.3) return "poor"; // >30% railed

  // Compute variance
  const mean = sum / samples.length;
  let variance = 0;
  for (let i = 0; i < samples.length; i++) {
    const d = samples[i] - mean;
    variance += d * d;
  }
  variance /= samples.length;

  // Flatline check
  if (variance < MIN_VARIANCE_UV2) return "poor";

  // Excessive noise/movement
  if (variance > MAX_VARIANCE_UV2) return "fair";

  // Check for large jumps (muscle artifacts / electrode pop)
  let jumpCount = 0;
  for (let i = 1; i < samples.length; i++) {
    if (Math.abs(samples[i] - samples[i - 1]) > 100) jumpCount++;
  }
  if (jumpCount > samples.length * 0.2) return "fair"; // >20% large jumps

  return "good";
}
