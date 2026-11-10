import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import type { RecordedSession } from "../types";
import { score } from "./sessionSamples";

/**
 * design-1110e: Neuro recordings (data contract in HANDOFF-1110e.md). The recorder keeps raw EEG for
 * TP9/AF7/AF8/TP10 today; AUX, PPG and motion recording, raw-window reads, band power over time and
 * retention removal are not wired, so the live adapter says so. Sample values only ever come from
 * `sampleRecordings`, behind `nexus.sampleAdapters=1`, and are shown with a "Sample data" tag.
 */
export type EegChannel = "TP9" | "AF7" | "AF8" | "TP10" | "AUX";
export type Contact = "good" | "fair" | "poor" | "none";
export type Retention = "kept" | "expiring";
export type WindowSecs = 2 | 5 | 10 | 30;
export const RETENTION_DAYS = 90;
export const EEG_CHANNELS: readonly EegChannel[] = ["TP9", "AF7", "AF8", "TP10"];
/** Taisun, 18:08 JST: the 5th electrode is always at the back of the head. */
export const PLACEMENT: Record<EegChannel, string> = { TP9: "Left ear", AF7: "Left forehead", AF8: "Right forehead", TP10: "Right ear", AUX: "Back of head" };

export interface AuxInfo { detected: boolean; placement: "occipital"; detectedBy: "contact" | "signal-quality"; detectedAt?: string }
export interface RecordingListItem {
  id: string; replayFile: string | null; startedAt: string; durationSecs: number; protocol: string | null;
  channels: EegChannel[]; aux: AuxInfo | null; hasHeart: boolean; hasMotion: boolean;
  avgFocus: number | null; avgCalm: number | null; timeInTarget: number | null; calmPct: number | null;
  raw: "available" | "recording" | "summary-only" | "missing"; sizeBytes?: number; retention: Retention;
}
export interface Lane<C extends string = EegChannel> { channel: C; contact?: Contact; values: number[] }
export interface SignalWindow { startSecs: number; endSecs: number; sampleRate: 256; lanes: Lane[] }
export interface BodyWindow {
  startSecs: number; endSecs: number;
  heart: { ppgRate: 64; values: number[]; bpm: number | null; estimate: true } | null;
  motion: { rate: 52; values: number[]; events: Array<{ atSecs: number; label: string }> } | null;
}
export type BandName = "delta" | "theta" | "alpha" | "beta" | "gamma";
export interface SessionSeries { stepSecs: number; bands: Record<BandName, number[]>; focus: number[]; calm: number[] }
export interface RetentionState { retentionDays: typeof RETENTION_DAYS; expiring: RecordingListItem[] }
export interface RemoveConfirmation { confirmedByUser: true; at: string }

/** Claude wires these (HANDOFF-1110e.md). Synchronous over already-loaded data, as in 1110d. */
export interface RecordingsAdapter {
  list(): Wired<RecordingListItem[]>;
  window(id: string, startSecs: number, spanSecs: WindowSecs, points?: number): Wired<SignalWindow>;
  body(id: string, startSecs: number, spanSecs: WindowSecs): Wired<BodyWindow>;
  series(id: string): Wired<SessionSeries>;
  contact(id: string): Wired<Partial<Record<EegChannel, Contact>>>;
  retention(): Wired<RetentionState>;
  /** The ONLY removal path. Called only from the confirmation dialog with the ids the user ticked. */
  removeConfirmed(ids: string[], confirmation: RemoveConfirmation): Wired<{ removed: string[] }>;
  /** Live: is the back-of-head electrode detected right now (automatic, no toggle)? */
  liveAux(): Wired<AuxInfo>;
}

const DAY = 86_400_000;
export const isExpiring = (startedAt: string, now: number) => now - new Date(startedAt).getTime() > RETENTION_DAYS * DAY;

/** Real list from saved sessions: what the session files say, nothing more. */
export function recordingFromSession(s: RecordedSession, now: number): RecordingListItem {
  return {
    id: s.id, replayFile: s.file_path ?? null, startedAt: s.start, durationSecs: s.duration_secs, protocol: null,
    channels: [...EEG_CHANNELS], aux: null, hasHeart: false, hasMotion: false,
    avgFocus: score(s.summary.avg_focus), avgCalm: score(s.summary.avg_relaxation),
    timeInTarget: s.summary.time_in_target_pct ?? null, calmPct: s.summary.calm_pct ?? null,
    raw: s.file_path ? "available" : "summary-only", retention: isExpiring(s.start, now) ? "expiring" : "kept",
  };
}

