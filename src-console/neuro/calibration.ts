/**
 * NEUROSENSE pre-meditation calibration (decision 0058). Separate from the neurofeedback protocol calibration
 * inside TrainingSession: this one records resting eyes-open and eyes-closed EEG, rejects artifact epochs and
 * computes signal features for the personal baseline. It never trains, rewards or interprets.
 *
 * Features are signal measurements (relative band power, alpha peak frequency, variability, channel quality),
 * not measures of emotion, attention, stress or relaxation. Reports carry numbers and counts, never raw EEG.
 */
import { assessChannel } from "../legacy-core/hooks/useSignalQuality";
import { detectAllArtifacts } from "../legacy-core/lib/artifactDetection";
import { removeLineNoise } from "../legacy-core/lib/lineNoise";
import { FFT_SIZE, powerSpectrum, relativeBandPowers, SAMPLE_RATE } from "./fft";
import type { MuseTelemetry } from "./telemetryStore";

export const CALIBRATION_VERSION = "calibration-v1-0058";
export const CHANNELS = ["TP9", "AF7", "AF8", "TP10"] as const;
export const DEFAULT_SECS = 60;
export const MIN_SECS = 20;
export const MAX_SECS = 300;
const STALE_MS = 1500;
const AMPLITUDE_UV = 150;

export const METHOD = {
  version: CALIBRATION_VERSION,
  epochSecs: FFT_SIZE / SAMPLE_RATE,
  fft: "Per channel, non-overlapping 1 s epochs (256 samples at 256 Hz nominal), mean removed, mains hum removed, Hann window; relative power of delta 1-4, theta 4-8, alpha 8-13, beta 13-30, gamma 30-45 Hz averaged over the four channels",
  rejection: "Epoch rejected if any channel is stale, frozen, poor contact, flat, over 150 uV, or a blink, jaw or movement artifact is detected",
};

export type Condition = "eyes_open" | "eyes_closed";
export type RejectReason = "dropout" | "frozen" | "contact" | "flat" | "amplitude" | "blink" | "jaw" | "movement";
export type EpochResult = { accepted: true; bands: Record<string, number>; logPower: number; alphaPeak: number | null } | { accepted: false; reason: RejectReason };

/** Alpha peak: the 7–13 Hz maximum of the averaged spectrum, refined between bins, only when it clearly stands out. */
export function alphaPeak(spectrum: Float64Array): number | null {
  const bin = SAMPLE_RATE / FFT_SIZE;
  const lo = Math.ceil(7 / bin), hi = Math.floor(13 / bin);
  let k = lo;
  for (let i = lo; i <= hi; i++) if (spectrum[i] > spectrum[k]) k = i;
  if (k === lo || k === hi) return null; // a slope, not a peak
  const flank = (spectrum[lo - 1] + spectrum[hi + 1]) / 2;
  if (!(spectrum[k] > 1.5 * flank)) return null; // no clear peak
  const [a, b, c] = [spectrum[k - 1], spectrum[k], spectrum[k + 1]];
  const d = a - 2 * b + c;
  const offset = d === 0 ? 0 : (0.5 * (a - c)) / d;
  const hz = (k + Math.max(-0.5, Math.min(0.5, offset))) * bin;
  return hz >= 7 && hz <= 13 ? Math.round(hz * 100) / 100 : null;
}

/** One 1 s epoch across the four channels: rejected with a reason, or its features. */
export function epoch(windows: Record<string, number[]>, frozen: boolean, now: number): EpochResult {
  if (frozen) return { accepted: false, reason: "frozen" };
  const clean = Object.fromEntries(CHANNELS.map((c) => [c, removeLineNoise(windows[c])]));
  for (const c of CHANNELS) {
    const w = windows[c];
    if (!w || w.length < FFT_SIZE || w.some((v) => !Number.isFinite(v))) return { accepted: false, reason: "dropout" };
    const q = assessChannel(w);
    if (q === "poor") {
      const spread = Math.max(...clean[c]) - Math.min(...clean[c]);
      return { accepted: false, reason: spread < 3 ? "flat" : "contact" };
    }
    const mean = clean[c].reduce((s, v) => s + v, 0) / clean[c].length;
    if (clean[c].some((v) => Math.abs(v - mean) > AMPLITUDE_UV)) return { accepted: false, reason: "amplitude" };
  }
  const artifacts = detectAllArtifacts(clean, 0, now);
  if (artifacts.length) {
    const t = artifacts[0].type;
    return { accepted: false, reason: t === "blink" ? "blink" : t === "jaw_clench" ? "jaw" : "movement" };
  }
  const per = CHANNELS.map((c) => relativeBandPowers(clean[c]));
  const bands = Object.fromEntries(["delta", "theta", "alpha", "beta", "gamma"].map((k) => [k, per.reduce((s, b) => s + b[k as keyof typeof b], 0) / per.length]));
  const spectra = CHANNELS.map((c) => powerSpectrum(clean[c]));
  const avg = new Float64Array(spectra[0].length);
  for (const sp of spectra) for (let i = 0; i < avg.length; i++) avg[i] += sp[i] / spectra.length;
  let total = 0;
  for (let i = 1; i < avg.length && i * (SAMPLE_RATE / FFT_SIZE) < 45; i++) total += avg[i];
  return { accepted: true, bands, logPower: Math.log10(Math.max(total, 1e-12)), alphaPeak: alphaPeak(avg) };
}

