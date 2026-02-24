import type {
  FrequencyBandPowers,
  NfProtocol,
  PolicyDecision,
  TrainingState,
} from "@/lib/types";
import { GAMMA_ARTIFACT_THRESHOLD } from "@/lib/types";
import type { InZoneThresholds } from "./brainState";

// ── Sliding-window state ──────────────────────────────────────

const COUPLING_WINDOW_SIZE = 10;  // 10 ticks = 5 seconds at 500ms
const STABILITY_WINDOW_SIZE = 10;
const DRIFT_WINDOW_SIZE = 6;      // 6 ticks = 3 seconds
const STABILITY_CV_THRESHOLD = 0.3;
const DRIFT_INCREASE_THRESHOLD = 0.10; // 10%
const SUSTAINING_THRESHOLD = 10;  // consecutive reward ticks

let couplingWindow: number[] = [];
let stabilityWindows: Map<string, number[]> = new Map();
let driftWindows: Map<string, number[]> = new Map();
let tickCount = 0;
let consecutiveRewardTicks = 0;
let wasInZone = false;

/** Clear all sliding-window state — call on session start/reset. */
export function resetPolicyState(): void {
  couplingWindow = [];
  stabilityWindows = new Map();
  driftWindows = new Map();
  tickCount = 0;
  consecutiveRewardTicks = 0;
  wasInZone = false;
}

// ── Extended return type ──────────────────────────────────────

export type ExtendedPolicyDecision = PolicyDecision & {
  trainingState: TrainingState;
};

// ── Main entry point ──────────────────────────────────────────

export function evaluatePolicy(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
  thresholds: InZoneThresholds,
): ExtendedPolicyDecision {
  tickCount++;

  // ── D. Gamma artifact safeguard ─────────────────────────────
  if (bands.gamma > GAMMA_ARTIFACT_THRESHOLD) {
    consecutiveRewardTicks = 0;
    wasInZone = false;
    return {
      action: "neutral",
      rewardIntensity: 0,
      inhibitIntensity: 0,
      detail: `Gamma artifact (${bands.gamma.toFixed(0)} µV² > ${GAMMA_ARTIFACT_THRESHOLD})`,
      trainingState: deriveTrainingState(false),
    };
  }

  // ── A. Coupling reward ──────────────────────────────────────
  if (protocol.couplingReward) {
    const result = evaluateCouplingReward(bands, protocol, thresholds);
    if (result) return result;
  }

  // ── B. Stability reward modifier ────────────────────────────
  // Computed here, applied to standard reward intensity below
  let stabilityFactor = 1.0;
  let stabilityActive = false;
  if (protocol.rewardStability) {
    const sf = computeStabilityFactor(bands, protocol);
    stabilityFactor = sf.factor;
    stabilityActive = sf.stable;
  }

  // ── C. Drift suppression ────────────────────────────────────
  if (protocol.suppressOnDrift) {
    const driftResult = evaluateDriftSuppression(bands, protocol, thresholds);
    if (driftResult) return driftResult;
  }

  // ── Standard reward/inhibit logic ───────────────────────────
  const rewardRatios: number[] = [];
  const inhibitRatios: number[] = [];

  let allRewardAbove = true;
  let anyInhibitAbove = false;
  let detail = "";

  for (const rb of protocol.rewardBands) {
    const power = getBandPower(bands, rb.label);
    const threshold = thresholds.rewardMin[rb.label];
    if (threshold == null || threshold === 0) {
      rewardRatios.push(0);
      allRewardAbove = false;
      continue;
    }
    const ratio = power / threshold;
    rewardRatios.push(ratio);
    if (ratio < 1.0) {
      allRewardAbove = false;
    }
  }

  for (const ib of protocol.inhibitBands) {
    const power = getBandPower(bands, ib.label);
    const threshold = thresholds.inhibitMax[ib.label];
    if (threshold == null || threshold === 0) {
      inhibitRatios.push(0);
      continue;
    }
    const ratio = power / threshold;
    inhibitRatios.push(ratio);
    if (ratio > 1.0) {
      anyInhibitAbove = true;
    }
  }

  if (anyInhibitAbove) {
    const maxInhibit = Math.max(...inhibitRatios, 0);
    const inhibitIntensity = clamp(maxInhibit - 1.0, 0, 1);
    detail = `Inhibit band elevated (ratio ${maxInhibit.toFixed(2)})`;
    consecutiveRewardTicks = 0;
    wasInZone = false;
    return {
      action: "inhibit",
      rewardIntensity: 0,
      inhibitIntensity,
      detail,
      trainingState: deriveTrainingState(false),
    };
  }

  if (allRewardAbove && rewardRatios.length > 0) {
    const minReward = Math.min(...rewardRatios);
    let rewardIntensity = clamp(minReward - 1.0, 0, 1);

    // Apply stability modifier if active
    if (protocol.rewardStability) {
      if (!stabilityActive) {
        // Power is spiking, not stable — don't reward
        detail = `Reward bands above threshold but unstable (stability factor ${stabilityFactor.toFixed(2)})`;
        consecutiveRewardTicks = 0;
        return {
          action: "neutral",
          rewardIntensity: 0,
          inhibitIntensity: 0,
          detail,
          trainingState: deriveTrainingState(false),
        };
      }
      rewardIntensity *= stabilityFactor;
      detail = `Stable reward (ratio ${minReward.toFixed(2)}, stability ${stabilityFactor.toFixed(2)})`;
    } else {
      detail = `All reward bands above threshold (min ratio ${minReward.toFixed(2)})`;
    }

    consecutiveRewardTicks++;
    wasInZone = true;
    return {
      action: "reward",
      rewardIntensity,
      inhibitIntensity: 0,
      detail,
      trainingState: deriveTrainingState(true),
    };
  }

  detail = "Neutral — reward bands not fully above threshold";
  consecutiveRewardTicks = 0;
  return {
    action: "neutral",
    rewardIntensity: 0,
    inhibitIntensity: 0,
    detail,
    trainingState: deriveTrainingState(false),
  };
}

