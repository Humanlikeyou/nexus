import { CHART_THEME } from "@/lib/chart-theme";
import ReactECharts from "echarts-for-react";

interface FrequencyBandsProps {
  bands: {
    delta: number;
    theta: number;
    alpha: number;
    beta: number;
    gamma: number;
  };
  smr?: number;
  rewardBands?: string[];
  inhibitBands?: string[];
  compact?: boolean;
}

const bandColors: Record<string, string> = {
  Delta: "#6366f1",
  Theta: "#8b5cf6",
  Alpha: "#06b6d4",
  SMR: "#14b8a6",
  Beta: "#f59e0b",
  Gamma: "#ef4444",
};

const bandInfo: Record<string, string> = {
  Delta: "1.5-4Hz · Deep sleep",
  Theta: "5-8Hz · Creativity",
  Alpha: "9-14Hz · Relaxation",
  SMR: "12-15Hz · Sensorimotor",
  Beta: "15-32Hz · Focus",
  Gamma: "32-100Hz · Peak focus",
};

const bandKeyMap: Record<string, string> = {
  delta: "Delta",
  theta: "Theta",
  alpha: "Alpha",
  smr: "SMR",
  beta: "Beta",
  lowbeta: "Beta",
  highbeta: "Beta",
  gamma: "Gamma",
};

export function FrequencyBands({
  bands,
  smr,
  rewardBands = [],
  inhibitBands = [],
  compact = false,
}: FrequencyBandsProps) {
  const data = [
    { name: "Delta", value: bands.delta },
    { name: "Theta", value: bands.theta },
    { name: "Alpha", value: bands.alpha },
    ...(smr != null ? [{ name: "SMR", value: smr }] : []),
    { name: "Beta", value: bands.beta },
    { name: "Gamma", value: bands.gamma },
  ];

  // Map reward/inhibit band keys to display names
  const rewardNames = new Set(
    rewardBands.map((k) => bandKeyMap[k] || k).filter(Boolean),
  );
  const inhibitNames = new Set(
    inhibitBands.map((k) => bandKeyMap[k] || k).filter(Boolean),
  );

  const chartOption = {
    backgroundColor: "transparent",
    grid: compact
      ? { top: 5, right: 20, bottom: 5, left: 50 }
      : { top: 10, right: 20, bottom: 5, left: 60 },
    xAxis: {
      type: "value" as const,
      max: 100,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { show: false },
      axisLabel: { show: false },
    },
    yAxis: {
      type: "category" as const,
      data: data.map((d) => d.name),
      inverse: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: {
        color: "#888",
        fontSize: CHART_THEME.axisLabel.fontSize,
      },
    },
    series: [
      {
        type: "bar",
        data: data.map((d) => {
          let color = bandColors[d.name];
          let borderColor = "transparent";
          let borderWidth = 0;

          if (rewardNames.has(d.name)) {
            borderColor = "#10b981";
            borderWidth = 2;
          } else if (inhibitNames.has(d.name)) {
            borderColor = "#ef4444";
            borderWidth = 2;
          }

          return {
            value: d.value,
            itemStyle: {
              color,
              borderColor,
              borderWidth,
              borderRadius: 4,
            },
          };
        }),
        barWidth: compact ? 10 : 14,
        label: {
          show: true,
          position: "right" as const,
          color: "#888",
          fontSize: CHART_THEME.axisLabel.fontSize,
          formatter: (p: { value: number }) => `${p.value.toFixed(1)}%`,
        },
        backgroundStyle: {
          color: "rgba(255,255,255,0.03)",
          borderRadius: 4,
        },
        showBackground: true,
        itemStyle: { borderRadius: 4 },
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
        let extra = "";
        if (rewardNames.has(p.name)) extra = '<br/><span style="color:#10b981">Reward band</span>';
        if (inhibitNames.has(p.name)) extra = '<br/><span style="color:#ef4444">Inhibit band</span>';
        return `${p.name}: ${p.value.toFixed(1)}%<br/><span style="font-size:10px;color:#888">${bandInfo[p.name] || ""}</span>${extra}`;
      },
    },
  };

  const height = compact
    ? data.length * 22 + 10
    : data.length * 30 + 20;

  return (
    <div className={compact ? "panel p-2 rounded-xl" : "panel p-4 space-y-3"}>
      {!compact && (
        <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">Frequency Bands</h3>
      )}
      <ReactECharts
        option={chartOption}
        style={{ height }}
        opts={{ renderer: "canvas" }}
      />
    </div>
  );
}
