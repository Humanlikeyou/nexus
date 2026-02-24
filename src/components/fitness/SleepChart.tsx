import { Moon } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { CHART_THEME } from "@/lib/chart-theme";
import type { SleepMetrics } from "@/lib/types";

interface SleepChartProps {
  metrics: SleepMetrics;
  streaming: boolean;
}

const PLACEHOLDER_STAGES = [
  { name: "Awake" as const, hours: 0, color: "#ef4444" },
  { name: "Light" as const, hours: 0, color: "#06b6d4" },
  { name: "REM" as const, hours: 0, color: "#8b5cf6" },
  { name: "Deep (SWS)" as const, hours: 0, color: "#6366f1" },
];

function formatHours(h: number): string {
  const hrs = Math.floor(h);
  const mins = Math.round((h - hrs) * 60);
  if (hrs === 0) return `${mins}m`;
  return `${hrs}h ${mins}m`;
}

export function SleepChart({ metrics, streaming }: SleepChartProps) {
  const showPlaceholder = !metrics.ready;
  const stages = showPlaceholder ? PLACEHOLDER_STAGES : metrics.stages;
  const maxHours = Math.max(8, ...stages.map((s) => s.hours));

  const chartOption = {
    backgroundColor: "transparent",
    grid: { top: 10, right: 10, bottom: 24, left: 80 },
    xAxis: {
      type: "value" as const,
      max: Math.ceil(maxHours),
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { show: false },
      axisLabel: {
        color: "#555",
        fontSize: CHART_THEME.axisLabel.fontSize,
        formatter: (v: number) => `${v}h`,
      },
    },
    yAxis: {
      type: "category" as const,
      data: stages.map((s) => s.name),
      inverse: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: "#888", fontSize: CHART_THEME.axisLabel.fontSize },
    },
    series: [
      {
        type: "bar",
        data: stages.map((s) => ({
          value: s.hours,
          itemStyle: { color: showPlaceholder ? "rgba(255,255,255,0.08)" : s.color },
        })),
        barWidth: 16,
        itemStyle: { borderRadius: 4 },
        showBackground: true,
        backgroundStyle: { color: "rgba(255,255,255,0.03)", borderRadius: 4 },
        label: {
          show: !showPlaceholder,
          position: "right" as const,
          color: "#666",
          fontSize: CHART_THEME.axisLabel.fontSize,
          formatter: (p: { value: number }) => p.value > 0 ? `${p.value}h` : "",
        },
      },
    ],
    tooltip: { show: false },
  };

  return (
    <div className="glass p-6 space-y-3 relative">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Moon size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Sleep</h3>
        </div>
        <div className="text-right">
          <p className="text-lg font-semibold">
            {showPlaceholder ? "--" : formatHours(metrics.totalHours)}
          </p>
          <p className="text-xs text-muted-foreground">
            {showPlaceholder ? "No data" : `Score: ${metrics.score}%`}
          </p>
        </div>
      </div>

      <ReactECharts
        option={chartOption}
        style={{ height: 140 }}
        opts={{ renderer: "canvas" }}
      />
      {showPlaceholder && !streaming && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/60 backdrop-blur-sm rounded-lg">
          <span className="text-xs text-muted-foreground/60 font-medium px-3 py-1.5 rounded-full bg-muted/20 border border-border/20">
            Record a sleep session
          </span>
        </div>
      )}
    </div>
  );
}
