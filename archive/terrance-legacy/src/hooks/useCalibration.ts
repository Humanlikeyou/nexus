/**
 * Calibration hook: collects a baseline of FFT band powers over a configurable
 * period, then computes per-band baselines. Focus/Relax scores are then
 * normalized so that baseline performance reads ~50.
 *
 * Usage:
 *   const cal = useCalibration(fftBands, streaming);
 *   // cal.calibratedBands — scores normalized to baseline
 *   // cal.startCalibration() — begin baseline capture
 *   // cal.status — "idle" | "calibrating" | "done"
 */

import { useState, useRef, useCallback, useEffect } from "react";
import type { FrequencyBandPowers } from "@/lib/types";

export type CalibrationStatus = "idle" | "calibrating" | "done";

const DEFAULT_DURATION_MS = 20_000; // 20 seconds
const SAMPLE_INTERVAL_MS = 500;     // collect every 500ms

export interface CalibrationBaseline {
  /** Average raw ratio values during baseline */
  focusRatio: number;    // beta / (beta + theta)
  relaxRatio: number;    // alpha / (alpha + beta)
  /** Per-band averages */
  bands: Record<string, number>;
  /** Timestamp when calibration was completed */
  timestamp: number;
}

export interface CalibratedBands extends FrequencyBandPowers {
  /** Focus score normalized so baseline = 50, range 0-100 */
  calibratedFocus: number;
  /** Relaxation score normalized so baseline = 50, range 0-100 */
  calibratedRelax: number;
}

export function useCalibration(
  fftBands: FrequencyBandPowers,
  streaming: boolean,
  durationMs: number = DEFAULT_DURATION_MS,
) {
  const [status, setStatus] = useState<CalibrationStatus>("idle");
  const [progress, setProgress] = useState(0); // 0-1
  const [baseline, setBaseline] = useState<CalibrationBaseline | null>(null);

  const samplesRef = useRef<FrequencyBandPowers[]>([]);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startTimeRef = useRef(0);
  const bandsRef = useRef(fftBands);

  // Keep bands ref current
  useEffect(() => {
    bandsRef.current = fftBands;
  }, [fftBands]);

  const startCalibration = useCallback(() => {
    if (!streaming) return;

    samplesRef.current = [];
    startTimeRef.current = Date.now();
    setStatus("calibrating");
    setProgress(0);

    intervalRef.current = setInterval(() => {
      const now = Date.now();
      const elapsed = now - startTimeRef.current;
      const pct = Math.min(1, elapsed / durationMs);
      setProgress(pct);

      // Collect sample
      samplesRef.current.push({ ...bandsRef.current });

      if (elapsed >= durationMs) {
        // Done — compute baseline
        if (intervalRef.current) {
          clearInterval(intervalRef.current);
          intervalRef.current = null;
        }

        const samples = samplesRef.current;
        if (samples.length > 0) {
          const avgFocus = avg(samples.map((s) => s.focusScore));
          const avgRelax = avg(samples.map((s) => s.relaxationScore));

          // Store raw ratios for normalization
          // focusScore = beta/(beta+theta)*100, so raw ratio = score/100
          const focusRatio = avgFocus / 100;
          const relaxRatio = avgRelax / 100;

          const bands: Record<string, number> = {};
          for (const key of ["delta", "theta", "alpha", "beta", "gamma", "smr", "lowBeta", "highBeta"] as const) {
            bands[key] = avg(samples.map((s) => s[key]));
          }

          setBaseline({
            focusRatio,
            relaxRatio,
            bands,
            timestamp: Date.now(),
          });
        }

        setStatus("done");
        setProgress(1);
      }
    }, SAMPLE_INTERVAL_MS);
  }, [streaming, durationMs]);

  const resetCalibration = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    setStatus("idle");
    setProgress(0);
    setBaseline(null);
    samplesRef.current = [];
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  // Compute calibrated bands
  const calibratedBands: CalibratedBands = {
    ...fftBands,
    calibratedFocus: baseline
      ? normalizeScore(fftBands.focusScore / 100, baseline.focusRatio)
      : fftBands.focusScore,
    calibratedRelax: baseline
      ? normalizeScore(fftBands.relaxationScore / 100, baseline.relaxRatio)
      : fftBands.relaxationScore,
  };

  return {
    status,
    progress,
    baseline,
    calibratedBands,
    startCalibration,
    resetCalibration,
  };
}

function avg(values: number[]): number {
  if (values.length === 0) return 0;
  return values.reduce((a, b) => a + b, 0) / values.length;
}

/**
 * Normalize a current ratio against baseline so that baseline = 50.
 * Uses a linear mapping: score = (currentRatio / baselineRatio) * 50
 * Clamped to 0-100.
 */
function normalizeScore(currentRatio: number, baselineRatio: number): number {
  if (baselineRatio <= 0) return 50;
  const raw = (currentRatio / baselineRatio) * 50;
  return Math.round(Math.max(0, Math.min(100, raw)));
}
