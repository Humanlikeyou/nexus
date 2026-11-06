import { beforeEach, describe, expect, it } from "vitest";
import { isOutputAllowed, deriveElectrodeMode } from "../../src/lib/electrode/electrodePolicy";
import { PROTOCOLS } from "../../src/lib/neurofeedback/protocols";
import type { MuseTelemetry } from "./telemetryStore";
import { createMuseTelemetryStore } from "./telemetryStore";
import { ADAPT_TICKS, AUX_PROVEN, BASELINE_TICKS, HOLD_COPY, TARGET_REWARD_RATE, TrainingSession, adaptScale, alphaRatio, feedbackLevel } from "./training";
import { GOALS, parseNotes, recommendedGoal } from "./goals";
import { PPG_BUFFER, PPG_RATE } from "./heartRate";

const calm = PROTOCOLS.find((p) => p.id === "calm")!;
const deep = PROTOCOLS.find((p) => p.id === "deep-relaxation")!;
const CHANNELS = ["TP9", "AF7", "AF8", "TP10"];
const TICK = 500;

/** One second of clean-looking EEG: alpha at the given size over a little beta and noise. */
function wave(alpha: number, phase: number, n = 256) {
  let seed = phase * 7 + 3;
  const noise = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280 - .5; };
  return Array.from({ length: n }, (_, i) => {
    const t = (i + phase * 128) / 256;
    return alpha * Math.sin(2 * Math.PI * 10 * t) + 4 * Math.sin(2 * Math.PI * 22 * t) + 3 * Math.sin(2 * Math.PI * 6 * t) + noise() * 3;
  });
}

let packet = 0;
function telemetry(now: number, alpha: number, patch: Partial<MuseTelemetry> & { only?: Record<string, number[]> } = {}): MuseTelemetry {
  packet += 1;
  const channels = Object.fromEntries(CHANNELS.map((c) => [c, patch.only?.[c] ?? wave(alpha, packet)]));
  return { channels, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: packet, sampleCount: packet * 12, lastPacketAt: now, channelLastPacketAt: Object.fromEntries(CHANNELS.map((c) => [c, now])), aux: null, ...patch };
}

/** Runs a session to the given point and returns it with the clock. */
function run(protocol = calm, options: { aux?: boolean } = {}) {
  const session = new TrainingSession(protocol, { guided: true, ...options });
  let now = 1_000_000;
  const tick = (alpha: number, patch?: Parameters<typeof telemetry>[2]) => { now += TICK; return session.tick(telemetry(now, alpha, patch), now); };
  const fit = () => { for (let i = 0; i < 6; i++) tick(20); session.advance(); };
  const baseline = (open = 12, closed = 24) => { for (let i = 0; i < BASELINE_TICKS; i++) tick(open); for (let i = 0; i < BASELINE_TICKS; i++) tick(closed); };
  return { session, tick, fit, baseline, at: () => now };
}

beforeEach(() => { packet = 0; });

describe("session flow", () => {
  it("won't start a baseline until every sensor has contact", () => {
    const { session, tick } = run();
    tick(20, { channelLastPacketAt: { TP9: 1_000_500 } });
    expect(session.state.stage).toBe("fit");
    expect(session.state.fit).toMatchObject({ TP9: "good", AF7: "none" });
    expect(session.state.fitReady).toBe(false);
    session.advance();
    expect(session.state.stage).toBe("fit");
  });
  it("moves from fit through eyes open and eyes closed into training", () => {
    const { session, tick, fit } = run();
    fit();
    expect(session.state.stage).toBe("baseline-open");
    for (let i = 0; i < BASELINE_TICKS; i++) tick(12);
    expect(session.state.stage).toBe("baseline-closed");
    for (let i = 0; i < BASELINE_TICKS; i++) tick(24);
    expect(session.state.stage).toBe("training");
  });
  it("sets thresholds from the baseline that matches the session: an eyes-closed goal reads the eyes-closed one", () => {
    const open = run(calm), closed = run(deep);
    for (const item of [open, closed]) { item.fit(); item.baseline(12, 24); }
    // At the eyes-closed level, the eyes-open session is well above its target and the eyes-closed one isn't.
    let openRewards = 0, closedRewards = 0;
    for (let i = 0; i < 8; i++) { if (open.tick(24).signal?.action === "reward") openRewards += 1; if (closed.tick(24).signal?.action === "reward") closedRewards += 1; }
    expect(openRewards).toBeGreaterThan(closedRewards);
  });
  it("measures eyes-closed alpha against eyes-open", () => {
    const item = run();
    item.fit(); item.baseline(10, 25); item.tick(25);
    expect(item.session.summary().guided!.alpha_eyes_closed_ratio).toBeGreaterThan(1.2);
    expect(alphaRatio([], [])).toBeNull();
  });
});

