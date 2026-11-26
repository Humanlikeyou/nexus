import type { FrequencyBandPowers, NfProtocol, OverallQuality, FeedbackSignal, TrainingState } from "../types";
import type { InZoneThresholds } from "./brainState";
import { evaluatePolicy } from "./policyEngine";
import { evaluateGate } from "./safetyGate";

let lastSignal: FeedbackSignal = {
  action: "neutral",
  intensity: 0,
  gated: false,
};

/**
 * Combine policy engine + safety gate into a single FeedbackSignal.
 *
 * 1. Run evaluatePolicy() → PolicyDecision (+ trainingState)
 * 2. Run evaluateGate() → GateResult (with optional per-protocol overrides)
 * 3. If gate says hold → return last signal with gated: true
 * 4. If gate says suppress → return neutral with gated: true
 * 5. If gate allows → return policy result ungated
 */
export function computeFeedbackSignal(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
  thresholds: InZoneThresholds,
  confidence: number,
  quality: OverallQuality,
  artifactRate: number,
  frozen: boolean,
): FeedbackSignal & { trainingState: TrainingState } {
  const policy = evaluatePolicy(bands, protocol, thresholds);
  const gate = evaluateGate(confidence, quality, artifactRate, frozen, protocol.gateOverrides);

  if (gate.holdState) {
    // Decision 0057: a held (frozen) signal keeps nothing of the last one that could reward. A repeated
    // "reward" from before the freeze would be feedback on data that no longer exists.
    return {
      ...lastSignal,
      action: "neutral",
      intensity: 0,
      gated: true,
      gateReason: gate.reason,
      policyDetail: policy.detail,
      trainingState: policy.trainingState,
    };
  }

  if (!gate.allowed) {
    const suppressed: FeedbackSignal = {
      action: "neutral",
      intensity: 0,
      gated: true,
      gateReason: gate.reason,
      policyDetail: policy.detail,
    };
    lastSignal = suppressed;
    return { ...suppressed, trainingState: policy.trainingState };
  }

  const intensity = policy.action === "reward"
    ? policy.rewardIntensity
    : policy.action === "inhibit"
      ? policy.inhibitIntensity
      : 0;

  const signal: FeedbackSignal = {
    action: policy.action,
    intensity,
    gated: false,
    policyDetail: policy.detail,
  };

  lastSignal = signal;
  return { ...signal, trainingState: policy.trainingState };
}

/** Reset held state (call on session start/reset). */
export function resetOutputEngine(): void {
  lastSignal = { action: "neutral", intensity: 0, gated: false };
}
