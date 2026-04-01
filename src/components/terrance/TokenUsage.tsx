import { useState, useEffect } from "react";
import { Coins } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { getLlmUsage, getSessionInsights } from "@/lib/api";
import { formatCost } from "@/lib/llm-router";
import { CHART_THEME } from "@/lib/chart-theme";
import type { LlmUsageRecord, SessionInsights } from "@/lib/types";

function weeklyStats(records: LlmUsageRecord[]) {
  const now = Date.now();
  const weekMs = 7 * 24 * 60 * 60 * 1000;
  const week = records.filter((r) => now - r.timestamp < weekMs);

  const totalTokens = week.reduce(
    (s, r) => s + r.prompt_tokens + r.completion_tokens,
    0,
  );
  const totalCost = week.reduce((s, r) => s + r.estimated_cost_usd, 0);

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
  const [insights, setInsights] = useState<SessionInsights | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const fetchData = () => {
      Promise.all([getLlmUsage(), getSessionInsights().catch(() => null)])
        .then(([usage, si]) => {
          if (!cancelled) {
            setRecords(usage);
            if (si) setInsights(si);
            setLoading(false);
          }
        })
        .catch(() => {
          if (!cancelled) setLoading(false);
        });
    };
    fetchData();
    const interval = setInterval(fetchData, 30_000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  const stats = weeklyStats(records);

  const chartOption = {
    grid: { left: 0, right: 0, top: 2, bottom: 18, containLabel: false },
    xAxis: {
      type: "category" as const,
      data: stats.days.map((d) => d.label),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { ...CHART_THEME.axisLabelDim, fontSize: 9 },
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
          borderRadius: [2, 2, 0, 0],
          color: {
            type: "linear",
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(74, 158, 126, 0.6)" },
              { offset: 1, color: "rgba(74, 158, 126, 0.12)" },
            ],
          },
        },
        barWidth: "50%",
      },
    ],
  };

  const hasData = stats.callCount > 0;

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Token Usage</h3>
        <span className="text-[10px] font-mono text-muted-foreground/60 uppercase tracking-wider">7d</span>
      </div>

      <div className="flex gap-5">
        <div>
          <p className="metric-value text-lg">
            {loading ? "—" : stats.totalTokens.toLocaleString()}
          </p>
          <p className="metric-label">Tokens</p>
        </div>
        <div>
          <p className="metric-value text-lg">
            {loading ? "—" : formatCost(stats.totalCost)}
          </p>
          <p className="metric-label">Cost</p>
        </div>
        <div>
          <p className="metric-value text-lg">
            {loading ? "—" : stats.callCount}
          </p>
          <p className="metric-label">Calls</p>
        </div>
      </div>

      {insights && (
        <div className="flex gap-5 border-t border-border/18 pt-2">
          <div>
            <p className="metric-value text-lg">{insights.totalSessions}</p>
            <p className="metric-label">Sessions</p>
          </div>
          <div>
            <p className="metric-value text-lg">{insights.recentSessionCount}</p>
            <p className="metric-label">30d Active</p>
          </div>
          {Object.entries(insights.byProvider ?? {}).length > 0 && (
            <div className="flex-1">
              <div className="flex gap-2 flex-wrap">
                {Object.entries(insights.byProvider ?? {}).map(([provider, count]) => (
                  <span key={provider} className="badge-status bg-accent/8 text-muted-foreground/70 border border-border/18">
                    {provider}: {count}
                  </span>
                ))}
              </div>
              <p className="metric-label mt-1">By Provider</p>
            </div>
          )}
        </div>
      )}

      {loading ? (
        <div className="h-[90px] bg-muted/10 rounded-sm animate-pulse" />
      ) : hasData ? (
        <ReactECharts
          option={chartOption}
          style={{ height: 90 }}
          opts={{ renderer: "svg" }}
        />
      ) : (
        <div className="h-[90px] flex items-center justify-center">
          <span className="text-[10px] font-mono text-muted-foreground/50 uppercase tracking-wider">
            No data
          </span>
        </div>
      )}
    </div>
  );
}
