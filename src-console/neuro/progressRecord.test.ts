import { beforeEach, describe, expect, it } from "vitest";
import { evaluateGate } from "../legacy-core/lib/neurofeedback/safetyGate";
import { computeFeedbackSignal, resetOutputEngine } from "../legacy-core/lib/neurofeedback/outputEngine";
import { PROTOCOLS } from "../legacy-core/lib/neurofeedback/protocols";
import type { FrequencyBandPowers } from "../legacy-core/lib/types";
import type { MuseTelemetry } from "./telemetryStore";
import { PROTOCOL_SET_VERSION, defaultSettings } from "./protocolConfig";
import { ADAPT_TICKS, AUX_PROVEN, BASELINE_TICKS, GATE_DEFAULTS, PROGRESS_RECORD_VERSION, TrainingSession } from "./training";

const calm = PROTOCOLS.find((p) => p.id === "calm")!;
const hypnagogic = PROTOCOLS.find((p) => p.id === "hypnagogic")!;
const CHANNELS = ["TP9", "AF7", "AF8", "TP10"];

function wave(alpha: number, phase: number, n = 256) {
  let seed = phase * 7 + 3;
  const noise = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280 - .5; };
  return Array.from({ length: n }, (_, i) => {
    const t = (i + phase * 128) / 256;
    return alpha * Math.sin(2 * Math.PI * 10 * t) + 4 * Math.sin(2 * Math.PI * 22 * t) + 3 * Math.sin(2 * Math.PI * 6 * t) + noise() * 3;
  });
}

let packet = 0;
function telemetry(now: number, alpha: number, patch: Partial<MuseTelemetry> = {}): MuseTelemetry {
  packet += 1;
  return { channels: Object.fromEntries(CHANNELS.map((c) => [c, wave(alpha, packet)])), bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: packet, sampleCount: packet * 12, lastPacketAt: now, channelLastPacketAt: Object.fromEntries(CHANNELS.map((c) => [c, now])), aux: null, source: "live", ...patch };
}

function trained(protocol = calm, settings = defaultSettings(calm)) {
  const session = new TrainingSession(protocol, { guided: true, settings });
  let now = 1_000_000;
  const tick = (alpha: number, patch?: Partial<MuseTelemetry>) => { now += 500; return session.tick(telemetry(now, alpha, patch), now); };
  for (let i = 0; i < 6; i++) tick(20);
  session.advance();
  for (let i = 0; i < BASELINE_TICKS; i++) tick(12);
  for (let i = 0; i < BASELINE_TICKS; i++) tick(24);
  return { session, tick };
}

beforeEach(() => { packet = 0; resetOutputEngine(); });

describe("decision 0059: session progress record", () => {
  it("is absent until training starts", () => {
    const session = new TrainingSession(calm, { guided: true, settings: defaultSettings(calm) });
    expect(session.progressRecord()).toBeNull();
    expect(new TrainingSession(calm).progressRecord()).toBeNull();
  });

  it("names the protocol set, features, settings, gate and method", () => {
    const { session, tick } = trained();
    for (let i = 0; i < 10; i++) tick(30);
    const r = session.summary().progress_record!;
    expect(r.version).toBe(PROGRESS_RECORD_VERSION);
    expect(r.protocol_set).toBe(PROTOCOL_SET_VERSION);
    expect(r.protocol_id).toBe("calm");
    expect(r.target_features.map((f) => f.id)).toEqual(["reward:alpha", "inhibit:highbeta"]);
    expect(r.settings).toEqual(defaultSettings(calm));
    expect(r.gate).toEqual({ min_confidence: 0.3, max_artifact_rate_per_min: 15, live_only: true, frozen_holds: true });
    expect(r.method).toMatch(/^training-v/);
  });

  it("logs the baseline thresholds, then every 30 s check with its reason", () => {
    const { session, tick } = trained();
    for (let i = 0; i < ADAPT_TICKS * 2; i++) tick(40); // strong alpha: rewarded most of the time
    const log = session.progressRecord()!.threshold_log;
    expect(log[0]).toMatchObject({ at_s: 0, event: "baseline", scale_before: 1, scale_after: 1 });
    expect(log[0].reason).toMatch(/eyes-open baseline \(\d+ clean half-second ticks\)/);
    expect(Object.keys(log[0].reward_min)).toEqual(["Alpha"]);
    const checks = log.filter((e) => e.event === "adapt");
    expect(checks.length).toBe(2);
    for (const c of checks) {
      expect(c.window_reward_rate).toBeGreaterThanOrEqual(0);
      expect(c.reason).toMatch(/of the last 30 s of clean ticks/);
      if (c.scale_after > c.scale_before) expect(c.reason).toMatch(/raised/);
      expect(c.reward_min.Alpha).toBeCloseTo(log[0].reward_min.Alpha * c.scale_after, 6);
    }
    expect(checks[1].at_s).toBeGreaterThan(checks[0].at_s);
  });

  it("time in target counts valid data only, and held time is kept by reason", () => {
    const { session, tick } = trained();
    for (let i = 0; i < 10; i++) tick(30);
    for (let i = 0; i < 4; i++) tick(30, { source: "replay" });
    const r = session.progressRecord()!;
    expect(r.held_seconds.not_live).toBe(2);
    expect(r.trained_seconds).toBeLessThanOrEqual(5);
    expect(r.valid_pct).toBeLessThan(100);
    expect(r.time_in_target_pct === null || (r.time_in_target_pct >= 0 && r.time_in_target_pct <= 100)).toBe(true);
  });

  it("the record holds no raw EEG", () => {
    const { session, tick } = trained();
    for (let i = 0; i < 4; i++) tick(30);
    const json = JSON.stringify(session.progressRecord());
    expect(json.length).toBeLessThan(6000);
    expect(json).not.toMatch(/"channels"|"samples"/);
  });

  it("a protocol's stricter gate is what the record reports", () => {
    const { session, tick } = trained(hypnagogic, defaultSettings(hypnagogic));
    for (let i = 0; i < 4; i++) tick(30);
    expect(session.progressRecord()!.gate).toMatchObject({ min_confidence: 0.4, max_artifact_rate_per_min: 8 });
  });
});

