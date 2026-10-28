import { describe, expect, it } from "vitest";
import { createBandHistory } from "./bandHistory";
import type { MuseTelemetry } from "./telemetryStore";

function fakeSource() {
  let snapshot: MuseTelemetry = { channels: {}, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} };
  const listeners = new Set<() => void>();
  return {
    subscribe(l: () => void) { listeners.add(l); return () => { listeners.delete(l); }; },
    getSnapshot: () => snapshot,
    publish(patch: Partial<MuseTelemetry>) { snapshot = { ...snapshot, ...patch }; for (const l of listeners) l(); },
    listeners,
  };
}
const bands = (alpha: number) => ({ delta: 10, theta: 20, alpha, beta: 30, gamma: 5 });

describe("band history (60 s ring buffer)", () => {
  it("records a point per new packet once a full window exists, and drops points older than 60 s", () => {
    const source = fakeSource();
    let clock = 0;
    const history = createBandHistory(source, 60_000, () => clock);
    const off = history.subscribe(() => {});
    source.publish({ packetCount: 1, sampleCount: 100, lastPacketAt: 0, bands: bands(1) });
    expect(history.getSnapshot().alpha).toEqual([]); // < 256 samples
    source.publish({ packetCount: 2, sampleCount: 300, lastPacketAt: 1_000, bands: bands(2) });
    source.publish({ packetCount: 2, sampleCount: 300, lastPacketAt: 1_000, bands: bands(9) }); // no new packet: ignored
    clock = 30_000;
    source.publish({ packetCount: 3, sampleCount: 400, lastPacketAt: 30_000, bands: bands(3) });
    expect(history.getSnapshot().alpha).toEqual([2, 3]);
    clock = 61_500;
    source.publish({ packetCount: 4, sampleCount: 500, lastPacketAt: 61_500, bands: bands(4) });
    expect(history.getSnapshot().alpha).toEqual([3, 4]);
    expect(history.getSnapshot().delta).toEqual([10, 10]);
    off();
    expect(source.listeners.size).toBe(0);
  });

  it("clears when telemetry resets", () => {
    const source = fakeSource();
    const history = createBandHistory(source, 60_000, () => 1_000);
    history.subscribe(() => {});
    source.publish({ packetCount: 5, sampleCount: 300, lastPacketAt: 1_000, bands: bands(2) });
    source.publish({ packetCount: 0, sampleCount: 0, lastPacketAt: null });
    expect(history.getSnapshot().alpha).toEqual([]);
  });

  it("drops points older than the window from now() and empties after a stall", () => {
    const source = fakeSource();
    let clock = 0;
    const history = createBandHistory(source, 60_000, () => clock);
    const listener = () => {};
    history.subscribe(listener);
    source.publish({ packetCount: 1, sampleCount: 300, lastPacketAt: 0, bands: bands(1) });
    clock = 40_000;
    source.publish({ packetCount: 2, sampleCount: 300, lastPacketAt: 40_000, bands: bands(2) });
    expect(history.getSnapshot().alpha).toEqual([1, 2]);
    clock = 70_000; // packet 1 is now 70 s old; the last packet (40 s) is still inside the window
    const pruned = history.getSnapshot();
    expect(pruned.alpha).toEqual([2]);
    expect(history.getSnapshot()).toBe(pruned); // stable reference until something changes
    clock = 100_001; // last packet > 60 s ago: stalled, no new publish needed
    expect(history.getSnapshot().alpha).toEqual([]);
    expect(history.getSnapshot().theta).toEqual([]);
    clock = 101_000; // packets resume
    source.publish({ packetCount: 3, sampleCount: 300, lastPacketAt: 101_000, bands: bands(3) });
    expect(history.getSnapshot().alpha).toEqual([3]);
  });
});
