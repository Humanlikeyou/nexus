import { isSnapshot, noopQuery } from "./noop";
import type { NoopSnapshotPayload } from "../types";

/**
 * One shared read of health_snapshot (29 days) for everything on the Mind-Body page: the page's own
 * load publishes its snapshot here and the Amara take reads it, so the snapshot is fetched once.
 * Shares an in-flight read and a recent one (FRESH_MS); a failed or malformed read is never kept.
 * In memory only.
 */
const FRESH_MS = 30_000;
let shared: { promise: Promise<NoopSnapshotPayload | null>; at: number } | null = null;

/** Publish a snapshot read that is already in flight (the page's batch). */
export function shareSnapshot(promise: Promise<NoopSnapshotPayload | null>): void {
  const entry = { promise, at: Date.now() };
  shared = entry;
  void promise.then((v) => { if (!v && shared === entry) shared = null; }, () => { if (shared === entry) shared = null; });
}

/** The snapshot: the shared read if there is one in flight or fresh, else a new single read. */
export function readSnapshot(): Promise<NoopSnapshotPayload | null> {
  if (shared && Date.now() - shared.at < FRESH_MS) return shared.promise;
  const promise = noopQuery("health_snapshot", { days: 29 }).then((r) => (r.ok && isSnapshot(r.value) ? r.value : null));
  shareSnapshot(promise);
  return promise;
}

/** Forget the shared read (tests, and a manual Retry). */
export function resetSharedSnapshot(): void { shared = null; }
