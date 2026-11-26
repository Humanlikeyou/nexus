import { describe, expect, it } from "vitest";
import { PROTOCOLS } from "../legacy-core/lib/neurofeedback/protocols";
import { alphaPeak, CalibrationRun, epoch, METHOD, summarise } from "./calibration";
import { FFT_SIZE, powerSpectrum } from "./fft";
import type { MuseTelemetry } from "./telemetryStore";
import { BASELINE_TICKS, TrainingSession } from "./training";

/** Decision 0058: pre-meditation calibration and the protocol calibration record. */
const CH = ["TP9", "AF7", "AF8", "TP10"];
function wave(alphaHz: number, amp: number, phase: number, n = FFT_SIZE) {
  let seed = phase * 13 + 5;
  const noise = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280 - .5; };
  return Array.from({ length: n }, (_, i) => { const t = (i + phase * n) / 256; return amp * Math.sin(2 * Math.PI * alphaHz * t) + 3 * Math.sin(2 * Math.PI * 21 * t) + 4 * noise(); });
}
const windows = (f: (c: string) => number[]) => Object.fromEntries(CH.map((c) => [c, f(c)]));

describe("features", () => {
  it("finds a clear alpha peak between bins, and none in its absence", () => {
    const peak = alphaPeak(powerSpectrum(wave(10.4, 20, 0)));
    expect(peak).not.toBeNull();
    expect(Math.abs(peak! - 10.4)).toBeLessThan(0.5);
    expect(alphaPeak(powerSpectrum(wave(10, 0, 0)))).toBeNull();
  });

  it("rejects frozen, flat, stale, large and blink epochs with a reason, and keeps clean ones", () => {
    expect(epoch(windows(() => wave(10, 15, 1)), true, 0)).toEqual({ accepted: false, reason: "frozen" });
    expect(epoch(windows(() => new Array(FFT_SIZE).fill(0)), false, 0)).toMatchObject({ accepted: false });
    expect(epoch(windows((c) => (c === "AF7" ? [] : wave(10, 15, 1))), false, 0)).toEqual({ accepted: false, reason: "dropout" });
    const blink = windows((c) => { const w = wave(10, 15, 1); if (c === "AF7" || c === "AF8") for (let i = 60; i < 110; i++) w[i] += 120; return w; });
    expect(epoch(blink, false, 0).accepted).toBe(false);
    const ok = epoch(windows(() => wave(10, 15, 2)), false, 0);
    expect(ok.accepted).toBe(true);
    if (ok.accepted) expect(ok.bands.alpha).toBeGreaterThan(ok.bands.beta);
  });

  it("summarises with medians, keeps the reject reasons, and calls the alpha peak not estimable when rare", () => {
    const r = summarise("eyes_closed", 60, [
      { accepted: true, bands: { delta: 10, theta: 10, alpha: 50, beta: 20, gamma: 10 }, logPower: 2, alphaPeak: null },
      { accepted: true, bands: { delta: 10, theta: 10, alpha: 60, beta: 15, gamma: 5 }, logPower: 2.2, alphaPeak: 10 },
      { accepted: true, bands: { delta: 10, theta: 10, alpha: 99, beta: 1, gamma: 0 }, logPower: 2.1, alphaPeak: null },
      { accepted: false, reason: "blink" },
    ], { TP9: { ok: 4, total: 4 } });
    expect(r.features?.relBands.alpha).toBe(60);
    expect(r.features?.alphaPeakHz).toBeNull();
    expect(r.rejected).toEqual({ blink: 1 });
    expect(r.channelQuality).toMatchObject({ TP9: 1, AF7: 0 });
    expect((r.features?.variability ?? -1) >= 0).toBe(true);
  });
});

function telemetry(now: number, packet: number, patch: Partial<MuseTelemetry> = {}): MuseTelemetry {
  return { channels: windows((c) => wave(10, 15, packet + c.length)), bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: packet, sampleCount: packet * 256 * 4,
    lastPacketAt: now, channelLastPacketAt: Object.fromEntries(CH.map((c) => [c, now])), source: "live", ...patch };
}

describe("a calibration run", () => {
  it("can't begin on replay or demo data, or without contact", () => {
    const run = new CalibrationRun(20);
    run.tick(telemetry(1000, 1, { source: "replay" }), 1000);
    expect(run.state.blocker).toMatch(/replay or demo/);
    expect(run.begin()).toBe(false);
    run.tick(telemetry(2000, 2, { channelLastPacketAt: {} }), 2000);
    expect(run.state.blocker).toMatch(/no signal/);
    expect(() => new CalibrationRun(5)).toThrow();
  });

  it("records eyes open then eyes closed for the chosen length, never the same samples twice, and reports counts only", () => {
    const run = new CalibrationRun(20);
    let now = 1000, p = 1;
    run.tick(telemetry(now, p), now);
    expect(run.begin()).toBe(true);
    // A second tick with no new samples adds no epoch.
    run.tick(telemetry(now, p), now);
    for (let i = 0; i < 60 && run.state.stage !== "done"; i++) { now += 1000; p += 1; run.tick(telemetry(now, p), now); }
    expect(run.state.stage).toBe("done");
    const report = run.report();
    expect(report.kind).toBe("pre_meditation");
    expect(report.source).toBe("live");
    expect(report.method).toBe(METHOD);
    expect(report.conditions.map((c) => c.condition)).toEqual(["eyes_open", "eyes_closed"]);
    expect(report.conditions[0].epochsTotal).toBe(20);
    expect(JSON.stringify(report).length).toBeLessThan(4000); // features and counts, no raw EEG
  });

  it("stopping early keeps what was recorded", () => {
    const run = new CalibrationRun(30);
    let now = 1000, p = 1;
    run.tick(telemetry(now, p), now); run.begin();
    for (let i = 0; i < 5; i++) { now += 1000; p += 1; run.tick(telemetry(now, p), now); }
    run.stop();
    expect(run.state.stage).toBe("done");
    expect(run.report().conditions[0].epochsTotal).toBe(5);
  });
});

describe("protocol calibration stays separate", () => {
  it("records the protocol's thresholds, baseline data and method once its baseline is done", () => {
    const calm = PROTOCOLS.find((x) => x.id === "calm")!;
    const s = new TrainingSession(calm, { guided: true });
    expect(s.protocolCalibration()).toBeNull();
    let now = 1_000_000, p = 0;
    const t = (alpha: number) => { now += 500; p += 1; return s.tick({ ...telemetry(now, p), channels: windows(() => wave(10, alpha, p)), packetCount: p, sampleCount: p * 12 }, now); };
    for (let i = 0; i < 6; i++) t(20);
    s.advance();
    for (let i = 0; i < BASELINE_TICKS * 2; i++) t(i < BASELINE_TICKS ? 12 : 24);
    const pc = s.protocolCalibration()!;
    expect(pc.protocolId).toBe("calm");
    expect(pc.baselineTicks.eyes_open).toBe(BASELINE_TICKS);
    expect(pc.thresholds.rewardMin).toBeTruthy();
    expect(pc.method.version).toMatch(/training/);
    expect(s.summary().protocol_calibration).toEqual(pc);
  });
});
