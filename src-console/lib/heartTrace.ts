import type { HeartSample } from "../bridge";

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

export interface TraceShape { paths: string[]; dot: { x: number; y: number } | null }

/** The samples inside the window that ends at `at`, oldest first. */
export function visibleSamples(trace: HeartSample[], at: number): HeartSample[] {
  return trace.filter((sample) => sample.at <= at && sample.at > at - TRACE_SECONDS);
}

const round = (value: number) => Math.round(value * 10) / 10;

/** One soft pass over the raw beats, so the line reads as a line and not as a staircase. */
function smooth(samples: HeartSample[]): number[] {
  return samples.map((sample, i) => {
    const near = samples.slice(Math.max(0, i - 1), i + 2);
    return near.reduce((sum, { bpm }) => sum + bpm, 0) / near.length;
  });
}

function curve(points: { x: number; y: number }[]): string {
  const [first, ...rest] = points;
  let path = `M${first.x} ${first.y}`;
  rest.forEach((point, i) => {
    const previous = points[i];
    const midX = round((previous.x + point.x) / 2);
    const midY = round((previous.y + point.y) / 2);
    path += i === 0 ? ` L${midX} ${midY}` : ` Q${previous.x} ${previous.y} ${midX} ${midY}`;
  });
  const last = points[points.length - 1];
  return rest.length ? `${path} L${last.x} ${last.y}` : path;
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
  return { paths: runs.filter((run) => run.length > 1).map(curve), dot: points[points.length - 1] };
}