describe("feedback is held, never faked", () => {
  const trained = () => { const item = run(); item.fit(); item.baseline(); return item; };
  const blink = () => { const samples = wave(14, 1); for (let i = 100; i < 140; i++) samples[i] += 400 * Math.sin(Math.PI * (i - 100) / 40); return { only: { AF7: samples, AF8: samples } }; };

  it("a blink holds the feedback and says why", () => {
    const { tick } = trained();
    const state = tick(40, blink());
    expect(state.phase).toBe("held");
    expect(state.hold).toBe("blink");
    expect(feedbackLevel(state)).toBe(0);
  });
  it("jaw tension holds it", () => {
    const { tick } = trained();
    const jaw = Array.from({ length: 256 }, (_, i) => 90 * Math.sin(2 * Math.PI * 32 * i / 256) + wave(10, 2)[i]);
    const state = tick(10, { only: { AF7: jaw, AF8: jaw } });
    expect(state.phase).toBe("held");
    expect(["jaw", "movement", "contact"]).toContain(state.hold);
  });
  it("a dropped sensor holds it", () => {
    const { tick, at } = trained();
    const state = tick(30, { channelLastPacketAt: { TP9: at() + TICK, AF7: at() + TICK, AF8: at() + TICK, TP10: at() - 5000 } });
    expect(state.hold).toBe("dropout");
    expect(state.fit?.TP10).toBe("none");
  });
  it("a stalled headset holds it, however good the last samples were", () => {
    const { session, at } = trained();
    const frozen = telemetry(at(), 40);
    session.tick(frozen, at() + TICK);
    expect(session.tick(frozen, at() + 2 * TICK).hold).toBe("dropout");
  });
  it("no headset at all never leaves the fit step", () => {
    const session = new TrainingSession(calm, { guided: true });
    const empty: MuseTelemetry = { channels: {}, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} };
    for (let i = 0; i < 10; i++) session.tick(empty, 2_000_000 + i * TICK);
    expect(session.state.stage).toBe("fit");
    expect(session.state.fitReady).toBe(false);
    expect(feedbackLevel(session.state)).toBe(0);
  });
  it("held time isn't counted as training, and is reported by cause", () => {
    const item = trained();
    item.tick(30); item.tick(30);
    const before = item.session.summary();
    item.tick(40, blink());
    const after = item.session.summary();
    expect(after.guided!.trained_seconds).toBe(before.guided!.trained_seconds);
    expect(after.guided!.held_seconds.blink).toBe(.5);
  });
  it("a blink during baseline costs one tick, not the baseline", () => {
    const { session, tick, fit } = run();
    fit();
    for (let i = 0; i < 10; i++) tick(12);
    const before = session.state.calibrationPercent;
    tick(12, blink());
    expect(session.state.calibrationPercent).toBe(before);
    tick(12);
    expect(session.state.calibrationPercent).toBeGreaterThan(before);
  });
  it("every hold reason has plain copy", () => {
    for (const line of Object.values(HOLD_COPY)) expect(line).toMatch(/^Holding: /);
  });
});

describe("the lens and the reinforcement schedule", () => {
  const base = { phase: "training" as const, stage: "training" as const };
  const signal = (action: "reward" | "neutral" | "inhibit") => ({ action, intensity: .5, gated: false });
  it("is dark unless he's really training", () => {
    expect(feedbackLevel({ ...base, phase: "held", signal: signal("reward"), streak: 20 })).toBe(0);
    expect(feedbackLevel({ ...base, stage: "baseline-open", signal: signal("reward"), streak: 20 })).toBe(0);
    expect(feedbackLevel({ ...base, signal: null })).toBe(0);
  });
  it("lifts in the target and fills as he stays there", () => {
    const start = feedbackLevel({ ...base, signal: signal("reward"), streak: 1 });
    const later = feedbackLevel({ ...base, signal: signal("reward"), streak: 12 });
    expect(start).toBeGreaterThan(feedbackLevel({ ...base, signal: signal("neutral"), streak: 0 }));
    expect(later).toBeGreaterThan(start);
    expect(feedbackLevel({ ...base, signal: signal("reward"), streak: 200 })).toBe(1);
  });
  it("adjusts the threshold toward the target rate, within limits", () => {
    expect(adaptScale(1, .95)).toBeGreaterThan(1);
    expect(adaptScale(1, .2)).toBeLessThan(1);
    expect(adaptScale(1, TARGET_REWARD_RATE)).toBe(1);
    let scale = 1; for (let i = 0; i < 100; i++) scale = adaptScale(scale, 1);
    expect(scale).toBe(1.5);
  });
  it("raises the bar during a session that's too easy, and records it", () => {
    const item = run();
    item.fit(); item.baseline(10, 10);
    for (let i = 0; i < ADAPT_TICKS + 1; i++) item.tick(40);
    const schedule = item.session.summary().guided!.reward_schedule;
    expect(schedule.adjustments.length).toBeGreaterThan(0);
    expect(schedule.threshold_scale).toBeGreaterThan(1);
  });
});

