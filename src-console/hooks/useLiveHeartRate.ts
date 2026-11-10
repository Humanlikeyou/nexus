import { useEffect, useState } from "react";
import { getLiveHeartRate, type HeartSample, type LiveHeartRate } from "../bridge";
import { visibleSamples } from "../lib/heartTrace";

/** NOOP delivers samples in batches about every 15 seconds, so polling faster shows nothing new. */
export const LIVE_HEART_RATE_POLL_MS = 5_000;
/** Each batch plays back one second at a time, so the line moves steadily instead of jumping. */
export const PLAYBACK_TICK_MS = 1_000;
/** Further behind than this, skip ahead rather than show old beats as live. */
const MAX_BEHIND_SECONDS = 34;
const MIN_BEAT_S = 0.25;
const MAX_BEAT_S = 2;

export interface HeartTraceView { bpm: number; at: number; samples: HeartSample[] }

/** Seconds per beat for the pulse animation, clamped so a bad value can never strobe.
 *  Null when there's no usable heart rate (missing, zero, negative or NaN), so the caller hides the pulse. */
export function beatSeconds(bpm: number | null | undefined): number | null {
  if (typeof bpm !== "number" || !Number.isFinite(bpm) || bpm <= 0) return null;
  return Math.min(MAX_BEAT_S, Math.max(MIN_BEAT_S, 60 / bpm));
}

/** The next playback position: one second on, never past the newest sample. */
export function nextPlayhead(previous: number | null, newest: number): number {
  if (previous == null || previous > newest || newest - previous > MAX_BEHIND_SECONDS) return newest;
  return Math.min(previous + 1, newest);
}

/** The live trace, or null when there's no live reading. Stops polling while the window is hidden. */
export function useLiveHeartRate(): HeartTraceView | null {
  const [feed, setFeed] = useState<LiveHeartRate | null>(null);
  const [playhead, setPlayhead] = useState<number | null>(null);
  const newest = feed?.at ?? null;

  useEffect(() => {
    let current = true;
    const check = () => {
      if (document.hidden) return;
      getLiveHeartRate().then(
        (next) => { if (current) setFeed(next ?? null); },
        () => { if (current) setFeed(null); },
      );
    };
    check();
    const timer = window.setInterval(check, LIVE_HEART_RATE_POLL_MS);
    document.addEventListener("visibilitychange", check);
    return () => { current = false; window.clearInterval(timer); document.removeEventListener("visibilitychange", check); };
  }, []);

  useEffect(() => {
    if (newest == null) return undefined;
    const tick = () => setPlayhead((previous) => nextPlayhead(previous, newest));
    // The playback clock pauses while the window is hidden and catches up when it's shown again.
    let timer: number | undefined;
    const start = () => { if (timer === undefined && !document.hidden) timer = window.setInterval(tick, PLAYBACK_TICK_MS); };
    const stop = () => { if (timer !== undefined) { window.clearInterval(timer); timer = undefined; } };
    const onVisibility = () => { if (document.hidden) stop(); else { tick(); start(); } };
    const first = window.setTimeout(tick, 0);
    start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => { window.clearTimeout(first); stop(); document.removeEventListener("visibilitychange", onVisibility); };
  }, [newest]);

  if (!feed || playhead == null) return null;
  const samples = visibleSamples(feed.trace ?? [], playhead);
  const last = samples[samples.length - 1];
  return last ? { bpm: last.bpm, at: last.at, samples } : null;
}
