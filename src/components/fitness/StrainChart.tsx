import { Flame } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { CHART_THEME } from "@/lib/chart-theme";
import type { StrainMetrics } from "@/lib/types";

interface StrainChartProps {
  metrics: StrainMetrics;
  streaming: boolean;
}

const ZONE_LABELS = ["Recovery", "Light", "Moderate", "Hard", "Max"];
const ZONE_COLORS = ["#10b981", "#06b6d4", "#f59e0b", "#f97316", "#ef4444"];

export function StrainChart({ metrics, streaming }: StrainChartProps) {
  const hasData = metrics.durationSecs > 0;

  const chartData = hasData
    ? [{ label: "Now", strain: metrics.strain }]
    : [];

  const chartOption = {
    backgroundColor: "transparent",
    grid: { top: 10, right: 10, bottom: 24, left: 35 },
    xAxis: {
      type: "category" as const,
      data: chartData.map((d) => d.label),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: CHART_THEME.axisLabel.color, fontSize: CHART_THEME.axisLabel.fontSize },
    },
    yAxis: {
      type: "value" as const,
      min: 0,
      max: 21,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: CHART_THEME.splitLine.color } },
      axisLabel: { color: CHART_THEME.axisLabelMuted.color, fontSize: CHART_THEME.axisLabel.fontSize },
    },
    series: [
      {
        type: "line" as const,
        data: chartData.map((d) => d.strain),
        smooth: true,
        showSymbol: true,
        symbolSize: 6,
        lineStyle: { color: "#f59e0b", width: 2 },
        itemStyle: { color: "#f59e0b" },
        areaStyle: {
          color: {
            type: "linear" as const,
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(245, 158, 11, 0.2)" },
              { offset: 1, color: "rgba(245, 158, 11, 0)" },
            ],
          },
        },
      },
    ],
    tooltip: {
      trigger: "axis" as const,
      backgroundColor: "rgba(0,0,0,0.8)",
      borderColor: "rgba(255,255,255,0.1)",
      borderRadius: CHART_THEME.tooltip.borderRadius,
      textStyle: { color: "#fff", fontSize: CHART_THEME.tooltip.textStyle.fontSize },
    },
  };

  return (
    <div className="panel p-4 space-y-3 relative">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Flame size={16} className="text-warning" />
          <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">Strain</h3>
        </div>
        <div className="text-right">
          <p className="text-lg font-semibold">
            {hasData ? metrics.strain.toFixed(1) : "--"}
          </p>
          <p className="text-xs text-muted-foreground">
            {hasData ? `${formatDuration(metrics.durationSecs)} / 21.0` : "No data"}
          </p>
        </div>
      </div>

      {chartData.length > 0 ? (
        <ReactECharts
          option={chartOption}
          style={{ height: 140 }}
          opts={{ renderer: "canvas" }}
        />
      ) : (
        <div className="h-[140px] flex items-center justify-center">
          <span className="text-xs text-muted-foreground/60">
            {streaming ? "Accumulating..." : "Start streaming to track strain"}
          </span>
        </div>
      )}

      {/* HR Zone breakdown */}
      {hasData && (
        <div className="flex gap-1">
          {metrics.hrZones.map((secs, i) => {
            const total = metrics.hrZones.reduce((a, b) => a + b, 0);
            const pct = total > 0 ? (secs / total) * 100 : 0;
            return (
              <div
                key={i}
                className="flex flex-col items-center gap-0.5"
                style={{ flex: Math.max(pct, 5) }}
              >
                <div
                  className="w-full h-1.5 rounded-sm"
                  style={{ backgroundColor: ZONE_COLORS[i], opacity: pct > 0 ? 1 : 0.2 }}
                />
                <span className="text-[8px] text-muted-foreground/50">{ZONE_LABELS[i]}</span>
              </div>
            );
          })}
        </div>
      )}

      {!hasData && !streaming && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/60 backdrop-blur-sm rounded-lg">
          <span className="text-xs text-muted-foreground/60 font-medium px-3 py-1.5 rounded-full bg-muted/20 border border-border/20">
            Connect Whoop to start
          </span>
        </div>
      )}
    </div>
  );
}

function formatDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}
