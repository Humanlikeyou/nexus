import { memo, useState, useMemo, useRef, useCallback } from "react";
import ReactECharts from "echarts-for-react";
import type { EegDebugInfo } from "@/hooks/useMuse";
import type { EegDisplayData } from "@/hooks/useEegDisplay";
import type { EegDisplayConfig, ChannelQuality } from "@/lib/types";
import { EEG_CHANNELS } from "@/lib/types";
import { CHART_THEME } from "@/lib/chart-theme";
import { EegControls } from "./EegControls";

type ViewMode = "raw" | "clean";

interface BrainWavesProps {
  displayData: EegDisplayData;
  config: EegDisplayConfig;
  onUpdateConfig: (patch: Partial<EegDisplayConfig>) => void;
  streaming: boolean;
  eegDebug?: EegDebugInfo;
  channelQuality?: Record<string, ChannelQuality>;
}

const channelColors: Record<string, string> = {
  TP9: "#8b5cf6",
  AF7: "#06b6d4",
  AF8: "#f59e0b",
  TP10: "#10b981",
};

const qualityDotColors: Record<ChannelQuality, string> = {
  good: "#10b981",
  fair: "#f59e0b",
  poor: "#ef4444",
};

/** Clip spikes beyond visual limit (µV). Visual only — does not affect DSP. */
const CLIP_LIMIT = 300;

function clip(v: number): number {
  return Math.max(-CLIP_LIMIT, Math.min(CLIP_LIMIT, v));
}

/** Moving average smoothing for visual readability. Does not affect DSP. */
function smooth(samples: number[], windowSize = 5): number[] {
  const result = new Array<number>(samples.length);
  for (let i = 0; i < samples.length; i++) {
    const start = Math.max(0, i - windowSize + 1);
    let sum = 0;
    for (let j = start; j <= i; j++) {
      sum += samples[j];
    }
    result[i] = sum / (i - start + 1);
  }
  return result;
}

/** Process chart data for display: clip + optional smoothing */
function processForDisplay(
  chartData: Record<string, number[]>,
  channels: string[],
  mode: ViewMode,
): Record<string, number[]> {
  const result: Record<string, number[]> = {};
  for (const ch of channels) {
    const raw = chartData[ch];
    if (!raw || raw.length === 0) {
      result[ch] = [];
      continue;
    }
    // Always clip spikes
    const clipped = raw.map(clip);
    // Smooth only in clean mode
    result[ch] = mode === "clean" ? smooth(clipped) : clipped;
  }
  return result;
}

