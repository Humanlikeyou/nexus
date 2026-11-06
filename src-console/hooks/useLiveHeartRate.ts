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

/** Seconds per beat for the pulse animation, clamped so a bad value can never strobe. */
export function beatSeconds(bpm: number): number {
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
    const first = window.setTimeout(tick, 0);
    const timer = window.setInterval(tick, PLAYBACK_TICK_MS);
    return () => { window.clearTimeout(first); window.clearInterval(timer); };
  }, [newest]);

  if (!feed || playhead == null) return null;
  const samples = visibleSamples(feed.trace ?? [], playhead);
  const last = samples[samples.length - 1];
  return last ? { bpm: last.bpm, at: last.at, samples } : null;
}
