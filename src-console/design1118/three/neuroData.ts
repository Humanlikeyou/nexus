import { hasSamples, sessionStats, type SessionWithSamples } from "../../neuro/sessionSamples";
import type { RecordedSession } from "../../types";

/** Delta to gamma, the order the ring runs clockwise from the top. */
export const RING_BANDS = [
  { key: "delta", name: "Delta", range: "1–4 Hz" },
  { key: "theta", name: "Theta", range: "4–8 Hz" },
  { key: "alpha", name: "Alpha", range: "8–13 Hz" },
  { key: "beta", name: "Beta", range: "13–30 Hz" },
  { key: "gamma", name: "Gamma", range: "30–44 Hz" },
] as const;
export type RingKey = (typeof RING_BANDS)[number]["key"];
export type Shares = Record<RingKey, number>;
export type Range = { lo: number; hi: number };
export type Place = "above" | "below" | "within";

export const USUAL_MAX_SESSIONS = 30;
export const USUAL_MIN_SESSIONS = 3;

const finite = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const round1 = (v: number) => Math.round(v * 10) / 10;

/** Each band's share of the five bands' total power, 0–100 (sums to 100). null when there is nothing positive to share. */
export function bandShares(avg: Partial<Record<RingKey, number>> | null | undefined): Shares | null {
  if (!avg) return null;
  const vals = RING_BANDS.map((b) => avg[b.key]);
  if (!vals.every((v) => finite(v) && v >= 0)) return null;
  const total = (vals as number[]).reduce((a, b) => a + b, 0);
  if (!(total > 0)) return null;
  return Object.fromEntries(RING_BANDS.map((b, i) => [b.key, round1(((vals[i] as number) / total) * 100)])) as Shares;
}

/** Shares of one saved session, or null when it has no band data. Read-only: uses the loaded samples only. */
export function sessionShares(s: RecordedSession | null | undefined): Shares | null {
  if (!s) return null;
  const w = s as SessionWithSamples;
  return hasSamples(w) ? bandShares(sessionStats(w.samples).bandAvg) : null;
}

/** Linear-interpolated percentile of an ascending list. */
export function percentile(sorted: readonly number[], p: number): number {
  if (!sorted.length) return NaN;
  const pos = (sorted.length - 1) * p, lo = Math.floor(pos), hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

/** "Usual" = the middle half (25th to 75th percentile) of the values. Fewer than `min` values: no range. */
export function usualOf(values: readonly (number | null | undefined)[], min = USUAL_MIN_SESSIONS): Range | null {
  const v = values.filter(finite).slice().sort((a, b) => a - b);
  if (v.length < min) return null;
  return { lo: round1(percentile(v, 0.25)), hi: round1(percentile(v, 0.75)) };
}

const recent = (sessions: readonly RecordedSession[], excludeId?: string) =>
  sessions.filter((s) => s.id !== excludeId).slice().sort((a, b) => Date.parse(b.start) - Date.parse(a.start));

/** His usual share per band from his last ≤30 sessions that have band data. `ranges` is null with fewer than 3 such sessions. */
export function usualBandRanges(sessions: readonly RecordedSession[], excludeId?: string): { ranges: Record<RingKey, Range> | null; count: number } {
  const all = recent(sessions, excludeId).map(sessionShares).filter((s): s is Shares => s !== null).slice(0, USUAL_MAX_SESSIONS);
  if (all.length < USUAL_MIN_SESSIONS) return { ranges: null, count: all.length };
  const ranges = Object.fromEntries(RING_BANDS.map((b) => [b.key, usualOf(all.map((s) => s[b.key]))])) as Record<RingKey, Range>;
  return { ranges, count: all.length };
}

/** Usual of a summary number (focus, calm, minutes) over the last ≤30 other sessions that carry it. */
export function usualSummary(sessions: readonly RecordedSession[], pick: (s: RecordedSession) => number | null | undefined, excludeId?: string): Range | null {
  return usualOf(recent(sessions, excludeId).map(pick).filter(finite).slice(0, USUAL_MAX_SESSIONS));
}

export const placeOf = (v: number, r: Range | null | undefined): Place | null => (!r ? null : v > r.hi ? "above" : v < r.lo ? "below" : "within");
export const placeWords = (p: Place | null) => (p === "above" ? "above your usual" : p === "below" ? "below your usual" : p === "within" ? "within your usual" : "");

export type BandRow = { key: RingKey; name: string; range: string; share: number; usual: Range | null; place: Place | null };
export function bandRows(shares: Shares, ranges: Record<RingKey, Range> | null): BandRow[] {
  return RING_BANDS.map((b) => ({ key: b.key, name: b.name, range: b.range, share: shares[b.key], usual: ranges?.[b.key] ?? null, place: placeOf(shares[b.key], ranges?.[b.key]) }));
}

/** Bar height for a share (0–100) in ring units; 40% is the full-height mark like the approved mock-up. */
export const shareHeight = (v: number) => Math.min(1.4, Math.max(0, v) / 40) * 0.62;

/** Annular sector angles for band i of 5, clockwise from 12 o'clock, with a small gap. */
export function sectorAngles(i: number, gap = 0.05): { a0: number; a1: number } {
  const step = (Math.PI * 2) / 5;
  return { a1: Math.PI / 2 - i * step - gap, a0: Math.PI / 2 - (i + 1) * step + gap };
}
