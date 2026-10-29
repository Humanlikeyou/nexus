/** Shared ECharts theme — Tactical Command Instrumentation */

export const CHART_THEME = {
  axisLabel: {
    fontSize: 10,
    color: "rgba(139, 149, 163, 0.8)",
    fontFamily: "JetBrains Mono, SF Mono, ui-monospace, monospace",
  },
  axisLabelMuted: {
    fontSize: 10,
    color: "rgba(139, 149, 163, 0.55)",
    fontFamily: "JetBrains Mono, SF Mono, ui-monospace, monospace",
  },
  axisLabelDim: {
    fontSize: 10,
    color: "rgba(139, 149, 163, 0.4)",
    fontFamily: "JetBrains Mono, SF Mono, ui-monospace, monospace",
  },
  tooltip: {
    backgroundColor: "rgba(8, 11, 15, 0.95)",
    borderColor: "rgba(38, 48, 64, 0.6)",
    borderRadius: 3,
    padding: [6, 10] as [number, number],
    textStyle: {
      color: "rgba(224, 227, 233, 0.9)",
      fontSize: 11,
      fontFamily: "JetBrains Mono, SF Mono, ui-monospace, monospace",
    },
  },
  gauge: {
    progressWidth: 6,
    axisLineWidth: 6,
    titleFontSize: 10,
    detailFontSize: 24,
  },
  line: {
    width: 1,
  },
  splitLine: {
    color: "rgba(38, 48, 64, 0.4)",
  },
  colors: {
    phthalo: "#4A9E7E",
    amber: "#A3923D",
    cyan: "#4A80A3",
    solar: "#A3803D",
    muted: "rgba(139, 149, 163, 0.25)",
  },
} as const;
