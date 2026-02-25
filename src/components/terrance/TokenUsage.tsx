import { useState, useEffect } from "react";
import { Coins } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { getLlmUsage } from "@/lib/api";
import { formatCost } from "@/lib/llm-router";
import { CHART_THEME } from "@/lib/chart-theme";
import type { LlmUsageRecord } from "@/lib/types";

function weeklyStats(records: LlmUsageRecord[]) {
  const now = Date.now();
  const weekMs = 7 * 24 * 60 * 60 * 1000;
  const week = records.filter((r) => now - r.timestamp < weekMs);

  const totalTokens = week.reduce(
    (s, r) => s + r.prompt_tokens + r.completion_tokens,
    0,
  );
  const totalCost = week.reduce((s, r) => s + r.estimated_cost_usd, 0);

  // Group by day (last 7 days)
  const days: { label: string; tokens: number }[] = [];
  for (let i = 6; i >= 0; i--) {
    const dayStart = new Date(now - i * 24 * 60 * 60 * 1000);
    dayStart.setHours(0, 0, 0, 0);
    const dayEnd = new Date(dayStart);
    dayEnd.setDate(dayEnd.getDate() + 1);

    const dayTokens = week
      .filter((r) => r.timestamp >= dayStart.getTime() && r.timestamp < dayEnd.getTime())
      .reduce((s, r) => s + r.prompt_tokens + r.completion_tokens, 0);

    days.push({
      label: dayStart.toLocaleDateString("en-US", { weekday: "short" }),
      tokens: dayTokens,
    });
  }

  return { totalTokens, totalCost, days, callCount: week.length };
}

export function TokenUsage() {
  const [records, setRecords] = useState<LlmUsageRecord[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getLlmUsage()
      .then((data) => {
        setRecords(data);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const stats = weeklyStats(records);

  const chartOption = {
    grid: { left: 0, right: 0, top: 4, bottom: 20, containLabel: false },
    xAxis: {
      type: "category" as const,
      data: stats.days.map((d) => d.label),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { ...CHART_THEME.axisLabelMuted, fontSize: 10 },
    },
    yAxis: { type: "value" as const, show: false },
    tooltip: {
      ...CHART_THEME.tooltip,
      trigger: "axis" as const,
      formatter: (params: Array<{ value: number; name: string }>) => {
        const p = params[0];
        return `${p.name}: ${p.value.toLocaleString()} tokens`;
      },
    },
    series: [
      {
        type: "bar",
        data: stats.days.map((d) => d.tokens),
        itemStyle: {
          borderRadius: [4, 4, 0, 0],
          color: {
            type: "linear",
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: "oklch(0.68 0.20 195 / 0.7)" },
              { offset: 1, color: "oklch(0.68 0.20 195 / 0.2)" },
            ],
          },
        },
        barWidth: "50%",
      },
    ],
  };

  const hasData = stats.callCount > 0;

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
          <p className="text-2xl font-semibold">
            {loading ? "--" : stats.totalTokens.toLocaleString()}
          </p>
          <p className="text-xs text-muted-foreground">Total tokens</p>
        </div>
        <div>
          <p className="text-2xl font-semibold">
            {loading ? "--" : formatCost(stats.totalCost)}
          </p>
          <p className="text-xs text-muted-foreground">Est. cost</p>
        </div>
        <div>
          <p className="text-2xl font-semibold">
            {loading ? "--" : stats.callCount}
          </p>
          <p className="text-xs text-muted-foreground">Calls</p>
        </div>
      </div>

      {loading ? (
        <div className="h-[120px] bg-muted/30 rounded animate-pulse" />
      ) : hasData ? (
        <ReactECharts
          option={chartOption}
          style={{ height: 120 }}
          opts={{ renderer: "svg" }}
        />
      ) : (
        <div className="h-[120px] flex items-center justify-center">
          <span className="text-xs text-muted-foreground/40">
            No usage data this week
          </span>
        </div>
      )}
    </div>
  );
}
