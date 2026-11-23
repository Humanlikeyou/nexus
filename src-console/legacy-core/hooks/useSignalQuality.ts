/**
 * Signal quality hook: monitors per-channel EEG signal quality.
 *
 * Detection methods:
 * 1. Amplitude check — a lifted electrode sits at the rail of the ±1000 µV 12-bit range
 * 2. Spread check, with mains hum removed first — hum alone says nothing about contact, but hum
 *    with no signal under it is a sensor off the skin, and sustained noise is a loose one
 * 3. Jump check — large sample-to-sample jumps, also with the hum removed
 *
 * The spread is robust (from the median), so a blink is left to artifact detection. With hum
 * removed, worn sensors read about 3 to 15 µV; their raw sd of 14 to 67 µV is mostly hum.
 *
 * Outputs per-channel quality (good/fair/poor) and an overall quality level.
 * When 2+ channels are poor, sets `frozen` flag to pause scores.
 */

import { useState, useEffect, useRef } from "react";
import type { EegRingBuffer } from "../lib/eegRingBuffer";
import type { EegChannel, ChannelQuality, OverallQuality } from "../lib/types";
import { EEG_CHANNELS } from "../lib/types";
import { removeLineNoise } from "../lib/lineNoise";

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
const RAIL_VOLTAGE_UV = 900;          // Above this = railed electrode (full scale is about 1000)
const FLATLINE_UV = 1.5;              // Below this spread = no signal under any hum
const FAIR_SPREAD_UV = 40;            // Above this spread = sustained noise
const POOR_SPREAD_UV = 100;           // Above this spread = the sensor isn't reading EEG

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

/** Standard deviation from the median absolute deviation: brief spikes barely move it. */
function robustSpread(samples: number[]): number {
  const middle = (values: number[]) => [...values].sort((a, b) => a - b)[values.length >> 1];
  const center = middle(samples);
  return 1.4826 * middle(samples.map((value) => Math.abs(value - center)));
}

export function assessChannel(samples: number[]): ChannelQuality {
  if (samples.length < 32) return "good"; // Not enough data yet

  // Check for railed values. Hum can rail a sensor too, so this reads the raw signal.
  let railedCount = 0;
  for (let i = 0; i < samples.length; i++) {
    if (Math.abs(samples[i]) > RAIL_VOLTAGE_UV) railedCount++;
  }
  if (railedCount > samples.length * 0.3) return "poor"; // >30% railed

  const clean = removeLineNoise(samples);
  const spread = robustSpread(clean);

  // Flatline, or hum with nothing under it
  if (spread < FLATLINE_UV) return "poor";

  // Sustained noise that a blink can't explain
  if (spread > POOR_SPREAD_UV) return "poor";
  if (spread > FAIR_SPREAD_UV) return "fair";

  // Check for large jumps (muscle artifacts / electrode pop)
  let jumpCount = 0;
  for (let i = 1; i < clean.length; i++) {
    if (Math.abs(clean[i] - clean[i - 1]) > 100) jumpCount++;
  }
  if (jumpCount > clean.length * 0.2) return "fair"; // >20% large jumps

  return "good";
}
