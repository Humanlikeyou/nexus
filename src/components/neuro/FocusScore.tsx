import { CHART_THEME } from "@/lib/chart-theme";
import ReactECharts from "echarts-for-react";

interface FocusScoreProps {
  score: number;
  label?: string;
  color?: string;
  confidence?: number;
  frozen?: boolean;
}

export function FocusScore({
  score,
  label = "Focus",
  color = "#0891b2",
  confidence,
  frozen,
}: FocusScoreProps) {
  const containerOpacity =
    confidence != null && confidence < 1
      ? 0.3 + confidence * 0.7
      : 1;

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
          offsetCenter: [0, "40%"],
          fontSize: CHART_THEME.gauge.titleFontSize,
          color: "#888",
        },
        detail: {
          valueAnimation: true,
          offsetCenter: [0, "-5%"],
          fontSize: CHART_THEME.gauge.detailFontSize,
          fontWeight: "bold" as const,
          color: "#fff",
          formatter: (v: number) => `${Math.round(v)}`,
        },
        data: [{ value: score, name: label }],
      },
    ],
  };

  return (
    <div
      className="panel p-4 flex flex-col items-center relative"
      style={{ opacity: frozen ? 0.3 : containerOpacity }}
    >
      <ReactECharts
        option={chartOption}
        style={{ height: 160, width: 160 }}
        opts={{ renderer: "canvas" }}
      />
      {frozen && (
        <div className="absolute inset-0 flex items-center justify-center">
          <span className="text-xs uppercase tracking-wider text-muted-foreground font-medium bg-background/80 px-2 py-1 rounded">
            Paused
          </span>
        </div>
      )}
    </div>
  );
}
