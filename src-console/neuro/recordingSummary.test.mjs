import assert from "node:assert/strict";
import { test } from "node:test";
import { build } from "esbuild";

const compiled = await build({ entryPoints: [new URL("./recordingSummary.ts", import.meta.url).pathname], bundle: true, write: false, format: "esm", platform: "node", tsconfig: new URL("../../tsconfig.json", import.meta.url).pathname });
const { RecordingSummarizer, summarizeReplay } = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputFiles[0].contents).toString("base64")}`);

const wave = (hz, amp, offset = 0) => (i) => amp * Math.sin(2 * Math.PI * hz * (i + offset) / 256);
function replay(seconds, signalAt) {
  const lines = [JSON.stringify({ kind: "header", sample_rate: 256, channels: ["TP9", "AF7", "AF8", "TP10"], start_ts: 0 })];
  for (let packet = 0; packet < seconds * 256 / 12; packet++) {
    const timestamp = 1_000 + packet * 12 / 256 * 1000;
    for (const channel of ["TP9", "AF7", "AF8", "TP10"]) {
      lines.push(JSON.stringify({ kind: "sample", dt_ms: 0, timestamp, channel, samples: Array.from({ length: 12 }, (_, k) => signalAt(packet * 12 + k, timestamp)) }));
    }
  }
  return lines.join("\n");
}

test("alpha-dominant recording scores high relaxation, beta-dominant scores high focus", () => {
  const alpha = summarizeReplay(replay(10, (i) => wave(10, 20)(i) + wave(22, 4)(i) + wave(6, 10)(i)));
  const beta = summarizeReplay(replay(10, (i) => wave(10, 4)(i) + wave(22, 20)(i) + wave(6, 10)(i)));
  for (const s of [alpha, beta]) for (const key of ["avg_focus", "avg_relaxation", "calm_pct"]) assert.ok(Number.isFinite(s[key]), key);
  assert.ok(alpha.avg_relaxation > 70 && beta.avg_relaxation < 30, `${alpha.avg_relaxation} ${beta.avg_relaxation}`);
  assert.ok(beta.avg_focus > alpha.avg_focus);
  assert.equal(alpha.time_in_target_pct, null);
});

test("calm % uses the first 60 s as baseline", () => {
  // 60 s balanced, then 30 s alpha-heavy: the later third is calm.
  const s = summarizeReplay(replay(90, (i, t) => t < 61_000 ? wave(10, 10)(i) + wave(22, 10)(i) : wave(10, 30)(i) + wave(22, 5)(i)));
  assert.ok(s.relaxation_baseline > 40 && s.relaxation_baseline < 60, String(s.relaxation_baseline));
  assert.ok(s.calm_pct > 25 && s.calm_pct < 40, String(s.calm_pct));
});

test("fewer than 256 samples leaves the summary null", () => {
  const s = summarizeReplay(replay(0.9, wave(10, 20)));
  assert.deepEqual([s.avg_focus, s.avg_relaxation, s.calm_pct], [null, null, null]);
  const live = new RecordingSummarizer();
  live.tick({ channels: { AF7: new Array(255).fill(1), AF8: new Array(255).fill(1) }, packetCount: 1 }, 0);
  assert.equal(live.summary().avg_focus, null);
});

test("flatlined or malformed data is gated or skipped, not averaged", () => {
  const s = summarizeReplay(replay(5, () => 0) + "\nnot json\n{\"kind\":\"sample\"}");
  assert.equal(s.avg_focus, null);
  assert.ok(s.entries > 0);
});
