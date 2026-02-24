import ReactECharts from "echarts-for-react";
import { CHART_THEME } from "@/lib/chart-theme";
import type { RecoveryMetrics } from "@/lib/types";

interface RecoveryScoreProps {
  metrics: RecoveryMetrics;
  streaming: boolean;
}

export function RecoveryScore({ metrics, streaming }: RecoveryScoreProps) {
  const score = metrics.ready ? metrics.score : 0;
  const showPlaceholder = !metrics.ready;

  const getColor = (s: number) => {
    if (s >= 67) return "#10b981";
    if (s >= 34) return "#f59e0b";
    return "#ef4444";
  };

  const getLabel = (s: number) => {
    if (s >= 67) return "Green";
    if (s >= 34) return "Yellow";
    return "Red";
  };

  const color = showPlaceholder ? "rgba(255,255,255,0.15)" : getColor(score);

  const chartOption = {
    backgroundColor: "transparent",
    series: [
      {
        type: "gauge",
        startAngle: 220,
        endAngle: -40,
        min: 0,
        max: 100,
        progress: {
          show: true,
          width: 14,
          itemStyle: { color },
        },
        axisLine: {
          lineStyle: { width: 14, color: [[1, "rgba(255,255,255,0.05)"]] },
        },
        axisTick: { show: false },
        splitLine: { show: false },
        axisLabel: { show: false },
        anchor: { show: false },
        pointer: { show: false },
        title: {
          offsetCenter: [0, "45%"],
          fontSize: CHART_THEME.gauge.titleFontSize,
          color: "#888",
        },
        detail: {
          valueAnimation: true,
          offsetCenter: [0, "-5%"],
          fontSize: CHART_THEME.gauge.detailFontSize,
          fontWeight: "bold" as const,
          color: showPlaceholder ? "#555" : color,
          formatter: (v: number) => `${Math.round(v)}%`,
        },
        data: [{
          value: showPlaceholder ? 0 : score,
          name: showPlaceholder
            ? "Waiting for data..."
            : `Recovery · ${getLabel(score)}`,
        }],
      },
    ],
  };

  return (
    <div className="glass p-6 flex flex-col items-center relative">
      <h3 className="text-base font-medium text-muted-foreground mb-2">Recovery Score</h3>
      <ReactECharts
        option={chartOption}
        style={{ height: 180, width: 180 }}
        opts={{ renderer: "canvas" }}
      />
      {metrics.ready && (
        <div className="flex gap-4 text-xs text-muted-foreground mt-1">
          <span>RHR: {metrics.restingHR} bpm</span>
          {metrics.hrvBaseline > 0 && <span>HRV: {metrics.hrvBaseline} ms</span>}
        </div>
      )}
      {showPlaceholder && !streaming && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/60 backdrop-blur-sm rounded-lg">
          <span className="text-xs text-muted-foreground/60 font-medium px-3 py-1.5 rounded-full bg-muted/20 border border-border/20">
            Connect Whoop to start
          </span>
        </div>
      )}
    </div>
  );
}
