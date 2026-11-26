import { bandFeatures } from "./fft";
import { assessChannel } from "../legacy-core/hooks/useSignalQuality";
import { calmPercent, relaxationBaseline, type TimelineEntry } from "./training";

/**
 * Summary for a plain recording (no training protocol). Uses the same frontal
 * band computation as TrainingSession (AF7/AF8, `neuro/fft.ts` bandFeatures, decision 0060):
 * focus = focusScore, relaxation = relaxationScore. Calm % uses the training
 * definition with the baseline from the first 60 s of valid entries.
 */
export const FRONTAL = ["AF7", "AF8"] as const;
export const MIN_WINDOW = 256;
const BUFFER = 512;
const TICK_MS = 500;

export type RecordingSummary = { avg_focus: number | null; avg_relaxation: number | null; calm_pct: number | null; time_in_target_pct: null; relaxation_baseline: number | null; entries: number };

export const EMPTY_RECORDING_SUMMARY: RecordingSummary = { avg_focus: null, avg_relaxation: null, calm_pct: null, time_in_target_pct: null, relaxation_baseline: null, entries: 0 };

export class RecordingSummarizer {
  private entries: TimelineEntry[] = [];
  private lastTick = -Infinity;
  private lastPacket = -1;
  private buffers: Record<string, number[]> = {};

  /** Evaluate one window set (each channel's recent samples) at time `now`. */
  addWindow(channels: Record<string, number[] | undefined>, now: number): void {
    if (now - this.lastTick < TICK_MS) return;
    const frontal = FRONTAL.map((name) => channels[name] ?? []);
    if (frontal.some((samples) => samples.length < MIN_WINDOW)) return;
    this.lastTick = now;
    const gated = frontal.some((samples) => samples.some((value) => !Number.isFinite(value)) || assessChannel(samples.slice(-MIN_WINDOW)) !== "good");
    // Decision 0060: the one pipeline, power averaged across AF7 and AF8 (a gap reads as 0 and gates the tick).
    const bands = bandFeatures(frontal.map((samples) => samples.slice(-MIN_WINDOW).map((value) => (Number.isFinite(value) ? value : 0))));
    const focus = bands.focusScore;
    const relaxation = bands.relaxationScore;
    this.entries.push({ timestamp: now, focus, relaxation, action: "record", gated: gated || !Number.isFinite(focus) || !Number.isFinite(relaxation) });
  }

  /** Live path: feed a telemetry snapshot (skips if no new packet arrived). */
  tick(telemetry: { channels: Record<string, number[]>; packetCount: number }, now: number): void {
    if (telemetry.packetCount === this.lastPacket) return;
    this.lastPacket = telemetry.packetCount;
    this.addWindow(telemetry.channels, now);
  }

  /** Replay path: feed one recorded packet. */
  addPacket(channel: string, samples: number[], timestamp: number): void {
    this.buffers[channel] = [...(this.buffers[channel] ?? []), ...samples].slice(-BUFFER);
    this.addWindow(this.buffers, timestamp);
  }

  summary(): RecordingSummary {
    const valid = this.entries.filter((entry) => !entry.gated);
    if (!valid.length) return { ...EMPTY_RECORDING_SUMMARY, entries: this.entries.length };
    const mean = (key: "focus" | "relaxation") => valid.reduce((sum, entry) => sum + entry[key], 0) / valid.length;
    const baseline = relaxationBaseline([], this.entries);
    return { avg_focus: mean("focus"), avg_relaxation: mean("relaxation"), calm_pct: calmPercent(this.entries, baseline), time_in_target_pct: null, relaxation_baseline: baseline, entries: this.entries.length };
  }
}

/** Summarize a replay ndjson (header + `kind: "sample"` lines). Malformed lines are skipped. */
export function summarizeReplay(text: string): RecordingSummary {
  const summarizer = new RecordingSummarizer();
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    let row: { kind?: string; channel?: unknown; samples?: unknown; timestamp?: unknown };
    try { row = JSON.parse(line); } catch { continue; }
    if (row.kind !== "sample" || typeof row.channel !== "string" || !Array.isArray(row.samples) || typeof row.timestamp !== "number") continue;
    summarizer.addPacket(row.channel, row.samples.map(Number), row.timestamp);
  }
  return summarizer.summary();
}
