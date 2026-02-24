/**
 * SignalQualityBar: visual indicator of per-channel signal quality.
 * Shows colored dots per channel and a banner when frozen.
 */

import type { SignalQualityState, ChannelQuality } from "@/hooks/useSignalQuality";
import type { EegChannel } from "@/lib/types";
import { EEG_CHANNELS } from "@/lib/types";

interface SignalQualityBarProps {
  quality: SignalQualityState;
  streaming: boolean;
}

const qualityColors: Record<ChannelQuality, string> = {
  good: "#10b981",   // green
  fair: "#f59e0b",   // amber
  poor: "#ef4444",   // red
};

const qualityLabels: Record<ChannelQuality, string> = {
  good: "Good",
  fair: "Fair",
  poor: "Poor",
};

export function SignalQualityBar({ quality, streaming }: SignalQualityBarProps) {
  if (!streaming) return null;

  return (
    <div className="space-y-2">
      {/* Frozen banner */}
      {quality.frozen && (
        <div className="px-3 py-2 rounded-lg bg-destructive/10 border border-destructive/30 flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-destructive animate-pulse" />
          <span className="text-xs text-destructive font-medium">
            {quality.message}
          </span>
        </div>
      )}

      {/* Channel indicators */}
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">Signal:</span>
        {EEG_CHANNELS.map((ch) => {
          const q = quality.channels[ch];
          return (
            <div key={ch} className="flex items-center gap-1">
              <div
                className="w-2 h-2 rounded-full"
                style={{ backgroundColor: qualityColors[q] }}
              />
              <span
                className="text-[13px] font-mono"
                style={{ color: qualityColors[q] }}
              >
                {ch}
              </span>
            </div>
          );
        })}
        {quality.message && !quality.frozen && (
          <span className="text-[13px] text-warning ml-auto">{quality.message}</span>
        )}
      </div>
    </div>
  );
}