// ── A. Coupling reward ────────────────────────────────────────

function evaluateCouplingReward(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
  thresholds: InZoneThresholds,
): ExtendedPolicyDecision | null {
  const coupling = protocol.couplingReward!;
  const powerA = getBandPower(bands, coupling.bandA);
  const powerB = getBandPower(bands, coupling.bandB);

  if (powerB === 0) return null; // avoid division by zero

  const ratio = powerA / powerB;

  // Push into sliding window
  couplingWindow.push(ratio);
  if (couplingWindow.length > COUPLING_WINDOW_SIZE) {
    couplingWindow.shift();
  }

  // Need at least 3 samples for meaningful variance
  if (couplingWindow.length < 3) return null;

  const mean = couplingWindow.reduce((a, b) => a + b, 0) / couplingWindow.length;
  const variance =
    couplingWindow.reduce((sum, v) => sum + (v - mean) ** 2, 0) / couplingWindow.length;
  const stddev = Math.sqrt(variance);
  const cv = mean !== 0 ? stddev / Math.abs(mean) : 1;

  // Also check standard inhibit bands — coupling doesn't bypass inhibits
  for (const ib of protocol.inhibitBands) {
    const power = getBandPower(bands, ib.label);
    const threshold = thresholds.inhibitMax[ib.label];
    if (threshold != null && threshold > 0 && power / threshold > 1.0) {
      const inhibitRatio = power / threshold;
      consecutiveRewardTicks = 0;
      wasInZone = false;
      return {
        action: "inhibit",
        rewardIntensity: 0,
        inhibitIntensity: clamp(inhibitRatio - 1.0, 0, 1),
        detail: `Coupling: inhibit band ${ib.label} elevated (ratio ${inhibitRatio.toFixed(2)})`,
        trainingState: deriveTrainingState(false),
      };
    }
  }

  if (coupling.rewardStability && cv < STABILITY_CV_THRESHOLD) {
    // Stable coupling → reward. Intensity = inverse of CV (higher stability → higher)
    const intensity = clamp(1 - cv / STABILITY_CV_THRESHOLD, 0, 1);
    consecutiveRewardTicks++;
    wasInZone = true;
    return {
      action: "reward",
      rewardIntensity: intensity,
      inhibitIntensity: 0,
      detail: `Coupling stable: ${coupling.bandA}/${coupling.bandB} ratio ${ratio.toFixed(2)}, CV ${cv.toFixed(3)}`,
      trainingState: deriveTrainingState(true),
    };
  }

  // Coupling not yet stable
  consecutiveRewardTicks = 0;
  return {
    action: "neutral",
    rewardIntensity: 0,
    inhibitIntensity: 0,
    detail: `Coupling unstable: ${coupling.bandA}/${coupling.bandB} CV ${cv.toFixed(3)} > ${STABILITY_CV_THRESHOLD}`,
    trainingState: deriveTrainingState(false),
  };
}

