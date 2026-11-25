import { computeBands } from "../legacy-core/hooks/useFFT";
import { assessChannel } from "../legacy-core/hooks/useSignalQuality";
import { computeFeedbackSignal, resetOutputEngine } from "../legacy-core/lib/neurofeedback/outputEngine";
import { resetPolicyState } from "../legacy-core/lib/neurofeedback/policyEngine";
import { computeJawPower, detectAllArtifacts } from "../legacy-core/lib/artifactDetection";
import { removeLineNoise } from "../legacy-core/lib/lineNoise";
import type { InZoneThresholds } from "../legacy-core/lib/neurofeedback/brainState";
import type { FrequencyBandPowers, NfProtocol, FeedbackSignal } from "../legacy-core/lib/types";
import type { MuseTelemetry } from "./useMuseTelemetry";
import { headMoved } from "./headMotion";
import { estimateHeartRate, heartRateSummary } from "./heartRate";

const CHANNELS = ["TP9", "AF7", "AF8", "TP10"];
const CALIBRATION_TICKS = 10;
const TICK_MS = 500;
/** Calm = relaxation at or above this multiple of the session's relaxation baseline (Decision 5). */
export const CALM_RATIO = 1.1;
const FALLBACK_BASELINE_MS = 60_000;

/** Eyes-open and eyes-closed baselines, 20 s each at the 500 ms tick. */
export const BASELINE_TICKS = 40;
/** The reinforcement schedule: reward about 60% of clean time, checked every 30 s (CRED-nf reporting). */
export const TARGET_REWARD_RATE = .6;
export const ADAPT_TICKS = 60;
const ADAPT_BAND = .12, ADAPT_STEP = 1.04, ADAPT_MIN = .7, ADAPT_MAX = 1.5;
/** An electrode reading this long gone is not contact. */
const STALE_MS = 1500;
const FIT_TICKS = 4;
/** A reading is centred half a second back in its one-second FFT window. */
const FFT_WINDOW_DELAY_MS = 500;
const HEART_RATE_EVERY_MS = 5000;
/**
 * AUX is not a training source or shown to anyone until an eyes-closed test proves it. The Muse
 * streams it whether or not an electrode is attached, and a real capture read 76-81% beta.
 */
export const AUX_PROVEN = false;

export type GuidedStage = "fit" | "baseline-open" | "baseline-closed" | "training";
export type HoldReason = "contact" | "dropout" | "blink" | "jaw" | "movement" | "noise" | "frozen" | "not_live";
export type ChannelFit = "good" | "fair" | "poor" | "none";
export const FIT_LABEL: Record<ChannelFit, string> = { good: "Good contact", fair: "Fair contact", poor: "Poor contact", none: "No signal" };

/** Why the feedback is held, in plain words. Holding keeps noise from ever being rewarded. */
export const HOLD_COPY: Record<HoldReason, string> = {
  contact: "Holding: adjust the headband until the sensors settle.",
  dropout: "Holding: the headset signal dropped out.",
  blink: "Holding: that was a blink. Rest your eyes.",
  jaw: "Holding: your jaw tightened. Let it soften.",
  movement: "Holding: you moved. Settle back in.",
  noise: "Holding: the signal is noisy right now.",
  frozen: "Holding: the signal froze, the same values are repeating. Check the headset connection.",
  not_live: "Holding: this is a replay or demo, not your live signal, so there is no feedback.",
};

/** How the numbers in a session summary were made, so each one can be traced (decision 0057). */
export const TRAINING_METHOD = {
  version: "training-v2-0057",
  tick_ms: TICK_MS,
  fft: "legacy-core computeBands: Welch (512 samples, 50% overlap) else single 256-sample PSD, Hann, 256 Hz nominal",
  bands_hz: { delta: [1.5, 4], theta: [4, 8], alpha: [8, 13], beta: [13, 30], gamma: [30, 50] },
  channels: "AF7 and AF8 averaged per band (AUX only when proven)",
  focus: "beta / (beta + theta) x 100",
  relaxation: "alpha / (alpha + beta) x 100",
  thresholds: "baseline mean x 1.1 (reward) and x 0.9 (inhibit), scaled toward 60% reward every 30 s",
} as const;

/** Training ticks looked back over for the feedback gate's confidence (decision 0057). */
const CONFIDENCE_TICKS = 20;

/** Next reward threshold scale from the last window's reward rate. Stays within sensible limits. */
export function adaptScale(scale: number, rewardRate: number): number {
  if (rewardRate > TARGET_REWARD_RATE + ADAPT_BAND) return Math.min(ADAPT_MAX, scale * ADAPT_STEP);
  if (rewardRate < TARGET_REWARD_RATE - ADAPT_BAND) return Math.max(ADAPT_MIN, scale / ADAPT_STEP);
  return scale;
}

