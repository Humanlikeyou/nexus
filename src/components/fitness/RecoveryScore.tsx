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
          width: CHART_THEME.gauge.progressWidth,
          itemStyle: { color },
        },
        axisLine: {
          lineStyle: { width: CHART_THEME.gauge.axisLineWidth, color: [[1, "rgba(30, 37, 48, 0.3)"]] },
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
    <div className="panel p-4 flex flex-col items-center relative">
      <h3 className="heading-section mb-1">Recovery</h3>
      <ReactECharts
        option={chartOption}
        style={{ height: 180, width: 180 }}
        opts={{ renderer: "canvas" }}
      />
      {metrics.ready && (
        <div className="flex gap-4 mt-1">
          <span className="metric-label">RHR: {metrics.restingHR}</span>
          {metrics.hrvBaseline > 0 && <span className="metric-label">HRV: {metrics.hrvBaseline}</span>}
        </div>
      )}
      {showPlaceholder && !streaming && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/70 rounded">
          <span className="text-[10px] font-mono text-muted-foreground/50 uppercase tracking-wider px-2.5 py-1 rounded-sm bg-muted/10 border border-border/35">
            Connect Whoop
          </span>
        </div>
      )}
    </div>
  );
}