const median = (xs: number[]) => { if (!xs.length) return null; const s = [...xs].sort((a, b) => a - b); const n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2; };
const mad = (xs: number[]) => { const m = median(xs); return m == null ? null : median(xs.map((x) => Math.abs(x - m))); };
const round = (v: number | null, d = 2) => (v == null ? null : Math.round(v * 10 ** d) / 10 ** d);

export type ConditionResult = {
  condition: Condition; plannedSecs: number; recordedSecs: number; epochsTotal: number; epochsAccepted: number;
  rejected: Partial<Record<RejectReason, number>>; channelQuality: Record<string, number>;
  features: { relBands: Record<string, number>; alphaPeakHz: number | null; alphaPeakEpochs: number; variability: number | null } | null;
};

/** Summarises one condition's epochs with robust statistics (medians; MAD for variability). */
export function summarise(condition: Condition, plannedSecs: number, epochs: EpochResult[], channelOk: Record<string, { ok: number; total: number }>): ConditionResult {
  const acc = epochs.filter((e): e is Extract<EpochResult, { accepted: true }> => e.accepted);
  const rejected: Partial<Record<RejectReason, number>> = {};
  for (const e of epochs) if (!e.accepted) rejected[e.reason] = (rejected[e.reason] ?? 0) + 1;
  const peaks = acc.map((e) => e.alphaPeak).filter((p): p is number => p != null);
  const features = acc.length ? {
    relBands: Object.fromEntries(["delta", "theta", "alpha", "beta", "gamma"].map((k) => [k, round(median(acc.map((e) => e.bands[k])))!])),
    // A peak needs at least a quarter of the accepted epochs to show one; otherwise it is not estimable.
    alphaPeakHz: peaks.length >= Math.max(3, acc.length / 4) ? round(median(peaks)) : null,
    alphaPeakEpochs: peaks.length,
    variability: acc.length >= 3 ? round(mad(acc.map((e) => e.logPower)), 3) : null,
  } : null;
  return {
    condition, plannedSecs, recordedSecs: epochs.length * METHOD.epochSecs, epochsTotal: epochs.length, epochsAccepted: acc.length, rejected,
    channelQuality: Object.fromEntries(CHANNELS.map((c) => [c, channelOk[c]?.total ? round(channelOk[c].ok / channelOk[c].total, 3)! : 0])), features,
  };
}

export type CalibrationStage = "check" | "eyes_open" | "eyes_closed" | "done";
export type CalibrationState = {
  stage: CalibrationStage;
  /** Why the check can't pass yet, in plain words; null when ready. */
  blocker: string | null;
  channelFit: Record<string, "good" | "fair" | "poor" | "none">;
  secs: number; plannedSecs: number; accepted: number; rejected: number;
  results: ConditionResult[];
};

/** Drives one pre-meditation calibration from telemetry, once a second. The person can stop at any time. */
export class CalibrationRun {
  private stage: CalibrationStage = "check";
  private epochs: EpochResult[] = [];
  private channelOk: Record<string, { ok: number; total: number }> = {};
  private lastSamples = -1;
  private results: ConditionResult[] = [];
  private fit: CalibrationState["channelFit"] = {};
  private blocker: string | null = "Waiting for the headset";
  private source: string | null = null;

  constructor(readonly plannedSecs = DEFAULT_SECS) {
    if (!(plannedSecs >= MIN_SECS && plannedSecs <= MAX_SECS)) throw new Error(`duration must be ${MIN_SECS}-${MAX_SECS} s`);
  }

  get state(): CalibrationState {
    const acc = this.epochs.filter((e) => e.accepted).length;
    return { stage: this.stage, blocker: this.blocker, channelFit: { ...this.fit }, secs: this.epochs.length * METHOD.epochSecs, plannedSecs: this.plannedSecs, accepted: acc, rejected: this.epochs.length - acc, results: [...this.results] };
  }