/**
 * What the lens shows, 0 to 1. Held and off-target are dark, so noise can't look like success; being
 * in the target lifts it, and staying there is what makes it full and steady.
 */
export function feedbackLevel(state: Pick<TrainingState, "phase" | "stage" | "signal" | "streak">): number {
  if (state.stage !== "training" || state.phase === "held" || !state.signal || state.signal.gated) return 0;
  if (state.signal.action === "reward") return Math.min(1, .5 + (state.streak ?? 0) / 16);
  return state.signal.action === "inhibit" ? .06 : .18;
}

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
  /* Guided sessions only. */
  stage?: GuidedStage;
  hold?: HoldReason | null;
  fit?: Record<string, ChannelFit>;
  fitReady?: boolean;
  /** Consecutive rewarded ticks: what steadies the lens. */
  streak?: number;
  /** Age of the newest EEG when this feedback was computed. */
  delayMs?: number | null;
  /** Share of clean training time rewarded so far, 0–100. */
  targetPercent?: number | null;
  /** The Myndlift electrode is the signal source. Only ever true with real AUX samples. */
  auxSource?: boolean;
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

/**
 * Share (0–100) of non-gated entries with relaxation >= CALM_RATIO x baseline; null without data.
 * A baseline <= 0 has no meaningful ratio, so it yields null; non-finite relaxation entries are
 * left out of both the numerator and the denominator.
 */
export function calmPercent(timeline: TimelineEntry[], baseline: number | null): number | null {
  if (baseline === null || !Number.isFinite(baseline) || baseline <= 0) return null;
  const valid = timeline.filter((entry) => !entry.gated && Number.isFinite(entry.relaxation));
  if (!valid.length) return null;
  return valid.filter((entry) => entry.relaxation >= CALM_RATIO * baseline).length / valid.length * 100;
}

function bandValue(bands: FrequencyBandPowers, label: string): number {
  const keys: Record<string, keyof FrequencyBandPowers> = { delta: "delta", theta: "theta", alpha: "alpha", beta: "beta", gamma: "gamma", smr: "smr", lowbeta: "lowBeta", highbeta: "highBeta" };
  return bands[keys[label.toLowerCase().replace(/\s+/g, "")]] ?? 0;
}

