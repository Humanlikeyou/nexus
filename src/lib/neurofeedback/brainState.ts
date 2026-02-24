import type { FrequencyBandPowers, BrainState, NfProtocol } from "@/lib/types";

export function classifyState(bands: FrequencyBandPowers): BrainState {
  const { alpha, theta, highBeta } = bands;

  // Alpha dominant → calm
  if (alpha > theta && alpha > highBeta && alpha > 25) {
    return "calm";
  }

  // Theta or high-beta dominant → active / agitated
  if (highBeta > alpha || theta > alpha * 1.5) {
    return "active";
  }

  return "neutral";
}

export interface InZoneThresholds {
  rewardMin: Record<string, number>;
  inhibitMax: Record<string, number>;
}

export function checkInZone(
  bands: FrequencyBandPowers,
  protocol: NfProtocol,
  thresholds: InZoneThresholds,
): boolean {
  // All reward bands must exceed their threshold
  for (const rb of protocol.rewardBands) {
    const power = getBandPower(bands, rb.label);
    const min = thresholds.rewardMin[rb.label] ?? 0;
    if (power < min) return false;
  }

  // All inhibit bands must be below their threshold
  for (const ib of protocol.inhibitBands) {
    const power = getBandPower(bands, ib.label);
    const max = thresholds.inhibitMax[ib.label] ?? 100;
    if (power > max) return false;
  }

  return true;
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
