import { listRecordings, readBodyWindow, readRecordingWindow, recordingSeries, type BodyWindowIpc, type RecordingListItemIpc, type RecordingWindowIpc, type SessionSeriesIpc } from "../bridge";

/**
 * W12c Chunk C: a small read-through cache so the (synchronous) recordings adapter can serve
 * windows that were loaded asynchronously from the read-only Rust readers.
 * - 30 s blocks, LRU 8 (EEG and body blocks share the budget).
 * - One request per key at a time (in-flight de-dupe).
 * - A failure is remembered (no retry loop from re-renders) until `retry()`.
 * - Reading a block near its end prefetches the next one (only if the recording is known to have it).
 * Nothing here writes or deletes anything: it only calls the read commands.
 */
export const BLOCK_SECS = 30;
export const MAX_BLOCKS = 8;
/** Prefetch the next block when the window ends within this many seconds of the block's end. */
export const PREFETCH_SECS = 6;
/** Highest point count read_recording_window allows (Rust clamps to 4096). */
export const BLOCK_POINTS = 4096;

export type BlockKind = "eeg" | "body";
export type CacheEntry<T> = { state: "ready"; value: T } | { state: "loading" } | { state: "failed" };

export interface RecordingsFetchers {
  list(): Promise<RecordingListItemIpc[]>;
  eeg(id: string, startSecs: number, spanSecs: number, points: number): Promise<RecordingWindowIpc>;
  body(id: string, startSecs: number, spanSecs: number): Promise<BodyWindowIpc>;
  series(id: string): Promise<SessionSeriesIpc>;
}

export const bridgeFetchers: RecordingsFetchers = {
  list: listRecordings,
  eeg: readRecordingWindow,
  body: readBodyWindow,
  series: recordingSeries,
};

type Loaded = RecordingWindowIpc | BodyWindowIpc | SessionSeriesIpc | RecordingListItemIpc[];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
/** A reply of the wrong shape is a failure, not data (a malformed reply must never reach the adapter). */
const checked = <T extends Loaded>(ok: (v: unknown) => boolean) => async (p: Promise<T>): Promise<T> => { const v = await p; if (!ok(v)) throw new Error("unexpected reply"); return v; };
const okList = checked<RecordingListItemIpc[]>((v) => Array.isArray(v));
const okEeg = checked<RecordingWindowIpc>((v) => isObj(v) && Array.isArray(v.lanes));
const okBody = checked<BodyWindowIpc>((v) => isObj(v));
const okSeries = checked<SessionSeriesIpc>((v) => isObj(v) && isObj(v.bands) && Array.isArray(v.focus) && Array.isArray(v.calm));

export function createRecordingsCache(fetchers: RecordingsFetchers = bridgeFetchers) {
  const store = new Map<string, Loaded>(); // ready values; blocks are in LRU order (oldest first)
  const failed = new Set<string>();
  const inflight = new Map<string, Promise<void>>();
  const listeners = new Set<() => void>();
  let version = 0;

  const notify = () => { version += 1; for (const l of listeners) l(); };
  const blockKey = (kind: BlockKind, id: string, block: number) => `${kind}\u0000${id}\u0000${block}`;
  const isBlockKey = (k: string) => k.startsWith("eeg\u0000") || k.startsWith("body\u0000");

  const evict = () => {
    const blocks = [...store.keys()].filter(isBlockKey);
    for (let i = 0; i < blocks.length - MAX_BLOCKS; i += 1) store.delete(blocks[i]);
  };

  const load = (key: string, run: () => Promise<Loaded>): void => {
    if (store.has(key) || failed.has(key) || inflight.has(key)) return;
    const p = run().then(
      (value) => { store.set(key, value); if (isBlockKey(key)) evict(); },
      () => { failed.add(key); },
    ).then(() => { inflight.delete(key); notify(); });
    inflight.set(key, p);
  };

  const entry = <T,>(key: string): CacheEntry<T> => {
    if (store.has(key)) {
      const v = store.get(key) as T;
      if (isBlockKey(key)) { store.delete(key); store.set(key, v as Loaded); } // touch: most recently used
      return { state: "ready", value: v };
    }
    return failed.has(key) ? { state: "failed" } : { state: "loading" };
  };

  return {
    subscribe(l: () => void) { listeners.add(l); return () => { listeners.delete(l); }; },
    getVersion: () => version,

    /** Kicks off the list load if needed; returns what is known now. */
    list(): CacheEntry<RecordingListItemIpc[]> {
      load("list", () => okList(fetchers.list()));
      return entry("list");
    },
    series(id: string): CacheEntry<SessionSeriesIpc> {
      const key = `series\u0000${id}`;
      load(key, () => okSeries(fetchers.series(id)));
      return entry(key);
    },
    /**
     * One 30 s block (index = floor(start / 30)). `durationSecs`, when known, lets it prefetch the next
     * block as the caller nears this block's end (`windowEnd` in seconds).
     */
    block<K extends BlockKind>(kind: K, id: string, index: number, opts: { windowEnd?: number; durationSecs?: number } = {}): CacheEntry<K extends "eeg" ? RecordingWindowIpc : BodyWindowIpc> {
      const start = index * BLOCK_SECS;
      const fetchBlock = (i: number) => {
        const s = i * BLOCK_SECS;
        load(blockKey(kind, id, i), () => (kind === "eeg" ? okEeg(fetchers.eeg(id, s, BLOCK_SECS, BLOCK_POINTS)) : okBody(fetchers.body(id, s, BLOCK_SECS))));
      };
      fetchBlock(index);
      const { windowEnd, durationSecs } = opts;
      if (windowEnd !== undefined && durationSecs !== undefined && start + BLOCK_SECS - windowEnd <= PREFETCH_SECS && start + BLOCK_SECS < durationSecs) fetchBlock(index + 1);
      return entry(blockKey(kind, id, index));
    },
    /** The most recently used ready EEG block of a recording (for contact), without touching the LRU order. */
    latestEeg(id: string): RecordingWindowIpc | null {
      const prefix = `eeg\u0000${id}\u0000`;
      const keys = [...store.keys()].filter((k) => k.startsWith(prefix));
      return keys.length ? (store.get(keys[keys.length - 1]) as RecordingWindowIpc) : null;
    },
    /** The saved-session list changed: reload the recordings list (blocks stay valid, files are append-only per id). */
    refreshList() { store.delete("list"); failed.delete("list"); notify(); },
    /** Clears remembered failures so the next read tries again (e.g. a Retry button). */
    retry() { if (failed.size) { failed.clear(); notify(); } },
    /** Drops everything (a new recording was saved, the list is stale). */
    invalidate() { store.clear(); failed.clear(); notify(); },
    /** Test seam. */
    stats: () => ({ blocks: [...store.keys()].filter(isBlockKey).length, inflight: inflight.size, failed: failed.size }),
  };
}

export type RecordingsCache = ReturnType<typeof createRecordingsCache>;
/** App-wide cache. */
export const recordingsCache: RecordingsCache = createRecordingsCache();
