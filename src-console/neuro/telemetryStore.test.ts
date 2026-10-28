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
});
