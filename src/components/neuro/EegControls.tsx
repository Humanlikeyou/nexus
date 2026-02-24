/**
 * EEG display controls: time window, FPS, Y-range, channel toggles, auto-scale.
 */

import type { EegDisplayConfig, EegChannel } from "@/lib/types";
import { EEG_CHANNELS } from "@/lib/types";

const channelColors: Record<string, string> = {
  TP9: "#8b5cf6",
  AF7: "#06b6d4",
  AF8: "#f59e0b",
  TP10: "#10b981",
};

interface EegControlsProps {
  config: EegDisplayConfig;
  onUpdate: (patch: Partial<EegDisplayConfig>) => void;
}

export function EegControls({ config, onUpdate }: EegControlsProps) {
  return (
    <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
      {/* Time window */}
      <label className="flex items-center gap-1">
        <span>Window</span>
        <select
          value={config.windowSecs}
          onChange={(e) => onUpdate({ windowSecs: Number(e.target.value) })}
          className="bg-muted/20 border border-border/20 rounded px-1 py-0.5 text-xs"
        >
          {[1, 2, 3, 5, 10, 15, 30].map((s) => (
            <option key={s} value={s}>
              {s}s
            </option>
          ))}
        </select>
      </label>

      {/* Display FPS */}
      <label className="flex items-center gap-1">
        <span>FPS</span>
        <select
          value={config.displayFps}
          onChange={(e) => onUpdate({ displayFps: Number(e.target.value) })}
          className="bg-muted/20 border border-border/20 rounded px-1 py-0.5 text-xs"
        >
          {[5, 10, 15, 20, 30].map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </label>

      {/* Y-Range */}
      <label className="flex items-center gap-1">
        <span>Y</span>
        <select
          value={config.autoScale ? "auto" : config.yRange}
          onChange={(e) => {
            if (e.target.value === "auto") {
              onUpdate({ autoScale: true });
            } else {
              onUpdate({ yRange: Number(e.target.value), autoScale: false });
            }
          }}
          className="bg-muted/20 border border-border/20 rounded px-1 py-0.5 text-xs"
        >
          {[50, 100, 200, 500, 1000].map((r) => (
            <option key={r} value={r}>
              {"\u00B1"}{r}{"\u00B5V"}
            </option>
          ))}
          <option value="auto">Auto</option>
        </select>
      </label>

      {/* Channel toggles */}
      <div className="flex items-center gap-1.5 ml-1">
        {EEG_CHANNELS.map((ch) => {
          const active = config.visibleChannels.includes(ch);
          return (
            <button
              key={ch}
              onClick={() => {
                const next = active
                  ? config.visibleChannels.filter((c) => c !== ch)
                  : [...config.visibleChannels, ch];
                if (next.length > 0) onUpdate({ visibleChannels: next as EegChannel[] });
              }}
              className="px-2 py-0.5 rounded-md font-medium text-[13px] font-mono border transition-colors"
              style={{
                borderColor: active ? channelColors[ch] : "transparent",
                color: active ? channelColors[ch] : "rgba(136,136,136,0.4)",
                backgroundColor: active ? `${channelColors[ch]}15` : "transparent",
              }}
            >
              {ch}
            </button>
          );
        })}
      </div>
    </div>
  );
}
