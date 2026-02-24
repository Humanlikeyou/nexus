import type { SleepMetrics, SleepStage } from "@/lib/types";

/**
 * Sleep analysis — inspired by OpenWhoop sleep.rs
 *
 * Without accelerometer or SpO2 data from BLE, we estimate sleep stages
 * from HR patterns during a "sleep window."
 *
 * Stage estimation from HR relative to session baseline:
 *   - Deep (SWS): HR < baseline - 10% (parasympathetic dominance, lowest HR)
 *   - REM: HR > baseline + 5% with high variability (active brain, variable HR)
 *   - Light: HR within ±5% of baseline (stable, moderate HR)
 *   - Awake: HR > baseline + 15% (elevated HR)
 *
 * Sleep score (0-100) from OpenWhoop:
 *   Based on total sleep duration + stage distribution.
 *   7-9h total = high score, good deep/REM ratio boosts further.
 *
 * This module processes recorded sleep sessions, not real-time streaming.
 * Call `analyzeSleep()` with a buffer of HR data from an overnight session.
 */

const WINDOW_SIZE = 5; // 5-sample moving average for HR smoothing
const STAGE_COLORS = {
  "Awake": "#ef4444",
  "Light": "#06b6d4",
  "REM": "#8b5cf6",
  "Deep (SWS)": "#6366f1",
} as const;

function movingAverage(values: number[], window: number): number[] {
  const result: number[] = [];
  for (let i = 0; i < values.length; i++) {
    const start = Math.max(0, i - Math.floor(window / 2));
    const end = Math.min(values.length, i + Math.floor(window / 2) + 1);
    const slice = values.slice(start, end);
    result.push(slice.reduce((a, b) => a + b, 0) / slice.length);
  }
  return result;
}

function localVariability(values: number[], idx: number, window: number): number {
  const start = Math.max(0, idx - window);
  const end = Math.min(values.length, idx + window + 1);
  const slice = values.slice(start, end);
  if (slice.length < 2) return 0;
  const mean = slice.reduce((a, b) => a + b, 0) / slice.length;
  const variance = slice.reduce((sum, v) => sum + Math.pow(v - mean, 2), 0) / slice.length;
  return Math.sqrt(variance);
}

type StageName = "Awake" | "Light" | "REM" | "Deep (SWS)";

function classifyStage(
  smoothedHR: number,
  baseline: number,
  variability: number,
  avgVariability: number,
): StageName {
  const pctFromBaseline = (smoothedHR - baseline) / baseline;

  // Awake: HR significantly above baseline
  if (pctFromBaseline > 0.15) return "Awake";

  // Deep: HR well below baseline with low variability
  if (pctFromBaseline < -0.10 && variability < avgVariability * 0.8) return "Deep (SWS)";

  // REM: HR slightly above baseline with high variability
  if (pctFromBaseline > 0.02 && variability > avgVariability * 1.2) return "REM";

  // Light: everything else near baseline
  return "Light";
}

export function analyzeSleep(
  hrReadings: { heart_rate: number; timestamp: number }[],
): SleepMetrics {
  const validReadings = hrReadings.filter((r) => r.heart_rate > 30 && r.heart_rate < 220);

  if (validReadings.length < 60) {
    return {
      totalHours: 0,
      score: 0,
      stages: [
        { name: "Awake", hours: 0, color: STAGE_COLORS["Awake"] },
        { name: "Light", hours: 0, color: STAGE_COLORS["Light"] },
        { name: "REM", hours: 0, color: STAGE_COLORS["REM"] },
        { name: "Deep (SWS)", hours: 0, color: STAGE_COLORS["Deep (SWS)"] },
      ],
      avgHR: 0,
      minHR: 0,
      avgHRV: 0,
      ready: false,
    };
  }

  const hrs = validReadings.map((r) => r.heart_rate);
  const smoothed = movingAverage(hrs, WINDOW_SIZE);

  // Baseline = 25th percentile of smoothed HR (represents resting level during sleep)
  const sortedSmoothed = [...smoothed].sort((a, b) => a - b);
  const baseline = sortedSmoothed[Math.floor(sortedSmoothed.length * 0.25)];

  // Compute local variability for each point
  const variabilities = smoothed.map((_, i) => localVariability(hrs, i, 5));
  const avgVariability = variabilities.reduce((a, b) => a + b, 0) / variabilities.length;

  // Classify each sample
  const stageCounts: Record<StageName, number> = {
    "Awake": 0,
    "Light": 0,
    "REM": 0,
    "Deep (SWS)": 0,
  };

  for (let i = 0; i < smoothed.length; i++) {
    const stage = classifyStage(smoothed[i], baseline, variabilities[i], avgVariability);
    stageCounts[stage]++;
  }

  // Convert counts to hours
  const durationSecs = (validReadings[validReadings.length - 1].timestamp - validReadings[0].timestamp) / 1000;
  const totalHours = durationSecs / 3600;
  const totalSamples = smoothed.length;

  const stages: SleepStage[] = (["Awake", "Light", "REM", "Deep (SWS)"] as const).map((name) => ({
    name,
    hours: Math.round((stageCounts[name] / totalSamples) * totalHours * 10) / 10,
    color: STAGE_COLORS[name],
  }));

  // Sleep score (from OpenWhoop): duration-based + stage quality bonus
  // Base: 8h = 85, scaled linearly. Deep+REM ratio bonus up to +15
  const sleepHours = totalHours - stages[0].hours; // subtract awake time
  const baseDurationScore = Math.min(85, (sleepHours / 8) * 85);
  const deepRemPct = (stageCounts["Deep (SWS)"] + stageCounts["REM"]) /
    Math.max(1, totalSamples - stageCounts["Awake"]);
  const stageBonus = deepRemPct * 15; // ideal deep+REM ~40-50% of sleep
  const score = Math.round(Math.max(0, Math.min(100, baseDurationScore + stageBonus)));

  // Stats
  const minHR = Math.round(Math.min(...hrs));
  const avgHR = Math.round(hrs.reduce((a, b) => a + b, 0) / hrs.length);

  return {
    totalHours: Math.round(totalHours * 10) / 10,
    score,
    stages,
    avgHR,
    minHR,
    avgHRV: 0, // populated by caller from RR intervals if available
    ready: true,
  };
}
