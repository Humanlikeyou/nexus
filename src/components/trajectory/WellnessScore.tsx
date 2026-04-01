import ReactECharts from "echarts-for-react";
import type { WellnessScore as WellnessScoreType } from "@/lib/types";

interface WellnessScoreProps {
  score: WellnessScoreType;
}

export function WellnessScore({ score }: WellnessScoreProps) {
  // Only include axes that have real data
  const axes: { name: string; value: number }[] = [];
  if (score.recovery != null) axes.push({ name: "Recovery", value: score.recovery });
  if (score.focus != null) axes.push({ name: "Focus", value: score.focus });
  if (score.productivity != null) axes.push({ name: "Productivity", value: score.productivity });

  const hasData = axes.length > 0;

  const chartOption = {
    backgroundColor: "transparent",
    radar: {
      indicator: axes.map((a) => ({ name: a.name, max: 100 })),
      shape: "circle" as const,
      splitNumber: 4,
      axisName: { color: "rgba(139, 149, 163, 0.8)", fontSize: 11 },
      splitLine: { lineStyle: { color: "rgba(38, 48, 64, 0.4)" } },
      splitArea: { show: false },
      axisLine: { lineStyle: { color: "rgba(38, 48, 64, 0.5)" } },
    },
    series: [
      {
        type: "radar",
        data: hasData
          ? [
              {
                value: axes.map((a) => a.value),
                name: "Wellness",
                lineStyle: { color: "#4A80A3", width: 2 },
                areaStyle: { color: "rgba(74, 128, 163, 0.18)" },
                itemStyle: { color: "#4A80A3" },
              },
            ]
          : [],
      },
    ],
  };

  const statTiles = [
    { label: "Recovery", value: score.recovery, color: "text-success" },
    { label: "Focus", value: score.focus, color: "text-accent" },
    { label: "Productivity", value: score.productivity, color: "text-warning" },
  ];

  return (
    <div className="panel p-4 space-y-5">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">
          Combined Wellness Score
        </h3>
        <div className="text-right">
          <p className="text-2xl font-bold text-accent">{hasData ? score.overall : "--"}</p>
          <p className="text-xs text-muted-foreground">/ 100</p>
        </div>
      </div>

      {hasData ? (
        <ReactECharts
          option={chartOption}
          style={{ height: 220 }}
          opts={{ renderer: "canvas" }}
        />
      ) : (
        <div className="h-[220px] flex items-center justify-center">
          <span className="text-xs text-muted-foreground/60">
            Connect Muse or Whoop to populate wellness data
          </span>
        </div>
      )}

      <div className="grid grid-cols-3 gap-3">
        {statTiles.map(({ label, value, color }) => (
          <div key={label} className="text-center p-2 rounded-lg bg-muted/10">
            <p className={`text-lg font-semibold ${value != null ? color : "text-muted-foreground/50"}`}>
              {value != null ? value : "--"}
            </p>
            <p className="text-xs text-muted-foreground">{label}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
