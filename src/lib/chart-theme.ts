/** Shared ECharts theme constants for consistent visual styling across all charts */

export const CHART_THEME = {
  axisLabel: {
    fontSize: 11,
    color: "#666",
  },
  axisLabelMuted: {
    fontSize: 11,
    color: "rgba(255, 255, 255, 0.3)",
  },
  axisLabelDim: {
    fontSize: 11,
    color: "#555",
  },
  tooltip: {
    backgroundColor: "rgba(0, 0, 0, 0.85)",
    borderColor: "rgba(255, 255, 255, 0.12)",
    borderRadius: 12,
    padding: [10, 14] as [number, number],
    textStyle: {
      color: "#fff",
      fontSize: 13,
    },
  },
  gauge: {
    progressWidth: 14,
    axisLineWidth: 14,
    titleFontSize: 13,
    detailFontSize: 32,
  },
} as const;
