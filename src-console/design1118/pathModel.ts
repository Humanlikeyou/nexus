import type { Roadmap, RoadmapStep, StepSize } from "../goals/roadmap";
import { FIB, GOLDEN_ANGLE, PHI } from "./golden";

/**
 * design-1118a, the Path (Direction A). Pure model: which station is done, current (You are here),
 * next, upcoming or a ghost, and the one progress meaning (roadmap percent + steps done). Nothing
 * here invents a percent: the percent comes from the roadmap store; after a local answer the glance
 * shows steps only until the store re-reads.
 */
export type StationState = "done" | "current" | "next" | "upcoming" | "ghost";
export type Station = { id: string; title: string | null; size: StepSize; state: StationState };
export type PathPhase = "unwired" | "planning" | "ready";

export const STATION_RADIUS: Record<StepSize, number> = { small: 6, medium: 9, big: 13 };
export const SIZE_LABEL: Record<StepSize, string> = { small: "Small", medium: "Medium", big: "Big" };

/** Planning = Amara hasn't decided the steps yet (no roadmap, drafting, or a roadmap with no steps): a quiet seed, no path. */
export function pathPhase(rm: Roadmap | null | undefined, unwired: boolean): PathPhase {
  if (unwired) return "unwired";
  if (!rm || rm.status === "drafting" || rm.steps.length === 0) return "planning";
  return "ready";
}

export function stationsOf(steps: RoadmapStep[]): Station[] {
  const ordered = steps.slice().sort((a, b) => a.order - b.order);
  let open = 0;
  return ordered.map((s) => {
    if (s.done) return { id: s.id, title: s.title, size: s.size, state: "done" as const };
    open += 1;
    return { id: s.id, title: s.title, size: s.size, state: open === 1 ? "current" as const : open === 2 ? "next" as const : "upcoming" as const };
  });
}

/** Anonymous stations for a goal whose step names we don't have (counts only, never names). */
export function anonymousStations(total: number, done = 0): Station[] {
  return Array.from({ length: Math.max(0, total) }, (_, i) => ({
    id: `anon-${i}`, title: null, size: "medium" as const,
    state: i < done ? "done" as const : i === done ? "current" as const : i === done + 1 ? "next" as const : "upcoming" as const,
  }));
}

export type Glance = { pct: number | null; done: number; total: number; text: string; short: string };

/** One progress meaning: roadmap percent and steps done together ("11% · 1 of 9"). With `stepsOnly`, "2 of 9 steps". */
export function glanceOf(rm: Roadmap | null | undefined, stations: Station[], stepsOnly = false): Glance {
  const total = stations.filter((s) => s.state !== "ghost").length;
  const done = stations.filter((s) => s.state === "done").length;
  if (!rm || total === 0) return { pct: null, done: 0, total: 0, text: "No route yet", short: "—" };
  if (stepsOnly) return { pct: null, done, total, text: `${done} of ${total} steps`, short: `${done}/${total}` };
  const pct = Math.round(Math.max(0, Math.min(100, rm.pct)));
  return { pct, done, total, text: `${pct}% · ${done} of ${total}`, short: `${pct}%` };
}

export const currentStation = (stations: Station[]) => stations.find((s) => s.state === "current") ?? null;

/** Accessible summary of a path: progress and where he is. */
export function pathLabel(goal: string, g: Glance, stations: Station[]): string {
  const here = currentStation(stations);
  if (g.total === 0) return `${goal}: Amara is planning the route`;
  const where = here ? `You are here: ${here.title ?? `step ${stations.indexOf(here) + 1}`}` : "All steps done";
  return `${goal}: ${g.text}. ${where}.`;
}

// ---- geometry (viewBox units) ----
export type Pt = { x: number; y: number };

/** Station positions along a gentle winding line, evenly spaced in x. Deterministic per seed. */
export function stationPoints(n: number, w = 320, h = 96, seed = 0, pad = 16): Pt[] {
  if (n <= 0) return [];
  if (n === 1) return [{ x: w / 2, y: h / 2 }];
  return Array.from({ length: n }, (_, i) => {
    const t = i / (n - 1);
    return { x: pad + t * (w - 2 * pad), y: h / 2 + (h * 0.26) * Math.sin(t * Math.PI * 2.2 + seed * 1.3 + 0.6) };
  });
}

/** Smooth Catmull-Rom curve through the points, as an SVG path. */
export function smoothPath(pts: Pt[]): string {
  if (pts.length === 0) return "";
  if (pts.length === 1) return `M${pts[0].x},${pts[0].y}`;
  let d = `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] ?? pts[i], p1 = pts[i], p2 = pts[i + 1], p3 = pts[i + 2] ?? p2;
    const c1 = { x: p1.x + (p2.x - p0.x) / 6, y: p1.y + (p2.y - p0.y) / 6 };
    const c2 = { x: p2.x - (p3.x - p1.x) / 6, y: p2.y - (p3.y - p1.y) / 6 };
    d += ` C${c1.x.toFixed(1)},${c1.y.toFixed(1)} ${c2.x.toFixed(1)},${c2.y.toFixed(1)} ${p2.x.toFixed(1)},${p2.y.toFixed(1)}`;
  }
  return d;
}

/** Fraction of the path filled: up to the current station (evenly spaced in x, so index / (n-1)). */
export function fillFraction(stations: Station[]): number {
  const live = stations.filter((s) => s.state !== "ghost");
  if (live.length <= 1) return live.length === 1 && live[0].state === "done" ? 1 : 0;
  const here = live.findIndex((s) => s.state === "current");
  return here < 0 ? 1 : here / (live.length - 1);
}

/**
 * A tangled strand for the Milestones arrival view. Periodic every 680 units, so a strand twice as wide loops seamlessly.
 * One wave per goal, in code only (no visible geometry): each goal gets a distinct Fibonacci fundamental
 * (2, 3, 5, 8 — integers so the loop stays seamless) plus quieter overtones at the next two Fibonacci numbers,
 * amplitudes in φ² steps, and a golden-angle (137.5°) phase offset so neighbouring strands weave rather than pile.
 * Period and amplitude breathing on T, Tφ, Tφ² live on the strand in CSS, not in this path.
 */
export function strandTerms(i: number) {
  const f = i % 4;
  return { k1: FIB[f + 2], k2: FIB[f + 3], k3: FIB[f + 4], phase: i * GOLDEN_ANGLE };
}

export function strandPath(i: number, w = 680, h = 120, phase = 0, samples = 96): string {
  const P = 680, { k1, k2, k3, phase: ph } = strandTerms(i);
  const a1 = h * 0.22, a2 = a1 / (PHI * PHI), a3 = a2 / (PHI * PHI);
  const pts: Pt[] = Array.from({ length: samples }, (_, k) => {
    const x = (k / (samples - 1)) * w, u = (2 * Math.PI * x) / P;
    const y = h / 2 + a1 * Math.sin(k1 * u + ph + phase) + a2 * Math.sin(k2 * u + ph * PHI - phase / PHI) + a3 * Math.sin(k3 * u + ph * PHI * PHI + phase / (PHI * PHI));
    return { x, y };
  });
  return smoothPath(pts);
}
