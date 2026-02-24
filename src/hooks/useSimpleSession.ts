/**
 * Simple session hook: guided Calm/Focus/Neutral sessions.
 *
 * State machine: idle → warmup(10s) → running → cooldown(5s) → complete
 * 1Hz timeline capture during running phase.
 */

import { useState, useRef, useCallback, useEffect } from "react";
import type {
  SessionMode,
  SessionModeId,
  SimpleSessionPhase,
  SessionTimelineEntry,
  SimpleSessionSummary,
  FrequencyBandPowers,
  OverallQuality,
} from "@/lib/types";

const WARMUP_SECS = 10;
const COOLDOWN_SECS = 5;
const TICK_MS = 1000; // 1Hz timeline capture

export const SESSION_MODES: SessionMode[] = [
  {
    id: "calm",
    label: "Calm",
    description: "Relaxation-focused session",
    icon: "Leaf",
    primaryMetric: "relaxationScore",
    defaultDurationMin: 10,
  },
  {
    id: "focus",
    label: "Focus",
    description: "Concentration training",
    icon: "Crosshair",
    primaryMetric: "focusScore",
    defaultDurationMin: 15,
  },
  {
    id: "neutral",
    label: "Neutral",
    description: "Alpha/Theta balance",
    icon: "Circle",
    primaryMetric: "alphaTheta",
    defaultDurationMin: 10,
  },
];

export function useSimpleSession(
  fftBands: FrequencyBandPowers,
  confidence: number,
  quality: OverallQuality,
) {
  const [phase, setPhase] = useState<SimpleSessionPhase>("idle");
  const [mode, setMode] = useState<SessionMode | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [duration, setDuration] = useState(0);
  const [summary, setSummary] = useState<SimpleSessionSummary | null>(null);

  const timelineRef = useRef<SessionTimelineEntry[]>([]);
  const tickRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const warmupRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cooldownRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const startedAtRef = useRef("");
  const bandsRef = useRef(fftBands);
  const confidenceRef = useRef(confidence);
  const qualityRef = useRef(quality);

  useEffect(() => { bandsRef.current = fftBands; }, [fftBands]);
  useEffect(() => { confidenceRef.current = confidence; }, [confidence]);
  useEffect(() => { qualityRef.current = quality; }, [quality]);

  const remaining = Math.max(0, duration - elapsed);

  const clearAllTimers = useCallback(() => {
    if (tickRef.current) { clearInterval(tickRef.current); tickRef.current = null; }
    if (warmupRef.current) { clearTimeout(warmupRef.current); warmupRef.current = null; }
    if (cooldownRef.current) { clearTimeout(cooldownRef.current); cooldownRef.current = null; }
  }, []);

  const startRunning = useCallback(() => {
    setPhase("running");
    setElapsed(0);
    timelineRef.current = [];
    startedAtRef.current = new Date().toISOString();

    tickRef.current = setInterval(() => {
      setElapsed((prev) => prev + 1);

      // Capture timeline entry
      const bands = bandsRef.current;
      timelineRef.current.push({
        timestamp: Date.now(),
        bands: { ...bands },
        focusScore: bands.focusScore,
        relaxationScore: bands.relaxationScore,
        confidence: confidenceRef.current,
        quality: qualityRef.current,
      });
    }, TICK_MS);
  }, []);

  const startSession = useCallback((sessionMode: SessionMode, durationMin: number) => {
    clearAllTimers();
    setMode(sessionMode);
    setDuration(durationMin * 60);
    setSummary(null);
    setElapsed(0);
    timelineRef.current = [];

    // Warmup phase
    setPhase("warmup");
    warmupRef.current = setTimeout(() => {
      warmupRef.current = null;
      startRunning();
    }, WARMUP_SECS * 1000);
  }, [clearAllTimers, startRunning]);

  // Auto-transition to cooldown when time runs out
  useEffect(() => {
    if (phase === "running" && elapsed >= duration && duration > 0) {
      endSession();
    }
  }, [elapsed, duration, phase]);

  const endSession = useCallback(() => {
    if (tickRef.current) {
      clearInterval(tickRef.current);
      tickRef.current = null;
    }

    // Cooldown phase
    setPhase("cooldown");
    cooldownRef.current = setTimeout(() => {
      cooldownRef.current = null;

      // Compute summary
      const tl = timelineRef.current;
      const currentMode = mode;
      if (tl.length > 0 && currentMode) {
        const avgFocus = tl.reduce((a, e) => a + e.focusScore, 0) / tl.length;
        const avgRelax = tl.reduce((a, e) => a + e.relaxationScore, 0) / tl.length;
        const avgConf = tl.reduce((a, e) => a + e.confidence, 0) / tl.length;

        let avgPrimary: number;
        if (currentMode.primaryMetric === "focusScore") {
          avgPrimary = avgFocus;
        } else if (currentMode.primaryMetric === "relaxationScore") {
          avgPrimary = avgRelax;
        } else {
          avgPrimary = tl.reduce((a, e) => a + e.bands.alphaTheta, 0) / tl.length;
        }

        setSummary({
          mode: currentMode.id,
          durationSecs: elapsed,
          avgPrimary: Math.round(avgPrimary * 10) / 10,
          avgFocus: Math.round(avgFocus),
          avgRelax: Math.round(avgRelax),
          avgConfidence: Math.round(avgConf * 100) / 100,
          timeline: tl,
          startedAt: startedAtRef.current,
          endedAt: new Date().toISOString(),
        });
      }

      setPhase("complete");
    }, COOLDOWN_SECS * 1000);
  }, [mode, elapsed]);

  const resetSession = useCallback(() => {
    clearAllTimers();
    setPhase("idle");
    setMode(null);
    setElapsed(0);
    setDuration(0);
    setSummary(null);
    timelineRef.current = [];
  }, [clearAllTimers]);

  // Cleanup on unmount
  useEffect(() => {
    return () => clearAllTimers();
  }, [clearAllTimers]);

  return {
    phase,
    mode,
    elapsed,
    remaining,
    duration,
    summary,
    startSession,
    endSession,
    resetSession,
  };
}
