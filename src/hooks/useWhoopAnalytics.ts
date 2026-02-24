import { useMemo, useRef, useCallback } from "react";
import type {
  HeartRateData,
  StressScore,
  StrainMetrics,
  RecoveryMetrics,
  SleepMetrics,
} from "@/lib/types";
import { computeStress } from "@/lib/whoop/stressCalculator";
import { computeStrain } from "@/lib/whoop/strainCalculator";
import { computeRecovery } from "@/lib/whoop/recoveryScore";
import { analyzeSleep } from "@/lib/whoop/sleepAnalysis";

/**
 * Computes real-time Whoop analytics from streaming HR/RR data.
 *
 * - Stress: computed from full HR history (needs 120+ samples)
 * - Strain: accumulated from timestamped HR data
 * - Recovery: computed from HR + RR intervals
 * - Sleep: computed on-demand from recorded sleep sessions
 *
 * All calculations are memoized and recompute only when inputs change.
 */
export function useWhoopAnalytics(
  hrData: HeartRateData[],
  rrIntervals: number[],
  streaming: boolean,
) {
  // Keep a reference to stored sleep data (from recorded sessions)
  const sleepDataRef = useRef<HeartRateData[]>([]);

  // Extract HR values for stress computation
  const hrValues = useMemo(
    () => hrData.map((d) => d.heart_rate),
    [hrData],
  );

  // Stress — recomputes every 30 samples (throttled via useMemo dep on length bucket)
  const stressBucket = Math.floor(hrValues.length / 30);
  const stress = useMemo<StressScore>(
    () => computeStress(hrValues),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [stressBucket],
  );

  // Strain — recomputes every 10 samples
  const strainBucket = Math.floor(hrData.length / 10);
  const strain = useMemo<StrainMetrics>(
    () => computeStrain(hrData),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [strainBucket],
  );

  // Recovery — recomputes every 30 samples
  const recoveryBucket = Math.floor(hrValues.length / 30);
  const recovery = useMemo<RecoveryMetrics>(
    () => computeRecovery(hrValues, rrIntervals),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [recoveryBucket, rrIntervals.length],
  );

  // Sleep — computed on demand from stored sleep session data
  const sleep = useMemo<SleepMetrics>(
    () => analyzeSleep(sleepDataRef.current),
    // Only recompute when sleep data reference changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sleepDataRef.current.length],
  );

  // Allow setting sleep data from a recorded session
  const setSleepData = useCallback((data: HeartRateData[]) => {
    sleepDataRef.current = data;
  }, []);

  return {
    stress,
    strain,
    recovery,
    sleep,
    setSleepData,
    /** True when we have enough data for meaningful metrics */
    hasData: streaming && hrData.length > 0,
  };
}
