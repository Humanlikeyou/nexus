/**
 * Decision 0059 (step 8a): configurable neurofeedback protocols with explicit target-feature definitions.
 *
 * A target feature says exactly what the feedback follows: which band, the edges the training FFT really
 * computes (not only the edges a protocol declares), the measure, which direction is rewarded, what it is
 * compared with, and the method version. The protocol set is versioned, so a session record can name it.
 *
 * These are EEG signal features. They are not measures of emotion, attention, stress or relaxation.
 */
import type { NfProtocol } from "../legacy-core/lib/types";
import { TARGET_REWARD_RATE, TRAINING_METHOD } from "./training";

export const PROTOCOL_SET_VERSION = "protocols-v1-0059";

/** The edges `computeBands` actually integrates, per protocol band label. */
export const COMPUTED_BANDS_HZ: Record<string, [number, number]> = {
  delta: [1.5, 4], theta: [4, 8], alpha: [8, 13], beta: [13, 30], gamma: [30, 50],
  smr: [12, 15], lowbeta: [15, 20], highbeta: [20, 30],
};

export type FeatureRole = "reward" | "inhibit" | "drift" | "coupling" | "monitor";

export type TargetFeature = {
  /** Stable id, for example `reward:alpha`. */
  id: string;
  role: FeatureRole;
  band: string;
  /** What the training computes. */
  computedHz: [number, number] | null;
  /** What the protocol declares, when that differs from what is computed. */
  declaredHz?: [number, number];
  measure: string;
  /** What earns reward (or what is held back). */
  direction: "above_threshold" | "below_threshold" | "stable_ratio" | "no_rise" | "shown_only";
  reference: string;
  method: string;
};

const key = (label: string) => label.toLowerCase().replace(/\s+/g, "");
const MEASURE = "relative band power, % of 1.5–50 Hz total, AF7 and AF8 averaged";
const REFERENCE = "this session's own baseline mean (eyes open or closed as the protocol trains): reward above x1.1, inhibit above x0.9; the reward threshold is scaled toward the target reward rate every 30 s";

function feature(role: FeatureRole, band: { label: string; minHz?: number; maxHz?: number }, direction: TargetFeature["direction"], reference = REFERENCE, measure = MEASURE): TargetFeature {
  const computed = COMPUTED_BANDS_HZ[key(band.label)] ?? null;
  const declared: [number, number] | undefined = band.minHz != null && band.maxHz != null ? [band.minHz, band.maxHz] : undefined;
  const differs = declared && (!computed || declared[0] !== computed[0] || declared[1] !== computed[1]);
  return { id: `${role}:${key(band.label)}`, role, band: band.label, computedHz: computed, ...(differs ? { declaredHz: declared } : {}), measure, direction, reference, method: TRAINING_METHOD.version };
}

/** Every feature a protocol trains on, monitors or holds back, in a fixed order. */
export function targetFeatures(protocol: NfProtocol): TargetFeature[] {
  const out: TargetFeature[] = [];
  if (protocol.couplingReward) {
    const { bandA, bandB } = protocol.couplingReward;
    out.push({ ...feature("coupling", { label: `${bandA}/${bandB}` }, "stable_ratio", "coefficient of variation of the ratio over the recent window, below the stability limit", `ratio of ${bandA} to ${bandB} relative power`), computedHz: null });
  }
  for (const band of protocol.rewardBands) out.push(feature("reward", band, protocol.rewardStability ? "stable_ratio" : "above_threshold"));
  for (const band of protocol.inhibitBands) out.push(feature("inhibit", band, "below_threshold"));
  for (const band of protocol.suppressOnDrift ?? []) out.push(feature("drift", band, "no_rise", "a sustained rise over the recent window holds reward"));
  for (const label of protocol.passthroughBands ?? []) out.push(feature("monitor", { label }, "shown_only", "none: shown, never rewarded or inhibited"));
  return out;
}

export type ProtocolSound = "soundscape" | "tone" | "silent";

export type ProtocolSettings = {
  durationMin: number;
  targetRewardRate: number;
  sound: ProtocolSound;
  visualFeedback: boolean;
  volume: number;
};

export const SETTINGS_LIMITS = { durationMin: [5, 60], targetRewardRate: [0.4, 0.8], volume: [0, 1] } as const;

/** Defaults reproduce the training behaviour from before 0059. */
export function defaultSettings(protocol: NfProtocol): ProtocolSettings {
  const [lo, hi] = SETTINGS_LIMITS.durationMin;
  return { durationMin: Math.min(hi, Math.max(lo, protocol.defaultDurationMin)), targetRewardRate: TARGET_REWARD_RATE, sound: protocol.audioMode, visualFeedback: false, volume: 0.7 };
}

/** Saved settings when valid, else the protocol's defaults. Never lets a bad value reach training. */
export function effectiveSettings(protocol: NfProtocol, saved: Partial<ProtocolSettings> | null | undefined): ProtocolSettings {
  const base = defaultSettings(protocol);
  if (!saved) return base;
  const inRange = (v: unknown, [lo, hi]: readonly [number, number]) => typeof v === "number" && Number.isFinite(v) && v >= lo && v <= hi;
  return {
    durationMin: inRange(saved.durationMin, SETTINGS_LIMITS.durationMin) && Number.isInteger(saved.durationMin) ? saved.durationMin as number : base.durationMin,
    targetRewardRate: inRange(saved.targetRewardRate, SETTINGS_LIMITS.targetRewardRate) ? saved.targetRewardRate as number : base.targetRewardRate,
    sound: saved.sound === "soundscape" || saved.sound === "tone" || saved.sound === "silent" ? saved.sound : base.sound,
    visualFeedback: typeof saved.visualFeedback === "boolean" ? saved.visualFeedback : base.visualFeedback,
    volume: inRange(saved.volume, SETTINGS_LIMITS.volume) ? saved.volume as number : base.volume,
  };
}

/** Plain words for a feature, for settings and records. */
export function describeFeature(f: TargetFeature): string {
  const hz = f.computedHz ? ` (${f.computedHz[0]}–${f.computedHz[1]} Hz)` : "";
  const what = { above_threshold: "rewarded above its threshold", below_threshold: "held back above its threshold", stable_ratio: "rewarded when steady", no_rise: "a sustained rise holds reward", shown_only: "shown only" }[f.direction];
  return `${f.band}${hz}: ${what}`;
}
