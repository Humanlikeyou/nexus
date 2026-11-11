import { afterEach, describe, expect, it, vi } from "vitest";
import { BLOCK_POINTS, createRecordingsCache, type RecordingsFetchers } from "./recordingsCache";
import { liveRecordings } from "./recordings";
import type { BodyWindowIpc, RecordingListItemIpc, RecordingWindowIpc } from "../bridge";

const NOW = Date.parse("2026-11-10T18:00:00+09:00");
const RATE = BLOCK_POINTS / 30;
const item = (id: string, over: Partial<RecordingListItemIpc> = {}): RecordingListItemIpc => ({
  id, sessionId: `s-${id}`, replayFile: `${id}.ndjson`, startedAt: "2026-11-07T11:57:00+09:00", durationSecs: 100, protocol: null, channels: ["TP9", "AF7", "AF8", "TP10"], aux: null,
  hasHeart: false, hasMotion: false, avgFocus: 41, avgCalm: 52, timeInTarget: null, calmPct: null, raw: "available", retention: "kept", ...over,
});
// Block b has value (b*1000 + sampleIndex), so slices are easy to check.
const eegBlock = (start: number): RecordingWindowIpc => ({ startSecs: start, endSecs: start + 30, sampleRate: 256, lanes: [
  { channel: "TP9", contact: start === 30 ? "poor" : "good", values: Array.from({ length: BLOCK_POINTS }, (_, i) => (i === 5 && start === 0 ? null : (start / 30) * 10000 + i)) },
] });
const setup = (over: Partial<RecordingsFetchers> = {}) => {
  const f: RecordingsFetchers = {
    list: vi.fn(async () => [item("a"), item("sum", { raw: "summary-only", replayFile: null })]),
    eeg: vi.fn(async (_i: string, s: number) => eegBlock(s)),
    body: vi.fn(async (_i: string, s: number): Promise<BodyWindowIpc> => ({ startSecs: s, endSecs: s + 30, heart: { ppgRate: 64, values: Array.from({ length: 1920 }, (_, i) => i), bpm: 66, estimate: true }, motion: null })),
    series: vi.fn(async () => ({ stepSecs: 5, bands: { delta: [1, null], theta: [1, 2], alpha: [1, 2], beta: [1, 2], gamma: [1, 2] }, focus: [10, null], calm: [20, 30] })),
    ...over,
  };
  const cache = createRecordingsCache(f);
  return { f, cache, live: () => liveRecordings([], NOW, cache) };
};
const settle = () => new Promise((r) => setTimeout(r, 0));
afterEach(() => vi.restoreAllMocks());

