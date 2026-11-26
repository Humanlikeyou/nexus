import type { HrWindow } from "../../hrTake/adapters";

export type HrPoint = { i: number; bpm: number };
export type HrGap = { from: number; to: number; minutes: number };
export type HrShape = {
  n: number; lo: number; hi: number;
  /** Runs of consecutive minutes that have readings; a gap (no reading) always splits a run. */
  runs: HrPoint[][];
  gaps: HrGap[];
  last: HrPoint | null;
  usual: { lo: number; hi: number } | null;
};

/** Pure geometry of the 15-minute take graph in data space: index along the window and bpm. Never joins across a gap. */
export function hrShape(w: HrWindow): HrShape {
  const pm = w.perMinute;
  const runs: HrPoint[][] = []; let cur: HrPoint[] = [];
  pm.forEach((m, i) => { if (m.bpm == null || !Number.isFinite(m.bpm)) { if (cur.length) runs.push(cur); cur = []; } else cur.push({ i, bpm: m.bpm }); });
  if (cur.length) runs.push(cur);
  const gaps: HrGap[] = [];
  for (let k = 1; k < runs.length; k++) {
    const from = runs[k - 1][runs[k - 1].length - 1].i, to = runs[k][0].i;
    gaps.push({ from, to, minutes: to - from - 1 });
  }
  const vals = runs.flat().map((p) => p.bpm);
  const lo = Math.min(...vals, w.usual?.lo ?? Infinity) - 3, hi = Math.max(...vals, w.usual?.hi ?? -Infinity) + 3;
  const lastRun = runs[runs.length - 1];
  return { n: Math.max(1, pm.length - 1), lo: Number.isFinite(lo) ? lo : 0, hi: Number.isFinite(hi) ? hi : 1, runs, gaps, last: lastRun ? lastRun[lastRun.length - 1] : null, usual: w.usual ? { lo: w.usual.lo, hi: w.usual.hi } : null };
}

export const aboveUsual = (bpm: number, s: HrShape) => !!s.usual && bpm > s.usual.hi;
export const gapLabel = (g: HrGap) => (g.minutes <= 1 ? "No signal" : `No signal · ${g.minutes} min`);

/** Plain-language facts for the visually hidden alternative: same facts the graph draws. */
export function hrFacts(w: HrWindow, s: HrShape): string[] {
  const out: string[] = [];
  if (!s.runs.length) return ["No heart-rate readings in this window."];
  out.push(`Last reading ${s.last?.bpm} bpm${s.usual ? (s.last!.bpm > s.usual.hi ? ", above your usual" : s.last!.bpm < s.usual.lo ? ", below your usual" : ", within your usual") : ""}.`);
  if (s.usual) out.push(`Your usual band is ${s.usual.lo} to ${s.usual.hi} bpm.`);
  for (const g of s.gaps) out.push(`No signal between minute ${g.from + 1} and minute ${g.to + 1} of the window (${g.minutes} min without a reading).`);
  return out;
}