describe("Myndlift electrode", () => {
  it("output stays locked off, whatever the state", () => {
    expect(isOutputAllowed()).toBe(false);
    expect(deriveElectrodeMode(true, true, "live", 1)).toBe("read");
    expect(deriveElectrodeMode(true, true, "demo", 1)).toBe("off");
  });
  it("stays out of training until AUX is proven, even with samples flowing", () => {
    const item = run();
    item.fit(); item.baseline();
    expect(AUX_PROVEN).toBe(false);
    expect(item.tick(20, { aux: { samples: wave(30, 3), lastPacketAt: item.at() + TICK } }).auxSource).toBe(false);
  });
  it("is only ever the source with real AUX samples, and only for alpha goals", () => {
    const item = run(calm, { aux: true });
    item.fit(); item.baseline();
    expect(item.tick(20).auxSource).toBe(false);
    const aux = { samples: wave(30, 3), lastPacketAt: item.at() + TICK };
    expect(item.tick(20, { aux }).auxSource).toBe(true);
    const focus = run(PROTOCOLS.find((p) => p.id === "focus"), { aux: true });
    focus.fit(); focus.baseline();
    expect(focus.tick(20, { aux: { samples: wave(30, 3), lastPacketAt: focus.at() + TICK } }).auxSource).toBe(false);
  });
  it("a stale AUX falls back to the Muse's own sensors", () => {
    const item = run(calm, { aux: true });
    item.fit(); item.baseline();
    expect(item.tick(20, { aux: { samples: wave(30, 3), lastPacketAt: item.at() - 9000 } }).auxSource).toBe(false);
  });
  it("the store only reports AUX after a real packet, and never mixes it into the four channels", () => {
    const store = createMuseTelemetryStore();
    store.ingest({ timestamp: 0, channel: "TP9", samples: [1, 2] });
    expect(store.getSnapshot().aux).toBeNull();
    store.ingest({ timestamp: 0, channel: "AUX", samples: [3, 4] });
    expect(store.readCounters().channelSamples).not.toHaveProperty("AUX");
  });
  it("recommends deep, eyes-closed alpha work only when the electrode is live", () => {
    expect(recommendedGoal(false)).toBeNull();
    expect(GOALS.find((goal) => goal.id === recommendedGoal(true))?.alpha).toBe(true);
  });
});

describe("goals and notes", () => {
  it("every protocol has a home", () => {
    const grouped = GOALS.flatMap((goal) => goal.protocolIds).sort();
    expect(grouped).toEqual(PROTOCOLS.map((p) => p.id).sort());
    expect(grouped).toHaveLength(8);
  });
  it("damaged notes are ignored", () => {
    expect(parseNotes("not json")).toEqual([]);
    expect(parseNotes(null)).toEqual([]);
    expect(parseNotes('[{"at":"x","note":"ok"},{"bad":1}]')).toHaveLength(1);
  });
  it("no protocol copy promises treatment", () => {
    for (const p of PROTOCOLS) expect(`${p.description} ${p.sessionPrompt ?? ""} ${(p.coachingCues ?? []).join(" ")}`).not.toMatch(/\b(cure|treat|therapy|diagnos|disorder|ADHD|anxiety|depression)\w*/i);
  });
});

describe("real-world signals", () => {
  const still = (now: number) => ({ samples: Array.from({ length: 260 }, () => [-6, 5, -3]), lastPacketAt: now });
  const turning = (now: number) => ({ samples: [...still(now).samples.slice(8), ...Array.from({ length: 8 }, () => [30, 5, -3])], lastPacketAt: now });

  it("mains hum on the ear sensors alone doesn't hold feedback", () => {
    const item = run();
    item.fit(); item.baseline();
    const hum = (size: number) => wave(12, 2, 512).map((value, i) => value + size * Math.sin(2 * Math.PI * 50 * i / 256));
    const state = item.tick(12, { only: { TP9: hum(45), AF7: wave(12, 2), AF8: wave(12, 3), TP10: hum(85) } });
    expect(state.fit).toMatchObject({ TP9: "good", TP10: "good" });
    expect(state.hold).toBeNull();
  });
  it("holds on head movement in plain words, and ignores the gyroscope's resting offset", () => {
    const item = run();
    item.fit(); item.baseline();
    expect(item.tick(12, { gyro: still(item.at() + TICK) }).hold).toBeNull();
    const state = item.tick(12, { gyro: turning(item.at() + TICK) });
    expect(state.hold).toBe("movement");
    expect(state.label).toBe(HOLD_COPY.movement);
    expect(item.session.summary().guided?.held_seconds.movement).toBe(.5);
  });
  it("carries on without a gyroscope, or with a stale one", () => {
    const item = run();
    item.fit(); item.baseline();
    expect(item.tick(12, { gyro: null }).hold).toBeNull();
    expect(item.tick(12, { gyro: turning(item.at() - 9000) }).hold).toBeNull();
  });
  it("puts a labelled heart rate estimate in the summary, and none without PPG", () => {
    const item = run();
    item.fit();
    expect(item.session.summary().heart_rate).toBeNull();
    const samples = Array.from({ length: PPG_BUFFER }, (_, i) => 53_500 + 300 * Math.sin(2 * Math.PI * (64 / 60) * i / PPG_RATE));
    item.tick(12, { ppg: { samples, lastPacketAt: item.at() + TICK } });
    expect(item.session.summary().heart_rate).toEqual({ bpm: 64, readings: 1, estimate: true, source: "ppg" });
  });
});
