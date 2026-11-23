import { useState, useEffect, useRef } from "react";
import type { FrequencyBandPowers } from "../lib/types";
import { SAMPLE_RATE, FFT_SIZE, welchPSD, singlePSD, bandPower } from "../lib/fft";

const WELCH_BUFFER = 512;   // Total buffer for Welch: 2s of data
const EMA_ALPHA = 0.2;      // Exponential moving average smoothing factor
const UPDATE_INTERVAL = 1000; // Recompute every 1s (was 500ms)

function ema(prev: number, next: number): number {
  return EMA_ALPHA * next + (1 - EMA_ALPHA) * prev;
}

export function computeBands(samples: number[]): FrequencyBandPowers {
  const powers = samples.length >= WELCH_BUFFER
    ? welchPSD(samples)
    : singlePSD(samples);

  const delta = bandPower(powers, 1.5, 4);
  const theta = bandPower(powers, 4, 8);
  const alpha = bandPower(powers, 8, 13);
  const beta = bandPower(powers, 13, 30);
  const gamma = bandPower(powers, 30, 50);
  const smr = bandPower(powers, 12, 15);
  const lowBeta = bandPower(powers, 15, 20);
  const highBeta = bandPower(powers, 20, 30);

  // Alpha peak frequency
  const binSize = SAMPLE_RATE / FFT_SIZE;
  const alphaMinBin = Math.floor(8 / binSize);
  const alphaMaxBin = Math.ceil(13 / binSize);
  let peakBin = alphaMinBin;
  let peakVal = 0;
  for (let i = alphaMinBin; i <= alphaMaxBin && i < powers.length; i++) {
    if (powers[i] > peakVal) { peakVal = powers[i]; peakBin = i; }
  }
  const alphaPeak = Math.round(peakBin * binSize * 10) / 10;

  const total = delta + theta + alpha + beta + gamma || 1;

  const deltaPct = (delta / total) * 100;
  const thetaPct = (theta / total) * 100;
  const alphaPct = (alpha / total) * 100;
  const betaPct = (beta / total) * 100;
  const gammaPct = (gamma / total) * 100;
  const smrPct = (smr / total) * 100;
  const lowBetaPct = (lowBeta / total) * 100;
  const highBetaPct = (highBeta / total) * 100;

  // Focus: beta / (beta + theta)
  const focusScore = beta + theta > 0
    ? Math.round((beta / (beta + theta)) * 100)
    : 0;

  // Relaxation: alpha / (alpha + beta)
  const relaxationScore = alpha + beta > 0
    ? Math.round((alpha / (alpha + beta)) * 100)
    : 0;

  // Ratios (raw, not percentage)
  const alphaTheta = theta > 0 ? Math.round((alpha / theta) * 100) / 100 : 0;
  const betaAlpha = alpha > 0 ? Math.round((beta / alpha) * 100) / 100 : 0;

  return {
    delta: Math.round(deltaPct * 10) / 10,
    theta: Math.round(thetaPct * 10) / 10,
    alpha: Math.round(alphaPct * 10) / 10,
    beta: Math.round(betaPct * 10) / 10,
    gamma: Math.round(gammaPct * 10) / 10,
    smr: Math.round(smrPct * 10) / 10,
    lowBeta: Math.round(lowBetaPct * 10) / 10,
    highBeta: Math.round(highBetaPct * 10) / 10,
    alphaPeak,
    focusScore,
    relaxationScore,
    alphaTheta,
    betaAlpha,
  };
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
