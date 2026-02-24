import { Coins } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { CHART_THEME } from "@/lib/chart-theme";

interface TokenUsageProps {
  loading: boolean;
}

export function TokenUsage({ loading }: TokenUsageProps) {
  // Placeholder data — will be populated from actual token tracking
  const dailyUsage = [
    { day: "Mon", tokens: 12400, cost: 0.31 },
    { day: "Tue", tokens: 18200, cost: 0.46 },
    { day: "Wed", tokens: 9800, cost: 0.25 },
    { day: "Thu", tokens: 22100, cost: 0.55 },
    { day: "Fri", tokens: 15600, cost: 0.39 },
    { day: "Sat", tokens: 8400, cost: 0.21 },
    { day: "Sun", tokens: 11200, cost: 0.28 },
  ];

  const totalTokens = dailyUsage.reduce((sum, d) => sum + d.tokens, 0);
  const totalCost = dailyUsage.reduce((sum, d) => sum + d.cost, 0);

  const chartOption = {
    backgroundColor: "transparent",
    grid: { top: 10, right: 10, bottom: 24, left: 40 },
    xAxis: {
      type: "category" as const,
      data: dailyUsage.map((d) => d.day),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: "#666", fontSize: CHART_THEME.axisLabel.fontSize },
    },
    yAxis: {
      type: "value" as const,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: "rgba(255,255,255,0.05)" } },
      axisLabel: {
        color: "#666",
        fontSize: CHART_THEME.axisLabel.fontSize,
        formatter: (v: number) => `${(v / 1000).toFixed(0)}k`,
      },
    },
    series: [
      {
        type: "bar",
        data: dailyUsage.map((d) => d.tokens),
        barWidth: "60%",
        itemStyle: {
          color: {
            type: "linear" as const,
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(8, 145, 178, 0.8)" },
              { offset: 1, color: "rgba(8, 145, 178, 0.2)" },
            ],
          },
          borderRadius: [4, 4, 0, 0],
        },
      },
    ],
    tooltip: {
      trigger: "axis" as const,
      backgroundColor: "rgba(0,0,0,0.8)",
      borderColor: "rgba(255,255,255,0.1)",
      borderRadius: CHART_THEME.tooltip.borderRadius,
      textStyle: { color: "#fff", fontSize: CHART_THEME.tooltip.textStyle.fontSize },
      formatter: (params: Array<{ name: string; value: number }>) => {
        const p = params[0];
        return `${p.name}: ${p.value.toLocaleString()} tokens`;
      },
    },
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Coins size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Token Usage</h3>
        </div>
        <span className="text-xs text-muted-foreground">This week</span>
      </div>

      <div className="flex gap-6">
        <div>
          <p className="text-2xl font-semibold">{(totalTokens / 1000).toFixed(1)}k</p>
          <p className="text-xs text-muted-foreground">Total tokens</p>
        </div>
        <div>
          <p className="text-2xl font-semibold">${totalCost.toFixed(2)}</p>
          <p className="text-xs text-muted-foreground">Est. cost</p>
        </div>
      </div>

      {loading ? (
        <div className="h-[120px] bg-muted/30 rounded animate-pulse" />
      ) : (
        <ReactECharts
          option={chartOption}
          style={{ height: 120 }}
          opts={{ renderer: "canvas" }}
        />
      )}
    </div>
  );
}