export function liveRecordings(sessions: RecordedSession[], now = Date.now()): RecordingsAdapter {
  const items = sessions.filter((s) => s.device_type === "muse").map((s) => recordingFromSession(s, now));
  return {
    list: () => ({ state: "live", value: items }),
    window: () => UNWIRED, body: () => UNWIRED, series: () => UNWIRED, contact: () => UNWIRED,
    // Which recordings are past 90 days is a fact from their dates; removing them is not wired.
    retention: () => ({ state: "live", value: { retentionDays: RETENTION_DAYS, expiring: items.filter((r) => r.retention === "expiring") } }),
    removeConfirmed: () => UNWIRED,
    liveAux: () => UNWIRED,
  };
}

// ---- Sample adapter (previews and tests only) --------------------------------------------------

function rng(seed: number) {
  let s = seed | 0;
  return () => { s = (s + 0x6d2b79f5) | 0; let t = Math.imul(s ^ (s >>> 15), 1 | s); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
const SHAPE: Record<EegChannel, { alpha: number; noise: number; blink?: number; seed: number }> = {
  TP9: { alpha: 16, noise: 7, seed: 3 }, AF7: { alpha: 9, noise: 6, blink: 70, seed: 5 }, AF8: { alpha: 9, noise: 6, blink: 62, seed: 7 },
  TP10: { alpha: 15, noise: 8, seed: 11 }, AUX: { alpha: 22, noise: 6, seed: 13 },
};
/** Synthetic µV at 256 Hz, averaged into `points` buckets (the averaging is the smoothing). */
export function sampleTrace(channel: EegChannel, startSecs: number, spanSecs: number, points = 900): number[] {
  const p = SHAPE[channel]; const next = rng(p.seed * 977 + Math.round(startSecs * 13));
  const n = Math.round(spanSecs * 256); const raw = new Float64Array(n); let pink = 0; const ph = next() * 6;
  for (let i = 0; i < n; i++) {
    const t = startSecs + i / 256; pink = pink * 0.96 + (next() - 0.5) * p.noise * 0.9;
    let v = p.alpha * Math.sin(2 * Math.PI * 10.2 * t + ph) * (0.75 + 0.25 * Math.sin(t * 1.3)) + 4 * Math.sin(2 * Math.PI * 21 * t + ph * 2) + 5 * Math.sin(2 * Math.PI * 5.5 * t) + pink;
    if (p.blink) { const b = (t % 3.7) - 2.1; if (b > 0 && b < 0.35) v += p.blink * Math.sin(Math.PI * b / 0.35); }
    raw[i] = v;
  }
  return bucket(Array.from(raw), points);
}
export function bucket(values: number[], points: number): number[] {
  if (values.length <= points) return values;
  const step = values.length / points; const out: number[] = [];
  for (let k = 0; k < points; k++) { let s = 0, c = 0; for (let i = Math.floor(k * step); i < Math.floor((k + 1) * step); i++) { s += values[i]; c++; } out.push(c ? s / c : 0); }
  return out;
}
const walk = (seed: number, n: number, amp: number, base: number) => { const next = rng(seed); let v = 0; return Array.from({ length: n }, () => (v = v * 0.85 + (next() - 0.5) * amp, base + v)); };
const clamp = (v: number) => Math.max(5, Math.min(95, v));
function sampleSeries(seed: number): SessionSeries {
  const n = 120; const names: BandName[] = ["delta", "theta", "alpha", "beta", "gamma"]; const base = [18, 16, 34, 22, 10];
  const raw = names.map((_, b) => walk(seed * 7 + b, n, 4, base[b]).map((v, i) => (b === 2 ? v + 6 * Math.sin(i / 9) : v)));
  const totals = Array.from({ length: n }, (_, i) => raw.reduce((s, r) => s + Math.max(1, r[i]), 0));
  const bands = Object.fromEntries(names.map((k, b) => [k, raw[b].map((v, i) => (Math.max(1, v) / totals[i]) * 100)])) as Record<BandName, number[]>;
  return { stepSecs: 6, bands, focus: walk(seed + 11, n, 9, 30).map((v, i) => clamp(v + 8 * Math.sin(i / 20))), calm: walk(seed + 23, n, 9, 50).map((v, i) => clamp(v - 6 * Math.sin(i / 17))) };
}
const AUX_SAMPLE: AuxInfo = { detected: true, placement: "occipital", detectedBy: "contact" };
const row = (id: string, startedAt: string, durationSecs: number, protocol: string, five: boolean, focus: number | null, calm: number | null, raw: RecordingListItem["raw"], retention: Retention = "kept"): RecordingListItem => ({
  id, replayFile: raw === "summary-only" ? null : `sample-${id}.ndjson`, startedAt, durationSecs, protocol, channels: five ? [...EEG_CHANNELS, "AUX"] : [...EEG_CHANNELS], aux: five ? AUX_SAMPLE : null,
  hasHeart: five, hasMotion: five, avgFocus: focus, avgCalm: calm, timeInTarget: focus == null ? null : 19, calmPct: calm == null ? null : 1, raw, retention,
});
/** The design-1110e mockup's rows. Generated: none of it is a real reading. */
export const SAMPLE_LIST: RecordingListItem[] = [
  row("r1", "2026-11-10T17:54:00+09:00", 0, "Free session", true, null, null, "recording"),
  row("r2", "2026-11-07T12:09:00+09:00", 136, "Peak Performance", true, 33, 48, "available"),
  row("r3", "2026-11-07T11:57:00+09:00", 682, "Peak Performance", false, 30, 50, "available"),
  row("r4", "2026-11-07T11:46:00+09:00", 378, "Peak Performance", false, 19, 65, "available"),
  row("r5", "2026-11-06T07:42:00+09:00", 120, "Guided calm", false, 18, 59, "available"),
  row("r6", "2026-10-29T19:09:00+09:00", 308, "Free session", false, 30, 51, "available"),
  row("r7", "2026-03-05T19:52:00+09:00", 17, "Free session", false, null, null, "available", "expiring"),
  row("r8", "2026-02-18T22:51:00+09:00", 270, "Legacy", false, null, null, "summary-only", "expiring"),
  row("r9", "2026-02-17T19:05:00+09:00", 0, "Free session", false, null, null, "available", "expiring"),
];

export function createSampleRecordings(list: RecordingListItem[] = SAMPLE_LIST): RecordingsAdapter & { removals: Array<{ ids: string[]; confirmation: RemoveConfirmation }> } {
  const removals: Array<{ ids: string[]; confirmation: RemoveConfirmation }> = [];
  const find = (id: string) => list.find((r) => r.id === id);
  return {
    removals,
    list: () => sample(list),
    window: (id, startSecs, spanSecs, points = 900) => {
      const r = find(id); if (!r || r.raw === "summary-only") return UNWIRED;
      return sample({ startSecs, endSecs: startSecs + spanSecs, sampleRate: 256, lanes: r.channels.map((channel) => ({ channel, contact: channel === "AF8" && !r.aux ? "fair" : "good", values: sampleTrace(channel, startSecs, spanSecs, points) })) });
    },
    body: (id, startSecs, spanSecs) => {
      const r = find(id); if (!r) return UNWIRED;
      const pts = 600; const f = 68 / 60;
      const heart = Array.from({ length: pts }, (_, i) => { const t = startSecs + (i / (pts - 1)) * spanSecs; const ph = (t * f) % 1; return 60 * Math.exp(-((ph - 0.18) ** 2) / 0.004) + 22 * Math.exp(-((ph - 0.42) ** 2) / 0.01) - 30; });
      const next = rng(91); let v = 0;
      const motion = Array.from({ length: pts }, (_, i) => { const t = (i / (pts - 1)) * spanSecs; v = v * 0.9 + (next() - 0.5) * 6; return v + (t > 2.6 && t < 3.1 ? 55 * Math.sin((Math.PI * (t - 2.6)) / 0.5) : 0); });
      return sample({ startSecs, endSecs: startSecs + spanSecs, heart: { ppgRate: 64, values: heart, bpm: 68, estimate: true }, motion: { rate: 52, values: motion, events: [{ atSecs: startSecs + 2.8, label: "Head turn" }] } });
    },
    series: (id) => (find(id) ? sample(sampleSeries(id.length * 31 + id.charCodeAt(1))) : UNWIRED),
    contact: (id) => { const r = find(id); if (!r) return UNWIRED; return sample(Object.fromEntries(r.channels.map((c) => [c, c === "AF8" && !r.aux ? "fair" : "good"])) as Partial<Record<EegChannel, Contact>>); },
    retention: () => sample({ retentionDays: RETENTION_DAYS, expiring: list.filter((r) => r.retention === "expiring") }),
    // Preview only: remembers the call so tests can prove it needs a confirmation. Never touches disk.
    removeConfirmed: (ids, confirmation) => {
      if (confirmation?.confirmedByUser !== true) throw new Error("removeConfirmed needs the user's confirmation");
      removals.push({ ids: [...ids], confirmation }); return sample({ removed: [...ids] });
    },
    liveAux: () => sample(AUX_SAMPLE),
  };
}

export function recordingsAdapter(sessions: RecordedSession[]): RecordingsAdapter {
  return sampleAdaptersEnabled() ? createSampleRecordings() : liveRecordings(sessions);
}