describe("liveRecordings over the cache", () => {
  it("cold block is unwired, then live after the load", async () => {
    const { live } = setup();
    expect(live().window("a", 0, 5).state).toBe("unwired");
    await settle();
    const w = live().window("a", 0, 5, 10_000);
    expect(w.state).toBe("live");
    if (w.state !== "live") return;
    expect(w.value.lanes[0].values).toHaveLength(Math.round(5 * RATE));
    expect(w.value.lanes[0].values[0]).toBe(0);
    expect(Number.isNaN(w.value.lanes[0].values[5])).toBe(true); // a gap stays a gap
    expect(w.value.lanes[0].contact).toBe("good");
  });

  it("slices across a block boundary and needs both blocks (worst contact wins)", async () => {
    const { f, live } = setup();
    live().window("a", 28, 5); await settle();
    expect(f.eeg).toHaveBeenCalledWith("a", 30, 30, BLOCK_POINTS);
    const w = live().window("a", 28, 5, 100_000);
    if (w.state !== "live") throw new Error("expected live");
    const v = w.value.lanes[0].values;
    expect(v).toHaveLength(Math.round(2 * RATE) + Math.round(3 * RATE));
    expect(v[0]).toBe(Math.round(28 * RATE)); // block 0 tail
    expect(v[Math.round(2 * RATE)]).toBe(10000); // block 1 head
    expect(w.value.lanes[0].contact).toBe("poor");
  });

  it("a failed block stays unwired; a summary-only recording is never read", async () => {
    const eeg = vi.fn(async () => { throw new Error("x"); });
    const { f, live } = setup({ eeg });
    live().list(); await settle();
    live().window("a", 0, 5); await settle();
    expect(live().window("a", 0, 5).state).toBe("unwired");
    expect(eeg).toHaveBeenCalledTimes(1);
    expect(live().window("sum", 0, 5).state).toBe("unwired");
    expect(f.eeg).not.toHaveBeenCalledWith("sum", expect.anything(), expect.anything(), expect.anything());
  });

  it("list() and retention() fall back to the saved sessions until the list loads, then use the readers", async () => {
    const sess = [{ id: "x", device_type: "muse", start: "2026-02-18T22:51:00+09:00", end: "", duration_secs: 5, data_points: 0, summary: {} }];
    const { cache } = setup();
    const a = () => liveRecordings(sess, NOW, cache);
    const before = a().list(); if (before.state !== "live") throw new Error("live");
    expect(before.value.map((r) => r.id)).toEqual(["x"]);
    const ret = a().retention(); if (ret.state !== "live") throw new Error("live");
    expect(ret.value.expiring.map((r) => r.id)).toEqual(["x"]);
    await settle();
    const after = a().list(); if (after.state !== "live") throw new Error("live");
    expect(after.value.map((r) => r.id)).toEqual(["a", "sum"]);
    expect("sessionId" in after.value[0]).toBe(false);
  });

  it("body and series come through with gaps as NaN, and contact reads the loaded block", async () => {
    const { live } = setup();
    live().list(); await settle();
    live().body("a", 0, 5); live().series("a"); live().window("a", 0, 5); await settle();
    const b = live().body("a", 1, 2); if (b.state !== "live") throw new Error("live");
    expect(b.value.heart?.values).toHaveLength(128); expect(b.value.heart?.values[0]).toBe(64); expect(b.value.heart?.bpm).toBe(66); expect(b.value.motion).toBeNull();
    const s = live().series("a"); if (s.state !== "live") throw new Error("live");
    expect(s.value.focus[0]).toBe(10); expect(Number.isNaN(s.value.focus[1])).toBe(true);
    expect(live().contact("a")).toEqual({ state: "live", value: { TP9: "good" } });
  });

  it("without a cache every reader is unwired (no side effects)", () => {
    const a = liveRecordings([], NOW);
    expect([a.window("a", 0, 5), a.body("a", 0, 5), a.series("a"), a.contact("a")].map((w) => w.state)).toEqual(["unwired", "unwired", "unwired", "unwired"]);
  });
});

describe("removal stays unwired and nothing deletes", () => {
  it("removeConfirmed is UNWIRED and no remove/delete command is ever invoked", async () => {
    vi.resetModules();
    const invoke = vi.fn(async (cmd: string) => (cmd === "list_recordings" ? [item("a")] : cmd === "read_recording_window" ? eegBlock(0) : null));
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const { createRecordingsCache: make } = await import("./recordingsCache");
    const { liveRecordings: live } = await import("./recordings");
    const cache = make();
    const a = live([], NOW, cache);
    a.list(); a.window("a", 0, 5); a.series("a"); a.body("a", 0, 5); await settle();
    expect(a.removeConfirmed(["a"], { confirmedByUser: true, at: "now" }).state).toBe("unwired");
    expect(invoke).toHaveBeenCalled();
    const names = invoke.mock.calls.map((c) => c[0] as string);
    expect(names.filter((n) => /remove|delete|purge|unlink/i.test(n))).toEqual([]);
    expect(new Set(names)).toEqual(new Set(["list_recordings", "read_recording_window", "recording_series", "read_body_window"]));
    vi.doUnmock("@tauri-apps/api/core");
  });
});

describe("robustness", () => {
  it("an empty reader list never hides saved sessions, and a malformed reply is a failure", async () => {
    const sess = [{ id: "x", device_type: "muse", start: "2026-11-07T11:57:00+09:00", end: "", duration_secs: 5, data_points: 0, summary: {} }];
    const { cache } = setup({ list: vi.fn(async () => []), eeg: vi.fn(async () => [] as unknown as RecordingWindowIpc) });
    const a = () => liveRecordings(sess, NOW, cache);
    a().list(); a().window("x", 0, 5); await settle();
    const l = a().list(); if (l.state !== "live") throw new Error("live");
    expect(l.value.map((r) => r.id)).toEqual(["x"]);
    expect(a().window("x", 0, 5).state).toBe("unwired");
  });
});