function BrainWavesInner({
  displayData,
  config,
  onUpdateConfig,
  streaming,
  eegDebug,
  channelQuality,
}: BrainWavesProps) {
  const [viewMode, setViewMode] = useState<ViewMode>("clean");
  const chartRef = useRef<ReactECharts | null>(null);

  const { chartData, timeLabels, yMin, yMax } = displayData;
  const channels = config.visibleChannels;
  const hasData = Object.values(chartData).some((arr) => arr.length > 0);

  // Process data for display (clip + optional smooth). Memoize to avoid recompute on every render.
  const processedData = useMemo(
    () => processForDisplay(chartData, channels, viewMode),
    [chartData, channels, viewMode],
  );

  const chartOption = useMemo(() => ({
    backgroundColor: "transparent",
    animation: false,
    grid: { top: 30, right: 20, bottom: 30, left: 50 },
    legend: {
      data: channels,
      top: 0,
      textStyle: { color: "#888", fontSize: CHART_THEME.axisLabel.fontSize },
      itemWidth: 12,
      itemHeight: 2,
    },
    xAxis: {
      type: "category" as const,
      data: timeLabels,
      show: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: {
        color: "#555",
        fontSize: CHART_THEME.axisLabel.fontSize,
        formatter: (v: string) => `${Number(v).toFixed(1)}s`,
        interval: Math.max(1, Math.floor(timeLabels.length / 6)),
      },
      splitLine: { show: false },
    },
    yAxis: {
      type: "value" as const,
      min: yMin,
      max: yMax,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: "rgba(255,255,255,0.04)" } },
      axisLabel: { color: "#555", fontSize: CHART_THEME.axisLabel.fontSize, formatter: (v: number) => `${v}\u00B5V` },
    },
    series: channels.map((ch) => ({
      name: ch,
      type: "line" as const,
      data: processedData[ch] || [],
      showSymbol: false,
      lineStyle: {
        width: viewMode === "clean" ? 1.2 : 1,
        color: channelColors[ch],
        opacity: viewMode === "clean" ? 0.85 : 1,
      },
      smooth: false,
    })),
    tooltip: { show: false },
  }), [channels, timeLabels, yMin, yMax, processedData, viewMode]);

  // Debounced config update to prevent freeze on rapid changes
  const pendingUpdate = useRef<ReturnType<typeof setTimeout> | null>(null);
  const debouncedUpdateConfig = useCallback(
    (patch: Partial<EegDisplayConfig>) => {
      if (pendingUpdate.current) clearTimeout(pendingUpdate.current);
      pendingUpdate.current = setTimeout(() => {
        onUpdateConfig(patch);
        pendingUpdate.current = null;
      }, 100);
    },
    [onUpdateConfig],
  );

  const timeSinceLastPacket = eegDebug?.lastPacketAt
    ? ((Date.now() - eegDebug.lastPacketAt) / 1000).toFixed(1)
    : null;
  const noPacketsWhileStreaming = streaming && eegDebug && eegDebug.packetsReceived === 0;

  return (
    <div className="glass p-6 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h3 className="text-base font-medium text-muted-foreground">
            Real-time EEG
          </h3>
          {/* Electrode contact quality indicators */}
          {streaming && channelQuality && (
            <div className="flex items-center gap-2">
              {EEG_CHANNELS.map((ch) => {
                const q = channelQuality[ch] || "good";
                return (
                  <div key={ch} className="flex items-center gap-0.5" title={`${ch}: ${q}`}>
                    <div
                      className="w-1.5 h-1.5 rounded-full"
                      style={{ backgroundColor: qualityDotColors[q] }}
                    />
                    <span className="text-[8px] font-mono" style={{ color: qualityDotColors[q] }}>
                      {ch}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* View mode toggle */}
          {streaming && (
            <div className="flex items-center rounded border border-border/20 overflow-hidden text-[13px]">
              <button
                onClick={() => setViewMode("raw")}
                className={`px-2 py-0.5 transition-colors ${
                  viewMode === "raw"
                    ? "bg-muted/30 text-foreground"
                    : "text-muted-foreground/50 hover:text-muted-foreground"
                }`}
              >
                RAW
              </button>
              <button
                onClick={() => setViewMode("clean")}
                className={`px-2 py-0.5 transition-colors ${
                  viewMode === "clean"
                    ? "bg-muted/30 text-foreground"
                    : "text-muted-foreground/50 hover:text-muted-foreground"
                }`}
              >
                CLEAN
              </button>
            </div>
          )}
          {streaming && (
            <div className="flex items-center gap-1.5">
              <div className="w-1.5 h-1.5 rounded-full bg-success animate-pulse-dot" />
              <span className="text-xs text-success">
                {config.windowSecs}s @ {config.displayFps}fps
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Controls */}
      {streaming && (
        <EegControls config={config} onUpdate={debouncedUpdateConfig} />
      )}

      <div className="relative">
        <ReactECharts
          ref={chartRef}
          option={chartOption}
          style={{ height: 200 }}
          opts={{ renderer: "canvas" }}
          notMerge={false}
        />

        {/* Overlay: streaming but no data */}
        {streaming && !hasData && (
          <div className="absolute inset-0 flex items-center justify-center bg-background/60 rounded-lg">
            <p className="text-xs text-warning font-medium">
              {noPacketsWhileStreaming
                ? "Streaming ON but no packets received — check Rust logs"
                : "Waiting for EEG data..."}
            </p>
          </div>
        )}

        {/* Overlay: not streaming, no data */}
        {!streaming && !hasData && (
          <div className="absolute inset-0 flex items-center justify-center">
            <p className="text-xs text-muted-foreground/50">
              Connect Muse headband and start streaming to see live EEG data
            </p>
          </div>
        )}
      </div>

      {/* Debug strip */}
      {streaming && eegDebug && (
        <div className="flex items-center gap-3 text-xs font-mono text-muted-foreground border-t border-border/20 pt-2">
          <span>
            pkts: <span className={eegDebug.packetsReceived > 0 ? "text-success" : "text-destructive"}>{eegDebug.packetsReceived}</span>
          </span>
          <span>
            last: {timeSinceLastPacket ? `${timeSinceLastPacket}s ago` : "never"}
          </span>
          <span>
            buf: {["TP9", "AF7", "AF8", "TP10"].map((ch) => `${ch}=${eegDebug.bufferLens[ch] || 0}`).join(" ")}
          </span>
          <span className="ml-auto text-muted-foreground/40">
            {viewMode === "clean" ? "smoothed" : "raw"} · ±{Math.abs(yMax)}µV
          </span>
          {noPacketsWhileStreaming && (
            <span className="text-destructive font-semibold">NO DATA</span>
          )}
        </div>
      )}
    </div>
  );
}

/** Memoized to prevent re-renders from parent state changes that don't affect the chart */
export const BrainWaves = memo(BrainWavesInner);
