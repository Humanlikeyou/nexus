import assert from "node:assert/strict";
import { test } from "node:test";
import { build } from "esbuild";

const compiled = await build({ entryPoints: [new URL("./training.ts", import.meta.url).pathname], bundle: true, write: false, format: "esm", platform: "node", tsconfig: new URL("../../tsconfig.json", import.meta.url).pathname });
const { TrainingSession } = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputFiles[0].contents).toString("base64")}`);
const protocol = { id: "calm", name: "Calm", rewardBands: [{ label: "Alpha", minHz: 8, maxHz: 13 }], inhibitBands: [{ label: "High Beta", minHz: 20, maxHz: 30 }] };
const channels = ["TP9", "AF7", "AF8", "TP10"];
const signal = Array.from({ length: 512 }, (_, i) => 10 * Math.sin(2 * Math.PI * 10 * i / 256) + 5 * Math.sin(2 * Math.PI * 22 * i / 256));
function telemetry(tick, overrides = {}) {
  const now = 10_000 + tick * 500;
  return { channels: Object.fromEntries(channels.map((name, index) => [name, signal.map((value) => index % 2 ? -value : value)])), channelLastPacketAt: Object.fromEntries(channels.map((name) => [name, now])), packetCount: tick + 1, lastPacketAt: now, ...overrides };
}

test("training calibrates on valid data and records real summary entries", () => {
  const session = new TrainingSession(protocol);
  for (let tick = 0; tick < 10; tick++) assert.equal(session.tick(telemetry(tick), 10_000 + tick * 500).phase, "calibrating");
  const result = session.tick(telemetry(10), 15_000);
  assert.equal(result.phase, "training");
  assert.ok(Number.isFinite(result.focus));
  const summary = session.summary();
  assert.equal(summary.protocol_id, "calm");
  assert.equal(summary.timeline.length, 1);
  assert.equal(summary.avg_focus, result.focus);
});

test("a stale single electrode holds feedback despite other live electrodes", () => {
  const session = new TrainingSession(protocol);
  const data = telemetry(0);
  data.channelLastPacketAt.AF7 = 0;
  assert.equal(session.tick(data, 10_000).phase, "held");
  assert.equal(session.summary().timeline.length, 0);
});

test("flatline and missing channels cannot calibrate", () => {
  for (const samples of [[], Array(512).fill(0), Array(512).fill(NaN)]) {
    const session = new TrainingSession(protocol);
    const data = telemetry(0);
    data.channels.AF8 = samples;
    assert.equal(session.tick(data, 10_000).phase, "held");
    assert.equal(session.state.calibrationPercent, 0);
  }
});

test("signal interruption after calibration immediately removes feedback", () => {
  const session = new TrainingSession(protocol);
  for (let tick = 0; tick <= 10; tick++) session.tick(telemetry(tick), 10_000 + tick * 500);
  const held = session.tick(telemetry(10), 18_000);
  assert.equal(held.phase, "held");
  assert.equal(held.signal, null);
  assert.equal(held.focus, null);
});
