import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import type { RecordedSession } from "../types";
import { recordingsAdapter, type RecordingsAdapter } from "./recordings";
import { recordingsCache, type RecordingsCache } from "./recordingsCache";

/**
 * The recordings adapter over the read-through cache. Re-renders when a block, series or the list
 * finishes loading. `enabled=false` (a caller supplied its own adapter) skips the cache entirely.
 */
export function useRecordingsAdapter(sessions: RecordedSession[], enabled = true, cache: RecordingsCache = recordingsCache): RecordingsAdapter {
  const version = useSyncExternalStore(cache.subscribe, cache.getVersion);
  const count = useRef(sessions.length);
  useEffect(() => {
    // A saved session adds a recording: reload the list (not on first render).
    if (count.current !== sessions.length) { count.current = sessions.length; if (enabled) cache.refreshList(); }
  }, [sessions.length, enabled, cache]);
  // `version` is a dependency on purpose: new loaded data means a new adapter.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => recordingsAdapter(sessions, enabled ? cache : undefined), [sessions, enabled, cache, version]);
}
