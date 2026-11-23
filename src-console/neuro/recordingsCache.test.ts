import { describe, expect, it, vi } from "vitest";
import { BLOCK_POINTS, MAX_BLOCKS, createRecordingsCache, type RecordingsFetchers } from "./recordingsCache";
import type { RecordingWindowIpc } from "../bridge";

const win = (start: number): RecordingWindowIpc => ({ startSecs: start, endSecs: start + 30, sampleRate: 256, lanes: [{ channel: "TP9", contact: "good", values: [start] }] });
const make = (over: Partial<RecordingsFetchers> = {}) => {
  const f = {
    list: vi.fn(async () => []),
    eeg: vi.fn(async (_id: string, s: number, _span?: number, _points?: number) => win(s)),
    body: vi.fn(async () => ({ startSecs: 0, endSecs: 30, heart: null, motion: null })),
    series: vi.fn(async () => ({ stepSecs: 5, bands: { delta: [], theta: [], alpha: [], beta: [], gamma: [] }, focus: [], calm: [] })),
  };
  return { f, cache: createRecordingsCache({ ...f, ...over }) };
};
const settle = () => new Promise((r) => setTimeout(r, 0));

describe("recordings cache", () => {
  it("de-dupes in-flight requests and serves the block once loaded", async () => {
    const { f, cache } = make();
    expect(cache.block("eeg", "a", 0).state).toBe("loading");
    expect(cache.block("eeg", "a", 0).state).toBe("loading");
    expect(f.eeg).toHaveBeenCalledTimes(1);
    expect(f.eeg).toHaveBeenCalledWith("a", 0, 30, BLOCK_POINTS);
    await settle();
    expect(cache.block("eeg", "a", 0).state).toBe("ready");
    expect(f.eeg).toHaveBeenCalledTimes(1);
  });

  it("notifies subscribers (version bump) when a load finishes", async () => {
    const { cache } = make(); const seen = vi.fn(); cache.subscribe(seen);
    const v = cache.getVersion(); cache.block("eeg", "a", 0); await settle();
    expect(seen).toHaveBeenCalled(); expect(cache.getVersion()).toBeGreaterThan(v);
  });

  it("keeps at most 8 blocks, evicting the least recently used", async () => {
    const { f, cache } = make();
    for (let i = 0; i < MAX_BLOCKS; i += 1) cache.block("eeg", "a", i);
    await settle();
    expect(cache.stats().blocks).toBe(8);
    cache.block("eeg", "a", 0); // touch block 0: block 1 is now the oldest
    cache.block("eeg", "a", 8); await settle();
    expect(cache.stats().blocks).toBe(8);
    expect(cache.block("eeg", "a", 0).state).toBe("ready");
    const calls = f.eeg.mock.calls.length;
    expect(cache.block("eeg", "a", 1).state).toBe("loading"); // evicted, reloads
    expect(f.eeg.mock.calls.length).toBe(calls + 1);
  });

  it("remembers a failure instead of retrying on every read, until retry()", async () => {
    const eeg = vi.fn(async () => { throw new Error("nope"); });
    const { cache } = make({ eeg });
    cache.block("eeg", "a", 0); await settle();
    expect(cache.block("eeg", "a", 0).state).toBe("failed");
    expect(cache.block("eeg", "a", 0).state).toBe("failed");
    expect(eeg).toHaveBeenCalledTimes(1);
    cache.retry();
    expect(cache.block("eeg", "a", 0).state).toBe("loading");
    expect(eeg).toHaveBeenCalledTimes(2);
  });

  it("prefetches the next block near the end of this one, only if the recording has it", async () => {
    const { f, cache } = make();
    cache.block("eeg", "a", 0, { windowEnd: 10, durationSecs: 100 }); await settle();
    expect(f.eeg).toHaveBeenCalledTimes(1);
    cache.block("eeg", "a", 0, { windowEnd: 26, durationSecs: 100 }); await settle();
    expect(f.eeg).toHaveBeenCalledTimes(2);
    expect(f.eeg).toHaveBeenLastCalledWith("a", 30, 30, BLOCK_POINTS);
    cache.block("eeg", "b", 0, { windowEnd: 28, durationSecs: 29 }); await settle();
    expect(f.eeg).toHaveBeenCalledTimes(3); // b/0 only: no block 1 past the end
  });

  it("list and series are loaded once each", async () => {
    const { f, cache } = make();
    cache.list(); cache.list(); cache.series("a"); cache.series("a"); await settle();
    expect(f.list).toHaveBeenCalledTimes(1); expect(f.series).toHaveBeenCalledTimes(1);
  });
});
