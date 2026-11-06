import { useEffect, useState } from "react";
import { getLiveHeartRate } from "../bridge";

/** NOOP delivers samples in batches about every 15 seconds, so polling faster shows nothing new. */
export const LIVE_HEART_RATE_POLL_MS = 5_000;
const MIN_BEAT_S = 0.25;
const MAX_BEAT_S = 2;

/** Seconds per beat for the pulse animation, clamped so a bad value can never strobe. */
export function beatSeconds(bpm: number): number {
  return Math.min(MAX_BEAT_S, Math.max(MIN_BEAT_S, 60 / bpm));
}

/** The latest heart rate, or null when there's no live reading. Stops polling while the window is hidden. */
export function useLiveHeartRate(): number | null {
  const [bpm, setBpm] = useState<number | null>(null);
  useEffect(() => {
    let current = true;
    const check = () => {
      if (document.hidden) return;
      getLiveHeartRate().then(
        (next) => { if (current) setBpm(next?.bpm ?? null); },
        () => { if (current) setBpm(null); },
      );
    };
    check();
    const timer = window.setInterval(check, LIVE_HEART_RATE_POLL_MS);
    document.addEventListener("visibilitychange", check);
    return () => { current = false; window.clearInterval(timer); document.removeEventListener("visibilitychange", check); };
  }, []);
  return bpm;
}
