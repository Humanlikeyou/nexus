import type { RecoveryMetrics } from "@/lib/types";
import { computeHRV } from "@/lib/utils";

/**
 * Recovery score calculator — inspired by OpenWhoop sleep/stress analysis
 * and WHOOP's recovery methodology.
 *
 * WHOOP Recovery (0-100) is primarily driven by:
 *   1. HRV (RMSSD) — higher = better recovery (40% weight)
 *   2. Resting Heart Rate — lower = better recovery (30% weight)
 *   3. Heart Rate Variability (coefficient of variation) — higher = better (20% weight)
 *   4. Respiratory rate — not available from Whoop BLE, omitted (10% → redistributed)
 *
 * Thresholds are population averages; ideally these would be personalized
 * from a 2-week baseline, but we use reasonable defaults.
 *
 * Scoring bands:
 *   67-100 = Green (well recovered)
 *   34-66  = Yellow (moderate recovery)
 *   0-33   = Red (low recovery)
 */

const MIN_RR_COUNT = 30;    // need at least 30 RR intervals for meaningful HRV
const MIN_HR_COUNT = 60;    // need at least 60 HR readings (~1 min)

// Population-average reference values
const REF_HRV_RMSSD = 50;  // ms — "good" HRV for adults
const REF_RESTING_HR = 60;  // bpm — typical resting HR
const REF_CV = 0.06;        // coefficient of variation reference

export function computeRecovery(
  hrReadings: number[],
  rrIntervals: number[],
): RecoveryMetrics {
  const hasEnoughRR = rrIntervals.length >= MIN_RR_COUNT;
  const hasEnoughHR = hrReadings.length >= MIN_HR_COUNT;

  if (!hasEnoughHR) {
    return {
      score: 0,
      hrvBaseline: 0,
      restingHR: 0,
      hrVariability: 0,
      ready: false,
    };
  }

  // 1. Compute HRV (RMSSD)
  const hrv = hasEnoughRR ? computeHRV(rrIntervals) : { rmssd: 0, sdnn: 0 };

  // 2. Find resting HR — 10th percentile of HR readings (avoids outlier spikes)
  const sorted = [...hrReadings].filter((hr) => hr > 30 && hr < 220).sort((a, b) => a - b);
  const p10Index = Math.floor(sorted.length * 0.1);
  const restingHR = sorted[p10Index] || 60;

  // 3. Coefficient of variation of HR
  const validHR = hrReadings.filter((hr) => hr > 30 && hr < 220);
  const mean = validHR.reduce((a, b) => a + b, 0) / validHR.length;
  const variance = validHR.reduce((sum, hr) => sum + Math.pow(hr - mean, 2), 0) / validHR.length;
  const cv = mean > 0 ? Math.sqrt(variance) / mean : 0;

  // Score components (each 0-100)

  // HRV component (40% weight) — higher RMSSD = better
  // Score = (rmssd / reference) * 100, capped at 100
  let hrvComponent = 0;
  if (hasEnoughRR && hrv.rmssd > 0) {
    hrvComponent = Math.min(100, (hrv.rmssd / REF_HRV_RMSSD) * 100);
  } else {
    // Without RR intervals, use HR variability as proxy (lower fidelity)
    hrvComponent = Math.min(100, (cv / REF_CV) * 60);
  }

  // Resting HR component (35% weight) — lower HR = better
  // Score inverts: 40 bpm → 100, 60 bpm → 67, 80 bpm → 33, 100+ bpm → 0
  const restingComponent = Math.max(0, Math.min(100,
    100 - ((restingHR - 40) / 60) * 100,
  ));

  // HR variability component (25% weight) — higher CV = better parasympathetic tone
  const cvComponent = Math.min(100, (cv / REF_CV) * 100);

  // Weighted average
  const weights = hasEnoughRR
    ? { hrv: 0.40, resting: 0.35, cv: 0.25 }
    : { hrv: 0.0, resting: 0.55, cv: 0.45 };

  const score = Math.round(
    hrvComponent * weights.hrv +
    restingComponent * weights.resting +
    cvComponent * weights.cv,
  );

  return {
    score: Math.max(0, Math.min(100, score)),
    hrvBaseline: hrv.rmssd,
    restingHR: Math.round(restingHR),
    hrVariability: Math.round(cv * 1000) / 1000,
    ready: true,
  };
}
