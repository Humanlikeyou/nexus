import type { HeartSample } from "../bridge";
import { monotonePath } from "../ui/charts/chartMath";

/** The trace's window in seconds, and its box: all Fibonacci, like the rest of Home. */
export const TRACE_SECONDS = 89;
export const TRACE_WIDTH = 233;
export const TRACE_HEIGHT = 55;
/** Room for the live dot's halo. */
const INSET = 8;
/** A resting heart barely moves, so the scale never spans less than this and a flat line stays flat. */
const MIN_SPAN_BPM = 21;
/** Past this the strap dropped out: the line breaks rather than joining across the gap. */
const MAX_GAP_SECONDS = 5;

export interface TraceShape { paths: string[]; areas?: string[]; dot: { x: number; y: number } | null }

/** The samples inside the window that ends at `at`, oldest first. */
export function visibleSamples(trace: HeartSample[], at: number): HeartSample[] {
  return trace.filter((sample) => sample.at <= at && sample.at > at - TRACE_SECONDS);
}

const round = (value: number) => Math.round(value * 10) / 10;

/** design-1110c C14: a 5-second average, so the line reads as a heart rate and not as a staircase. */
export const SMOOTH_SECONDS = 5;
function smooth(samples: HeartSample[]): number[] {
  return samples.map((sample) => {
    const near = samples.filter((other) => Math.abs(other.at - sample.at) <= SMOOTH_SECONDS / 2);
    return near.reduce((sum, { bpm }) => sum + bpm, 0) / near.length;
  });
}

/** A monotone curve through the run (never overshoots a beat), plus the soft fill under it. */
function curve(points: { x: number; y: number }[]): string {
  return monotonePath(points.map(({ x, y }) => [x, y] as const));
}

/** Draws the samples right to left from `at`. Lone samples between gaps have no line to draw. */
export function traceShape(samples: HeartSample[], at: number): TraceShape {
  if (samples.length < 2) return { paths: [], dot: null };
  const values = smooth(samples);
  const low = Math.min(...values);
  const span = Math.max(MIN_SPAN_BPM, Math.max(...values) - low);
  const middle = low + (Math.max(...values) - low) / 2;
  const plotWidth = TRACE_WIDTH - INSET * 2;
  const plotHeight = TRACE_HEIGHT - INSET * 2;
  const points = samples.map((sample, i) => ({
    x: round(INSET + plotWidth * (1 - (at - sample.at) / TRACE_SECONDS)),
    y: round(TRACE_HEIGHT / 2 - ((values[i] - middle) / span) * plotHeight),
  }));
  const runs: { x: number; y: number }[][] = [[]];
  points.forEach((point, i) => {
    if (i > 0 && samples[i].at - samples[i - 1].at > MAX_GAP_SECONDS) runs.push([]);
    runs[runs.length - 1].push(point);
  });
  const drawn = runs.filter((run) => run.length > 1);
  const floor = TRACE_HEIGHT - INSET / 2;
  return {
    paths: drawn.map(curve),
    areas: drawn.map((run) => `${curve(run)}L${run[run.length - 1].x},${floor}L${run[0].x},${floor}Z`),
    dot: points[points.length - 1],
  };
}
