import { Heart, Activity } from "lucide-react";
import ReactECharts from "echarts-for-react";
import { computeHRV } from "@/lib/utils";
import type { HeartRateData, StressScore } from "@/lib/types";

interface HrvTrendProps {
  hrData: HeartRateData[];
  rrIntervals: number[];
  currentHR: number | null;
  stress: StressScore;
}

function stressLabel(score: number): { label: string; color: string } {
  if (score <= 2) return { label: "Low", color: "#10b981" };
  if (score <= 5) return { label: "Moderate", color: "#f59e0b" };
  if (score <= 7) return { label: "High", color: "#f97316" };
  return { label: "Very High", color: "#ef4444" };
}

export function HrvTrend({ hrData, rrIntervals, currentHR, stress }: HrvTrendProps) {
  const hrv = computeHRV(rrIntervals);

  // Use real data if streaming, otherwise placeholder
  const displayData = hrData.length > 0
    ? hrData.slice(-60).map((d, i) => ({ idx: i, hr: d.heart_rate }))
    : [
        { idx: 0, hr: 62 }, { idx: 1, hr: 64 }, { idx: 2, hr: 61 },
        { idx: 3, hr: 65 }, { idx: 4, hr: 63 }, { idx: 5, hr: 60 },
        { idx: 6, hr: 66 }, { idx: 7, hr: 68 }, { idx: 8, hr: 65 },
        { idx: 9, hr: 63 },
      ];

  const displayHR = currentHR || 63;
  const stressInfo = stressLabel(stress.score);

  const chartOption = {
    backgroundColor: "transparent",
    animation: false,
    grid: { top: 10, right: 10, bottom: 24, left: 35 },
    xAxis: {
      type: "category" as const,
      data: displayData.map((d) => d.idx),
      show: false,
    },
    yAxis: {
      type: "value" as const,
      min: (v: { min: number }) => Math.max(40, v.min - 5),
      max: (v: { max: number }) => v.max + 5,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: "rgba(255,255,255,0.04)" } },
      axisLabel: { color: "#555", fontSize: 9 },
    },
    series: [
      {
        type: "line" as const,
        data: displayData.map((d) => d.hr),
        smooth: true,
        showSymbol: false,
        lineStyle: { color: "#ef4444", width: 2 },
        areaStyle: {
          color: {
            type: "linear" as const,
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(239, 68, 68, 0.15)" },
              { offset: 1, color: "rgba(239, 68, 68, 0)" },
            ],
          },
        },
      },
    ],
    tooltip: { show: false },
  };

  return (
    <div className="glass p-6 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Heart size={16} className="text-red-500" />
          <h3 className="text-base font-medium text-muted-foreground">Heart Rate & HRV</h3>
        </div>
        {hrData.length > 0 && (
          <div className="flex items-center gap-1.5">
            <div className="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse-dot" />
            <span className="text-xs text-red-400">Live</span>
          </div>
        )}
      </div>

      <div className="flex gap-6">
        <div>
          <p className="text-2xl font-bold">{displayHR}</p>
          <p className="text-xs text-muted-foreground">BPM</p>
        </div>
        <div>
          <p className="text-2xl font-bold text-accent">
            {hrv.rmssd > 0 ? hrv.rmssd : 42}
          </p>
          <p className="text-xs text-muted-foreground">HRV (RMSSD)</p>
        </div>
        <div>
          <p className="text-2xl font-bold text-success">
            {hrv.sdnn > 0 ? hrv.sdnn : 38}
          </p>
          <p className="text-xs text-muted-foreground">SDNN</p>
        </div>
        {/* Stress indicator */}
        <div className="ml-auto text-right">
          <div className="flex items-center gap-1.5 justify-end">
            <Activity size={12} style={{ color: stressInfo.color }} />
            <p className="text-2xl font-bold" style={{ color: stressInfo.color }}>
              {stress.ready ? stress.score.toFixed(1) : "--"}
            </p>
          </div>
          <p className="text-xs text-muted-foreground">
            {stress.ready ? `Stress · ${stressInfo.label}` : `Stress (${stress.sampleCount}/120)`}
          </p>
        </div>
      </div>

      <ReactECharts
        option={chartOption}
        style={{ height: 120 }}
        opts={{ renderer: "canvas" }}
      />
    </div>
  );
}
