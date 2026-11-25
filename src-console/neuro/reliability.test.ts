import { beforeEach, describe, expect, it } from "vitest";
import { computeFeedbackSignal, resetOutputEngine } from "../legacy-core/lib/neurofeedback/outputEngine";
import { resetPolicyState } from "../legacy-core/lib/neurofeedback/policyEngine";
import { PROTOCOLS } from "../legacy-core/lib/neurofeedback/protocols";
import type { FrequencyBandPowers } from "../legacy-core/lib/types";
import type { EegSample } from "../types";
import { createBandHistory } from "./bandHistory";
import { createMuseTelemetryStore, FROZEN_REPEATS, type MuseTelemetry } from "./telemetryStore";
import { BASELINE_TICKS, TRAINING_METHOD, TrainingSession } from "./training";

/** Decision 0057 regression tests: data-source labelling, frozen and missing data, and the reward gate. */

const calm = PROTOCOLS.find((p) => p.id === "calm")!;
const CHANNELS = ["TP9", "AF7", "AF8", "TP10"];
const TICK = 500;

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
  return {
    channels: Object.fromEntries(CHANNELS.map((c) => [c, wave(alpha, packet)])), bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 },
    packetCount: packet, sampleCount: packet * 12, lastPacketAt: now, channelLastPacketAt: Object.fromEntries(CHANNELS.map((c) => [c, now])),
    aux: null, source: "live", ...patch,
  };
}
function trained(patch: () => Partial<MuseTelemetry> = () => ({})) {
  const session = new TrainingSession(calm, { guided: true });
  let now = 1_000_000;
  const tick = (alpha: number, p: Partial<MuseTelemetry> = patch()) => { now += TICK; return session.tick(telemetry(now, alpha, p), now); };
  for (let i = 0; i < 6; i++) tick(20, {});
  session.advance();
  for (let i = 0; i < BASELINE_TICKS; i++) tick(12, {});
  for (let i = 0; i < BASELINE_TICKS; i++) tick(24, {});
  return { session, tick };
}
beforeEach(() => { packet = 0; });

const pkt = (channel: string, samples: number[], extra: Partial<EegSample> = {}): EegSample => ({ timestamp: 1, channel, samples, source: "live", ...extra });
const ramp = (k: number) => Array.from({ length: 12 }, (_, i) => k * 12 + i);

describe("telemetry: source, completeness, continuity", () => {
  it("labels the source of the data and resets it", () => {
    const store = createMuseTelemetryStore();
    store.ingest(pkt("TP9", ramp(0), { source: "replay" }));
    expect(store.readCounters().source).toBe("replay");
    store.ingest(pkt("TP9", ramp(1), { source: undefined }));
    expect(store.readCounters().source).toBeNull();
    store.reset();
    expect(store.readCounters().source).toBeNull();
    expect(store.getSnapshot().source).toBeNull();
  });

  it("a change of source never mixes samples in one buffer", () => {
    const store = createMuseTelemetryStore();
    store.ingest(pkt("TP9", ramp(0), { source: "replay" }));
    store.ingest(pkt("TP9", ramp(1), { source: "live" }));
    expect(store.readCounters().channelSamples.TP9).toBe(12);
    expect(store.readCounters().source).toBe("live");
  });

  it("counts packets the headset counter says never arrived, across the wrap", () => {
    const store = createMuseTelemetryStore();
    store.ingest(pkt("TP9", ramp(0), { sequence: 65534 }));
    store.ingest(pkt("TP9", ramp(1), { sequence: 1 }));
    store.ingest(pkt("TP9", ramp(2), { sequence: 2 }));
    expect(store.readCounters().continuity.TP9).toMatchObject({ packets: 3, missedPackets: 2, frozen: false });
  });

  it("calls a channel frozen when the same packet values repeat, and clears it when they change", () => {
    const store = createMuseTelemetryStore();
    const same = ramp(5);
    for (let i = 0; i <= FROZEN_REPEATS; i++) store.ingest(pkt("AF7", same));
    expect(store.readCounters().continuity.AF7.frozen).toBe(true);
    store.ingest(pkt("AF7", ramp(6)));
    expect(store.readCounters().continuity.AF7.frozen).toBe(false);
    expect(store.readCounters().continuity.AF7.repeatedPackets).toBe(FROZEN_REPEATS);
  });
});

