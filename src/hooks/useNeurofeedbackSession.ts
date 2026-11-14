import { useState, useRef, useCallback, useEffect } from "react";
import type {
  NfProtocol,
  BrainState,
  SessionPhase,
  StateTimelineEntry,
  FrequencyBandPowers,
  NfSession,
  FeedbackSignal,
  OverallQuality,
} from "../../src-console/legacy-core/lib/types";
import { classifyState, checkInZone, type InZoneThresholds } from "../../src-console/legacy-core/lib/neurofeedback/brainState";
import { computeTickScore, computeTimeInZonePct, computeTotalScore } from "@/lib/neurofeedback/scoring";
import { computeFeedbackSignal, resetOutputEngine } from "../../src-console/legacy-core/lib/neurofeedback/outputEngine";
import { resetPolicyState } from "../../src-console/legacy-core/lib/neurofeedback/policyEngine";
import {
  MockActuator,
  AudioActuator,
  VisualActuator,
  type FeedbackActuator,
  type VisualFeedbackState,
} from "@/lib/neurofeedback/actuators";
import type { useAudioFeedback } from "../../src-console/legacy-core/hooks/useAudioFeedback";

const TICK_MS = 500;
const CALIBRATION_MS = 5000;
const COUNTDOWN_SECS = 3;

export interface GateStatus {
  allowed: boolean;
  gated: boolean;
  reason?: string;
}

