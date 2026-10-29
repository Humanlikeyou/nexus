/**
 * Score confidence + hysteresis: produces smooth display scores.
 *
 * Confidence (0-1) based on signal quality and artifact rate.
 * Hysteresis: rate-limits score changes to MAX_DELTA points/sec.
 * Frozen: holds last-good values when signal is frozen.
 */

import { useState, useRef, useEffect } from "react";
import type { ScoreConfidenceState, OverallQuality } from "@/lib/types";

const TICK_MS = 500;
const MAX_DELTA_PER_SEC = 5; // 5 points/sec → 2.5 per 500ms tick
const MAX_DELTA_PER_TICK = MAX_DELTA_PER_SEC * (TICK_MS / 1000);

function signalFactor(quality: OverallQuality): number {
  switch (quality) {
    case "good": return 1.0;
    case "fair": return 0.7;
    case "poor": return 0.3;
    case "frozen": return 0.0;
  }
}

function artifactFactor(rate: number): number {
  if (rate <= 0) return 1.0;
  if (rate >= 20) return 0.1;
  // Linear interpolation: 0→1.0, 5→0.7, 10→0.4, 20→0.1
  if (rate <= 5) return 1.0 - (rate / 5) * 0.3;
  if (rate <= 10) return 0.7 - ((rate - 5) / 5) * 0.3;
  return 0.4 - ((rate - 10) / 10) * 0.3;
}

function clampDelta(raw: number, prev: number, maxDelta: number): number {
  const diff = raw - prev;
  if (Math.abs(diff) <= maxDelta) return raw;
  return prev + Math.sign(diff) * maxDelta;
}

export function useScoreConfidence(
  rawFocus: number,
  rawRelax: number,
  quality: OverallQuality,
  frozen: boolean,
  artifactRate: number,
): ScoreConfidenceState {
  const [state, setState] = useState<ScoreConfidenceState>({
    confidence: 1,
    displayFocus: rawFocus,
    displayRelax: rawRelax,
    frozen: false,
  });

  const prevFocusRef = useRef(rawFocus);
  const prevRelaxRef = useRef(rawRelax);
  const lastGoodFocusRef = useRef(rawFocus);
  const lastGoodRelaxRef = useRef(rawRelax);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Keep raw refs current for the interval callback
  const rawFocusRef = useRef(rawFocus);
  const rawRelaxRef = useRef(rawRelax);
  const qualityRef = useRef(quality);
  const frozenRef = useRef(frozen);
  const artifactRateRef = useRef(artifactRate);

  useEffect(() => {
    rawFocusRef.current = rawFocus;
    rawRelaxRef.current = rawRelax;
    qualityRef.current = quality;
    frozenRef.current = frozen;
    artifactRateRef.current = artifactRate;
  }, [rawFocus, rawRelax, quality, frozen, artifactRate]);

  useEffect(() => {
    intervalRef.current = setInterval(() => {
      const isFrozen = frozenRef.current;
      const q = qualityRef.current;
      const rate = artifactRateRef.current;

      const conf = signalFactor(q) * artifactFactor(rate);

      if (isFrozen) {
        // Hold last-good values
        setState({
          confidence: conf,
          displayFocus: lastGoodFocusRef.current,
          displayRelax: lastGoodRelaxRef.current,
          frozen: true,
        });
        return;
      }

      // Apply hysteresis
      const targetFocus = rawFocusRef.current;
      const targetRelax = rawRelaxRef.current;
      const newFocus = clampDelta(targetFocus, prevFocusRef.current, MAX_DELTA_PER_TICK);
      const newRelax = clampDelta(targetRelax, prevRelaxRef.current, MAX_DELTA_PER_TICK);

      prevFocusRef.current = newFocus;
      prevRelaxRef.current = newRelax;
      lastGoodFocusRef.current = newFocus;
      lastGoodRelaxRef.current = newRelax;

      setState({
        confidence: conf,
        displayFocus: Math.round(newFocus),
        displayRelax: Math.round(newRelax),
        frozen: false,
      });
    }, TICK_MS);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, []);

  return state;
}
