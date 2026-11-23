/**
 * Shared failure backoff for scheduled work (scheduler jobs, ritual sends).
 * Retry after 5, 15, 60 min, max 3 tries per window. No fixed 5-minute retry loop.
 */
export const BACKOFF_DELAYS_MS = [5 * 60_000, 15 * 60_000, 60 * 60_000] as const;
export const MAX_ATTEMPTS_PER_WINDOW = 3;

interface FailureState {
  count: number;
  nextAt: number;
  window: string;
}

/** In-memory failure tracker keyed by job/ritual id. Counters reset when the window changes. */
export class Backoff<K extends string = string> {
  private failures = new Map<K, FailureState>();

  inBackoff(id: K, now: number, window: string): boolean {
    const f = this.failures.get(id);
    if (!f) return false;
    if (f.window !== window) {
      this.failures.delete(id);
      return false;
    }
    return f.count >= MAX_ATTEMPTS_PER_WINDOW || now < f.nextAt;
  }

  recordFailure(id: K, now: number, window: string): void {
    const prev = this.failures.get(id);
    const count = prev && prev.window === window ? prev.count + 1 : 1;
    const delay = BACKOFF_DELAYS_MS[Math.min(count - 1, BACKOFF_DELAYS_MS.length - 1)];
    this.failures.set(id, { count, nextAt: now + delay, window });
  }

  /** Failures recorded for `id` in `window` (0 for none, or for another window). */
  failureCount(id: K, window: string): number {
    const f = this.failures.get(id);
    return f && f.window === window ? f.count : 0;
  }

  recordSuccess(id: K): void {
    this.failures.delete(id);
  }
}
