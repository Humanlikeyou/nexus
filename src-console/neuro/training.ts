import { computeBands } from "../../src/hooks/useFFT";
import { assessChannel } from "../../src/hooks/useSignalQuality";
import { computeFeedbackSignal, resetOutputEngine } from "../../src/lib/neurofeedback/outputEngine";
import { resetPolicyState } from "../../src/lib/neurofeedback/policyEngine";
import { computeJawPower, detectAllArtifacts } from "../../src/lib/artifactDetection";
import type { InZoneThresholds } from "../../src/lib/neurofeedback/brainState";
import type { FrequencyBandPowers, NfProtocol, FeedbackSignal } from "../../src/lib/types";
import type { MuseTelemetry } from "./useMuseTelemetry";

const CHANNELS = ["TP9", "AF7", "AF8", "TP10"];
const CALIBRATION_TICKS = 10;
const TICK_MS = 500;
/** Calm = relaxation at or above this multiple of the session's relaxation baseline (Decision 5). */
export const CALM_RATIO = 1.1;
const FALLBACK_BASELINE_MS = 60_000;

export type TimelineEntry = { timestamp: number; focus: number; relaxation: number; action: string; gated: boolean };

export type TrainingState = {
  phase: "off" | "calibrating" | "training" | "held";
  label: string;
  calibrationPercent: number;
  signal: FeedbackSignal | null;
  focus: number | null;
  relaxation: number | null;
  /** Live Calm % (0–100) so far this session; null until there is a baseline and a valid entry. */
  calmPercent: number | null;
};

export const INACTIVE_TRAINING: TrainingState = { phase: "off", label: "Recording only", calibrationPercent: 0, signal: null, focus: null, relaxation: null, calmPercent: null };

const mean = (values: number[]) => values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null;

/**
 * The session's relaxation baseline: the mean relaxation during calibration, or,
 * with no calibration, the mean over non-gated entries in the first 60 s.
 */
export function relaxationBaseline(calibration: number[], timeline: TimelineEntry[]): number | null {
  const calibrated = mean(calibration.filter(Number.isFinite));
  if (calibrated !== null) return calibrated;
  const valid = timeline.filter((entry) => !entry.gated && Number.isFinite(entry.relaxation));
  if (!valid.length) return null;
  const start = valid[0].timestamp;
  return mean(valid.filter((entry) => entry.timestamp - start < FALLBACK_BASELINE_MS).map((entry) => entry.relaxation));
}

/** Share (0–100) of non-gated entries with relaxation >= CALM_RATIO x baseline; null without data. */
export function calmPercent(timeline: TimelineEntry[], baseline: number | null): number | null {
  if (baseline === null || !Number.isFinite(baseline)) return null;
  const valid = timeline.filter((entry) => !entry.gated);
  if (!valid.length) return null;
  return valid.filter((entry) => entry.relaxation >= CALM_RATIO * baseline).length / valid.length * 100;
}

function bandValue(bands: FrequencyBandPowers, label: string): number {
  const keys: Record<string, keyof FrequencyBandPowers> = { delta: "delta", theta: "theta", alpha: "alpha", beta: "beta", gamma: "gamma", smr: "smr", lowbeta: "lowBeta", highbeta: "highBeta" };
  return bands[keys[label.toLowerCase().replace(/\s+/g, "")]] ?? 0;
}

export class TrainingSession {
  private baseline: FrequencyBandPowers[] = [];
  private thresholds: InZoneThresholds | null = null;
  private lastTick = -Infinity;
  private lastPacket = -1;
  private jawBaseline = 0;
  private artifacts: number[] = [];
  private lastArtifactAt = -Infinity;
  private entries: TimelineEntry[] = [];
  private calibrationRelaxation: number[] = [];
  state: TrainingState = { ...INACTIVE_TRAINING };

  constructor(readonly protocol: NfProtocol) {
    resetOutputEngine();
    resetPolicyState();
    this.state = { ...INACTIVE_TRAINING, phase: "calibrating", label: "Calibrating" };
  }