/** Eyes-closed alpha against eyes-open. A clear rise is a good sign the signal is real brain activity. */
export function alphaRatio(open: FrequencyBandPowers[], closed: FrequencyBandPowers[]): number | null {
  const alpha = (items: FrequencyBandPowers[]) => mean(items.map((item) => item.alpha));
  const a = alpha(open), b = alpha(closed);
  return a && b ? b / a : null;
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
  private readonly guided: boolean;
  private stage: GuidedStage = "fit";
  private fitGoodTicks = 0;
  private baselines: Record<"open" | "closed", FrequencyBandPowers[]> = { open: [], closed: [] };
  private baseThresholds: InZoneThresholds | null = null;
  private scale = 1;
  private adjustments: number[] = [];
  private streak = 0;
  private window: boolean[] = [];
  private held: Record<HoldReason, number> = { contact: 0, dropout: 0, blink: 0, jaw: 0, movement: 0, noise: 0, frozen: 0, not_live: 0 };
  /** Training-stage ticks: valid (reached the policy) or not, newest last; and each channel's fit tally. */
  /** Recent training ticks with good-or-fair contact on every electrode (the gate's confidence). */
  private recentFit: boolean[] = [];
  private trainingTicks = 0;
  private validTicks = 0;
  private fitTally: Record<string, Record<ChannelFit, number>> = Object.fromEntries(CHANNELS.map((c) => [c, { good: 0, fair: 0, poor: 0, none: 0 }]));
  private sources = new Set<string>();
  private readonly auxAllowed: boolean;
  private heartRates: number[] = [];
  private lastHeartRateAt = -Infinity;
  state: TrainingState = { ...INACTIVE_TRAINING };

  constructor(readonly protocol: NfProtocol, options: { guided?: boolean; aux?: boolean } = {}) {
    resetOutputEngine();
    resetPolicyState();
    this.guided = options.guided ?? false;
    this.auxAllowed = options.aux ?? AUX_PROVEN;
    this.state = { ...INACTIVE_TRAINING, phase: "calibrating", label: "Calibrating" };
    if (this.guided) this.state = { ...this.state, label: "Checking the fit", stage: "fit", fit: {}, fitReady: false };
  }

  /** Fit is good enough: start the eyes-open baseline. */
  advance() {
    if (this.guided && this.stage === "fit" && this.state.fitReady) {
      this.stage = "baseline-open";
      this.state = { ...this.state, stage: this.stage, label: "Eyes open", calibrationPercent: 0, hold: null };
    }
  }

  private channelFit(telemetry: MuseTelemetry, channel: string, now: number): ChannelFit {
    const at = telemetry.channelLastPacketAt[channel];
    const samples = telemetry.channels[channel] ?? [];
    if (!at || now - at > STALE_MS || samples.length < 256) return "none";
    return samples.some((value) => !Number.isFinite(value)) ? "poor" : assessChannel(samples.slice(-256));
  }

  /** Thresholds from the baseline this protocol trains in: eyes closed for eyes-closed goals. */
  private finishBaseline() {
    const source = this.baselines[this.protocol.eyesClosed ? "closed" : "open"];
    this.calibrationRelaxation = source.map((item) => item.relaxationScore);
    const average = (label: string) => source.reduce((sum, item) => sum + bandValue(item, label), 0) / source.length;
    this.baseThresholds = {
      rewardMin: Object.fromEntries(this.protocol.rewardBands.map((band) => [band.label, Math.max(0.01, average(band.label) * 1.1)])),
      inhibitMax: Object.fromEntries(this.protocol.inhibitBands.map((band) => [band.label, Math.max(0.01, average(band.label) * 0.9)])),
    };
    this.thresholds = this.baseThresholds;
    this.stage = "training";
  }

  private applyScale() {
    if (!this.baseThresholds) return;
    this.thresholds = { ...this.baseThresholds, rewardMin: Object.fromEntries(Object.entries(this.baseThresholds.rewardMin).map(([label, value]) => [label, value * this.scale])) };
  }

  /** Counts a training tick as valid or not (the valid-data percentage, and the gate's confidence). */
  private note(valid: boolean) {
    if (this.stage !== "training") return;
    this.trainingTicks += 1;
    if (valid) this.validTicks += 1;
  }

  private hold(reason: HoldReason, telemetry: MuseTelemetry, now: number, fit: Record<string, ChannelFit>): TrainingState {
    if (this.stage === "training") this.held[reason] += 1;
    this.note(false);
    this.streak = 0;
    this.state = { ...this.state, phase: "held", label: HOLD_COPY[reason], stage: this.stage, hold: reason, fit, signal: null, focus: null, relaxation: null, streak: 0, delayMs: telemetry.lastPacketAt ? now - telemetry.lastPacketAt + FFT_WINDOW_DELAY_MS : null };
    return this.state;
  }

  /** The gyroscope is optional: without a fresh stream, this gate stays out of the way. */
  private moved(telemetry: MuseTelemetry, now: number): boolean {
    return telemetry.gyro != null && now - telemetry.gyro.lastPacketAt < STALE_MS && headMoved(telemetry.gyro.samples);
  }

  /** A pulse reading every few seconds, skipping movement, which swamps the optical signal. */
  private readHeartRate(telemetry: MuseTelemetry, now: number, moved: boolean) {
    if (moved || now - this.lastHeartRateAt < HEART_RATE_EVERY_MS) return;
    if (!telemetry.ppg || now - telemetry.ppg.lastPacketAt >= STALE_MS) return;
    this.lastHeartRateAt = now;
    const bpm = estimateHeartRate(telemetry.ppg.samples);
    if (bpm !== null) this.heartRates.push(bpm);
  }

  tick(telemetry: MuseTelemetry, now: number): TrainingState {
    if (now - this.lastTick < TICK_MS) return this.state;
    this.lastTick = now;
    const moved = this.moved(telemetry, now);
    this.readHeartRate(telemetry, now, moved);
    const fit = this.guided ? Object.fromEntries(CHANNELS.map((channel) => [channel, this.channelFit(telemetry, channel, now)])) as Record<string, ChannelFit> : {};
    if (this.stage === "training") {
      for (const c of CHANNELS) if (fit[c]) this.fitTally[c][fit[c]] += 1;
      this.recentFit.push(CHANNELS.every((c) => fit[c] === "good" || fit[c] === "fair"));
      if (this.recentFit.length > CONFIDENCE_TICKS) this.recentFit.shift();
    }
    if (telemetry.source) this.sources.add(telemetry.source);
    // Decision 0057: only the live headset can earn feedback. Replay, demo and unlabelled data never do.
    if (telemetry.packetCount > 0 && telemetry.source !== "live") {
      if (this.guided) return this.hold("not_live", telemetry, now, fit);
      this.state = { ...this.state, phase: "held", label: HOLD_COPY.not_live, signal: null, focus: null, relaxation: null };
      return this.state;
    }
    // A channel repeating its last packet exactly is frozen: nothing new to train on.
    if (CHANNELS.some((c) => telemetry.continuity?.[c]?.frozen)) {
      if (this.guided) return this.hold("frozen", telemetry, now, fit);
      this.state = { ...this.state, phase: "held", label: HOLD_COPY.frozen, signal: null, focus: null, relaxation: null };
      return this.state;
    }
    if (this.guided && this.stage === "fit") {
      const ready = CHANNELS.every((channel) => fit[channel] === "good" || fit[channel] === "fair");
      this.fitGoodTicks = ready ? this.fitGoodTicks + 1 : 0;
      this.state = { ...this.state, fit, fitReady: this.fitGoodTicks >= FIT_TICKS };
      return this.state;
    }
    if (this.guided) {
      if (Object.values(fit).includes("none")) return this.hold("dropout", telemetry, now, fit);
      if (Object.values(fit).includes("poor")) return this.hold(moved ? "movement" : "contact", telemetry, now, fit);
      if (telemetry.packetCount === this.lastPacket) return this.hold("dropout", telemetry, now, fit);
    }
    const stale = CHANNELS.some((channel) => !telemetry.channelLastPacketAt[channel] || now - telemetry.channelLastPacketAt[channel] > STALE_MS);
    const incomplete = CHANNELS.some((channel) => telemetry.channels[channel]?.length < 256);
    const poor = !this.guided && CHANNELS.some((channel) => {
      const samples = telemetry.channels[channel] ?? [];
      return samples.some((value) => !Number.isFinite(value)) || assessChannel(samples.slice(-256)) !== "good";
    });
    if (stale || incomplete || poor || telemetry.packetCount === this.lastPacket) {
      if (!this.thresholds) this.baseline = [];
      this.state = { ...this.state, phase: "held", label: stale || incomplete ? "Waiting for all electrodes" : "Adjust headset contact", signal: null, focus: null, relaxation: null, calibrationPercent: this.thresholds ? 100 : 0 };
      return this.state;
    }
    this.lastPacket = telemetry.packetCount;
    const windows = Object.fromEntries(CHANNELS.map((channel) => [channel, removeLineNoise(telemetry.channels[channel].slice(-256))]));
    const detected = detectAllArtifacts(windows, this.jawBaseline, now);
    this.artifacts = this.artifacts.filter((time) => now - time < 60_000);
    if (detected.length || moved) {
      if (now - this.lastArtifactAt >= 1000) { this.artifacts.push(now); this.lastArtifactAt = now; }
      if (this.guided) {
        // A blink costs one tick of the baseline, never the whole baseline.
        const type = moved ? "motion" : detected[0].type;
        return this.hold(type === "blink" ? "blink" : type === "jaw_clench" ? "jaw" : "movement", telemetry, now, fit);
      }
      if (!this.thresholds) this.baseline = [];
      this.state = { ...this.state, phase: "held", label: "Movement detected", signal: null, focus: null, relaxation: null, calibrationPercent: this.thresholds ? 100 : 0 };
      return this.state;
    }
    const jawPower = (computeJawPower(windows.AF7) + computeJawPower(windows.AF8)) / 2;
    if (!this.thresholds) this.jawBaseline = this.jawBaseline ? this.jawBaseline * 0.9 + jawPower * 0.1 : jawPower;
    // Preserve the established frontal-electrode metrics without averaging phases.
    // The back-of-head electrode reads alpha better than the forehead, when it's really there.
    const auxLive = this.guided && this.auxAllowed && telemetry.aux != null && now - telemetry.aux.lastPacketAt < STALE_MS && telemetry.aux.samples.length >= 256 && assessChannel(telemetry.aux.samples.slice(-256)) !== "poor";
    const auxSource = auxLive && this.protocol.rewardBands.some((band) => band.label === "Alpha");
    const frontal = (auxSource ? ["AUX"] : ["AF7", "AF8"]).map((channel) => {
      const samples = removeLineNoise(channel === "AUX" ? (telemetry.aux as NonNullable<MuseTelemetry["aux"]>).samples : telemetry.channels[channel]);
      const mean = samples.reduce((sum, value) => sum + value, 0) / samples.length;
      return computeBands(samples.map((value) => value - mean));
    });
    const bands = Object.fromEntries(Object.keys(frontal[0]).map((key) => [key, frontal.reduce((sum, item) => sum + item[key as keyof FrequencyBandPowers], 0) / frontal.length])) as unknown as FrequencyBandPowers;
    if (this.guided && !this.thresholds) {
      const eyes = this.stage === "baseline-closed" ? "closed" : "open";
      this.baselines[eyes].push(bands);
      if (this.baselines[eyes].length >= BASELINE_TICKS) {
        if (eyes === "open") this.stage = "baseline-closed";
        else this.finishBaseline();
      }
      const done = this.stage === "baseline-closed" ? 0 : this.baselines[eyes].length;
      this.state = { ...this.state, phase: this.stage === "training" ? "training" : "calibrating", stage: this.stage, hold: null, fit, auxSource,
        label: this.stage === "baseline-closed" ? "Eyes closed" : this.stage === "training" ? "Training" : "Eyes open",
        calibrationPercent: this.stage === "training" ? 100 : (this.stage === "baseline-closed" ? this.baselines.closed.length : done) / BASELINE_TICKS * 100 };
      return this.state;
    }
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
    // Real gate inputs (decision 0057), not the old fixed "confidence 1, good, not frozen".
    const worst = Object.values(fit).includes("poor") ? "poor" : Object.values(fit).includes("fair") ? "fair" : "good";
    // Confidence: the share of the last 10 s with good contact on every electrode. After a long hold it
    // recovers within a few seconds of good contact (the gate needs 0.3), so feedback never locks out.
    const confidence = this.recentFit.length ? this.recentFit.filter(Boolean).length / this.recentFit.length : 1;
    // Frozen channels already held above; passed again so the engine's own gate agrees (defence in depth).
    const frozen = CHANNELS.some((c) => telemetry.continuity?.[c]?.frozen === true);
    const signal = computeFeedbackSignal(bands, this.protocol, this.thresholds, confidence, worst, this.artifacts.length, frozen);
    if (!signal.gated) this.note(true);
    this.entries.push({ timestamp: now, focus: bands.focusScore, relaxation: bands.relaxationScore, action: signal.action, gated: signal.gated });
    if (this.guided) {
      if (signal.gated) return this.hold("noise", telemetry, now, fit);
      const rewarded = signal.action === "reward";
      this.streak = rewarded ? this.streak + 1 : 0;
      this.window.push(rewarded);
      if (this.window.length >= ADAPT_TICKS) {
        this.scale = adaptScale(this.scale, this.window.filter(Boolean).length / this.window.length);
        this.adjustments.push(this.scale);
        this.applyScale();
        this.window = [];
      }
      const valid = this.entries.filter((entry) => !entry.gated);
      this.state = {
        ...this.state, phase: "training", stage: "training", hold: null, fit, signal, auxSource, streak: this.streak,
        label: rewarded ? "In your target" : signal.action === "inhibit" ? "Ease off a little" : "Keep going",
        focus: bands.focusScore, relaxation: bands.relaxationScore, calibrationPercent: 100,
        calmPercent: calmPercent(this.entries, relaxationBaseline(this.calibrationRelaxation, this.entries)),
        delayMs: telemetry.lastPacketAt ? now - telemetry.lastPacketAt + FFT_WINDOW_DELAY_MS : null,
        targetPercent: valid.length ? valid.filter((entry) => entry.action === "reward").length / valid.length * 100 : null,
      };
      return this.state;
    }
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
      heart_rate: heartRateSummary(this.heartRates),
      ...(this.guided ? { guided: {
        reward_schedule: { target_rate: TARGET_REWARD_RATE, check_every_s: ADAPT_TICKS * TICK_MS / 1000, threshold_scale: this.scale, adjustments: [...this.adjustments] },
        held_seconds: Object.fromEntries(Object.entries(this.held).map(([reason, ticks]) => [reason, ticks * TICK_MS / 1000])) as Record<HoldReason, number>,
        trained_seconds: valid.length * TICK_MS / 1000,
        alpha_eyes_closed_ratio: alphaRatio(this.baselines.open, this.baselines.closed),
        aux_source: !!this.state.auxSource,
        // Decision 0057: how much of the training time was usable, and each electrode's contact over it.
        valid_pct: this.trainingTicks ? Math.round(this.validTicks / this.trainingTicks * 1000) / 10 : null,
        training_ticks: this.trainingTicks,
        channel_fit_ticks: Object.fromEntries(Object.entries(this.fitTally).map(([c, t]) => [c, { ...t }])),
      } } : {}),
      data_sources: [...this.sources].sort(),
      method: TRAINING_METHOD,
    };
  }
}
