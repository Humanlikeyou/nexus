// Regenerates the synthetic Muse fixtures in this folder. No real recording is used or copied.
// Run: node test/fixtures/muse/generate.mjs
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = dirname(fileURLToPath(import.meta.url));
const START = 1_760_000_000_000;
let seed = 12345;
const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296) - 0.5;
const r2 = (v) => Math.round(v * 100) / 100;
const eeg = (ch, i) => {
  const t = i / 256;
  const alpha = ch === "TP9" || ch === "TP10" ? 14 : 8;
  return r2(alpha * Math.sin(2 * Math.PI * 10 * t) + 6 * Math.sin(2 * Math.PI * 20 * t + 1) + 5 * Math.sin(2 * Math.PI * 6 * t) + 6 * rnd());
};
const flat = () => r2(0.2 * rnd());
const line = (o) => JSON.stringify(o);

/** secs of 4 EEG channels (+AUX), 12 samples per packet, one packet group every 46.875 ms. */
function body({ secs, aux, ppg, motion, gap }) {
  const out = [];
  const groups = Math.floor((secs * 256) / 12);
  let prev = null;
  for (let g = 0; g < groups; g++) {
    const ts = START + Math.round(g * 46.875);
    const chans = ["TP9", "AF7", "AF8", "TP10", ...(aux ? ["AUX"] : [])];
    for (const ch of chans) {
      if (gap && ch === "TP9" && ts - START >= gap[0] * 1000 && ts - START < gap[1] * 1000) continue;
      const samples = Array.from({ length: 12 }, (_, k) => (ch === "AUX" && aux === "flat" ? flat() : ch === "AUX" ? eeg("AF7", g * 12 + k) : eeg(ch, g * 12 + k)));
      out.push(line({ kind: "sample", dt_ms: prev === null ? 0 : ts - prev, timestamp: ts, channel: ch, samples }));
      prev = ts;
    }
    if (ppg && g % 1 === 0 && ((g * 46.875) % 93.75) < 46.875) {
      // 6 samples per packet at 64 Hz = one packet every 93.75 ms; infrared at 72 bpm
      const n = Math.round((g * 46.875) / 93.75);
      const samples = Array.from({ length: 6 }, (_, k) => {
        const t = (n * 6 + k) / 64;
        const ph = (t * 72 / 60) % 1;
        return Math.round(50000 + 1000 * Math.exp(-((ph - 0.2) ** 2) / 0.004));
      });
      out.push(line({ kind: "ppg", source: "infrared", timestamp: ts, samples }));
    }
    if (motion && g % 1 === 0 && ((g * 46.875) % 57.7) < 46.875) {
      const n = Math.round((g * 46.875) / 57.7);
      const moving = n >= 40 && n < 60;
      const samples = Array.from({ length: 3 }, () => [moving ? 60 : r2(2 + rnd()), r2(1 + rnd()), r2(rnd())]);
      out.push(line({ kind: "motion", sensor: "gyroscope", timestamp: ts, samples }));
    }
  }
  return out;
}

const header1 = line({ kind: "header", sample_rate: 256, channels: ["TP9", "AF7", "AF8", "TP10"], start_ts: START, session_id: "fixture-format1" });
const header2 = (extra) => line({ kind: "header", format: 2, sample_rate: 256, channels: ["TP9", "AF7", "AF8", "TP10", "AUX"], ppg: ["infrared"], motion: ["gyroscope"], start_ts: START, session_id: "fixture-format2", ...extra });
const trailer = (aux) => line({ kind: "trailer", end_ts: START + 6000, aux });
const nl = (a) => a.join("\n") + "\n";

writeFileSync(join(dir, "format1_4ch.ndjson"), nl([header1, ...body({ secs: 6 })]));
writeFileSync(join(dir, "format2_aux_on.ndjson"), nl([header2({}), ...body({ secs: 6, aux: "eeg", ppg: true, motion: true }), trailer({ detected: true, placement: "occipital", detectedBy: "signal-quality", detectedAt: "2026-11-11T00:00:00Z" })]));
writeFileSync(join(dir, "format2_aux_off.ndjson"), nl([header2({ ppg: [], motion: [] }), ...body({ secs: 6, aux: "flat" }), trailer({ detected: false, placement: "occipital", detectedBy: "signal-quality" })]));
writeFileSync(join(dir, "gap_tp9.ndjson"), nl([header1, ...body({ secs: 6, gap: [2, 3.5] })]));
const trunc = nl([header2({ ppg: [], motion: [], channels: ["TP9", "AF7", "AF8", "TP10"] }), ...body({ secs: 3 })]);
writeFileSync(join(dir, "truncated.ndjson"), trunc.slice(0, trunc.length - 40)); // cut mid-line, no trailer
writeFileSync(join(dir, "header_only.ndjson"), nl([header2({ ppg: [], motion: [] }), trailer(null)]));

const summary = (extra) => ({ avg_focus: 0.5, avg_relaxation: 0.6, avg_hr: null, max_hr: null, avg_hrv: null, ...extra });
writeFileSync(join(dir, "legacy_session.json"), JSON.stringify({ session: { id: "legacy1", device_type: "muse", start: "2026-02-18T22:51:00Z", end: "2026-02-18T22:55:30Z", duration_secs: 270, data_points: 4, file_path: "", summary: summary({}) }, data: [{ t: 0, v: 1 }, { t: 1, v: 2 }] }));
const timeline = Array.from({ length: 12 }, (_, i) => ({ timestamp: START + 500 + i * 500, focus: i < 6 ? 40 : 60, relaxation: 30, action: "record", gated: false }));
writeFileSync(join(dir, "session_with_timeline.json"), JSON.stringify({ session: { id: "tl1", device_type: "muse", start: "2025-10-09T00:00:00Z", end: "2025-10-09T00:00:06Z", duration_secs: 6, data_points: 100, file_path: "format2_aux_on.ndjson", summary: summary({ avg_focus: 50, avg_relaxation: 30, score_scale: 100 }) }, data: { source: "live_muse", neurofeedback: { protocol_name: "Free session", timeline } } }));

// Parity input: one 256-sample window per channel kind, computed from formulas the TS test repeats.
const win = (f) => Array.from({ length: 256 }, (_, i) => f(i / 256));
const inputs = {
  alpha10: win((t) => 20 * Math.sin(2 * Math.PI * 10 * t)),
  mixed: win((t) => 14 * Math.sin(2 * Math.PI * 10 * t) + 6 * Math.sin(2 * Math.PI * 20 * t + 1) + 5 * Math.sin(2 * Math.PI * 6 * t) + 3 * Math.sin(2 * Math.PI * 2.5 * t)),
  hum50: win((t) => 10 * Math.sin(2 * Math.PI * 10 * t) + 30 * Math.sin(2 * Math.PI * 50 * t)),
  flat: win(() => 0),
  noisy: win((t) => 150 * Math.sin(2 * Math.PI * 9 * t)),
};
writeFileSync(join(dir, "parity_inputs.json"), JSON.stringify(Object.fromEntries(Object.entries(inputs).map(([k, v]) => [k, v.map((x) => Math.round(x * 1e6) / 1e6)]))));
