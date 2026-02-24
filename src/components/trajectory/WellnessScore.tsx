import ReactECharts from "echarts-for-react";
import { CHART_THEME } from "@/lib/chart-theme";
import type { WellnessScore as WellnessScoreType } from "@/lib/types";

interface WellnessScoreProps {
  score: WellnessScoreType;
}

export function WellnessScore({ score }: WellnessScoreProps) {
  const chartOption = {
    backgroundColor: "transparent",
    radar: {
      indicator: [
        { name: "Recovery", max: 100 },
        { name: "Focus", max: 100 },
        { name: "Productivity", max: 100 },
      ],
      shape: "circle" as const,
      splitNumber: 4,
      axisName: {
        color: "#888",
        fontSize: 11,
      },
      splitLine: {
        lineStyle: { color: "rgba(255,255,255,0.06)" },
      },
      splitArea: { show: false },
      axisLine: { lineStyle: { color: "rgba(255,255,255,0.1)" } },
    },
    series: [
      {
        type: "radar",
        data: [
          {
            value: [
              score.recovery,
              score.focus,
              score.productivity,
            ],
            name: "Wellness",
            lineStyle: { color: "#0891b2", width: 2 },
            areaStyle: { color: "rgba(8, 145, 178, 0.15)" },
            itemStyle: { color: "#0891b2" },
          },
        ],
      },
    ],
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-medium text-muted-foreground">
          Combined Wellness Score
        </h3>
        <div className="text-right">
          <p className="text-2xl font-bold text-accent">{score.overall}</p>
          <p className="text-xs text-muted-foreground">/ 100</p>
        </div>
      </div>

      <ReactECharts
        option={chartOption}
        style={{ height: 220 }}
        opts={{ renderer: "canvas" }}
      />

      <div className="grid grid-cols-3 gap-3">
        <div className="text-center p-2 rounded-lg bg-muted/10">
          <p className="text-lg font-semibold text-success">{score.recovery}</p>
          <p className="text-xs text-muted-foreground">Recovery</p>
        </div>
        <div className="text-center p-2 rounded-lg bg-muted/10">
          <p className="text-lg font-semibold text-accent">{score.focus}</p>
          <p className="text-xs text-muted-foreground">Focus</p>
        </div>
        <div className="text-center p-2 rounded-lg bg-muted/10">
          <p className="text-lg font-semibold text-warning">{score.productivity}</p>
          <p className="text-xs text-muted-foreground">Productivity</p>
        </div>
      </div>
    </div>
  );
}