// ── B. Stability reward ───────────────────────────────────────

function computeStabilityFactor(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
): { factor: number; stable: boolean } {
  let worstCV = 0;

  for (const rb of protocol.rewardBands) {
    const power = getBandPower(bands, rb.label);
    const key = rb.label;

    // Push into per-band sliding window
    const window = stabilityWindows.get(key) ?? [];
    window.push(power);
    if (window.length > STABILITY_WINDOW_SIZE) {
      window.shift();
    }
    stabilityWindows.set(key, window);

    if (window.length < 3) continue;

    const mean = window.reduce((a, b) => a + b, 0) / window.length;
    const variance = window.reduce((sum, v) => sum + (v - mean) ** 2, 0) / window.length;
    const stddev = Math.sqrt(variance);
    const cv = mean !== 0 ? stddev / Math.abs(mean) : 1;

    if (cv > worstCV) worstCV = cv;
  }

  const stable = worstCV < STABILITY_CV_THRESHOLD;
  const factor = stable ? clamp(1 - worstCV, 0, 1) : 0;
  return { factor, stable };
}

// ── C. Drift suppression ──────────────────────────────────────

function evaluateDriftSuppression(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
  thresholds: InZoneThresholds,
): ExtendedPolicyDecision | null {
  for (const db of protocol.suppressOnDrift!) {
    const power = getBandPower(bands, db.label);
    const key = db.label;

    // Push into per-band drift window
    const window = driftWindows.get(key) ?? [];
    window.push(power);
    if (window.length > DRIFT_WINDOW_SIZE) {
      window.shift();
    }
    driftWindows.set(key, window);

    // Need full window for trend analysis
    if (window.length < DRIFT_WINDOW_SIZE) continue;

    // Compare last 3 ticks vs first 3 ticks
    const half = Math.floor(DRIFT_WINDOW_SIZE / 2);
    const firstHalf = window.slice(0, half);
    const lastHalf = window.slice(half);

    const firstAvg = firstHalf.reduce((a, b) => a + b, 0) / firstHalf.length;
    const lastAvg = lastHalf.reduce((a, b) => a + b, 0) / lastHalf.length;

    if (firstAvg === 0) continue;

    const increase = (lastAvg - firstAvg) / firstAvg;

    // Check if trending upward and current power is above baseline
    // Use inhibit threshold if available, otherwise use a reasonable baseline
    const baseline = thresholds.inhibitMax[db.label] ?? thresholds.rewardMin[db.label] ?? firstAvg;

    if (increase > DRIFT_INCREASE_THRESHOLD && power > baseline) {
      const driftIntensity = clamp(increase, 0, 1);
      consecutiveRewardTicks = 0;
      wasInZone = false;
      return {
        action: "inhibit",
        rewardIntensity: 0,
        inhibitIntensity: driftIntensity,
        detail: `Drift suppression: ${db.label} trending up +${(increase * 100).toFixed(0)}%`,
        trainingState: deriveTrainingState(false),
      };
    }
  }

  return null; // no drift detected
}

// ── E. Training state derivation ──────────────────────────────

function deriveTrainingState(rewarding: boolean): TrainingState {
  if (tickCount < 5) return "warming";

  if (rewarding) {
    if (consecutiveRewardTicks >= SUSTAINING_THRESHOLD) return "sustaining";
    return "in_zone";
  }

  if (wasInZone) {
    wasInZone = false;
    return "drifting";
  }

  return "approaching";
}

// ── Helpers ───────────────────────────────────────────────────

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function getBandPower(bands: FrequencyBandPowers, label: string): number {
  const key = label.toLowerCase().replace(/\s+/g, "");
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