  /** Connection and channel check; start recording only once it passes. */
  private check(t: MuseTelemetry, now: number) {
    this.fit = Object.fromEntries(CHANNELS.map((c) => {
      const at = t.channelLastPacketAt[c]; const s = t.channels[c] ?? [];
      if (!at || now - at > STALE_MS || s.length < FFT_SIZE) return [c, "none"];
      if (t.continuity?.[c]?.frozen) return [c, "poor"];
      return [c, assessChannel(s.slice(-FFT_SIZE))];
    })) as CalibrationState["channelFit"];
    this.source = t.source ?? null;
    this.blocker = t.packetCount === 0 ? "Connect the Muse and start streaming"
      : t.source !== "live" ? "This is a replay or demo, not the live headset"
      : Object.values(this.fit).includes("none") ? "An electrode has no signal"
      : Object.values(this.fit).includes("poor") ? "Adjust the headband until every sensor shows good or fair contact"
      : null;
  }

  /** Begin the eyes-open recording (the check must pass). */
  begin(): boolean {
    if (this.stage !== "check" || this.blocker) return false;
    this.stage = "eyes_open";
    this.lastSamples = -1;
    return true;
  }

  tick(t: MuseTelemetry, now: number): CalibrationState {
    if (this.stage === "check") { this.check(t, now); return this.state; }
    if (this.stage === "done") return this.state;
    // One non-overlapping epoch per 256 new samples on every channel: never the same data twice.
    const fresh = CHANNELS.every((c) => t.channelLastPacketAt[c] && now - t.channelLastPacketAt[c] <= STALE_MS);
    if (this.lastSamples >= 0 && t.sampleCount - this.lastSamples < FFT_SIZE * CHANNELS.length && fresh) return this.state;
    this.lastSamples = t.sampleCount;
    const windows = Object.fromEntries(CHANNELS.map((c) => [c, (t.channels[c] ?? []).slice(-FFT_SIZE)]));
    const frozen = CHANNELS.some((c) => t.continuity?.[c]?.frozen);
    const result: EpochResult = !fresh ? { accepted: false, reason: "dropout" } : t.source !== "live" ? { accepted: false, reason: "dropout" } : epoch(windows, !!frozen, now);
    this.epochs.push(result);
    for (const c of CHANNELS) {
      const k = (this.channelOk[c] ??= { ok: 0, total: 0 });
      k.total += 1;
      const w = windows[c];
      if (fresh && w.length >= FFT_SIZE && assessChannel(w) !== "poor") k.ok += 1;
    }
    if (this.epochs.length * METHOD.epochSecs >= this.plannedSecs) this.finishCondition();
    return this.state;
  }

  private finishCondition() {
    const cond = this.stage as Condition;
    this.results.push(summarise(cond, this.plannedSecs, this.epochs, this.channelOk));
    this.epochs = []; this.channelOk = {}; this.lastSamples = -1;
    this.stage = cond === "eyes_open" ? "eyes_closed" : "done";
  }

  /** Stop early: whatever was recorded is kept and summarised. */
  stop() {
    if (this.stage === "eyes_open" || this.stage === "eyes_closed") {
      if (this.epochs.length) this.finishCondition();
      this.stage = "done";
    }
  }

  /** The report for `neuro_calibration_save`: features and counts only. */
  report(kind: "pre_meditation" | "post_session" = "pre_meditation") {
    // begin() only passes with live data; anything else is labelled as such, never assumed live.
    const source = this.source === "live" || this.source === "demo" ? this.source : "replay";
    // Decision 0059: the same recording after a session is `post_session`, kept out of the baseline.
    return { kind, source, method: METHOD, conditions: this.results, protocol: null, sessionId: null };
  }
}

/** Plain-words labels for features (signal terms only, never mood or state). */
export const FEATURE_LABEL: Record<string, string> = {
  delta: "Delta power (1–4 Hz)", theta: "Theta power (4–8 Hz)", alpha: "Alpha power (8–13 Hz)", beta: "Beta power (13–30 Hz)", gamma: "Gamma power (30–45 Hz)",
  alpha_peak_hz: "Alpha peak frequency", variability: "Signal variability",
};
export const REJECT_LABEL: Record<RejectReason, string> = { dropout: "signal gap", frozen: "frozen signal", contact: "poor contact", flat: "flat signal", amplitude: "too large", blink: "blink", jaw: "jaw", movement: "movement" };
