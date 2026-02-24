import { useState, useMemo } from "react";
import { TrendingUp } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { cn } from "@/lib/utils";
import { CHART_THEME } from "@/lib/chart-theme";
import type { ArmRevenueSummary } from "@/lib/types";

type TimeRange = "today" | "week" | "month" | "all";

interface RevenueChartProps {
  revenueSummary: ArmRevenueSummary | null;
}

export function RevenueChart({ revenueSummary }: RevenueChartProps) {
  const [range, setRange] = useState<TimeRange>("month");

  const summary = revenueSummary;
  const net = summary ? summary.total_revenue - summary.total_expenses : 0;

  const displayRevenue = useMemo(() => {
    if (!summary) return 0;
    switch (range) {
      case "today":
        return summary.today_revenue;
      case "week":
        return summary.week_revenue;
      case "month":
        return summary.month_revenue;
      case "all":
        return summary.total_revenue;
    }
  }, [summary, range]);

  const chartOption = useMemo(
    () => ({
      backgroundColor: "transparent",
      animation: true,
      grid: { top: 20, right: 16, bottom: 24, left: 44 },
      tooltip: {
        trigger: "axis" as const,
        backgroundColor: "rgba(0, 0, 0, 0.75)",
        borderColor: "rgba(255, 255, 255, 0.1)",
        borderRadius: CHART_THEME.tooltip.borderRadius,
        textStyle: { color: "#fff", fontSize: CHART_THEME.tooltip.textStyle.fontSize },
      },
      xAxis: {
        type: "category" as const,
        data: (summary?.by_strategy ?? []).map((s) => s.strategy_name),
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: {
          color: "rgba(255, 255, 255, 0.3)",
          fontSize: CHART_THEME.axisLabel.fontSize,
          rotate: 0,
        },
      },
      yAxis: {
        type: "value" as const,
        axisLine: { show: false },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: "rgba(255, 255, 255, 0.04)" } },
        axisLabel: {
          color: "rgba(255, 255, 255, 0.3)",
          fontSize: CHART_THEME.axisLabel.fontSize,
          formatter: (v: number) => `$${v}`,
        },
      },
      series: [
        {
          name: "Revenue",
          type: "bar",
          data: (summary?.by_strategy ?? []).map((s) => s.revenue),
          itemStyle: {
            color: "rgba(52, 211, 153, 0.6)",
            borderRadius: [4, 4, 0, 0],
          },
          barMaxWidth: 32,
        },
        {
          name: "Expenses",
          type: "bar",
          data: (summary?.by_strategy ?? []).map((s) => s.expenses),
          itemStyle: {
            color: "rgba(248, 113, 113, 0.4)",
            borderRadius: [4, 4, 0, 0],
          },
          barMaxWidth: 32,
        },
      ],
    }),
    [summary],
  );

  const ranges: { key: TimeRange; label: string }[] = [
    { key: "today", label: "Today" },
    { key: "week", label: "Week" },
    { key: "month", label: "Month" },
    { key: "all", label: "All" },
  ];

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <TrendingUp size={16} className="text-success" />
          <h3 className="text-base font-medium text-muted-foreground">Revenue</h3>
        </div>
        <div className="flex items-center gap-1 bg-muted/15 rounded-lg p-0.5">
          {ranges.map((r) => (
            <button
              key={r.key}
              onClick={() => setRange(r.key)}
              className={cn(
                "px-2 py-1 rounded text-xs font-medium transition-colors",
                range === r.key
                  ? "bg-accent/20 text-accent"
                  : "text-muted-foreground/50 hover:text-muted-foreground",
              )}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {/* Summary Cards */}
      <div className="grid grid-cols-3 gap-3">
        <div className="px-3 py-2 rounded-lg bg-muted/10">
          <p className="text-xs text-muted-foreground/40">Revenue</p>
          <p className="text-lg font-mono font-semibold text-success">
            ${displayRevenue.toFixed(2)}
          </p>
        </div>
        <div className="px-3 py-2 rounded-lg bg-muted/10">
          <p className="text-xs text-muted-foreground/40">Expenses</p>
          <p className="text-lg font-mono font-semibold text-destructive">
            ${summary?.total_expenses.toFixed(2) ?? "0.00"}
          </p>
        </div>
        <div className="px-3 py-2 rounded-lg bg-muted/10">
          <p className="text-xs text-muted-foreground/40">Net Income</p>
          <p
            className={cn(
              "text-lg font-mono font-semibold",
              net >= 0 ? "text-success" : "text-destructive",
            )}
          >
            {net >= 0 ? "+" : ""}${net.toFixed(2)}
          </p>
        </div>
      </div>

      {/* Chart */}
      {summary && summary.by_strategy.length > 0 ? (
        <ReactECharts
          option={chartOption}
          style={{ height: 200 }}
          opts={{ renderer: "canvas" }}
        />
      ) : (
        <div className="h-[200px] flex items-center justify-center">
          <p className="text-xs text-muted-foreground/30">
            Revenue data will appear once strategies are active
          </p>
        </div>
      )}
    </div>
  );
}