describe("bands are a reading only with every channel", () => {
  it("marks bands invalid while a channel is incomplete, and the history skips them", () => {
    let snap: MuseTelemetry = { channels: {}, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} };
    const listeners = new Set<() => void>();
    const source = { getSnapshot: () => snap, subscribe: (l: () => void) => { listeners.add(l); return () => listeners.delete(l); } };
    const history = createBandHistory(source as never, 60_000, () => 1250);
    const stop = history.subscribe(() => {});
    snap = { ...snap, packetCount: 30, sampleCount: 400, lastPacketAt: 1000, bandsValid: false };
    listeners.forEach((l) => l());
    expect(history.getSnapshot().alpha.length).toBe(0);
    snap = { ...snap, packetCount: 31, sampleCount: 412, lastPacketAt: 1250, bandsValid: true, bands: { delta: 10, theta: 20, alpha: 40, beta: 20, gamma: 10 } };
    listeners.forEach((l) => l());
    expect(history.getSnapshot().alpha).toEqual([40]);
    stop();
  });
});

describe("feedback gate", () => {
  it("replay, demo and unlabelled data never earn feedback", () => {
    for (const source of ["replay", "demo", undefined] as const) {
      packet = 0;
      const { tick } = trained(() => ({ source }));
      const state = tick(40);
      expect(state.hold).toBe("not_live");
      expect(state.signal).toBeNull();
      expect(state.label).toMatch(/replay or demo/);
    }
  });

  it("a frozen channel holds feedback however good the values look", () => {
    const { session, tick } = trained(() => ({ continuity: { AF7: { packets: 9, missedPackets: 0, repeatedPackets: 3, frozen: true } } }));
    const state = tick(40);
    expect(state.hold).toBe("frozen");
    expect(state.signal).toBeNull();
    expect(session.summary().guided?.held_seconds.frozen).toBeGreaterThan(0);
  });

  it("a held (frozen) gate never repeats the last reward", () => {
    resetOutputEngine(); resetPolicyState();
    const bands = { delta: 5, theta: 5, alpha: 60, beta: 5, gamma: 5, smr: 5, lowBeta: 5, highBeta: 5, focusScore: 50, relaxationScore: 90, alphaPeak: 10 } as unknown as FrequencyBandPowers;
    const thresholds = { rewardMin: { Alpha: 1 }, inhibitMax: {} };
    for (let i = 0; i < 12; i++) computeFeedbackSignal(bands, calm, thresholds, 1, "good", 0, false);
    const held = computeFeedbackSignal(bands, calm, thresholds, 1, "good", 0, true);
    expect(held.gated).toBe(true);
    expect(held.action).toBe("neutral");
    expect(held.intensity).toBe(0);
  });

  it("after a long contact loss, feedback comes back within a few seconds of good contact", () => {
    const { session, tick } = trained();
    for (let i = 0; i < 25; i++) tick(30, { channelLastPacketAt: {} });
    expect(session.state.hold).toBe("dropout");
    let back = false;
    for (let i = 0; i < 12 && !back; i++) back = tick(30, {}).phase === "training";
    expect(back).toBe(true);
  });

  it("the summary says how much was usable, per electrode, from which source, and how it was computed", () => {
    const { session, tick } = trained();
    for (let i = 0; i < 6; i++) tick(30, {});
    for (let i = 0; i < 2; i++) tick(30, { channelLastPacketAt: {} });
    const s = session.summary();
    expect(s.guided?.training_ticks).toBe(8);
    expect(s.guided?.valid_pct).toBeLessThan(100);
    expect(s.guided?.valid_pct).toBeGreaterThan(0);
    expect(s.guided?.channel_fit_ticks.AF7.none).toBe(2);
    expect(s.data_sources).toEqual(["live"]);
    expect(s.method).toBe(TRAINING_METHOD);
    expect(TRAINING_METHOD.version).toMatch(/0057/);
  });

  it("a session with no training time reports no valid percentage rather than 0 or 100", () => {
    const s = new TrainingSession(calm, { guided: true }).summary();
    expect(s.guided?.valid_pct).toBeNull();
  });
});
