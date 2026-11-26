import { useState, useEffect, useRef } from "react";
import type { FrequencyBandPowers } from "../lib/types";
import { FFT_SIZE } from "../lib/fft";
import { bandFeatures } from "../../neuro/fft";

const WELCH_BUFFER = 512;   // Total buffer for Welch: 2s of data
const EMA_ALPHA = 0.2;      // Exponential moving average smoothing factor
const UPDATE_INTERVAL = 1000; // Recompute every 1s (was 500ms)

function ema(prev: number, next: number): number {
  return EMA_ALPHA * next + (1 - EMA_ALPHA) * prev;
}

/** Decision 0060: one band-power pipeline. This is `neuro/fft.ts` `bandFeatures` for one channel. */
export function computeBands(samples: number[]): FrequencyBandPowers {
  return bandFeatures([samples]);
}

const ZERO_BANDS: FrequencyBandPowers = {
  delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0,
  smr: 0, lowBeta: 0, highBeta: 0,
  alphaPeak: 10, focusScore: 0, relaxationScore: 0,
  alphaTheta: 0, betaAlpha: 0,
};

export function useFFT(eegData: Record<string, number[]>) {
  const [bands, setBands] = useState<FrequencyBandPowers>(ZERO_BANDS);
  const lastCompute = useRef(0);
  const prevBands = useRef<FrequencyBandPowers>(ZERO_BANDS);

  useEffect(() => {
    const af7 = eegData.AF7 || [];
    const af8 = eegData.AF8 || [];

    // Need at least FFT_SIZE samples from one frontal channel
    if (af7.length < FFT_SIZE && af8.length < FFT_SIZE) return;

    // Throttle: recompute every UPDATE_INTERVAL ms
    const now = Date.now();
    if (now - lastCompute.current < UPDATE_INTERVAL) return;
    lastCompute.current = now;

    // Average frontal channels, use up to WELCH_BUFFER samples for Welch
    const maxLen = Math.max(af7.length, af8.length);
    const useLen = Math.min(maxLen, WELCH_BUFFER);
    const af7Slice = af7.slice(-useLen);
    const af8Slice = af8.slice(-useLen);
    const samples = new Array(useLen);
    for (let i = 0; i < useLen; i++) {
      const v7 = af7Slice[i] ?? 0;
      const v8 = af8Slice[i] ?? 0;
      samples[i] = (v7 + v8) / 2;
    }

    const raw = computeBands(samples);

    // Apply EMA smoothing to all band percentages and scores
    const prev = prevBands.current;
    const smoothed: FrequencyBandPowers = {
      delta: Math.round(ema(prev.delta, raw.delta) * 10) / 10,
      theta: Math.round(ema(prev.theta, raw.theta) * 10) / 10,
      alpha: Math.round(ema(prev.alpha, raw.alpha) * 10) / 10,
      beta: Math.round(ema(prev.beta, raw.beta) * 10) / 10,
      gamma: Math.round(ema(prev.gamma, raw.gamma) * 10) / 10,
      smr: Math.round(ema(prev.smr, raw.smr) * 10) / 10,
      lowBeta: Math.round(ema(prev.lowBeta, raw.lowBeta) * 10) / 10,
      highBeta: Math.round(ema(prev.highBeta, raw.highBeta) * 10) / 10,
      alphaPeak: Math.round(ema(prev.alphaPeak, raw.alphaPeak) * 10) / 10,
      focusScore: Math.round(ema(prev.focusScore, raw.focusScore)),
      relaxationScore: Math.round(ema(prev.relaxationScore, raw.relaxationScore)),
      alphaTheta: Math.round(ema(prev.alphaTheta, raw.alphaTheta) * 100) / 100,
      betaAlpha: Math.round(ema(prev.betaAlpha, raw.betaAlpha) * 100) / 100,
    };

    prevBands.current = smoothed;
    setBands(smoothed);
  }, [eegData]);

  return bands;
}
