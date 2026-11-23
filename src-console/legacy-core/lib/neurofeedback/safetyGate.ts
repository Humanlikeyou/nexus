import type { OverallQuality, GateResult } from "../types";

/**
 * Evaluate whether feedback output should be allowed, held, or suppressed.
 *
 * Rules (in priority order):
 * 1. confidence < 0.3 → suppress all output
 * 2. quality === "frozen" → hold last state
 * 3. artifactRate >= 15/min → suppress (artifact burst)
 * 4. quality === "poor" → suppress
 * 5. Otherwise → allowed
 */
export function evaluateGate(
  confidence: number,
  quality: OverallQuality,
  artifactRate: number,
  frozen: boolean,
  overrides?: { maxArtifactRate?: number; minConfidence?: number },
): GateResult {
  const minConfidence = overrides?.minConfidence ?? 0.3;
  const maxArtifactRate = overrides?.maxArtifactRate ?? 15;

  if (confidence < minConfidence) {
    return { allowed: false, holdState: false, reason: "Low confidence" };
  }

  if (frozen || quality === "frozen") {
    return { allowed: false, holdState: true, reason: "Signal frozen" };
  }

  if (artifactRate >= maxArtifactRate) {
    return { allowed: false, holdState: false, reason: "Artifact burst" };
  }

  if (quality === "poor") {
    return { allowed: false, holdState: false, reason: "Poor signal quality" };
  }

  return { allowed: true, holdState: false, reason: "OK" };
}
