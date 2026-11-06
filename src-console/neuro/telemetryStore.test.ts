import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
import { createMuseTelemetryStore, UI_INTERVAL_MS } from "./telemetryStore";

describe("muse telemetry store", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("publishes nothing while idle (no packets)", async () => {
    const store = createMuseTelemetryStore();
    const listener = vi.fn();
    store.subscribe(listener);
    const stop = store.start();
    const before = store.getSnapshot();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(listener).not.toHaveBeenCalled();
    expect(store.getSnapshot()).toBe(before);
    stop();
  });

  it("publishes once per interval when packets arrive, and stops when they stop", async () => {
    const store = createMuseTelemetryStore();
    const listener = vi.fn();
    store.subscribe(listener);
    const stop = store.start();
    store.ingest({ channel: "AF7", samples: [1, 2, 3] } as never);
    store.ingest({ channel: "TP9", samples: [4] } as never);
    await vi.advanceTimersByTimeAsync(UI_INTERVAL_MS);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getSnapshot().packetCount).toBe(2);
    expect(store.getSnapshot().channels.AF7).toEqual([1, 2, 3]);
    await vi.advanceTimersByTimeAsync(UI_INTERVAL_MS * 10);
    expect(listener).toHaveBeenCalledTimes(1); // unchanged -> no setState
    stop();
  });

  it("start is ref-counted and stop clears the interval", async () => {
    const store = createMuseTelemetryStore();
    const listener = vi.fn();
    store.subscribe(listener);
    const a = store.start();
    const b = store.start();
    a();
    store.ingest({ channel: "AF8", samples: [1] } as never);
    await vi.advanceTimersByTimeAsync(UI_INTERVAL_MS);
    expect(listener).toHaveBeenCalledTimes(1);
    b();
    store.ingest({ channel: "AF8", samples: [1] } as never);
    await vi.advanceTimersByTimeAsync(UI_INTERVAL_MS * 4);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("keeps gyroscope and infrared PPG, skips the other sensors, and clears on reset", async () => {
    const store = createMuseTelemetryStore();
    const stop = store.start();
    store.ingestMotion({ timestamp: 0, sensor: "accelerometer", unit: "g", sequence: 1, samples: [[0, 0, 1]] });
    store.ingestMotion({ timestamp: 0, sensor: "gyroscope", unit: "deg/s", sequence: 1, samples: [[1, 2, 3], [4, 5, 6], [7, 8, 9]] });
    store.ingestPpg({ timestamp: 0, channel: "ambient", sequence: 1, samples: [1, 1, 1, 1, 1, 1] });
    store.ingestPpg({ timestamp: 0, channel: "infrared", sequence: 1, samples: [9, 9, 9, 9, 9, 9] });
    store.ingest({ timestamp: 0, channel: "AF7", samples: [1] });
    await vi.advanceTimersByTimeAsync(UI_INTERVAL_MS);
    expect(store.getSnapshot().gyro?.samples).toHaveLength(3);
    expect(store.getSnapshot().ppg?.samples).toEqual([9, 9, 9, 9, 9, 9]);
    store.reset();
    expect(store.getSnapshot().gyro).toBeNull();
    expect(store.getSnapshot().ppg).toBeNull();
    stop();
  });
});