  tick(telemetry: MuseTelemetry, now: number): TrainingState {
    if (now - this.lastTick < TICK_MS) return this.state;
    this.lastTick = now;
    const stale = CHANNELS.some((channel) => !telemetry.channelLastPacketAt[channel] || now - telemetry.channelLastPacketAt[channel] > 1500);
    const incomplete = CHANNELS.some((channel) => telemetry.channels[channel]?.length < 256);
    const poor = CHANNELS.some((channel) => {
      const samples = telemetry.channels[channel] ?? [];
      return samples.some((value) => !Number.isFinite(value)) || assessChannel(samples.slice(-256)) !== "good";
    });
    if (stale || incomplete || poor || telemetry.packetCount === this.lastPacket) {
      if (!this.thresholds) this.baseline = [];
      this.state = { ...this.state, phase: "held", label: stale || incomplete ? "Waiting for all electrodes" : "Adjust headset contact", signal: null, focus: null, relaxation: null, calibrationPercent: this.thresholds ? 100 : 0 };
      return this.state;
    }
    this.lastPacket = telemetry.packetCount;
    const windows = Object.fromEntries(CHANNELS.map((channel) => [channel, telemetry.channels[channel].slice(-256)]));
    const detected = detectAllArtifacts(windows, this.jawBaseline, now);
    this.artifacts = this.artifacts.filter((time) => now - time < 60_000);
    if (detected.length) {
      if (now - this.lastArtifactAt >= 1000) { this.artifacts.push(now); this.lastArtifactAt = now; }
      if (!this.thresholds) this.baseline = [];
      this.state = { ...this.state, phase: "held", label: "Movement detected", signal: null, focus: null, relaxation: null, calibrationPercent: this.thresholds ? 100 : 0 };
      return this.state;
    }
    const jawPower = (computeJawPower(windows.AF7) + computeJawPower(windows.AF8)) / 2;
    if (!this.thresholds) this.jawBaseline = this.jawBaseline ? this.jawBaseline * 0.9 + jawPower * 0.1 : jawPower;
    // Preserve the established frontal-electrode metrics without averaging phases.
    const frontal = ["AF7", "AF8"].map((channel) => {
      const samples = telemetry.channels[channel];
      const mean = samples.reduce((sum, value) => sum + value, 0) / samples.length;
      return computeBands(samples.map((value) => value - mean));
    });
    const bands = Object.fromEntries(Object.keys(frontal[0]).map((key) => [key, frontal.reduce((sum, item) => sum + item[key as keyof FrequencyBandPowers], 0) / frontal.length])) as unknown as FrequencyBandPowers;
    if (!this.thresholds) {
      this.baseline.push(bands);
      if (this.baseline.length >= CALIBRATION_TICKS) {
        this.calibrationRelaxation = this.baseline.map((item) => item.relaxationScore);
        const average = (label: string) => this.baseline.reduce((sum, item) => sum + bandValue(item, label), 0) / this.baseline.length;
        this.thresholds = {
          rewardMin: Object.fromEntries(this.protocol.rewardBands.map((band) => [band.label, Math.max(0.01, average(band.label) * 1.1)])),
          inhibitMax: Object.fromEntries(this.protocol.inhibitBands.map((band) => [band.label, Math.max(0.01, average(band.label) * 0.9)])),
        };
      }
      this.state = { phase: "calibrating", label: "Calibrating", calibrationPercent: Math.min(100, this.baseline.length / CALIBRATION_TICKS * 100), signal: null, focus: null, relaxation: null, calmPercent: null };
      return this.state;
    }
    const signal = computeFeedbackSignal(bands, this.protocol, this.thresholds, 1, "good", this.artifacts.length, false);
    this.entries.push({ timestamp: now, focus: bands.focusScore, relaxation: bands.relaxationScore, action: signal.action, gated: signal.gated });
    this.state = {
      phase: signal.gated ? "held" : "training",
      label: signal.gated ? "Feedback paused" : signal.action === "reward" ? "In target range" : signal.action === "inhibit" ? "Above inhibit threshold" : "Approaching target",
      calibrationPercent: 100, signal, focus: signal.gated ? null : bands.focusScore, relaxation: signal.gated ? null : bands.relaxationScore,
      calmPercent: calmPercent(this.entries, relaxationBaseline(this.calibrationRelaxation, this.entries)),
    };
    return this.state;
  }

  summary() {
    const valid = this.entries.filter((entry) => !entry.gated);
    const average = (key: "focus" | "relaxation") => valid.length ? valid.reduce((sum, entry) => sum + entry[key], 0) / valid.length : null;
    const baseline = relaxationBaseline(this.calibrationRelaxation, this.entries);
    return {
      protocol_id: this.protocol.id, protocol_name: this.protocol.name, avg_focus: average("focus"), avg_relaxation: average("relaxation"),
      time_in_target_pct: valid.length ? valid.filter((entry) => entry.action === "reward").length / valid.length * 100 : null,
      relaxation_baseline: baseline, calm_pct: calmPercent(this.entries, baseline), timeline: [...this.entries],
    };
  }
}