export function useNeurofeedbackSession(
  fftBands: FrequencyBandPowers,
  audio: ReturnType<typeof useAudioFeedback>,
  confidence: number,
  quality: OverallQuality,
  artifactRate: number,
  frozen: boolean,
) {
  const [phase, setPhase] = useState<SessionPhase>("idle");
  const [protocol, setProtocol] = useState<NfProtocol | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [duration, setDuration] = useState(0);
  const [currentState, setCurrentState] = useState<BrainState>("neutral");
  const [inZone, setInZone] = useState(false);
  const [score, setScore] = useState(0);
  const [consecutiveCalm, setConsecutiveCalm] = useState(0);
  const [timeline, setTimeline] = useState<StateTimelineEntry[]>([]);
  const [signal, setSignal] = useState<FeedbackSignal>({ action: "neutral", intensity: 0, gated: false });
  const [visualState, setVisualState] = useState<VisualFeedbackState>({
    glowColor: "transparent", glowIntensity: 0, pulseActive: false, gated: false,
  });
  const [gateStatus, setGateStatus] = useState<GateStatus>({ allowed: true, gated: false });

  const thresholdsRef = useRef<InZoneThresholds>({ rewardMin: {}, inhibitMax: {} });
  const calibrationRef = useRef<FrequencyBandPowers[]>([]);
  const tickIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const sessionStartRef = useRef<string>("");
  const bandsRef = useRef(fftBands);
  const confidenceRef = useRef(confidence);
  const qualityRef = useRef(quality);
  const artifactRateRef = useRef(artifactRate);
  const frozenRef = useRef(frozen);

  // Actuator instances
  const actuatorsRef = useRef<FeedbackActuator[]>([]);
  const visualActuatorRef = useRef<VisualActuator | null>(null);

  // Keep refs current
  useEffect(() => { bandsRef.current = fftBands; }, [fftBands]);
  useEffect(() => { confidenceRef.current = confidence; }, [confidence]);
  useEffect(() => { qualityRef.current = quality; }, [quality]);
  useEffect(() => { artifactRateRef.current = artifactRate; }, [artifactRate]);
  useEffect(() => { frozenRef.current = frozen; }, [frozen]);

  const remaining = Math.max(0, duration - elapsed);

  const initActuators = useCallback(() => {
    const visual = new VisualActuator();
    const audioActuator = new AudioActuator(audio);
    const actuators: FeedbackActuator[] = [audioActuator, visual];

    // Add mock actuator in development
    try {
      // @ts-expect-error Vite injects import.meta.env at build time
      if (import.meta.env?.DEV) {
        actuators.push(new MockActuator());
      }
    } catch {
      // Not in a Vite environment
    }

    for (const a of actuators) a.init();
    actuatorsRef.current = actuators;
    visualActuatorRef.current = visual;
  }, [audio]);

  const cleanupActuators = useCallback(() => {
    for (const a of actuatorsRef.current) a.cleanup();
    actuatorsRef.current = [];
    visualActuatorRef.current = null;
  }, []);

  const dispatchToActuators = useCallback((sig: FeedbackSignal) => {
    for (const a of actuatorsRef.current) a.update(sig);
    if (visualActuatorRef.current) {
      setVisualState({ ...visualActuatorRef.current.state });
    }
  }, []);

  /** Core tick logic — shared between startSession and resumeSession */
  const runTick = useCallback(() => {
    setElapsed((prev) => prev + 0.5);

    const bands = bandsRef.current;
    const state = classifyState(bands);
    const prot = protocol;
    const iz = prot ? checkInZone(bands, prot, thresholdsRef.current) : false;
    const tickScore = computeTickScore(state, iz);

    setCurrentState(state);
    setInZone(iz);
    setScore((prev) => prev + tickScore);

    setTimeline((prev) => [
      ...prev,
      { timestamp: Date.now(), state, inZone: iz, score: tickScore },
    ]);

    // Track consecutive calm for legacy reward logic
    setConsecutiveCalm((prev) => {
      const next = state === "calm" && iz ? prev + 0.5 : 0;
      return next >= 5 ? 0 : next;
    });

    // Output engine: policy → gate → signal → actuators
    if (prot) {
      const sig = computeFeedbackSignal(
        bands,
        prot,
        thresholdsRef.current,
        confidenceRef.current,
        qualityRef.current,
        artifactRateRef.current,
        frozenRef.current,
      );
      setSignal(sig);
      setGateStatus({
        allowed: !sig.gated,
        gated: sig.gated,
        reason: sig.gateReason,
      });
      dispatchToActuators(sig);
    }
  }, [protocol, dispatchToActuators]);

  const selectProtocol = useCallback((p: NfProtocol, durationMin: number) => {
    setProtocol(p);
    setDuration(durationMin * 60);
    setPhase("selecting");
    setScore(0);
    setElapsed(0);
    setTimeline([]);
    setConsecutiveCalm(0);
    setCurrentState("neutral");
    setInZone(false);
    setSignal({ action: "neutral", intensity: 0, gated: false });
    setGateStatus({ allowed: true, gated: false });
  }, []);

  const startCalibration = useCallback(() => {
    if (!protocol) return;

    setPhase("calibrating");
    calibrationRef.current = [];
    resetOutputEngine();
    resetPolicyState();
    initActuators();

    audio.init();
    audio.setMode(protocol.audioMode);

    // Collect baseline for 5 seconds
    const calInterval = setInterval(() => {
      calibrationRef.current.push({ ...bandsRef.current });
    }, TICK_MS);

    setTimeout(() => {
      clearInterval(calInterval);

      // Compute adaptive thresholds from baseline
      const samples = calibrationRef.current;
      if (samples.length > 0) {
        const rewardMin: Record<string, number> = {};
        const inhibitMax: Record<string, number> = {};

        for (const rb of protocol.rewardBands) {
          const key = rb.label.toLowerCase().replace(/\s+/g, "");
          const vals = samples.map((s) => getBandFromPowers(s, key));
          const avg = vals.reduce((a, b) => a + b, 0) / vals.length;
          rewardMin[rb.label] = avg * 1.1;
        }

        for (const ib of protocol.inhibitBands) {
          const key = ib.label.toLowerCase().replace(/\s+/g, "");
          const vals = samples.map((s) => getBandFromPowers(s, key));
          const avg = vals.reduce((a, b) => a + b, 0) / vals.length;
          inhibitMax[ib.label] = avg * 0.9;
        }

        thresholdsRef.current = { rewardMin, inhibitMax };
      }

      startCountdown();
    }, CALIBRATION_MS);
  }, [protocol, audio, initActuators]);

  const startCountdown = useCallback(() => {
    setPhase("countdown");
    audio.playBell("countdown");

    let count = COUNTDOWN_SECS;
    const cdInterval = setInterval(() => {
      count--;
      if (count <= 0) {
        clearInterval(cdInterval);
        countdownRef.current = null;
        startSession();
      }
    }, 1000);
    countdownRef.current = cdInterval;
  }, [audio]);

  const startSession = useCallback(() => {
    setPhase("running");
    setElapsed(0);
    setTimeline([]);
    setScore(0);
    setConsecutiveCalm(0);
    sessionStartRef.current = new Date().toISOString();

    audio.playBell("start");

    const interval = setInterval(runTick, TICK_MS);
    tickIntervalRef.current = interval;
  }, [audio, runTick]);

  // Auto-complete when time runs out
  useEffect(() => {
    if (phase === "running" && elapsed >= duration && duration > 0) {
      endSession();
    }
  }, [elapsed, duration, phase]);

  const pauseSession = useCallback(() => {
    if (tickIntervalRef.current) {
      clearInterval(tickIntervalRef.current);
      tickIntervalRef.current = null;
    }
    setPhase("paused");
  }, []);

  const resumeSession = useCallback(() => {
    setPhase("running");
    const interval = setInterval(runTick, TICK_MS);
    tickIntervalRef.current = interval;
  }, [runTick]);

  const endSession = useCallback((): NfSession | null => {
    if (tickIntervalRef.current) {
      clearInterval(tickIntervalRef.current);
      tickIntervalRef.current = null;
    }

    audio.playBell("end");
    setPhase("complete");
    cleanupActuators();

    if (!protocol) return null;

    const alphaPeaks = timeline
      .map(() => bandsRef.current.alphaPeak)
      .filter((v) => v > 0);
    const avgAlphaPeak =
      alphaPeaks.length > 0
        ? alphaPeaks.reduce((a, b) => a + b, 0) / alphaPeaks.length
        : null;

    const session: NfSession = {
      id: crypto.randomUUID(),
      device_type: "muse",
      start: sessionStartRef.current,
      end: new Date().toISOString(),
      duration_secs: Math.round(elapsed),
      data_points: timeline.length,
      file_path: "",
      summary: {
        avg_focus: bandsRef.current.focusScore,
        avg_relaxation: bandsRef.current.relaxationScore,
      },
      protocol_id: protocol.id,
      protocol_name: protocol.name,
      total_score: computeTotalScore(timeline),
      time_in_zone_pct: computeTimeInZonePct(timeline),
      state_timeline: timeline,
      avg_alpha_peak: avgAlphaPeak,
      band_averages: {
        delta: bandsRef.current.delta,
        theta: bandsRef.current.theta,
        alpha: bandsRef.current.alpha,
        beta: bandsRef.current.beta,
        smr: bandsRef.current.smr,
        lowBeta: bandsRef.current.lowBeta,
        highBeta: bandsRef.current.highBeta,
      },
    };

    return session;
  }, [protocol, timeline, elapsed, audio, cleanupActuators]);

  const reset = useCallback(() => {
    if (tickIntervalRef.current) {
      clearInterval(tickIntervalRef.current);
      tickIntervalRef.current = null;
    }
    if (countdownRef.current) {
      clearInterval(countdownRef.current);
      countdownRef.current = null;
    }
    cleanupActuators();
    resetOutputEngine();
    resetPolicyState();
    audio.cleanup();
    setPhase("idle");
    setProtocol(null);
    setElapsed(0);
    setDuration(0);
    setScore(0);
    setTimeline([]);
    setCurrentState("neutral");
    setInZone(false);
    setConsecutiveCalm(0);
    setSignal({ action: "neutral", intensity: 0, gated: false });
    setVisualState({ glowColor: "transparent", glowIntensity: 0, pulseActive: false, gated: false });
    setGateStatus({ allowed: true, gated: false });
  }, [audio, cleanupActuators]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (tickIntervalRef.current) clearInterval(tickIntervalRef.current);
      if (countdownRef.current) clearInterval(countdownRef.current);
    };
  }, []);

  return {
    phase,
    protocol,
    elapsed,
    remaining,
    duration,
    currentState,
    inZone,
    score,
    consecutiveCalm,
    timeline,
    signal,
    visualState,
    gateStatus,
    selectProtocol,
    startCalibration,
    startSession,
    pauseSession,
    resumeSession,
    endSession,
    reset,
  };
}

function getBandFromPowers(bands: FrequencyBandPowers, key: string): number {
  const map: Record<string, number> = {
    delta: bands.delta,
    theta: bands.theta,
    alpha: bands.alpha,
    beta: bands.beta,
    gamma: bands.gamma,
    smr: bands.smr,
    lowbeta: bands.lowBeta,
    highbeta: bands.highBeta,
  };
  return map[key] ?? 0;
}