describe("decision 0059: existing safety gates are unchanged (pinned)", () => {
  const bands = { delta: 10, theta: 10, alpha: 60, beta: 15, gamma: 5, smr: 5, lowBeta: 5, highBeta: 5, alphaPeak: 10, focusScore: 50, relaxationScore: 80, alphaTheta: 6, betaAlpha: .25 } as FrequencyBandPowers;
  const thresholds = { rewardMin: { Alpha: 20 }, inhibitMax: { "High Beta": 50 } };

  it("the gate's order and limits", () => {
    expect(GATE_DEFAULTS).toEqual({ minConfidence: 0.3, maxArtifactRate: 15 });
    expect(evaluateGate(0.29, "good", 0, false)).toMatchObject({ allowed: false, reason: "Low confidence" });
    expect(evaluateGate(1, "good", 0, true)).toMatchObject({ allowed: false, holdState: true, reason: "Signal frozen" });
    expect(evaluateGate(1, "frozen", 0, false)).toMatchObject({ allowed: false, holdState: true });
    expect(evaluateGate(1, "good", 15, false)).toMatchObject({ allowed: false, reason: "Artifact burst" });
    expect(evaluateGate(1, "poor", 0, false)).toMatchObject({ allowed: false, reason: "Poor signal quality" });
    expect(evaluateGate(0.3, "fair", 14, false)).toMatchObject({ allowed: true });
  });

  it("a gated signal never rewards, and a frozen hold does not repeat an earlier reward", () => {
    const ok = computeFeedbackSignal(bands, calm, thresholds, 1, "good", 0, false);
    expect(ok.action).toBe("reward");
    for (const [conf, q, rate, frozen] of [[0.1, "good", 0, false], [1, "poor", 0, false], [1, "good", 20, false], [1, "good", 0, true]] as const) {
      const s = computeFeedbackSignal(bands, calm, thresholds, conf, q, rate, frozen);
      expect(s.gated).toBe(true);
      expect(s.action).toBe("neutral");
      expect(s.intensity).toBe(0);
    }
  });

  it("the electrode stays locked", () => {
    expect(AUX_PROVEN).toBe(false);
  });
});

describe("decision 0059: session records card", () => {
  it("shows a record's numbers, why it is not used for trends, and never a 0 for a missing value", async () => {
    const { render, screen } = await import("@testing-library/react");
    const { createElement } = await import("react");
    const { TrainingRecordsCard } = await import("./TrainingView");
    const rec = { sessionId: "s1", start: "2026-11-20T10:00:00Z", durationSecs: 600, protocolId: "calm", protocolSet: "protocols-v1-0059", recordVersion: "progress-v1-0059", method: "training-v2-0057",
      source: "replay", validPct: null, timeInTargetPct: 58, trainedSeconds: 540, targetRewardRate: 0.6, thresholdChecks: 18, finalThresholdScale: 1.08,
      baselineLevels: { Alpha: 30 }, heldSeconds: { blink: 4, frozen: 0 }, eligible: false, notEligibleReason: "not recorded from the live headset" };
    render(createElement(TrainingRecordsCard, { protocols: PROTOCOLS, load: async () => [rec] }));
    const list = await screen.findByRole("list", { name: "Session records" });
    expect(list).toHaveTextContent("Calm & Relax");
    expect(list).toHaveTextContent("Usable Not measured of training");
    expect(list).toHaveTextContent("Alpha 30.0%");
    expect(list).toHaveTextContent("x1.08 of the baseline threshold");
    expect(list).toHaveTextContent("Held: blinks 4 s");
    expect(list).not.toHaveTextContent("frozen signal 0");
    expect(list).toHaveTextContent("Not used for trends: not recorded from the live headset.");
  });

  it("says so plainly when there are no records", async () => {
    const { render, screen } = await import("@testing-library/react");
    const { createElement } = await import("react");
    const { TrainingRecordsCard } = await import("./TrainingView");
    render(createElement(TrainingRecordsCard, { protocols: PROTOCOLS, load: async () => [] }));
    expect(await screen.findByText(/No records yet/)).toBeInTheDocument();
  });
});
