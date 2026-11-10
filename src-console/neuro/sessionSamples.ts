import type { RecordedSession } from "../types";

/**
 * design-1110c C15–C20: the per-session time series Neuro history draws. The recorder saves raw EEG
 * and the focus/calm timeline, but nothing reads them back yet, so real sessions arrive without
 * `samples` (design-1110e: see HANDOFF-1110e.md for the reader).
 */
export type BandKey = "gamma" | "beta" | "alpha" | "theta" | "delta";
export type SensorKey = "TP9" | "AF7" | "AF8" | "TP10";

export interface SessionSamples {
  /** Seconds between points. */
  stepSecs: number;
  /** Relative band power, % of total (0–100), one value per step. */
  bands: Record<BandKey, number[]>;
  /** Focus and calm scores, 0–100, one value per step. */
  focus: number[];
  calm: number[];
  /** Relative band power per sensor, % (optional: older recordings may not split by sensor). */
  sensors?: Partial<Record<SensorKey, Partial<Record<BandKey, number[]>>>>;
  /** Contact quality per sensor over the session, 0–100. */
  contact?: Partial<Record<SensorKey, number>>;
}

export type SessionWithSamples = RecordedSession & { samples?: SessionSamples | null; sample?: boolean };

export const BANDS: ReadonlyArray<{ key: BandKey; name: string; range: string }> = [
  { key: "gamma", name: "Gamma", range: "30–44 Hz" },
  { key: "beta", name: "Beta", range: "13–30 Hz" },
  { key: "alpha", name: "Alpha", range: "8–13 Hz" },
  { key: "theta", name: "Theta", range: "4–8 Hz" },
  { key: "delta", name: "Delta", range: "1–4 Hz" },
];

export const SENSORS: ReadonlyArray<{ key: SensorKey; name: string; x: number; y: number }> = [
  { key: "TP9", name: "Left ear", x: 14, y: 58 },
  { key: "AF7", name: "Left forehead", x: 34, y: 20 },
  { key: "AF8", name: "Right forehead", x: 66, y: 20 },
  { key: "TP10", name: "Right ear", x: 86, y: 58 },
];

/** Seconds treated as "settling in" at the start of a session (C18). */
export const SETTLE_SECS = 90;

const mean = (xs: readonly number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN);

/** True when a session carries enough samples to draw (two or more steps of focus or a band). */
export function hasSamples(session: SessionWithSamples): session is SessionWithSamples & { samples: SessionSamples } {
  const s = session.samples;
  return !!s && s.stepSecs > 0 && (s.focus.length > 1 || BANDS.some(({ key }) => (s.bands[key]?.length ?? 0) > 1));
}

/** Summary scores are stored as 0–1 on some sessions and 0–100 on others; show them as 0–100. */
export const score = (v: number | null | undefined) => (v == null || !Number.isFinite(v) ? null : v <= 1 ? v * 100 : v);

/** C16: the session's headline numbers from its samples. */
export function sessionStats(s: SessionSamples) {
  const bandAvg = Object.fromEntries(BANDS.map(({ key }) => [key, mean(s.bands[key] ?? [])])) as Record<BandKey, number>;
  const strongest = BANDS.reduce((best, b) => (bandAvg[b.key] > bandAvg[best.key] ? b : best), BANDS[0]);
  const contacts = Object.values(s.contact ?? {}).filter((v): v is number => typeof v === "number");
  return { focus: mean(s.focus), calm: mean(s.calm), bandAvg, strongest, contact: contacts.length ? Math.min(...contacts) : null };
}

/** mm:ss from the start of the session, for the time axis. */
export const clockAt = (i: number, step: number) => { const t = Math.round(i * step); return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, "0")}`; };

/**
 * C20: sample sessions exist only in dev (`vite`) and under the e2e/capture mock. A production build
 * talking to the real backend never shows them, and they are never merged into the real list.
 */
export function sampleMode(): boolean {
  const env = (import.meta as ImportMeta & { env?: { DEV?: boolean; MODE?: string } }).env;
  if (env?.MODE === "test") return false;
  if (env?.DEV) return true;
  return typeof window !== "undefined" && "__E2E_DATA" in window;
}

/* ---------- seeded sample generator (sample data only, labelled everywhere it shows) ---------- */
const rng = (seed: number) => () => {
  seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const walk = (r: () => number, n: number, amp: number) => { let v = 0; const w = Array.from({ length: n }, () => (v = v * .92 + (r() - .5))); return w.map((x, i) => mean(w.slice(Math.max(0, i - 3), i + 4)) * amp); };
const BASE: Record<BandKey, number> = { gamma: 9, beta: 19, alpha: 27, theta: 21, delta: 24 };
const SAMPLE_DAYS: ReadonlyArray<[string, string, number]> = [["2026-09-29", "07:40", 12], ["2026-10-02", "21:05", 15], ["2026-10-06", "07:35", 10], ["2026-10-09", "21:20", 20], ["2026-10-13", "21:00", 16], ["2026-10-17", "08:10", 12], ["2026-10-22", "21:10", 18], ["2026-10-26", "07:45", 14]];

/** Eight deterministic sample sessions, most recent first, each flagged `sample: true`. */
export function sampleSessions(step = 5): SessionWithSamples[] {
  return SAMPLE_DAYS.map(([day, start, min], idx) => {
    const r = rng((idx + 11) * 7919), n = Math.round((min * 60) / step);
    const settle = (i: number) => Math.min(1, i / (SETTLE_SECS / step));
    const raw = Object.fromEntries(BANDS.map(({ key }) => {
      const w = walk(r, n, BASE[key] * .32);
      return [key, w.map((x, i) => Math.max(2, BASE[key] + x + (key === "alpha" ? idx * .5 + 6 * settle(i) - 3 : key === "beta" ? 2 - 3 * settle(i) + Math.sin(i / 23) * 1.5 : 0)))];
    })) as Record<BandKey, number[]>;
    const bands = Object.fromEntries(BANDS.map(({ key }) => [key, raw[key].map((v, i) => (v / BANDS.reduce((sum, b) => sum + raw[b.key][i], 0)) * 100)])) as Record<BandKey, number[]>;
    const clamp = (v: number) => Math.max(5, Math.min(95, v));
    const focus = bands.beta.map((b, i) => clamp(34 + (b - 18) * 2.2 + (bands.gamma[i] - 9) * 1.2 + 16 * settle(i) + idx * .8));
    const calm = bands.alpha.map((a, i) => clamp(38 + (a - 27) * 2.2 + (bands.theta[i] - 21) * .6 + 12 * settle(i) + idx * .7));
    const sensors = Object.fromEntries(SENSORS.map(({ key }, si) => {
      const off = walk(r, n, 1.6);
      return [key, Object.fromEntries(BANDS.map((b) => [b.key, bands[b.key].map((v, i) => Math.max(1, v + off[i] + (si === 0 || si === 3 ? 1.5 : -1)))]))];
    })) as SessionSamples["sensors"];
    const contact = Object.fromEntries(SENSORS.map(({ key }, si) => [key, Math.round(99 - r() * 7 - (si === 0 ? 4 : 0))])) as SessionSamples["contact"];
    const startIso = `${day}T${start}:00+09:00`;
    const end = new Date(new Date(startIso).getTime() + min * 60_000).toISOString();
    return {
      id: `sample-${day}`, device_type: "muse", start: startIso, end, duration_secs: min * 60, data_points: n * 256, sample: true,
      summary: { avg_focus: mean(focus), avg_relaxation: mean(calm) },
      samples: { stepSecs: step, bands, focus, calm, sensors, contact },
    };
  }).reverse();
}
